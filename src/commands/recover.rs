use std::fs;
use std::io::{self, BufRead, Write};

use anyhow::{bail, Context, Result};

use crate::backend::make_backend;
use crate::config::{find_vault_root, VaultConfig};
use crate::error::CsnotesError;
use crate::manifest::{Manifest, ManifestLock};
use crate::report::REPORT_FILENAME;
use crate::workspace::{cleanup, copy_dir, restore_snapshot, WorkspaceMeta};

pub struct RecoverArgs {
    /// Skip interactive prompt and resume automatically.
    pub resume: bool,
    /// Skip interactive prompt and discard automatically.
    pub discard: bool,
    /// Rebuild _synthetic/ from vault state and clear the session report.
    /// The workspace structure (raw notes, _session.md, sources) is preserved.
    /// Use this when the AI has made a mess of _synthetic/ and you want a
    /// clean slate without re-assembling a new workspace.
    pub reset: bool,
}

pub fn run(args: RecoverArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let _lock = ManifestLock::acquire(&vault_root)?;
    let mut manifest = Manifest::load(&vault_root)?;

    if args.reset {
        return run_reset(&vault_root, &config, &manifest);
    }

    let rec = match manifest.session_in_progress.clone() {
        Some(r) => r,
        None => return Err(CsnotesError::NothingToRecover.into()),
    };

    println!("Found in-progress session:");
    println!("  run_id:    {}", rec.run_id);
    println!(
        "  started:   {}",
        rec.started_at.format("%Y-%m-%d %H:%M UTC")
    );
    println!("  phase:     {}", rec.phase);
    println!("  workspace: {}", rec.workspace_path.display());
    if let Some(ref err) = rec.error {
        println!("  error:     {}", err);
    }

    // Check for a mid-merge snapshot first
    let snapshot_path = vault_root.join(format!("_synthetic_snapshot_{}", rec.run_id));
    if snapshot_path.exists() {
        println!("\nPre-merge snapshot found — crash interrupted the merge.");
        println!("Restoring vault from snapshot...");
        restore_snapshot(&vault_root, &config.synthetic_dir, &rec.run_id)?;
        println!("Vault restored.");
    }

    // If the workspace is gone, the only sensible action is to clear the record.
    if !rec.workspace_path.exists() {
        println!("\nWorkspace no longer exists. Nothing to resume.");
        if !args.discard {
            prompt_discard_only()?;
        }
        manifest.session_in_progress = None;
        manifest.save(&vault_root)?;
        println!("In-progress record cleared. Vault is clean.");
        return Ok(());
    }

    // Decide: resume or discard
    let choice = if args.resume {
        Choice::Resume
    } else if args.discard {
        Choice::Discard
    } else {
        prompt_choice()?
    };

    match choice {
        Choice::Resume => {
            let report_path = rec.workspace_path.join(REPORT_FILENAME);

            // Check whether a parseable report already exists.  A file that
            // exists but fails to parse is treated as absent: re-enter the
            // session so the AI can fix it, rather than looping on teardown
            // failures forever.
            let report_ready = if report_path.exists() {
                match crate::report::SessionReport::load(&rec.workspace_path) {
                    Ok(_) => true,
                    Err(e) => {
                        eprintln!("Session report exists but is malformed: {}", e);
                        eprintln!("Re-entering session to fix the report.\n");
                        false
                    }
                }
            } else {
                false
            };

            if report_ready {
                // Report already written and valid — skip re-entry, just run teardown.
                println!("Session report found. Running teardown...");
                crate::commands::process::run_teardown(
                    &vault_root,
                    &rec.workspace_path,
                    &rec.run_id,
                    &config,
                    &mut manifest,
                )
            } else {
                // No report yet (or malformed) — re-enter the AI session, then tear down.
                if !report_path.exists() {
                    println!("No session report found. Re-entering AI session...");
                }
                println!("Write `_session_report.json` before exiting.\n");

                let backend_kind = rec.backend.unwrap_or(config.default_backend);
                let skill_variant = rec.skill_variant.unwrap_or(config.skill_variant);
                let backend = make_backend(
                    backend_kind,
                    skill_variant,
                    None,
                    config.agy_model.clone(),
                    None,
                    true,
                );

                if let Err(e) = backend.launch(&rec.workspace_path) {
                    eprintln!("Backend exited with error: {}", e);
                    eprintln!("Workspace preserved. Run `csnotes recover` again.");
                    if let Some(ref mut r) = manifest.session_in_progress {
                        r.error = Some(e.to_string());
                    }
                    manifest.save(&vault_root)?;
                    return Ok(());
                }

                crate::commands::process::run_teardown(
                    &vault_root,
                    &rec.workspace_path,
                    &rec.run_id,
                    &config,
                    &mut manifest,
                )
            }
        }
        Choice::Discard => {
            println!("Discarding workspace...");
            cleanup(&rec.workspace_path, &vault_root, &rec.run_id)?;
            manifest.session_in_progress = None;
            manifest.save(&vault_root)?;
            println!("Done. Vault is clean.");
            Ok(())
        }
    }
}

