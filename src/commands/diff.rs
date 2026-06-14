/// `csnotes diff` — show a summary of what changed in a session.
///
/// Reads from `_generated/reports/<session-id>.json` when `--session` is
/// given, or `_generated/last_report.json` otherwise.
use anyhow::{bail, Result};

use crate::config::{find_vault_root, VaultConfig};
use crate::manifest::Manifest;
use crate::report::{Op, SessionReport};

pub struct DiffArgs {
    pub session: Option<String>,
}

pub fn run(args: DiffArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;

    // ── Locate the report ─────────────────────────────────────────────────────
    let report_path = if let Some(ref session_id) = args.session {
        // Resolve partial session IDs (date-only, course-prefixed, etc.)
        let full_id = resolve_session_id(session_id, &manifest)?;
        let p = manifest.session_report_path(&full_id);
        if !p.exists() {
            bail!(
                "No report found for session '{}'. \
                 Reports are only kept for sessions processed with this version of csnotes.",
                full_id
            );
        }
        p
    } else {
        let p = manifest.last_report_path();
        if !p.exists() {
            println!("No session report found. Run `csnotes process` first.");
            return Ok(());
        }
        p
    };

    let content = std::fs::read_to_string(&report_path)?;
    let report: SessionReport = serde_json::from_str(&content)?;

    // ── Header ────────────────────────────────────────────────────────────────
    let session_label = report.scope.sessions.join(", ");
    println!(
        "Session: {}  ({})",
        session_label,
        report.completed_at.format("%Y-%m-%d %H:%M UTC")
    );
    println!("Backend: {}  run_id: {}", report.backend, report.run_id);

    let synthetic_dir = &config.synthetic_dir;

    // ── Operations ────────────────────────────────────────────────────────────
    let creates: Vec<_> = report
        .operations
        .iter()
        .filter_map(|op| {
            if let Op::CreateNote(o) = op {
                Some(o)
            } else {
                None
            }
        })
        .collect();
    let updates: Vec<_> = report
        .operations
        .iter()
        .filter_map(|op| {
            if let Op::UpdateNote(o) = op {
                Some(o)
            } else {
                None
            }
        })
        .collect();
    let structural: Vec<_> = report
        .operations
        .iter()
        .filter(|op| op.is_structural())
        .collect();

    if !creates.is_empty() {
        println!("\nCreated ({}):", creates.len());
        for op in &creates {
            let display = op.path.trim_start_matches(&format!("{}/", synthetic_dir));
            println!("  + {}  — {}", display, op.change_summary);
            if !op.embed_in.is_empty() {
                for idx in &op.embed_in {
                    let idx_display = idx.trim_start_matches(&format!("{}/", synthetic_dir));
                    println!("      ↳ embedded in {}", idx_display);
                }
            }
        }
    }

    if !updates.is_empty() {
        println!("\nUpdated ({}):", updates.len());
        for op in &updates {
            let display = op.path.trim_start_matches(&format!("{}/", synthetic_dir));
            println!("  ~ {}  — {}", display, op.change_summary);
            if !op.sections.is_empty() {
                println!("      sections: {}", op.sections.join(", "));
            }
        }
    }

    if !structural.is_empty() {
        println!("\nStructural ops ({}):", structural.len());
        for op in &structural {
            println!("  * {}", op.kind_str());
        }
    }

    // ── Flags ─────────────────────────────────────────────────────────────────
    let actionable: Vec<_> = report
        .review_flags
        .iter()
        .filter(|f| f.kind.is_actionable())
        .collect();
    let threads: Vec<_> = report
        .review_flags
        .iter()
        .filter(|f| f.kind.is_thread())
        .collect();

    if !actionable.is_empty() {
        println!("\nActionable flags ({}):", actionable.len());
        for flag in &actionable {
            println!("  ! [{}] {}", flag.kind.tier_label(), flag.message);
            if let Some(ref p) = flag.path {
                let display = p.trim_start_matches(&format!("{}/", synthetic_dir));
                println!("      {}", display);
            }
        }
    }

    if !threads.is_empty() {
        println!("\nOpen questions ({}):", threads.len());
        for flag in &threads {
            println!("  ? {}", flag.message);
        }
    }

    if report.operations.is_empty() && report.review_flags.is_empty() {
        println!("\n(no operations or flags recorded)");
    }

    Ok(())
}

// ── Session ID resolution ─────────────────────────────────────────────────────

fn resolve_session_id(input: &str, manifest: &Manifest) -> Result<String> {
    // Exact match first
    if manifest.sessions.contains_key(input) {
        return Ok(input.to_string());
    }
    // Partial date match (e.g. "09-03" matches "CPSC5001-09-03")
    let matches: Vec<_> = manifest
        .sessions
        .keys()
        .filter(|id| id.ends_with(input))
        .cloned()
        .collect();
    match matches.len() {
        0 => bail!("No session matching '{}' found in manifest.", input),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => bail!(
            "'{}' matches multiple sessions: {}. Use the full session ID.",
            input,
            matches.join(", ")
        ),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode};
    use crate::manifest::{ManifestConfig, SessionEntry, SessionStatus};
    use chrono::NaiveDate;

    fn make_manifest(sessions: &[&str]) -> Manifest {
        let cfg = ManifestConfig {
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
        };
        let mut m = Manifest::empty(std::path::PathBuf::from("/tmp"), cfg);
        for &id in sessions {
            let parts: Vec<&str> = id.splitn(2, '-').collect();
            let course = parts[0].to_string();
            let date = NaiveDate::parse_from_str(&id[course.len() + 1..], "%Y-%m-%d")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
            m.sessions.insert(
                id.to_string(),
                SessionEntry {
                    date,
                    course,
                    filename_format: "{course}-{mm}-{dd}".into(),
                    raw_note: format!("notes/{}.md", id),
                    recording_exports: vec![],
                    artifacts: vec![],
                    recording_missing: false,
                    status: SessionStatus::Unprocessed,
                    processed_at: None,
                    topics_updated: vec![],
                },
            );
        }
        m
    }

    #[test]
    fn resolve_session_id_exact_match() {
        let m = make_manifest(&["CPSC5001-2026-09-03"]);
        assert_eq!(
            resolve_session_id("CPSC5001-2026-09-03", &m).unwrap(),
            "CPSC5001-2026-09-03"
        );
    }

    #[test]
    fn resolve_session_id_suffix_match() {
        let m = make_manifest(&["CPSC5001-2026-09-03"]);
        assert_eq!(
            resolve_session_id("09-03", &m).unwrap(),
            "CPSC5001-2026-09-03"
        );
    }

    #[test]
    fn resolve_session_id_no_match_errors() {
        let m = make_manifest(&["CPSC5001-2026-09-03"]);
        assert!(resolve_session_id("06-15", &m).is_err());
    }

    #[test]
    fn resolve_session_id_multiple_matches_errors() {
        let m = make_manifest(&["CPSC5001-2026-09-03", "CPSC5002-2026-09-03"]);
        assert!(resolve_session_id("09-03", &m).is_err());
    }
}
