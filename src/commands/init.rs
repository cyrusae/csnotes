use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{
    ensure_no_spaces, AiBackend, FilenameFormat, SkillVariant, SnapshotMode, VaultConfig,
};
use crate::flags::FlagStore;
use crate::manifest::{Manifest, ManifestConfig};

/// `csnotes init`
///
/// Scaffolds a new vault: prompts for configuration, creates the directory
/// tree, writes `.csnotes`, an empty manifest, an empty flag store, and
/// instruction stub files.
pub fn run(vault_root: Option<PathBuf>) -> Result<()> {
    let vault_root = vault_root.unwrap_or_else(|| std::env::current_dir().unwrap());
    let vault_root = vault_root.canonicalize().unwrap_or(vault_root);

    println!("Initialising csnotes vault at: {}", vault_root.display());
    println!("(Press Enter to accept defaults)\n");

    // ── Prompts ────────────────────────────────────────────────────────────
    let filename_format = prompt(
        "Filename format [{course}-{mm}-{dd}]: ",
        "{course}-{mm}-{dd}",
    )?;
    FilenameFormat::parse(&filename_format)
        .context("invalid filename format")?;

    let default_course = prompt("Default course (e.g. CS501): ", "")?;
    if !default_course.is_empty() {
        ensure_no_spaces(&default_course, "course name")?;
    }

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
        plaud_dir: "plaud".into(),
        artifacts_dir: "artifacts".into(),
        sources_dir: "sources".into(),
        synthetic_dir: "_synthetic".into(),
        generated_dir: "_generated".into(),
        csnotes_dir: "_csnotes".into(),
        filename_format,
        active_courses: if default_course.is_empty() {
            vec![]
        } else {
            vec![default_course]
        },
        default_backend,
        skill_variant,
        snapshot_mode: SnapshotMode::PreMerge,
        archive_threshold_weeks: 8,
        plaud_qualifiers: vec!["transcript".into(), "summary".into(), "mindmap".into()],
    };

    // ── Create directory tree ──────────────────────────────────────────────
    let dirs = [
        &cfg.raw_dir,
        &cfg.plaud_dir,
        &cfg.artifacts_dir,
        &cfg.sources_dir,
        &cfg.synthetic_dir,
        &cfg.generated_dir,
    ];
    for d in &dirs {
        let path = vault_root.join(d);
        fs::create_dir_all(&path)
            .with_context(|| format!("creating {}", path.display()))?;
    }

    // Instructions directory
    let instructions_dir = vault_root
        .join(&cfg.csnotes_dir)
        .join("instructions");
    fs::create_dir_all(&instructions_dir)?;

    // ── Write .csnotes ─────────────────────────────────────────────────────
    cfg.save(&vault_root)?;

    // ── Write empty manifest ───────────────────────────────────────────────
    let manifest_config = ManifestConfig::from_vault_config(&cfg);
    let manifest = Manifest::empty(vault_root.clone(), manifest_config);
    manifest.save(&vault_root)?;

    // ── Write empty flag store ─────────────────────────────────────────────
    let flag_store = FlagStore::default();
    flag_store.save(&vault_root.join(&cfg.generated_dir).join("flags.json"))?;

    // ── Write instruction files ────────────────────────────────────────────
    write_instruction_files(&vault_root, &cfg)?;

    // ── Done ───────────────────────────────────────────────────────────────
    println!("\nVault initialised.");
    println!(
        "  Instruction files: {}/instructions/",
        cfg.csnotes_dir
    );
    println!("  Edit claude.md / gemini.md / synthesis.md before your first session.");
    println!("\nNext: add a raw note to {}/{}/",  cfg.raw_dir, cfg.active_courses.first().unwrap_or(&"<course>".to_string()));
    println!("      then run: csnotes process");

    Ok(())
}

// ── Instruction file stubs ────────────────────────────────────────────────────

