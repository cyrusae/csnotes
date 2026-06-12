#![allow(dead_code)]
/// The persistent review-flag store (`_generated/flags.json`).
///
/// Flags are proposed by the AI in the session report; the CLI appends them
/// here with an `id` and `open` state; the user resolves them via
/// `csnotes flags resolve <id>`.
///
/// Flag tiers:
/// - `possible_misread` / `needs_confirmation` → actionable: persists, nags
///   in `status`.
/// - `unresolved_question` → thread: persists, surfaces only via briefing
///   re-injection.
/// - `ambiguity` → changelog: auto-resolved on append, never nags.
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::report::{FlagKind, ReviewFlag};

// ── Flag store ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FlagStore {
    pub flags: Vec<StoredFlag>,
}

impl FlagStore {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(FlagStore::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let store: FlagStore = serde_json::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(store)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("serializing flag store")?;
        std::fs::write(path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Append flags from a session report.  `ambiguity` flags are
    /// auto-resolved immediately (logged only).
    pub fn append_from_report(
        &mut self,
        flags: &[ReviewFlag],
        run_id: &str,
        now: DateTime<Utc>,
    ) {
        for flag in flags {
            let auto_resolved = flag.kind == FlagKind::Ambiguity;
            self.flags.push(StoredFlag {
                id: Uuid::new_v4().to_string(),
                kind: flag.kind,
                path: flag.path.clone(),
                anchor: flag.anchor.clone(),
                message: flag.message.clone(),
                open: !auto_resolved,
                created_at: now,
                resolved_at: if auto_resolved { Some(now) } else { None },
                run_id: run_id.to_string(),
                follow_up: None,
            });
        }
    }

    /// All open actionable flags (`possible_misread` | `needs_confirmation`).
    pub fn open_actionable(&self) -> impl Iterator<Item = &StoredFlag> {
        self.flags
            .iter()
            .filter(|f| f.open && f.kind.is_actionable())
    }

    /// All open thread flags (`unresolved_question`).
    pub fn open_threads(&self) -> impl Iterator<Item = &StoredFlag> {
        self.flags
            .iter()
            .filter(|f| f.open && f.kind.is_thread())
    }

    /// Open flags relevant to a specific note path (for briefing injection).
    pub fn open_for_path<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a StoredFlag> + 'a {
        self.flags.iter().filter(move |f| {
            f.open && f.path.as_deref() == Some(path)
        })
    }

    /// Open flags relevant to a topic (flags whose path starts with the topic
    /// directory — used to populate the briefing).
    pub fn open_for_topic<'a>(&'a self, topic: &'a str, synthetic_dir: &str) -> impl Iterator<Item = &'a StoredFlag> + 'a {
        let prefix = format!("{}/{}/", synthetic_dir, topic);
        self.flags.iter().filter(move |f| {
            f.open && f.path.as_deref().map_or(false, |p| p.starts_with(&prefix))
        })
    }

    /// Resolved flags that have a follow-up note — injected into the next
    /// session briefing so the AI has context from previous resolutions.
    pub fn resolved_with_follow_up(&self) -> impl Iterator<Item = &StoredFlag> {
        self.flags
            .iter()
            .filter(|f| !f.open && f.follow_up.is_some())
    }

    /// Resolve a flag by ID.  Returns `false` if not found.
    pub fn resolve(&mut self, id: &str, now: DateTime<Utc>, follow_up: Option<String>) -> bool {
        if let Some(flag) = self.flags.iter_mut().find(|f| f.id == id) {
            flag.open = false;
            flag.resolved_at = Some(now);
            flag.follow_up = follow_up;
            true
        } else {
            false
        }
    }

    pub fn count_open_actionable(&self) -> usize {
        self.open_actionable().count()
    }
}

// ── StoredFlag ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredFlag {
    pub id: String,
    pub kind: FlagKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    pub message: String,
    pub open: bool,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    /// Which run produced this flag.
    pub run_id: String,
    /// A follow-up note recorded at resolution time (e.g., the user's answer
    /// to a `needs_confirmation`), to be re-injected into the next briefing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<String>,
}

