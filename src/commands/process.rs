use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::audit::{invariant_suite, precondition_pass};
use crate::backend::make_backend;
use crate::commands::reconcile;
use crate::config::{AiBackend, VaultConfig, find_vault_root};
use crate::error::CsnotesError;
use crate::flags::FlagStore;
use crate::manifest::{InProgressRecord, Manifest, SessionStatus};
use crate::ops::content::{execute_create_note, execute_update_note};
use crate::report::{Op, SessionReport, REPORT_FILENAME};
use crate::workspace::{
    assemble, cleanup, merge_back, new_run_id, take_snapshot, WorkspaceParams,
    WorkspaceScope,
};

/// `csnotes process` arguments (set by main.rs from clap).
pub struct ProcessArgs {
    pub session: Option<String>,
    pub course: Option<String>,
    pub source: Option<String>,
    pub topic: Option<String>,
    pub dry_run: bool,
    pub backend: Option<AiBackend>,
    pub fixture: Option<String>,
    /// Per-run Gemini model override (`--agy-model`).  Falls back to
    /// `config.agy_model`, then `agy`'s built-in default.
    pub agy_model: Option<String>,
}

pub fn run(args: ProcessArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;

    // Auto-reconcile: pick up any raw notes, Plaud exports, artifacts, or
    // sources added since the last run.  Quiet when nothing is new so the
    // normal process flow isn't cluttered; still prints "+ session …" lines
    // if new files are discovered.
    reconcile::run_for_vault(&vault_root, &config, reconcile::ReconcileArgs {
        notify: false,
        rename_spaces: None,
        quiet: true,
    })?;

    let mut manifest = Manifest::load(&vault_root)?;

    // ── Guard: existing in-progress session ───────────────────────────────
    if manifest.session_in_progress.is_some() {
        bail!(
            "A session is already in progress. Run `csnotes recover` to resume or discard it."
        );
    }

    // ── Resolve scope ─────────────────────────────────────────────────────
    let scope = resolve_scope(&args, &manifest)?;

    // ── Auto-reconcile ────────────────────────────────────────────────────
    // (Phase 2: reconcile runs here automatically)

    // ── No-Plaud prompt ───────────────────────────────────────────────────
    if let WorkspaceScope::Session { session_id } = &scope {
        let needs_prompt = manifest
            .sessions
            .get(session_id)
            .map_or(false, |e| e.plaud_exports.is_empty() && !e.plaud_missing);
        if needs_prompt {
            let choice = prompt_no_plaud(session_id)?;
            match choice {
                PlaudChoice::Continue => {
                    manifest
                        .sessions
                        .get_mut(session_id)
                        .unwrap()
                        .plaud_missing = true;
                }
                PlaudChoice::Pause => {
                    println!("Paused. Add Plaud exports and run `csnotes process` again.");
                    return Ok(());
                }
                PlaudChoice::Quit => return Ok(()),
            }
        }
    }

    // ── Assemble workspace ────────────────────────────────────────────────
    let run_id = new_run_id();
    let backend_kind = args.backend.unwrap_or(config.default_backend);
    let skill_variant = match backend_kind {
        crate::config::AiBackend::Claude => crate::config::SkillVariant::Claude,
        crate::config::AiBackend::Agy   => crate::config::SkillVariant::Gemini,
        crate::config::AiBackend::Mock  => config.skill_variant, // tests may want either
    };

    let ws_params = WorkspaceParams {
        vault_root: &vault_root,
        config: &config,
        manifest: &manifest,
        run_id: &run_id,
        scope,
        dry_run: args.dry_run,
    };

    let workspace_root = assemble(&ws_params)?;

    if args.dry_run {
        // Print scope summary
        match &ws_params.scope {
            WorkspaceScope::Session { session_id } => {
                println!("dry-run  scope   : session {}", session_id);
                if let Some(entry) = manifest.sessions.get(session_id) {
                    println!("         raw note: {}", entry.raw_note);
                    println!("         plaud   : {} export{}", entry.plaud_exports.len(),
                        if entry.plaud_exports.len() == 1 { "" } else { "s" });
                    if !entry.artifacts.is_empty() {
                        println!("         artifacts: {}", entry.artifacts.len());
                    }
                }
            }
            WorkspaceScope::Source { source_id } => {
                println!("dry-run  scope   : source {}", source_id);
                if let Some(entry) = manifest.sources.get(source_id) {
                    println!("         path    : {}", entry.path);
                    println!("         kind    : {:?}", entry.kind);
                }
            }
            WorkspaceScope::Topic { topic } => {
                println!("dry-run  scope   : topic {}", topic);
                if let Some(entry) = manifest.topics.get(topic) {
                    println!("         atomics : {}", entry.atomic_notes.len());
                }
            }
        }
        println!("dry-run  backend : {}", backend_kind);
        println!("dry-run  workspace: {}", workspace_root.display());
        return Ok(());
    }

    // ── Record in-progress ────────────────────────────────────────────────
    manifest.session_in_progress = Some(InProgressRecord {
        run_id: run_id.clone(),
        started_at: Utc::now(),
        workspace_path: workspace_root.clone(),
        phase: "synthesizing".to_string(),
        error: None,
        backend: Some(backend_kind),
        skill_variant: Some(skill_variant),
    });
    manifest.save(&vault_root)?;

    // ── Launch AI ─────────────────────────────────────────────────────────
    // Per-run override wins; fall back to config value, then agy's built-in default.
    let agy_model = args.agy_model.or(config.agy_model.clone());
    let backend = make_backend(backend_kind, skill_variant, args.fixture, agy_model);
    let launch_result = backend.launch(&workspace_root);

    if let Err(e) = launch_result {
        eprintln!("Backend exited with error: {}", e);
        eprintln!("Workspace preserved at: {}", workspace_root.display());
        eprintln!("Run `csnotes recover` to resume or discard.");
        // Record the error in the manifest
        if let Some(ref mut rec) = manifest.session_in_progress {
            rec.error = Some(e.to_string());
        }
        manifest.save(&vault_root)?;
        return Ok(());
    }

    // ── Teardown ──────────────────────────────────────────────────────────
    run_teardown(&vault_root, &workspace_root, &run_id, &config, &mut manifest)
}

