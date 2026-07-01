use chrono::{DateTime, NaiveDate, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{AiBackend, SkillVariant, SnapshotMode};

pub const MANIFEST_VERSION: &str = "2";
pub const MANIFEST_FILENAME: &str = "csnotes.json";

// ── Top-level ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    pub version: String,
    pub vault_root: PathBuf,
    pub config: ManifestConfig,
    pub sessions: IndexMap<String, SessionEntry>,
    pub sources: IndexMap<String, SourceEntry>,
    pub topics: IndexMap<String, TopicEntry>,
    pub session_in_progress: Option<InProgressRecord>,
    pub flags_path: String,
}

impl Manifest {
    pub fn empty(vault_root: PathBuf, config: ManifestConfig) -> Self {
        Manifest {
            version: MANIFEST_VERSION.to_string(),
            vault_root,
            config,
            sessions: IndexMap::new(),
            sources: IndexMap::new(),
            topics: IndexMap::new(),
            session_in_progress: None,
            flags_path: "_generated/flags.json".to_string(),
        }
    }

    pub fn load(vault_root: &Path) -> Result<Self> {
        let path = vault_root.join(MANIFEST_FILENAME);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let manifest: Manifest = serde_json::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(manifest)
    }

    /// Load the manifest, or create and save an empty one if it doesn't exist.
    /// Used by `reconcile` so that the first run in a new vault bootstraps
    /// `csnotes.json` without requiring a separate `init` step.
    pub fn load_or_create(
        vault_root: &Path,
        vault_config: &crate::config::VaultConfig,
    ) -> Result<Self> {
        let path = vault_root.join(MANIFEST_FILENAME);
        if path.exists() {
            return Self::load(vault_root);
        }
        let manifest = Manifest::empty(
            vault_root.to_path_buf(),
            ManifestConfig::from_vault_config(vault_config),
        );
        manifest.save(vault_root)?;
        println!("Created {}", path.display());
        Ok(manifest)
    }

    pub fn save(&self, vault_root: &Path) -> Result<()> {
        let path = vault_root.join(MANIFEST_FILENAME);
        let content = serde_json::to_string_pretty(self).context("serializing manifest")?;
        std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Path to the flags store (absolute).
    pub fn flags_path_absolute(&self) -> PathBuf {
        self.vault_root.join(&self.flags_path)
    }

    /// Path to the last session report copy (written during merge-back).
    pub fn last_report_path(&self) -> PathBuf {
        self.vault_root
            .join(&self.config.generated_dir)
            .join("last_report.json")
    }

    /// Path for storing the report from a specific session.
    pub fn session_report_path(&self, session_id: &str) -> PathBuf {
        self.vault_root
            .join(&self.config.generated_dir)
            .join("reports")
            .join(format!("{}.json", session_id))
    }
}

// ── ManifestConfig ────────────────────────────────────────────────────────────

/// Snapshot of the VaultConfig fields relevant to manifest operations.
/// Stored in the manifest so that operations like `reconcile` can recover
/// the `filename_format` that was in effect when each session was registered,
/// even across a mid-program config change.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManifestConfig {
    pub raw_dir: String,
    pub recordings_dir: String,
    pub artifacts_dir: String,
    pub sources_dir: String,
    pub synthetic_dir: String,
    pub generated_dir: String,
    pub filename_format: String,
    pub default_backend: AiBackend,
    pub skill_variant: SkillVariant,
    pub snapshot_mode: SnapshotMode,
}

impl ManifestConfig {
    pub fn from_vault_config(cfg: &crate::config::VaultConfig) -> Self {
        ManifestConfig {
            raw_dir: cfg.raw_dir.clone(),
            recordings_dir: cfg.recordings_dir.clone(),
            artifacts_dir: cfg.artifacts_dir.clone(),
            sources_dir: cfg.sources_dir.clone(),
            synthetic_dir: cfg.synthetic_dir.clone(),
            generated_dir: cfg.generated_dir.clone(),
            filename_format: cfg.filename_format.clone(),
            default_backend: cfg.default_backend,
            skill_variant: cfg.skill_variant,
            snapshot_mode: cfg.snapshot_mode,
        }
    }
}

