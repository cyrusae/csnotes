use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{
    ensure_no_spaces, find_vault_root, AiBackend, FilenameFormat, SkillVariant, SnapshotMode,
    VaultConfig,
};
use crate::flags::FlagStore;
use crate::manifest::{Manifest, ManifestConfig, ManifestLock};

/// `csnotes init`
///
/// With no flags: scaffolds a new vault (prompts for config, creates
/// directories, writes `csnotes.toml`, manifest, flag store, instruction files).
///
/// With `--instructions-only`: writes/updates the three instruction files in
/// the current vault's `_csnotes/instructions/` without touching anything else.
/// Useful for bootstrapping an existing vault or updating stale instructions.
pub fn run(vault_root: Option<PathBuf>, instructions_only: bool) -> Result<()> {
    if instructions_only {
        return run_instructions_only();
    }

    let vault_root = vault_root.unwrap_or_else(|| std::env::current_dir().unwrap());
    let vault_root = vault_root.canonicalize().unwrap_or(vault_root);

    println!("Initialising csnotes vault at: {}", vault_root.display());
    println!("(Press Enter to accept defaults)\n");

    // ── Prompts ────────────────────────────────────────────────────────────────
    let filename_format = prompt(
        "Filename format [{course}-{mm}-{dd}]: ",
        "{course}-{mm}-{dd}",
    )?;
    FilenameFormat::parse(&filename_format).context("invalid filename format")?;

    // Detect existing course-like folders.  If found, per-course layout is
    // implied and we skip the explicit question.  For a fresh vault with
    // nothing to detect, ask first so the user isn't surprised by the
    // directory structure that gets created.
    let candidates = scan_course_candidates(&vault_root);
    let active_courses = if !candidates.is_empty() {
        prompt_courses(&candidates)?
    } else {
        let per_course = prompt_bool("Use per-course folder layout? [Y/n]: ", true)?;
        if per_course {
            prompt_courses(&[])?
        } else {
            vec![]
        }
    };

    let raw_dir = prompt("Raw notes directory [notes]: ", "notes")?;
    ensure_no_spaces(&raw_dir, "raw_dir")?;

    let backend_str = prompt("Default AI backend [claude/agy]: ", "claude")?;
    let default_backend = match backend_str.trim() {
        "agy" => AiBackend::Agy,
        _ => AiBackend::Claude,
    };

    let skill_variant = match default_backend {
        AiBackend::Agy => SkillVariant::Gemini,
        _ => SkillVariant::Claude,
    };

    let cfg = VaultConfig {
        raw_dir,
        recordings_dir: "recordings".into(),
        artifacts_dir: "artifacts".into(),
        sources_dir: "sources".into(),
        resources_dir: "resources".into(),
        synthetic_dir: "_synthetic".into(),
        generated_dir: "_generated".into(),
        csnotes_dir: "_csnotes".into(),
        filename_format,
        active_courses,
        default_backend,
        skill_variant,
        snapshot_mode: SnapshotMode::PreMerge,
        archive_threshold_weeks: 8,
        recording_qualifiers: vec!["transcript".into(), "summary".into(), "mindmap".into()],
        agy_model: None,
        require_recordings: true,
        courses_without_recordings: vec![],
        scan_ai_conversations: true,
        sources_ignore_dirs: vec![],
    };

    // ── Create directory tree ──────────────────────────────────────────────────
    create_vault_dirs(&vault_root, &cfg)?;

    // Instructions directory
    let instructions_dir = vault_root.join(&cfg.csnotes_dir).join("instructions");
    fs::create_dir_all(&instructions_dir)?;

    // ── Write csnotes.toml ────────────────────────────────────────────────────
    let config_path = vault_root.join("csnotes.toml");
    if config_path.exists() {
        println!("  Skipped: csnotes.toml (already exists)");
    } else {
        cfg.save(&vault_root)?;
        println!("  Created: csnotes.toml");
    }

    // ── Write empty manifest ───────────────────────────────────────────────────
    let manifest_path = vault_root.join(crate::manifest::MANIFEST_FILENAME);
    if manifest_path.exists() {
        println!("  Skipped: csnotes.json (already exists)");
    } else {
        let _lock = ManifestLock::acquire(&vault_root)?;
        let manifest_config = ManifestConfig::from_vault_config(&cfg);
        let manifest = Manifest::empty(vault_root.clone(), manifest_config);
        manifest.save(&vault_root)?;
        println!("  Created: csnotes.json");
    }

    // ── Write empty flag store ─────────────────────────────────────────────────
    let flags_path = vault_root.join(&cfg.generated_dir).join("flags.json");
    if !flags_path.exists() {
        let flag_store = FlagStore::default();
        flag_store.save(&flags_path)?;
        println!("  Created: {}/flags.json", cfg.generated_dir);
    }

    // ── Write instruction files ────────────────────────────────────────────────
    write_instruction_files(&instructions_dir, &cfg, false)?;

    // ── Done ──────────────────────────────────────────────────────────────────
    println!("\nVault initialised.");
    println!("  Edit _csnotes/instructions/ before your first session.");
    println!("\nNext steps:");
    if cfg.active_courses.is_empty() {
        println!("  1. Add your course(s) to active_courses in csnotes.toml");
        println!("  2. csnotes reconcile");
        println!("  3. csnotes process");
    } else {
        for course in &cfg.active_courses {
            println!("  • Add raw notes to {}/{}/", course, cfg.raw_dir);
        }
        println!("  1. csnotes reconcile");
        println!("  2. csnotes process");
    }

    Ok(())
}

