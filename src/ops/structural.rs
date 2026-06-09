#![allow(dead_code)]
/// Structural ops: rename_topic (Phase 1) and the rest (Phase 4).
///
/// These ops perform vault-global mutations that the AI only *requests* via
/// the session report.  The CLI executes them here, transactionally against
/// the workspace copy.
///
/// **Phase 1:** `rename_topic`
/// **Phase 4:** `move_atomic`, `promote_atomic`, `demote_topic`,
///              `merge_topics`, `split_topic`, `set_embed`
use std::path::Path;

use anyhow::{bail, Result};

use crate::report::*;

/// Execute a `rename_topic` op against the workspace.
/// Renames the topic folder and rewrites all inbound links across `_synthetic/`.
pub fn execute_rename_topic(
    _op: &RenameTopicOp,
    _workspace_root: &Path,
) -> Result<()> {
    // Phase 1 — not yet implemented.
    bail!("rename_topic is implemented in Phase 1");
}

/// Execute a `move_atomic` op against the workspace.
pub fn execute_move_atomic(
    _op: &MoveAtomicOp,
    _workspace_root: &Path,
) -> Result<()> {
    bail!("move_atomic is implemented in Phase 4");
}

pub fn execute_promote_atomic(
    _op: &PromoteAtomicOp,
    _workspace_root: &Path,
) -> Result<()> {
    bail!("promote_atomic is implemented in Phase 4");
}

pub fn execute_demote_topic(
    _op: &DemoteTopicOp,
    _workspace_root: &Path,
) -> Result<()> {
    bail!("demote_topic is implemented in Phase 4");
}

pub fn execute_merge_topics(
    _op: &MergeTopicsOp,
    _workspace_root: &Path,
) -> Result<()> {
    bail!("merge_topics is implemented in Phase 4");
}

pub fn execute_split_topic(
    _op: &SplitTopicOp,
    _workspace_root: &Path,
) -> Result<()> {
    bail!("split_topic is implemented in Phase 4");
}

pub fn execute_set_embed(
    _op: &SetEmbedOp,
    _workspace_root: &Path,
) -> Result<()> {
    bail!("set_embed is implemented in Phase 4");
}

// ── Link rewriter (used by all structural ops) ────────────────────────────────

/// Replace all occurrences of `[[old_target` → `[[new_target` and
/// `![[old_target` → `![[new_target` across all `.md` files under `root`.
///
/// Designed to accept an arbitrary `root` so Phase 4 can pass the full vault
/// root while Phase 1 passes only `_synthetic/`.
pub fn rewrite_links(root: &Path, old_target: &str, new_target: &str) -> Result<usize> {
    use walkdir::WalkDir;

    let old_wiki = format!("[[{}", old_target);
    let new_wiki = format!("[[{}", new_target);
    let old_embed = format!("![[{}", old_target);
    let new_embed = format!("![[{}", new_target);

    let mut files_changed = 0;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = std::fs::read_to_string(entry.path())?;
        if content.contains(&old_wiki) || content.contains(&old_embed) {
            let updated = content
                .replace(&old_embed, &new_embed) // embeds first (longer prefix)
                .replace(&old_wiki, &new_wiki);
            std::fs::write(entry.path(), updated)?;
            files_changed += 1;
        }
    }

    Ok(files_changed)
}
