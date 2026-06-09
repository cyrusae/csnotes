# csnotes — Implementation Plan

> Generated from DESIGN.md v2. Issues and open questions are flagged inline as **⚠ Issue N** and collected in §7.

---

## 1. Technology Stack

| Need | Crate | Notes |
|---|---|---|
| CLI parsing | `clap` (derive) | Subcommands + typed flags |
| Serialization | `serde` + `serde_json` + `serde_yaml` | Manifest, report, frontmatter |
| Config file | `toml` | `.csnotes` |
| Markdown parsing | `comrak` (GFM extensions) | Phase 1+; Phase 0 uses raw line passes only |
| Dates/times | `chrono` | ISO 8601 everywhere |
| Filesystem walk | `walkdir` | `reconcile`, link rewriting, reindex |
| Unique run IDs | `uuid` (v4) | `run_id` in manifest + report |
| Error handling | `anyhow` (app) + `thiserror` (library errors) | |
| XDG paths | `dirs` or `xdg` | Workspace location; macOS `$TMPDIR` fallback |
| File copy | `fs_extra` | Workspace assembly |
| Temp dirs | `tempfile` | Workspace root under XDG_RUNTIME_DIR |
| Regex | `regex` | Block-anchor extraction (`^block-id`), wiki-link extraction |
| Property tests | `proptest` | Phase 3 idempotency/transactionality |
| Unit tests | std | §11 CI harness; mock backend |

**No async.** Every operation is synchronous filesystem + process I/O; async adds complexity for no throughput gain here.

---

## 2. Module Architecture

```
csnotes/
  Cargo.toml
  src/
    main.rs              ← clap dispatch only; no logic
    error.rs             ← CsnotesError enum (thiserror)
    config.rs            ← .csnotes (VaultConfig); init validation
    manifest.rs          ← csnotes.json types + load/save/update
    frontmatter.rs       ← YAML fence read/write; provenance delta merge
    report.rs            ← SessionReport types + parse/schema-validate
    workspace.rs         ← assemble, xml_wrap, teardown, snapshot
    audit.rs             ← invariant suite (hard + soft); reindex engine
    backend.rs           ← BackendLauncher trait; ClaudeBackend, MockBackend
    flags.rs             ← FlagStore load/append/resolve
    obsidian.rs          ← wiki-link + block-anchor extraction (regex, Phase 0+)
    markdown.rs          ← comrak wrapper + heading-scheme derivation (Phase 1+)
    ops/
      mod.rs             ← Op enum dispatch
      content.rs         ← create_note, update_note execution
      structural.rs      ← rename_topic (Phase 1), move_atomic etc. (Phase 4)
    commands/
      init.rs
      process.rs         ← the main lifecycle (§7)
      status.rs
      diff.rs
      flags_cmd.rs
      extract.rs
      reconcile.rs
      recover.rs
      audit_cmd.rs
      config_cmd.rs
  tests/
    fixtures/            ← fixture vaults, fixture reports, fixture body edits
    mock_backend.rs      ← fixture copier invoked by --backend mock
    lifecycle_tests.rs   ← end-to-end §7 lifecycle
    property_tests.rs    ← Phase 3 proptest suites
```

The `ops/` split is deliberate: content ops (indexing-only, Phase 0) and structural ops (link-rewriting, Phase 1+) have different execution models and test surfaces.

---

## 3. Key Rust Types

This section gives the canonical types that the rest of the code is built around. Getting these right before writing any command logic is the single most leveraged design decision.

