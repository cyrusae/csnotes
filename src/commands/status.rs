use anyhow::Result;
use chrono::Utc;
use owo_colors::OwoColorize;

use crate::config::{find_vault_root, VaultConfig};
use crate::flags::FlagStore;
use crate::manifest::{Manifest, SessionStatus};
use crate::ui::rainbow;

pub fn run() -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;
    let flags_path = vault_root.join(&config.generated_dir).join("flags.json");
    let flag_store = FlagStore::load(&flags_path).unwrap_or_default();

    // ── Header ────────────────────────────────────────────────────────────
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
