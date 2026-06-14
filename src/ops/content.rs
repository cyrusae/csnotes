/// Execution of indexing ops: `create_note` and `update_note`.
///
/// These ops run in Phase 0 teardown (step 6).  They operate against the
/// workspace copy of `_synthetic/`, never the vault directly.
///
/// Contract:
/// - The AI has already written the note body into the workspace.
/// - The op declares what was done (provenance deltas, block IDs, embed
///   targets).
/// - The CLI merges the declared provenance into the frontmatter fence and
///   bumps `last_updated`.
///
/// Preconditions are checked separately in `audit::precondition_pass` before
/// any execution runs.
use std::path::Path;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

use crate::error::CsnotesError;
use crate::frontmatter::{write_frontmatter, NoteFrontmatter, NoteKind};
use crate::pathutil::safe_join;
use crate::report::{CreateNoteOp, UpdateNoteOp};

// ── create_note ───────────────────────────────────────────────────────────────

/// Execute a `create_note` op against the workspace.
///
/// Precondition (already checked): the file exists in the workspace (the AI
/// wrote it) and has no csnotes frontmatter yet, OR the file does not exist
/// and we need to create skeleton frontmatter.
///
/// In practice the AI writes the body and we stamp the frontmatter fence.
pub fn execute_create_note(
    op: &CreateNoteOp,
    workspace_root: &Path,
    now: DateTime<Utc>,
) -> Result<()> {
    let note_path = safe_join(workspace_root, &op.path)?;

    // Read the existing body (the AI wrote this).
    let body = if note_path.exists() {
        let raw = crate::frontmatter::read_note(&note_path)?;
        // Strip any accidental frontmatter the AI may have written (we own it).
        match crate::frontmatter::split_frontmatter(&raw) {
            Some((_fm, body)) => body.to_string(),
            None => raw,
        }
    } else {
        // Create a minimal skeleton — the AI should have written the file,
        // but we handle the missing-file case gracefully here; the
        // precondition pass will have already caught a genuine violation.
        bail!(CsnotesError::UpdateNotePathMissing(op.path.clone()));
    };

    // Build frontmatter from scratch.
    let mut fm = match op.kind {
        NoteKind::Atomic => {
            let block_id = op.block_id.as_deref().unwrap_or("").to_string();
            NoteFrontmatter::new_atomic(&op.topic, &op.title, block_id, now)
        }
        NoteKind::Index => NoteFrontmatter::new_index(&op.topic, &op.title, now),
    };

    fm.merge_provenance(&op.provenance, now);

    write_frontmatter(&note_path, &fm, &body)
}

// ── update_note ───────────────────────────────────────────────────────────────

/// Execute an `update_note` op against the workspace.
///
/// The AI has already edited the note body in place.  We open the file,
/// merge the declared provenance delta into the existing frontmatter, and
/// write it back.
pub fn execute_update_note(
    op: &UpdateNoteOp,
    workspace_root: &Path,
    now: DateTime<Utc>,
) -> Result<()> {
    let note_path = safe_join(workspace_root, &op.path)?;

    if !note_path.exists() {
        bail!(CsnotesError::UpdateNotePathMissing(op.path.clone()));
    }

    let content = crate::frontmatter::read_note(&note_path)?;
    let (yaml, body) = crate::frontmatter::split_frontmatter(&content)
        .ok_or_else(|| CsnotesError::NoFrontmatter(note_path.clone()))?;

    let mut fm: NoteFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| CsnotesError::FrontmatterParse {
            path: note_path.clone(),
            reason: e.to_string(),
        })?;

    fm.merge_provenance(&op.add_provenance, now);
    // Always touch `last_updated` — the body was changed even if provenance
    // was already recorded (e.g. a second pass to fix a typo).
    fm.touch(now);

    write_frontmatter(&note_path, &fm, body)
}