/// The §7 teardown pipeline.  Separated so `recover` can call it too.
pub fn run_teardown(
    vault_root: &PathBuf,
    workspace_root: &PathBuf,
    run_id: &str,
    config: &VaultConfig,
    manifest: &mut Manifest,
) -> Result<()> {
    let now = Utc::now();

    // Step 2: Locate + parse report
    let report_path = workspace_root.join(REPORT_FILENAME);
    if !report_path.exists() {
        eprintln!("No session report found at {}.", report_path.display());
        eprintln!("Re-enter the session and have the AI write the report:");
        eprintln!("  csnotes recover --resume");
        return Ok(());
    }

    let report = match SessionReport::load(workspace_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Session report parse error: {}", e);
            eprintln!("Workspace preserved. Re-enter and fix the report:");
            eprintln!("  csnotes recover --resume");
            return Ok(());
        }
    };

    // Step 3: run_id match
    if report.run_id != run_id {
        bail!(CsnotesError::RunIdMismatch {
            manifest: run_id.to_string(),
            report: report.run_id.clone(),
        });
    }

    // Step 4: Precondition pass
    if let Err(e) = precondition_pass(&report, workspace_root) {
        eprintln!("Precondition failure — discarding workspace.");
        eprintln!("  {}", e);
        cleanup(workspace_root, vault_root, run_id)?;
        manifest.session_in_progress = None;
        manifest.save(vault_root)?;
        return Err(e);
    }

    // Step 5: Structural ops.  All ops run against the workspace copy before
    // any content ops so the directory layout is stable for step 6.
    for op in &report.operations {
        let result = match op {
            Op::RenameTopic(o) => crate::ops::structural::execute_rename_topic(
                o, workspace_root, &config.synthetic_dir,
            ),
            Op::MoveAtomic(o) => crate::ops::structural::execute_move_atomic(
                o, workspace_root, &config.synthetic_dir,
            ),
            Op::PromoteAtomic(o) => crate::ops::structural::execute_promote_atomic(
                o, workspace_root, &config.synthetic_dir,
            ),
            Op::DemoteTopic(o) => crate::ops::structural::execute_demote_topic(
                o, workspace_root, &config.synthetic_dir,
            ),
            Op::MergeTopics(o) => crate::ops::structural::execute_merge_topics(
                o, workspace_root, &config.synthetic_dir,
            ),
            Op::SplitTopic(o) => crate::ops::structural::execute_split_topic(
                o, workspace_root, &config.synthetic_dir,
            ),
            Op::SetEmbed(o) => crate::ops::structural::execute_set_embed(o, workspace_root),
            _ => continue, // content ops handled in step 6
        };
        if let Err(e) = result {
            eprintln!("{} failed: {}", op.kind_str(), e);
            cleanup(workspace_root, vault_root, run_id)?;
            manifest.session_in_progress = None;
            manifest.save(vault_root)?;
            return Err(e);
        }
    }

    // Step 6: Execute content ops
    for op in &report.operations {
        match op {
            Op::CreateNote(op) => execute_create_note(op, workspace_root, now)?,
            Op::UpdateNote(op) => execute_update_note(op, workspace_root, now)?,
            _ => {} // structural handled above
        }
    }

    // Step 7: Build updated manifest
    let updated_manifest = crate::audit::reindex(workspace_root, config)?;
    // Preserve sessions and sources from the existing manifest (reindex only
    // touches topics)
    let mut new_manifest = manifest.clone();
    new_manifest.topics = updated_manifest.topics;

    // Mark session(s) as processed and sources as processed
    update_session_status(&report, &mut new_manifest, now);
    update_source_status(&report, &mut new_manifest, now);

    // Step 8: Invariant suite
    let audit = invariant_suite(
        workspace_root,
        &config.synthetic_dir,
        &report,
        &new_manifest,
    )?;

    if !audit.is_clean() {
        eprintln!("Invariant violations — discarding workspace.");
        audit.print();
        cleanup(workspace_root, vault_root, run_id)?;
        manifest.session_in_progress = None;
        manifest.save(vault_root)?;
        bail!("invariant suite failed");
    }

    audit.print(); // Print any soft warnings

    // Step 9: Pre-merge snapshot
    manifest.session_in_progress.as_mut().map(|r| r.phase = "merging".to_string());
    manifest.save(vault_root)?;
    let _snapshot = take_snapshot(vault_root, &config.synthetic_dir, run_id)?;

    // Step 10: Merge-back
    merge_back(workspace_root, vault_root, &config.synthetic_dir)?;

    // Copy last_report.json and per-session report copies
    let report_src = workspace_root.join(REPORT_FILENAME);
    if report_src.exists() {
        std::fs::copy(&report_src, new_manifest.last_report_path()).ok();

        for session_id in &report.scope.sessions {
            let dest = new_manifest.session_report_path(session_id);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::copy(&report_src, &dest).ok();
        }
    }

    // Append flags to flag store
    let flags_path = vault_root
        .join(&config.generated_dir)
        .join("flags.json");
    let mut flag_store = FlagStore::load(&flags_path).unwrap_or_default();
    flag_store.append_from_report(&report.review_flags, run_id, now);
    flag_store.save(&flags_path)?;

    // Write final manifest, clear in-progress
    new_manifest.session_in_progress = None;
    new_manifest.save(vault_root)?;

    // Cleanup
    cleanup(workspace_root, vault_root, run_id)?;

    println!("Session committed.");
    let n_ops = report.operations.len();
    let n_flags = report.review_flags.iter().filter(|f| f.kind.is_actionable()).count();
    println!("  {} operation{}", n_ops, if n_ops == 1 { "" } else { "s" });
    if n_flags > 0 {
        println!("  {} actionable flag{} — run `csnotes flags list`",
            n_flags, if n_flags == 1 { "" } else { "s" });
    }

    Ok(())
}