### 3.1 Config (`.csnotes`)

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct VaultConfig {
    pub vault_root:       PathBuf,
    pub raw_dir:          String,          // default "notes"
    pub plaud_dir:        String,          // default "plaud"
    pub artifacts_dir:    String,          // default "artifacts"
    pub sources_dir:      String,          // default "sources"
    pub synthetic_dir:    String,          // default "_synthetic"
    pub generated_dir:    String,          // default "_generated"
    pub csnotes_dir:      String,          // default "_csnotes"
    pub filename_format:  String,          // e.g. "{course}-{mm}-{dd}"
    pub active_courses:   Vec<String>,
    pub default_backend:  AiBackend,       // claude | agy | mock; overridden per-run by --backend flag
    pub skill_variant:    SkillVariant,    // claude | gemini
    pub snapshot_mode:    SnapshotMode,    // pre_merge | shadow_git
    pub archive_threshold_weeks: u32,      // default 8; see §7 issue note
    pub plaud_qualifiers: Vec<String>,     // recognized Plaud export suffixes; see §2.1
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AiBackend { Claude, Agy, Mock }

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum SkillVariant { Claude, Gemini }

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode { PreMerge, ShadowGit }
```

### 3.2 Manifest (`csnotes.json`)

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct Manifest {
    pub version:             String,               // "2"
    pub vault_root:          PathBuf,
    pub config:              ManifestConfig,       // mirror of relevant VaultConfig fields
    pub sessions:            IndexMap<String, SessionEntry>,
    pub sources:             IndexMap<String, SourceEntry>,
    pub topics:              IndexMap<String, TopicEntry>,
    pub session_in_progress: Option<InProgressRecord>,
    pub flags_path:          String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SessionEntry {
    pub date:            NaiveDate,
    pub course:          String,
    pub filename_format: String,
    pub raw_note:        String,
    pub plaud_exports:   Vec<PlaudExport>,
    pub artifacts:       Vec<ArtifactEntry>,
    pub plaud_missing:   bool,
    pub status:          SessionStatus,
    pub processed_at:    Option<DateTime<Utc>>,
    pub topics_updated:  Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus { Unprocessed, InProgress, Processed }

#[derive(Serialize, Deserialize, Debug)]
pub struct InProgressRecord {
    pub run_id:         String,
    pub started_at:     DateTime<Utc>,
    pub workspace_path: PathBuf,
    pub phase:          String,            // "synthesizing" | "merging"
    pub error:          Option<String>,
}

// SourceEntry, TopicEntry follow §6.3 / §6.4 directly.
```

### 3.3 Frontmatter

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct NoteFrontmatter {
    pub csnotes_schema:          u32,          // 1
    pub kind:                    NoteKind,
    pub topic:                   String,
    pub title:                   String,
    pub block_id:                Option<String>,    // atomic only
    pub embeds:                  Option<Vec<String>>, // index only
    pub contributing_sessions:   Vec<SessionContrib>,
    pub contributing_sources:    Vec<SourceContrib>,
    pub cross_embedded_in:       Option<Vec<String>>, // atomic only
    pub created:                 DateTime<Utc>,
    pub last_updated:            DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SessionContrib {
    pub course:       String,
    pub date:         NaiveDate,
    pub relationship: Relationship,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Relationship { Introduced, Extended, Reframed, Contradicted, Nuanced }
```

### 3.4 Session Report

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct SessionReport {
    pub csnotes_report_schema: u32,
    pub run_id:                String,
    pub backend:               String,
    pub started_at:            DateTime<Utc>,
    pub completed_at:          DateTime<Utc>,
    pub scope:                 ReportScope,
    pub operations:            Vec<Op>,
    pub review_flags:          Vec<ReviewFlag>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    CreateNote(CreateNoteOp),
    UpdateNote(UpdateNoteOp),
    RenameTopic(RenameTopicOp),
    MoveAtomic(MoveAtomicOp),
    PromoteAtomic(PromoteAtomicOp),
    DemoteTopic(DemoteTopicOp),
    MergeTopics(MergeTopicsOp),
    SplitTopic(SplitTopicOp),
    SetEmbed(SetEmbedOp),
}
```

Using `#[serde(tag = "op")]` gives you the JSON `"op": "create_note"` discriminant cleanly and makes exhaustive matching free.

### 3.5 Audit Result

```rust
pub struct AuditResult {
    pub hard_violations: Vec<AuditViolation>,  // → discard
    pub soft_warnings:   Vec<AuditWarning>,    // → log, don't block
}
// Distinct types so call sites cannot accidentally treat a warning as a violation.
```

---

## 4. Phase-by-Phase Implementation Plan

### Phase 0 — Minimum Viable Core + Safety

**Goal:** A real interactive synthesis session commits via a valid report; a malformed report blocks teardown without losing state; a simulated crash recovers to a clean vault; `audit --reindex` reproduces the manifest.

#### 0.1 `csnotes init`

1. Prompt for vault root (default: cwd), subdir names (offer defaults), default course, `filename_format`.
2. Validate `filename_format`: must contain `{course}`, must contain ≥1 date token, must use `-` delimiters, must reject spaces.
3. Create the directory tree: `raw/`, `plaud/`, `artifacts/`, `sources/`, `_synthetic/`, `_generated/`, `_csnotes/instructions/`.
4. Write `.csnotes` (TOML).
5. Write empty `csnotes.json` and `_generated/flags.json`.
6. Write real `_csnotes/instructions/claude.md`, `gemini.md`, and `synthesis.md`.

**⚠ Issue 2 — Instruction file contents.** The design says `init` writes "real (not stub) instruction sources," and §9 gives the directive list, but the actual prose of `claude.md`, `gemini.md`, and `synthesis.md` is not in DESIGN.md. These files are load-bearing for Phase 0 to work at all (the AI needs them to know about XML tags, the report schema, atomization rules, etc.). **Writing these is a design task, not just an implementation task.** Approach: annotate what each file needs to contain while building the CLI, then draft them with LLM assistance before Phase 0 is declared functional. Treat the instruction files as a co-equal Phase 0 deliverable.

**`synthesis.skill` → `synthesis.md`.** This is a plain markdown instructions file that gets copied into the workspace root and is referenced by `CLAUDE.md`/`GEMINI.md` (e.g., *"When wrapping up, read `synthesis.md` for the full procedure."*). It is not a slash-command skill — just a file the AI reads on demand via its native file-read tools. The CLI copies it to the workspace root alongside the main instruction file; no special discovery mechanism is needed.

#### 0.2 Workspace Assembly

`workspace.rs` is the heart of Phase 0. Sequence:

1. Resolve scope (session/source) from manifest.
2. Create `$XDG_RUNTIME_DIR/csnotes/<run_id>/` (fallback: `$TMPDIR/csnotes/<run_id>/` on macOS, `$XDG_CACHE_HOME/csnotes/<run_id>/` on Linux if `XDG_RUNTIME_DIR` unset).
3. XML-wrap each input and write to workspace (read-only via `chmod a-w` on the copies after writing).
4. Copy the full `_synthetic/` tree into workspace as a writable working copy.
5. Render `_session.md` (see §0.5 below).
6. Copy the instruction file as `CLAUDE.md` or `GEMINI.md` at the workspace root. Copy `synthesis.md` alongside it at the workspace root.
7. Record `session_in_progress` in the manifest.

**XML wrapping** is a pure string transformation. Write a function `xml_wrap(content: &str, tag: &str, attrs: &[(&str, &str)]) -> String` that produces `<tag attr="val">content</tag>`. Use it for each input type with the tag names from §8.

**Permissions:** After copying raw inputs, set them read-only (`std::fs::set_permissions`). On the `_synthetic/` working copy, leave write permissions. This is the physical enforcement of the sandbox.

**⚠ Issue 4 — macOS XDG fallback.** `$XDG_RUNTIME_DIR` is not set by default on macOS; `$XDG_CACHE_HOME` also often is not. The fallback chain should be: `$XDG_RUNTIME_DIR` → `$TMPDIR` (macOS/Linux standard) → `/tmp`. The `dirs` crate's `cache_dir()` gives the OS-appropriate cache path as a further fallback. Codify this hierarchy explicitly; a silent fallback to a wrong location is hard to debug.

#### 0.3 AI Launch

```rust
pub trait BackendLauncher {
    fn launch(&self, workspace: &Path, skill_variant: SkillVariant) -> Result<()>;
}
```

`ClaudeBackend` executes `std::process::Command::new("claude").args(&["--system-prompt", "."]).current_dir(workspace).status()` (blocking). The `--system-prompt "."` flag scopes the system prompt to the workspace's own `CLAUDE.md` only, preventing any global `~/.claude/CLAUDE.md` the user has from leaking into the session. `MockBackend` copies fixture files from `tests/fixtures/` into the workspace instead of launching a process.

**`AgyBackend`** (Antigravity): `agy` does not auto-load a `GEMINI.md` from cwd the way `claude` does. Its project discovery (`.antigravitycli/`) is a registry pointer only. Context is provided via `--add-dir` (exposes a directory's files to the AI's read tools) and bootstrapped with `-i` (initial interactive prompt). Launch:

```
agy -i "Read GEMINI.md in this workspace for your instructions." --add-dir <workspace>
```

This explicitly bootstraps instruction loading regardless of what `agy` auto-loads, which is more reliable than implicit discovery. `AgyBackend` is implemented after `ClaudeBackend` is working; stub it with `todo!()` for Phase 0.

#### 0.4 Teardown (§7 pipeline)

`commands/process.rs` runs this after the AI exits:

1. **Locate report.** `workspace/_session_report.json`. If absent → preserve workspace, print "no report found; re-enter the session and have the AI write the report." Exit without touching the vault.
2. **Parse + schema-validate.** `report::parse(path)`. On failure → preserve workspace, print parse error. Exit.
3. **`run_id` match.** Report's `run_id` must equal `manifest.session_in_progress.run_id`. On mismatch → preserve workspace, report. Exit.
4. **Precondition pass** (pure read, no mutation, against the workspace):
   - `create_note`: workspace path must not exist.
   - `update_note`: workspace path must exist with valid frontmatter.
   - Declared `block_id`s must appear as `^block-id` anchors in the workspace note body (regex pass — see §0.6).
   - `embed_in` targets must exist in the workspace.
   On any failure → discard workspace, report which op failed. The vault was never touched.
5. **Execute structural ops** (Phase 1+; Phase 0 has no structural ops other than no-ops — `create_note`/`update_note` are indexing ops, not structural). In Phase 0, skip this step.
6. **Execute content ops** (create_note, update_note):
   - For `create_note`: the file must already exist in the workspace (the AI wrote it). Merge declared provenance into frontmatter, set `created` and `last_updated`, write back to workspace.
   - For `update_note`: open workspace file, merge `add_provenance` into frontmatter (dedupe on `(course,date)` / `(source_id, location.path)`), bump `last_updated`, write back.
7. **Build updated manifest** from workspace frontmatter + declared `topics_updated`.
8. **Invariant suite** (§0.7) against the workspace. Hard violations → discard workspace. Soft warnings → log.
9. **Pre-merge snapshot**: `cp -r vault/_synthetic/ vault/_synthetic_snapshot_<run_id>/`.
10. **Merge-back**: copy modified workspace `_synthetic/` into vault `_synthetic/`. Write manifest. Append flags to flag store. Clear `session_in_progress`. Delete snapshot and workspace.
11. On any crash during step 10 → `recover` will find the snapshot and restart from it.

#### 0.5 `_session.md` Rendering

**⚠ Issue 6 — `_session.md` format is unspecified.** The design describes the briefing's content (existing block IDs, relevant open flags, manifest state) but gives no template. The AI will use this every session. Before implementing workspace assembly, write the template. Proposed structure:

```markdown
# Session Briefing — <course> <date>

## Scope
- Course: <COURSE>
- Date: <DATE>
- Sessions being processed: <list>

## Inputs in This Workspace
- Raw notes: `<raw_student_notes>` tag
- Plaud: <transcript/summary/mindmap or "not available">
- Artifacts: <list or "none">

## Existing Synthetic Notes (topics relevant to this session)
### <topic>
- Index: `_synthetic/<topic>/<topic>.md`
- Atomics: `_synthetic/<topic>/<atomic>.md`
- Block IDs in use: `^<id>`, `^<id>`, ...
- Last updated: <date>

## Open Flags (relevant to this session)
- [<id>] <kind>: <message>

## All Known Block IDs (vault-wide)
<flat list of ^block-id → owning file, for collision avoidance>
```

The "all known block IDs" section is critical for Phase 0 correctness — the AI needs it to avoid collisions. Generating it requires a regex scan of all `_synthetic/` notes before workspace launch.

#### 0.6 Block-Anchor and Wiki-Link Extraction (`obsidian.rs`)

These are needed in Phase 0 for the precondition pass and invariant suite. `comrak` is not used here — these are raw regex passes.

```rust
// Extract all ^block-id anchors from a markdown string.
// Obsidian syntax: a line ending with " ^some-id" or "^some-id" alone.
pub fn extract_block_ids(content: &str) -> Vec<String>

// Extract all [[wikilink]] and [[wikilink#section]] targets.
pub fn extract_wikilinks(content: &str) -> Vec<WikiLink>

// Extract all ![[embed]] and ![[embed#^block-id]] targets.
pub fn extract_embeds(content: &str) -> Vec<EmbedTarget>
```

Test these exhaustively — they are the foundation of link-resolution checking.

**Block-anchor regex:** `r"(?m)\^([a-z0-9][a-z0-9-]*)$"` (end of line, lowercase, hyphen-separated). Validate this against the Obsidian spec.

#### 0.7 Invariant Suite (`audit.rs`)

Phase 0 hard invariants (run against the workspace, before merge-back):

1. Report parsed and `run_id` matches. *(Already checked in teardown step 3 — re-check here as a belt-and-suspenders assertion.)*
2. Every note declared in `create_note`/`update_note` exists in the workspace with parseable frontmatter.
3. Every note with `kind: atomic` has a `block_id` in frontmatter AND a matching `^<block-id>` anchor in the body.
4. Block IDs are unique across the entire working `_synthetic/` tree (not just the session's notes).
5. Every `![[embed#^block-id]]` target resolves: the target file exists and the `^block-id` exists in it.
6. Every `[[wikilink]]` target resolves to an existing file in the workspace.
7. Manifest referential integrity (§6.5): topics referenced in `topics_updated` exist in topics map; session IDs referenced in topic `contributing_sessions` exist in sessions map.

Phase 0 soft warnings:

- Orphan atomic (has no `![[embed]]` pointing to it from any index in its topic folder).
- `last_updated` predates its most recent `contributing_sessions` date.
- Duplicate provenance entries (same `(course,date)`) after merge — indicates a re-run.
- Open flag count.

#### 0.8 `csnotes recover`

Read `manifest.session_in_progress`:

- If null → "nothing to recover."
- If `workspace_path` exists and `phase == "synthesizing"` → offer **[r]esume** (relaunch AI against same workspace) or **[d]iscard** (delete workspace, clear `session_in_progress`).
- If a `_synthetic_snapshot_<run_id>/` exists in the vault → crash during merge. Restore snapshot (`mv vault/_synthetic/ vault/_synthetic_broken/`, `mv vault/_synthetic_snapshot_<run_id>/ vault/_synthetic/`), then offer Resume or Discard. Delete `_synthetic_broken/` on confirm.

#### 0.9 `csnotes status` (sessions only, Phase 0)

Read manifest, print:

- Per-session: status (`unprocessed` / `in_progress` / `processed`), date, course, whether Plaud exports are present.
- `session_in_progress` warning if set.
- Count of open actionable flags.

#### 0.10 `csnotes audit [--reindex]`

Without `--reindex`: run the invariant suite read-only against the vault (not a workspace). Print violations and warnings.

`--reindex`: Walk `_synthetic/`, parse each note's frontmatter, rebuild `topics` and the session/source provenance cross-references. Write new `csnotes.json`. This must produce an identical manifest to the committed one (property test for Phase 3).

#### 0.11 Mock Backend + CI

`--backend mock` in `process` swaps `ClaudeBackend` for `MockBackend`. `MockBackend::launch` copies files from a named fixture set into the workspace (fixture reports + fixture body edits). The fixture set is identified by a `--fixture NAME` flag or a default.

CI tests (`tests/lifecycle_tests.rs`):

- Happy path: init a fixture vault, run `process --backend mock`, assert manifest state, frontmatter content, flag store.
- Missing report: mock backend that writes no report → assert workspace preserved, vault untouched.
- Precondition failure: fixture report with `create_note` on existing path → assert workspace discarded.
- Resume flow: simulate a crash at "synthesizing" phase → `recover --resume` re-runs teardown with the same workspace.
- Snapshot restore: simulate a crash mid-merge (rename snapshot dir manually) → `recover` detects and restores.

---

### Phase 1 — Topic Tracking, Sources, Markdown Infrastructure

**Goal:** Manifest tracks session/source ↔ topic relationships derived from frontmatter; diff and flags are useful review surfaces; locations are queryable.

#### 1.1 `comrak` Integration (`markdown.rs`)

Add `comrak` with extensions. Implement:

- `parse_headings(content: &str) -> Vec<Heading>` — heading text + level + byte offset.
- `derive_heading_scheme(headings: &[Heading]) -> Vec<String>` — infer `["chapter", "section", "subsection"]` or similar from depth counts.
- `resolve_location(raw: &str, tree: &[Heading]) -> Location` — parse `"1.1.2"` or heading text to `{path: [1,1,2], label: "...", raw: "1.1.2"}`.

**⚠ Issue 7 — `comrak` and Obsidian markdown.** `comrak` parses CommonMark (+ GFM extensions). Obsidian wiki links (`[[...]]`) and block anchors (`^block-id`) are **not** CommonMark — `comrak` will not give you AST nodes for them. This is expected (the design acknowledges block anchors must be a regex pass), but it also applies to wiki links: `[[inheritance]]` will be parsed by `comrak` as literal text, not a link node. The `obsidian.rs` regex extraction (Phase 0) is the correct approach for both. Do not attempt to configure `comrak` to parse these as links — it will not work without a custom AST extension that is more trouble than the regex approach. **Document this explicitly in `obsidian.rs` so future contributors know it is intentional.**

#### 1.2 Topic Entries in Manifest

Richer `_session.md` (topics, coverage, pending, existing block IDs, open flags per topic). Implement `pending_sessions` derivation: sessions whose `topics_updated` overlap with the topic's domain and postdate `last_updated`.

#### 1.3 `sources/` Ingestion (`process --source ID`)

Source path resolution: flat file → `sources/TAPL-notes.md` → source ID `TAPL-notes`; nested → `sources/SICP/SICP-ch01.md` → ID `SICP/SICP-ch01`. Wrap in `<textbook_source id="..." book="..." unit="...">` tag. Skip Plaud checks. Feed the same §7 pipeline.

#### 1.4 `rename_topic` Structural Op

Execution:

1. Rename `_synthetic/<old>/` → `_synthetic/<new>/` in the workspace.
2. Update `topic:` field in frontmatter of all moved notes.
3. Regex-replace all `[[old` → `[[new` and `![[old` → `![[new` occurrences across the entire workspace `_synthetic/` tree.
4. Update manifest topic key.

**⚠ Issue 8 — Vault-wide link rewriting scope.** `rename_topic` must rewrite links across **all files in `_synthetic/`**, not just the notes in the renamed topic. In Phase 4, structural ops will need to rewrite links across the full vault (raw notes might reference synthetic notes via wiki links). For Phase 1, confine rewriting to `_synthetic/` — that is where the AI writes links. But design the `link_rewriter` function to accept an arbitrary root so Phase 4 can extend it trivially.

**Why `rename_topic` in Phase 1 and not Phase 0:** A mistyped topic name is a genuine week-one friction point. A wrong-topic note without `rename_topic` means manual filesystem surgery, which defeats the point of the CLI. This is the right call.

#### 1.5 `csnotes diff`

Read `_generated/last_report.json` (copied there from the workspace during merge-back). Render per-note change summaries and flags. If shadow-git is configured, augment with `git diff --stat`.

#### 1.6 `csnotes flags`

`list`: read `_generated/flags.json`, filter to `open: true` and `kind` ∈ {`possible_misread`, `needs_confirmation`} (actionable). With `--all`: also show threads and changelog.

`resolve <id>`: mark `open: false`, record `resolved_at`. For `needs_confirmation` flags that imply a data change (e.g., relationship label), record the follow-up instruction for the next session's briefing (store in the flag entry as `follow_up`; surface in `_session.md` re-injection at Phase 1).

---

### Phase 2 — Extraction, Reconciliation, Artifacts

**Goal:** Passive manifest maintenance without a daemon; extraction is usable; missing inputs surface gracefully.

#### 2.1 `csnotes reconcile`

Walk `raw/`, `plaud/`, `artifacts/`, `sources/`. For each file:

- Parse the filename against the `filename_format` stored per-session (or the current config for new files).
- Register new unprocessed sessions in the manifest.
- Match Plaud exports and artifacts to their session by (course, date) key.
- **Flag** any filename containing a space; with `--rename-spaces [hyphens|underscores]` (default: hyphens), rename the flagged files immediately and update the manifest. Without the flag, report-only — no mutation.
- **Flag** any artifact folder that contains no `.md` file (only non-text formats).
- Run reconcile automatically at the start of `process` and `status`.

**Plaud qualifier parsing:** After the base stem (e.g., `CS501-07-30`), the qualifier is whatever follows the last `-`. The recognized qualifier set is **configurable** via `plaud_qualifiers` in `.csnotes` — the default list is `["transcript", "summary", "mindmap"]` plus single lowercase letters (`a`–`z`). Users can extend the list as they discover Plaud output formats they want to name (e.g., adding `"summary-1"`, `"summary-2"`, `"highlights"`). Any suffix not in the configured list is treated as an unrecognized file and flagged — never silently ignored or misidentified.

```toml
# .csnotes — default; extend freely
plaud_qualifiers = ["transcript", "summary", "mindmap"]
# single lowercase letters [a-z] are always recognized as anonymous recordings
```

`csnotes init` writes the default list. `csnotes config --set` updates it.

#### 2.2 `csnotes extract`

Regex-based scan of raw notes and Plaud exports. Patterns:

- Action items: lines starting with `- [ ]`, `TODO`, `action:`, `hw:`.
- Deadlines: date-adjacent words (`due`, `deadline`, `submit by`).
- Questions: lines ending in `?` or starting with `Q:`.

Output to `_generated/action-items.md` or stdout. Touches no manifest.

#### 2.3 `process --topic TOPIC` (Study Session)

A focused session with no new session input — just the topic's existing synthetic notes plus the briefing. Use case: review, quiz, or refine an existing topic outside of post-lecture synthesis.

Workspace assembly differences from a normal session:
- No XML-wrapped session inputs (no raw notes, Plaud, or artifacts).
- `_session.md` is topic-focused: full content of the topic's notes, all block IDs, open flags for that topic.
- The instruction file still gets copied; `synthesis.md` is still available if the user wants to make edits.

The teardown pipeline is identical. If the AI makes edits and emits a report with `update_note` ops, they commit normally. If it emits an empty `operations: []`, the pipeline completes cleanly with no vault mutations. A `--topic` session does *not* set `plaud_missing` — there is no expected Plaud input to miss.

#### 2.4 No-Plaud-Export Prompt

Before workspace assembly in `process`, if no Plaud exports match the session: print `No Plaud exports found for <course> <date>. [p]ause / [c]ontinue / [q]uit`. `[c]` sets `plaud_missing: true` in the session manifest entry and adds a note to `_session.md`: `> Plaud exports not available for this session. Synthesize from raw notes only.`

---

### Phase 3 — Crash-Safety Hardening

**Goal:** An interrupted session never corrupts the manifest, never orphans a blocking workspace, and never partially applies a report.

#### 3.1 Property Tests

Use `proptest` for:

- **Idempotency:** apply the same fixture report twice → `assert_eq!(vault_state_after_first, vault_state_after_second)`. This requires a `vault_snapshot()` function that hashes all frontmatter and the manifest.
- **Transactionality:** any precondition failure in any op → `assert_eq!(vault_bytes_before, vault_bytes_after)`. Parameterize over which op in the sequence fails.
- **Reindex fidelity:** `csnotes audit --reindex` on a committed vault → `assert_eq!(reindexed_manifest, committed_manifest)`.

#### 3.2 Mid-merge Crash Simulation

In test code, patch the merge-back to `panic!()` at various points (after N files copied). Assert:

- Snapshot is present and restorable.
- `recover` restores the snapshot, leaving the vault byte-identical to pre-session.
- Second `recover` run is idempotent.

---

### Phase 4 — Structural Ops, Polish, Extension

**Goal:** The knowledge graph can be refactored safely and reversibly; vault hygiene is automatable.

#### 4.1 Full Structural Op Suite

Implement the remaining ops: `move_atomic`, `promote_atomic`, `demote_topic`, `merge_topics`, `split_topic`, `set_embed`. Each follows the same pattern:

1. Validate preconditions (sources exist, targets are free).
2. Mutate the workspace filesystem (move files, update frontmatter).
3. Rewrite all inbound `[[wikilinks]]` and `![[embeds]]` across the workspace.

For Phase 4 structural ops, link rewriting must cover the full workspace (not just `_synthetic/`) because raw notes and sources may contain `[[synthetic-note]]` references. Use the `cross_embedded_in` reverse index from frontmatter as a fast path, then fall back to a full `walkdir` scan for plain wikilinks.

**⚠ Issue 11 — `cross_embedded_in` maintenance.** The `cross_embedded_in` field in atomic frontmatter records which other notes embed this atomic. The CLI must keep this up to date during every content op (Phase 0) and during structural ops (Phase 4). In Phase 0, the only source of truth for this field is `embed_in` declarations in `create_note` ops and `![[embed]]` lines found in the body by the regex pass. Define the maintenance rule precisely: rebuild `cross_embedded_in` from a fresh regex scan of `_synthetic/` during every merge-back, rather than trying to incrementally maintain it. This is O(n) in synthetic notes and is correct by construction.

#### 4.2 Shadow-git Snapshot Mode

If `snapshot_mode = shadow_git`: instead of a directory snapshot, commit the `_synthetic/` working copy to a bare git repo outside the vault (e.g., `$XDG_DATA_HOME/csnotes/<vault_id>/synthetic.git`). Use `std::process::Command` to invoke `git` (no git2 crate needed). Diff command: `git diff HEAD~1 HEAD --stat`.

#### 4.3 Remaining Commands

- `config --archive COURSE`: move course to an `archived_courses` list in `.csnotes`; `status` stops surfacing its unprocessed sessions as actionable.
- `audit --fix`: apply mechanical repairs (re-insert missing `![[embed]]` line into an index note whose frontmatter declares the atomic in `embeds`; clear orphan `session_in_progress`).
- `config --migrate`: rename existing files to the new `filename_format` after user confirmation.
- `--dry-run` on `process`: print workspace plan and would-be `_session.md` without writing anything.

---

## 5. Filename Format Parser

This is a small but error-prone piece that appears in multiple commands. Implement it as a standalone module with exhaustive tests.

```rust
pub struct FilenameFormat(String);  // validated at construction

impl FilenameFormat {
    pub fn parse(s: &str) -> Result<Self>   // validates tokens, separators, no-spaces
    pub fn render(&self, course: &str, date: NaiveDate) -> String
    pub fn try_parse_filename(&self, stem: &str) -> Option<(String, NaiveDate)>
    // Returns (course, date) if the stem matches this format, None otherwise.
}
```

The `try_parse_filename` function is used by `reconcile` to identify session files. It must handle all valid token orderings and the `-`-delimiter constraint. Reject ambiguous parses at `parse()` time (two adjacent variable-length tokens without a separator).

---

## 6. Implementation Order Within Phase 0

The order matters: some pieces have no dependencies; others block everything.

1. `error.rs`, `config.rs` — no dependencies.
2. `manifest.rs` types — data structures only, no logic.
3. `frontmatter.rs` — YAML read/write, provenance merge, dedupe.
4. `report.rs` — JSON parse/validate.
5. `obsidian.rs` — block-anchor + wiki-link regex (unit-testable immediately).
6. `commands/init.rs` — depends on config + manifest + a stub instruction file.
7. `workspace.rs` — depends on config + manifest + obsidian (for block-ID scan for `_session.md`).
8. `backend.rs` + mock — depends on workspace.
9. `ops/content.rs` — create_note + update_note execution, depends on frontmatter + report.
10. `audit.rs` — depends on obsidian + manifest + frontmatter.
11. `commands/process.rs` — wires 7+8+9+10 into the §7 pipeline.
12. `commands/recover.rs` — depends on manifest + workspace.
13. `commands/status.rs`, `commands/audit_cmd.rs` — read-only, depend on manifest.
14. `tests/lifecycle_tests.rs` — wire the mock backend end-to-end.

---

## 7. Open Questions and Issues (Collected)

| # | Status | Location | Issue | Resolution |
|---|---|---|---|---|
| 2 | ⚠ open | Instruction files | `claude.md`, `gemini.md`, `synthesis.md` are load-bearing but unspecified. | Annotate requirements during CLI build; draft with LLM assistance as a co-equal Phase 0 deliverable. |
| 4 | ✓ | macOS `$XDG_RUNTIME_DIR` | Not set by default on macOS; fallback chain needs to be explicit. | `$XDG_RUNTIME_DIR` → `$TMPDIR` → `/tmp`. Codified in §0.2. |
| 6 | ⚠ open | `_session.md` format | Content described but no template given. AI depends on it. | Draft template in §0.5; finalize before workspace assembly is implemented. |
| 7 | ✓ | `comrak` + Obsidian syntax | Wiki links and block anchors are not CommonMark; `comrak` will not parse them. | Regex-only extraction in `obsidian.rs`. Documented in §1.1. |
| 8 | ✓ | Link-rewriting scope | Phase 1 `rename_topic` rewrites `_synthetic/` only; Phase 4 needs full-vault scope. | `link_rewriter` takes an arbitrary root. Noted in §1.4. |
| 11 | ✓ | `cross_embedded_in` maintenance | Incremental maintenance is complex and error-prone. | Rebuild from full `_synthetic/` scan during every merge-back. Noted in §4.1. |
| 13 | ✓ | `embed_in` cross-check | AI can declare `embed_in` without writing the `![[…]]` line, or vice versa. | Precondition pass verifies declared `embed_in` has a matching embed line in the target index body. Soft warning if the line exists but wasn't declared. |
| 14 | ✓ | Manifest scalability | Single JSON file for a multi-year program. | Not a concern at expected scale (< 10 MB). Migrating to SQLite would only touch `manifest.rs`. |
| 15 | ⚠ open | LiveSync + merge-back window | Files written to `_synthetic/` during merge-back will be synced mid-write. | Write files one-by-one (fast, seconds-long window). LiveSync syncing a partially-written note is recoverable. Known limitation; document in user-facing notes. A staging-dir + atomic rename approach is possible but complex on macOS (directory rename is not atomic). |

---

## 8. Non-Goals (Explicit)

- **Windows support.** Unix filesystem semantics, `$XDG_RUNTIME_DIR`/`$TMPDIR`, `chmod`, and process spawning are all Unix-only. PRs welcome; not a priority.
- **Headless synthesis.** The AI runs as an interactive session. No `-p`/`--print` mode is planned.
- **PDF/PPTX extraction (for now).** The user pre-converts slide PDFs and other lecture artifacts to `.md` before running `process`. The CLI does not embed a document extractor — the right formats aren't known until class starts. This is a candidate for a companion tool once the formats are understood.
- **Live-reload / daemon.** Replaced by discrete `reconcile`. Schedule externally via launchd/systemd-timer.
- **Assignment work tracking.** Assignment _work_ repos are independent git repos outside the vault. Only reflection notes live in the vault.
