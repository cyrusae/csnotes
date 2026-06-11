use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Utc;
use owo_colors::OwoColorize;

use crate::audit::{invariant_suite, precondition_pass};
use crate::backend::make_backend;
use crate::commands::reconcile;
use crate::config::{AiBackend, VaultConfig, find_vault_root};
use crate::error::CsnotesError;
use crate::flags::FlagStore;
use crate::manifest::{InProgressRecord, Manifest, ManifestLock, SessionStatus};
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
    let _lock = ManifestLock::acquire(&vault_root)?;

    // Auto-reconcile: pick up any raw notes, recording exports, artifacts, or
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

    // ── No-recording prompt ──────────────────────────────────────────────
    if let WorkspaceScope::Session { session_id } = &scope {
        if let Some(entry) = manifest.sessions.get(session_id) {
            let needs_prompt = config.recordings_required_for(&entry.course)
                && entry.recording_exports.is_empty()
                && !entry.recording_missing;
            if needs_prompt {
                let choice = prompt_no_recording(session_id)?;
                match choice {
                    RecordingChoice::Continue => {
                        manifest
                            .sessions
                            .get_mut(session_id)
                            .unwrap()
                            .recording_missing = true;
                    }
                    RecordingChoice::Pause => {
                        println!("Paused. Add recording exports and run `csnotes process` again.");
                        return Ok(());
                    }
                    RecordingChoice::Quit => return Ok(()),
                }
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
        let tag = "dry-run".yellow().bold().to_string();
        let dim_colon = |label: &str, value: &str| {
            println!("{}  {} {}", tag, label.dimmed(), value);
        };
        let continuation = |label: &str, value: &str| {
            println!("         {} {}", label.dimmed(), value);
        };
        match &ws_params.scope {
            WorkspaceScope::Session { session_id } => {
                dim_colon("scope   :", &format!("session {}", session_id));
                if let Some(entry) = manifest.sessions.get(session_id) {
                    continuation("raw note:", &entry.raw_note);
                    continuation("recordings:", &format!("{} export{}", entry.recording_exports.len(),
                        if entry.recording_exports.len() == 1 { "" } else { "s" }));
                    if !entry.artifacts.is_empty() {
                        continuation("artifacts:", &entry.artifacts.len().to_string());
                    }
                }
            }
            WorkspaceScope::Source { source_id } => {
                dim_colon("scope   :", &format!("source {}", source_id));
                if let Some(entry) = manifest.sources.get(source_id) {
                    continuation("path    :", &entry.path);
                    continuation("kind    :", &format!("{:?}", entry.kind));
                }
            }
            WorkspaceScope::Topic { topic } => {
                dim_colon("scope   :", &format!("topic {}", topic));
                if let Some(entry) = manifest.topics.get(topic) {
                    continuation("atomics :", &entry.atomic_notes.len().to_string());
                }
            }
        }
        dim_colon("backend :", &backend_kind.to_string());
        dim_colon("workspace:", &workspace_root.display().to_string());
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
    let backend = make_backend(backend_kind, skill_variant, args.fixture, agy_model, false);
    let launch_result = backend.launch(&workspace_root);

    if let Err(e) = launch_result {
        eprintln!("{} {}", "Backend exited with error:".red().bold(), e);
        eprintln!("Workspace preserved at: {}", workspace_root.display());
        eprintln!("Run {} to resume or discard.", "`csnotes recover`".bold());
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
        eprintln!("{}", "No session report found.".red().bold());
        eprintln!("Re-enter the session and have the AI write the report:");
        eprintln!("  {}", "csnotes recover --resume".bold());
        return Ok(());
    }

    let report = match SessionReport::load(workspace_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} {}", "Session report parse error:".red().bold(), e);
            eprintln!("Workspace preserved. Re-enter and fix the report:");
            eprintln!("  {}", "csnotes recover --resume".bold());
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
        eprintln!("{}", "Precondition failure — discarding workspace.".red().bold());
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
    update_session_status(&report, &mut new_manifest, now, &config.synthetic_dir);
    update_source_status(&report, &mut new_manifest, now, &config.synthetic_dir);

    // Step 8: Invariant suite
    let audit = invariant_suite(
        workspace_root,
        &config.synthetic_dir,
        &report,
        &new_manifest,
    )?;

    if !audit.is_clean() {
        eprintln!("{}", "Invariant violations — discarding workspace.".red().bold());
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

    println!("{}", "Session committed.".green().bold());
    let n_ops = report.operations.len();
    let n_flags = report.review_flags.iter().filter(|f| f.kind.is_actionable()).count();
    println!("  {} operation{}", n_ops, if n_ops == 1 { "" } else { "s" });
    if n_flags > 0 {
        println!(
            "  {}",
            format!(
                "{} actionable flag{} — run `csnotes flags list`",
                n_flags,
                if n_flags == 1 { "" } else { "s" }
            )
            .yellow()
        );
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
        // Date only: must be unique across courses.
        // Accepts full YYYY-MM-DD or a suffix like MM-DD / DD that uniquely
        // identifies one session; ambiguity is handled by the match below.
        let matches: Vec<String> = manifest
            .sessions
            .iter()
            .filter(|(_, e)| {
                let full = e.date.to_string();
                full == *session || full.ends_with(&format!("-{}", session))
            })
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
    synthetic_dir: &str,
) {
    let topics = topics_touched_by_report(report, synthetic_dir);
    for session_id in &report.scope.sessions {
        if let Some(entry) = manifest.sessions.get_mut(session_id) {
            entry.status = SessionStatus::Processed;
            entry.processed_at = Some(now);
            entry.topics_updated = topics.clone();
        }
    }
}

/// Collect the distinct topic names touched by the content ops in a report.
fn topics_touched_by_report(report: &SessionReport, synthetic_dir: &str) -> Vec<String> {
    use crate::report::Op;
    use std::collections::BTreeSet;

    let mut seen = BTreeSet::new();
    for op in &report.operations {
        let topic = match op {
            Op::CreateNote(o) => Some(o.topic.clone()),
            Op::UpdateNote(o) => topic_from_path(&o.path, synthetic_dir),
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
    synthetic_dir: &str,
) {
    let topics = topics_touched_by_report(report, synthetic_dir);
    for source_id in &report.scope.sources {
        if let Some(entry) = manifest.sources.get_mut(source_id) {
            entry.status = crate::manifest::SourceStatus::Processed;
            entry.last_processed_at = Some(now);
            entry.topics_updated = topics.clone();
        }
    }
}

/// Extract a topic name from a workspace-relative path like
/// `{synthetic_dir}/{topic}/something.md`.
fn topic_from_path(path: &str, synthetic_dir: &str) -> Option<String> {
    let path = path.trim_start_matches('/');
    // Strip the configured synthetic dir prefix if present.
    let inner = path
        .strip_prefix(&format!("{}/", synthetic_dir))
        .unwrap_or(path);
    // Require at least two components (topic/filename) so bare dir names
    // like "_synthetic" don't produce a spurious topic.
    let mut parts = inner.splitn(3, '/');
    let topic = parts.next()?;
    if topic.is_empty() || parts.next().is_none() {
        return None;
    }
    Some(topic.to_string())
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
            topic_from_path("_synthetic/inheritance/poly.md", "_synthetic"),
            Some("inheritance".to_string())
        );
    }

    #[test]
    fn topic_from_path_handles_no_prefix() {
        assert_eq!(
            topic_from_path("inheritance/poly.md", "_synthetic"),
            Some("inheritance".to_string())
        );
    }

    #[test]
    fn topic_from_path_returns_none_for_shallow_path() {
        assert_eq!(topic_from_path("_synthetic", "_synthetic"), None);
        assert_eq!(topic_from_path("", "_synthetic"), None);
    }

    #[test]
    fn topic_from_path_respects_configured_synthetic_dir() {
        // Non-default synthetic dir should be stripped correctly.
        assert_eq!(
            topic_from_path("notes_synth/sorting/quicksort.md", "notes_synth"),
            Some("sorting".to_string())
        );
        // Default dir should NOT match when a different dir is configured.
        assert_eq!(
            topic_from_path("_synthetic/sorting/quicksort.md", "notes_synth"),
            Some("_synthetic".to_string())
        );
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
        let topics = topics_touched_by_report(&make_report(ops), "_synthetic");
        assert_eq!(topics, vec!["inheritance", "types"]);
    }

    #[test]
    fn topics_touched_includes_rename_from_and_to() {
        let ops = vec![Op::RenameTopic(RenameTopicOp {
            from: "old-name".to_string(),
            to: "new-name".to_string(),
            reason: "clearer".to_string(),
        })];
        let topics = topics_touched_by_report(&make_report(ops), "_synthetic");
        assert!(topics.contains(&"old-name".to_string()));
        assert!(topics.contains(&"new-name".to_string()));
    }

    fn make_manifest_with_sessions(sessions: &[(&str, &str, &str)]) -> Manifest {
        use crate::config::{AiBackend, SkillVariant, SnapshotMode};
        use crate::manifest::{ManifestConfig, SessionEntry};
        use chrono::NaiveDate;

        let cfg = ManifestConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
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
        for (id, course, date_str) in sessions {
            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").unwrap();
            manifest.sessions.insert(
                id.to_string(),
                SessionEntry {
                    date,
                    course: course.to_string(),
                    filename_format: "{course}-{mm}-{dd}".into(),
                    raw_note: format!("notes/{}.md", id),
                    recording_exports: vec![],
                    artifacts: vec![],
                    recording_missing: false,
                    status: SessionStatus::Unprocessed,
                    processed_at: None,
                    topics_updated: vec![],
                },
            );
        }
        manifest
    }

    fn resolve(session: Option<&str>, course: Option<&str>, manifest: &Manifest) -> Result<String> {
        resolve_session_id(
            &ProcessArgs {
                session: session.map(|s| s.to_string()),
                course: course.map(|s| s.to_string()),
                source: None,
                topic: None,
                dry_run: false,
                backend: None,
                fixture: None,
                agy_model: None,
            },
            manifest,
        )
    }

    #[test]
    fn resolve_session_full_date_matches() {
        let m = make_manifest_with_sessions(&[("CS101-2026-07-28", "CS101", "2026-07-28")]);
        assert_eq!(resolve(Some("2026-07-28"), None, &m).unwrap(), "CS101-2026-07-28");
    }

    #[test]
    fn resolve_session_partial_mm_dd_matches() {
        let m = make_manifest_with_sessions(&[("CS101-2026-07-28", "CS101", "2026-07-28")]);
        assert_eq!(resolve(Some("07-28"), None, &m).unwrap(), "CS101-2026-07-28");
    }

    #[test]
    fn resolve_session_partial_ambiguous_errors() {
        let m = make_manifest_with_sessions(&[
            ("CS101-2026-07-28", "CS101", "2026-07-28"),
            ("CS202-2026-07-28", "CS202", "2026-07-28"),
        ]);
        assert!(resolve(Some("07-28"), None, &m).is_err());
    }

    #[test]
    fn resolve_session_no_match_errors() {
        let m = make_manifest_with_sessions(&[("CS101-2026-07-28", "CS101", "2026-07-28")]);
        assert!(resolve(Some("06-15"), None, &m).is_err());
    }

    #[test]
    fn update_source_status_marks_processed() {
        use crate::manifest::{ManifestConfig, SourceEntry, SourceKind};
        use crate::config::{AiBackend, SkillVariant, SnapshotMode};

        let cfg = ManifestConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
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
                summary: None,
                tags: vec![],
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

        update_source_status(&report, &mut manifest, Utc::now(), "_synthetic");

        let entry = manifest.sources.get("SICP/ch01").unwrap();
        assert_eq!(entry.status, SourceStatus::Processed);
        assert!(entry.last_processed_at.is_some());
        assert_eq!(entry.topics_updated, vec!["algorithms"]);
    }
}

// ── No-recording prompt ───────────────────────────────────────────────────────

enum RecordingChoice {
    Continue,
    Pause,
    Quit,
}

fn prompt_no_recording(session_id: &str) -> Result<RecordingChoice> {
    use std::io::{self, BufRead, Write};
    print!(
        "No recording exports found for {}. [c]ontinue / [p]ause / [q]uit: ",
        session_id
    );
    io::stdout().flush()?;
    let stdin = io::stdin();
    let line = stdin.lock().lines().next().unwrap_or(Ok(String::new()))?;
    Ok(match line.trim() {
        "p" => RecordingChoice::Pause,
        "q" => RecordingChoice::Quit,
        _ => RecordingChoice::Continue,
    })
}