enum Choice {
    Resume,
    Discard,
}

fn prompt_choice() -> Result<Choice> {
    print!("[r]esume session / [d]iscard workspace: ");
    io::stdout().flush()?;
    let stdin = io::stdin();
    let line = stdin.lock().lines().next().unwrap_or(Ok(String::new()))?;
    Ok(match line.trim() {
        "r" | "resume" => Choice::Resume,
        _ => Choice::Discard,
    })
}

fn prompt_discard_only() -> Result<()> {
    print!("Clear in-progress record? [y/N]: ");
    io::stdout().flush()?;
    let stdin = io::stdin();
    let line = stdin.lock().lines().next().unwrap_or(Ok(String::new()))?;
    match line.trim() {
        "y" | "Y" => Ok(()),
        _ => {
            println!("Aborted.");
            std::process::exit(0);
        }
    }
}

/// Rebuild `_synthetic/` in the workspace from the vault's current state, then
/// clear the session report and reset `committed_ops`.  Everything else in the
/// workspace (raw notes, `_session.md`, sources, hooks) is preserved.
///
/// Use this when the AI has made a mess of `_synthetic/` and the student wants
/// a clean slate without tearing down and re-assembling a fresh workspace.
pub(crate) fn run_reset(
    vault_root: &std::path::Path,
    config: &VaultConfig,
    manifest: &Manifest,
) -> Result<()> {
    let rec = match manifest.session_in_progress.as_ref() {
        Some(r) => r,
        None => bail!(
            "No session is currently in progress. Nothing to reset.\n\
             Start a session with `csnotes process` first."
        ),
    };

    if !rec.workspace_path.exists() {
        bail!(
            "Workspace no longer exists at {}.\n\
             Use `csnotes recover --discard` to clean up the stale record.",
            rec.workspace_path.display()
        );
    }

    use owo_colors::OwoColorize;

    let ws_synthetic = rec.workspace_path.join(&config.synthetic_dir);
    let vault_synthetic = vault_root.join(&config.synthetic_dir);

    // 1. Wipe workspace _synthetic/ and rebuild from vault.
    if ws_synthetic.exists() {
        fs::remove_dir_all(&ws_synthetic)
            .with_context(|| format!("clearing workspace {}", config.synthetic_dir))?;
    }
    if vault_synthetic.exists() {
        copy_dir(&vault_synthetic, &ws_synthetic)
            .with_context(|| format!("copying vault {} into workspace", config.synthetic_dir))?;
    } else {
        fs::create_dir_all(&ws_synthetic)?;
    }

    // 2. Delete the session report so the AI starts fresh.
    let report_path = rec.workspace_path.join(REPORT_FILENAME);
    if report_path.exists() {
        fs::remove_file(&report_path).with_context(|| format!("removing {}", REPORT_FILENAME))?;
    }

    // 3. Reset committed_ops in _workspace_meta.json.
    if let Ok(meta) = WorkspaceMeta::load(&rec.workspace_path) {
        WorkspaceMeta {
            vault_root: meta.vault_root,
            run_id: meta.run_id,
            committed_ops: Vec::new(),
        }
        .save(&rec.workspace_path)?;
    }

    println!(
        "{} {} rebuilt from vault; report and committed_ops cleared.",
        "recover --reset:".green().bold(),
        config.synthetic_dir,
    );
    println!("  Workspace preserved at {}", rec.workspace_path.display());
    println!("  Resume the session with `csnotes recover --resume`.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant};
    use crate::manifest::{InProgressRecord, Manifest, ManifestConfig};
    use crate::report::Op;
    use crate::workspace::WORKSPACE_META_FILENAME;
    use tempfile::TempDir;

    fn make_config() -> VaultConfig {
        serde_json::from_str("{}").unwrap()
    }

    fn make_manifest_config() -> ManifestConfig {
        ManifestConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            default_backend: AiBackend::Mock,
            skill_variant: SkillVariant::Claude,
            snapshot_mode: crate::config::SnapshotMode::PreMerge,
        }
    }

    fn make_in_progress(workspace_path: std::path::PathBuf) -> InProgressRecord {
        InProgressRecord {
            run_id: "test-run".into(),
            started_at: chrono::Utc::now(),
            workspace_path,
            phase: "synthesizing".into(),
            error: None,
            backend: None,
            skill_variant: None,
        }
    }

    fn write(path: &std::path::Path, content: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    // ── run_reset bails ───────────────────────────────────────────────────────

    #[test]
    fn reset_bails_with_no_session_in_progress() {
        let vault = TempDir::new().unwrap();
        let config = make_config();
        let manifest = Manifest::empty(vault.path().to_path_buf(), make_manifest_config());

        let err = run_reset(vault.path(), &config, &manifest).unwrap_err();
        assert!(err.to_string().contains("No session"), "{err}");
    }

    #[test]
    fn reset_bails_when_workspace_missing() {
        let vault = TempDir::new().unwrap();
        let config = make_config();
        let mut manifest = Manifest::empty(vault.path().to_path_buf(), make_manifest_config());
        // Point to a workspace that doesn't exist on disk.
        manifest.session_in_progress = Some(make_in_progress(vault.path().join("ghost-ws")));

        let err = run_reset(vault.path(), &config, &manifest).unwrap_err();
        assert!(err.to_string().contains("longer exists"), "{err}");
    }

    // ── run_reset core behaviour ──────────────────────────────────────────────

    #[test]
    fn reset_clears_ws_synthetic_and_rebuilds_from_vault() {
        let vault = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        let config = make_config();

        // Vault has a clean note.
        write(
            &vault.path().join("_synthetic/cs/bfs.md"),
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: BFS\n\
             contributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nBFS content.\n",
        );

        // Workspace _synthetic/ has a messy/extra file.
        write(&ws.path().join("_synthetic/cs/garbage.md"), "mess");

        // Workspace meta (committed_ops will be cleared).
        WorkspaceMeta {
            vault_root: vault.path().to_path_buf(),
            run_id: "test-run".into(),
            committed_ops: vec![Op::UpdateNote(crate::report::UpdateNoteOp {
                path: "_synthetic/cs/bfs.md".into(),
                add_provenance: Default::default(),
                sections: vec![],
                change_summary: "prior commit".into(),
            })],
        }
        .save(ws.path())
        .unwrap();

        // Session report exists (should be deleted).
        write(
            &ws.path().join("_session_report.json"),
            r#"{"run_id":"test-run"}"#,
        );

        let mut manifest = Manifest::empty(vault.path().to_path_buf(), make_manifest_config());
        manifest.session_in_progress = Some(make_in_progress(ws.path().to_path_buf()));

        run_reset(vault.path(), &config, &manifest).unwrap();

        // Vault note must be present in workspace.
        assert!(
            ws.path().join("_synthetic/cs/bfs.md").exists(),
            "vault note must be copied into workspace _synthetic/"
        );
        // Garbage must be gone.
        assert!(
            !ws.path().join("_synthetic/cs/garbage.md").exists(),
            "pre-reset workspace file must be wiped"
        );
        // Report must be deleted.
        assert!(
            !ws.path().join("_session_report.json").exists(),
            "session report must be deleted"
        );
        // committed_ops must be empty.
        let meta = WorkspaceMeta::load(ws.path()).unwrap();
        assert!(
            meta.committed_ops.is_empty(),
            "committed_ops must be reset to []"
        );
    }

    #[test]
    fn reset_with_no_vault_synthetic_creates_empty_dir() {
        let vault = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        let config = make_config();

        // Workspace has a synthetic dir with some content.
        write(&ws.path().join("_synthetic/cs/note.md"), "content");

        WorkspaceMeta {
            vault_root: vault.path().to_path_buf(),
            run_id: "test-run".into(),
            committed_ops: vec![],
        }
        .save(ws.path())
        .unwrap();

        let mut manifest = Manifest::empty(vault.path().to_path_buf(), make_manifest_config());
        manifest.session_in_progress = Some(make_in_progress(ws.path().to_path_buf()));

        // Vault has no _synthetic/ — reset must create an empty dir.
        run_reset(vault.path(), &config, &manifest).unwrap();

        let ws_synthetic = ws.path().join("_synthetic");
        assert!(ws_synthetic.exists(), "_synthetic/ must be created");
        let is_empty = std::fs::read_dir(&ws_synthetic)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        assert!(is_empty, "_synthetic/ must be empty when vault has none");
    }

    #[test]
    fn reset_preserves_session_md_and_sources() {
        let vault = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        let config = make_config();

        // Write files that must survive the reset.
        write(&ws.path().join("_session.md"), "# Session\n");
        write(&ws.path().join("sources/textbook.md"), "# Textbook\n");
        write(&ws.path().join("CLAUDE.md"), "# Instructions\n");

        WorkspaceMeta {
            vault_root: vault.path().to_path_buf(),
            run_id: "test-run".into(),
            committed_ops: vec![],
        }
        .save(ws.path())
        .unwrap();

        let mut manifest = Manifest::empty(vault.path().to_path_buf(), make_manifest_config());
        manifest.session_in_progress = Some(make_in_progress(ws.path().to_path_buf()));

        run_reset(vault.path(), &config, &manifest).unwrap();

        assert!(
            ws.path().join("_session.md").exists(),
            "_session.md preserved"
        );
        assert!(
            ws.path().join("sources/textbook.md").exists(),
            "sources preserved"
        );
        assert!(ws.path().join("CLAUDE.md").exists(), "CLAUDE.md preserved");
        assert!(
            ws.path().join(WORKSPACE_META_FILENAME).exists(),
            "_workspace_meta.json preserved"
        );
    }
}
