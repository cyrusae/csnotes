/// Path normalization for multi-rename commit batches.
///
/// When a commit batch contains rename ops followed by content/other ops that
/// reference the old paths, execution fails because the rename already moved
/// the files.  This module provides:
///
/// - `normalize_batch`: rewrite a slice of ops so each op's path fields
///   reflect the post-rename state of preceding ops in the same batch.
/// - `patch_with_batch_renames`: apply the rename effects of a committed batch
///   to a mutable slice of future (uncommitted) ops.
use std::path::Path;

use crate::report::{
    CreateNoteOp, DemoteTopicOp, MergeTopicsOp, MoveAtomicOp, Op, PromoteAtomicOp, RenameAtomicOp,
    RenameTopicOp, SetEmbedOp, SplitTarget, SplitTopicOp, UpdateNoteOp,
};

// ── Rename effects ────────────────────────────────────────────────────────────

/// A single rename transformation produced by executing a structural op.
enum RenameEffect {
    /// All paths starting with `old` get the prefix replaced with `new`.
    Prefix { old: String, new: String },
    /// An exact path `old` becomes `new`.
    Exact { old: String, new: String },
    /// A topic-name field with value `old` becomes `new`.
    TopicName { old: String, new: String },
}

/// Record the rename effects that `op` produces when it executes.
/// `op` should already have its own paths normalized by preceding effects.
fn record_effects(op: &Op, effects: &mut Vec<RenameEffect>, synthetic_dir: &str) {
    match op {
        Op::RenameTopic(r) => {
            // The index file (named after the topic slug) is also renamed by
            // execute_rename_topic.  Add an exact rename before the prefix
            // rename so apply_path maps it to the new slug name, not just the
            // new folder (the prefix rename alone would give {to}/{from}.md).
            effects.push(RenameEffect::Exact {
                old: format!("{}/{}/{}.md", synthetic_dir, r.from, r.from),
                new: format!("{}/{}/{}.md", synthetic_dir, r.to, r.to),
            });
            effects.push(RenameEffect::Prefix {
                old: format!("{}/{}/", synthetic_dir, r.from),
                new: format!("{}/{}/", synthetic_dir, r.to),
            });
            effects.push(RenameEffect::TopicName {
                old: r.from.clone(),
                new: r.to.clone(),
            });
        }
        Op::RenameAtomic(r) => {
            let parent = Path::new(&r.path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(synthetic_dir);
            let new_path = format!("{}/{}.md", parent, r.new_slug);
            effects.push(RenameEffect::Exact {
                old: r.path.clone(),
                new: new_path,
            });
        }
        Op::MoveAtomic(r) => {
            let slug = Path::new(&r.from_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let new_path = format!("{}/{}/{}.md", synthetic_dir, r.to_topic, slug);
            effects.push(RenameEffect::Exact {
                old: r.from_path.clone(),
                new: new_path,
            });
        }
        Op::DemoteTopic(r) => {
            effects.push(RenameEffect::Prefix {
                old: format!("{}/{}/", synthetic_dir, r.from_topic),
                new: format!("{}/{}/", synthetic_dir, r.into_topic),
            });
            effects.push(RenameEffect::TopicName {
                old: r.from_topic.clone(),
                new: r.into_topic.clone(),
            });
        }
        Op::MergeTopics(r) => {
            for from in &r.from {
                effects.push(RenameEffect::Prefix {
                    old: format!("{}/{}/", synthetic_dir, from),
                    new: format!("{}/{}/", synthetic_dir, r.into),
                });
                effects.push(RenameEffect::TopicName {
                    old: from.clone(),
                    new: r.into.clone(),
                });
            }
        }
        // PromoteAtomic and SplitTopic produce complex rearrangements that
        // depend on which atomics land where; skip for now.
        _ => {}
    }
}

// ── Path / topic string helpers ───────────────────────────────────────────────

fn apply_path(path: &str, effects: &[RenameEffect]) -> String {
    let mut s = path.to_string();
    for effect in effects {
        match effect {
            RenameEffect::Prefix { old, new } => {
                if s.starts_with(old.as_str()) {
                    s = format!("{}{}", new, &s[old.len()..]);
                }
            }
            RenameEffect::Exact { old, new } => {
                if s == *old {
                    s = new.clone();
                }
            }
            RenameEffect::TopicName { .. } => {}
        }
    }
    s
}

fn apply_topic(topic: &str, effects: &[RenameEffect]) -> String {
    let mut s = topic.to_string();
    for effect in effects {
        if let RenameEffect::TopicName { old, new } = effect {
            if s == *old {
                s = new.clone();
            }
        }
    }
    s
}

// ── Op-level application ──────────────────────────────────────────────────────

fn apply_effects(op: &Op, effects: &[RenameEffect]) -> Op {
    if effects.is_empty() {
        return op.clone();
    }
    match op {
        Op::CreateNote(o) => Op::CreateNote(CreateNoteOp {
            path: apply_path(&o.path, effects),
            topic: apply_topic(&o.topic, effects),
            embed_in: o.embed_in.iter().map(|p| apply_path(p, effects)).collect(),
            ..o.clone()
        }),
        Op::UpdateNote(o) => Op::UpdateNote(UpdateNoteOp {
            path: apply_path(&o.path, effects),
            ..o.clone()
        }),
        Op::RenameAtomic(o) => Op::RenameAtomic(RenameAtomicOp {
            path: apply_path(&o.path, effects),
            ..o.clone()
        }),
        Op::RenameTopic(o) => Op::RenameTopic(RenameTopicOp {
            from: apply_topic(&o.from, effects),
            to: apply_topic(&o.to, effects),
            ..o.clone()
        }),
        Op::MoveAtomic(o) => Op::MoveAtomic(MoveAtomicOp {
            from_path: apply_path(&o.from_path, effects),
            to_topic: apply_topic(&o.to_topic, effects),
            ..o.clone()
        }),
        Op::PromoteAtomic(o) => Op::PromoteAtomic(PromoteAtomicOp {
            from_path: apply_path(&o.from_path, effects),
            to_topic: apply_topic(&o.to_topic, effects),
            ..o.clone()
        }),
        Op::DemoteTopic(o) => Op::DemoteTopic(DemoteTopicOp {
            from_topic: apply_topic(&o.from_topic, effects),
            into_topic: apply_topic(&o.into_topic, effects),
            ..o.clone()
        }),
        Op::MergeTopics(o) => Op::MergeTopics(MergeTopicsOp {
            from: o.from.iter().map(|t| apply_topic(t, effects)).collect(),
            into: apply_topic(&o.into, effects),
            ..o.clone()
        }),
        Op::SplitTopic(o) => Op::SplitTopic(SplitTopicOp {
            from: apply_topic(&o.from, effects),
            into: o
                .into
                .iter()
                .map(|st| SplitTarget {
                    topic: apply_topic(&st.topic, effects),
                    atomics: st.atomics.iter().map(|a| apply_path(a, effects)).collect(),
                })
                .collect(),
            ..o.clone()
        }),
        Op::SetEmbed(o) => Op::SetEmbed(SetEmbedOp {
            atomic_path: apply_path(&o.atomic_path, effects),
            index_path: apply_path(&o.index_path, effects),
            ..o.clone()
        }),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Normalize paths within a commit batch so each op sees post-rename paths
/// from all preceding ops in the slice.
///
/// Example: `[rename_topic java-fundamentals → java, rename_atomic
/// _synthetic/java-fundamentals/foo.md → bar]` — after normalization the
/// rename_atomic path becomes `_synthetic/java/foo.md`, which is where the
/// file actually lives after rename_topic executes.
pub fn normalize_batch(ops: &[Op], synthetic_dir: &str) -> Vec<Op> {
    let mut effects: Vec<RenameEffect> = Vec::new();
    let mut result = Vec::with_capacity(ops.len());
    for op in ops {
        let patched = apply_effects(op, &effects);
        record_effects(&patched, &mut effects, synthetic_dir);
        result.push(patched);
    }
    result
}

/// Apply the rename effects produced by `committed_batch` to each op in
/// `tail`.  Used after a commit to keep future ops' paths in sync with
/// what the committed renames actually did.
pub fn patch_with_batch_renames(tail: &mut [Op], committed_batch: &[Op], synthetic_dir: &str) {
    if tail.is_empty() {
        return;
    }
    let mut effects: Vec<RenameEffect> = Vec::new();
    for op in committed_batch {
        record_effects(op, &mut effects, synthetic_dir);
    }
    if effects.is_empty() {
        return;
    }
    for op in tail.iter_mut() {
        *op = apply_effects(op, &effects);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::{NoteKind, ProvenanceDelta};
    use crate::report::{CreateNoteOp, RenameAtomicOp, RenameTopicOp, UpdateNoteOp};

    fn make_provenance() -> ProvenanceDelta {
        ProvenanceDelta {
            sessions: vec![],
            sources: vec![],
        }
    }

    fn rename_topic(from: &str, to: &str) -> Op {
        Op::RenameTopic(RenameTopicOp {
            from: from.into(),
            to: to.into(),
            reason: String::new(),
        })
    }

    fn rename_atomic(path: &str, new_slug: &str) -> Op {
        Op::RenameAtomic(RenameAtomicOp {
            path: path.into(),
            new_slug: new_slug.into(),
            new_title: String::new(),
            reason: String::new(),
        })
    }

    fn update_note(path: &str) -> Op {
        Op::UpdateNote(UpdateNoteOp {
            path: path.into(),
            add_provenance: make_provenance(),
            sections: vec![],
            change_summary: String::new(),
        })
    }

    fn create_note(path: &str, topic: &str) -> Op {
        Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: path.into(),
            title: String::new(),
            topic: topic.into(),
            block_id: None,
            embed_in: vec![format!("_synthetic/{}/{}.md", topic, topic)],
            provenance: make_provenance(),
            change_summary: String::new(),
        })
    }

    // normalize_batch ─────────────────────────────────────────────────────────

    #[test]
    fn normalize_batch_noop_when_no_renames() {
        let ops = vec![update_note("_synthetic/java/foo.md")];
        let result = normalize_batch(&ops, "_synthetic");
        assert_eq!(
            match &result[0] {
                Op::UpdateNote(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/java/foo.md"
        );
    }

    #[test]
    fn normalize_batch_patches_rename_atomic_after_rename_topic() {
        let ops = vec![
            rename_topic("java-fundamentals", "java"),
            rename_atomic("_synthetic/java-fundamentals/foo.md", "bar"),
        ];
        let result = normalize_batch(&ops, "_synthetic");
        // rename_atomic path should use the post-rename_topic folder
        assert_eq!(
            match &result[1] {
                Op::RenameAtomic(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/java/foo.md"
        );
    }

    #[test]
    fn normalize_batch_patches_update_note_after_rename_topic() {
        let ops = vec![
            rename_topic("old-topic", "new-topic"),
            update_note("_synthetic/old-topic/note.md"),
        ];
        let result = normalize_batch(&ops, "_synthetic");
        assert_eq!(
            match &result[1] {
                Op::UpdateNote(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/new-topic/note.md"
        );
    }

    #[test]
    fn normalize_batch_patches_create_note_path_and_topic_and_embed_in() {
        let ops = vec![
            rename_topic("old", "new"),
            create_note("_synthetic/old/note.md", "old"),
        ];
        let result = normalize_batch(&ops, "_synthetic");
        match &result[1] {
            Op::CreateNote(o) => {
                assert_eq!(o.path, "_synthetic/new/note.md");
                assert_eq!(o.topic, "new");
                assert_eq!(o.embed_in[0], "_synthetic/new/new.md");
            }
            _ => panic!("expected CreateNote"),
        }
    }

    #[test]
    fn normalize_batch_chains_rename_topic_then_rename_atomic_effect() {
        // After rename_topic A→B and rename_atomic _synthetic/B/foo→bar,
        // an update_note for _synthetic/A/foo.md should end up as _synthetic/B/bar.md
        let ops = vec![
            rename_topic("a", "b"),
            rename_atomic("_synthetic/a/foo.md", "bar"),
            update_note("_synthetic/a/foo.md"),
        ];
        let result = normalize_batch(&ops, "_synthetic");
        assert_eq!(
            match &result[2] {
                Op::UpdateNote(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/b/bar.md"
        );
    }

    // patch_with_batch_renames ─────────────────────────────────────────────────

    #[test]
    fn patch_tail_updates_update_note_after_rename_topic_commit() {
        let committed = vec![rename_topic("java-fundamentals", "java")];
        let mut tail = vec![update_note("_synthetic/java-fundamentals/foo.md")];
        patch_with_batch_renames(&mut tail, &committed, "_synthetic");
        assert_eq!(
            match &tail[0] {
                Op::UpdateNote(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/java/foo.md"
        );
    }

    #[test]
    fn patch_tail_noop_when_no_structural_renames() {
        let committed = vec![update_note("_synthetic/java/foo.md")];
        let mut tail = vec![update_note("_synthetic/java/bar.md")];
        patch_with_batch_renames(&mut tail, &committed, "_synthetic");
        assert_eq!(
            match &tail[0] {
                Op::UpdateNote(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/java/bar.md"
        );
    }

    #[test]
    fn patch_tail_updates_after_rename_atomic_commit() {
        let committed = vec![rename_atomic("_synthetic/java/old.md", "new-slug")];
        let mut tail = vec![update_note("_synthetic/java/old.md")];
        patch_with_batch_renames(&mut tail, &committed, "_synthetic");
        assert_eq!(
            match &tail[0] {
                Op::UpdateNote(o) => &o.path,
                _ => panic!(),
            },
            "_synthetic/java/new-slug.md"
        );
    }
}