// ── Scope resolution ──────────────────────────────────────────────────────────

fn resolve_scope(args: &ProcessArgs, manifest: &Manifest) -> Result<WorkspaceScope> {
    if let Some(source_id) = &args.source {
        return Ok(WorkspaceScope::Source { source_id: source_id.clone() });
    }
    if let Some(topic) = &args.topic {
        return Ok(WorkspaceScope::Topic { topic: topic.clone() });
    }

    let session_id = resolve_session_id(args, manifest)?;
    Ok(WorkspaceScope::Session { session_id })
}

fn resolve_session_id(args: &ProcessArgs, manifest: &Manifest) -> Result<String> {
    let unprocessed: Vec<String> = manifest
        .sessions
        .iter()
        .filter(|(_, e)| e.status == SessionStatus::Unprocessed)
        .map(|(id, _)| id.clone())
        .collect();

    if let Some(session) = &args.session {
        if let Some(course) = &args.course {
            let id = format!("{}-{}", course, session);
            if manifest.sessions.contains_key(&id) {
                return Ok(id);
            }
            bail!("Session '{}' not found in manifest", id);
        }
        // Date only: must be unique across courses
        let matches: Vec<String> = manifest
            .sessions
            .iter()
            .filter(|(_, e)| e.date.to_string() == *session)
            .map(|(id, _)| id.clone())
            .collect();
        match matches.len() {
            0 => bail!("No session found for date '{}'", session),
            1 => return Ok(matches.into_iter().next().unwrap()),
            _ => bail!(
                "Date '{}' matches multiple sessions: {}. Use --course to disambiguate.",
                session,
                matches.join(", ")
            ),
        }
    }

    // No explicit session: auto-resolve
    match unprocessed.len() {
        0 => bail!("No unprocessed sessions. Run `csnotes status` to review."),
        1 => Ok(unprocessed.into_iter().next().unwrap()),
        _ => {
            eprintln!("Multiple unprocessed sessions:");
            for id in &unprocessed {
                eprintln!("  {}", id);
            }
            bail!("Use --session or --course to specify which session to process.");
        }
    }
}