// ── Session entries ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionEntry {
    pub date: NaiveDate,
    pub course: String,
    /// The filename_format that was active when this session was registered.
    pub filename_format: String,
    pub raw_note: String,
    pub recording_exports: Vec<RecordingExport>,
    pub artifacts: Vec<ArtifactEntry>,
    pub recording_missing: bool,
    pub status: SessionStatus,
    pub processed_at: Option<DateTime<Utc>>,
    pub topics_updated: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Unprocessed,
    InProgress,
    Processed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecordingExport {
    pub path: String,
    pub kind: RecordingKind,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingKind {
    Transcript,
    Summary,
    Mindmap,
    /// Anonymous recording export (e.g., `-a`, `-b`).
    Anonymous,
    /// User-defined qualifier from `recording_qualifiers` config.
    Custom,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ArtifactEntry {
    pub path: String,
    pub kind: ArtifactKind,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Slides,
    Code,
    Other,
}

// ── Source entries ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SourceEntry {
    pub path: String,
    pub kind: SourceKind,
    pub status: SourceStatus,
    pub last_processed_at: Option<DateTime<Utc>>,
    /// Heading scheme derived by the comrak pass (Phase 1).
    /// e.g. ["chapter", "section", "subsection"]
    pub heading_scheme: Vec<String>,
    pub topics_updated: Vec<String>,
    /// Short description from the source file's YAML frontmatter `summary:` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Topic tags from the source file's YAML frontmatter `tags:` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Courses this source is associated with, from frontmatter `courses:` field.
    /// Empty = available in all session workspaces (untagged).
    /// Non-empty (Textbook only) = included only when the session's course matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub courses: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Verbatim third-party text (Zybooks, ripped textbook chapters, slides).
    /// Treat as reference material; key off the student's stated understanding.
    Textbook,
    /// Student's own notes on a paper textbook or reading.
    /// Treat as primary input alongside lecture notes.
    PersonalNotes,
    AiConversation,
    Paper,
    AssignmentFeedback,
    Other,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Textbook => "textbook",
            Self::PersonalNotes => "personal_notes",
            Self::AiConversation => "ai_conversation",
            Self::Paper => "paper",
            Self::AssignmentFeedback => "assignment_feedback",
            Self::Other => "other",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    /// Registered but not yet read — excluded from workspace assembly.
    /// Accepts "staged", "incomplete", or "unread" in YAML.
    #[serde(alias = "incomplete", alias = "unread")]
    Staged,
    Unprocessed,
    InProgress,
    Processed,
}

// ── Topic entries ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopicEntry {
    pub index_note: String,
    pub atomic_notes: Vec<String>,
    pub contributing_sessions: Vec<SessionContrib>,
    pub contributing_sources: Vec<SourceContrib>,
    /// Sessions that postdate `last_updated` and whose `topics_updated`
    /// includes this topic — i.e., unprocessed work that touches this topic.
    pub pending_sessions: Vec<String>,
    pub last_updated: DateTime<Utc>,
    pub open_flags: u32,
    pub source_types: Vec<String>,
}

// ── Provenance ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SessionContrib {
    pub course: String,
    pub date: NaiveDate,
    pub relationship: Relationship,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SourceContrib {
    pub source_id: String,
    pub relationship: Relationship,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Relationship {
    Introduced,
    Extended,
    Reframed,
    Contradicted,
    Nuanced,
}

// ── In-progress record ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InProgressRecord {
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub workspace_path: PathBuf,
    /// "synthesizing" | "merging"
    pub phase: String,
    pub error: Option<String>,
    /// Which backend launched this session (absent in old manifests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<AiBackend>,
    /// Which skill variant was used (absent in old manifests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_variant: Option<SkillVariant>,
}

// ── ManifestLock ─────────────────────────────────────────────────────────────

pub const LOCK_FILENAME: &str = ".csnotes.lock";

/// Advisory exclusive lock over the manifest file.
///
/// Acquire with `ManifestLock::acquire(vault_root)` before any load→modify→save
/// sequence. The lock is released automatically when this value is dropped.
/// On non-Unix platforms the lock is a no-op (the struct still exists so call
/// sites compile unchanged).
pub struct ManifestLock {
    #[cfg(unix)]
    _file: std::fs::File,
}

impl ManifestLock {
    pub fn acquire(vault_root: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let lock_path = vault_root.join(LOCK_FILENAME);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&lock_path)
                .with_context(|| format!("opening lock file {}", lock_path.display()))?;
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if ret != 0 {
                anyhow::bail!(
                    "could not acquire manifest lock (another csnotes process may be running): {}",
                    std::io::Error::last_os_error()
                );
            }
            Ok(ManifestLock { _file: file })
        }
        #[cfg(not(unix))]
        {
            let _ = vault_root;
            Ok(ManifestLock {})
        }
    }
}