// ── --instructions-only ───────────────────────────────────────────────────────

fn run_instructions_only() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let vault_root = find_vault_root(&cwd)?;
    let config = VaultConfig::load(&vault_root)?;

    let instructions_dir = vault_root.join(&config.csnotes_dir).join("instructions");
    fs::create_dir_all(&instructions_dir)?;

    write_instruction_files(&instructions_dir, &config, true)?;
    println!(
        "Instructions written to {}/instructions/",
        config.csnotes_dir
    );

    Ok(())
}

// ── Instruction file writing ──────────────────────────────────────────────────

fn write_instruction_files(dir: &Path, cfg: &VaultConfig, force: bool) -> Result<()> {
    // claude.md / gemini.md — variant-specific entry point
    // CourseReview is runtime-only and never stored in config, but handle it gracefully.
    let (entry_name, entry_content) = match cfg.skill_variant {
        SkillVariant::Claude | SkillVariant::CourseReview | SkillVariant::ResourceReview => {
            ("claude.md", CLAUDE_MD)
        }
        SkillVariant::Gemini => ("gemini.md", GEMINI_MD),
    };
    write_instruction(dir, entry_name, entry_content, force)?;

    // Always write both backend entry points and shared files.
    if cfg.skill_variant == SkillVariant::Gemini {
        write_instruction(dir, "claude.md", CLAUDE_MD, force)?;
    } else {
        write_instruction(dir, "gemini.md", GEMINI_MD, force)?;
    }
    write_instruction(dir, "course-review.md", COURSE_REVIEW_MD, force)?;
    write_instruction(dir, "resource-review.md", RESOURCE_REVIEW_MD, force)?;
    write_instruction(dir, "synthesis.md", SYNTHESIS_MD, force)?;
    write_instruction(dir, "csnotes_reference.md", CSNOTES_REFERENCE_MD, force)?;
    let schema_md = crate::commands::report_schema_cmd::generate();
    write_instruction(dir, "report_schema.md", &schema_md, force)?;

    let _ = entry_name; // suppress unused warning
    Ok(())
}

fn write_instruction(dir: &Path, filename: &str, content: &str, force: bool) -> Result<()> {
    let path = dir.join(filename);
    if path.exists() && !force {
        println!(
            "  Skipped: _csnotes/instructions/{} (already exists)",
            filename
        );
    } else {
        let verb = if path.exists() { "Updated" } else { "Created" };
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        println!("  {}: _csnotes/instructions/{}", verb, filename);
    }
    Ok(())
}

// ── Embedded instruction files ────────────────────────────────────────────────
//
// `_csnotes/instructions/` in this repo is the single source of truth: edit
// the .md files there, never a copy in Rust.  `report_schema.md` is the one
// exception — it is generated by `report_schema_cmd::generate()` and the
// checked-in copy is kept in sync by a test.

const CLAUDE_MD: &str = include_str!("../../_csnotes/instructions/claude.md");
const GEMINI_MD: &str = include_str!("../../_csnotes/instructions/gemini.md");
const COURSE_REVIEW_MD: &str = include_str!("../../_csnotes/instructions/course-review.md");
const RESOURCE_REVIEW_MD: &str = include_str!("../../_csnotes/instructions/resource-review.md");
const CSNOTES_REFERENCE_MD: &str = include_str!("../../_csnotes/instructions/csnotes_reference.md");
const SYNTHESIS_MD: &str = include_str!("../../_csnotes/instructions/synthesis.md");

// ── Directory helpers ─────────────────────────────────────────────────────────

fn create_dir_if_absent(vault_root: &Path, rel: &str) -> Result<()> {
    let path = vault_root.join(rel);
    if !path.exists() {
        fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        println!("  Created: {}/", rel);
    }
    Ok(())
}

