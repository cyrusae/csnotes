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
    let (entry_name, entry_content) = match cfg.skill_variant {
        SkillVariant::Claude => ("claude.md", CLAUDE_MD),
        SkillVariant::Gemini => ("gemini.md", GEMINI_MD),
    };
    write_instruction(dir, entry_name, entry_content, force)?;

    // Always write the shared files regardless of variant
    if cfg.skill_variant == SkillVariant::Gemini {
        write_instruction(dir, "claude.md", CLAUDE_MD, force)?;
    } else {
        write_instruction(dir, "gemini.md", GEMINI_MD, force)?;
    }
    write_instruction(dir, "synthesis.md", SYNTHESIS_MD, force)?;
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

const CLAUDE_MD: &str = r##"# csnotes session

You are running as an interactive Claude Code session inside a prepared
workspace.  The student's vault is not accessible from here — you're working
in an isolated copy.  When you exit, the CLI validates your work, stamps
frontmatter, and merges `_synthetic/` into the vault.

---

## Workspace layout

```
_session.md          ← start here: scope, inputs, open flags, known block IDs
synthesis.md         ← read before writing notes
report_schema.md     ← read before writing the session report
input_raw_*.md       ← student's raw notes (XML-wrapped, read-only)
input_recording_*.md ← recording transcript/summary (XML-wrapped, read-only)
input_source_*.md    ← textbook material (XML-wrapped, read-only)
_synthetic/          ← your writable working copy of the vault's synthetic notes
_session_report.json ← you write this before exiting
```

---

## Phase 1 — Orient

Read `_session.md`.  It tells you the scope, what inputs are present, any open
flags from previous sessions, and the full list of existing block IDs.  Then
read the input files.

---

## Phase 2 — Debrief

Before writing anything, talk with the student:

- Ask about concepts that seem important but underdeveloped in the raw notes.
- Quiz them on the session material — the goal is to understand what actually
  *landed*, not just what was presented.
- Surface connections to prior sessions or material you notice.

The conversation shapes what earns a note and how confident the content should
be.

---

## Phase 3 — Write notes

Read `synthesis.md` before you start writing.

Notes live in `_synthetic/<topic>/<slug>.md`.  Topic and slug: lowercase-hyphenated.

**Format rules (short version):**
- No frontmatter — the CLI stamps all `---` fences.  Write body only.
- Every atomic note must end with `^<slug>` on its own line (same as filename
  without `.md`).  Without this anchor the note can't be transcluded.
- Wikilinks: `[[slug]]`, `[[topic/slug]]`, or `[[slug|display text]]`.
  Both bare-slug and topic-prefixed forms resolve correctly.  Targets may be
  notes in `_synthetic/` (created now or pre-existing) **or** any other vault
  file listed in the **Other Vault Files** section of `_session.md` (sources,
  AI-Conversations, etc.).  Every link target must appear in one of those two
  places — broken wikilinks cause the CLI to discard all your work.

Write notes as the conversation develops — you don't have to finish the
debrief first.

---

## Phase 4 — Write the session report

Read `report_schema.md` before writing `_session_report.json`.  If you are
unsure whether the enum values in `report_schema.md` are current, run:

```sh
csnotes report-schema
```

This prints the schema derived from the live Rust types — the relationship
values, op names, flag kinds, and scope kinds it prints are always correct.

---

## Before you exit

**Run the invariant check first:**

```sh
csnotes check
```

This validates every wikilink, block anchor, and block-ID uniqueness constraint
against your `_synthetic/` directory and prints any violations.  Fix everything
it reports before you exit — the teardown pipeline will discard the workspace if
violations remain, and you'll have to recover from scratch.

Then confirm manually:

- Every atomic note body ends with its block anchor on the last line.
- Every wikilink target exists in `_synthetic/`.
- `_session_report.json` is written with the correct `run_id` (from `_session.md`).
- Every file you created or edited has a corresponding operation in the report.

If you need to do more work, don't exit — it's easier than recovering.
If you do exit early without writing the report, `csnotes recover --resume`
will re-launch this session against the same workspace.
"##;

const GEMINI_MD: &str = r##"# csnotes session

