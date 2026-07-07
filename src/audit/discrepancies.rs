use std::path::Path;

use anyhow::Result;

use crate::manifest::Manifest;

/// A single discrepancy between the manifest and the actual filesystem.
#[derive(Debug, PartialEq, Eq)]
pub struct Discrepancy {
    pub kind: DiscrepancyKind,
    pub topic: String,
    pub path: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DiscrepancyKind {
    /// Manifest records this topic but no matching index note exists on disk.
    StaleTopicInManifest,
    /// A topic folder with an index note exists on disk but has no manifest entry.
    UntrackedTopicOnDisk,
    /// Manifest lists this atomic note but the file does not exist on disk.
    StaleAtomicInManifest,
    /// An atomic note file exists on disk but is absent from the manifest for its topic.
    UntrackedAtomicOnDisk,
}

impl Discrepancy {
    pub fn display(&self) -> String {
        match self.kind {
            DiscrepancyKind::StaleTopicInManifest => format!(
                "stale   topic  [manifest-only]  {} ({})",
                self.topic, self.path
            ),
            DiscrepancyKind::UntrackedTopicOnDisk => format!(
                "untracked topic [disk-only]      {} ({})",
                self.topic, self.path
            ),
            DiscrepancyKind::StaleAtomicInManifest => {
                format!("stale   atomic [manifest-only]  {}", self.path)
            }
            DiscrepancyKind::UntrackedAtomicOnDisk => {
                format!("untracked atomic [disk-only]    {}", self.path)
            }
        }
    }
}

/// Compare `manifest.topics` against the current filesystem state and return
/// all differences.
pub fn manifest_discrepancies(
    vault_root: &Path,
    config: &crate::config::VaultConfig,
    manifest: &Manifest,
) -> Result<Vec<Discrepancy>> {
    use crate::manifest::ManifestConfig;

    let manifest_config = ManifestConfig::from_vault_config(config);
    let mut fresh = Manifest::empty(vault_root.to_path_buf(), manifest_config);
    super::reindex::rebuild_topics(vault_root, &config.synthetic_dir, &mut fresh)?;

    let mut out: Vec<Discrepancy> = Vec::new();

    for (topic, entry) in &manifest.topics {
        if !fresh.topics.contains_key(topic.as_str()) {
            out.push(Discrepancy {
                kind: DiscrepancyKind::StaleTopicInManifest,
                topic: topic.clone(),
                path: entry.index_note.clone(),
            });
        }
    }

    for (topic, entry) in &fresh.topics {
        if !manifest.topics.contains_key(topic.as_str()) {
            out.push(Discrepancy {
                kind: DiscrepancyKind::UntrackedTopicOnDisk,
                topic: topic.clone(),
                path: entry.index_note.clone(),
            });
        }
    }

    let mut common_topics: Vec<&str> = manifest
        .topics
        .keys()
        .filter(|t| fresh.topics.contains_key(t.as_str()))
        .map(|s| s.as_str())
        .collect();
    common_topics.sort_unstable();

    for topic in common_topics {
        let manifest_atomics: std::collections::HashSet<&str> = manifest.topics[topic]
            .atomic_notes
            .iter()
            .map(|s| s.as_str())
            .collect();
        let disk_atomics: std::collections::HashSet<&str> = fresh.topics[topic]
            .atomic_notes
            .iter()
            .map(|s| s.as_str())
            .collect();

        let mut stale: Vec<&str> = manifest_atomics
            .difference(&disk_atomics)
            .copied()
            .collect();
        stale.sort_unstable();
        for path in stale {
            out.push(Discrepancy {
                kind: DiscrepancyKind::StaleAtomicInManifest,
                topic: topic.to_string(),
                path: path.to_string(),
            });
        }

        let mut untracked: Vec<&str> = disk_atomics
            .difference(&manifest_atomics)
            .copied()
            .collect();
        untracked.sort_unstable();
        for path in untracked {
            out.push(Discrepancy {
                kind: DiscrepancyKind::UntrackedAtomicOnDisk,
                topic: topic.to_string(),
                path: path.to_string(),
            });
        }
    }

    Ok(out)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode, VaultConfig};
    use crate::manifest::ManifestConfig;
    use tempfile::TempDir;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        if let Some(parent) = dir.join(name).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn make_vault_config() -> VaultConfig {
        serde_json::from_str("{}").unwrap()
    }

