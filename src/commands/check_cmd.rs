/// `csnotes check` — run invariant checks against the workspace _synthetic/
/// directory without needing a session report.
///
/// Designed to be run by Claude from inside the workspace before `exit` so
/// any violations can be corrected before the CLI teardown would discard the
/// workspace entirely.  All findings are printed to stdout/stderr; the process
/// exits with code 1 if hard violations are found, 0 if clean.
use std::path::PathBuf;

use anyhow::Result;

use crate::audit::check_workspace;
use crate::report::{SessionReport, REPORT_FILENAME};

pub struct CheckArgs {
    /// Workspace root to check (defaults to current directory).
    pub workspace: Option<PathBuf>,
}

pub fn run(args: CheckArgs) -> Result<()> {
    let workspace_root = match args.workspace {
        Some(p) => p,
        None => std::env::current_dir()?,
    };

    // Sanity check: warn if this doesn't look like a csnotes workspace.
    let session_file = workspace_root.join("_session.md");
    if !session_file.exists() {
        eprintln!(
            "warning: '_session.md' not found in '{}' — \
             are you running this from inside a csnotes workspace?",
            workspace_root.display()
        );
    }

    // If the session report already exists, validate it now so the AI can fix
    // any problems before exiting rather than hitting teardown failures.
    let report_path = workspace_root.join(REPORT_FILENAME);
    let mut report_errors: Vec<String> = vec![];
    if report_path.exists() {
        match SessionReport::load(&workspace_root) {
            Ok(report) => {
                println!("check: session report OK");
                // Validate report preconditions against the workspace so the AI
                // can catch embed_in / path mismatches before exiting.
                if let Err(e) = crate::audit::precondition_pass(&report, &workspace_root) {
                    report_errors.push(format!("precondition failure: {}", e));
                }
            }
            Err(e) => report_errors.push(format!("session report invalid: {}", e)),
        }
    }

    let result = check_workspace(&workspace_root, "_synthetic")?;

    let n_hard = result.hard_violations.len() + report_errors.len();
    let n_soft = result.soft_warnings.len();

    for e in &report_errors {
        eprintln!("ERROR: {}", e);
    }
    for v in &result.hard_violations {
        eprintln!("ERROR: {}", v);
    }
    for w in &result.soft_warnings {
        println!("WARN:  {}", w);
    }

    if n_hard == 0 && n_soft == 0 {
        println!("check: clean — no violations found");
        return Ok(());
    }

    if n_hard > 0 {
        eprintln!(
            "\ncheck: {} hard violation(s), {} warning(s)",
            n_hard, n_soft
        );
        eprintln!(
            "Fix the ERRORs above before exiting — \
             your work is preserved until you exit cleanly."
        );
        std::process::exit(1);
    }

    println!("\ncheck: clean (0 errors, {} warning(s))", n_soft);
    Ok(())
}
