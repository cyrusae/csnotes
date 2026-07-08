use std::path::Path;

use anyhow::{bail, Result};
use chrono::Utc;
use owo_colors::OwoColorize;

use crate::audit::{invariant_suite, precondition_pass_ops};
use crate::backend::make_backend;
use crate::commands::reconcile;
use crate::config::{find_vault_root, AiBackend, VaultConfig};
use crate::error::CsnotesError;
use crate::flags::FlagStore;
use crate::manifest::{InProgressRecord, Manifest, ManifestLock, SessionStatus};
use crate::ops::content::{execute_create_note, execute_update_note};
use crate::report::{Op, SessionReport, REPORT_FILENAME};
use crate::workspace::{
    assemble, cleanup, cleanup_workspace_snapshot, merge_back, new_run_id,
    restore_workspace_snapshot, take_snapshot, take_workspace_snapshot, WorkspaceMeta,
    WorkspaceParams, WorkspaceScope,
};

/// `csnotes process` arguments (set by main.rs from clap).
pub struct ProcessArgs {
    pub session: Option<String>,
    /// Disambiguator: `{course}-{date}` session ID prefix when date alone is ambiguous.
    pub for_course: Option<String>,
    /// Scope flag: assemble a course-wide workspace (all processed sessions + sources).
    pub course: Option<String>,
    /// Source IDs or path prefixes passed via `--source`.  Each entry is either
    /// an exact source ID or a prefix that expands to all matching IDs.
    pub sources: Vec<String>,
    pub topic: Option<String>,
    /// Resource ID or fuzzy name for a self-directed study resource.
    pub resource: Option<String>,
    /// Resource IDs to include alongside a course or other primary scope.
    pub include_resources: Vec<String>,
    pub dry_run: bool,
    pub backend: Option<AiBackend>,
    pub fixture: Option<String>,
    /// Per-run Gemini model override (`--agy-model`).  Falls back to
    /// `config.agy_model`, then `agy`'s built-in default.
    pub agy_model: Option<String>,
    /// Per-run Claude model override (`--claude-model`).
    pub claude_model: Option<String>,
    /// Auto-select the oldest pending session instead of erroring on ambiguity.
    pub next: bool,
}