/// Create the vault directory tree under `vault_root` according to `cfg`.
///
/// `_synthetic` and `_generated` are always created at vault root.
/// Content dirs (`raw_dir`, `recordings_dir`, `artifacts_dir`, `sources_dir`)
/// are created flat at vault root when `active_courses` is empty, or nested
/// under each course folder when courses are configured.
pub fn create_vault_dirs(vault_root: &Path, cfg: &VaultConfig) -> Result<()> {
    for d in &[cfg.synthetic_dir.as_str(), cfg.generated_dir.as_str()] {
        create_dir_if_absent(vault_root, d)?;
    }

    let content_dirs = [
        cfg.raw_dir.as_str(),
        cfg.recordings_dir.as_str(),
        cfg.artifacts_dir.as_str(),
        cfg.sources_dir.as_str(),
    ];

    if cfg.active_courses.is_empty() {
        for d in &content_dirs {
            create_dir_if_absent(vault_root, d)?;
        }
    } else {
        for course in &cfg.active_courses {
            for d in &content_dirs {
                let rel = format!("{}/{}", course, d);
                create_dir_if_absent(vault_root, &rel)?;
            }
        }
    }

    Ok(())
}

// ── Course detection ──────────────────────────────────────────────────────────

/// Scan `vault_root` for subdirectories that look like course folders.
///
/// Excludes hidden dirs (`.`-prefixed), system dirs (`_`-prefixed), and a
/// fixed list of names that init itself might create.  What remains is
/// presented to the user as candidates for `active_courses`.
pub fn scan_course_candidates(vault_root: &Path) -> Vec<String> {
    const EXCLUDE: &[&str] = &[
        "notes",
        "recordings",
        "artifacts",
        "sources",
        "target",
        "node_modules",
    ];

    let Ok(entries) = std::fs::read_dir(vault_root) else {
        return vec![];
    };

    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with('.') || name.starts_with('_') {
                return None;
            }
            if EXCLUDE.contains(&name.as_str()) {
                return None;
            }
            if name.contains(' ') {
                return None;
            }
            if !e.path().is_dir() {
                return None;
            }
            Some(name)
        })
        .collect();

    candidates.sort();
    candidates
}

/// Prompt the user for active courses, pre-filling with detected candidates.
///
/// - If candidates were found: shows them and accepts Enter to use all of them,
///   or lets the user type a space-separated override list.
/// - If no candidates: plain prompt for a space-separated list (may be empty).
///
/// Each course name is validated with `ensure_no_spaces`.
fn prompt_courses(candidates: &[String]) -> Result<Vec<String>> {
    let raw = if candidates.is_empty() {
        prompt(
            "Active courses (space-separated, e.g. CPSC5001 CPSC5002), or leave blank: ",
            "",
        )?
    } else {
        let detected = candidates.join(" ");
        println!("Detected course folders: {}", detected);
        prompt(
            "Active courses [Enter to accept all, or type space-separated list]: ",
            &detected,
        )?
    };

    let courses: Vec<String> = raw.split_whitespace().map(|s| s.to_string()).collect();

    for c in &courses {
        ensure_no_spaces(c, "course name")?;
    }

    Ok(courses)
}

// ── Prompt helpers ────────────────────────────────────────────────────────────

fn prompt_bool(message: &str, default: bool) -> Result<bool> {
    print!("{}", message);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(match input.trim().to_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    })
}

