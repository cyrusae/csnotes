# csnotes

AI-assisted synthesis of graduate lecture notes into an Obsidian vault.

`csnotes` manages a structured vault of atomic notes derived from your raw lecture notes, Plaud transcripts, slides, and textbook readings. After each class you run `csnotes process`, which assembles a workspace, launches an AI session (Claude or Gemini via Agy), and then applies the AI's structured report — creating and updating notes, rewriting wikilinks, resolving embeds — with full rollback on failure.

---

## How it works

```text
raw notes + recording exports + slides
          │
          ▼
  csnotes process
          │
    assembles workspace (_session.md + all inputs)
          │
    launches AI (Claude Code or Agy/Gemini)
    ← AI reads _session.md, synthesises notes, writes session_report.json
          │
    teardown pipeline:
      structural ops  (rename/move/merge/split topics)
      content ops     (create/update atomic notes + index notes)
      invariant check (block IDs, link integrity)
      snapshot        (pre-merge copy)
      merge-back      (workspace → vault)
          │
          ▼
    Obsidian vault updated, manifest saved
```

The vault is plain Markdown — no proprietary format. Every note has YAML frontmatter (`block_id`, `topic`, `provenance`, etc.) and the manifest (`csnotes.json`) tracks sessions, sources, topics, and in-progress state.

---

## Installation

### Runtime dependencies

| Dependency | Required | Purpose |
|---|---|---|
| `claude` CLI | for Claude backend | runs the AI session |
| `agy` CLI | for Agy/Gemini backend | runs the AI session |
| `pdftotext` | optional | extracts text from PDF lecture slides |
| `pandoc` | optional | extracts text from DOCX source files |

`pdftotext` is part of **poppler-utils**. Install it if you have PDF slides in your artifacts directories:

```sh
# macOS
brew install poppler

# Debian/Ubuntu
sudo apt install poppler-utils
```