fn update_session_status(
    report: &SessionReport,
    manifest: &mut Manifest,
    now: chrono::DateTime<Utc>,
) {
    let topics = topics_touched_by_report(report);
    for session_id in &report.scope.sessions {
        if let Some(entry) = manifest.sessions.get_mut(session_id) {
            entry.status = SessionStatus::Processed;
            entry.processed_at = Some(now);
            entry.topics_updated = topics.clone();
        }
    }
}

/// Collect the distinct topic names touched by the content ops in a report.
fn topics_touched_by_report(report: &SessionReport) -> Vec<String> {
    use crate::report::Op;
    use std::collections::BTreeSet;

    let mut seen = BTreeSet::new();
    for op in &report.operations {
        let topic = match op {
            Op::CreateNote(o) => Some(o.topic.clone()),
            Op::UpdateNote(o) => topic_from_path(&o.path),
            Op::RenameTopic(o) => {
                seen.insert(o.from.clone());
                Some(o.to.clone())
            }
            _ => None,
        };
        if let Some(t) = topic {
            seen.insert(t);
        }
    }
    seen.into_iter().collect()
}

fn update_source_status(
    report: &SessionReport,
    manifest: &mut Manifest,
    now: chrono::DateTime<Utc>,
) {
    let topics = topics_touched_by_report(report);
    for source_id in &report.scope.sources {
        if let Some(entry) = manifest.sources.get_mut(source_id) {
            entry.status = crate::manifest::SourceStatus::Processed;
            entry.last_processed_at = Some(now);
            entry.topics_updated = topics.clone();
        }
    }
}

