use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::frontmatter::{parse_frontmatter, read_note, NoteKind};
use crate::manifest::Manifest;
use crate::obsidian::extract_block_ids;

/// A single mechanical repair that `audit --fix --apply` can execute.
pub struct FixItem {
    /// Human-readable description for the dry-run preview.
    pub description: String,
    pub action: FixAction,
}

pub enum FixAction {
    /// Append a `^block_id` anchor to the end of the note body.
    AppendAnchor { path: PathBuf, block_id: String },
    /// Add source credits (harvested from body wikilinks) to frontmatter.
    BackfillSources {
        path: PathBuf,
        sources: Vec<crate::manifest::SourceContrib>,
    },
}

/// Collect all auto-repairable issues in `_synthetic/`.
///
/// Detects:
/// - Atomic notes whose frontmatter declares a `block_id` but whose body is
///   missing the corresponding `^id` anchor.
/// - Notes with `[[wikilinks]]` to registered source files that are not yet
///   recorded in `contributing_sources`.
pub fn collect_fixes(
    vault_root: &Path,
    config: &crate::config::VaultConfig,
    manifest: &Manifest,
) -> Result<Vec<FixItem>> {
    let synthetic_root = vault_root.join(&config.synthetic_dir);
    let mut fixes = Vec::new();

    if !synthetic_root.exists() {
        return Ok(fixes);
    }

    for entry in walkdir::WalkDir::new(&synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let fm = match parse_frontmatter(&content, entry.path()) {
            Ok(fm) => fm,
            Err(_) => continue,
        };

        if fm.kind == NoteKind::Atomic {
            if let Some(id) = &fm.block_id {
                if !extract_block_ids(&content).contains(id) {
                    let rel = entry
                        .path()
                        .strip_prefix(vault_root)
                        .unwrap_or(entry.path());
                    fixes.push(FixItem {
                        description: format!(
                            "'{}': append '^{}' anchor to body",
                            rel.display(),
                            id
                        ),
                        action: FixAction::AppendAnchor {
                            path: entry.path().to_path_buf(),
                            block_id: id.clone(),
                        },
                    });
                }
            }
        }

        let body = match crate::frontmatter::split_frontmatter(&content) {
            Some((_yaml, b)) => b,
            None => continue,
        };
        let harvested = crate::ops::content::harvest_source_contribs(body, manifest);
        let new_sources: Vec<_> = harvested
            .into_iter()
            .filter(|sc| {
                !fm.contributing_sources
                    .iter()
                    .any(|e| e.source_id == sc.source_id)
            })
            .collect();
        if !new_sources.is_empty() {
            let rel = entry
                .path()
                .strip_prefix(vault_root)
                .unwrap_or(entry.path());
            let ids: Vec<&str> = new_sources.iter().map(|s| s.source_id.as_str()).collect();
            fixes.push(FixItem {
                description: format!(
                    "'{}': credit source(s) [{}] from body wikilinks",
                    rel.display(),
                    ids.join(", ")
                ),
                action: FixAction::BackfillSources {
                    path: entry.path().to_path_buf(),
                    sources: new_sources,
                },
            });
        }
    }

    Ok(fixes)
}

