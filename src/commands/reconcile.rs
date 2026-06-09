#![allow(dead_code)]
use anyhow::Result;

/// `csnotes reconcile` — Phase 2 stub.
pub struct ReconcileArgs {
    pub notify: bool,
    pub rename_spaces: Option<String>, // "hyphens" | "underscores"
}

pub fn run(_args: ReconcileArgs) -> Result<()> {
    println!("csnotes reconcile: not yet implemented (Phase 2).");
    Ok(())
}
