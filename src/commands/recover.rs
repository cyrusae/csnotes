use std::io::{self, BufRead, Write};

use anyhow::Result;

use crate::backend::make_backend;
use crate::config::{VaultConfig, find_vault_root};
use crate::error::CsnotesError;
use crate::manifest::{Manifest, ManifestLock};
use crate::report::REPORT_FILENAME;
use crate::workspace::{cleanup, restore_snapshot};

pub struct RecoverArgs {
    /// Skip interactive prompt and resume automatically.
    pub resume: bool,
    /// Skip interactive prompt and discard automatically.
    pub discard: bool,
}

pub fn run(args: RecoverArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let _lock = ManifestLock::acquire(&vault_root)?;
    let mut manifest = Manifest::load(&vault_root)?;

    let rec = match manifest.session_in_progress.clone() {
        Some(r) => r,
        None => return Err(CsnotesError::NothingToRecover.into()),
    };

    println!("Found in-progress session:");
    println!("  run_id:    {}", rec.run_id);
    println!("  started:   {}", rec.started_at.format("%Y-%m-%d %H:%M UTC"));
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

            if report_path.exists() {
                // Report already written — skip re-entry, just run teardown.
                println!("Session report found. Running teardown...");
                crate::commands::process::run_teardown(
                    &vault_root,
                    &rec.workspace_path,
                    &rec.run_id,
                    &config,
                    &mut manifest,
                )
            } else {
                // No report yet — re-enter the AI session, then tear down.
                println!("No session report found. Re-entering AI session...");
                println!("Write `_session_report.json` before exiting.\n");

                let backend_kind = rec.backend.unwrap_or(config.default_backend);
                let skill_variant = rec.skill_variant.unwrap_or(config.skill_variant);
                let backend = make_backend(backend_kind, skill_variant, None, config.agy_model.clone(), true);

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