If `pdftotext` is not installed, PDF artifacts are silently skipped during workspace assembly (no crash — the session just won't have slide content). PPTX files are extracted in pure Rust with no external tools.

Install `pandoc` if you have DOCX files (lecture notes, textbook chapters) in your `sources/` directory:

```sh
# macOS
brew install pandoc

# Debian/Ubuntu
sudo apt install pandoc
```

If `pandoc` is not installed, DOCX sources are silently skipped when assembling a workspace (no crash — the session continues without that content).

### Build

Requires Rust (stable). Install from GitHub with `cargo install`:

```sh
cargo install --git https://github.com/cyrusae/csnotes.git
```

To update to the latest commit:

```sh
cargo install --git https://github.com/cyrusae/csnotes.git --force
```

This puts the `csnotes` binary in `~/.cargo/bin/`. Make sure that directory is on your `PATH` (it usually is after `rustup` setup; if not, add `export PATH="$HOME/.cargo/bin:$PATH"` to your shell profile).

### One-time Rust setup (if needed)

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# follow prompts, then restart shell or: source "$HOME/.cargo/env"
```

---

## Vault setup

### 1. Create the vault

Run `csnotes init` inside an empty directory (or your existing Obsidian vault root):

```sh
mkdir ~/notes && cd ~/notes
csnotes init
```

`init` will prompt for:

- **Filename format** — how your raw note files are named. Default: `{course}-{mm}-{dd}`. Tokens: `{course}`, `{yyyy}`, `{mm}`, `{dd}`.
- **Default course** — e.g. `CPSC5001`. Added to `active_courses`. You can skip this and add courses later.
- **Raw notes directory** — where you drop raw lecture notes. Default: `notes`.
- **Default AI backend** — `claude` (Claude Code) or `agy` (Antigravity/Gemini).

It creates:

```text
notes/                ← flat fallback dirs (only used if active_courses is empty)
recordings/
artifacts/
sources/              ← textbook chapters, papers (always vault-level)
_synthetic/           ← AI-maintained atomic notes (do not edit manually)
_generated/           ← manifests, reports, extracts, flag store
_csnotes/
  instructions/
    claude.md         ← workflow instructions for Claude
    gemini.md         ← workflow instructions for Gemini/Agy
    synthesis.md      ← note-writing philosophy
    report_schema.md  ← JSON schema reference
csnotes.toml          ← TOML config (see below)
csnotes.json          ← manifest (sessions, sources, topics, in-progress state)
```

**Important:** when `active_courses` is set, `reconcile` looks for files under `{course}/notes/`, `{course}/recordings/`, and `{course}/artifacts/` — not the flat `notes/` directory that `init` creates. Create those per-course directories yourself (they just need to exist):

```sh
mkdir -p CPSC5001/notes CPSC5001/recordings CPSC5001/artifacts
mkdir -p CPSC5002/notes CPSC5002/recordings CPSC5002/artifacts
```

The flat `notes/`, `recordings/`, `artifacts/` at the vault root are only scanned when `active_courses = []`.

### 2. Add courses

Either during `init` or afterwards:

```sh
csnotes config --add-course CPSC5001
csnotes config --add-course CPSC5002
```

### 3. Drop your files and reconcile

Put raw notes in `notes/`, recording exports in `recordings/`, slides in `artifacts/`, textbook chapters in `sources/`.

**Slide files (PDF and PPTX)** placed in an `artifacts/` directory are extracted automatically when a workspace is assembled. The pipeline strips deck-wide boilerplate (copyright footers, running headers) and deduplicates near-identical slides so the AI sees concise content, not repeated build-up slides. Text extraction requires `pdftotext` for PDF files; PPTX files are handled in pure Rust. If extraction fails (tool absent, corrupt file), a warning is printed and the session continues without that artifact.

Then:

```sh
csnotes reconcile
```

`reconcile` scans all directories, registers new sessions and sources in the manifest, and reports what it found. It runs automatically before every `process` invocation.

---

## `csnotes.toml` reference

`csnotes.toml` is a TOML file at the vault root. All keys are optional — missing keys use the defaults shown below.

```toml
# Courses currently being taken.  No spaces allowed in names.
# Add with: csnotes config --add-course CPSC5001
active_courses = ["CPSC5001"]

# How raw note files are named.
# Tokens: {course} {yyyy} {mm} {dd}
filename_format = "{course}-{mm}-{dd}"

# Subdirectory names (relative to vault root)
raw_dir          = "notes"        # raw lecture notes
recordings_dir   = "recordings"   # recording exports (transcripts, summaries, mindmaps)
artifacts_dir    = "artifacts"    # slides, code handouts
sources_dir      = "sources"      # textbooks, papers
synthetic_dir    = "_synthetic"   # AI-maintained notes (managed automatically)
generated_dir    = "_generated"   # reports, flags, extracts (managed automatically)

# AI backend: "claude" or "agy"
default_backend = "claude"

# Recording export filename qualifiers (any file whose name contains one of these
# strings is treated as a recording export and attached to the matching session)
recording_qualifiers = ["transcript", "summary", "mindmap"]

# Set to false to never require or prompt for recording exports
require_recordings = true

# Courses that never have recording exports (even when require_recordings = true)
# Edit csnotes.toml directly to manage this list
courses_without_recordings = []

# Weeks before a processed course disappears from `status` output
archive_threshold_weeks = 8

# Agy/Gemini model (optional — omit to use agy's default)
# agy_model = "gemini-2.5-flash"

# Include AI conversation files in vault audit (default: true)
scan_ai_conversations = true

# Subdirectories of sources_dir to skip during source scanning.
# Useful for tool/template folders that live inside sources/ but shouldn't
# be treated as source material.  Edit csnotes.toml directly to manage this list.
sources_ignore_dirs = []
```

### Configuring via CLI

```sh
csnotes config --show                            # print current config
csnotes config --set default_backend=agy         # change a value
csnotes config --set agy_model=gemini-2.5-pro    # set Gemini model
csnotes config --set archive_threshold_weeks=12
csnotes config --set require_recordings=false    # disable recording prompts globally
csnotes config --add-course CPSC5005             # add a course
csnotes config --archive CPSC5001                # remove from active list
```

Settable keys: `filename_format`, `raw_dir`, `recordings_dir`, `artifacts_dir`, `sources_dir`, `default_backend`, `require_recordings`, `archive_threshold_weeks`, `agy_model`, `scan_ai_conversations`. Run `csnotes config --help` for descriptions of each key.

`sources_ignore_dirs` is not settable via `--set` — edit `csnotes.toml` directly.

---

## Usage guide

### Normal session workflow

```sh
# 1. Drop raw note + Plaud export(s) in their directories, then:
csnotes status            # see what's pending

# 2. Run a session (auto-reconciles first)
csnotes process           # auto-picks the one unprocessed session
csnotes process --next               # oldest pending session (backlog catch-up)
csnotes process --session 09-03              # by date (unique across courses)
csnotes process --session 09-03 --for-course CPSC5001  # disambiguate when same date in multiple courses
csnotes process --course CPSC5002            # course-wide review workspace (all sessions + sources)
csnotes process --backend agy               # override backend for this run
csnotes process --agy-model gemini-2.5-pro  # override Gemini model
csnotes process --dry-run                   # show scope and inputs, don't launch
csnotes process --resume                    # re-enter an interrupted session (same as recover --resume)

# 3. After the session completes:
csnotes diff                    # semantic diff of what changed in the last session
csnotes diff --session 09-03    # diff for a specific session
csnotes flags list              # actionable review flags raised by the AI
```

### Processing sources (textbook chapters, papers)

```sh
# Drop file in sources/ — e.g. sources/SICP/ch01.md
csnotes reconcile                        # registers it
csnotes process --source SICP/ch01       # synthesise from this source
```

### Topic review sessions

```sh
# Focused review of an existing topic — no new raw notes required
csnotes process --topic "sorting-algorithms"
```

### Course review sessions

```sh
# Assemble all processed sessions + course-tagged sources for a course
csnotes process --course CPSC5002
```

Launches a `course-review.md`-driven session. The AI writes a journal entry to `_journal/<course>/review-<date>.md` (study narrative: what you discussed, lingering confusion, instructor framing) and optionally emits atomic ops to `_synthetic/` when something reference-worthy surfaces. The journal entry is always the primary deliverable.

### Flags

```sh
csnotes flags list                          # open actionable flags
csnotes flags list --all                    # include threads and changelog
csnotes flags show <id>                     # full flag detail
csnotes flags resolve <id>                  # mark resolved
csnotes flags resolve <id> --follow-up "addressed in CPSC5001-10-04"
```

### Extract action items

```sh
csnotes extract                             # actions + deadlines + questions → _generated/extracts/
csnotes extract --type actions
csnotes extract --type deadlines
csnotes extract --session 09-03             # extract from a specific session
csnotes extract --stdout                    # print to stdout instead of file
```

Questions are any line containing `?` — including mid-line marks like `"arrays(? or linked lists?) are the default"` — or lines starting with `Q:` / `QUESTION:` even without a trailing `?`.

### Progressive commit (mid-session checkpointing)

The AI can call `csnotes commit` from inside the workspace to execute the current batch of ops and merge them to the vault without ending the session. The workspace stays alive and work continues.

```sh
# From inside an active AI workspace:
csnotes commit           # execute current ops, merge to vault, keep session open
csnotes commit --dry-run # preview what would run without executing anything
```

This is useful for structural reorganisation: the AI declares rename/move ops, commits them so the CLI executes the file moves, then continues writing notes that reference the new paths. Between commits the AI can freely rewrite `_session_report.json` — the CLI tracks committed ops independently in `_workspace_meta.json`.

### Crash recovery

If the AI session crashes or the process is interrupted:

```sh
csnotes recover           # prompts to resume or discard
csnotes recover --resume  # re-launch AI against the preserved workspace
csnotes recover --discard # throw away the workspace and clear in-progress state
csnotes recover --reset   # rebuild _synthetic/ from vault state; clear report + committed_ops
                          # use this when the AI has made a mess of _synthetic/ and you want
                          # a clean slate without tearing down the whole workspace
```

### In-workspace validation

The AI is instructed to run this before exiting a session:

```sh
csnotes check             # validate wikilinks, block anchors, and block-ID uniqueness
                          # from inside the workspace (no vault access needed)
```

Prints violations and exits with code 1 if any hard violations are found. Soft warnings (orphan atomics, missing sidecars) are printed but don't fail.

### Audit and repair

```sh
csnotes audit                      # read-only invariant check across the full vault
csnotes audit --reindex            # rebuild csnotes.json from frontmatter + filesystem
csnotes audit --fix                # show mechanical repairs (dry-run)
csnotes audit --fix --apply        # execute repairs
csnotes audit --show-discrepancies # diff manifest topics vs actual filesystem
```

### Renaming the filename format after the fact

```sh
csnotes config --set filename_format="{course}-{yyyy}-{mm}-{dd}"
csnotes config --migrate          # show rename plan (dry-run)
csnotes config --migrate --apply  # execute renames + update manifest
```

---

## Vault layout

When `active_courses` is set, each course gets its own subdirectory and the per-session directories nest inside it:

```text
CPSC5001/
  notes/
    CPSC5001-09-03.md              ← raw lecture note
    CPSC5001-09-10.md
  recordings/
    CPSC5001-09-03-transcript.md   ← recording export (matched by course+date prefix)
    CPSC5001-09-03-summary.md      ← second export for the same session
  artifacts/
    CPSC5001-09-03-slides.pdf      ← flat pattern: session prefix → Slides
    CPSC5001-09-03/                ← folder pattern: directory name = session ID
      slides-lecture.pdf           ←   Slides (pdf → always Slides)
      slides-reading.pdf           ←   Slides
      day1/                        ←   professor's upload subfolder
        Thing.java                 ←     Code (qualifier: "day1-Thing")
        Node.java                  ←     Code (qualifier: "day1-Node")

CPSC5002/
  notes/
  recordings/
  artifacts/

sources/                        ← vault-level, not per-course
  SICP/
    ch01.md                     ← source ID: SICP/ch01
  dragon-book.md                ← source ID: dragon-book

_synthetic/                     ← vault-level, AI-managed
  sorting-algorithms/
    index.md
    quicksort.md
    mergesort.md
  …
```

The subdirectory names (`notes`, `recordings`, `artifacts`) are the values of `raw_dir`, `recordings_dir`, and `artifacts_dir` in `csnotes.toml`. `sources/` and `_synthetic/` are always at the vault root.

If `active_courses` is empty, the fallback is a flat layout with `raw_dir`, `recordings_dir`, and `artifacts_dir` directly at the vault root — useful for single-course or test setups.

Recording exports are matched to sessions by filename prefix (`{course}-{mm}-{dd}`). Any file whose name starts with a known session prefix and contains one of the `recording_qualifiers` strings (`transcript`, `summary`, `mindmap` by default) is attached to that session.

### Artifact naming patterns

Artifacts in the `artifacts/` directory can follow two naming patterns — both are scanned by `csnotes reconcile`:

**Pattern A — session-prefixed filename:**
```
CPSC5001-09-03-slides.pdf        → session CPSC5001-09-03, qualifier "slides"
CPSC5001-09-03.md                → session CPSC5001-09-03, no qualifier
```

**Pattern B — session-named directory:**
```
CPSC5001-09-03/slides.pdf        → session CPSC5001-09-03, qualifier "slides"
CPSC5001-09-03/day1/Thing.java   → session CPSC5001-09-03, qualifier "day1-Thing"
```

The directory pattern is useful when an instructor uploads a folder per class (often named `day1`, `week3`, etc.) that you want to attach to a session. Create a directory named after the session ID, drop the instructor's folder inside, and reconcile.

Kind classification follows this priority:
1. **`pdf`, `pptx`, `ppt`** → Slides (content extracted at workspace assembly time)
2. Qualifier contains a slide keyword (`slides`, `deck`, `handout`, …) → Slides
3. Text format (`md`, `html`, `tex`, `txt`) with no qualifier → Slides
4. Code extension (`java`, `py`, `rs`, `js`, …) → Code
5. Anything else → Other

---

## AI backends

### Claude (default)

Requires [Claude Code](https://claude.ai/code) (`claude` CLI on PATH). Launches an interactive Claude Code session with the workspace as the working directory and `_csnotes/instructions/claude.md` as the system prompt.

### Agy / Gemini

Requires [Antigravity](https://antigravity.dev) (`agy` CLI on PATH). Configured with `default_backend = "agy"` in `csnotes.toml` or `--backend agy` per run.

```sh
csnotes config --set default_backend=agy
csnotes config --set agy_model=gemini-2.5-flash  # optional model pin
```

### Recovering after a failed session

Both backends preserve the workspace on failure so you can re-enter without losing the AI's partial work:

```sh
csnotes recover --resume
```

---

## Command reference

| Command | What it does |
|---|---|
| `csnotes init` | Scaffold a new vault (directories, instruction files, `csnotes.toml`). Use `--instructions-only` to refresh just the instruction files. |
| `csnotes process` | Run an AI synthesis session against pending notes. Auto-reconciles first. |
| `csnotes reconcile` | Scan all directories and register new sessions, sources, and artifacts in the manifest. |
| `csnotes status` | Show unprocessed sessions, unprocessed sources, topic health, open flags, and any in-progress warning. `--json` emits compact JSON. `--topic <name>` shows detailed view for one topic. |
| `csnotes diff` | Semantic diff of what the last session created, updated, or restructured. |
| `csnotes extract` | Pull action items, deadlines, and questions out of raw notes into `_generated/extracts/`. |
| `csnotes flags list` | List open review flags. `--all` includes threads and changelog flags. |
| `csnotes flags show <id>` | Full detail for one flag. |
| `csnotes flags resolve <id>` | Mark a flag resolved. `--follow-up "..."` records a note at resolution. |
| `csnotes commit` | Progressive mid-session commit: execute the current batch of ops and merge to vault without ending the session. `--dry-run` previews without executing. |
| `csnotes recover` | Resume or discard an in-progress session after a crash. `--reset` rebuilds `_synthetic/` from vault state without tearing down the workspace. |
| `csnotes audit` | Read-only invariant check across the full vault. `--reindex` rebuilds `csnotes.json`. `--fix --apply` runs mechanical repairs. `--show-discrepancies` diffs manifest topics against the filesystem. |
| `csnotes check` | Validate wikilinks, block anchors, block-ID uniqueness, and structural op preconditions from inside an active workspace. Run by the AI before exiting. |
| `csnotes config` | Read or update vault configuration (see above for options). |

### `csnotes process` flags

```sh
csnotes process                            # auto-picks the one unprocessed session
csnotes process --next                     # oldest pending session (backlog catch-up)
csnotes process --session 09-03           # specific session by date
csnotes process --session 09-03 --course CPSC5001
csnotes process --source SICP/ch01        # process a source file instead of a session
csnotes process --source Textbooks/SICP  # expand prefix → all sources under that path
csnotes process --topic "sorting-algorithms"  # review session on an existing topic
csnotes process --backend agy             # override AI backend for this run
csnotes process --agy-model gemini-2.5-pro
csnotes process --dry-run                 # show scope without launching
csnotes process --resume                  # re-enter an interrupted session
```

### `csnotes status` flags

```sh
csnotes status                 # human-readable dashboard
csnotes status --json          # compact JSON for scripting or AI agent context
csnotes status --topic <name>  # detailed view of one topic: atomics, sessions, sources, flags
```

### `csnotes audit` flags

```sh
csnotes audit                      # read-only invariant check
csnotes audit --reindex            # rebuild csnotes.json from frontmatter + filesystem
csnotes audit --fix                # show mechanical repairs (dry-run)
csnotes audit --fix --apply        # execute repairs
csnotes audit --show-discrepancies # diff manifest topics vs actual filesystem
```

### `csnotes commit` flags

```sh
csnotes commit           # execute current batch of ops and merge to vault (from inside workspace)
csnotes commit --dry-run # preview ops and precondition status without executing
```

### `csnotes reconcile` flags

```sh
csnotes reconcile                         # scan and register new files
csnotes reconcile --notify                # desktop notification if anything new is found
csnotes reconcile --rename-spaces hyphens # rename filenames that contain spaces
csnotes reconcile --rename-spaces underscores
csnotes reconcile --reset                 # wipe all sessions+sources and re-scan from scratch
```

### `csnotes config` flags

```sh
csnotes config --show                     # print current config
csnotes config --set key=value            # set a config key (see config --help for keys)
csnotes config --add-course CPSC5001      # add a course
csnotes config --archive CPSC5001         # remove from active courses
csnotes config --migrate                  # show rename plan for current filename_format
csnotes config --migrate --apply          # execute the rename plan
```

---

## Tips

- **`csnotes status`** is your daily dashboard: unprocessed sessions, unprocessed sources, topic health, open flags, and any in-progress warning.
- **`csnotes diff`** after every session shows exactly what was created, updated, and restructured — useful for reviewing what the AI did before opening Obsidian.
- **Instruction files** live in `_csnotes/instructions/` and are read by the AI during the session. You can edit them to tune the AI's behaviour. Run `csnotes init --instructions-only` to restore the defaults without touching anything else.
- **`csnotes commit`** inside a workspace runs a mid-session checkpoint: structural ops execute (rename/move/merge topics), the result merges to vault, and the session continues. Use it after declaring structural ops so the AI can then write notes that reference the new paths. The CLI tracks committed ops independently of the report, so the AI can rewrite `_session_report.json` freely between commits.
- **`csnotes recover --reset`** rebuilds `_synthetic/` from the vault's current state and clears the session report, giving the AI a clean slate without tearing down the workspace. Use it when the AI has made a structural mess mid-session.
- **Never manually edit `_synthetic/`** while a session is in progress — the manifest won't reflect your changes until the next `audit --reindex`.
- **`csnotes.json`** is the source of truth for session/source/topic state. If it ever gets out of sync, `csnotes audit --reindex` rebuilds it from frontmatter.
- **Backlog catch-up**: use `csnotes process --next` to automatically pick up the oldest unprocessed session when you have multiple pending. Re-run it after each session completes to work through the queue.