pub fn run(args: ProcessArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let _lock = ManifestLock::acquire(&vault_root)?;

    // Auto-reconcile: pick up any raw notes, recording exports, artifacts, or
    // sources added since the last run.  Quiet when nothing is new so the
    // normal process flow isn't cluttered; still prints "+ session …" lines
    // if new files are discovered.
    reconcile::run_for_vault(
        &vault_root,
        &config,
        reconcile::ReconcileArgs {
            notify: false,
            rename_spaces: None,
            quiet: true,
            reset: false,
        },
    )?;

    let mut manifest = Manifest::load(&vault_root)?;

    // ── Guard: existing in-progress session ───────────────────────────────
    if manifest.session_in_progress.is_some() {
        bail!("A session is already in progress. Run `csnotes recover` to resume or discard it.");
    }

    // ── Resolve scope ─────────────────────────────────────────────────────
    let scope = resolve_scope(&args, &manifest)?;
    let include_resources = resolve_include_resources(&args.include_resources, &manifest)?;

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

    // ── Near-empty file check ─────────────────────────────────────────────
    if let WorkspaceScope::Session { session_id } = &scope {
        if let Some(entry) = manifest.sessions.get(session_id) {
            const WORD_THRESHOLD: usize = 100;
            let mut short_files: Vec<(String, usize)> = vec![];

            let note_path = vault_root.join(&entry.raw_note);
            if let Some(wc) = count_words_in_file(&note_path) {
                if wc < WORD_THRESHOLD {
                    short_files.push((entry.raw_note.clone(), wc));
                }
            }
            for rec in &entry.recording_exports {
                let rec_path = vault_root.join(&rec.path);
                if let Some(wc) = count_words_in_file(&rec_path) {
                    if wc < WORD_THRESHOLD {
                        short_files.push((rec.path.clone(), wc));
                    }
                }
            }

            if !short_files.is_empty() {
                match prompt_short_files(session_id, &short_files)? {
                    ShortFileChoice::Continue => {}
                    ShortFileChoice::Pause => {
                        println!("Paused. Fill in the placeholder files and run `csnotes process` again.");
                        return Ok(());
                    }
                    ShortFileChoice::Quit => return Ok(()),
                }
            }
        }
    }

    // ── Assemble workspace ────────────────────────────────────────────────
    let run_id = new_run_id();
    let backend_kind = args.backend.unwrap_or(config.default_backend);
    let skill_variant = match backend_kind {
        crate::config::AiBackend::Claude => crate::config::SkillVariant::Claude,
        crate::config::AiBackend::Agy => crate::config::SkillVariant::Gemini,
        crate::config::AiBackend::Mock => config.skill_variant, // tests may want either
    };
    // Course and Resource scopes use their respective review skills.
    let skill_variant = if args.course.is_some() {
        crate::config::SkillVariant::CourseReview
    } else if args.resource.is_some() {
        crate::config::SkillVariant::ResourceReview
    } else {
        skill_variant
    };

    let ws_params = WorkspaceParams {
        vault_root: &vault_root,
        config: &config,
        manifest: &manifest,
        run_id: &run_id,
        scope,
        skill_variant,
        backend_kind,
        dry_run: args.dry_run,
        include_resources,
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
                    continuation(
                        "recordings:",
                        &format!(
                            "{} export{}",
                            entry.recording_exports.len(),
                            if entry.recording_exports.len() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        ),
                    );
                    if !entry.artifacts.is_empty() {
                        continuation("artifacts:", &entry.artifacts.len().to_string());
                    }
                }
            }
            WorkspaceScope::Source { source_ids } => {
                dim_colon("scope   :", &format!("source(s) [{}]", source_ids.len()));
                for source_id in source_ids {
                    if let Some(entry) = manifest.sources.get(source_id) {
                        continuation(
                            "source  :",
                            &format!("{} ({})", source_id, entry.kind.as_str()),
                        );
                    } else {
                        continuation("source  :", source_id);
                    }
                }
            }
            WorkspaceScope::Topic { topic } => {
                dim_colon("scope   :", &format!("topic {}", topic));
                if let Some(entry) = manifest.topics.get(topic) {
                    continuation("atomics :", &entry.atomic_notes.len().to_string());
                }
            }
            WorkspaceScope::Course { course } => {
                use crate::manifest::SessionStatus;
                let session_count = manifest
                    .sessions
                    .values()
                    .filter(|e| &e.course == course && e.status == SessionStatus::Processed)
                    .count();
                dim_colon("scope   :", &format!("course {}", course));
                continuation("sessions:", &format!("{} processed", session_count));
            }
            WorkspaceScope::Resource { resource_id } => {
                dim_colon("scope   :", &format!("resource {}", resource_id));
                if let Some(entry) = manifest.resources.get(resource_id) {
                    continuation("kind    :", entry.kind.as_str());
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

    // Release the manifest lock before launching the AI session.  The AI may
    // call `csnotes commit` as a subprocess, which needs its own LOCK_EX
    // acquire.  Holding the lock across `backend.launch()` (a blocking wait)
    // would cause that subprocess to deadlock indefinitely.
    drop(_lock);

    // ── Launch AI ─────────────────────────────────────────────────────────
    // Per-run override wins; fall back to config value, then agy's built-in default.
    let agy_model = args.agy_model.or(config.agy_model.clone());
    let backend = make_backend(
        backend_kind,
        skill_variant,
        args.fixture,
        agy_model,
        args.claude_model,
        false,
    );
    let launch_result = backend.launch(&workspace_root);

    // Re-acquire the lock for teardown and error recording.
    let _lock = ManifestLock::acquire(&vault_root)?;
    let mut manifest = Manifest::load(&vault_root)?;

    if let Err(e) = launch_result {
        eprintln!("{} {}", "Backend exited with error:".red().bold(), e);
        eprintln!("Workspace preserved at: {}", workspace_root.display());
        eprintln!("Run {} to resume or discard.", "`csnotes recover`".bold());
        if let Some(ref mut rec) = manifest.session_in_progress {
            rec.error = Some(e.to_string());
        }
        manifest.save(&vault_root)?;
        return Ok(());
    }

    // ── Teardown ──────────────────────────────────────────────────────────
    run_teardown(
        &vault_root,
        &workspace_root,
        &run_id,
        &config,
        &mut manifest,
    )
}

/// The §7 teardown pipeline.  Separated so `recover` can call it too.
pub fn run_teardown(
    vault_root: &Path,
    workspace_root: &Path,
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

    // Check how many ops were already committed by `csnotes commit`.
    // When committed_through == total ops we can skip execution entirely.
    let committed_through = WorkspaceMeta::load(workspace_root)
        .map(|m| m.committed_ops.len())
        .unwrap_or(0);
    let all_committed = committed_through >= report.operations.len();
    let remaining = &report.operations[committed_through.min(report.operations.len())..];

    if !all_committed {
        // Step 4: Precondition pass (only for uncommitted ops — no mutations yet)
        if let Err(e) = precondition_pass_ops(remaining, workspace_root) {
            eprintln!(
                "{}",
                "Precondition failure — workspace preserved.".red().bold()
            );
            eprintln!("  {}", e);
            eprintln!();
            eprintln!("Your work is safe. Re-enter the workspace, fix the error, and exit again:");
            eprintln!("  {}", "csnotes recover --resume".bold());
            return Err(e);
        }

        // Steps 5-6: Snapshot → execute → restore on failure, cleanup on success.
        // Structural ops can fail mid-batch leaving a partial rename; the snapshot
        // makes this block atomic so the AI always re-enters a clean workspace.
        take_workspace_snapshot(workspace_root, &config.synthetic_dir)?;
        let ops_result = execute_teardown_ops(remaining, workspace_root, config, manifest, now);
        if let Err(e) = ops_result {
            if let Err(re) = restore_workspace_snapshot(workspace_root, &config.synthetic_dir) {
                eprintln!("warning: failed to restore workspace snapshot: {}", re);
            }
            return Err(e);
        }
        cleanup_workspace_snapshot(workspace_root)?;
    }

    // Step 7: Build updated manifest (always — topics must reflect current state)
    let updated_manifest = crate::audit::reindex(workspace_root, config)?;
    // Preserve sessions and sources from the existing manifest (reindex only
    // touches topics)
    let mut new_manifest = manifest.clone();
    new_manifest.topics = updated_manifest.topics;

    // Mark session(s) as processed and sources as processed
    update_session_status(&report, &mut new_manifest, now, &config.synthetic_dir);
    update_source_status(&report, &mut new_manifest, now, &config.synthetic_dir);

    if !all_committed {
        // Step 8: Invariant suite
        let audit = invariant_suite(
            workspace_root,
            &config.synthetic_dir,
            &report,
            &new_manifest,
        )?;

        if !audit.is_clean() {
            eprintln!(
                "{}",
                "Invariant violations — workspace preserved.".red().bold()
            );
            audit.print();
            eprintln!();
            eprintln!(
                "Your work is safe. Re-enter the workspace, fix the violations, and exit again:"
            );
            eprintln!("  {}", "csnotes recover --resume".bold());
            bail!("invariant suite failed");
        }

        audit.print(); // Print any soft warnings

        // Step 9: Pre-merge snapshot
        if let Some(r) = manifest.session_in_progress.as_mut() {
            r.phase = "merging".to_string();
        }
        manifest.save(vault_root)?;
        let _snapshot = take_snapshot(vault_root, &config.synthetic_dir, run_id)?;

        // Step 10: Merge-back
        merge_back(workspace_root, vault_root, &config.synthetic_dir)?;
    }

    // Step 10.5: Relink raw notes — propagate path changes from structural ops
    // into user-authored raw notes.  Runs once at final teardown regardless of
    // whether ops were committed progressively.
    let raw_roots = collect_raw_note_roots(vault_root, config);
    let raw_root_refs: Vec<&std::path::Path> = raw_roots.iter().map(|p| p.as_path()).collect();
    let relinked =
        crate::ops::structural::relink_raw_notes(&report.operations, &raw_root_refs).unwrap_or(0);

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
    let flags_path = vault_root.join(&config.generated_dir).join("flags.json");
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
    let n_flags = report
        .review_flags
        .iter()
        .filter(|f| f.kind.is_actionable())
        .count();
    println!("  {} operation{}", n_ops, if n_ops == 1 { "" } else { "s" });
    if relinked > 0 {
        println!(
            "  {} raw note{} relinked",
            relinked,
            if relinked == 1 { "" } else { "s" }
        );
    }
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

// ── Teardown op execution ─────────────────────────────────────────────────────

/// Run structural then content ops for teardown.  Caller holds a workspace
/// snapshot and restores it if this returns `Err`.
fn execute_teardown_ops(
    ops: &[Op],
    workspace_root: &Path,
    config: &VaultConfig,
    manifest: &Manifest,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    if let Err(e) = execute_structural_ops_slice(ops, workspace_root, config) {
        eprintln!("{}", "Structural op failed.".red().bold());
        eprintln!("  {}", e);
        eprintln!();
        eprintln!(
            "Workspace restored to pre-op state. Re-enter the workspace, fix the error, and exit again:"
        );
        eprintln!("  {}", "csnotes recover --resume".bold());
        return Err(e);
    }
    execute_content_ops_slice(ops, workspace_root, now, manifest)?;
    Ok(())
}

// ── Shared op execution helpers ───────────────────────────────────────────────

/// Run structural ops from `ops` against the workspace `_synthetic/` copy.
/// Skips content ops (`CreateNote`, `UpdateNote`).
/// Returns `Err` with op-kind context on the first failure.
pub(crate) fn execute_structural_ops_slice(
    ops: &[Op],
    workspace_root: &Path,
    config: &VaultConfig,
) -> Result<()> {
    use anyhow::Context as _;
    for op in ops {
        let result = match op {
            Op::RenameTopic(o) => crate::ops::structural::execute_rename_topic(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::RenameAtomic(o) => crate::ops::structural::execute_rename_atomic(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::MoveAtomic(o) => crate::ops::structural::execute_move_atomic(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::PromoteAtomic(o) => crate::ops::structural::execute_promote_atomic(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::DemoteTopic(o) => crate::ops::structural::execute_demote_topic(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::MergeTopics(o) => crate::ops::structural::execute_merge_topics(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::SplitTopic(o) => crate::ops::structural::execute_split_topic(
                o,
                workspace_root,
                &config.synthetic_dir,
            ),
            Op::SetEmbed(o) => crate::ops::structural::execute_set_embed(o, workspace_root),
            _ => continue,
        };
        result.with_context(|| format!("{} failed", op.kind_str()))?;
    }
    Ok(())
}

/// Run content ops (`CreateNote`, `UpdateNote`) from `ops` against the
/// workspace.  Skips structural ops.
pub(crate) fn execute_content_ops_slice(
    ops: &[Op],
    workspace_root: &Path,
    now: chrono::DateTime<Utc>,
    manifest: &Manifest,
) -> Result<()> {
    for op in ops {
        match op {
            Op::CreateNote(o) => execute_create_note(o, workspace_root, now, manifest)?,
            Op::UpdateNote(o) => execute_update_note(o, workspace_root, now, manifest)?,
            _ => {}
        }
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect the raw-note directories for all active courses (or the flat
/// vault-root layout when `active_courses` is empty).  Only returns paths
/// that actually exist on disk.
pub(crate) fn collect_raw_note_roots(
    vault_root: &std::path::Path,
    config: &VaultConfig,
) -> Vec<std::path::PathBuf> {
    if config.active_courses.is_empty() {
        let p = vault_root.join(&config.raw_dir);
        if p.exists() {
            vec![p]
        } else {
            vec![]
        }
    } else {
        config
            .active_courses
            .iter()
            .map(|course| vault_root.join(course).join(&config.raw_dir))
            .filter(|p| p.exists())
            .collect()
    }
}

// ── Scope resolution ──────────────────────────────────────────────────────────

fn resolve_scope(args: &ProcessArgs, manifest: &Manifest) -> Result<WorkspaceScope> {
    if !args.sources.is_empty() {
        let source_ids = expand_source_ids(&args.sources, manifest)?;
        return Ok(WorkspaceScope::Source { source_ids });
    }
    if let Some(topic) = &args.topic {
        return Ok(WorkspaceScope::Topic {
            topic: topic.clone(),
        });
    }
    if let Some(course) = &args.course {
        if args.for_course.is_some() {
            anyhow::bail!("--for-course is a session disambiguator and cannot be combined with --course (scope)");
        }
        return Ok(WorkspaceScope::Course {
            course: course.clone(),
        });
    }
    if let Some(resource_query) = &args.resource {
        let resource_id = fuzzy_resolve_resource(resource_query, manifest)?;
        return Ok(WorkspaceScope::Resource { resource_id });
    }

    let session_id = resolve_session_id(args, manifest)?;
    Ok(WorkspaceScope::Session { session_id })
}

/// Resolve a resource query (exact ID or fuzzy substring) to a manifest resource ID.
fn fuzzy_resolve_resource(query: &str, manifest: &Manifest) -> Result<String> {
    let normalize = |s: &str| s.to_lowercase().replace(['-', '_', '/'], "");
    let query_norm = normalize(query);

    // Exact match first.
    if manifest.resources.contains_key(query) {
        return Ok(query.to_string());
    }

    let matches: Vec<&str> = manifest
        .resources
        .keys()
        .filter(|id| normalize(id).contains(&query_norm))
        .map(|s| s.as_str())
        .collect();

    match matches.len() {
        0 => anyhow::bail!(
            "resource '{}' not found (no match in manifest — run `csnotes reconcile` first)",
            query
        ),
        1 => Ok(matches[0].to_string()),
        _ => anyhow::bail!(
            "resource '{}' is ambiguous — matches: {}",
            query,
            matches.join(", ")
        ),
    }
}

/// Resolve include_resource queries to manifest resource IDs.
pub fn resolve_include_resources(queries: &[String], manifest: &Manifest) -> Result<Vec<String>> {
    queries
        .iter()
        .map(|q| fuzzy_resolve_resource(q, manifest))
        .collect()
}

/// Expand a list of source IDs or path prefixes to concrete manifest IDs.
///
/// Each entry is either an exact source ID or a prefix: `Textbooks/SICP` expands
/// to all source IDs that start with `Textbooks/SICP/`.
fn expand_source_ids(raw: &[String], manifest: &Manifest) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for id in raw {
        if manifest.sources.contains_key(id.as_str()) {
            result.push(id.clone());
        } else {
            let prefix = format!("{}/", id);
            let mut matches: Vec<String> = manifest
                .sources
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect();
            matches.sort();
            if matches.is_empty() {
                anyhow::bail!(
                    "source '{}' not found (no exact match or prefix match in manifest)",
                    id
                );
            }
            result.extend(matches);
        }
    }
    Ok(result)
}

fn resolve_session_id(args: &ProcessArgs, manifest: &Manifest) -> Result<String> {
    let unprocessed: Vec<String> = manifest
        .sessions
        .iter()
        .filter(|(_, e)| e.status == SessionStatus::Unprocessed)
        .map(|(id, _)| id.clone())
        .collect();

    if let Some(session) = &args.session {
        if let Some(course) = &args.for_course {
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
                "Date '{}' matches multiple sessions: {}. Use --for-course to disambiguate.",
                session,
                matches.join(", ")
            ),
        }
    }

    // No explicit session: auto-resolve
    match unprocessed.len() {
        0 => bail!("No unprocessed sessions. Use --topic <name> for a topic review or --course <name> for a course-wide review. Run `csnotes status` to see what's available."),
        1 => Ok(unprocessed.into_iter().next().unwrap()),
        _ => {
            if args.next {
                // Pick the oldest pending session by date, breaking ties by ID.
                let id = unprocessed
                    .into_iter()
                    .min_by_key(|id| {
                        manifest
                            .sessions
                            .get(id)
                            .map(|e| (e.date, id.clone()))
                            .unwrap_or_default()
                    })
                    .unwrap();
                return Ok(id);
            }
            eprintln!("Multiple unprocessed sessions:");
            for id in &unprocessed {
                eprintln!("  {}", id);
            }
            bail!("Use --session, --for-course, or --next to specify which session to process.");
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
    use crate::report::{
        CreateNoteOp, Op, RenameTopicOp, ReportScope, ScopeKind, SessionReport, UpdateNoteOp,
    };
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

    fn resolve(
        session: Option<&str>,
        for_course: Option<&str>,
        manifest: &Manifest,
    ) -> Result<String> {
        resolve_session_id(
            &ProcessArgs {
                session: session.map(|s| s.to_string()),
                for_course: for_course.map(|s| s.to_string()),
                course: None,
                sources: vec![],
                topic: None,
                resource: None,
                include_resources: vec![],
                dry_run: false,
                backend: None,
                fixture: None,
                agy_model: None,
                claude_model: None,
                next: false,
            },
            manifest,
        )
    }

    #[test]
    fn resolve_session_full_date_matches() {
        let m = make_manifest_with_sessions(&[("CS101-2026-07-28", "CS101", "2026-07-28")]);
        assert_eq!(
            resolve(Some("2026-07-28"), None, &m).unwrap(),
            "CS101-2026-07-28"
        );
    }

    #[test]
    fn resolve_session_partial_mm_dd_matches() {
        let m = make_manifest_with_sessions(&[("CS101-2026-07-28", "CS101", "2026-07-28")]);
        assert_eq!(
            resolve(Some("07-28"), None, &m).unwrap(),
            "CS101-2026-07-28"
        );
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
        use crate::config::{AiBackend, SkillVariant, SnapshotMode};
        use crate::manifest::{ManifestConfig, SourceEntry, SourceKind};

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
                courses: vec![],
            },
        );

        let mut report = make_report(vec![Op::UpdateNote(UpdateNoteOp {
            path: "_synthetic/algorithms/search.md".to_string(),
            add_provenance: ProvenanceDelta::default(),
            sections: vec![],
            change_summary: "added".to_string(),
        })]);
        report.scope.sources = vec!["SICP/ch01".to_string()];

        update_source_status(&report, &mut manifest, Utc::now(), "_synthetic");

        let entry = manifest.sources.get("SICP/ch01").unwrap();
        assert_eq!(entry.status, SourceStatus::Processed);
        assert!(entry.last_processed_at.is_some());
        assert_eq!(entry.topics_updated, vec!["algorithms"]);
    }

    fn make_manifest_with_sources(sources: &[&str]) -> Manifest {
        use crate::config::{AiBackend, SkillVariant, SnapshotMode};
        use crate::manifest::{ManifestConfig, SourceEntry, SourceKind, SourceStatus};
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
        let mut m = Manifest::empty(std::path::PathBuf::from("/tmp"), cfg);
        for &id in sources {
            m.sources.insert(
                id.to_string(),
                SourceEntry {
                    path: format!("sources/{}.md", id),
                    kind: SourceKind::Textbook,
                    status: SourceStatus::Unprocessed,
                    last_processed_at: None,
                    heading_scheme: vec![],
                    topics_updated: vec![],
                    summary: None,
                    tags: vec![],
                    courses: vec![],
                },
            );
        }
        m
    }

    #[test]
    fn expand_source_ids_exact_match() {
        let m = make_manifest_with_sources(&["Textbooks/SICP/Ch01"]);
        let result = expand_source_ids(&["Textbooks/SICP/Ch01".to_string()], &m).unwrap();
        assert_eq!(result, vec!["Textbooks/SICP/Ch01"]);
    }

    #[test]
    fn expand_source_ids_prefix_expands_all_sorted() {
        let m = make_manifest_with_sources(&[
            "Textbooks/SICP/Ch02",
            "Textbooks/SICP/Ch01",
            "Textbooks/TAPL/Ch01",
        ]);
        let result = expand_source_ids(&["Textbooks/SICP".to_string()], &m).unwrap();
        assert_eq!(result, vec!["Textbooks/SICP/Ch01", "Textbooks/SICP/Ch02"]);
    }

    #[test]
    fn expand_source_ids_no_match_errors() {
        let m = make_manifest_with_sources(&["Textbooks/SICP/Ch01"]);
        assert!(expand_source_ids(&["Textbooks/TAPL".to_string()], &m).is_err());
    }

    // ── count_words_in_file ───────────────────────────────────────────────────

    #[test]
    fn count_words_returns_correct_count() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "one two three four five").unwrap();
        assert_eq!(count_words_in_file(tmp.path()), Some(5));
    }

    #[test]
    fn count_words_empty_file_returns_zero() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "").unwrap();
        assert_eq!(count_words_in_file(tmp.path()), Some(0));
    }

    #[test]
    fn count_words_missing_file_returns_none() {
        let path = std::path::Path::new("/tmp/csnotes-test-nonexistent-file-xyzzy.md");
        assert_eq!(count_words_in_file(path), None);
    }

    #[test]
    fn count_words_whitespace_only_returns_zero() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "   \n\t\n   ").unwrap();
        assert_eq!(count_words_in_file(tmp.path()), Some(0));
    }

    #[test]
    fn count_words_multiline_counts_all_words() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "first line\nsecond line\nthird line\n").unwrap();
        assert_eq!(count_words_in_file(tmp.path()), Some(6));
    }

    // ── resolve_scope ─────────────────────────────────────────────────────────

    fn make_args_course(course: &str) -> ProcessArgs {
        ProcessArgs {
            session: None,
            for_course: None,
            course: Some(course.to_string()),
            sources: vec![],
            topic: None,
            resource: None,
            include_resources: vec![],
            dry_run: false,
            backend: None,
            fixture: None,
            agy_model: None,
            claude_model: None,
            next: false,
        }
    }

    #[test]
    fn resolve_scope_course_returns_course_scope() {
        let m = make_manifest_with_sessions(&[]);
        let scope = resolve_scope(&make_args_course("CPSC5002"), &m).unwrap();
        assert!(matches!(scope, WorkspaceScope::Course { course } if course == "CPSC5002"));
    }

    #[test]
    fn resolve_scope_course_and_for_course_conflict_errors() {
        let m = make_manifest_with_sessions(&[]);
        let args = ProcessArgs {
            session: None,
            for_course: Some("CPSC5002".to_string()),
            course: Some("CPSC5002".to_string()),
            sources: vec![],
            topic: None,
            resource: None,
            include_resources: vec![],
            dry_run: false,
            backend: None,
            fixture: None,
            agy_model: None,
            claude_model: None,
            next: false,
        };
        assert!(resolve_scope(&args, &m).is_err());
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

/// Count whitespace-delimited words in a file.  Returns `None` if the file
/// cannot be read (missing or unreadable — those errors are handled elsewhere).
fn count_words_in_file(path: &std::path::Path) -> Option<usize> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(content.split_whitespace().count())
}

enum ShortFileChoice {
    Continue,
    Pause,
    Quit,
}

fn prompt_short_files(session_id: &str, files: &[(String, usize)]) -> Result<ShortFileChoice> {
    use std::io::{self, BufRead, Write};
    eprintln!(
        "Warning: {} input file{} for {} {} suspiciously short (< 100 words) — may be a placeholder:",
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        session_id,
        if files.len() == 1 { "is" } else { "are" },
    );
    for (path, wc) in files {
        eprintln!("  {} ({} words)", path, wc);
    }
    print!("[c]ontinue / [p]ause / [q]uit: ");
    io::stdout().flush()?;
    let stdin = io::stdin();
    let line = stdin.lock().lines().next().unwrap_or(Ok(String::new()))?;
    Ok(match line.trim() {
        "p" => ShortFileChoice::Pause,
        "q" => ShortFileChoice::Quit,
        _ => ShortFileChoice::Continue,
    })
}
