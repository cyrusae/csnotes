use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{
    ensure_no_spaces, AiBackend, FilenameFormat, SkillVariant, SnapshotMode, VaultConfig,
    find_vault_root,
};
use crate::flags::FlagStore;
use crate::manifest::{Manifest, ManifestConfig};

/// `csnotes init`
///
/// With no flags: scaffolds a new vault (prompts for config, creates
/// directories, writes `.csnotes`, manifest, flag store, instruction files).
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

    // ── Create directory tree ──────────────────────────────────────────────────
    let dirs = [
        cfg.raw_dir.as_str(),
        cfg.plaud_dir.as_str(),
        cfg.artifacts_dir.as_str(),
        cfg.sources_dir.as_str(),
        cfg.synthetic_dir.as_str(),
        cfg.generated_dir.as_str(),
    ];
    for d in &dirs {
        let path = vault_root.join(d);
        if !path.exists() {
            fs::create_dir_all(&path)
                .with_context(|| format!("creating {}", path.display()))?;
            println!("  Created: {}/", d);
        }
    }

    // Instructions directory
    let instructions_dir = vault_root.join(&cfg.csnotes_dir).join("instructions");
    fs::create_dir_all(&instructions_dir)?;

    // ── Write .csnotes ─────────────────────────────────────────────────────────
    let config_path = vault_root.join(".csnotes");
    if config_path.exists() {
        println!("  Skipped: .csnotes (already exists)");
    } else {
        cfg.save(&vault_root)?;
        println!("  Created: .csnotes");
    }

    // ── Write empty manifest ───────────────────────────────────────────────────
    let manifest_path = vault_root.join(crate::manifest::MANIFEST_FILENAME);
    if manifest_path.exists() {
        println!("  Skipped: csnotes.json (already exists)");
    } else {
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
    write_instruction_files(&instructions_dir, &cfg)?;

    // ── Done ──────────────────────────────────────────────────────────────────
    println!("\nVault initialised.");
    println!("  Edit _csnotes/instructions/ before your first session.");
    if let Some(course) = cfg.active_courses.first() {
        println!("\nNext steps:");
        println!("  1. Add raw notes to {}/{}/", course, cfg.raw_dir);
        println!("  2. csnotes reconcile");
        println!("  3. csnotes process");
    } else {
        println!("\nNext steps:");
        println!("  1. Add your course(s) to active_courses in .csnotes");
        println!("  2. csnotes reconcile");
        println!("  3. csnotes process");
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

    write_instruction_files(&instructions_dir, &config)?;
    println!("Instructions written to {}/instructions/", config.csnotes_dir);

    Ok(())
}

// ── Instruction file writing ──────────────────────────────────────────────────

fn write_instruction_files(dir: &Path, cfg: &VaultConfig) -> Result<()> {
    // claude.md / gemini.md — variant-specific entry point
    let (entry_name, entry_content) = match cfg.skill_variant {
        SkillVariant::Claude => ("claude.md", CLAUDE_MD),
        SkillVariant::Gemini => ("gemini.md", GEMINI_MD),
    };
    write_if_absent(dir, entry_name, entry_content)?;

    // Always write the shared files regardless of variant
    if cfg.skill_variant == SkillVariant::Gemini {
        write_if_absent(dir, "claude.md", CLAUDE_MD)?;
    } else {
        write_if_absent(dir, "gemini.md", GEMINI_MD)?;
    }
    write_if_absent(dir, "synthesis.md", SYNTHESIS_MD)?;
    write_if_absent(dir, "report_schema.md", REPORT_SCHEMA_MD)?;

    let _ = entry_name; // suppress unused warning
    Ok(())
}

fn write_if_absent(dir: &Path, filename: &str, content: &str) -> Result<()> {
    let path = dir.join(filename);
    if path.exists() {
        println!("  Skipped: _csnotes/instructions/{} (already exists)", filename);
    } else {
        fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("  Created: _csnotes/instructions/{}", filename);
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
input_plaud_*.md     ← Plaud transcript/summary (XML-wrapped, read-only)
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
- Wikilinks: `[[target-slug]]` or `[[target-slug|display text]]`.  Every
  target must exist in `_synthetic/` already or be created this session.
  Broken wikilinks cause the CLI to discard all your work.

Write notes as the conversation develops — you don't have to finish the
debrief first.

---

## Phase 4 — Write the session report

Read `report_schema.md` before writing `_session_report.json`.

---

## Before you exit

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

The workflow is identical to the Claude backend.  See `claude.md` for the full
phase-by-phase description.  The only difference is the report field:
`"backend": "gemini"`.

Read `_session.md` first, then proceed through the four phases described in
`claude.md`.
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

const REPORT_SCHEMA_MD: &str = r##"# Session report schema

Write `_session_report.json` using the schema below.  This is a metadata
manifest — do NOT include note bodies.  The CLI reads the files you wrote
directly; the report tells it what you did and why.

---

## Top-level structure

```json
{
  "csnotes_report_schema": 1,
  "run_id": "<copy from _session.md exactly>",
  "backend": "claude",
  "started_at": "<ISO 8601 UTC>",
  "completed_at": "<ISO 8601 UTC>",
  "scope": {
    "kind": "session",
    "sessions": ["<session-id>"],
    "sources": []
  },
  "operations": [ ... ],
  "review_flags": [ ... ]
}
```

**`run_id`** must be copied verbatim from `_session.md`.  A mismatch causes
the CLI to discard the workspace.

---

## `create_note` operation

One entry for every note file you created.

```json
{
  "op": "create_note",
  "kind": "atomic",
  "path": "_synthetic/algorithm-analysis/red-black-trees.md",
  "title": "Red-Black Trees",
  "topic": "algorithm-analysis",
  "block_id": "red-black-trees",
  "embed_in": ["_synthetic/algorithm-analysis/algorithm-analysis.md"],
  "provenance": {
    "sessions": [
      {
        "course": "CPSC5001",
        "date": "YYYY-MM-DD",
        "relationship": "introduced"
      }
    ],
    "sources": []
  },
  "change_summary": "One sentence: what this note captures."
}
```

| Field | Notes |
|---|---|
| `kind` | `"atomic"` or `"index"` |
| `path` | workspace-relative; matches the file you wrote |
| `block_id` | required for atomic; omit for index |
| `embed_in` | index notes this atomic should appear in; `[]` if none |
| `relationship` | `"introduced"` / `"expanded"` / `"revised"` / `"applied"` |

---

## `update_note` operation

One entry for every existing note you edited.

```json
{
  "op": "update_note",
  "path": "_synthetic/algorithm-analysis/sorting.md",
  "add_provenance": {
    "sessions": [
      {
        "course": "CPSC5001",
        "date": "YYYY-MM-DD",
        "relationship": "expanded"
      }
    ],
    "sources": []
  },
  "sections": ["Comparison with heapsort"],
  "change_summary": "One sentence: what changed and why."
}
```

`sections` is an informal list of headings or concepts you touched — used by
`csnotes diff`, informational only.

---

## Review flags

```json
{
  "kind": "possible_misread",
  "path": "_synthetic/algorithm-analysis/red-black-trees.md",
  "message": "Raw notes say 'n log n' but rotation analysis is O(log n) — kept O(log n), please confirm."
}
```

| Kind | Behaviour |
|---|---|
| `"possible_misread"` | Persists; shown in `csnotes status` until resolved |
| `"needs_confirmation"` | Persists; shown in `csnotes status` until resolved |
| `"unresolved_question"` | Surfaces in future session briefings; doesn't nag |
| `"ambiguity"` | Logged only; auto-resolved |

Flags do not block the commit.
"##;

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
