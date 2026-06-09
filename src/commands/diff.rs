#![allow(dead_code)]
use anyhow::Result;

use crate::config::find_vault_root;
use crate::manifest::Manifest;
use crate::report::SessionReport;

pub struct DiffArgs {
    pub session: Option<String>,
}

pub fn run(_args: DiffArgs) -> Result<()> {
    // Phase 1
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let manifest = Manifest::load(&vault_root)?;

    let report_path = manifest.last_report_path();
    if !report_path.exists() {
        println!("No previous session report found.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&report_path)?;
    let report: SessionReport = serde_json::from_str(&content)?;

    println!("Last session: {} ({})", report.run_id, report.completed_at.format("%Y-%m-%d"));
    println!();

    for op in &report.operations {
        match op {
            crate::report::Op::CreateNote(op) => {
                println!("  + {} ({})", op.path, op.change_summary);
            }
            crate::report::Op::UpdateNote(op) => {
                println!("  ~ {} — {}", op.path, op.change_summary);
            }
            _ => {
                println!("  * {} (structural)", op.kind_str());
            }
        }
    }

    for flag in &report.review_flags {
        if flag.kind.is_actionable() {
            println!("  ! [{}] {}", flag.kind.tier_label(), flag.message);
        }
    }

    Ok(())
}
