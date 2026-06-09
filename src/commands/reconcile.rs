/// `csnotes reconcile` — Phase 0/1/2.
///
/// Phase 0: scan `raw_dir` for new raw notes, register them as unprocessed
/// sessions; scan `plaud_dir` for export files and attach them to their
/// sessions.
///
/// Phase 1: scan `sources_dir` for source files (.md), derive heading schemes
/// via comrak, and register them as unprocessed `SourceEntry` records.
///
/// Phase 2: scan `artifacts_dir` per course for slides and code samples,
/// match them to sessions by filename prefix, and attach them to
/// `SessionEntry::artifacts`.
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};

use crate::config::{FilenameFormat, VaultConfig, find_vault_root};
use crate::manifest::{
    ArtifactEntry, ArtifactKind, Manifest, PlaudExport, PlaudKind, SessionEntry, SessionStatus,
    SourceEntry, SourceKind, SourceStatus,
};
use crate::markdown::{derive_heading_scheme, parse_headings};

pub struct ReconcileArgs {
    pub notify: bool,
    pub rename_spaces: Option<String>, // "hyphens" | "underscores"
    /// When true, suppress the "nothing new" message (used by auto-reconcile
    /// inside `csnotes process`).
    pub quiet: bool,
}

/// Entry point for `csnotes reconcile` (CLI invocation).
pub fn run(args: ReconcileArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    run_for_vault(&vault_root, &config, args)
}

