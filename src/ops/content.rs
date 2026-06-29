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
/// - Source wikilinks in the note body are harvested automatically and
///   merged into `contributing_sources` (relationship defaults to
///   `introduced`; AI-declared sources take precedence).
///
/// Preconditions are checked separately in `audit::precondition_pass` before
/// any execution runs.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

use crate::error::CsnotesError;
use crate::frontmatter::{write_frontmatter, NoteFrontmatter, NoteKind, ProvenanceDelta};
use crate::manifest::{Manifest, Relationship, SourceContrib};
use crate::obsidian::extract_wikilinks;
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
    manifest: &Manifest,
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

    // Merge AI-declared provenance first so it takes priority over harvested.
    fm.merge_provenance(&op.provenance, now);

    // Harvest source wikilinks from the body and add any sources the AI
    // didn't already declare (deduplication is by source_id).
    let harvested = harvest_source_contribs(&body, manifest);
    let extra = sources_not_yet_in(&harvested, &fm);
    if !extra.is_empty() {
        fm.merge_provenance(
            &ProvenanceDelta {
                sessions: vec![],
                sources: extra,
            },
            now,
        );
    }

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
    manifest: &Manifest,
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

    // Merge AI-declared provenance first so it takes priority over harvested.
    fm.merge_provenance(&op.add_provenance, now);

    // Harvest source wikilinks from the body and add any sources the AI
    // didn't already declare (includes sources from previous sessions).
    let harvested = harvest_source_contribs(body, manifest);
    let extra = sources_not_yet_in(&harvested, &fm);
    if !extra.is_empty() {
        fm.merge_provenance(
            &ProvenanceDelta {
                sessions: vec![],
                sources: extra,
            },
            now,
        );
    }

    // Always touch `last_updated` — the body was changed even if provenance
    // was already recorded (e.g. a second pass to fix a typo).
    fm.touch(now);

    write_frontmatter(&note_path, &fm, body)
}

// ── source wikilink harvesting ────────────────────────────────────────────────

/// Scan `body` for `[[wikilinks]]` that resolve to source entries in `manifest`
/// and return a `SourceContrib` for each matched source (relationship:
/// `introduced`).
///
/// Three wikilink forms are recognised for each source ID
/// `<sources_dir>/<source_id>`:
///
/// - Bare stem: `[[Chapter-05]]`
/// - Source-ID path: `[[Textbooks/SICP/Chapter-05]]`
/// - Full vault-relative path: `[[sources/Textbooks/SICP/Chapter-05]]`
///
/// Stems that are shared by more than one source are treated as ambiguous and
/// ignored; use the full ID path instead.
pub fn harvest_source_contribs(body: &str, manifest: &Manifest) -> Vec<SourceContrib> {
    let sources_dir = &manifest.config.sources_dir;

    // Build: lowercased wikilink target → canonical source_id.
    // Track stem conflicts so ambiguous short-form links are skipped.
    let mut lookup: HashMap<String, &str> = HashMap::new();
    let mut stem_conflicts: HashSet<String> = HashSet::new();

    for source_id in manifest.sources.keys() {
        // Full vault-relative form: `sources/Textbooks/SICP/Chapter-05`
        let full = format!("{}/{}", sources_dir, source_id).to_lowercase();
        lookup.insert(full, source_id.as_str());

        // Source-ID form: `Textbooks/SICP/Chapter-05`
        lookup.insert(source_id.to_lowercase(), source_id.as_str());

        // Stem form: `Chapter-05`
        if let Some(stem) = source_id.split('/').next_back() {
            let stem_lower = stem.to_lowercase();
            if !stem_conflicts.contains(&stem_lower) {
                match lookup.entry(stem_lower.clone()) {
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(source_id.as_str());
                    }
                    std::collections::hash_map::Entry::Occupied(e) => {
                        // Second source with this stem → ambiguous, remove.
                        e.remove();
                        stem_conflicts.insert(stem_lower);
                    }
                }
            }
        }
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut result: Vec<SourceContrib> = Vec::new();

    for link in extract_wikilinks(body) {
        let target_lower = link.target.to_lowercase();
        if let Some(&sid) = lookup.get(target_lower.as_str()) {
            if seen.insert(sid) {
                result.push(SourceContrib {
                    source_id: sid.to_string(),
                    relationship: Relationship::Introduced,
                });
            }
        }
    }

    result
}

