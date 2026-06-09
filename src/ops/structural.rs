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

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::report::*;

/// Execute a `rename_topic` op against the workspace.
///
/// Steps:
/// 1. Rename `_synthetic/{from}/` → `_synthetic/{to}/`
/// 2. Update the `topic` frontmatter field in every note in the renamed folder
/// 3. Rewrite any path-qualified wikilinks (`[[{from}/…]]`) across `_synthetic/`
pub fn execute_rename_topic(
    op: &RenameTopicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_dir = synthetic_root.join(&op.from);
    let to_dir = synthetic_root.join(&op.to);

    if !from_dir.exists() {
        bail!(
            "rename_topic: source topic folder '{}' does not exist in workspace",
            op.from
        );
    }
    if to_dir.exists() {
        bail!(
            "rename_topic: destination topic folder '{}' already exists \
             (use merge_topics to combine topics)",
            op.to
        );
    }

    // 1. Rename the folder.
    std::fs::rename(&from_dir, &to_dir).with_context(|| {
        format!(
            "rename_topic: renaming '{}' → '{}'",
            from_dir.display(),
            to_dir.display()
        )
    })?;

    // 2. Update `topic` in frontmatter for every note now in the new folder.
    for entry in WalkDir::new(&to_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        crate::frontmatter::update_frontmatter(entry.path(), |fm| {
            if fm.topic == op.from {
                fm.topic = op.to.clone();
            }
        })?;
    }

    // 3. Rewrite any path-qualified wikilinks across all of `_synthetic/`.
    //    Pure stem links (e.g. `[[polymorphism]]`) are unaffected because
    //    Obsidian resolves them by filename, not by folder.
    //    Path-qualified links (e.g. `[[inheritance/polymorphism]]`) need
    //    the folder prefix updated.
    rewrite_links(
        &synthetic_root,
        &format!("{}/", op.from),
        &format!("{}/", op.to),
    )?;

    Ok(())
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_note(dir: &std::path::Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn minimal_note(topic: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: {topic}\ntitle: Test\n\
             block_id: test-id\nembeds: ~\ncontributing_sessions: []\n\
             contributing_sources: []\ncreated: \"2026-01-01T00:00:00Z\"\n\
             last_updated: \"2026-01-01T00:00:00Z\"\n---\n\nBody text.\n\n^test-id\n"
        )
    }

    #[test]
    fn rename_topic_renames_folder_and_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "inheritance/index.md", &minimal_note("inheritance"));
        write_note(&synthetic, "inheritance/poly.md", &minimal_note("inheritance"));

        let op = RenameTopicOp {
            from: "inheritance".to_string(),
            to: "oop-basics".to_string(),
            reason: "clearer name".to_string(),
        };
        execute_rename_topic(&op, tmp.path(), "_synthetic").unwrap();

        // Old folder gone, new folder present
        assert!(!synthetic.join("inheritance").exists());
        assert!(synthetic.join("oop-basics").exists());
        assert!(synthetic.join("oop-basics/index.md").exists());
        assert!(synthetic.join("oop-basics/poly.md").exists());

        // Frontmatter topic field updated
        let content = std::fs::read_to_string(synthetic.join("oop-basics/index.md")).unwrap();
        assert!(content.contains("topic: oop-basics"), "frontmatter topic not updated");
        assert!(!content.contains("topic: inheritance"), "old topic name still present");
    }

    #[test]
    fn rename_topic_rewrites_path_qualified_links() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "inheritance/index.md", &minimal_note("inheritance"));
        // Another note that uses a path-qualified link to the old topic
        write_note(
            &synthetic,
            "other/other.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: other\ntitle: Other\n\
             embeds: []\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nSee [[inheritance/poly]] and ![[inheritance/poly#^test-id]].\n",
        );

        let op = RenameTopicOp {
            from: "inheritance".to_string(),
            to: "oop-basics".to_string(),
            reason: "test".to_string(),
        };
        execute_rename_topic(&op, tmp.path(), "_synthetic").unwrap();

        let content = std::fs::read_to_string(synthetic.join("other/other.md")).unwrap();
        assert!(content.contains("[[oop-basics/poly]]"));
        assert!(content.contains("![[oop-basics/poly#^test-id]]"));
        assert!(!content.contains("[[inheritance/"));
    }

    #[test]
    fn rename_topic_fails_if_source_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic")).unwrap();
        let op = RenameTopicOp {
            from: "nonexistent".to_string(),
            to: "new-name".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_topic(&op, tmp.path(), "_synthetic").is_err());
    }

    #[test]
    fn rename_topic_fails_if_dest_exists() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "old-name/note.md", &minimal_note("old-name"));
        write_note(&synthetic, "new-name/note.md", &minimal_note("new-name"));

        let op = RenameTopicOp {
            from: "old-name".to_string(),
            to: "new-name".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_topic(&op, tmp.path(), "_synthetic").is_err());
    }

    #[test]
    fn rewrite_links_only_touches_matching_files() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a.md");
        let b = tmp.path().join("b.md");
        std::fs::write(&a, "See [[old/thing]] and ![[old/embed]].\n").unwrap();
        std::fs::write(&b, "No matching links here.\n").unwrap();

        let changed = rewrite_links(tmp.path(), "old/", "new/").unwrap();
        assert_eq!(changed, 1);
        let content_a = std::fs::read_to_string(&a).unwrap();
        assert!(content_a.contains("[[new/thing]]"));
        assert!(content_a.contains("![[new/embed]]"));
        let content_b = std::fs::read_to_string(&b).unwrap();
        assert_eq!(content_b, "No matching links here.\n");
    }
}