/// Extract a topic name from a workspace-relative path like
/// `_synthetic/{topic}/something.md`.
fn topic_from_path(path: &str) -> Option<String> {
    // Strip a leading synthetic dir prefix (e.g. "_synthetic/") if present,
    // then take the first path component as the topic name.
    let without_prefix = path
        .trim_start_matches('/')
        .split_once('/')
        .map(|(first, rest)| {
            // If first component looks like a synthetic dir (starts with "_"),
            // return the next component; otherwise use first.
            if first.starts_with('_') {
                rest.split('/').next().unwrap_or("").to_string()
            } else {
                first.to_string()
            }
        })?;
    if without_prefix.is_empty() {
        None
    } else {
        Some(without_prefix)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::{NoteKind, ProvenanceDelta};
    use crate::manifest::SourceStatus;
    use crate::report::{CreateNoteOp, Op, RenameTopicOp, ReportScope, ScopeKind, SessionReport, UpdateNoteOp};
    use chrono::Utc;

    fn make_report(ops: Vec<Op>) -> SessionReport {
        SessionReport {
            csnotes_report_schema: 1,
            run_id: "test-run".to_string(),
            backend: "mock".to_string(),
            started_at: Utc::now(),
            completed_at: Utc::now(),
            scope: ReportScope {
                kind: ScopeKind::Session,
                sessions: vec!["CS101-01-01".to_string()],
                sources: vec![],
                topic: None,
            },
            operations: ops,
            review_flags: vec![],
        }
    }

    #[test]
    fn topic_from_path_strips_synthetic_prefix() {
        assert_eq!(
            topic_from_path("_synthetic/inheritance/poly.md"),
            Some("inheritance".to_string())
        );
    }

    #[test]
    fn topic_from_path_handles_no_prefix() {
        assert_eq!(
            topic_from_path("inheritance/poly.md"),
            Some("inheritance".to_string())
        );
    }

    #[test]
    fn topic_from_path_returns_none_for_shallow_path() {
        assert_eq!(topic_from_path("_synthetic"), None);
        assert_eq!(topic_from_path(""), None);
    }

    #[test]
    fn topics_touched_deduplicates_and_sorts() {
        let ops = vec![
            Op::CreateNote(CreateNoteOp {
                kind: NoteKind::Atomic,
                path: "_synthetic/inheritance/poly.md".to_string(),
                title: "Polymorphism".to_string(),
                topic: "inheritance".to_string(),
                block_id: Some("poly".to_string()),
                embed_in: vec![],
                provenance: ProvenanceDelta::default(),
                change_summary: "new".to_string(),
            }),
            Op::UpdateNote(UpdateNoteOp {
                path: "_synthetic/types/types.md".to_string(),
                add_provenance: ProvenanceDelta::default(),
                sections: vec![],
                change_summary: "updated".to_string(),
            }),
            // Second create in the same topic — should deduplicate
            Op::CreateNote(CreateNoteOp {
                kind: NoteKind::Index,
                path: "_synthetic/inheritance/index.md".to_string(),
                title: "Inheritance".to_string(),
                topic: "inheritance".to_string(),
                block_id: None,
                embed_in: vec![],
                provenance: ProvenanceDelta::default(),
                change_summary: "new".to_string(),
            }),
        ];
        let topics = topics_touched_by_report(&make_report(ops));
        assert_eq!(topics, vec!["inheritance", "types"]);
    }

    #[test]
    fn topics_touched_includes_rename_from_and_to() {
        let ops = vec![Op::RenameTopic(RenameTopicOp {
            from: "old-name".to_string(),
            to: "new-name".to_string(),
            reason: "clearer".to_string(),
        })];
        let topics = topics_touched_by_report(&make_report(ops));
        assert!(topics.contains(&"old-name".to_string()));
        assert!(topics.contains(&"new-name".to_string()));
    }

    #[test]
    fn update_source_status_marks_processed() {
        use crate::manifest::{ManifestConfig, SourceEntry, SourceKind};
        use crate::config::{AiBackend, SkillVariant, SnapshotMode};

        let cfg = ManifestConfig {
            raw_dir: "notes".into(),
            plaud_dir: "plaud".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            default_backend: AiBackend::Mock,
            skill_variant: SkillVariant::Claude,
            snapshot_mode: SnapshotMode::PreMerge,
        };
        let mut manifest = Manifest::empty(std::path::PathBuf::from("/tmp"), cfg);
        manifest.sources.insert(
            "SICP/ch01".to_string(),
            SourceEntry {
                path: "sources/SICP/ch01.md".to_string(),
                kind: SourceKind::Textbook,
                status: SourceStatus::Unprocessed,
                last_processed_at: None,
                heading_scheme: vec![],
                topics_updated: vec![],
            },
        );

        let mut report = make_report(vec![
            Op::UpdateNote(UpdateNoteOp {
                path: "_synthetic/algorithms/search.md".to_string(),
                add_provenance: ProvenanceDelta::default(),
                sections: vec![],
                change_summary: "added".to_string(),
            }),
        ]);
        report.scope.sources = vec!["SICP/ch01".to_string()];

        update_source_status(&report, &mut manifest, Utc::now());

        let entry = manifest.sources.get("SICP/ch01").unwrap();
        assert_eq!(entry.status, SourceStatus::Processed);
        assert!(entry.last_processed_at.is_some());
        assert_eq!(entry.topics_updated, vec!["algorithms"]);
    }
}

// ── No-Plaud prompt ───────────────────────────────────────────────────────────

enum PlaudChoice {
    Continue,
    Pause,
    Quit,
}

fn prompt_no_plaud(session_id: &str) -> Result<PlaudChoice> {
    use std::io::{self, BufRead, Write};
    print!(
        "No Plaud exports found for {}. [c]ontinue / [p]ause / [q]uit: ",
        session_id
    );
    io::stdout().flush()?;
    let stdin = io::stdin();
    let line = stdin.lock().lines().next().unwrap_or(Ok(String::new()))?;
    Ok(match line.trim() {
        "p" => PlaudChoice::Pause,
        "q" => PlaudChoice::Quit,
        _ => PlaudChoice::Continue,
    })
}