/// Filter `candidates` to only those source IDs not already in
/// `fm.contributing_sources`.
fn sources_not_yet_in(candidates: &[SourceContrib], fm: &NoteFrontmatter) -> Vec<SourceContrib> {
    candidates
        .iter()
        .filter(|sc| {
            !fm.contributing_sources
                .iter()
                .any(|e| e.source_id == sc.source_id)
        })
        .cloned()
        .collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestConfig, SourceEntry, SourceKind, SourceStatus};
    use indexmap::IndexMap;

    fn make_manifest(source_ids: &[&str]) -> Manifest {
        let mut sources = IndexMap::new();
        for &id in source_ids {
            sources.insert(
                id.to_string(),
                SourceEntry {
                    path: format!("sources/{}.md", id),
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
        }
        Manifest {
            version: "2".to_string(),
            vault_root: std::path::PathBuf::from("/vault"),
            config: ManifestConfig {
                raw_dir: "raw".into(),
                recordings_dir: "recordings".into(),
                artifacts_dir: "artifacts".into(),
                sources_dir: "sources".into(),
                synthetic_dir: "_synthetic".into(),
                generated_dir: "_generated".into(),
                filename_format: "{course}-{yyyy}-{mm}-{dd}".into(),
                default_backend: crate::config::AiBackend::Claude,
                skill_variant: crate::config::SkillVariant::Claude,
                snapshot_mode: crate::config::SnapshotMode::PreMerge,
            },
            sessions: IndexMap::new(),
            sources,
            topics: IndexMap::new(),
            session_in_progress: None,
            flags_path: "_generated/flags.json".into(),
        }
    }

    #[test]
    fn harvest_full_source_id_wikilink() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05"]);
        let body = "See [[Textbooks/SICP/Chapter-05]] for details.\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_id, "Textbooks/SICP/Chapter-05");
        assert_eq!(result[0].relationship, Relationship::Introduced);
    }

    #[test]
    fn harvest_stem_wikilink() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05"]);
        let body = "See [[Chapter-05]] and some other text.\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_id, "Textbooks/SICP/Chapter-05");
    }

    #[test]
    fn harvest_full_vault_path_wikilink() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05"]);
        let body = "See [[sources/Textbooks/SICP/Chapter-05]].\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_id, "Textbooks/SICP/Chapter-05");
    }

    #[test]
    fn harvest_ignores_non_source_wikilinks() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05"]);
        let body = "See [[some-other-note]] and [[inheritance]].\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert!(result.is_empty());
    }

    #[test]
    fn harvest_deduplicates_multiple_links_to_same_source() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05"]);
        let body = "[[Chapter-05]] and [[Textbooks/SICP/Chapter-05]] again.\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn harvest_multiple_sources() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05", "Papers/dijkstra-1968"]);
        let body = "From [[Chapter-05]] and [[dijkstra-1968]].\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn ambiguous_stem_is_skipped() {
        // Two sources share the same stem — stem-form link should not match either.
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05", "Papers/Chapter-05"]);
        let body = "See [[Chapter-05]].\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert!(result.is_empty(), "ambiguous stem should not match");
    }

    #[test]
    fn ambiguous_stem_but_full_id_still_resolves() {
        let manifest = make_manifest(&["Textbooks/SICP/Chapter-05", "Papers/Chapter-05"]);
        let body = "See [[Textbooks/SICP/Chapter-05]].\n^slug\n";
        let result = harvest_source_contribs(body, &manifest);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_id, "Textbooks/SICP/Chapter-05");
    }

    #[test]
    fn sources_not_yet_in_filters_already_present() {
        use chrono::Utc;
        let now = Utc::now();
        let mut fm = NoteFrontmatter::new_atomic("topic", "Title", "slug".to_string(), now);
        fm.contributing_sources.push(SourceContrib {
            source_id: "Textbooks/SICP/Chapter-05".to_string(),
            relationship: Relationship::Extended,
        });

        let candidates = vec![
            SourceContrib {
                source_id: "Textbooks/SICP/Chapter-05".to_string(),
                relationship: Relationship::Introduced,
            },
            SourceContrib {
                source_id: "Papers/dijkstra-1968".to_string(),
                relationship: Relationship::Introduced,
            },
        ];

        let extra = sources_not_yet_in(&candidates, &fm);
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0].source_id, "Papers/dijkstra-1968");
    }
}
