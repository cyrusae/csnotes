use std::io::{self, BufRead, Write};

use anyhow::{bail, Result};

use crate::config::{VaultConfig, find_vault_root};
use crate::error::CsnotesError;
use crate::manifest::Manifest;
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
            println!("Resuming session...");
            // Re-use the existing workspace and run teardown
            if !rec.workspace_path.exists() {
                bail!("Workspace no longer exists at {}. Discard instead.", rec.workspace_path.display());
            }
            crate::commands::process::run_teardown(
                &vault_root,
                &rec.workspace_path,
                &rec.run_id,
                &config,
                &mut manifest,
            )
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
