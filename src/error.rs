#![allow(dead_code)]
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CsnotesError {
    // ── Vault / config ────────────────────────────────────────────────────────
    #[error("no csnotes.toml config file found (searched upward from {0})")]
    VaultNotFound(PathBuf),

    #[error("invalid filename_format '{format}': {reason}")]
    InvalidFilenameFormat { format: String, reason: String },

    #[error("config key '{key}' is invalid or read-only")]
    InvalidConfigKey { key: String },

    // ── Manifest ──────────────────────────────────────────────────────────────
    #[error("manifest is corrupt or missing: {0}")]
    ManifestCorrupt(String),

    #[error("session '{0}' not found in manifest")]
    SessionNotFound(String),

    #[error("source '{0}' not found in manifest")]
    SourceNotFound(String),

    #[error("topic '{0}' not found in manifest")]
    TopicNotFound(String),

    // ── Frontmatter ───────────────────────────────────────────────────────────
    #[error("no frontmatter found in {0}")]
    NoFrontmatter(PathBuf),

    #[error("frontmatter parse error in {path}: {reason}")]
    FrontmatterParse { path: PathBuf, reason: String },

    #[error("frontmatter schema version {got} unsupported (expected {expected}) in {path}")]
    FrontmatterSchemaMismatch {
        path: PathBuf,
        got: u32,
        expected: u32,
    },

    // ── Session report ────────────────────────────────────────────────────────
    #[error("no session report found at {0}")]
    ReportMissing(PathBuf),

    #[error("session report parse error: {0}")]
    ReportParse(String),

    #[error("run_id mismatch: manifest has '{manifest}', report has '{report}'")]
    RunIdMismatch { manifest: String, report: String },

    // ── Precondition failures (per-op, named for clear error messages) ────────
    #[error("create_note precondition failed: '{0}' already exists in workspace")]
    CreateNotePathExists(String),

    #[error("create_note precondition failed: '{0}' not found in workspace (write the file before declaring the op)")]
    CreateNotePathMissing(String),

    #[error("update_note precondition failed: '{0}' not found in workspace")]
    UpdateNotePathMissing(String),

    #[error("block_id '{id}' declared in op but anchor '^{id}' not found in body of '{path}'")]
    BlockIdAnchorMissing { id: String, path: String },

    #[error("embed_in target '{0}' declared but file not found in workspace")]
    EmbedInTargetMissing(String),

    #[error("rename_topic precondition failed: source topic '{0}' not found in workspace")]
    RenameTopicSourceMissing(String),

    #[error("rename_topic precondition failed: destination topic '{0}' already exists")]
    RenameTopicDestExists(String),

    #[error("rename_atomic precondition failed: source note '{0}' not found in workspace")]
    RenameAtomicSourceMissing(String),

    #[error("rename_atomic precondition failed: '{slug}' already exists in topic '{topic}'")]
    RenameAtomicDestExists { slug: String, topic: String },

    #[error("move_atomic precondition failed: source note '{0}' not found in workspace")]
    MoveAtomicSourceMissing(String),

    #[error("move_atomic precondition failed: target topic '{0}' does not exist (use promote_atomic for new topics)")]
    MoveAtomicTargetMissing(String),

    #[error("move_atomic precondition failed: '{slug}' already exists in target topic '{topic}'")]
    MoveAtomicDestExists { slug: String, topic: String },

    #[error("promote_atomic precondition failed: source note '{0}' not found in workspace")]
    PromoteAtomicSourceMissing(String),

    #[error("promote_atomic precondition failed: target topic '{0}' already exists (use move_atomic for existing topics)")]
    PromoteAtomicTargetExists(String),

    #[error("demote_topic precondition failed: source topic '{0}' does not exist")]
    DemoteTopicSourceMissing(String),

    #[error("demote_topic precondition failed: target topic '{0}' does not exist")]
    DemoteTopicTargetMissing(String),

    #[error("demote_topic precondition failed: source and target are the same topic '{0}'")]
    DemoteTopicSameTarget(String),

    #[error("merge_topics precondition failed: source topic '{0}' does not exist")]
    MergeTopicsSourceMissing(String),

    #[error("merge_topics precondition failed: filename conflict — '{0}' would collide in the merge target")]
    MergeTopicsFileConflict(String),

    #[error("split_topic precondition failed: source topic '{0}' does not exist")]
    SplitTopicSourceMissing(String),

    #[error("split_topic precondition failed: '{slug}' not found in source topic '{topic}'")]
    SplitTopicAtomicMissing { slug: String, topic: String },

    #[error("split_topic precondition failed: '{slug}' already exists in target topic '{topic}'")]
    SplitTopicDestExists { slug: String, topic: String },

    #[error("set_embed precondition failed: index note '{0}' not found in workspace")]
    SetEmbedIndexMissing(String),

    #[error("set_embed precondition failed: atomic note '{0}' not found in workspace")]
    SetEmbedAtomicMissing(String),

    #[error("set_embed precondition failed: atomic '{0}' has no block_id in frontmatter")]
    SetEmbedAtomicNoBlockId(String),

    // ── Invariant violations ──────────────────────────────────────────────────
    #[error("block ID '^{id}' is not vault-unique; also defined in '{other}'")]
    BlockIdCollision { id: String, other: String },

    #[error("wikilink [[{target}]] in '{in_file}' does not resolve to any note")]
    BrokenWikilink { target: String, in_file: String },

    #[error("embed ![[{target}]] in '{in_file}' does not resolve")]
    BrokenEmbed { target: String, in_file: String },

    // ── Workspace ─────────────────────────────────────────────────────────────
    #[error("workspace already exists at {0}; use `csnotes recover` to resume or discard")]
    WorkspaceExists(PathBuf),

    #[error("could not determine temporary directory for workspace")]
    NoTempDir,

    // ── Backend ───────────────────────────────────────────────────────────────
    #[error("AI backend '{0}' exited with error")]
    BackendFailed(String),

    #[error("backend '{0}' is not yet implemented")]
    BackendUnimplemented(String),

    // ── Recovery ──────────────────────────────────────────────────────────────
    #[error("nothing to recover: no session_in_progress in manifest")]
    NothingToRecover,

    // ── I/O passthrough ───────────────────────────────────────────────────────
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
