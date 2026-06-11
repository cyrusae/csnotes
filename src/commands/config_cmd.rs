use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::config::{ensure_no_spaces, find_vault_root, FilenameFormat, VaultConfig};
use crate::manifest::Manifest;

pub struct ConfigArgs {
    pub set: Option<String>,
    pub show: bool,
    pub add_course: Option<String>,
    pub archive: Option<String>,
    pub migrate: bool,
    /// Only meaningful when `migrate` is true.  Without `--apply`, `--migrate`
    /// shows the rename plan without writing anything.
    pub apply: bool,
}

pub fn run(args: ConfigArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let mut config = VaultConfig::load(&vault_root)?;

    if args.show {
        let s = toml::to_string_pretty(&config)?;
        println!("{}", s);
        return Ok(());
    }

    if let Some(course) = args.add_course {
        ensure_no_spaces(&course, "course name")?;
        if config.active_courses.contains(&course) {
            println!("Course '{}' is already active.", course);
        } else {
            config.active_courses.push(course.clone());
            config.save(&vault_root)?;
            println!("Added course '{}'.", course);
        }
        return Ok(());
    }

    if let Some(course) = args.archive {
        ensure_no_spaces(&course, "course name")?;
        let before = config.active_courses.len();
        config.active_courses.retain(|c| c != &course);
        if config.active_courses.len() == before {
            bail!("Course '{}' not found in active_courses.", course);
        }
        config.save(&vault_root)?;
        println!("Archived course '{}'.", course);
        return Ok(());
    }

    if let Some(kv) = args.set {
        let (key, value) = kv
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set requires key=value format"))?;
        apply_set(&mut config, key.trim(), value.trim())?;
        config.save(&vault_root)?;
        println!("Set {} = {}", key.trim(), value.trim());
        return Ok(());
    }

    if args.migrate {
        let manifest = Manifest::load(&vault_root)?;
        run_migrate(&config, &vault_root, &manifest, args.apply)?;
        return Ok(());
    }

    println!("Use --show, --set key=value, --add-course COURSE, --archive COURSE, or --migrate.");
    Ok(())
}

// ── Migrate ───────────────────────────────────────────────────────────────────

/// A single file rename within the migration plan.
struct FileRename {
    old_abs: PathBuf,
    new_abs: PathBuf,
    /// Relative path for display (old side).
    old_rel: String,
    /// Relative path used to update manifest (new side).
    new_rel: String,
}

/// Per-session rename descriptor.
struct SessionRename {
    old_id: String,
    new_id: String,
    new_raw_note: String,
    /// `(recording_export_index, new_rel_path)`
    new_recordings: Vec<(usize, String)>,
    /// `(artifact_index, new_rel_path)`
    new_artifacts: Vec<(usize, String)>,
    files: Vec<FileRename>,
}

