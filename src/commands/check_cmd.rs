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

    let result = check_workspace(&workspace_root, "_synthetic")?;

    if result.hard_violations.is_empty() && result.soft_warnings.is_empty() {
        println!("check: clean — no violations found");
        return Ok(());
    }

    let n_hard = result.hard_violations.len();
    let n_soft = result.soft_warnings.len();

    for v in &result.hard_violations {
        eprintln!("ERROR: {}", v);
    }
    for w in &result.soft_warnings {
        println!("WARN:  {}", w);
    }

    if n_hard > 0 {
        eprintln!(
            "\ncheck: {} hard violation(s), {} warning(s)",
            n_hard, n_soft
        );
        eprintln!(
            "Fix the ERRORs above before exiting — \
             the teardown pipeline will discard the workspace if they remain."
        );
        std::process::exit(1);
    }

    println!("\ncheck: clean (0 errors, {} warning(s))", n_soft);
    Ok(())
}
