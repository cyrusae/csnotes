#![allow(dead_code)]
/// Frontmatter reading, writing, and provenance-delta merging.
///
/// Obsidian (and this project) uses YAML fences:
///
/// ```text
/// ---
/// key: value
/// ---
///
/// Body content here.
/// ```
///
/// The CLI is the **sole writer** of frontmatter.  The AI reads it (via the
/// briefing) but never writes it — it declares deltas in the session report
/// and the CLI merges them here.
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CsnotesError;
use crate::manifest::{SessionContrib, SourceContrib};

pub const FRONTMATTER_SCHEMA_VERSION: u32 = 1;

// ── Note kinds ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Atomic,
    Index,
}

// ── NoteFrontmatter ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NoteFrontmatter {
    pub csnotes_schema: u32,
    pub kind: NoteKind,
    pub topic: String,
    pub title: String,

    /// Primary block anchor (atomic notes only; None for index notes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,

    /// Ordered list of atomic note stems this index embeds (index notes only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<String>>,

    pub contributing_sessions: Vec<SessionContrib>,
    pub contributing_sources: Vec<SourceContrib>,

    /// Which other notes embed this atomic (reverse index; atomic only).
    /// Rebuilt from a full `_synthetic/` scan during every merge-back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_embedded_in: Option<Vec<String>>,

    pub created: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
}

impl NoteFrontmatter {
    /// Create frontmatter for a brand-new atomic note.
    pub fn new_atomic(
        topic: impl Into<String>,
        title: impl Into<String>,
        block_id: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        NoteFrontmatter {
            csnotes_schema: FRONTMATTER_SCHEMA_VERSION,
            kind: NoteKind::Atomic,
            topic: topic.into(),
            title: title.into(),
            block_id: Some(block_id.into()),
            embeds: None,
            contributing_sessions: vec![],
            contributing_sources: vec![],
            cross_embedded_in: None,
            created: now,
            last_updated: now,
        }
    }

    /// Create frontmatter for a brand-new index note.
    pub fn new_index(
        topic: impl Into<String>,
        title: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        NoteFrontmatter {
            csnotes_schema: FRONTMATTER_SCHEMA_VERSION,
            kind: NoteKind::Index,
            topic: topic.into(),
            title: title.into(),
            block_id: None,
            embeds: Some(vec![]),
            contributing_sessions: vec![],
            contributing_sources: vec![],
            cross_embedded_in: None,
            created: now,
            last_updated: now,
        }
    }

    /// Merge a provenance delta (from a session report op) into this
    /// frontmatter.  Deduplicates on `(course, date)` for sessions and
    /// `(source_id, location.path)` for sources.
    pub fn merge_provenance(&mut self, delta: &ProvenanceDelta, now: DateTime<Utc>) {
        let mut changed = false;

        for contrib in &delta.sessions {
            let key = (&contrib.course, contrib.date);
            if !self
                .contributing_sessions
                .iter()
                .any(|c| (&c.course, c.date) == key)
            {
                self.contributing_sessions.push(contrib.clone());
                changed = true;
            }
        }

        for contrib in &delta.sources {
            if !self
                .contributing_sources
                .iter()
                .any(|c| c.source_id == contrib.source_id)
            {
                self.contributing_sources.push(contrib.clone());
                changed = true;
            }
        }

        if changed {
            self.last_updated = now;
        }
    }

    /// Force `last_updated` to `now` (used when note body was changed even if
    /// provenance was unchanged, e.g. an `update_note` with no new sessions).
    pub fn touch(&mut self, now: DateTime<Utc>) {
        self.last_updated = now;
    }
}

// ── ProvenanceDelta ───────────────────────────────────────────────────────────

/// The provenance information declared in a session report op (`provenance`
/// field for `create_note`, `add_provenance` for `update_note`).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProvenanceDelta {
    #[serde(default)]
    pub sessions: Vec<SessionContrib>,
    #[serde(default)]
    pub sources: Vec<SourceContrib>,
}

// ── Parsing & serialising the YAML fence ──────────────────────────────────────

/// Split a markdown file's content into (frontmatter_yaml, body).
///
/// Returns `None` if the content doesn't start with a `---` fence.
/// The body is everything after the closing `---\n` (or `---` at EOF).
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content.strip_prefix("---\n")?;
    // Find the closing fence (must be `---` on its own line)
    let close = content
        .find("\n---\n")
        .or_else(|| content.strip_suffix("\n---").map(|_| content.len() - 4))?;
    let yaml = &content[..close];
    let body_start = close + "\n---\n".len();
    let body = if body_start <= content.len() {
        &content[body_start..]
    } else {
        ""
    };
    Some((yaml, body))
}