fn run_migrate(
    config: &VaultConfig,
    vault_root: &Path,
    manifest: &Manifest,
    apply: bool,
) -> Result<()> {
    // ── Pre-flight: require reconcile before changing format ──────────────────
    check_no_unregistered(vault_root, config, manifest)?;

    let new_fmt = FilenameFormat::parse(&config.filename_format)?;

    // ── Build rename plan ─────────────────────────────────────────────────────
    let mut renames: Vec<SessionRename> = Vec::new();

    for (session_id, entry) in &manifest.sessions {
        let new_stem = new_fmt.render(&entry.course, entry.date);

        if session_id == &new_stem {
            continue; // already matches current format
        }

        let mut sr = SessionRename {
            old_id: session_id.clone(),
            new_id: new_stem.clone(),
            new_raw_note: String::new(),
            new_recordings: Vec::new(),
            new_artifacts: Vec::new(),
            files: Vec::new(),
        };

        // Raw note
        let new_raw_rel = rename_in_path(&entry.raw_note, session_id, &new_stem)?;
        sr.files.push(FileRename {
            old_abs: vault_root.join(&entry.raw_note),
            new_abs: vault_root.join(&new_raw_rel),
            old_rel: entry.raw_note.clone(),
            new_rel: new_raw_rel.clone(),
        });
        sr.new_raw_note = new_raw_rel;

        // Recording exports
        for (i, p) in entry.recording_exports.iter().enumerate() {
            if let Some(new_rel) = try_rename_in_path(&p.path, session_id, &new_stem) {
                sr.files.push(FileRename {
                    old_abs: vault_root.join(&p.path),
                    new_abs: vault_root.join(&new_rel),
                    old_rel: p.path.clone(),
                    new_rel: new_rel.clone(),
                });
                sr.new_recordings.push((i, new_rel));
            }
        }

        // Artifacts
        for (i, a) in entry.artifacts.iter().enumerate() {
            if let Some(new_rel) = try_rename_in_path(&a.path, session_id, &new_stem) {
                sr.files.push(FileRename {
                    old_abs: vault_root.join(&a.path),
                    new_abs: vault_root.join(&new_rel),
                    old_rel: a.path.clone(),
                    new_rel: new_rel.clone(),
                });
                sr.new_artifacts.push((i, new_rel));
            }
        }

        renames.push(sr);
    }

    if renames.is_empty() {
        println!("All session files already match the current filename_format.");
        return Ok(());
    }

    // ── Conflict check ────────────────────────────────────────────────────────
    for sr in &renames {
        // Target file must not already exist
        for fr in &sr.files {
            if fr.new_abs.exists() {
                bail!(
                    "Target already exists: '{}'. Resolve manually before migrating.",
                    fr.new_rel
                );
            }
        }
        // New session ID must not already be in manifest (unless it IS the
        // same entry being renamed — i.e. a no-op we already filtered out)
        if manifest.sessions.contains_key(&sr.new_id) {
            bail!(
                "Session '{}' would conflict with an existing session entry. \
                 Resolve manually before migrating.",
                sr.new_id
            );
        }
    }

    // ── Print plan ────────────────────────────────────────────────────────────
    let total_files: usize = renames.iter().map(|r| r.files.len()).sum();
    println!(
        "Rename plan ({} session{}, {} file{}):",
        renames.len(),
        if renames.len() == 1 { "" } else { "s" },
        total_files,
        if total_files == 1 { "" } else { "s" }
    );
    for sr in &renames {
        println!("  {} → {}", sr.old_id, sr.new_id);
        for fr in &sr.files {
            println!("    {} → {}", fr.old_rel, fr.new_rel);
        }
    }

    if !apply {
        println!();
        println!("Run `csnotes config --migrate --apply` to execute.");
        return Ok(());
    }

    // ── Apply ─────────────────────────────────────────────────────────────────
    let mut new_manifest = manifest.clone();

    for sr in &renames {
        // Rename files on disk
        for fr in &sr.files {
            if !fr.old_abs.exists() {
                eprintln!("  warn: source not found, skipping: {}", fr.old_rel);
                continue;
            }
            // Ensure parent dir exists (e.g., if course root changed)
            if let Some(parent) = fr.new_abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&fr.old_abs, &fr.new_abs)?;
        }

        // Update manifest: remove old key, insert under new key
        if let Some(mut entry) = new_manifest.sessions.shift_remove(&sr.old_id) {
            entry.raw_note = sr.new_raw_note.clone();
            for (i, new_path) in &sr.new_recordings {
                entry.recording_exports[*i].path = new_path.clone();
            }
            for (i, new_path) in &sr.new_artifacts {
                entry.artifacts[*i].path = new_path.clone();
            }
            entry.filename_format = config.filename_format.clone();
            new_manifest.sessions.insert(sr.new_id.clone(), entry);
        }
    }

    new_manifest.save(vault_root)?;
    println!(
        "\nRenamed {} session(s), {} file(s). Manifest updated.",
        renames.len(),
        total_files
    );

    Ok(())
}

// ── Path rename helpers ───────────────────────────────────────────────────────

/// Replace the `old_stem` prefix in the filename component of `rel_path` with
/// `new_stem`, returning the updated relative path.
///
/// Example: `"CPSC5001/notes/CPSC5001-09-03.md"`, `"CPSC5001-09-03"`,
/// `"CPSC5001-2024-09-03"` → `"CPSC5001/notes/CPSC5001-2024-09-03.md"`
fn try_rename_in_path(rel_path: &str, old_stem: &str, new_stem: &str) -> Option<String> {
    let p = Path::new(rel_path);
    let filename = p.file_name()?.to_str()?;
    if !filename.starts_with(old_stem) {
        return None;
    }
    let suffix = &filename[old_stem.len()..]; // e.g. ".md" or "-summary.md"
    let new_filename = format!("{}{}", new_stem, suffix);
    let new_path = match p.parent() {
        Some(parent) if parent != Path::new("") => {
            parent.join(new_filename).to_string_lossy().to_string()
        }
        _ => new_filename,
    };
    Some(new_path)
}

