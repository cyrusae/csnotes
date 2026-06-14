#![allow(dead_code)]
/// Session report types and parsing.
///
/// The AI writes `_session_report.json` into the workspace at wrap-up.
/// This is the sole channel by which session work becomes durable — the CLI
/// validates it, executes the declared ops, and then discards the workspace.
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CsnotesError;
use crate::frontmatter::{NoteKind, ProvenanceDelta};

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const REPORT_FILENAME: &str = "_session_report.json";

// ── Top-level ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionReport {
    pub csnotes_report_schema: u32,
    pub run_id: String,
    pub backend: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub scope: ReportScope,
    pub operations: Vec<Op>,
    pub review_flags: Vec<ReviewFlag>,
}

impl SessionReport {
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let path = workspace_root.join(REPORT_FILENAME);
        if !path.exists() {
            return Err(CsnotesError::ReportMissing(path).into());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let report: SessionReport =
            serde_json::from_str(&content).map_err(|e| CsnotesError::ReportParse(e.to_string()))?;
        Ok(report)
    }
}

// ── Scope ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReportScope {
    pub kind: ScopeKind,
    #[serde(default)]
    pub sessions: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    /// Present when `process --topic` is used (study session with no new input).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Session,
    Source,
    Topic,
    Mixed,
}

// ── Operations ────────────────────────────────────────────────────────────────

/// All operations the AI can declare.
///
/// **Indexing ops** (Phase 0): `CreateNote`, `UpdateNote` — the AI writes note
/// bodies in place and declares these to register what it did.
///
/// **Structural ops** (Phase 1+): the AI *requests* these; the CLI executes
/// the vault-global mutations.  The AI never touches folder layout or links.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    // ── Indexing (Phase 0) ─────────────────────────────────────────────────
    CreateNote(CreateNoteOp),
    UpdateNote(UpdateNoteOp),

    // ── Structural (Phase 1+) ──────────────────────────────────────────────
    RenameTopic(RenameTopicOp),
    RenameAtomic(RenameAtomicOp),
    MoveAtomic(MoveAtomicOp),
    PromoteAtomic(PromoteAtomicOp),
    DemoteTopic(DemoteTopicOp),
    MergeTopics(MergeTopicsOp),
    SplitTopic(SplitTopicOp),
    SetEmbed(SetEmbedOp),
}

impl Op {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Op::CreateNote(_) => "create_note",
            Op::UpdateNote(_) => "update_note",
            Op::RenameTopic(_) => "rename_topic",
            Op::RenameAtomic(_) => "rename_atomic",
            Op::MoveAtomic(_) => "move_atomic",
            Op::PromoteAtomic(_) => "promote_atomic",
            Op::DemoteTopic(_) => "demote_topic",
            Op::MergeTopics(_) => "merge_topics",
            Op::SplitTopic(_) => "split_topic",
            Op::SetEmbed(_) => "set_embed",
        }
    }

    pub fn is_indexing(&self) -> bool {
        matches!(self, Op::CreateNote(_) | Op::UpdateNote(_))
    }

    pub fn is_structural(&self) -> bool {
        !self.is_indexing()
    }
}

// ── Indexing op structs ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreateNoteOp {
    pub kind: NoteKind,
    /// Workspace-relative path (e.g. `_synthetic/inheritance/polymorphism.md`).
    pub path: String,
    pub title: String,
    pub topic: String,
    /// Required for `kind: atomic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    /// Index notes this atomic should be embedded in.
    #[serde(default)]
    pub embed_in: Vec<String>,
    pub provenance: ProvenanceDelta,
    pub change_summary: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UpdateNoteOp {
    pub path: String,
    pub add_provenance: ProvenanceDelta,
    /// Sections the AI touched (informational; used by `diff`).
    #[serde(default)]
    pub sections: Vec<String>,
    pub change_summary: String,
}