fn write_instruction_files(vault_root: &Path, cfg: &VaultConfig) -> Result<()> {
    let dir = vault_root.join(&cfg.csnotes_dir).join("instructions");

    // claude.md — written only if it doesn't exist
    let claude_path = dir.join("claude.md");
    if !claude_path.exists() {
        fs::write(&claude_path, CLAUDE_MD_STUB)?;
        println!("  Created: {}", claude_path.display());
    }

    // gemini.md
    let gemini_path = dir.join("gemini.md");
    if !gemini_path.exists() {
        fs::write(&gemini_path, GEMINI_MD_STUB)?;
        println!("  Created: {}", gemini_path.display());
    }

    // synthesis.md
    let synthesis_path = dir.join("synthesis.md");
    if !synthesis_path.exists() {
        fs::write(&synthesis_path, SYNTHESIS_MD_STUB)?;
        println!("  Created: {}", synthesis_path.display());
    }

    Ok(())
}

// ── Instruction stubs (TODO: replace with real content before first session) ──

const CLAUDE_MD_STUB: &str = r#"# csnotes — Claude Instructions

<!-- TODO: Fill in before your first session.  See _docs/PLAN.md §2 for the
     full directive list.  Key sections to write:

     1. Role: tutor + synthesiser dual persona
     2. Vault structure and input XML tags (raw_student_notes, plaud_transcript,
        plaud_summary, plaud_mindmap, lecture_slides, instructor_code_sample,
        textbook_source)
     3. Per-run state: read _session.md for current block IDs, open flags,
        and existing note coverage
     4. Tone matching: read raw notes for register before synthesising
     5. Synthesis rules: extend existing topics; restructure only on reframe/
        contradiction; label every section with contributing session/source
     6. Fact-checking asides format: > **[Claude: possible misread]** ...
     7. Atomisation heuristic (man-page level)
     8. Intellectual history: Current Understanding + Conceptual History
     9. Raw notes and Plaud exports are READ-ONLY — never suggest edits to them
    10. When ready to wrap up: "Reference synthesis.md for the procedure."
-->

You are a tutor and synthesiser for a graduate CS programme.
This workspace contains your session inputs and a working copy of _synthetic/.

**Read _session.md for the current session context before anything else.**
"#;

const GEMINI_MD_STUB: &str = r#"# csnotes — Gemini/Antigravity Instructions

<!-- TODO: Fill in before your first session.  Mirror of claude.md adapted
     for Gemini style.  See claude.md comments for the full directive list. -->

You are a tutor and synthesiser for a graduate CS programme.
This workspace contains your session inputs and a working copy of _synthetic/.

**Read _session.md for the current session context before anything else.**
"#;

const SYNTHESIS_MD_STUB: &str = r#"# Synthesis Procedure

<!-- TODO: Fill in the full synthesis wrap-up protocol.  Key sections:

     1. Pre-synthesis checklist
        - Have you discussed the material and resolved confusions?
        - Are there open questions worth flagging?

     2. For each topic touched this session:
        a. Decide: create_note (new topic/atomic) or update_note (existing)?
        b. Write or update the note body:
           - # Topic heading
           - ## Current Understanding (best synthesis)
           - ## Conceptual History (per-contribution with relationship label)
           - Block anchor ^block-id on atomic notes
           - ![[atomic#^block-id]] embed lines in index notes
        c. Fact-check asides for likely transcription errors

     3. Write _session_report.json:
        - One op per touched note (create_note or update_note)
        - Provenance deltas with correct relationship labels
        - review_flags for anything uncertain
        - change_summary for each op (used by `csnotes diff`)

     4. Verify the report before exiting:
        - run_id matches the value in _session.md
        - Every declared path exists in _synthetic/
        - Every declared block_id has a matching ^anchor in the body
        - Every declared embed_in target has the ![[...]] line
-->

When you are ready to wrap up and write the synthesis:

1. Review what was discussed.
2. Write or update note bodies in `_synthetic/`.
3. Write `_session_report.json` following the schema in this workspace.
4. Confirm the report is complete, then exit the session.
"#;

// ── Prompt helper ─────────────────────────────────────────────────────────────

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
