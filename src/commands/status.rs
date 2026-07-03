use anyhow::Result;
use chrono::Utc;
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::config::{find_vault_root, VaultConfig};
use crate::flags::FlagStore;
use crate::manifest::{Manifest, SessionStatus};
use crate::ui::rainbow;

pub struct StatusArgs {
    pub json: bool,
    pub topic: Option<String>,
}

pub fn run(args: StatusArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;
    let flags_path = vault_root.join(&config.generated_dir).join("flags.json");
    let flag_store = FlagStore::load(&flags_path).unwrap_or_default();

    if let Some(ref name) = args.topic {
        return print_topic(name, &manifest);
    }

    if args.json {
        let payload = build_json(&manifest, &flag_store);
        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }

    print_human(&manifest, &config, &flag_store)
}

// ── JSON output ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusJson {
    pub session_in_progress: bool,
    pub sessions_total: usize,
    pub sessions_processed: usize,
    pub sessions_pending: Vec<String>,
    pub sources_unprocessed: Vec<String>,
    pub topics_total: usize,
    pub topics: Vec<TopicSummary>,
    pub open_actionable_flags: usize,
}

#[derive(Serialize)]
pub struct TopicSummary {
    pub name: String,
    pub atomic_count: usize,
    pub pending_sessions: usize,
    pub open_flags: u32,
}

fn build_json(manifest: &Manifest, flag_store: &FlagStore) -> StatusJson {
    let sessions_pending: Vec<String> = manifest
        .sessions
        .iter()
        .filter(|(_, e)| e.status == SessionStatus::Unprocessed)
        .map(|(id, _)| id.clone())
        .collect();

    let sessions_processed = manifest
        .sessions
        .values()
        .filter(|e| e.status == SessionStatus::Processed)
        .count();

    let sources_unprocessed: Vec<String> = manifest
        .sources
        .iter()
        .filter(|(_, e)| matches!(e.status, crate::manifest::SourceStatus::Unprocessed))
        .map(|(id, _)| id.clone())
        .collect();

    let topics: Vec<TopicSummary> = manifest
        .topics
        .iter()
        .map(|(name, t)| TopicSummary {
            name: name.clone(),
            atomic_count: t.atomic_notes.len(),
            pending_sessions: t.pending_sessions.len(),
            open_flags: t.open_flags,
        })
        .collect();

    StatusJson {
        session_in_progress: manifest.session_in_progress.is_some(),
        sessions_total: manifest.sessions.len(),
        sessions_processed,
        sessions_pending,
        sources_unprocessed,
        topics_total: manifest.topics.len(),
        topics,
        open_actionable_flags: flag_store.count_open_actionable(),
    }
}

// ── Per-topic detail view ─────────────────────────────────────────────────────