// ── Structural op structs ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RenameTopicOp {
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RenameAtomicOp {
    /// Workspace-relative path to the note (e.g. `_synthetic/cs/old-slug.md`).
    pub path: String,
    pub new_slug: String,
    pub new_title: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MoveAtomicOp {
    pub from_path: String,
    pub to_topic: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PromoteAtomicOp {
    pub from_path: String,
    pub to_topic: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoteTopicOp {
    pub from_topic: String,
    pub into_topic: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MergeTopicsOp {
    pub from: Vec<String>,
    pub into: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SplitTopicOp {
    pub from: String,
    pub into: Vec<SplitTarget>,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SplitTarget {
    pub topic: String,
    pub atomics: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetEmbedOp {
    pub atomic_path: String,
    pub index_path: String,
    pub present: bool,
}

// ── Review flags ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReviewFlag {
    pub kind: FlagKind,
    /// Note path this flag is about (not all flags are path-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Block anchor within the note (e.g. `^static-defn`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    /// Likely transcription error — actionable, persists, nags in `status`.
    PossibleMisread,
    /// Relationship label or other assertion needs user confirmation —
    /// actionable, persists, nags.
    NeedsConfirmation,
    /// Open question the user should revisit — thread, surfaces via briefing
    /// re-injection, does not nag.
    UnresolvedQuestion,
    /// Informational design choice — auto-resolved, logged only.
    Ambiguity,
}

impl FlagKind {
    pub fn is_actionable(self) -> bool {
        matches!(
            self,
            FlagKind::PossibleMisread | FlagKind::NeedsConfirmation
        )
    }

    pub fn is_thread(self) -> bool {
        matches!(self, FlagKind::UnresolvedQuestion)
    }

    pub fn tier_label(self) -> &'static str {
        match self {
            FlagKind::PossibleMisread => "actionable",
            FlagKind::NeedsConfirmation => "actionable",
            FlagKind::UnresolvedQuestion => "thread",
            FlagKind::Ambiguity => "changelog",
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_REPORT: &str = r#"{
        "csnotes_report_schema": 1,
        "run_id": "9f2c-test",
        "backend": "mock",
        "started_at": "2027-03-12T08:55:00Z",
        "completed_at": "2027-03-12T09:30:00Z",
        "scope": { "kind": "session", "sessions": ["CS601-03-12"], "sources": [] },
        "operations": [
            {
                "op": "update_note",
                "path": "_synthetic/inheritance/inheritance.md",
                "add_provenance": {
                    "sessions": [{"course": "CS601", "date": "2027-03-12", "relationship": "reframed"}],
                    "sources": []
                },
                "sections": ["Current Understanding", "Conceptual History"],
                "change_summary": "Reframed inheritance as interface-sharing."
            }
        ],
        "review_flags": [
            {
                "kind": "needs_confirmation",
                "path": "_synthetic/inheritance/inheritance.md",
                "message": "Called this 'reframed' not 'contradicted' — confirm?"
            }
        ]
    }"#;

    #[test]
    fn parse_sample_report() {
        let report: SessionReport = serde_json::from_str(SAMPLE_REPORT).unwrap();
        assert_eq!(report.run_id, "9f2c-test");
        assert_eq!(report.operations.len(), 1);
        assert!(matches!(report.operations[0], Op::UpdateNote(_)));
        assert_eq!(report.review_flags.len(), 1);
        assert_eq!(report.review_flags[0].kind, FlagKind::NeedsConfirmation);
    }

    #[test]
    fn create_note_op_roundtrip() {
        let op = Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: "_synthetic/inheritance/polymorphism.md".into(),
            title: "Polymorphism".into(),
            topic: "inheritance".into(),
            block_id: Some("polymorphism-core".into()),
            embed_in: vec!["_synthetic/inheritance/inheritance.md".into()],
            provenance: ProvenanceDelta::default(),
            change_summary: "New atomic.".into(),
        });
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains(r#""op":"create_note""#));
        let back: Op = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Op::CreateNote(_)));
    }

    // ── Op::kind_str / is_indexing / is_structural ────────────────────────────

    fn rename_op() -> Op {
        Op::RenameTopic(RenameTopicOp {
            from: "a".into(),
            to: "b".into(),
            reason: "test".into(),
        })
    }

    fn update_op() -> Op {
        Op::UpdateNote(UpdateNoteOp {
            path: "p.md".into(),
            add_provenance: ProvenanceDelta::default(),
            sections: vec![],
            change_summary: "test".into(),
        })
    }

    fn create_op() -> Op {
        Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: "p.md".into(),
            title: "T".into(),
            topic: "t".into(),
            block_id: None,
            embed_in: vec![],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })
    }

    #[test]
    fn op_kind_str_returns_correct_names() {
        assert_eq!(create_op().kind_str(), "create_note");
        assert_eq!(update_op().kind_str(), "update_note");
        assert_eq!(rename_op().kind_str(), "rename_topic");
    }

    #[test]
    fn op_is_indexing_true_for_create_and_update() {
        assert!(create_op().is_indexing());
        assert!(update_op().is_indexing());
        assert!(!rename_op().is_indexing());
    }

    #[test]
    fn op_is_structural_inverse_of_is_indexing() {
        assert!(!create_op().is_structural());
        assert!(!update_op().is_structural());
        assert!(rename_op().is_structural());
    }

    // ── FlagKind methods ──────────────────────────────────────────────────────

    #[test]
    fn flag_kind_is_actionable_correct_variants() {
        assert!(FlagKind::PossibleMisread.is_actionable());
        assert!(FlagKind::NeedsConfirmation.is_actionable());
        assert!(!FlagKind::UnresolvedQuestion.is_actionable());
        assert!(!FlagKind::Ambiguity.is_actionable());
    }

    #[test]
    fn flag_kind_is_thread_only_unresolved_question() {
        assert!(FlagKind::UnresolvedQuestion.is_thread());
        assert!(!FlagKind::PossibleMisread.is_thread());
        assert!(!FlagKind::NeedsConfirmation.is_thread());
        assert!(!FlagKind::Ambiguity.is_thread());
    }

    #[test]
    fn flag_kind_tier_label_matches_variant() {
        assert_eq!(FlagKind::PossibleMisread.tier_label(), "actionable");
        assert_eq!(FlagKind::NeedsConfirmation.tier_label(), "actionable");
        assert_eq!(FlagKind::UnresolvedQuestion.tier_label(), "thread");
        assert_eq!(FlagKind::Ambiguity.tier_label(), "changelog");
    }
}
