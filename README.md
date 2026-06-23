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

`pdftotext` is part of **poppler-utils**. Install it if you have PDF slides in your artifacts directories:

```sh
# macOS
brew install poppler

# Debian/Ubuntu
sudo apt install poppler-utils
```

If `pdftotext` is not installed, PDF artifacts are silently skipped during workspace assembly (no crash — the session just won't have slide content). PPTX files are extracted in pure Rust with no external tools.

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

---

## Usage guide

### Normal session workflow

```sh
# 1. Drop raw note + Plaud export(s) in their directories, then:
csnotes status            # see what's pending

# 2. Run a session (auto-reconciles first)
csnotes process           # auto-picks the one unprocessed session
csnotes process --session 09-03              # by date (unique across courses)
csnotes process --session 09-03 --course CPSC5001  # explicit course+date
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

### Crash recovery

If the AI session crashes or the process is interrupted:

```sh
csnotes recover           # prompts to resume or discard
csnotes recover --resume  # re-launch AI against the preserved workspace
csnotes recover --discard # throw away the workspace and clear in-progress state
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
csnotes audit             # read-only invariant check across the full vault
csnotes audit --reindex   # rebuild csnotes.json from frontmatter + filesystem
csnotes audit --fix       # show mechanical repairs (dry-run)
csnotes audit --fix --apply  # execute repairs
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
    CPSC5001-09-03-slides.pdf      ← matched by session prefix, classified as Slides

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

## Tips

- **`csnotes status`** is your daily dashboard: unprocessed sessions, unprocessed sources, topic health, open flags, and any in-progress warning.
- **`csnotes diff`** after every session shows exactly what was created, updated, and restructured — useful for reviewing what the AI did before opening Obsidian.
- **Instruction files** live in `_csnotes/instructions/` and are read by the AI during the session. You can edit them to tune the AI's behaviour. Run `csnotes init --instructions-only` to restore the defaults without touching anything else.
- **Never manually edit `_synthetic/`** while a session is in progress — the manifest won't reflect your changes until the next `audit --reindex`.
- **`csnotes.json`** is the source of truth for session/source/topic state. If it ever gets out of sync, `csnotes audit --reindex` rebuilds it from frontmatter.
