# csnotes

AI-assisted synthesis of graduate lecture notes into an Obsidian vault.

`csnotes` manages a structured vault of atomic notes derived from your raw lecture notes, Plaud transcripts, slides, and textbook readings. After each class you run `csnotes process`, which assembles a workspace, launches an AI session (Claude or Gemini via Agy), and then applies the AI's structured report — creating and updating notes, rewriting wikilinks, resolving embeds — with full rollback on failure.

---

## How it works

```
raw notes + Plaud exports + slides
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

```
notes/                ← raw lecture notes (one .md per session)
plaud/                ← Plaud voice recorder exports
artifacts/            ← slides, code handouts, etc.
sources/              ← textbook chapters, papers
_synthetic/           ← AI-maintained atomic notes (do not edit manually)
_generated/           ← manifests, reports, extracts, flag store
_csnotes/
  instructions/
    claude.md         ← workflow instructions for Claude
    gemini.md         ← workflow instructions for Gemini/Agy
    synthesis.md      ← note-writing philosophy
    report_schema.md  ← JSON schema reference
.csnotes              ← TOML config (see below)
csnotes.json          ← manifest (sessions, sources, topics, in-progress state)
```

### 2. Add courses

Either during `init` or afterwards:

```sh
csnotes config --add-course CPSC5001
csnotes config --add-course CPSC5002
```

### 3. Drop your files and reconcile

Put raw notes in `notes/`, Plaud exports in `plaud/`, slides in `artifacts/`, textbook chapters in `sources/`. Then:

```sh
csnotes reconcile
```

`reconcile` scans all directories, registers new sessions and sources in the manifest, and reports what it found. It runs automatically before every `process` invocation.

---

## `.csnotes` reference

`.csnotes` is a TOML file at the vault root. All keys are optional — missing keys use the defaults shown below.

```toml
# Courses currently being taken.  No spaces allowed in names.
# Add with: csnotes config --add-course CPSC5001
active_courses = ["CPSC5001", "CPSC5002"]

# How raw note files are named.
# Tokens: {course} {yyyy} {mm} {dd}
filename_format = "{course}-{mm}-{dd}"

# Subdirectory names (relative to vault root)
raw_dir       = "notes"       # raw lecture notes
plaud_dir     = "plaud"       # Plaud exports
artifacts_dir = "artifacts"   # slides, code handouts
sources_dir   = "sources"     # textbooks, papers
synthetic_dir = "_synthetic"  # AI-maintained notes (managed automatically)
generated_dir = "_generated"  # reports, flags, extracts (managed automatically)

# AI backend: "claude" or "agy"
default_backend = "claude"

# Plaud filename qualifiers (anything containing these strings is a Plaud export)
plaud_qualifiers = ["transcript", "summary", "mindmap"]

# Weeks before a processed course disappears from `status` output
archive_threshold_weeks = 8

# Agy/Gemini model (optional — omit to use agy's default)
# agy_model = "gemini-2.5-flash"
```

### Configuring via CLI

```sh
csnotes config --show                          # print current config
csnotes config --set default_backend=agy       # change a value
csnotes config --set agy_model=gemini-2.5-pro  # set Gemini model
csnotes config --set archive_threshold_weeks=12
csnotes config --add-course CPSC5005           # add a course
csnotes config --archive CPSC5001              # remove from active list
```

Settable keys: `filename_format`, `raw_dir`, `plaud_dir`, `artifacts_dir`, `sources_dir`, `default_backend`, `archive_threshold_weeks`, `agy_model`.

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

# 3. After the session completes:
csnotes diff              # semantic diff of what changed
csnotes flags list        # actionable review flags raised by the AI
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
csnotes extract --stdout                    # print to stdout instead of file
```

### Crash recovery

If the AI session crashes or the process is interrupted:

```sh
csnotes recover           # prompts to resume or discard
csnotes recover --resume  # re-launch AI against the preserved workspace
csnotes recover --discard # throw away the workspace and clear in-progress state
```

### Audit and repair

```sh
csnotes audit             # read-only invariant check
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

## Vault layout inside a course

`csnotes` expects one flat directory per course under your `active_courses` list. Files are matched by the `filename_format` you configured:

```
notes/
  CPSC5001-09-03.md       ← raw lecture note
  CPSC5001-09-10.md

plaud/
  CPSC5001-09-03-transcript.md   ← Plaud export (matched by course+date prefix)
  CPSC5001-09-03-summary.md      ← second export for the same session

artifacts/
  CPSC5001-09-03-slides.pdf      ← matched by session prefix, classified as Slides

sources/
  SICP/
    ch01.md                      ← source ID: SICP/ch01
  dragon-book.md                 ← source ID: dragon-book

_synthetic/
  sorting-algorithms/
    index.md
    quicksort.md
    mergesort.md
  …
```

Plaud exports are matched to sessions by the filename prefix (`{course}-{mm}-{dd}`). Any file in `plaud/` whose name starts with a known session prefix and contains one of the `plaud_qualifiers` strings (`transcript`, `summary`, `mindmap` by default) is attached to that session.

---

## AI backends

### Claude (default)

Requires [Claude Code](https://claude.ai/code) (`claude` CLI on PATH). Launches an interactive Claude Code session with the workspace as the working directory and `_csnotes/instructions/claude.md` as the system prompt.

### Agy / Gemini

Requires [Antigravity](https://antigravity.dev) (`agy` CLI on PATH). Configured with `default_backend = "agy"` in `.csnotes` or `--backend agy` per run.

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
