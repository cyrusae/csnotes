use anyhow::Result;
use chrono::Utc;

use crate::config::{VaultConfig, find_vault_root};
use crate::flags::FlagStore;
use crate::manifest::{Manifest, SessionStatus};

pub fn run() -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;
    let flags_path = vault_root.join(&config.generated_dir).join("flags.json");
    let flag_store = FlagStore::load(&flags_path).unwrap_or_default();

    // ── In-progress warning ───────────────────────────────────────────────
    if let Some(ref rec) = manifest.session_in_progress {
        println!("⚠  Session in progress (run_id: {})", rec.run_id);
        println!("   Started: {}", rec.started_at.format("%Y-%m-%d %H:%M UTC"));
        println!("   Phase: {}", rec.phase);
        if let Some(ref err) = rec.error {
            println!("   Error: {}", err);
        }
        println!("   Run `csnotes recover` to resume or discard.\n");
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

    println!("Sessions: {} total, {} processed, {} pending",
        manifest.sessions.len(), processed_count, unprocessed.len());

    for (id, entry) in &unprocessed {
        let plaud = if entry.plaud_missing {
            " (no Plaud)"
        } else if entry.plaud_exports.is_empty() {
            " (Plaud missing)"
        } else {
            ""
        };
        println!("  • {} — {}{}", id, entry.date, plaud);
    }

    // ── Sources ───────────────────────────────────────────────────────────
    let unprocessed_sources: Vec<_> = manifest
        .sources
        .iter()
        .filter(|(_, e)| {
            matches!(e.status, crate::manifest::SourceStatus::Unprocessed)
        })
        .collect();

    if !unprocessed_sources.is_empty() {
        println!("\nSources: {} unprocessed", unprocessed_sources.len());
        for (id, _) in &unprocessed_sources {
            println!("  • {}", id);
        }
    }

    // ── Topics ────────────────────────────────────────────────────────────
    if !manifest.topics.is_empty() {
        println!("\nTopics: {}", manifest.topics.len());
        for (name, topic) in &manifest.topics {
            let pending = if topic.pending_sessions.is_empty() {
                String::new()
            } else {
                format!(" [{} pending]", topic.pending_sessions.len())
            };
            let flags = if topic.open_flags > 0 {
                format!(" ⚑ {} flag{}", topic.open_flags, if topic.open_flags == 1 { "" } else { "s" })
            } else {
                String::new()
            };
            println!("  {} — {} atomic(s){}{}",
                name,
                topic.atomic_notes.len(),
                pending,
                flags,
            );
        }
    }

    // ── Archive nudges ────────────────────────────────────────────────────
    let now = Utc::now().date_naive();
    let threshold = chrono::Duration::weeks(config.archive_threshold_weeks as i64);

    for course in &config.active_courses {
        // Check the latest raw note date for this course
        let latest = manifest
            .sessions
            .values()
            .filter(|e| &e.course == course)
            .map(|e| e.date)
            .max();
        if let Some(last_date) = latest {
            let age = now - last_date;
            if age > threshold {
                println!(
                    "\n  Nudge: '{}' last active {} ({} weeks ago). \
                     Consider `csnotes config --archive {}`.",
                    course,
                    last_date,
                    age.num_weeks(),
                    course
                );
            }
        }
    }

    // ── Open flags ────────────────────────────────────────────────────────
    let open_actionable = flag_store.count_open_actionable();
    if open_actionable > 0 {
        println!(
            "\n⚑  {} open actionable flag{} — run `csnotes flags list`",
            open_actionable,
            if open_actionable == 1 { "" } else { "s" }
        );
    }

    Ok(())
}