fn print_topic(name: &str, manifest: &Manifest) -> Result<()> {
    let entry = manifest.topics.get(name).ok_or_else(|| {
        anyhow::anyhow!(
            "topic '{}' not found in manifest (known: {})",
            name,
            manifest
                .topics
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    println!("{}", name.bold());
    println!("  index    {}", entry.index_note.dimmed());
    println!(
        "  updated  {}",
        entry.last_updated.format("%Y-%m-%d").to_string().dimmed()
    );
    println!("  atomics  {}", entry.atomic_notes.len());
    for path in &entry.atomic_notes {
        println!("    • {}", path.dimmed());
    }

    if !entry.contributing_sessions.is_empty() {
        println!("  sessions");
        for s in &entry.contributing_sessions {
            println!(
                "    {} {}  {}",
                s.date.to_string().dimmed(),
                s.course,
                format!("{:?}", s.relationship).to_lowercase().yellow()
            );
        }
    }

    if !entry.contributing_sources.is_empty() {
        println!("  sources");
        for s in &entry.contributing_sources {
            println!(
                "    {}  {}",
                s.source_id,
                format!("{:?}", s.relationship).to_lowercase().yellow()
            );
        }
    }

    if !entry.pending_sessions.is_empty() {
        println!(
            "  {}",
            format!(
                "pending  {} session(s) not yet integrated:",
                entry.pending_sessions.len()
            )
            .yellow()
        );
        for id in &entry.pending_sessions {
            println!("    • {}", id.yellow());
        }
    }

    if entry.open_flags > 0 {
        println!(
            "  {}",
            format!(
                "⚑ {} open flag{} — run `csnotes flags list`",
                entry.open_flags,
                if entry.open_flags == 1 { "" } else { "s" }
            )
            .red()
        );
    }

    Ok(())
}

// ── Human-readable output ─────────────────────────────────────────────────────

fn print_human(manifest: &Manifest, config: &VaultConfig, flag_store: &FlagStore) -> Result<()> {
    // ── Header ────────────────────────────────────────────────────────────
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    println!(
        "{}  {}",
        rainbow("csnotes"),
        vault_root.display().to_string().dimmed()
    );
    println!();

    // ── In-progress warning ───────────────────────────────────────────────
    if let Some(ref rec) = manifest.session_in_progress {
        println!("{}", "⚠  Session in progress".red().bold());
        println!("   run_id : {}", rec.run_id.dimmed());
        println!(
            "   started: {}",
            rec.started_at
                .format("%Y-%m-%d %H:%M UTC")
                .to_string()
                .dimmed()
        );
        println!("   phase  : {}", rec.phase);
        if let Some(ref err) = rec.error {
            println!("   error  : {}", err.red());
        }
        println!(
            "   Run {} to resume or discard.",
            "`csnotes recover`".bold()
        );
        println!();
    }

    // ── Sessions ──────────────────────────────────────────────────────────
    let unprocessed: Vec<_> = manifest
        .sessions
        .iter()
        .filter(|(_, e)| e.status == SessionStatus::Unprocessed)
        .collect();
    let processed_count = manifest
        .sessions
        .values()
        .filter(|e| e.status == SessionStatus::Processed)
        .count();

    println!(
        "{}  {} total · {} processed · {} pending",
        "Sessions".bold(),
        manifest.sessions.len(),
        processed_count.to_string().green(),
        if unprocessed.is_empty() {
            "0".to_string()
        } else {
            unprocessed.len().to_string().yellow().to_string()
        },
    );

    for (id, entry) in &unprocessed {
        let annotation = if !config.recordings_required_for(&entry.course) {
            String::new()
        } else if entry.recording_missing {
            format!("  {}", "(no recording)".dimmed())
        } else if entry.recording_exports.is_empty() {
            format!("  {}", "(recording missing)".dimmed())
        } else {
            String::new()
        };
        println!(
            "  {} {} — {}{}",
            "•".yellow(),
            id.yellow(),
            entry.date.to_string().dimmed(),
            annotation,
        );
    }

    // ── Sources ───────────────────────────────────────────────────────────
    let unprocessed_sources: Vec<_> = manifest
        .sources
        .iter()
        .filter(|(_, e)| matches!(e.status, crate::manifest::SourceStatus::Unprocessed))
        .collect();

    if !unprocessed_sources.is_empty() {
        println!();
        println!(
            "{}  {} unprocessed",
            "Sources".bold(),
            unprocessed_sources.len().to_string().yellow(),
        );
        for (id, _) in &unprocessed_sources {
            println!("  {} {}", "•".yellow(), id.yellow());
        }
    }

    // ── Topics ────────────────────────────────────────────────────────────
    if !manifest.topics.is_empty() {
        println!();
        println!("{}  {}", "Topics".bold(), manifest.topics.len());
        for (name, topic) in &manifest.topics {
            let pending = if topic.pending_sessions.is_empty() {
                String::new()
            } else {
                format!(
                    "  {}",
                    format!("[{} pending]", topic.pending_sessions.len()).yellow()
                )
            };
            let flags = if topic.open_flags > 0 {
                format!(
                    "  {}",
                    format!(
                        "⚑ {} flag{}",
                        topic.open_flags,
                        if topic.open_flags == 1 { "" } else { "s" }
                    )
                    .red()
                )
            } else {
                String::new()
            };
            println!(
                "  {} — {} atomic{}{}{}",
                name,
                topic.atomic_notes.len(),
                if topic.atomic_notes.len() == 1 {
                    ""
                } else {
                    "s"
                },
                pending,
                flags,
            );
        }
    }

    // ── Archive nudges ────────────────────────────────────────────────────
    let now = Utc::now().date_naive();
    let threshold = chrono::Duration::weeks(config.archive_threshold_weeks as i64);

    for course in &config.active_courses {
        let latest = manifest
            .sessions
            .values()
            .filter(|e| &e.course == course)
            .map(|e| e.date)
            .max();
        if let Some(last_date) = latest {
            let age = now - last_date;
            if age > threshold {
                println!();
                println!(
                    "  {}",
                    format!(
                        "Nudge: '{}' last active {} ({} weeks ago). Consider `csnotes config --archive {}`.",
                        course, last_date, age.num_weeks(), course
                    )
                    .yellow()
                );
            }
        }
    }

    // ── Open flags ────────────────────────────────────────────────────────
    let open_actionable = flag_store.count_open_actionable();
    if open_actionable > 0 {
        println!();
        println!(
            "{}",
            format!(
                "⚑  {} open actionable flag{} — run `csnotes flags list`",
                open_actionable,
                if open_actionable == 1 { "" } else { "s" }
            )
            .red()
            .bold()
        );
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode};
    use crate::manifest::{ManifestConfig, TopicEntry};
    use chrono::Utc;
    use indexmap::IndexMap;

    fn empty_manifest() -> Manifest {
        Manifest::empty(
            std::path::PathBuf::from("/tmp/vault"),
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

    fn topic_entry(atomic_count: usize) -> TopicEntry {
        TopicEntry {
            index_note: "idx.md".into(),
            atomic_notes: (0..atomic_count).map(|i| format!("note{i}.md")).collect(),
            contributing_sessions: vec![],
            contributing_sources: vec![],
            pending_sessions: vec![],
            last_updated: Utc::now(),
            open_flags: 0,
            source_types: vec![],
        }
    }

    #[test]
    fn build_json_empty_manifest() {
        let manifest = empty_manifest();
        let flags = FlagStore::default();
        let out = build_json(&manifest, &flags);
        assert!(!out.session_in_progress);
        assert_eq!(out.sessions_total, 0);
        assert_eq!(out.sessions_processed, 0);
        assert!(out.sessions_pending.is_empty());
        assert!(out.sources_unprocessed.is_empty());
        assert_eq!(out.topics_total, 0);
        assert!(out.topics.is_empty());
        assert_eq!(out.open_actionable_flags, 0);
    }

    #[test]
    fn build_json_counts_topics_and_atomics() {
        let mut manifest = empty_manifest();
        manifest.topics = IndexMap::from([
            ("java".to_string(), topic_entry(3)),
            ("algorithms".to_string(), topic_entry(7)),
        ]);
        let flags = FlagStore::default();
        let out = build_json(&manifest, &flags);
        assert_eq!(out.topics_total, 2);
        let java = out.topics.iter().find(|t| t.name == "java").unwrap();
        assert_eq!(java.atomic_count, 3);
        let alg = out.topics.iter().find(|t| t.name == "algorithms").unwrap();
        assert_eq!(alg.atomic_count, 7);
    }

    #[test]
    fn print_topic_errors_on_unknown_topic() {
        let manifest = empty_manifest();
        let err = print_topic("nonexistent", &manifest).unwrap_err();
        assert!(err.to_string().contains("not found in manifest"));
    }

    #[test]
    fn print_topic_succeeds_for_known_topic() {
        let mut manifest = empty_manifest();
        manifest.topics = IndexMap::from([("java".to_string(), topic_entry(2))]);
        // Should not error — output goes to stdout which we don't capture here,
        // but the return value must be Ok.
        assert!(print_topic("java", &manifest).is_ok());
    }

    #[test]
    fn build_json_serializes_to_valid_json() {
        let manifest = empty_manifest();
        let flags = FlagStore::default();
        let out = build_json(&manifest, &flags);
        let json = serde_json::to_string(&out).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("sessions_total").is_some());
        assert!(parsed.get("topics").is_some());
        assert!(parsed.get("session_in_progress").is_some());
    }
}