    fn make_empty_manifest(vault_root: &Path) -> Manifest {
        Manifest::empty(
            vault_root.to_path_buf(),
            ManifestConfig {
                raw_dir: "notes".into(),
                recordings_dir: "recordings".into(),
                artifacts_dir: "artifacts".into(),
                sources_dir: "sources".into(),
                synthetic_dir: "_synthetic".into(),
                generated_dir: "_generated".into(),
                filename_format: "{course}-{mm}-{dd}".into(),
                default_backend: AiBackend::Mock,
                skill_variant: SkillVariant::Claude,
                snapshot_mode: SnapshotMode::PreMerge,
            },
        )
    }

    fn disc_atomic_note(topic: &str, block_id: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: {topic}\ntitle: Test\n\
             block_id: {block_id}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nContent.\n\n^{block_id}\n"
        )
    }

    fn disc_index_note(topic: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: index\ntopic: {topic}\ntitle: {topic}\n\
             contributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\n"
        )
    }

    #[test]
    fn discrepancies_empty_when_manifest_matches_disk() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();
        let config = make_vault_config();
        write(vault, "_synthetic/java/java.md", &disc_index_note("java"));
        write(
            vault,
            "_synthetic/java/java-generics.md",
            &disc_atomic_note("java", "java-gen-01"),
        );
        let mut manifest = make_empty_manifest(vault);
        super::super::reindex::rebuild_topics(vault, "_synthetic", &mut manifest).unwrap();
        let result = manifest_discrepancies(vault, &config, &manifest).unwrap();
        assert!(result.is_empty(), "{:?}", result);
    }

    #[test]
    fn discrepancies_detects_stale_topic_in_manifest() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();
        let config = make_vault_config();
        write(vault, "_synthetic/java/java.md", &disc_index_note("java"));
        let mut manifest = make_empty_manifest(vault);
        super::super::reindex::rebuild_topics(vault, "_synthetic", &mut manifest).unwrap();
        std::fs::remove_dir_all(vault.join("_synthetic/java")).unwrap();
        let result = manifest_discrepancies(vault, &config, &manifest).unwrap();
        assert!(
            result
                .iter()
                .any(|d| d.kind == DiscrepancyKind::StaleTopicInManifest && d.topic == "java"),
            "{:?}",
            result
        );
    }

    #[test]
    fn discrepancies_detects_untracked_topic_on_disk() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();
        let config = make_vault_config();
        write(vault, "_synthetic/java/java.md", &disc_index_note("java"));
        let manifest = make_empty_manifest(vault);
        let result = manifest_discrepancies(vault, &config, &manifest).unwrap();
        assert!(
            result
                .iter()
                .any(|d| d.kind == DiscrepancyKind::UntrackedTopicOnDisk && d.topic == "java"),
            "{:?}",
            result
        );
    }

    #[test]
    fn discrepancies_detects_stale_atomic_in_manifest() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();
        let config = make_vault_config();
        write(vault, "_synthetic/java/java.md", &disc_index_note("java"));
        write(
            vault,
            "_synthetic/java/java-generics.md",
            &disc_atomic_note("java", "java-gen-01"),
        );
        let mut manifest = make_empty_manifest(vault);
        super::super::reindex::rebuild_topics(vault, "_synthetic", &mut manifest).unwrap();
        std::fs::remove_file(vault.join("_synthetic/java/java-generics.md")).unwrap();
        let result = manifest_discrepancies(vault, &config, &manifest).unwrap();
        assert!(
            result
                .iter()
                .any(|d| d.kind == DiscrepancyKind::StaleAtomicInManifest
                    && d.path.contains("java-generics")),
            "{:?}",
            result
        );
    }

    #[test]
    fn discrepancies_detects_untracked_atomic_on_disk() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();
        let config = make_vault_config();
        write(vault, "_synthetic/java/java.md", &disc_index_note("java"));
        let mut manifest = make_empty_manifest(vault);
        super::super::reindex::rebuild_topics(vault, "_synthetic", &mut manifest).unwrap();
        write(
            vault,
            "_synthetic/java/java-generics.md",
            &disc_atomic_note("java", "java-gen-01"),
        );
        let result = manifest_discrepancies(vault, &config, &manifest).unwrap();
        assert!(
            result
                .iter()
                .any(|d| d.kind == DiscrepancyKind::UntrackedAtomicOnDisk
                    && d.path.contains("java-generics")),
            "{:?}",
            result
        );
    }
}