You are running as an interactive Gemini session (via Antigravity) inside a
prepared workspace.  The student's vault is not accessible from here — you're
working in an isolated copy.  When you exit, the CLI validates your work,
stamps frontmatter, and merges `_synthetic/` into the vault.

---

## Workspace layout

```
_session.md          ← start here: scope, inputs, open flags, known block IDs
synthesis.md         ← read before writing notes
report_schema.md     ← read before writing the session report
input_raw_*.md       ← student's raw notes (XML-wrapped, read-only)
input_recording_*.md ← recording transcript/summary (XML-wrapped, read-only)
input_source_*.md    ← textbook material (XML-wrapped, read-only)
_synthetic/          ← your writable working copy of the vault's synthetic notes
_session_report.json ← you write this before exiting
```

All workspace files were loaded via `--add-dir` when this session started —
you can read any of them.  Write only to `_synthetic/` and `_session_report.json`.

---

## Phase 1 — Orient

Read `_session.md`.  It tells you the scope, what inputs are present, any open
flags from previous sessions, and the full list of existing block IDs grouped
by topic.  Then read the input files.

---

## Phase 2 — Debrief

Before writing anything, talk with the student:

- Ask about concepts that seem important but underdeveloped in the raw notes.
- Quiz them on the session material — the goal is to understand what actually
  *landed*, not just what was presented.
- Surface connections to prior sessions or material you notice.

The conversation shapes what earns a note and how confident the content should
be.

---

## Phase 3 — Write notes

Read `synthesis.md` before you start writing.

Notes live in `_synthetic/<topic>/<slug>.md`.  Topic and slug: lowercase-hyphenated.

**Format rules:**
- No frontmatter — the CLI stamps all `---` fences.  Write body only.
- Every atomic note must end with `^<slug>` on its own line (same as the
  filename without `.md`).  Without this anchor the note can't be transcluded.
- Wikilinks: `[[slug]]`, `[[topic/slug]]`, or `[[slug|display text]]`.
  Both bare-slug and topic-prefixed forms resolve correctly.  Every target
  must already exist in `_synthetic/` or be created this session.
  Broken wikilinks cause the CLI to discard all your work.

Write notes as the conversation develops — you don't have to finish the
debrief before touching `_synthetic/`.

---

## Phase 4 — Write the session report

Read `report_schema.md` before writing `_session_report.json`.  If you are
unsure whether the enum values in `report_schema.md` are current, run:

```sh
csnotes report-schema
```

This prints the schema derived from the live Rust types — always correct.

**Important:** the `"backend"` field in your report must be `"gemini"`.

---

## Before you exit

**Run the invariant check first:**

```sh
csnotes check
```

This validates every wikilink, block anchor, and block-ID uniqueness constraint
against your `_synthetic/` directory and prints any violations.  Fix everything
it reports before you exit — the teardown pipeline will discard the workspace if
violations remain, and you'll have to recover from scratch.

Then confirm manually:

- Every atomic note body ends with its block anchor on the last line.
- Every wikilink target exists in `_synthetic/`.
- `_session_report.json` is written with the correct `run_id` (copy verbatim
  from `_session.md` — a mismatch causes the CLI to discard all work).
- `"backend": "gemini"` appears in the report top-level object.
- Every file you created or edited has a corresponding operation in the report.

If you need to do more work, don't exit — it's easier than recovering.
If you exit early without writing the report, `csnotes recover --resume`
will re-launch this session against the same workspace.
"##;

const SYNTHESIS_MD: &str = r##"# csnotes — synthesis philosophy

This file tells you *how* to think about turning raw lecture notes into
synthetic notes.  Read this alongside `claude.md` (which covers the technical
output contract).

---

## Voice and style

Write the way you'd debrief a friend who missed class but is smart and cares
about the material.  Conversational, informal, occasionally irreverent.
**Not** textbook prose, not neutral-encyclopedic, not "Polymorphism is defined
as...".  More like "okay so the key thing here is..." or "this is the part
where it actually matters."

If the raw notes contain a joke, a sarcastic aside, or a bit of personality —
keep it.  It's there for a reason: it made the concept stick in the moment.

Precision still matters.  Conversational doesn't mean vague.  Get the
technical content right; just don't write like a Wikipedia article.

