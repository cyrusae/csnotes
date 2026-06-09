/// `csnotes reconcile` — Phase 0 subset.
///
/// Phase 0: scan `raw_dir` for new raw notes, register them as unprocessed
/// sessions; scan `plaud_dir` for export files and attach them to their
/// sessions.
///
/// Phase 2 will add: source registration, artifact detection, LiveSync window
/// awareness, full `--rename-spaces` pipeline.
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};

use crate::config::{FilenameFormat, VaultConfig, find_vault_root};
use crate::manifest::{
    Manifest, PlaudExport, PlaudKind, SessionEntry, SessionStatus,
};

pub struct ReconcileArgs {
    pub notify: bool,
    pub rename_spaces: Option<String>, // "hyphens" | "underscores"
}

pub fn run(args: ReconcileArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let mut manifest = Manifest::load_or_create(&vault_root, &config)?;

    let fmt = FilenameFormat::parse(&config.filename_format)?;

    let mut new_sessions: Vec<String> = Vec::new();
    let mut new_plaud: Vec<(String, String)> = Vec::new(); // (session_id, path)
    let mut space_warnings: Vec<PathBuf> = Vec::new();

    // ── Resolve course roots ──────────────────────────────────────────────────
    // If `active_courses` is set, raw notes and Plaud exports live under
    // `{course}/{raw_dir}/` for each course.  Otherwise fall back to the flat
    // `{raw_dir}/` layout (used in tests and single-course vaults).
    let course_roots: Vec<PathBuf> = if config.active_courses.is_empty() {
        vec![vault_root.clone()]
    } else {
        config
            .active_courses
            .iter()
            .map(|c| vault_root.join(c))
            .collect()
    };

    // ── Scan raw_dir ──────────────────────────────────────────────────────────
    for course_root in &course_roots {
        let raw_dir = course_root.join(&config.raw_dir);
        if raw_dir.exists() {
            scan_raw_dir(
                &raw_dir,
                &vault_root,
                &config,
                &fmt,
                &args,
                &mut manifest,
                &mut new_sessions,
                &mut space_warnings,
            )?;
        }
    }

    // ── Scan plaud_dir ────────────────────────────────────────────────────────
    for course_root in &course_roots {
        let plaud_dir = course_root.join(&config.plaud_dir);
        if plaud_dir.exists() {
            scan_plaud_dir(
                &plaud_dir,
                &vault_root,
                &config,
                &fmt,
                &args,
                &mut manifest,
                &mut new_plaud,
                &mut space_warnings,
            )?;
        }
    }

    // ── Space warnings ────────────────────────────────────────────────────────
    for path in &space_warnings {
        eprintln!(
            "  warn: filename has spaces: {} (use --rename-spaces to fix)",
            path.display()
        );
    }

    // ── Save + report ─────────────────────────────────────────────────────────
    manifest.save(&vault_root)?;

    if new_sessions.is_empty() && new_plaud.is_empty() && space_warnings.is_empty() {
        println!("reconcile: nothing new.");
    } else {
        for id in &new_sessions {
            println!("  + session  {}", id);
        }
        for (session_id, path) in &new_plaud {
            println!("  + plaud    {} → {}", path, session_id);
        }
        if !space_warnings.is_empty() {
            println!("  {} file(s) have spaces in their names", space_warnings.len());
        }
    }

    // ── Desktop notification (macOS only, best-effort) ────────────────────────
    if args.notify && (!new_sessions.is_empty() || !new_plaud.is_empty()) {
        let msg = format!(
            "{} new session(s), {} new Plaud export(s)",
            new_sessions.len(),
            new_plaud.len()
        );
        notify(&msg);
    }

    Ok(())
}

// ── Raw note scanning ─────────────────────────────────────────────────────────

