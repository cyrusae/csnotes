use anyhow::Result;

use crate::config::{VaultConfig, find_vault_root};
use crate::manifest::Manifest;

pub struct AuditArgs {
    pub reindex: bool,
    pub fix: bool,
}

pub fn run(args: AuditArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let _manifest = Manifest::load(&vault_root)?;

    if args.reindex {
        println!("Rebuilding manifest from frontmatter...");
        let new_manifest = crate::audit::reindex(&vault_root, &config)?;
        new_manifest.save(&vault_root)?;
        println!("Manifest rebuilt: {} topics, {} sessions, {} sources",
            new_manifest.topics.len(),
            new_manifest.sessions.len(),
            new_manifest.sources.len(),
        );
        return Ok(());
    }

    // Read-only invariant run against the vault
    println!("Running invariant suite (read-only)...");

    // We need a dummy report to satisfy the invariant_suite signature.
    // For a direct audit, we check the vault state without a report context.
    // TODO: decouple audit from SessionReport for direct vault audits.
    let audit = crate::audit::audit_vault(&vault_root, &config)?;
    audit.print();

    if args.fix {
        println!("\nApplying mechanical repairs (--fix)...");
        println!("  (Phase 4 — not yet implemented)");
    }

    if audit.is_clean() {
        println!("Vault is clean.");
    } else {
        std::process::exit(1);
    }

    Ok(())
}