/// Core reconcile logic.  Called both by `run` (CLI) and by `process`
/// (auto-reconcile before launching the AI).
pub fn run_for_vault(
    vault_root: &Path,
    config: &VaultConfig,
    args: ReconcileArgs,
) -> Result<()> {
    let mut manifest = Manifest::load_or_create(vault_root, config)?;

    let fmt = FilenameFormat::parse(&config.filename_format)?;

    let mut new_sessions: Vec<String> = Vec::new();
    let mut new_plaud: Vec<(String, String)> = Vec::new();   // (session_id, path)
    let mut new_sources: Vec<String> = Vec::new();            // source IDs
    let mut new_artifacts: Vec<(String, String)> = Vec::new();// (session_id, path)
    let mut space_warnings: Vec<PathBuf> = Vec::new();

    // ── Resolve course roots ──────────────────────────────────────────────────
    // If `active_courses` is set, raw notes and Plaud exports live under
    // `{course}/{raw_dir}/` for each course.  Otherwise fall back to the flat
    // `{raw_dir}/` layout (used in tests and single-course vaults).
    let course_roots: Vec<PathBuf> = if config.active_courses.is_empty() {
        vec![vault_root.to_path_buf()]
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
                vault_root,
                config,
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
                vault_root,
                config,
                &fmt,
                &args,
                &mut manifest,
                &mut new_plaud,
                &mut space_warnings,
            )?;
        }
    }

    // ── Scan artifacts_dir ────────────────────────────────────────────────────
    for course_root in &course_roots {
        let artifacts_dir = course_root.join(&config.artifacts_dir);
        if artifacts_dir.exists() {
            scan_artifacts_dir(
                &artifacts_dir,
                vault_root,
                &mut manifest,
                &mut new_artifacts,
            )?;
        }
    }

    // ── Scan sources_dir ──────────────────────────────────────────────────────
    let sources_dir = vault_root.join(&config.sources_dir);
    if sources_dir.exists() {
        scan_sources_dir(&sources_dir, vault_root, &mut manifest, &mut new_sources)?;
    }

    // ── Space warnings ────────────────────────────────────────────────────────
    for path in &space_warnings {
        eprintln!(
            "  warn: filename has spaces: {} (use --rename-spaces to fix)",
            path.display()
        );
    }

    // ── Save + report ─────────────────────────────────────────────────────────
    manifest.save(vault_root)?;

    let nothing_new = new_sessions.is_empty()
        && new_plaud.is_empty()
        && new_sources.is_empty()
        && new_artifacts.is_empty()
        && space_warnings.is_empty();

    if nothing_new {
        if !args.quiet {
            println!("reconcile: nothing new.");
        }
    } else {
        for id in &new_sessions {
            println!("  + session   {}", id);
        }
        for (session_id, path) in &new_plaud {
            println!("  + plaud     {} → {}", path, session_id);
        }
        for (session_id, path) in &new_artifacts {
            println!("  + artifact  {} → {}", path, session_id);
        }
        for id in &new_sources {
            println!("  + source    {}", id);
        }
        if !space_warnings.is_empty() {
            println!("  {} file(s) have spaces in their names", space_warnings.len());
        }
    }

    // ── Desktop notification (best-effort) ────────────────────────────────────
    if args.notify && !nothing_new {
        let msg = format!(
            "{} new session(s), {} Plaud, {} artifact(s), {} source(s)",
            new_sessions.len(),
            new_plaud.len(),
            new_artifacts.len(),
            new_sources.len(),
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

// ── Artifact scanning ─────────────────────────────────────────────────────────

/// Text-readable file extensions we'll wrap and pass to the AI.
/// Binary formats (PDF, images, office docs) are skipped — they can't be
/// read as UTF-8 and the AI can't use them directly.
const TEXT_ARTIFACT_EXTENSIONS: &[&str] = &[
    // Markup / documentation
    "md", "txt", "html", "htm", "tex",
    // Code
    "py", "java", "rs", "js", "ts", "jsx", "tsx",
    "c", "cpp", "h", "hpp", "cc", "cxx",
    "go", "rb", "swift", "kt", "kts",
    "cs", "fs", "ml", "mli", "hs", "lhs",
    "r", "rmd", "sql", "sh", "bash", "zsh", "fish",
    "yaml", "yml", "toml", "json", "xml", "csv",
    "ipynb",
];

/// Extensions that signal lecture slides / handouts (text-format only).
const SLIDE_EXTENSIONS: &[&str] = &["md", "html", "htm", "tex", "txt"];

/// Qualifier keywords (in the filename suffix) that override kind → Slides.
const SLIDE_QUALIFIERS: &[&str] = &[
    "slides", "slide", "deck", "handout", "handouts", "lecture", "notes",
];

/// Walk `{course}/{artifacts_dir}/` for text-readable files whose stems start
/// with a known session ID and attach them as `ArtifactEntry` records.
///
/// Matching rule (same spirit as Plaud scanning):
///   `{session_id}.{ext}`       → attached, no qualifier
///   `{session_id}-{rest}.{ext}`→ attached, qualifier = rest
///
/// Kind classification (in priority order):
/// 1. If qualifier contains a slide keyword → Slides
/// 2. If extension is in `SLIDE_EXTENSIONS` and no qualifier (bare session) → Slides
/// 3. If extension is a code extension → Code
/// 4. Otherwise → Other
fn scan_artifacts_dir(
    artifacts_dir: &Path,
    vault_root: &Path,
    manifest: &mut Manifest,
    new_artifacts: &mut Vec<(String, String)>,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(artifacts_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // Only text-readable formats.
        let ext = match path.extension().and_then(|x| x.to_str()) {
            Some(e) => e.to_ascii_lowercase(),
            None => continue,
        };
        if !TEXT_ARTIFACT_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // Try to match stem → (session_id, qualifier).
        let (session_id, qualifier): (&str, &str) =
            if manifest.sessions.contains_key(stem) {
                // Bare `{session_id}.{ext}` — no qualifier.
                (stem, "")
            } else if let Some((prefix, suffix)) = stem.split_once('-').and_then(|_| {
                // We want the longest matching session_id prefix, so we try
                // `rsplit_once` which gives us the last `-` split.  If the
                // prefix is a known session, use it.
                stem.rsplit_once('-')
            }) {
                if manifest.sessions.contains_key(prefix) {
                    (prefix, suffix)
                } else {
                    continue; // prefix not a session — skip
                }
            } else {
                continue; // no `-` in stem and not an exact session match
            };

        let entry_in_manifest = match manifest.sessions.get_mut(session_id) {
            Some(e) => e,
            None => continue,
        };

        let rel_path = path
            .strip_prefix(vault_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Skip if already recorded.
        if entry_in_manifest.artifacts.iter().any(|a| a.path == rel_path) {
            continue;
        }

        let kind = classify_artifact_kind(&ext, qualifier);
        new_artifacts.push((session_id.to_string(), rel_path.clone()));
        entry_in_manifest.artifacts.push(ArtifactEntry {
            path: rel_path,
            kind,
        });
    }
    Ok(())
}

fn classify_artifact_kind(ext: &str, qualifier: &str) -> ArtifactKind {
    let q_lower = qualifier.to_ascii_lowercase();

    // Explicit slide qualifier in the filename suffix.
    if SLIDE_QUALIFIERS.iter().any(|kw| q_lower.contains(kw)) {
        return ArtifactKind::Slides;
    }
    // Markdown / text with no qualifier → treat as slides/handout by default.
    if qualifier.is_empty() && SLIDE_EXTENSIONS.contains(&ext) {
        return ArtifactKind::Slides;
    }
    // Code extensions.
    let code_exts = &[
        "py", "java", "rs", "js", "ts", "jsx", "tsx",
        "c", "cpp", "h", "hpp", "cc", "cxx",
        "go", "rb", "swift", "kt", "kts",
        "cs", "fs", "ml", "mli", "hs", "lhs",
        "r", "rmd", "sql", "sh", "bash", "zsh", "fish",
        "ipynb",
    ];
    if code_exts.contains(&ext) {
        return ArtifactKind::Code;
    }
    ArtifactKind::Other
}

// ── Source scanning ───────────────────────────────────────────────────────────

/// Walk `sources_dir` for `.md` files and register any not already in the
/// manifest.
///
/// Source IDs follow the path structure relative to `sources_dir`:
/// - Flat file:       `{stem}`           e.g. `SICP-ch01`
/// - In subdirectory: `{subdir}/{stem}`  e.g. `SICP/SICP-ch01`
///
/// Heading schemes are derived immediately via `comrak` so the AI can
/// reference textbook locations precisely in session briefings.
fn scan_sources_dir(
    sources_dir: &Path,
    vault_root: &Path,
    manifest: &mut Manifest,
    new_sources: &mut Vec<String>,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(sources_dir)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }

        // Derive the source ID from the path relative to sources_dir.
        let rel_to_sources = path
            .strip_prefix(sources_dir)
            .unwrap_or(path);
        let source_id = match (
            rel_to_sources.parent().and_then(|p| p.to_str()),
            path.file_stem().and_then(|s| s.to_str()),
        ) {
            (Some(parent), Some(stem)) if !parent.is_empty() => {
                format!("{}/{}", parent, stem)
            }
            (_, Some(stem)) => stem.to_string(),
            _ => continue,
        };

        if manifest.sources.contains_key(&source_id) {
            continue; // already registered
        }

        let rel_path = path
            .strip_prefix(vault_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Derive heading scheme via comrak.
        let heading_scheme = match std::fs::read_to_string(path) {
            Ok(content) => derive_heading_scheme(&parse_headings(&content)),
            Err(_) => vec![],
        };

        manifest.sources.insert(
            source_id.clone(),
            SourceEntry {
                path: rel_path,
                kind: SourceKind::Textbook, // default; user can update via config
                status: SourceStatus::Unprocessed,
                last_processed_at: None,
                heading_scheme,
                topics_updated: vec![],
            },
        );
        new_sources.push(source_id);
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestConfig, SessionEntry, SessionStatus};
    use crate::config::{AiBackend, SkillVariant, SnapshotMode};
    use chrono::NaiveDate;
    use tempfile::TempDir;

    fn make_manifest_with_session(vault_root: &std::path::Path, session_id: &str) -> Manifest {
        let cfg = ManifestConfig {
            raw_dir: "notes".into(),
            plaud_dir: "plaud".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            default_backend: AiBackend::Mock,
            skill_variant: SkillVariant::Claude,
            snapshot_mode: SnapshotMode::PreMerge,
        };
        let mut m = Manifest::empty(vault_root.to_path_buf(), cfg);
        m.sessions.insert(
            session_id.to_string(),
            SessionEntry {
                date: NaiveDate::from_ymd_opt(2026, 9, 3).unwrap(),
                course: "CPSC5001".into(),
                filename_format: "{course}-{mm}-{dd}".into(),
                raw_note: format!("notes/{}.md", session_id),
                plaud_exports: vec![],
                artifacts: vec![],
                plaud_missing: false,
                status: SessionStatus::Unprocessed,
                processed_at: None,
                topics_updated: vec![],
            },
        );
        m
    }

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), content).unwrap();
    }

    // ── classify_artifact_kind ────────────────────────────────────────────────

    #[test]
    fn classify_slide_by_qualifier() {
        assert_eq!(classify_artifact_kind("md", "slides"), ArtifactKind::Slides);
        assert_eq!(classify_artifact_kind("md", "handout"), ArtifactKind::Slides);
        assert_eq!(classify_artifact_kind("py", "slides"), ArtifactKind::Slides);
    }

    #[test]
    fn classify_slide_by_bare_md() {
        // Bare .md with no qualifier → treat as slide/handout material
        assert_eq!(classify_artifact_kind("md", ""), ArtifactKind::Slides);
        assert_eq!(classify_artifact_kind("html", ""), ArtifactKind::Slides);
    }

    #[test]
    fn classify_code_by_extension() {
        assert_eq!(classify_artifact_kind("py", "BST"), ArtifactKind::Code);
        assert_eq!(classify_artifact_kind("java", "Node"), ArtifactKind::Code);
        assert_eq!(classify_artifact_kind("rs", ""), ArtifactKind::Code);
    }

    #[test]
    fn classify_other() {
        assert_eq!(classify_artifact_kind("csv", "data"), ArtifactKind::Other);
        assert_eq!(classify_artifact_kind("json", "schema"), ArtifactKind::Other);
    }

    // ── scan_artifacts_dir ────────────────────────────────────────────────────

    #[test]
    fn scan_attaches_slide_by_qualifier() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-slides.md", "# Slides");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts).unwrap();

        assert_eq!(new_artifacts.len(), 1);
        assert_eq!(new_artifacts[0].0, "CPSC5001-09-03");
        let entry = &manifest.sessions["CPSC5001-09-03"];
        assert_eq!(entry.artifacts.len(), 1);
        assert_eq!(entry.artifacts[0].kind, ArtifactKind::Slides);
    }

    #[test]
    fn scan_attaches_code_by_extension() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-BinarySearch.java", "class BinarySearch {}");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts).unwrap();

        assert_eq!(new_artifacts.len(), 1);
        assert_eq!(manifest.sessions["CPSC5001-09-03"].artifacts[0].kind, ArtifactKind::Code);
    }

    #[test]
    fn scan_skips_binary_extensions() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-slides.pdf", "%PDF content");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts).unwrap();

        assert!(new_artifacts.is_empty(), "PDF should be skipped");
    }

    #[test]
    fn scan_skips_unmatched_stems() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "BinarySearch.java", "class BinarySearch {}");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts).unwrap();

        assert!(new_artifacts.is_empty(), "Unmatched stem should be skipped");
    }

    #[test]
    fn scan_idempotent() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-slides.md", "# Slides");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts).unwrap();
        // Run again — should not double-register.
        let mut new_artifacts2 = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts2).unwrap();

        assert!(new_artifacts2.is_empty(), "Second scan should add nothing");
        assert_eq!(manifest.sessions["CPSC5001-09-03"].artifacts.len(), 1);
    }

    #[test]
    fn scan_bare_session_id_md_is_slides() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        // File named exactly as the session ID (no qualifier suffix)
        write_file(&artifacts_dir, "CPSC5001-09-03.md", "# Lecture notes");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(&artifacts_dir, tmp.path(), &mut manifest, &mut new_artifacts).unwrap();

        assert_eq!(new_artifacts.len(), 1);
        assert_eq!(manifest.sessions["CPSC5001-09-03"].artifacts[0].kind, ArtifactKind::Slides);
    }
}

fn notify(message: &str) {
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
    #[cfg(target_os = "linux")]
    {
        // notify-send is provided by libnotify-bin on Debian/Ubuntu and
        // equivalent packages on other distros.  Best-effort: silently skip
        // if not installed.
        let _ = std::process::Command::new("notify-send")
            .args(["csnotes", message])
            .status();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = message;
    }
}
