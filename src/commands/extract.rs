#![allow(dead_code)]
use anyhow::Result;

/// `csnotes extract` — Phase 2 stub.
pub struct ExtractArgs {
    pub session: Option<String>,
    pub kind: Option<String>,
    pub stdout: bool,
}

pub fn run(_args: ExtractArgs) -> Result<()> {
    println!("csnotes extract: not yet implemented (Phase 2).");
    Ok(())
}