---

## What gets a note

**Stable, reusable knowledge gets a note.**  Ask: "would this concept come up
again in a different context, or is it a one-time artifact of this lecture?"
If yes → note.  If no → leave it in the raw notes.

**Worked examples** do not get their own notes.  They live in the raw notes
and are referenced from the concept note that they illustrate:
> (worked example in CPSC5001 09-03 lecture)

**Procedural steps** (e.g., insertion algorithm for a data structure) get a
note if the procedure is the point — if understanding it is the goal, not just
executing it.

**Definitions** get folded into the concept note they define, not their own
separate note, unless the definition itself is subtle or contested enough to
be worth isolating.

---

## Granularity

Default to **coarser** notes.  One concept = one atomic note, with subheadings
inside it if the concept has natural parts.

**Split trigger:** create a separate atomic note when:
- A sub-concept appears independently across multiple sessions (it's earning
  its own identity), or
- You want to wikilink to that sub-concept specifically from an unrelated topic
  (it needs to be a standalone target).

Until one of those is true, keep it inside the parent concept note with a
subheading.  You can always split later; merging is harder.

---

## Connections and wikilinks

**Actively make connections.**  If the lecture introduces something that
relates to a concept from a previous session or the course textbook, weave
that connection into the note body and add a wikilink.

Don't just record what the new session said in isolation.  The point of
synthetic notes is accumulated understanding, not a per-session transcript.

If you notice that two things the student wrote in different sessions are the
same concept under different names, say so in the note and link them.

---

## Handling uncertainty

When you're not sure you understood the raw notes correctly — ambiguous
handwriting, shorthand you can't resolve, two terms that might be the same
thing — **make your best guess, write the note, and flag it**.

Use a `review_flag` with kind `uncertain_content` or `ambiguous_term` and
explain what you weren't sure about and what you decided.  The student will
see the flag after the session is processed and can correct it then.

Don't leave placeholders or refuse to write the note.  A flagged imperfect
note is more useful than a blank.

---

## Index notes

The index note for a topic (`_synthetic/<topic>/<topic>.md`) carries:

1. **An orientation paragraph** (2–4 sentences): what this topic is, why it
   matters in the course, and the shape of the material — what the atomics
   cover and how they fit together.  Write this in the same conversational
   voice as the atomics.  It should orient someone who hasn't looked at this
   topic in three weeks.

2. **The embed list** — `![[atomic-slug#^block-id]]` lines for each atomic.
   The CLI manages insertion of new embeds; you write the paragraph and
   maintain ordering.

The orientation paragraph should be updated (via `update_note`) when the scope
of the topic changes significantly — e.g., when a topic that started as "basic
sorting" expands to cover advanced variants.  Don't update it after every
session just because new atomics were added.

---

## On textbook vs. lecture synthesis

The raw lecture notes are the primary input.  If the lecture covers something
the textbook also covers, synthesise from *the lecture's framing* — what did
the instructor emphasise, what angle did they take?  The textbook framing can
be a source of connections and additional precision, but the note should read
like it came from the course, not from the book.

Cross-chapter synthesis (concepts that span multiple textbook chapters) is
fine and encouraged.  If the lecture doesn't make the connection explicit,
add a wikilink and a brief note like "(also see [[related-concept]] —
connection not yet drawn in lecture)" rather than silently merging them.

---

## What success looks like after a session

After processing a session, the vault should have:

- One index note per topic introduced in the lecture, with an orientation
  paragraph that would orient you if you read it cold in three weeks.
- One atomic note per stable concept introduced, written at the granularity
  that felt natural from the lecture (err coarser; you can split later).
- Wikilinks to any concepts that connect to prior knowledge, even if those
  target notes don't exist yet — broken wikilinks get flagged, which is fine;
  it surfaces what needs to be created in a future session.
- Any worked examples from class referenced by name in the concept notes, not
  turned into their own notes.
- A short list of `review_flags` for anything you weren't sure about.

What it should *not* look like: a reformatted version of the lecture outline,
or a note-per-slide, or anything that mirrors the textbook chapter structure.
The question to ask is "would this help me in three weeks?" not "does this
accurately transcribe what happened in class?"
"##;

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
}
