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
        let content = serde_json::to_string_pretty(self)
            .context("serializing manifest")?;
        std::fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Path to the flags store (absolute).
    pub fn flags_path_absolute(&self) -> PathBuf {
        self.vault_root.join(&self.flags_path)
    }

    /// Path to the last session report copy (written during merge-back).
    pub fn last_report_path(&self) -> PathBuf {
        self.vault_root.join(&self.config.generated_dir).join("last_report.json")
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
    pub plaud_dir: String,
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
            plaud_dir: cfg.plaud_dir.clone(),
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
    pub plaud_exports: Vec<PlaudExport>,
    pub artifacts: Vec<ArtifactEntry>,
    pub plaud_missing: bool,
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
pub struct PlaudExport {
    pub path: String,
    pub kind: PlaudKind,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaudKind {
    Transcript,
    Summary,
    Mindmap,
    /// Anonymous recording export (e.g., `-a`, `-b`).
    Anonymous,
    /// User-defined qualifier from `plaud_qualifiers` config.
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
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Textbook,
    Paper,
    AssignmentFeedback,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
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
    pub location: SourceLocation,
    pub relationship: Relationship,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Canonical numeric coordinate through the source's heading tree.
    /// e.g. [1, 1, 2] for section 1.1.2
    pub path: Vec<serde_json::Value>,
    /// Resolved heading text (stable across renumbering).
    pub label: String,
    /// What the AI wrote (raw string before canonicalisation).
    pub raw: String,
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let vault_root = PathBuf::from("/tmp/test-vault");
        let config = ManifestConfig {
            raw_dir: "notes".into(),
            plaud_dir: "plaud".into(),
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