impl StoredFlag {
    pub fn display_kind(&self) -> &'static str {
        match self.kind {
            FlagKind::PossibleMisread => "possible misread",
            FlagKind::NeedsConfirmation => "needs confirmation",
            FlagKind::UnresolvedQuestion => "unresolved question",
            FlagKind::Ambiguity => "ambiguity (auto-resolved)",
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::ReviewFlag;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 3, 12, 9, 30, 0).unwrap()
    }

    fn make_flag(kind: FlagKind) -> ReviewFlag {
        ReviewFlag {
            kind,
            path: Some("_synthetic/inheritance/inheritance.md".into()),
            anchor: None,
            message: "Test message.".into(),
        }
    }

    #[test]
    fn actionable_flags_persist_open() {
        let mut store = FlagStore::default();
        store.append_from_report(
            &[make_flag(FlagKind::NeedsConfirmation)],
            "run-1",
            now(),
        );
        assert_eq!(store.count_open_actionable(), 1);
    }

    #[test]
    fn ambiguity_auto_resolved() {
        let mut store = FlagStore::default();
        store.append_from_report(&[make_flag(FlagKind::Ambiguity)], "run-1", now());
        assert_eq!(store.count_open_actionable(), 0);
        assert!(!store.flags[0].open);
    }

    #[test]
    fn resolve_flag() {
        let mut store = FlagStore::default();
        store.append_from_report(
            &[make_flag(FlagKind::PossibleMisread)],
            "run-1",
            now(),
        );
        let id = store.flags[0].id.clone();
        assert!(store.resolve(&id, now(), None));
        assert_eq!(store.count_open_actionable(), 0);
    }

    #[test]
    fn resolve_nonexistent_returns_false() {
        let mut store = FlagStore::default();
        assert!(!store.resolve("nonexistent-id", now(), None));
    }

    #[test]
    fn roundtrip_json() {
        let mut store = FlagStore::default();
        store.append_from_report(
            &[
                make_flag(FlagKind::NeedsConfirmation),
                make_flag(FlagKind::Ambiguity),
            ],
            "run-1",
            now(),
        );
        let json = serde_json::to_string_pretty(&store).unwrap();
        let back: FlagStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back.flags.len(), 2);
    }

    // ── FlagStore filter methods ──────────────────────────────────────────────

    fn stored(kind: FlagKind, path: Option<&str>, open: bool, follow_up: Option<&str>) -> StoredFlag {
        StoredFlag {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            path: path.map(|s| s.to_string()),
            anchor: None,
            message: "test".into(),
            open,
            created_at: now(),
            resolved_at: None,
            run_id: "run-1".into(),
            follow_up: follow_up.map(|s| s.to_string()),
        }
    }

    #[test]
    fn open_threads_returns_only_unresolved_question_flags() {
        let store = FlagStore {
            flags: vec![
                stored(FlagKind::UnresolvedQuestion, None, true, None),
                stored(FlagKind::NeedsConfirmation, None, true, None),
                stored(FlagKind::UnresolvedQuestion, None, false, None), // closed
            ],
        };
        let threads: Vec<_> = store.open_threads().collect();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].kind, FlagKind::UnresolvedQuestion);
    }

    #[test]
    fn open_for_path_filters_by_exact_path() {
        let target = "_synthetic/cs/sorting.md";
        let store = FlagStore {
            flags: vec![
                stored(FlagKind::NeedsConfirmation, Some(target), true, None),
                stored(FlagKind::NeedsConfirmation, Some("_synthetic/cs/other.md"), true, None),
                stored(FlagKind::NeedsConfirmation, Some(target), false, None), // closed
            ],
        };
        let hits: Vec<_> = store.open_for_path(target).collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path.as_deref(), Some(target));
    }

    #[test]
    fn open_for_topic_filters_by_path_prefix() {
        let store = FlagStore {
            flags: vec![
                stored(FlagKind::NeedsConfirmation, Some("_synthetic/cs/sorting.md"), true, None),
                stored(FlagKind::NeedsConfirmation, Some("_synthetic/math/sets.md"), true, None),
                stored(FlagKind::NeedsConfirmation, Some("_synthetic/cs/graphs.md"), false, None),
            ],
        };
        let hits: Vec<_> = store.open_for_topic("cs", "_synthetic").collect();
        assert_eq!(hits.len(), 1, "only open cs flags should match");
        assert!(hits[0].path.as_deref().unwrap().contains("/cs/"));
    }

    #[test]
    fn resolved_with_follow_up_returns_closed_flags_with_note() {
        let store = FlagStore {
            flags: vec![
                stored(FlagKind::NeedsConfirmation, None, false, Some("The answer is yes.")),
                stored(FlagKind::NeedsConfirmation, None, false, None), // no follow-up
                stored(FlagKind::NeedsConfirmation, None, true, Some("Still open")), // open
            ],
        };
        let hits: Vec<_> = store.resolved_with_follow_up().collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].follow_up.as_deref(), Some("The answer is yes."));
    }

    #[test]
    fn stored_flag_display_kind_matches_variant() {
        assert_eq!(stored(FlagKind::PossibleMisread, None, true, None).display_kind(), "possible misread");
        assert_eq!(stored(FlagKind::NeedsConfirmation, None, true, None).display_kind(), "needs confirmation");
        assert_eq!(stored(FlagKind::UnresolvedQuestion, None, true, None).display_kind(), "unresolved question");
        assert_eq!(stored(FlagKind::Ambiguity, None, true, None).display_kind(), "ambiguity (auto-resolved)");
    }
}
