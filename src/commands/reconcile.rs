/// `csnotes reconcile` — Phase 0/1/2.
///
/// Phase 0: scan `raw_dir` for new raw notes, register them as unprocessed
/// sessions; scan `recordings_dir` for export files and attach them to their
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

use crate::config::{find_vault_root, FilenameFormat, VaultConfig};
use crate::manifest::{
    ArtifactEntry, ArtifactKind, Manifest, ManifestLock, RecordingExport, RecordingKind,
    SessionEntry, SessionStatus, SourceEntry, SourceKind, SourceStatus,
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
    let _lock = ManifestLock::acquire(&vault_root)?;
    run_for_vault(&vault_root, &config, args)
}

/// Core reconcile logic.  Called both by `run` (CLI) and by `process`
/// (auto-reconcile before launching the AI).
pub fn run_for_vault(vault_root: &Path, config: &VaultConfig, args: ReconcileArgs) -> Result<()> {
    let mut manifest = Manifest::load_or_create(vault_root, config)?;

    let fmt = FilenameFormat::parse(&config.filename_format)?;

    let mut new_sessions: Vec<String> = Vec::new();
    let mut new_recordings: Vec<(String, String)> = Vec::new(); // (session_id, path)
    let mut new_sources: Vec<String> = Vec::new(); // source IDs
    let mut new_artifacts: Vec<(String, String)> = Vec::new(); // (session_id, path)
    let mut space_warnings: Vec<PathBuf> = Vec::new();

    // ── Resolve course roots ──────────────────────────────────────────────────
    // If `active_courses` is set, raw notes and recording exports live under
    // `{course}/{raw_dir}/` for each course.  Otherwise fall back to the flat
    // `{raw_dir}/` layout (used in tests and single-course vaults).
    // Each entry is (Option<course_name>, path) so per-course recording checks work.
    let course_entries: Vec<(Option<String>, PathBuf)> = if config.active_courses.is_empty() {
        vec![(None, vault_root.to_path_buf())]
    } else {
        config
            .active_courses
            .iter()
            .map(|c| (Some(c.clone()), vault_root.join(c)))
            .collect()
    };
    // Flat view used by scans that don't need the course name.
    let course_roots: Vec<PathBuf> = course_entries.iter().map(|(_, p)| p.clone()).collect();

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

    // ── Scan recordings_dir ──────────────────────────────────────────────────
    for (course_name, course_root) in &course_entries {
        let required = match course_name.as_deref() {
            Some(c) => config.recordings_required_for(c),
            None => config.require_recordings,
        };
        if !required {
            continue;
        }
        let recordings_dir = course_root.join(&config.recordings_dir);
        if recordings_dir.exists() {
            scan_recordings_dir(
                &recordings_dir,
                vault_root,
                config,
                &fmt,
                &args,
                &mut manifest,
                &mut new_recordings,
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
        scan_sources_dir(
            &sources_dir,
            vault_root,
            &mut manifest,
            config.scan_ai_conversations,
            &config.sources_ignore_dirs,
            &mut new_sources,
        )?;
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
        && new_recordings.is_empty()
        && new_sources.is_empty()
        && new_artifacts.is_empty()
        && space_warnings.is_empty();

    if nothing_new {
        if !args.quiet {
            use owo_colors::OwoColorize;
            println!("{}", "reconcile: nothing new.".dimmed());
        }
    } else {
        use owo_colors::OwoColorize;
        for id in &new_sessions {
            println!("  {} {}   {}", "+".green().bold(), "session".dimmed(), id);
        }
        for (session_id, path) in &new_recordings {
            println!(
                "  {} {}  {} {} {}",
                "+".green().bold(),
                "recording".dimmed(),
                path,
                "→".dimmed(),
                session_id
            );
        }
        for (session_id, path) in &new_artifacts {
            println!(
                "  {} {}  {} {} {}",
                "+".green().bold(),
                "artifact".dimmed(),
                path,
                "→".dimmed(),
                session_id
            );
        }
        for id in &new_sources {
            println!("  {} {}    {}", "+".green().bold(), "source".dimmed(), id);
        }
        if !space_warnings.is_empty() {
            println!(
                "  {}",
                format!(
                    "{} file(s) have spaces in their names",
                    space_warnings.len()
                )
                .yellow()
            );
        }
    }

    // ── Desktop notification (best-effort) ────────────────────────────────────
    if args.notify && !nothing_new {
        let msg = format!(
            "{} new session(s), {} recording(s), {} artifact(s), {} source(s)",
            new_sessions.len(),
            new_recordings.len(),
            new_artifacts.len(),
            new_sources.len(),
        );
        notify(&msg);
    }

    Ok(())
}

// ── Raw note scanning ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
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
                recording_exports: vec![],
                artifacts: vec![],
                recording_missing: false,
                status: SessionStatus::Unprocessed,
                processed_at: None,
                topics_updated: vec![],
            },
        );
        new_sessions.push(session_id);
    }
    Ok(())
}

