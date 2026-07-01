/// `csnotes commit` — run the teardown pipeline from inside an active session.
///
/// Called by the AI (or user) from within the workspace directory to commit
/// the current batch of ops to the vault without ending the session.  The
/// workspace persists after a successful commit so work can continue.
///
/// Progressive workflow:
///
///   1. AI declares ops 1-N in `_session_report.json`.
///   2. `csnotes commit` executes those ops and merges them to the vault.
///      Committed ops are saved to `_workspace_meta.json` (CLI-owned ground truth).
///   3. AI continues — may freely rewrite `_session_report.json`.
///   4. `csnotes commit` again — only ops after the committed count run.
///   5. AI exits.  Teardown sees all ops committed and skips straight to
///      session-status update + cleanup.
///
/// The committed op list in `_workspace_meta.json` is the authoritative record.
/// If the AI rewrites the report and removes already-committed ops, commit
/// detects the count mismatch and bails before executing anything.
///
/// Each commit is atomic: a workspace-local snapshot is taken before ops run;
/// on any failure the snapshot is restored so the AI can fix and retry.
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::Utc;
use owo_colors::OwoColorize;

use crate::audit::{invariant_suite, precondition_pass_ops};
use crate::config::{find_vault_root, VaultConfig};
use crate::manifest::{Manifest, ManifestLock};
use crate::report::{SessionReport, REPORT_FILENAME};
use crate::workspace::{
    cleanup_workspace_snapshot, merge_back, restore_workspace_snapshot, take_snapshot,
    take_workspace_snapshot, WorkspaceMeta, WORKSPACE_META_FILENAME,
};

use super::process::{execute_content_ops_slice, execute_structural_ops_slice};

pub struct CommitArgs {
    /// Workspace root to commit from (defaults to current directory).
    pub workspace: Option<PathBuf>,
    /// Validate preconditions and print what would run without executing.
    pub dry_run: bool,
}