/// Like `try_rename_in_path` but errors if the path doesn't start with
/// `old_stem` (used for the raw_note path, which must always match).
fn rename_in_path(rel_path: &str, old_stem: &str, new_stem: &str) -> Result<String> {
    try_rename_in_path(rel_path, old_stem, new_stem).ok_or_else(|| {
        anyhow::anyhow!(
            "raw_note path '{}' doesn't start with expected stem '{}'",
            rel_path,
            old_stem
        )
    })
}

// ── Pre-flight: unregistered file check ──────────────────────────────────────

/// Bail if any files in the raw directories match an old filename format used
/// by registered sessions but are not themselves registered in the manifest.
///
/// This enforces "run `csnotes reconcile` before changing `filename_format`"
/// so that we don't silently leave orphaned files behind.
fn check_no_unregistered(
    vault_root: &Path,
    config: &VaultConfig,
    manifest: &Manifest,
) -> Result<()> {
    if manifest.sessions.is_empty() {
        return Ok(());
    }

    // Collect old formats from manifest entries
    let old_format_strs: HashSet<String> = manifest
        .sessions
        .values()
        .map(|e| e.filename_format.clone())
        .collect();
    let old_fmts: Vec<FilenameFormat> = old_format_strs
        .iter()
        .filter_map(|s| FilenameFormat::parse(s).ok())
        .collect();

    if old_fmts.is_empty() {
        return Ok(());
    }

    // Paths already registered as raw notes
    let known_raw: HashSet<String> = manifest.sessions.values()
        .map(|e| e.raw_note.clone())
        .collect();

    // Course roots (mirrors reconcile logic)
    let course_roots: Vec<PathBuf> = if config.active_courses.is_empty() {
        vec![vault_root.to_path_buf()]
    } else {
        config
            .active_courses
            .iter()
            .map(|c| vault_root.join(c))
            .collect()
    };

    let mut unregistered = Vec::new();
    for course_root in &course_roots {
        let raw_dir = course_root.join(&config.raw_dir);
        if !raw_dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&raw_dir)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().map_or(false, |x| x == "md")
            })
        {
            let rel = entry
                .path()
                .strip_prefix(vault_root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            if known_raw.contains(&rel) {
                continue;
            }
            let stem = match entry.path().file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            // Does this unregistered file match any old format?
            if old_fmts.iter().any(|fmt| fmt.try_parse_stem(stem).is_some()) {
                unregistered.push(rel);
            }
        }
    }

    if !unregistered.is_empty() {
        let list = unregistered
            .iter()
            .map(|s| format!("  {}", s))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "Found unregistered session file(s) matching the old filename format.\n\
             Run `csnotes reconcile` to register them before migrating:\n{}",
            list
        );
    }

    Ok(())
}

// ── --set key=value ───────────────────────────────────────────────────────────

fn apply_set(config: &mut VaultConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "filename_format" => {
            FilenameFormat::parse(value)?;
            config.filename_format = value.to_string();
        }
        "raw_dir" => {
            ensure_no_spaces(value, "raw_dir")?;
            config.raw_dir = value.to_string();
        }
        "recordings_dir" => {
            ensure_no_spaces(value, "recordings_dir")?;
            config.recordings_dir = value.to_string();
        }
        "require_recordings" => {
            config.require_recordings = match value {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => bail!("require_recordings must be true or false"),
            };
        }
        "artifacts_dir" => {
            ensure_no_spaces(value, "artifacts_dir")?;
            config.artifacts_dir = value.to_string();
        }
        "sources_dir" => {
            ensure_no_spaces(value, "sources_dir")?;
            config.sources_dir = value.to_string();
        }
        "default_backend" => {
            config.default_backend = match value {
                "claude" => crate::config::AiBackend::Claude,
                "agy" => crate::config::AiBackend::Agy,
                "mock" => crate::config::AiBackend::Mock,
                _ => bail!("unknown backend '{}' (claude | agy | mock)", value),
            };
        }
        "archive_threshold_weeks" => {
            config.archive_threshold_weeks = value
                .parse()
                .map_err(|_| anyhow::anyhow!("archive_threshold_weeks must be a positive integer"))?;
        }
        "agy_model" => {
            if value.is_empty() {
                config.agy_model = None;
            } else {
                config.agy_model = Some(value.to_string());
            }
        }
        _ => bail!(crate::error::CsnotesError::InvalidConfigKey { key: key.to_string() }),
    }
    Ok(())
}