// ── Recording export scanning ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn scan_recordings_dir(
    recordings_dir: &Path,
    vault_root: &Path,
    config: &VaultConfig,
    fmt: &FilenameFormat,
    args: &ReconcileArgs,
    manifest: &mut Manifest,
    new_recordings: &mut Vec<(String, String)>,
    space_warnings: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(recordings_dir)
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

        // A recording file is `{session_stem}-{qualifier}`.
        // Split on the last `-` and check if the qualifier is recognized
        // and the prefix is a known session ID.
        let (session_id, qualifier) = match stem.rsplit_once('-') {
            Some((prefix, suffix)) => (prefix, suffix),
            None => continue,
        };

        if !config.is_recording_qualifier(qualifier) {
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
        if entry.recording_exports.iter().any(|p| p.path == rel_path) {
            continue;
        }

        let kind = recording_kind(qualifier, config);
        new_recordings.push((session_id.to_string(), rel_path.clone()));
        entry.recording_exports.push(RecordingExport {
            path: rel_path,
            kind,
        });
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a `NaiveDate` from a parsed stem.  Fills in the current year when
/// the format doesn't include `{yyyy}`.
fn build_date(parsed: &crate::config::ParsedStem) -> Option<NaiveDate> {
    let year = parsed
        .year
        .unwrap_or_else(|| Utc::now().date_naive().year());
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

fn recording_kind(qualifier: &str, config: &VaultConfig) -> RecordingKind {
    match qualifier {
        "transcript" => RecordingKind::Transcript,
        "summary" => RecordingKind::Summary,
        "mindmap" => RecordingKind::Mindmap,
        q if q.len() == 1 && q.chars().next().is_some_and(|c| c.is_ascii_lowercase()) => {
            RecordingKind::Anonymous
        }
        _ => {
            // Must be a custom qualifier from config.recording_qualifiers
            let _ = config; // used implicitly via is_recording_qualifier upstream
            RecordingKind::Custom
        }
    }
}

// ── Artifact scanning ─────────────────────────────────────────────────────────

/// Text-readable file extensions we'll wrap and pass to the AI.
/// Binary formats (PDF, images, office docs) are skipped — they can't be
/// read as UTF-8 and the AI can't use them directly.
/// Extensions accepted during artifact scanning.  Includes binary presentation
/// formats (pdf, pptx, ppt) that are extracted via the slides pipeline at
/// workspace-assembly time rather than read directly as text.
const ACCEPTED_ARTIFACT_EXTENSIONS: &[&str] = &[
    // Binary slide formats (extracted by workspace via pdftotext / PPTX parser)
    "pdf", "pptx", "ppt", // Markup / documentation
    "md", "txt", "html", "htm", "tex", // Code
    "py", "java", "rs", "js", "ts", "jsx", "tsx", "c", "cpp", "h", "hpp", "cc", "cxx", "go", "rb",
    "swift", "kt", "kts", "cs", "fs", "ml", "mli", "hs", "lhs", "r", "rmd", "sql", "sh", "bash",
    "zsh", "fish", "yaml", "yml", "toml", "json", "xml", "csv", "ipynb",
];

/// Extensions that signal lecture slides / handouts (text-format only).
const SLIDE_EXTENSIONS: &[&str] = &["md", "html", "htm", "tex", "txt"];

/// Qualifier keywords (in the filename suffix) that override kind → Slides.
const SLIDE_QUALIFIERS: &[&str] = &[
    "slides", "slide", "deck", "handout", "handouts", "lecture", "notes",
];

/// Walk `{course}/{artifacts_dir}/` for files matching a session and attach
/// them as `ArtifactEntry` records.
///
/// Two matching patterns (tried in order):
///
///   Pattern A — flat file with session-prefixed stem:
///     `{session_id}.{ext}`        → attached, qualifier = ""
///     `{session_id}-{rest}.{ext}` → attached, qualifier = rest
///
///   Pattern B — file inside a session-named directory:
///     `{session_id}/{file}.{ext}`         → attached, qualifier = stem
///     `{session_id}/{sub}/{file}.{ext}`   → attached, qualifier = "sub-stem"
///
///   Example: `CPSC5001-09-03/day1/Thing.java` → session CPSC5001-09-03,
///   qualifier "day1-Thing", kind Code.
///
/// Kind classification (in priority order):
/// 1. Extension is pdf/pptx/ppt → Slides (extracted by workspace pipeline)
/// 2. Qualifier contains a slide keyword → Slides
/// 3. Extension is in `SLIDE_EXTENSIONS` and qualifier is empty → Slides
/// 4. Extension is a code extension → Code
/// 5. Otherwise → Other
fn scan_artifacts_dir(
    artifacts_dir: &Path,
    vault_root: &Path,
    manifest: &mut Manifest,
    new_artifacts: &mut Vec<(String, String)>,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(artifacts_dir)
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
        if !ACCEPTED_ARTIFACT_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // ── Match the file to a session ───────────────────────────────────────
        // Pattern A: stem is `{session_id}` or `{session_id}-{rest}`.
        // Pattern B: first path component under artifacts_dir is a session ID.
        let (session_id, qualifier): (String, String) = if manifest.sessions.contains_key(stem) {
            (stem.to_string(), String::new())
        } else if let Some(sid) = stem
            .rsplit_once('-')
            .and_then(|(pre, _)| manifest.sessions.contains_key(pre).then_some(pre))
        {
            (sid.to_string(), stem[sid.len() + 1..].to_string())
        } else {
            // Pattern B: file lives inside a directory named after a session.
            let rel = match path.strip_prefix(artifacts_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let mut components = rel.components();
            let first = match components.next().and_then(|c| c.as_os_str().to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            if !manifest.sessions.contains_key(first.as_str()) {
                continue;
            }
            // Qualifier: remaining path components joined by '-', no extension.
            let rest: std::path::PathBuf = components.collect();
            let qual = rest
                .with_extension("")
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect::<Vec<_>>()
                .join("-");
            (first, qual)
        };

        let entry_in_manifest = match manifest.sessions.get_mut(session_id.as_str()) {
            Some(e) => e,
            None => continue,
        };

        let rel_path = path
            .strip_prefix(vault_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Skip if already recorded.
        if entry_in_manifest
            .artifacts
            .iter()
            .any(|a| a.path == rel_path)
        {
            continue;
        }

        let kind = classify_artifact_kind(&ext, &qualifier);
        new_artifacts.push((session_id, rel_path.clone()));
        entry_in_manifest.artifacts.push(ArtifactEntry {
            path: rel_path,
            kind,
        });
    }
    Ok(())
}

fn classify_artifact_kind(ext: &str, qualifier: &str) -> ArtifactKind {
    // Binary presentation formats are always slides regardless of qualifier.
    if matches!(ext, "pdf" | "pptx" | "ppt") {
        return ArtifactKind::Slides;
    }

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
        "py", "java", "rs", "js", "ts", "jsx", "tsx", "c", "cpp", "h", "hpp", "cc", "cxx", "go",
        "rb", "swift", "kt", "kts", "cs", "fs", "ml", "mli", "hs", "lhs", "r", "rmd", "sql", "sh",
        "bash", "zsh", "fish", "ipynb",
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
/// - Flat file:         `{stem}`                    e.g. `SICP-ch01`
/// - In subdirectory:   `{subdir}/{stem}`            e.g. `Textbooks/SICP/Chapter-01`
///
/// Kind is inferred from the top-level subdirectory:
/// - `AI-Conversations/` → `AiConversation` (skipped when `scan_ai_conversations` is false;
///   requires YAML frontmatter — files without it are silently skipped)
/// - `Textbooks/` → `Textbook`
/// - Everything else → `Other`
///
/// Heading schemes are derived via `comrak` for all kinds except `AiConversation`
/// (conversations use blockquotes, not chapter/section headings).
/// Summary and tags are read from YAML frontmatter when present.
fn scan_sources_dir(
    sources_dir: &Path,
    vault_root: &Path,
    manifest: &mut Manifest,
    scan_ai_conversations: bool,
    ignore_dirs: &[String],
    new_sources: &mut Vec<String>,
) -> Result<()> {
    // Evict any already-registered sources whose ID path passes through an
    // ignored directory.  This handles the case where a directory was added to
    // `sources_ignore_dirs` after the source was first registered.
    if !ignore_dirs.is_empty() {
        manifest.sources.retain(|id, _| {
            !std::path::Path::new(id)
                .parent()
                .map(|p| {
                    p.components()
                        .any(|c| ignore_dirs.iter().any(|d| c.as_os_str() == d.as_str()))
                })
                .unwrap_or(false)
        });
    }

    for entry in walkdir::WalkDir::new(sources_dir)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() > 0 && e.file_type().is_dir() {
                let name = e.file_name().to_str().unwrap_or("");
                !ignore_dirs.iter().any(|d| d == name)
            } else {
                true
            }
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }

        // Derive the source ID from the path relative to sources_dir.
        let rel_to_sources = path.strip_prefix(sources_dir).unwrap_or(path);

        // Infer kind from the top-level subdirectory name.
        let top_dir = rel_to_sources
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("");
        let kind = match top_dir {
            "AI-Conversations" => SourceKind::AiConversation,
            "Textbooks" => SourceKind::Textbook,
            _ => SourceKind::Other,
        };

        if kind == SourceKind::AiConversation && !scan_ai_conversations {
            continue;
        }

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

        let rel_path = path
            .strip_prefix(vault_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Read content once; used for heading scheme and frontmatter extraction.
        let content = match crate::frontmatter::read_note(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // AI conversations require YAML frontmatter — skip unprocessed files.
        if kind == SourceKind::AiConversation
            && crate::frontmatter::split_frontmatter(&content).is_none()
        {
            continue;
        }

        // Extract summary, tags, courses, and optional hints from frontmatter.
        let meta = extract_source_meta(&content);

        // source_type frontmatter overrides the directory-inferred kind.
        let kind = meta.kind_hint.unwrap_or(kind);

        // Heading scheme: meaningful only for structured documents (not conversations).
        let heading_scheme = if kind == SourceKind::AiConversation {
            vec![]
        } else {
            derive_heading_scheme(&parse_headings(&content))
        };

        if let Some(entry) = manifest.sources.get_mut(&source_id) {
            // Already registered: refresh informational metadata while preserving
            // operational state (status, last_processed_at, topics_updated).
            // kind is refreshed from frontmatter so adding source_type later takes effect.
            // Exception: promote to Staged when frontmatter says so and the source
            // hasn't been worked on yet — don't downgrade InProgress or Processed.
            entry.summary = meta.summary;
            entry.tags = meta.tags;
            entry.courses = meta.courses;
            entry.heading_scheme = heading_scheme;
            entry.kind = kind;
            if meta.status_hint == Some(SourceStatus::Staged)
                && entry.status == SourceStatus::Unprocessed
            {
                entry.status = SourceStatus::Staged;
            }
            continue;
        }

        // New source: use frontmatter status hint if Staged, otherwise Unprocessed.
        let initial_status = if meta.status_hint == Some(SourceStatus::Staged) {
            SourceStatus::Staged
        } else {
            SourceStatus::Unprocessed
        };

        manifest.sources.insert(
            source_id.clone(),
            SourceEntry {
                path: rel_path,
                kind,
                status: initial_status,
                last_processed_at: None,
                heading_scheme,
                topics_updated: vec![],
                summary: meta.summary,
                tags: meta.tags,
                courses: meta.courses,
            },
        );
        new_sources.push(source_id);
    }
    Ok(())
}

struct SourceMeta {
    summary: Option<String>,
    tags: Vec<String>,
    courses: Vec<String>,
    status_hint: Option<SourceStatus>,
    kind_hint: Option<SourceKind>,
}

/// Extract metadata from a source file's YAML frontmatter.
/// Returns defaults when there is no frontmatter or fields are absent.
fn extract_source_meta(content: &str) -> SourceMeta {
    let Some((yaml, _)) = crate::frontmatter::split_frontmatter(content) else {
        return SourceMeta {
            summary: None,
            tags: vec![],
            courses: vec![],
            status_hint: None,
            kind_hint: None,
        };
    };
    #[derive(serde::Deserialize, Default)]
    struct SourceFrontmatter {
        summary: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default)]
        courses: Vec<String>,
        status: Option<SourceStatus>,
        source_type: Option<String>,
    }
    let meta: SourceFrontmatter = serde_yml::from_str(yaml).unwrap_or_default();
    SourceMeta {
        summary: meta.summary,
        tags: meta.tags,
        courses: meta.courses,
        status_hint: meta.status,
        kind_hint: meta.source_type.as_deref().and_then(kind_from_source_type),
    }
}

/// Map a `source_type` frontmatter string to a `SourceKind` refinement.
/// Returns `None` for unrecognised values (directory-inferred kind is kept).
fn kind_from_source_type(s: &str) -> Option<SourceKind> {
    match s {
        "highlights" | "notes" => Some(SourceKind::PersonalNotes),
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode};
    use crate::manifest::{ManifestConfig, SessionEntry, SessionStatus};
    use chrono::NaiveDate;
    use tempfile::TempDir;

    fn make_manifest_with_session(vault_root: &std::path::Path, session_id: &str) -> Manifest {
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
        let mut m = Manifest::empty(vault_root.to_path_buf(), cfg);
        m.sessions.insert(
            session_id.to_string(),
            SessionEntry {
                date: NaiveDate::from_ymd_opt(2026, 9, 3).unwrap(),
                course: "CPSC5001".into(),
                filename_format: "{course}-{mm}-{dd}".into(),
                raw_note: format!("notes/{}.md", session_id),
                recording_exports: vec![],
                artifacts: vec![],
                recording_missing: false,
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
        assert_eq!(
            classify_artifact_kind("md", "handout"),
            ArtifactKind::Slides
        );
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
        assert_eq!(
            classify_artifact_kind("json", "schema"),
            ArtifactKind::Other
        );
    }

    // ── scan_artifacts_dir ────────────────────────────────────────────────────

    #[test]
    fn scan_attaches_slide_by_qualifier() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-slides.md", "# Slides");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

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
        write_file(
            &artifacts_dir,
            "CPSC5001-09-03-BinarySearch.java",
            "class BinarySearch {}",
        );

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(new_artifacts.len(), 1);
        assert_eq!(
            manifest.sessions["CPSC5001-09-03"].artifacts[0].kind,
            ArtifactKind::Code
        );
    }

    #[test]
    fn scan_registers_pdf_as_slides() {
        // PDFs are registered (kind: Slides) so the workspace slide-extraction
        // pipeline can process them; they are NOT skipped at reconcile time.
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-slides.pdf", "%PDF content");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(new_artifacts.len(), 1, "PDF must be registered");
        assert_eq!(
            manifest.sessions["CPSC5001-09-03"].artifacts[0].kind,
            ArtifactKind::Slides,
            "PDF must be classified as Slides"
        );
    }

    #[test]
    fn scan_registers_pptx_as_slides() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-deck.pptx", "PK binary");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(new_artifacts.len(), 1);
        assert_eq!(
            manifest.sessions["CPSC5001-09-03"].artifacts[0].kind,
            ArtifactKind::Slides
        );
    }

    #[test]
    fn scan_folder_flat_file_in_session_dir_is_attached() {
        // `CPSC5001-09-03/slides.pdf` — first directory component is the session ID.
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(
            &artifacts_dir.join("CPSC5001-09-03"),
            "slides.pdf",
            "%PDF content",
        );

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(
            new_artifacts.len(),
            1,
            "file in session folder must be attached"
        );
        assert_eq!(new_artifacts[0].0, "CPSC5001-09-03");
        assert_eq!(
            manifest.sessions["CPSC5001-09-03"].artifacts[0].kind,
            ArtifactKind::Slides
        );
    }

    #[test]
    fn scan_folder_nested_code_file_is_attached() {
        // `CPSC5001-09-03/day1/Thing.java` — subfolder inside the session folder.
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(
            &artifacts_dir.join("CPSC5001-09-03").join("day1"),
            "Thing.java",
            "class Thing {}",
        );

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(new_artifacts.len(), 1, "nested code file must be attached");
        assert_eq!(new_artifacts[0].0, "CPSC5001-09-03");
        assert_eq!(
            manifest.sessions["CPSC5001-09-03"].artifacts[0].kind,
            ArtifactKind::Code,
            "java file should be classified as Code"
        );
    }

    #[test]
    fn scan_folder_mixed_contents_all_attached() {
        // Multiple files in different subfolders — all go to the same session.
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        let sess_dir = artifacts_dir.join("CPSC5001-09-03");
        write_file(&sess_dir, "slides-lecture.pdf", "%PDF lecture");
        write_file(&sess_dir, "slides-reading.pdf", "%PDF reading");
        write_file(&sess_dir.join("day1"), "Thing.java", "class Thing {}");
        write_file(&sess_dir.join("day1"), "Node.java", "class Node {}");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(new_artifacts.len(), 4, "all four files must be attached");
        assert!(new_artifacts.iter().all(|(sid, _)| sid == "CPSC5001-09-03"));
        let kinds: Vec<_> = manifest.sessions["CPSC5001-09-03"]
            .artifacts
            .iter()
            .map(|a| &a.kind)
            .collect();
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == &ArtifactKind::Slides)
                .count(),
            2
        );
        assert_eq!(
            kinds.iter().filter(|k| **k == &ArtifactKind::Code).count(),
            2
        );
    }

    #[test]
    fn scan_folder_unmatched_dir_is_skipped() {
        // A directory whose name is not a session ID — must be ignored.
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir.join("day1"), "Thing.java", "class Thing {}");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert!(
            new_artifacts.is_empty(),
            "file in non-session directory must be skipped"
        );
    }

    #[test]
    fn scan_skips_unmatched_stems() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "BinarySearch.java", "class BinarySearch {}");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert!(new_artifacts.is_empty(), "Unmatched stem should be skipped");
    }

    #[test]
    fn scan_idempotent() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        write_file(&artifacts_dir, "CPSC5001-09-03-slides.md", "# Slides");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();
        // Run again — should not double-register.
        let mut new_artifacts2 = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts2,
        )
        .unwrap();

        assert!(new_artifacts2.is_empty(), "Second scan should add nothing");
        assert_eq!(manifest.sessions["CPSC5001-09-03"].artifacts.len(), 1);
    }

    #[test]
    fn scan_artifacts_finds_nested_in_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        // Artifact nested one level deeper than artifacts_dir (e.g. per-course folder).
        let course_dir = artifacts_dir.join("CPSC5001");
        write_file(&course_dir, "CPSC5001-09-03-slides.md", "# Slides");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(
            new_artifacts.len(),
            1,
            "artifact in subdirectory should be found"
        );
    }

    fn make_empty_manifest_for_sources(vault_root: &std::path::Path) -> Manifest {
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

    #[test]
    fn scan_sources_ai_conversation_with_frontmatter_is_registered() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let deep_dir = sources_dir.join("AI-Conversations").join("Gemini");
        write_file(
            &deep_dir,
            "sorting-questions.md",
            "---\nsummary: Discussion about sorting algorithms\ntags: [algorithms, sorting]\n---\n\n# Sorting Q&A\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        assert_eq!(
            new_sources.len(),
            1,
            "AI conversation with frontmatter should be registered"
        );
        assert_eq!(new_sources[0], "AI-Conversations/Gemini/sorting-questions");
        let entry = &manifest.sources["AI-Conversations/Gemini/sorting-questions"];
        assert_eq!(entry.kind, crate::manifest::SourceKind::AiConversation);
        assert_eq!(
            entry.summary.as_deref(),
            Some("Discussion about sorting algorithms")
        );
        assert_eq!(entry.tags, vec!["algorithms", "sorting"]);
        assert!(
            entry.heading_scheme.is_empty(),
            "AI conversations should have no heading scheme"
        );
    }

    #[test]
    fn scan_sources_ai_conversation_without_frontmatter_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let deep_dir = sources_dir.join("AI-Conversations").join("Gemini");
        write_file(
            &deep_dir,
            "sorting-questions.md",
            "# Sorting Q&A\n\nNo frontmatter here.\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        assert!(
            new_sources.is_empty(),
            "AI conversation without frontmatter must be skipped"
        );
    }

    #[test]
    fn scan_sources_ai_conversations_skipped_when_flag_false() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let deep_dir = sources_dir.join("AI-Conversations").join("Gemini");
        write_file(
            &deep_dir,
            "sorting-questions.md",
            "---\nsummary: Has frontmatter\n---\n\nContent.\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            false,
            &[],
            &mut new_sources,
        )
        .unwrap();

        assert!(
            new_sources.is_empty(),
            "AI conversations should be ignored when scan_ai_conversations is false"
        );
    }

    #[test]
    fn scan_sources_ai_conversation_courses_extracted() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let deep_dir = sources_dir.join("AI-Conversations").join("Gemini");
        write_file(
            &deep_dir,
            "exam-prep.md",
            "---\nsummary: Exam prep chat\ncourses: [CPSC5001]\n---\n\nContent.\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        let entry = &manifest.sources["AI-Conversations/Gemini/exam-prep"];
        assert_eq!(entry.courses, vec!["CPSC5001"]);
    }

    #[test]
    fn scan_sources_ai_conversation_path_is_vault_relative() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let deep_dir = sources_dir.join("AI-Conversations").join("Gemini");
        write_file(
            &deep_dir,
            "sorting-questions.md",
            "---\nsummary: Discussion\n---\n\nContent.\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        let entry = &manifest.sources["AI-Conversations/Gemini/sorting-questions"];
        assert_eq!(
            entry.path, "sources/AI-Conversations/Gemini/sorting-questions.md",
            "stored path must be relative to vault root, not sources_dir"
        );
    }

    #[test]
    fn scan_sources_multiple_providers_get_distinct_ids() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        write_file(
            &sources_dir.join("AI-Conversations").join("Gemini"),
            "chat-a.md",
            "---\nsummary: Gemini chat\n---\n\nContent.\n",
        );
        write_file(
            &sources_dir.join("AI-Conversations").join("Claude"),
            "chat-b.md",
            "---\nsummary: Claude chat\n---\n\nContent.\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        assert_eq!(new_sources.len(), 2);
        assert!(manifest
            .sources
            .contains_key("AI-Conversations/Gemini/chat-a"));
        assert!(manifest
            .sources
            .contains_key("AI-Conversations/Claude/chat-b"));
    }

    #[test]
    fn scan_sources_textbook_registered_without_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let chapter_dir = sources_dir.join("Textbooks").join("SICP");
        write_file(
            &chapter_dir,
            "Chapter-01.md",
            "# Chapter 1: Building Abstractions\n\nContent.\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        assert_eq!(
            new_sources.len(),
            1,
            "textbook chapter should register without frontmatter"
        );
        let entry = &manifest.sources["Textbooks/SICP/Chapter-01"];
        assert_eq!(entry.kind, crate::manifest::SourceKind::Textbook);
        assert!(entry.summary.is_none());
        assert!(entry.tags.is_empty());
    }

    #[test]
    fn scan_sources_textbook_extracts_frontmatter_when_present() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let chapter_dir = sources_dir.join("Textbooks").join("SICP");
        write_file(
            &chapter_dir,
            "Chapter-01.md",
            "---\nsummary: Procedures and processes\ntags: [lisp, recursion]\n---\n\n# Chapter 1\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();

        let entry = &manifest.sources["Textbooks/SICP/Chapter-01"];
        assert_eq!(entry.summary.as_deref(), Some("Procedures and processes"));
        assert_eq!(entry.tags, vec!["lisp", "recursion"]);
    }

    #[test]
    fn scan_sources_refreshes_metadata_for_existing_entry() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let chapter_dir = sources_dir.join("Textbooks").join("SICP");

        // First scan: no frontmatter.
        write_file(&chapter_dir, "Chapter-01.md", "# Chapter 1\n\nContent.\n");
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();
        assert_eq!(new_sources.len(), 1);

        // Simulate a processed session having touched this source.
        {
            let entry = manifest
                .sources
                .get_mut("Textbooks/SICP/Chapter-01")
                .unwrap();
            entry.status = SourceStatus::Processed;
            entry.last_processed_at = Some(chrono::Utc::now());
            entry.topics_updated = vec!["lambda-calculus".into()];
        }

        // Second scan: file now has frontmatter with courses and tags.
        write_file(
            &chapter_dir,
            "Chapter-01.md",
            "---\nsummary: Building Abstractions\ntags: [lisp]\ncourses: [CPSC5001]\n---\n\n# Chapter 1\n",
        );
        let mut new_sources2 = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources2,
        )
        .unwrap();

        assert!(
            new_sources2.is_empty(),
            "no new sources should be registered"
        );
        let entry = &manifest.sources["Textbooks/SICP/Chapter-01"];
        assert_eq!(
            entry.summary.as_deref(),
            Some("Building Abstractions"),
            "summary updated"
        );
        assert_eq!(entry.tags, vec!["lisp"], "tags updated");
        assert_eq!(entry.courses, vec!["CPSC5001"], "courses updated");
        // Operational state must be preserved.
        assert_eq!(entry.status, SourceStatus::Processed, "status preserved");
        assert!(
            entry.last_processed_at.is_some(),
            "last_processed_at preserved"
        );
        assert_eq!(
            entry.topics_updated,
            vec!["lambda-calculus"],
            "topics_updated preserved"
        );
    }

    #[test]
    fn scan_sources_source_type_highlights_sets_personal_notes() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let dir = sources_dir.join("Textbooks").join("Gaddis");
        write_file(
            &dir,
            "Chapter-03-notes.md",
            "---\nsource_type: highlights\n---\n\n# My notes on ch3\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();

        let entry = &manifest.sources["Textbooks/Gaddis/Chapter-03-notes"];
        assert_eq!(
            entry.kind,
            crate::manifest::SourceKind::PersonalNotes,
            "source_type: highlights should set PersonalNotes"
        );
    }

    #[test]
    fn scan_sources_source_type_notes_sets_personal_notes() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let dir = sources_dir.join("Textbooks").join("Gaddis");
        write_file(
            &dir,
            "Chapter-03-notes.md",
            "---\nsource_type: notes\n---\n\n# My notes on ch3\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();

        assert_eq!(
            manifest.sources["Textbooks/Gaddis/Chapter-03-notes"].kind,
            crate::manifest::SourceKind::PersonalNotes,
        );
    }

    #[test]
    fn scan_sources_no_source_type_keeps_directory_inferred_kind() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let dir = sources_dir.join("Textbooks").join("Gaddis");
        write_file(&dir, "Chapter-03.md", "# Chapter 3\n\nVerbatim content.\n");

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();

        assert_eq!(
            manifest.sources["Textbooks/Gaddis/Chapter-03"].kind,
            crate::manifest::SourceKind::Textbook,
            "no source_type should keep directory-inferred Textbook kind"
        );
    }

    #[test]
    fn scan_sources_source_type_updates_kind_on_existing_entry() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let dir = sources_dir.join("Textbooks").join("Gaddis");

        // First scan: no source_type → Textbook.
        write_file(&dir, "Chapter-03.md", "# Chapter 3\n");
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();
        assert_eq!(
            manifest.sources["Textbooks/Gaddis/Chapter-03"].kind,
            crate::manifest::SourceKind::Textbook
        );

        // User adds source_type: highlights → kind updates to PersonalNotes.
        write_file(
            &dir,
            "Chapter-03.md",
            "---\nsource_type: highlights\n---\n\n# Chapter 3\n",
        );
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();
        assert_eq!(
            manifest.sources["Textbooks/Gaddis/Chapter-03"].kind,
            crate::manifest::SourceKind::PersonalNotes,
            "adding source_type to existing entry should update kind"
        );
    }

    #[test]
    fn scan_sources_staged_from_frontmatter_on_new_source() {
        // "incomplete", "unread", and "staged" all deserialize to Staged.
        for status_str in &["incomplete", "unread", "staged"] {
            let tmp = TempDir::new().unwrap();
            let sources_dir = tmp.path().join("sources");
            let chapter_dir = sources_dir.join("Textbooks").join("Zybooks");
            write_file(
                &chapter_dir,
                "Chapter-07.md",
                &format!("---\nstatus: {}\n---\n\n# Chapter 7\n", status_str),
            );

            let mut manifest = make_empty_manifest_for_sources(tmp.path());
            let mut new_sources = vec![];
            scan_sources_dir(
                &sources_dir,
                tmp.path(),
                &mut manifest,
                true,
                &[],
                &mut new_sources,
            )
            .unwrap();

            let entry = &manifest.sources["Textbooks/Zybooks/Chapter-07"];
            assert_eq!(
                entry.status,
                SourceStatus::Staged,
                "status '{}' should register as Staged",
                status_str
            );
        }
    }

    #[test]
    fn scan_sources_staged_frontmatter_upgrades_unprocessed_existing() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let chapter_dir = sources_dir.join("Textbooks").join("Zybooks");

        // First scan: no status in frontmatter → Unprocessed.
        write_file(&chapter_dir, "Chapter-07.md", "# Chapter 7\n");
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();
        assert_eq!(
            manifest.sources["Textbooks/Zybooks/Chapter-07"].status,
            SourceStatus::Unprocessed
        );

        // User adds `status: incomplete` to the file.
        write_file(
            &chapter_dir,
            "Chapter-07.md",
            "---\nstatus: incomplete\n---\n\n# Chapter 7\n",
        );
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();
        assert_eq!(
            manifest.sources["Textbooks/Zybooks/Chapter-07"].status,
            SourceStatus::Staged,
            "Unprocessed should be promoted to Staged when frontmatter says incomplete"
        );
    }

    #[test]
    fn scan_sources_staged_frontmatter_does_not_downgrade_processed() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let chapter_dir = sources_dir.join("Textbooks").join("Zybooks");

        write_file(
            &chapter_dir,
            "Chapter-07.md",
            "---\nstatus: incomplete\n---\n\n# Chapter 7\n",
        );
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();

        // Simulate prior processing.
        manifest
            .sources
            .get_mut("Textbooks/Zybooks/Chapter-07")
            .unwrap()
            .status = SourceStatus::Processed;

        // Re-scan with same frontmatter — Processed must not be downgraded.
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut vec![],
        )
        .unwrap();
        assert_eq!(
            manifest.sources["Textbooks/Zybooks/Chapter-07"].status,
            SourceStatus::Processed,
            "Processed should not be downgraded to Staged by frontmatter"
        );
    }

    #[test]
    fn scan_sources_ignore_dirs_skips_matching_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        // A real source alongside a tool folder that should be skipped.
        write_file(
            &sources_dir.join("Textbooks").join("SICP"),
            "Chapter-01.md",
            "# Chapter 1\n",
        );
        write_file(
            &sources_dir.join("_tools"),
            "generate.md",
            "# Generator instructions\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        let ignore = vec!["_tools".to_string()];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &ignore,
            &mut new_sources,
        )
        .unwrap();

        assert_eq!(
            new_sources.len(),
            1,
            "only the textbook chapter should be registered"
        );
        assert!(manifest.sources.contains_key("Textbooks/SICP/Chapter-01"));
        assert!(
            !manifest.sources.contains_key("_tools/generate"),
            "ignored dir must not be registered"
        );
    }

    #[test]
    fn scan_sources_ignore_dirs_matches_by_name_not_full_path() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        // Nested: sources/AI-Conversations/_tools/helper.md
        // The name "_tools" appears deep in the tree; it must still be skipped.
        write_file(
            &sources_dir.join("AI-Conversations").join("Gemini"),
            "chat.md",
            "---\nsummary: Real chat\n---\n\nContent.\n",
        );
        write_file(
            &sources_dir.join("AI-Conversations").join("_tools"),
            "prompt-template.md",
            "# Prompt template\n",
        );

        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        let ignore = vec!["_tools".to_string()];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &ignore,
            &mut new_sources,
        )
        .unwrap();

        assert_eq!(new_sources.len(), 1);
        assert!(manifest
            .sources
            .contains_key("AI-Conversations/Gemini/chat"));
        assert!(!manifest
            .sources
            .contains_key("AI-Conversations/_tools/prompt-template"));
    }

    #[test]
    fn scan_sources_ignore_dirs_evicts_previously_registered_entry() {
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        write_file(
            &sources_dir.join("Textbooks").join("SICP"),
            "Chapter-01.md",
            "# Chapter 1\n",
        );
        write_file(
            &sources_dir.join("Textbooks").join("_tools"),
            "tagging-workflow.md",
            "# Workflow\n",
        );

        // First scan with no ignore list — both files get registered.
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &[],
            &mut new_sources,
        )
        .unwrap();
        assert_eq!(new_sources.len(), 2);
        assert!(manifest
            .sources
            .contains_key("Textbooks/_tools/tagging-workflow"));

        // Second scan with _tools ignored — stale entry must be evicted.
        let ignore = vec!["_tools".to_string()];
        let mut new_sources2 = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            true,
            &ignore,
            &mut new_sources2,
        )
        .unwrap();
        assert!(new_sources2.is_empty(), "no new sources");
        assert!(
            !manifest
                .sources
                .contains_key("Textbooks/_tools/tagging-workflow"),
            "stale ignored entry must be evicted"
        );
        assert!(
            manifest.sources.contains_key("Textbooks/SICP/Chapter-01"),
            "real source preserved"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn notify_macos_message_is_separate_argv_element() {
        // The security property: the message must land as a distinct argv
        // element, never embedded in the script strings, so AppleScript-
        // breaking chars cannot alter the script structure.
        let injection = "\" & do shell script \"rm -rf /\" & \"";
        let args = notify_macos_args(injection);
        // Script strings are the first 6 elements; message is last.
        assert_eq!(args.last(), Some(&injection));
        // None of the script strings contain the injection payload.
        for script_arg in &args[..args.len() - 1] {
            assert!(!script_arg.contains(injection));
        }

        // Other tricky inputs also land verbatim as the final element.
        for msg in ["'; display dialog \"pwned\"; '", "line1\nline2\ttab"] {
            let args = notify_macos_args(msg);
            assert_eq!(args.last(), Some(&msg));
        }
    }

    // ── recording_kind ────────────────────────────────────────────────────────

    fn make_vault_config() -> crate::config::VaultConfig {
        crate::config::VaultConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            csnotes_dir: ".csnotes".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            active_courses: vec![],
            default_backend: AiBackend::Mock,
            skill_variant: SkillVariant::Claude,
            snapshot_mode: SnapshotMode::PreMerge,
            archive_threshold_weeks: 8,
            recording_qualifiers: vec!["transcript".into(), "summary".into(), "mindmap".into()],
            agy_model: None,
            require_recordings: true,
            courses_without_recordings: vec![],
            scan_ai_conversations: true,
            sources_ignore_dirs: vec![],
        }
    }

    #[test]
    fn recording_kind_returns_transcript() {
        let cfg = make_vault_config();
        assert_eq!(
            recording_kind("transcript", &cfg),
            RecordingKind::Transcript
        );
    }

    #[test]
    fn recording_kind_returns_summary() {
        let cfg = make_vault_config();
        assert_eq!(recording_kind("summary", &cfg), RecordingKind::Summary);
    }

    #[test]
    fn recording_kind_returns_mindmap() {
        let cfg = make_vault_config();
        assert_eq!(recording_kind("mindmap", &cfg), RecordingKind::Mindmap);
    }

    #[test]
    fn recording_kind_single_lowercase_is_anonymous() {
        let cfg = make_vault_config();
        assert_eq!(recording_kind("a", &cfg), RecordingKind::Anonymous);
        assert_eq!(recording_kind("z", &cfg), RecordingKind::Anonymous);
    }

    #[test]
    fn recording_kind_uppercase_single_letter_is_not_anonymous() {
        // Catches `replace && with ||` in the guard: with `||`, len==1 alone would
        // be sufficient, so "A" would incorrectly return Anonymous.
        let cfg = make_vault_config();
        assert_eq!(recording_kind("A", &cfg), RecordingKind::Custom);
    }

    #[test]
    fn recording_kind_multi_char_is_not_anonymous() {
        // Catches `replace == with !=` and `replace match guard with true`.
        let cfg = make_vault_config();
        assert_ne!(recording_kind("ab", &cfg), RecordingKind::Anonymous);
    }

    #[test]
    fn recording_kind_unknown_qualifier_is_custom() {
        let cfg = make_vault_config();
        assert_eq!(recording_kind("deep-dive", &cfg), RecordingKind::Custom);
    }

    // ── scan_sources_dir ──────────────────────────────────────────────────────

    #[test]
    fn scan_sources_dir_registers_top_level_md() {
        // Catches `delete match arm (_, Some(stem))`: without that arm, top-level
        // sources (empty parent) fall through to `_ => continue` and are skipped.
        // Also catches `replace match guard !parent.is_empty() with true`: the
        // source_id would be "/sicp" instead of "sicp".
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        write_file(&sources_dir, "sicp.md", "# SICP\n");
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            false,
            &[],
            &mut new_sources,
        )
        .unwrap();
        assert!(
            manifest.sources.contains_key("sicp"),
            "top-level .md should be registered with bare stem as key"
        );
    }

    #[test]
    fn scan_sources_dir_depth_filter_doesnt_exclude_root() {
        // Catches `replace > with >= in scan_sources_dir` and `replace && with ||`:
        // with either mutation, if the sources_dir's own name appears in ignore_dirs,
        // filter_entry would exclude the root, preventing all subdirectory recursion.
        let tmp = TempDir::new().unwrap();
        let sources_dir = tmp.path().join("sources");
        let books_dir = sources_dir.join("Textbooks");
        write_file(&books_dir, "sicp.md", "# SICP\n");
        let mut manifest = make_empty_manifest_for_sources(tmp.path());
        let mut new_sources = vec![];
        scan_sources_dir(
            &sources_dir,
            tmp.path(),
            &mut manifest,
            false,
            &["sources".to_string()],
            &mut new_sources,
        )
        .unwrap();
        assert!(
            !new_sources.is_empty(),
            "sources should be found even when ignore_dirs contains the root dir name"
        );
    }

    #[test]
    fn scan_bare_session_id_md_is_slides() {
        let tmp = TempDir::new().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        // File named exactly as the session ID (no qualifier suffix)
        write_file(&artifacts_dir, "CPSC5001-09-03.md", "# Lecture notes");

        let mut manifest = make_manifest_with_session(tmp.path(), "CPSC5001-09-03");
        let mut new_artifacts = vec![];
        scan_artifacts_dir(
            &artifacts_dir,
            tmp.path(),
            &mut manifest,
            &mut new_artifacts,
        )
        .unwrap();

        assert_eq!(new_artifacts.len(), 1);
        assert_eq!(
            manifest.sessions["CPSC5001-09-03"].artifacts[0].kind,
            ArtifactKind::Slides
        );
    }
}

// Returns the osascript argv so the argument structure can be tested without
// spawning a process (and without showing a real notification in tests).
#[cfg(target_os = "macos")]
fn notify_macos_args(message: &str) -> Vec<&str> {
    vec![
        "-e",
        "on run argv",
        "-e",
        "display notification (item 1 of argv) with title \"csnotes\"",
        "-e",
        "end run",
        message,
    ]
}

fn notify(message: &str) {
    // Escape hatch for test runs and cargo mutants: set CSNOTES_NO_NOTIFY=1 to
    // suppress all desktop notifications without touching the notify flag path.
    // This prevents mutations of the `if args.notify` guard from spawning
    // osascript as a side effect of the test suite.
    if std::env::var("CSNOTES_NO_NOTIFY").is_ok() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        // Pass the message as argv rather than interpolating into the script
        // string, so that quotes or backslashes in the message can't break
        // the AppleScript syntax.
        let _ = std::process::Command::new("osascript")
            .args(notify_macos_args(message))
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
