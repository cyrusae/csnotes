use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::audit::{invariant_suite, precondition_pass};
use crate::backend::make_backend;
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
}

pub fn run(args: ProcessArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
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
        println!("dry-run: would launch {} against {}", backend_kind, workspace_root.display());
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
    let backend = make_backend(backend_kind, skill_variant, args.fixture);
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

    // Step 5: Structural ops (Phase 1+; Phase 0 has none)
    for op in &report.operations {
        if op.is_structural() {
            eprintln!("Structural op '{}' is not yet supported (Phase 1+). Discarding workspace.", op.kind_str());
            cleanup(workspace_root, vault_root, run_id)?;
            manifest.session_in_progress = None;
            manifest.save(vault_root)?;
            bail!("structural ops not yet implemented");
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

    // Mark session as processed
    update_session_status(&report, &mut new_manifest, now);

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

    // Copy last_report.json
    let report_src = workspace_root.join(REPORT_FILENAME);
    if report_src.exists() {
        std::fs::copy(
            &report_src,
            new_manifest.last_report_path(),
        )
        .ok(); // non-fatal
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
    for session_id in &report.scope.sessions {
        if let Some(entry) = manifest.sessions.get_mut(session_id) {
            entry.status = SessionStatus::Processed;
            entry.processed_at = Some(now);
            // topics_updated will be populated from reindex in Phase 1+
        }
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