fn prompt(message: &str, default: &str) -> Result<String> {
    print!("{}", message);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup(dirs: &[&str], files: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for d in dirs {
            fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        for f in files {
            fs::write(tmp.path().join(f), "").unwrap();
        }
        tmp
    }

    #[test]
    fn scan_returns_course_like_dirs() {
        let tmp = setup(&["CPSC5001", "CPSC5002"], &[]);
        let mut got = scan_course_candidates(tmp.path());
        got.sort();
        assert_eq!(got, vec!["CPSC5001", "CPSC5002"]);
    }

    #[test]
    fn scan_excludes_system_dirs() {
        let tmp = setup(
            &[
                "CPSC5001",
                "_synthetic",
                "_generated",
                "notes",
                "recordings",
                "artifacts",
                "sources",
            ],
            &[],
        );
        let got = scan_course_candidates(tmp.path());
        assert_eq!(got, vec!["CPSC5001"]);
    }

    #[test]
    fn scan_excludes_hidden_dirs() {
        let tmp = setup(&["CPSC5001", ".git", ".obsidian"], &[]);
        let got = scan_course_candidates(tmp.path());
        assert_eq!(got, vec!["CPSC5001"]);
    }

    #[test]
    fn scan_excludes_files() {
        let tmp = setup(&["CPSC5001"], &["csnotes.toml", "csnotes.json"]);
        let got = scan_course_candidates(tmp.path());
        assert_eq!(got, vec!["CPSC5001"]);
    }

    #[test]
    fn scan_returns_empty_when_no_candidates() {
        let tmp = setup(&["notes", "_synthetic"], &[]);
        let got = scan_course_candidates(tmp.path());
        assert!(got.is_empty());
    }

    #[test]
    fn scan_results_are_sorted() {
        let tmp = setup(&["CPSC5005", "CPSC5001", "CPSC5002"], &[]);
        let got = scan_course_candidates(tmp.path());
        assert_eq!(got, vec!["CPSC5001", "CPSC5002", "CPSC5005"]);
    }

    fn minimal_config(active_courses: Vec<String>) -> VaultConfig {
        VaultConfig {
            raw_dir: "notes".into(),
            recordings_dir: "recordings".into(),
            artifacts_dir: "artifacts".into(),
            sources_dir: "sources".into(),
            resources_dir: "resources".into(),
            synthetic_dir: "_synthetic".into(),
            generated_dir: "_generated".into(),
            csnotes_dir: "_csnotes".into(),
            filename_format: "{course}-{mm}-{dd}".into(),
            active_courses,
            default_backend: crate::config::AiBackend::Mock,
            skill_variant: crate::config::SkillVariant::Claude,
            snapshot_mode: crate::config::SnapshotMode::PreMerge,
            archive_threshold_weeks: 8,
            recording_qualifiers: vec![],
            agy_model: None,
            require_recordings: false,
            courses_without_recordings: vec![],
            scan_ai_conversations: true,
            sources_ignore_dirs: vec![],
        }
    }

    #[test]
    fn create_vault_dirs_flat_when_no_courses() {
        let tmp = TempDir::new().unwrap();
        let cfg = minimal_config(vec![]);
        create_vault_dirs(tmp.path(), &cfg).unwrap();

        for d in &[
            "notes",
            "recordings",
            "artifacts",
            "sources",
            "_synthetic",
            "_generated",
        ] {
            assert!(tmp.path().join(d).is_dir(), "expected flat dir: {}", d);
        }
    }

    #[test]
    fn create_vault_dirs_per_course_when_courses_set() {
        let tmp = TempDir::new().unwrap();
        let cfg = minimal_config(vec!["CPSC5001".into(), "CPSC5002".into()]);
        create_vault_dirs(tmp.path(), &cfg).unwrap();

        // Shared dirs always at root.
        assert!(tmp.path().join("_synthetic").is_dir());
        assert!(tmp.path().join("_generated").is_dir());

        // Content dirs nested under each course.
        for course in &["CPSC5001", "CPSC5002"] {
            for d in &["notes", "recordings", "artifacts", "sources"] {
                let p = tmp.path().join(course).join(d);
                assert!(p.is_dir(), "expected {}/{}", course, d);
            }
        }

        // Flat content dirs must NOT exist at root.
        for d in &["notes", "recordings", "artifacts", "sources"] {
            assert!(
                !tmp.path().join(d).exists(),
                "flat {} should not exist when courses set",
                d
            );
        }
    }

    #[test]
    fn create_vault_dirs_skips_existing() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("CPSC5001/notes")).unwrap();
        let cfg = minimal_config(vec!["CPSC5001".into()]);
        // Should not error on pre-existing dirs.
        create_vault_dirs(tmp.path(), &cfg).unwrap();
        assert!(tmp.path().join("CPSC5001/notes").is_dir());
    }

    // ── Embedded instruction content ─────────────────────────────────────────

    #[test]
    fn synthesis_md_keeps_bullets_not_prose_rule() {
        assert!(
            SYNTHESIS_MD.contains("**Bullets, not prose.**"),
            "synthesis.md lost its 'Bullets, not prose' voice rule"
        );
    }

    #[test]
    fn checked_in_report_schema_matches_generator() {
        let checked_in = include_str!("../../_csnotes/instructions/report_schema.md");
        assert_eq!(
            checked_in,
            crate::commands::report_schema_cmd::generate(),
            "_csnotes/instructions/report_schema.md is stale; regenerate it with \
             `cargo run -q -- report-schema > _csnotes/instructions/report_schema.md`"
        );
    }

    #[test]
    fn write_instruction_files_writes_embedded_content() {
        let tmp = TempDir::new().unwrap();
        let cfg = minimal_config(vec![]);
        write_instruction_files(tmp.path(), &cfg, true).unwrap();
        for (name, expected) in [
            ("claude.md", CLAUDE_MD),
            ("gemini.md", GEMINI_MD),
            ("course-review.md", COURSE_REVIEW_MD),
            ("resource-review.md", RESOURCE_REVIEW_MD),
            ("synthesis.md", SYNTHESIS_MD),
            ("csnotes_reference.md", CSNOTES_REFERENCE_MD),
        ] {
            let written = fs::read_to_string(tmp.path().join(name)).unwrap();
            assert_eq!(written, expected, "{name} content mismatch");
        }
    }
}
