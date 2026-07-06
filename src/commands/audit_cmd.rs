use anyhow::Result;

use crate::config::{find_vault_root, VaultConfig};
use crate::manifest::{Manifest, ManifestLock};

pub struct AuditArgs {
    pub reindex: bool,
    pub fix: bool,
    /// Only meaningful when `fix` is also true.  Without `--apply`, `--fix`
    /// shows the repair plan without writing anything.
    pub apply: bool,
    pub show_discrepancies: bool,
}

pub fn run(args: AuditArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;

    if args.reindex {
        let _lock = ManifestLock::acquire(&vault_root)?;
        println!("Rebuilding manifest from frontmatter...");
        let new_manifest = crate::audit::reindex(&vault_root, &config)?;
        new_manifest.save(&vault_root)?;
        println!(
            "Manifest rebuilt: {} topics, {} sessions, {} sources",
            new_manifest.topics.len(),
            new_manifest.sessions.len(),
            new_manifest.sources.len(),
        );
        return Ok(());
    }

    // ── Read-only invariant run ───────────────────────────────────────────────
    println!("Running invariant suite (read-only)...");
    let audit = crate::audit::audit_vault(&vault_root, &config, &manifest)?;
    audit.print();

    // ── Fix plan ──────────────────────────────────────────────────────────────
    if args.fix {
        println!();
        let fixes = crate::audit::collect_fixes(&vault_root, &config, &manifest)?;

        if fixes.is_empty() {
            println!("No mechanical repairs needed.");
        } else {
            println!("Repair plan ({}):", fixes.len());
            for fix in &fixes {
                println!("  ~ {}", fix.description);
            }

            if args.apply {
                let n = crate::audit::apply_fixes(&fixes)?;
                println!("\nApplied {} repair(s).", n);
                println!("Re-run `csnotes audit` to verify the vault is now clean.");
            } else {
                println!();
                println!("Run `csnotes audit --fix --apply` to execute these repairs.");
            }
        }

        // Note violations that are NOT auto-fixable
        if !audit.hard_violations.is_empty() {
            let fixable = fixes.len();
            let total = audit.hard_violations.len();
            // The fixable violations are a subset of hard_violations (anchor
            // mismatches).  Any remaining violations need manual work.
            if total > fixable {
                println!();
                println!(
                    "{} violation(s) above require manual resolution.",
                    total - fixable
                );
            }
        }
    }

    // ── Discrepancy report ────────────────────────────────────────────────────
    let mut has_discrepancies = false;
    if args.show_discrepancies {
        println!();
        println!("Manifest vs filesystem discrepancies:");
        let discrepancies = crate::audit::manifest_discrepancies(&vault_root, &config, &manifest)?;
        if discrepancies.is_empty() {
            println!("  none — manifest matches filesystem");
        } else {
            has_discrepancies = true;
            for d in &discrepancies {
                println!("  {}", d.display());
            }
        }
    }

    // ── Exit code ─────────────────────────────────────────────────────────────
    if !audit.is_clean() || has_discrepancies {
        std::process::exit(1);
    }

    if !args.fix {
        println!();
        if let Some(ref rec) = manifest.session_in_progress {
            println!(
                "⚠  session in progress  run_id={}  phase={}",
                rec.run_id, rec.phase
            );
        }
        println!("Vault is clean.");
        if !manifest.topics.is_empty() {
            println!();
            for (name, topic) in &manifest.topics {
                println!(
                    "  {:<30}  {} atomic{}",
                    name,
                    topic.atomic_notes.len(),
                    if topic.atomic_notes.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                );
            }
        }
    }

    Ok(())
}
