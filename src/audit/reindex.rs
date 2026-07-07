use std::path::Path;

use anyhow::Result;

use crate::frontmatter::{parse_frontmatter, NoteKind};
use crate::manifest::Manifest;

/// Rescan `_synthetic/` and rebuild `manifest.topics` from frontmatter.
///
/// Clears the existing topics map first so renamed or deleted topics are
/// removed.  Call this whenever the synthetic vault may have changed outside
/// of normal session processing (manual edits, recovery copies, etc.).
pub fn rebuild_topics(
    vault_root: &Path,
    synthetic_dir: &str,
    manifest: &mut Manifest,
) -> Result<()> {
    use crate::manifest::TopicEntry;

    let synthetic_root = vault_root.join(synthetic_dir);
    manifest.topics.clear();

    if !synthetic_root.exists() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(&synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match crate::frontmatter::read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let fm = match parse_frontmatter(&content, entry.path()) {
            Ok(fm) => fm,
            Err(_) => continue,
        };

        let rel_path = entry
            .path()
            .strip_prefix(vault_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .to_string();

        let topic = manifest
            .topics
            .entry(fm.topic.clone())
            .or_insert_with(|| TopicEntry {
                index_note: String::new(),
                atomic_notes: vec![],
                contributing_sessions: vec![],
                contributing_sources: vec![],
                pending_sessions: vec![],
                last_updated: fm.last_updated,
                open_flags: 0,
                source_types: vec![],
            });

        match fm.kind {
            NoteKind::Index => topic.index_note = rel_path,
            NoteKind::Atomic => {
                if !topic.atomic_notes.contains(&rel_path) {
                    topic.atomic_notes.push(rel_path);
                }
            }
        }

        for contrib in fm.contributing_sessions {
            if !topic
                .contributing_sessions
                .iter()
                .any(|c| c.course == contrib.course && c.date == contrib.date)
            {
                topic.contributing_sessions.push(contrib);
            }
        }

        if fm.last_updated > topic.last_updated {
            topic.last_updated = fm.last_updated;
        }
    }

    // pending_sessions: sessions processed after the topic's last_updated that
    // declared they touched this topic — signals possible desync.
    let sessions_snapshot: Vec<(String, Option<chrono::DateTime<chrono::Utc>>, Vec<String>)> =
        manifest
            .sessions
            .iter()
            .map(|(id, s)| (id.clone(), s.processed_at, s.topics_updated.clone()))
            .collect();

    for (topic_name, topic_entry) in manifest.topics.iter_mut() {
        topic_entry.pending_sessions = sessions_snapshot
            .iter()
            .filter(|(_, processed_at, topics_updated)| {
                processed_at.is_some_and(|t| t > topic_entry.last_updated)
                    && topics_updated.contains(topic_name)
            })
            .map(|(id, _, _)| id.clone())
            .collect();
    }

    Ok(())
}

/// Rebuild the manifest from frontmatter + filesystem.
pub fn reindex(vault_root: &Path, config: &crate::config::VaultConfig) -> Result<Manifest> {
    use crate::manifest::ManifestConfig;

    let manifest_config = ManifestConfig::from_vault_config(config);
    let mut manifest = Manifest::empty(vault_root.to_path_buf(), manifest_config);

    if let Ok(old) = Manifest::load(vault_root) {
        manifest.sessions = old.sessions;
        manifest.sources = old.sources;
    }

    rebuild_topics(vault_root, &config.synthetic_dir, &mut manifest)?;

    Ok(manifest)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode, VaultConfig};
    use crate::manifest::{ManifestConfig, SessionEntry, SessionStatus};
    use chrono::{DateTime, Utc};
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

    fn note_with_contrib(block_id: &str, course: &str, date: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\n\
             contributing_sessions:\n- course: {course}\n  date: \"{date}\"\n  relationship: introduced\n\
             contributing_sources: []\ncreated: \"2026-01-01T00:00:00Z\"\n\
             last_updated: \"2026-01-01T00:00:00Z\"\n---\nContent.\n\n^{block_id}\n"
        )
    }

    fn write_manifest_with_session(
        vault_root: &Path,
        session_id: &str,
        processed_at: Option<DateTime<Utc>>,
        topics_updated: Vec<String>,
    ) {
        let mut m = make_empty_manifest(vault_root);
        m.sessions.insert(
            session_id.to_string(),
            SessionEntry {
                date: chrono::NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
                course: "CS101".into(),
                filename_format: "{course}-{mm}-{dd}".into(),
                raw_note: "notes/cs101-01-10.md".into(),
                recording_exports: vec![],
                artifacts: vec![],
                recording_missing: false,
                status: SessionStatus::Processed,
                processed_at,
                topics_updated,
            },
        );
        m.save(vault_root).unwrap();
    }

    #[test]
    fn reindex_collects_atomic_notes_for_topic() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest
            .topics
            .get("cs")
            .expect("cs topic should exist after reindex");
        assert_eq!(topic.atomic_notes.len(), 1);
        assert!(
            topic.atomic_notes[0].contains("sorting.md"),
            "{:?}",
            topic.atomic_notes
        );
    }

    #[test]
    fn reindex_deduplicates_identical_contributing_sessions() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/note-a.md",
            &note_with_contrib("id-a", "CS101", "2026-01-10"),
        );
        write(
            tmp.path(),
            "_synthetic/cs/note-b.md",
            &note_with_contrib("id-b", "CS101", "2026-01-10"),
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert_eq!(
            topic.contributing_sessions.len(),
            1,
            "{:?}",
            topic.contributing_sessions
        );
    }

    #[test]
    fn reindex_keeps_distinct_contributing_sessions_by_date() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/note-a.md",
            &note_with_contrib("id-a", "CS101", "2026-01-10"),
        );
        write(
            tmp.path(),
            "_synthetic/cs/note-b.md",
            &note_with_contrib("id-b", "CS101", "2026-01-17"),
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert_eq!(
            topic.contributing_sessions.len(),
            2,
            "{:?}",
            topic.contributing_sessions
        );
    }

    #[test]
    fn reindex_pending_sessions_includes_post_topic_session() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let processed = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        write_manifest_with_session(tmp.path(), "sess-1", Some(processed), vec!["cs".into()]);
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert!(
            topic.pending_sessions.contains(&"sess-1".to_string()),
            "{:?}",
            topic.pending_sessions
        );
    }

    #[test]
    fn reindex_pending_sessions_excludes_pre_topic_session() {
        let tmp = TempDir::new().unwrap();
        let late_note = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Late\n\
                         block_id: late-01\ncontributing_sessions: []\ncontributing_sources: []\n\
                         created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-06-01T00:00:00Z\"\n\
                         ---\nContent.\n\n^late-01\n";
        write(tmp.path(), "_synthetic/cs/late.md", late_note);
        let processed = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        write_manifest_with_session(tmp.path(), "old-sess", Some(processed), vec!["cs".into()]);
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert!(
            !topic.pending_sessions.contains(&"old-sess".to_string()),
            "{:?}",
            topic.pending_sessions
        );
    }

    #[test]
    fn reindex_pending_sessions_excludes_session_not_for_topic() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let processed = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        write_manifest_with_session(
            tmp.path(),
            "wrong-topic-sess",
            Some(processed),
            vec!["other-topic".into()],
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert!(
            !topic
                .pending_sessions
                .contains(&"wrong-topic-sess".to_string()),
            "{:?}",
            topic.pending_sessions
        );
    }

    #[test]
    fn reindex_tracks_last_updated_as_max() {
        let tmp = TempDir::new().unwrap();
        let early = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Early\n\
                     block_id: early-01\ncontributing_sessions: []\ncontributing_sources: []\n\
                     created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
                     ---\nContent.\n\n^early-01\n";
        let late = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Late\n\
                     block_id: late-01\ncontributing_sessions: []\ncontributing_sources: []\n\
                     created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-06-01T00:00:00Z\"\n\
                     ---\nContent.\n\n^late-01\n";
        write(tmp.path(), "_synthetic/cs/early.md", early);
        write(tmp.path(), "_synthetic/cs/late.md", late);
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert_eq!(
            topic.last_updated.format("%Y-%m-%d").to_string(),
            "2026-06-01",
            "{}",
            topic.last_updated
        );
    }
}