pub fn run(args: CommitArgs) -> Result<()> {
    // Locate workspace root: explicit arg, else current dir.
    let workspace_root = match args.workspace {
        Some(p) => p,
        None => std::env::current_dir()?,
    };

    if !workspace_root.join(WORKSPACE_META_FILENAME).exists() {
        bail!(
            "Not inside a csnotes workspace. \
             Run this command from the workspace directory, \
             or use `csnotes commit --workspace <path>`."
        );
    }

    let meta = WorkspaceMeta::load(&workspace_root)?;
    let vault_root = find_vault_root(&meta.vault_root)?;
    let config = VaultConfig::load(&vault_root)?;
    let _lock = ManifestLock::acquire(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;

    let report_path = workspace_root.join(REPORT_FILENAME);
    if !report_path.exists() {
        bail!(
            "No session report found at {}. \
             Write `_session_report.json` before committing.",
            report_path.display()
        );
    }
    let report = SessionReport::load(&workspace_root)?;

    // run_id guard — mismatch indicates a workspace/vault mismatch.
    if report.run_id != meta.run_id {
        bail!(
            "run_id mismatch: workspace_meta says '{}', report says '{}'. \
             Do not copy workspaces between sessions.",
            meta.run_id,
            report.run_id
        );
    }

    let total_ops = report.operations.len();
    let committed_count = meta.committed_ops.len();

    if total_ops < committed_count {
        bail!(
            "{} op{} already committed but report only has {} op{}. \
             The report may have been rewritten — do not remove committed ops. \
             Fix the report and run `csnotes commit` again.",
            committed_count,
            if committed_count == 1 { "" } else { "s" },
            total_ops,
            if total_ops == 1 { "" } else { "s" },
        );
    }

    let remaining = &report.operations[committed_count..];

    if remaining.is_empty() {
        println!(
            "{} all {} op{} already committed.",
            "commit:".green().bold(),
            total_ops,
            if total_ops == 1 { "" } else { "s" }
        );
        return Ok(());
    }

    if args.dry_run {
        return dry_run_report(remaining, committed_count, total_ops, &workspace_root);
    }

    // ── Precondition pass ─────────────────────────────────────────────────────
    if let Err(e) = precondition_pass_ops(remaining, &workspace_root) {
        eprintln!(
            "{}",
            "Precondition failure — nothing committed.".red().bold()
        );
        eprintln!("  {}", e);
        eprintln!();
        eprintln!("Fix the issue and run `csnotes commit` again.");
        return Err(e);
    }

    // ── Workspace snapshot (atomic guarantee) ─────────────────────────────────
    // Taken before any mutation so the workspace can be restored on failure.
    take_workspace_snapshot(&workspace_root, &config.synthetic_dir)?;

    let result = commit_ops(
        remaining,
        &workspace_root,
        &vault_root,
        &config,
        &manifest,
        &meta,
        total_ops,
    );

    if let Err(e) = result {
        // Restore workspace to pre-commit state so the AI can fix and retry.
        if let Err(restore_err) = restore_workspace_snapshot(&workspace_root, &config.synthetic_dir)
        {
            eprintln!(
                "warning: failed to restore workspace snapshot: {}",
                restore_err
            );
        }
        return Err(e);
    }

    cleanup_workspace_snapshot(&workspace_root)?;
    Ok(())
}

/// Execute the remaining ops, merge to vault, and save committed_ops to meta.
/// Workspace snapshot is already in place; caller handles restore on Err.
fn commit_ops(
    remaining: &[crate::report::Op],
    workspace_root: &Path,
    vault_root: &Path,
    config: &VaultConfig,
    manifest: &Manifest,
    meta: &WorkspaceMeta,
    total_ops: usize,
) -> Result<()> {
    let now = Utc::now();

    // Structural ops first so layout is stable for content ops.
    if let Err(e) = execute_structural_ops_slice(remaining, workspace_root, config) {
        eprintln!("{}", "Structural op failed.".red().bold());
        eprintln!("  {}", e);
        eprintln!();
        eprintln!(
            "Workspace restored to pre-commit state. Fix the issue and run `csnotes commit` again."
        );
        return Err(e);
    }

    // Content ops (stamp frontmatter on notes the AI wrote).
    execute_content_ops_slice(remaining, workspace_root, now, manifest)?;

    // Invariant suite against the full workspace.
    let audit = invariant_suite(
        workspace_root,
        &config.synthetic_dir,
        // Pass the full report so the suite sees all created notes.
        &SessionReport::load(workspace_root)?,
        manifest,
    )?;

    if !audit.is_clean() {
        eprintln!("{}", "Invariant violations — commit aborted.".red().bold());
        audit.print();
        eprintln!();
        eprintln!("Workspace restored to pre-commit state. Fix the violations and run `csnotes commit` again.");
        return Err(anyhow::anyhow!("invariant suite failed"));
    }

    audit.print();

    // Pre-merge vault snapshot (recovery if merge crashes mid-flight).
    let _vault_snapshot = take_snapshot(vault_root, &config.synthetic_dir, &meta.run_id)?;

    // Merge-back to vault.
    merge_back(workspace_root, vault_root, &config.synthetic_dir)?;

    // Vault snapshot successfully consumed — clean it up.
    // (If merge_back crashes the snapshot persists for `csnotes recover`.)
    let snapshot_path = vault_root.join(format!("_synthetic_snapshot_{}", meta.run_id));
    if snapshot_path.exists() {
        std::fs::remove_dir_all(&snapshot_path).ok();
    }

    // Extend committed_ops with what we just ran — this is the CLI's ground
    // truth; it is independent of any future report rewrites by the AI.
    let mut new_committed_ops = meta.committed_ops.clone();
    new_committed_ops.extend_from_slice(remaining);
    let new_committed = new_committed_ops.len().min(total_ops);
    WorkspaceMeta {
        vault_root: meta.vault_root.clone(),
        run_id: meta.run_id.clone(),
        committed_ops: new_committed_ops,
    }
    .save(workspace_root)?;

    let n = remaining.len();
    println!(
        "{} {} op{} committed ({}/{} total).",
        "commit:".green().bold(),
        n,
        if n == 1 { "" } else { "s" },
        new_committed,
        total_ops
    );
    if new_committed == total_ops {
        println!("  All ops committed. Exit the session to finalise.");
    } else {
        println!(
            "  {} op{} remaining. Continue working, then commit or exit.",
            total_ops - new_committed,
            if total_ops - new_committed == 1 {
                ""
            } else {
                "s"
            }
        );
    }

    Ok(())
}

/// Print what `csnotes commit` would do without executing anything.
fn dry_run_report(
    remaining: &[crate::report::Op],
    committed_count: usize,
    total_ops: usize,
    workspace_root: &Path,
) -> Result<()> {
    let tag = "dry-run".yellow().bold().to_string();
    println!(
        "{}  {} op{} to commit (ops {}-{} of {}).",
        tag,
        remaining.len(),
        if remaining.len() == 1 { "" } else { "s" },
        committed_count + 1,
        total_ops,
        total_ops
    );

    // Validate preconditions and report result.
    match precondition_pass_ops(remaining, workspace_root) {
        Ok(()) => println!("{}  preconditions: OK", tag),
        Err(e) => {
            println!("{}  preconditions: FAIL — {}", tag, e);
        }
    }

    for (i, op) in remaining.iter().enumerate() {
        println!(
            "{}  [{}/{}] {}",
            tag,
            committed_count + i + 1,
            total_ops,
            op.kind_str()
        );
    }

    Ok(())
}