impl Drop for ManifestLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode};

    fn vault_root() -> PathBuf {
        PathBuf::from("/tmp/test-vault")
    }

    fn test_config() -> ManifestConfig {
        ManifestConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            default_backend: AiBackend::Claude,
            skill_variant: SkillVariant::Claude,
            snapshot_mode: SnapshotMode::PreMerge,
        }
    }

    fn test_manifest() -> Manifest {
        Manifest::empty(vault_root(), test_config())
    }

    #[test]
    fn empty_sets_manifest_version() {
        assert_eq!(test_manifest().version, MANIFEST_VERSION);
    }

    #[test]
    fn empty_sets_default_flags_path() {
        assert_eq!(test_manifest().flags_path, "_generated/flags.json");
    }

    #[test]
    fn flags_path_absolute_joins_vault_root() {
        let m = test_manifest();
        assert_eq!(
            m.flags_path_absolute(),
            PathBuf::from("/tmp/test-vault/_generated/flags.json")
        );
    }

    #[test]
    fn last_report_path_is_under_generated_dir() {
        let m = test_manifest();
        assert_eq!(
            m.last_report_path(),
            PathBuf::from("/tmp/test-vault/_generated/last_report.json"),
        );
    }

    #[test]
    fn session_report_path_includes_session_id() {
        let m = test_manifest();
        let p = m.session_report_path("run-abc");
        assert_eq!(
            p,
            PathBuf::from("/tmp/test-vault/_generated/reports/run-abc.json"),
        );
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let m = Manifest::empty(tmp.path().to_path_buf(), test_config());
        m.save(tmp.path()).unwrap();
        let loaded = Manifest::load(tmp.path()).unwrap();
        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert!(loaded.sessions.is_empty());
    }

    fn minimal_vault_config() -> crate::config::VaultConfig {
        serde_json::from_str(r#"{"active_courses":[]}"#).unwrap()
    }

    #[test]
    fn load_or_create_creates_when_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let m = Manifest::load_or_create(tmp.path(), &minimal_vault_config()).unwrap();
        assert_eq!(m.version, MANIFEST_VERSION);
        assert!(tmp.path().join(MANIFEST_FILENAME).exists());
    }

    #[test]
    fn load_or_create_loads_when_present() {
        let tmp = tempfile::TempDir::new().unwrap();
        let m = Manifest::empty(tmp.path().to_path_buf(), test_config());
        m.save(tmp.path()).unwrap();
        let loaded = Manifest::load_or_create(tmp.path(), &minimal_vault_config()).unwrap();
        assert_eq!(loaded.version, MANIFEST_VERSION);
    }

    // ── SourceKind::as_str ────────────────────────────────────────────────────

    #[test]
    fn source_kind_as_str_all_variants() {
        assert_eq!(SourceKind::Textbook.as_str(), "textbook");
        assert_eq!(SourceKind::AiConversation.as_str(), "ai_conversation");
        assert_eq!(SourceKind::Paper.as_str(), "paper");
        assert_eq!(
            SourceKind::AssignmentFeedback.as_str(),
            "assignment_feedback"
        );
        assert_eq!(SourceKind::Other.as_str(), "other");
    }

    #[test]
    #[cfg(unix)]
    fn manifest_lock_is_exclusive() {
        use std::os::unix::io::AsRawFd;
        let tmp = tempfile::TempDir::new().unwrap();

        let _lock = ManifestLock::acquire(tmp.path()).expect("first acquire should succeed");

        // A second open+flock(LOCK_EX|LOCK_NB) on the same path must fail.
        let lock_path = tmp.path().join(LOCK_FILENAME);
        let file2 = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        let ret = unsafe { libc::flock(file2.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_ne!(
            ret, 0,
            "second lock attempt should fail while first is held"
        );
    }

    #[test]
    fn manifest_roundtrip() {
        let vault_root = PathBuf::from("/tmp/test-vault");
        let config = ManifestConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            default_backend: AiBackend::Claude,
            skill_variant: SkillVariant::Claude,
            snapshot_mode: SnapshotMode::PreMerge,
        };
        let manifest = Manifest::empty(vault_root, config);
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, MANIFEST_VERSION);
        assert!(back.sessions.is_empty());
    }
}