/// Execute a collected fix plan.  Returns the number of repairs applied.
pub fn apply_fixes(fixes: &[FixItem]) -> Result<usize> {
    use anyhow::Context;
    let mut applied = 0;
    for fix in fixes {
        match &fix.action {
            FixAction::AppendAnchor { path, block_id } => {
                let content = crate::frontmatter::read_note(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let trimmed = content.trim_end_matches('\n');
                let gap = if trimmed.ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                let new_content = format!("{}{}\n^{}\n", trimmed, gap, block_id);
                std::fs::write(path, new_content)
                    .with_context(|| format!("writing {}", path.display()))?;
                applied += 1;
            }
            FixAction::BackfillSources { path, sources } => {
                let content = crate::frontmatter::read_note(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let (yaml, body) = crate::frontmatter::split_frontmatter(&content)
                    .ok_or_else(|| anyhow::anyhow!("no frontmatter in '{}'", path.display()))?;
                let mut fm: crate::frontmatter::NoteFrontmatter = serde_yml::from_str(yaml)
                    .with_context(|| format!("parsing frontmatter in '{}'", path.display()))?;
                for sc in sources {
                    if !fm
                        .contributing_sources
                        .iter()
                        .any(|e| e.source_id == sc.source_id)
                    {
                        fm.contributing_sources.push(sc.clone());
                    }
                }
                crate::frontmatter::write_frontmatter(path, &fm, body)
                    .with_context(|| format!("writing '{}'", path.display()))?;
                applied += 1;
            }
        }
    }
    Ok(applied)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode, VaultConfig};
    use crate::manifest::{ManifestConfig, SourceEntry, SourceKind, SourceStatus};
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

    fn clean_note(block_id: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nContent.\n\n^{block_id}\n"
        )
    }

    fn note_missing_anchor(block_id: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nContent.\n"
        )
    }

    fn note_with_source_link(block_id: &str, link: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nSee [[{link}]] for details.\n\n^{block_id}\n"
        )
    }

    fn manifest_with_source(vault_root: &Path, source_id: &str) -> Manifest {
        let mut m = make_empty_manifest(vault_root);
        m.sources.insert(
            source_id.to_string(),
            SourceEntry {
                path: format!("sources/{}.md", source_id),
                kind: SourceKind::Textbook,
                status: SourceStatus::Unprocessed,
                last_processed_at: None,
                heading_scheme: vec![],
                topics_updated: vec![],
                summary: None,
                tags: vec![],
                courses: vec![],
            },
        );
        m
    }

    #[test]
    fn collect_fixes_returns_empty_when_no_synthetic_dir() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_empty_manifest(tmp.path());
        let fixes = collect_fixes(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert!(fixes.is_empty());
    }

    #[test]
    fn collect_fixes_returns_fix_for_atomic_missing_anchor() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &note_missing_anchor("sort-01"),
        );
        let manifest = make_empty_manifest(tmp.path());
        let fixes = collect_fixes(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.description).collect::<Vec<_>>()
        );
        assert!(
            fixes[0].description.contains("sort-01"),
            "{}",
            fixes[0].description
        );
    }

    #[test]
    fn collect_fixes_returns_no_fix_when_anchor_present() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let manifest = make_empty_manifest(tmp.path());
        let fixes = collect_fixes(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert!(fixes.is_empty());
    }

    #[test]
    fn apply_fixes_appends_anchor_and_reports_count() {
        let tmp = TempDir::new().unwrap();
        let note_path = tmp.path().join("note.md");
        std::fs::write(&note_path, "Content.\n").unwrap();
        let fixes = vec![FixItem {
            description: "test fix".into(),
            action: FixAction::AppendAnchor {
                path: note_path.clone(),
                block_id: "sort-01".into(),
            },
        }];
        let count = apply_fixes(&fixes).unwrap();
        assert_eq!(count, 1);
        let content = std::fs::read_to_string(&note_path).unwrap();
        assert!(
            content.contains("^sort-01"),
            "anchor not appended:\n{content}"
        );
    }

    #[test]
    fn apply_fixes_counts_all_applied() {
        let tmp = TempDir::new().unwrap();
        let path_a = tmp.path().join("a.md");
        let path_b = tmp.path().join("b.md");
        std::fs::write(&path_a, "Content A.\n").unwrap();
        std::fs::write(&path_b, "Content B.\n").unwrap();
        let fixes = vec![
            FixItem {
                description: "fix a".into(),
                action: FixAction::AppendAnchor {
                    path: path_a.clone(),
                    block_id: "id-a".into(),
                },
            },
            FixItem {
                description: "fix b".into(),
                action: FixAction::AppendAnchor {
                    path: path_b.clone(),
                    block_id: "id-b".into(),
                },
            },
        ];
        let count = apply_fixes(&fixes).unwrap();
        assert_eq!(count, 2);
        assert!(std::fs::read_to_string(&path_a).unwrap().contains("^id-a"));
        assert!(std::fs::read_to_string(&path_b).unwrap().contains("^id-b"));
    }

    #[test]
    fn collect_fixes_detects_missing_source_credit() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &note_with_source_link("sort-01", "Textbooks/SICP/Chapter-05"),
        );
        let manifest = manifest_with_source(tmp.path(), "Textbooks/SICP/Chapter-05");
        let fixes = collect_fixes(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.description).collect::<Vec<_>>()
        );
        assert!(
            fixes[0].description.contains("Textbooks/SICP/Chapter-05"),
            "{}",
            fixes[0].description
        );
    }

    #[test]
    fn collect_fixes_no_fix_when_source_already_credited() {
        let tmp = TempDir::new().unwrap();
        let content = format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: sort-01\ncontributing_sessions: []\n\
             contributing_sources:\n  - source_id: Textbooks/SICP/Chapter-05\n    relationship: introduced\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nSee [[Textbooks/SICP/Chapter-05]] for details.\n\n^sort-01\n"
        );
        write(tmp.path(), "_synthetic/cs/sorting.md", &content);
        let manifest = manifest_with_source(tmp.path(), "Textbooks/SICP/Chapter-05");
        let fixes = collect_fixes(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert!(
            fixes.is_empty(),
            "{:?}",
            fixes.iter().map(|f| &f.description).collect::<Vec<_>>()
        );
    }

    #[test]
    fn apply_fixes_backfill_adds_source_and_preserves_last_updated() {
        let tmp = TempDir::new().unwrap();
        let note_path = tmp.path().join("note.md");
        std::fs::write(
            &note_path,
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: sort-01\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nSee [[Textbooks/SICP/Chapter-05]].\n\n^sort-01\n",
        )
        .unwrap();

        let fixes = vec![FixItem {
            description: "backfill test".into(),
            action: FixAction::BackfillSources {
                path: note_path.clone(),
                sources: vec![crate::manifest::SourceContrib {
                    source_id: "Textbooks/SICP/Chapter-05".into(),
                    relationship: crate::manifest::Relationship::Introduced,
                }],
            },
        }];
        let count = apply_fixes(&fixes).unwrap();
        assert_eq!(count, 1);
        let content = std::fs::read_to_string(&note_path).unwrap();
        assert!(
            content.contains("Textbooks/SICP/Chapter-05"),
            "source not written:\n{content}"
        );
        assert!(
            content.contains("last_updated: \"2026-01-01T00:00:00Z\""),
            "last_updated changed:\n{content}"
        );
    }
}