fn scan_raw_dir(
    raw_dir: &Path,
    vault_root: &Path,
    config: &VaultConfig,
    fmt: &FilenameFormat,
    args: &ReconcileArgs,
    manifest: &mut Manifest,
    new_sessions: &mut Vec<String>,
    space_warnings: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(raw_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }

        // Handle spaces
        let path = match handle_spaces(path, args, space_warnings)? {
            Some(renamed) => renamed,
            None => path.to_path_buf(),
        };
        let path = path.as_path();

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // Must parse against the configured filename format
        let parsed = match fmt.try_parse_stem(stem) {
            Some(p) => p,
            None => continue,
        };

        let date = match build_date(&parsed) {
            Some(d) => d,
            None => continue,
        };

        let session_id = stem.to_string();

        if manifest.sessions.contains_key(&session_id) {
            continue; // already registered
        }

        let rel_path = path
            .strip_prefix(vault_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        manifest.sessions.insert(
            session_id.clone(),
            SessionEntry {
                date,
                course: parsed.course.clone(),
                filename_format: config.filename_format.clone(),
                raw_note: rel_path,
                plaud_exports: vec![],
                artifacts: vec![],
                plaud_missing: false,
                status: SessionStatus::Unprocessed,
                processed_at: None,
                topics_updated: vec![],
            },
        );
        new_sessions.push(session_id);
    }
    Ok(())
}

// ── Plaud export scanning ─────────────────────────────────────────────────────

fn scan_plaud_dir(
    plaud_dir: &Path,
    vault_root: &Path,
    config: &VaultConfig,
    fmt: &FilenameFormat,
    args: &ReconcileArgs,
    manifest: &mut Manifest,
    new_plaud: &mut Vec<(String, String)>,
    space_warnings: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(plaud_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }

        let path = match handle_spaces(path, args, space_warnings)? {
            Some(renamed) => renamed,
            None => path.to_path_buf(),
        };
        let path = path.as_path();

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // A Plaud file is `{session_stem}-{qualifier}`.
        // Split on the last `-` and check if the qualifier is recognized
        // and the prefix is a known session ID.
        let (session_id, qualifier) = match stem.rsplit_once('-') {
            Some((prefix, suffix)) => (prefix, suffix),
            None => continue,
        };

        if !config.is_plaud_qualifier(qualifier) {
            continue;
        }

        // The prefix must match the filename format (i.e. be a valid raw note stem)
        if fmt.try_parse_stem(session_id).is_none() {
            continue;
        }

        // Session must exist in manifest
        let entry = match manifest.sessions.get_mut(session_id) {
            Some(e) => e,
            None => continue,
        };

        let rel_path = path
            .strip_prefix(vault_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Skip if already recorded
        if entry.plaud_exports.iter().any(|p| p.path == rel_path) {
            continue;
        }

        let kind = plaud_kind(qualifier, config);
        new_plaud.push((session_id.to_string(), rel_path.clone()));
        entry.plaud_exports.push(PlaudExport { path: rel_path, kind });
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a `NaiveDate` from a parsed stem.  Fills in the current year when
/// the format doesn't include `{yyyy}`.
fn build_date(parsed: &crate::config::ParsedStem) -> Option<NaiveDate> {
    let year = parsed.year.unwrap_or_else(|| Utc::now().date_naive().year());
    let month = parsed.month?;
    let day = parsed.day?;
    NaiveDate::from_ymd_opt(year, month, day)
}

/// Detect (and optionally rename) files whose names contain spaces.
/// Returns `Some(new_path)` if a rename was performed, `None` otherwise.
fn handle_spaces(
    path: &Path,
    args: &ReconcileArgs,
    space_warnings: &mut Vec<PathBuf>,
) -> Result<Option<PathBuf>> {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return Ok(None),
    };
    if !name.contains(' ') {
        return Ok(None);
    }

    match &args.rename_spaces {
        Some(style) => {
            let replacement = if style == "underscores" { '_' } else { '-' };
            let new_name = name.replace(' ', &replacement.to_string());
            let new_path = path.with_file_name(&new_name);
            std::fs::rename(path, &new_path)?;
            Ok(Some(new_path))
        }
        None => {
            space_warnings.push(path.to_path_buf());
            Ok(None)
        }
    }
}

fn plaud_kind(qualifier: &str, config: &VaultConfig) -> PlaudKind {
    match qualifier {
        "transcript" => PlaudKind::Transcript,
        "summary" => PlaudKind::Summary,
        "mindmap" => PlaudKind::Mindmap,
        q if q.len() == 1 && q.chars().next().map_or(false, |c| c.is_ascii_lowercase()) => {
            PlaudKind::Anonymous
        }
        _ => {
            // Must be a custom qualifier from config.plaud_qualifiers
            let _ = config; // used implicitly via is_plaud_qualifier upstream
            PlaudKind::Custom
        }
    }
}

fn notify(message: &str) {
    // macOS: osascript; other platforms: no-op for now (Phase 2: notify-rust)
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"csnotes\"",
                    message
                ),
            ])
            .status();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = message; // suppress unused warning
    }
}