/// Read a markdown file from disk, normalizing CRLF → LF so that all
/// downstream parsing (frontmatter, regex anchors, wikilinks) works
/// regardless of how the file was saved.
pub fn read_note(path: &Path) -> std::io::Result<String> {
    let raw = std::fs::read_to_string(path)?;
    if raw.contains('\r') {
        Ok(raw.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Ok(raw)
    }
}

/// Parse the frontmatter from a markdown file on disk.
pub fn read_frontmatter(path: &Path) -> Result<NoteFrontmatter> {
    let content = read_note(path).with_context(|| format!("reading {}", path.display()))?;
    parse_frontmatter(&content, path)
}

/// Parse frontmatter from an in-memory string.  `path` is used only for
/// error messages.
pub fn parse_frontmatter(content: &str, path: &Path) -> Result<NoteFrontmatter> {
    let (yaml, _body) = split_frontmatter(content)
        .ok_or_else(|| CsnotesError::NoFrontmatter(path.to_path_buf()))?;

    let fm: NoteFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| CsnotesError::FrontmatterParse {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

    if fm.csnotes_schema != FRONTMATTER_SCHEMA_VERSION {
        bail!(CsnotesError::FrontmatterSchemaMismatch {
            path: path.to_path_buf(),
            got: fm.csnotes_schema,
            expected: FRONTMATTER_SCHEMA_VERSION,
        });
    }

    Ok(fm)
}

/// Write updated frontmatter back to a file, preserving the body unchanged.
pub fn write_frontmatter(path: &Path, fm: &NoteFrontmatter, body: &str) -> Result<()> {
    let yaml = serde_yml::to_string(fm).map_err(|e| CsnotesError::FrontmatterParse {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // serde_yml may or may not emit a leading `---\n`; strip it if present so
    // we control the fence format ourselves.  Also trim any trailing newlines
    // so the closing `---` always lands on its own line.
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);
    let yaml = yaml.trim_end_matches('\n');

    let content = format!("---\n{}\n---\n\n{}", yaml, body.trim_start_matches('\n'));
    std::fs::write(path, content)
        .with_context(|| format!("writing frontmatter to {}", path.display()))?;
    Ok(())
}

/// Read frontmatter + body from a file, apply a mutation, then write back.
pub fn update_frontmatter<F>(path: &Path, f: F) -> Result<()>
where
    F: FnOnce(&mut NoteFrontmatter),
{
    let content = read_note(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, body) = split_frontmatter(&content)
        .ok_or_else(|| CsnotesError::NoFrontmatter(path.to_path_buf()))?;

    let mut fm: NoteFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| CsnotesError::FrontmatterParse {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

    f(&mut fm);
    write_frontmatter(path, &fm, body)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
---
csnotes_schema: 1
kind: atomic
topic: inheritance
title: Polymorphism
block_id: polymorphism-core
contributing_sessions: []
contributing_sources: []
created: \"2026-07-30T14:10:00Z\"
last_updated: \"2026-07-30T14:10:00Z\"
---

# Polymorphism

Body content here. ^polymorphism-core
";

    #[test]
    fn split_basic() {
        let (yaml, body) = split_frontmatter(SAMPLE).unwrap();
        assert!(yaml.contains("kind: atomic"));
        assert!(body.contains("# Polymorphism"));
    }

    #[test]
    fn parse_round_trip() {
        let fm = parse_frontmatter(SAMPLE, Path::new("test.md")).unwrap();
        assert_eq!(fm.kind, NoteKind::Atomic);
        assert_eq!(fm.topic, "inheritance");
        assert_eq!(fm.block_id.as_deref(), Some("polymorphism-core"));
    }

    #[test]
    fn merge_provenance_dedupes() {
        use crate::manifest::Relationship;
        use chrono::TimeZone;

        let now = Utc.with_ymd_and_hms(2026, 7, 30, 14, 10, 0).unwrap();
        let mut fm = NoteFrontmatter::new_atomic("inheritance", "Polymorphism", "poly-core", now);

        let delta = ProvenanceDelta {
            sessions: vec![SessionContrib {
                course: "CS501".into(),
                date: chrono::NaiveDate::from_ymd_opt(2026, 7, 30).unwrap(),
                relationship: Relationship::Introduced,
            }],
            sources: vec![],
        };

        // Apply once
        fm.merge_provenance(&delta, now);
        assert_eq!(fm.contributing_sessions.len(), 1);

        // Apply again — should not duplicate
        fm.merge_provenance(&delta, now);
        assert_eq!(fm.contributing_sessions.len(), 1);
    }

    #[test]
    fn no_frontmatter_returns_error() {
        let result = parse_frontmatter("# Just a header\n\nNo fence.", Path::new("bare.md"));
        assert!(result.is_err());
    }

    /// Verifies that write_frontmatter produces output that parse_frontmatter
    /// can round-trip back.  This exercises the serde_yml serialisation path.
    #[test]
    fn write_then_parse_roundtrip() {
        use chrono::TimeZone;
        use tempfile::NamedTempFile;

        let now = Utc.with_ymd_and_hms(2026, 9, 3, 18, 0, 0).unwrap();
        let fm = NoteFrontmatter::new_atomic("cpsc5001", "Algorithm Analysis", "algo-intro", now);
        let body = "Big-O notation.\n\n^algo-intro\n";

        let file = NamedTempFile::new().unwrap();
        write_frontmatter(file.path(), &fm, body).unwrap();

        let content = std::fs::read_to_string(file.path()).unwrap();
        assert!(
            content.starts_with("---\n"),
            "file must start with YAML fence; got: {:?}",
            &content[..content.len().min(40)]
        );

        let parsed = parse_frontmatter(&content, file.path()).unwrap();
        assert_eq!(parsed.kind, NoteKind::Atomic);
        assert_eq!(parsed.block_id.as_deref(), Some("algo-intro"));
        assert!(
            content.contains("Big-O notation."),
            "body must be preserved"
        );
    }

    // ── CRLF normalization tests ───────────────────────────────────────────

    #[test]
    fn read_note_strips_crlf() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        // Write a file with CRLF line endings
        let crlf_content = "---\r\nkey: value\r\n---\r\nbody\r\n";
        std::fs::write(tmp.path(), crlf_content).unwrap();

        let result = read_note(tmp.path()).unwrap();
        assert!(!result.contains('\r'), "read_note must strip \\r");
        assert_eq!(result, "---\nkey: value\n---\nbody\n");
    }

    #[test]
    fn split_frontmatter_works_after_crlf_normalization() {
        // Simulate a CRLF note normalized by read_note
        let crlf = "---\r\ncsnotes_schema: 1\r\nkind: atomic\r\ntopic: t\r\ntitle: T\r\nblock_id: x\r\ncontributing_sessions: []\r\ncontributing_sources: []\r\ncreated: \"2026-01-01T00:00:00Z\"\r\nlast_updated: \"2026-01-01T00:00:00Z\"\r\n---\r\nbody\r\n";
        let normalized = crlf.replace("\r\n", "\n");
        let (yaml, body) =
            split_frontmatter(&normalized).expect("should parse after normalization");
        assert!(!yaml.contains('\r'));
        assert_eq!(body.trim(), "body");
    }

    #[test]
    fn read_note_passthrough_when_no_crlf() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let lf_content = "---\nkey: value\n---\nbody\n";
        std::fs::write(tmp.path(), lf_content).unwrap();

        let result = read_note(tmp.path()).unwrap();
        assert_eq!(
            result, lf_content,
            "LF-only content must be returned unchanged"
        );
    }

    // ── touch ─────────────────────────────────────────────────────────────────

    #[test]
    fn touch_updates_last_updated() {
        use chrono::TimeZone;
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let mut fm = NoteFrontmatter::new_atomic("cs", "Sorting", "sort-01", t0);
        assert_eq!(fm.last_updated, t0);
        fm.touch(t1);
        assert_eq!(fm.last_updated, t1);
    }

    // ── merge_provenance updates last_updated ─────────────────────────────────

    #[test]
    fn merge_provenance_updates_last_updated_when_new_session_added() {
        use crate::manifest::Relationship;
        use chrono::TimeZone;
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let mut fm = NoteFrontmatter::new_atomic("cs", "Sorting", "sort-01", t0);
        let delta = ProvenanceDelta {
            sessions: vec![SessionContrib {
                course: "CPSC5001".into(),
                date: chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                relationship: Relationship::Introduced,
            }],
            sources: vec![],
        };
        fm.merge_provenance(&delta, t1);
        assert_eq!(
            fm.last_updated, t1,
            "last_updated should advance when provenance changes"
        );
        assert_eq!(fm.contributing_sessions.len(), 1);
    }

    #[test]
    fn merge_provenance_does_not_update_last_updated_when_nothing_new() {
        use crate::manifest::Relationship;
        use chrono::TimeZone;
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let mut fm = NoteFrontmatter::new_atomic("cs", "Sorting", "sort-01", t0);
        let contrib = SessionContrib {
            course: "CPSC5001".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            relationship: Relationship::Introduced,
        };
        let delta = ProvenanceDelta {
            sessions: vec![contrib],
            sources: vec![],
        };
        fm.merge_provenance(&delta, t1); // adds it
        fm.merge_provenance(&delta, t1); // duplicate — no change
                                         // last_updated must still be t1, not something newer
        assert_eq!(
            fm.contributing_sessions.len(),
            1,
            "duplicate session must not be added"
        );
    }

    // ── split_frontmatter boundary ────────────────────────────────────────────

    #[test]
    fn split_frontmatter_body_starts_immediately_after_fence() {
        let content = "---\nkind: atomic\n---\nFirst line of body.\n";
        let (yaml, body) = split_frontmatter(content).unwrap();
        assert_eq!(yaml.trim(), "kind: atomic");
        assert!(
            body.starts_with("First line"),
            "body must start right after closing fence; got: {:?}",
            body
        );
    }

    #[test]
    fn split_frontmatter_no_trailing_newline_after_fence() {
        // Exercises the strip_suffix("\n---") branch (line 173: content.len() - 4).
        let content = "---\nkind: atomic\n---";
        let (yaml, body) = split_frontmatter(content).unwrap();
        assert!(yaml.contains("kind: atomic"));
        assert_eq!(body, "");
    }
}
