# csnotes — Design Document (v2)

## 1. Project Overview

`csnotes` is a Rust CLI for AI-assisted synthesis of lecture notes across a multi-year graduate program. It manages the boundary between two kinds of notes that serve different purposes and must never be conflated: **raw notes**, a chronological, append-only personal record, and **synthetic notes**, an AI-maintained knowledge graph organized by topic.

The tool's job is to enforce that boundary, track provenance, take crash-safe snapshots, and set up an interactive AI coding-assistant session with enough context to do synthesis work correctly. The AI's job has two halves: it acts as a **tutor** — discussing the material and quizzing the user on what is unclear — and as a **synthesizer** — maintaining the topic notes in the user's register, flagging likely errors or reframings without silently overwriting prior understanding. A session typically flows from the first half into the second: talk it through, then converge on the written notes.

**The central architectural principle (the "spine"):** the AI _declares_ what it did and what it wants done; the CLI _executes_. The AI writes note bodies and proposes structured operations; the CLI performs all folder/link mutations, owns provenance metadata, and maintains the index. This split assigns each actor the work it is reliable at — the AI for local semantics, the CLI for global mechanics — and is the organizing idea behind every section below.

**Primary use case:** After a lecture, the user runs `csnotes process`. The CLI assembles session context into an out-of-vault workspace (read-only input copies plus a writable copy of the synthetic notes) and launches an interactive AI session that runs entirely inside it. The user discusses the material; the AI edits the synthetic-note working copies and emits a structured session report on exit; the CLI validates the report, executes any structural operations transactionally, merges the results back into the vault, stamps provenance, and reindexes — or, on failure, discards the workspace, leaving the vault untouched.

**Secondary use cases:** Digesting date-agnostic reference material (textbook chapters, papers) via `csnotes process --source`; extracting action items and deadlines; reviewing what has and hasn't been processed; reconciling newly dropped files into the manifest.

**Platform:** Linux/macOS only. A deliberate constraint: the tool relies on Unix filesystem semantics and is developed on Linux and Mac. Released FOSS as a worked example of AI-directed CLI development in Rust; platform-extension contributions are welcome but not a priority.

**AI backend:** Configurable, designed for Claude Code (`claude`) or Antigravity (`agy`) interchangeably, run as **interactive** sessions (not headless `-p`). The session-context format is model-agnostic; model-specific instruction files handle style differences. A `mock` backend exists for testing (§11).

---

## 2. Core Architecture

### 2.1 The Two-Layer Model

|Layer|Format|Author|Mutability|Purpose|
|---|---|---|---|---|
|Raw notes|Chronological `.md` per session|User|Append-only|Ground truth, personal record|
|Synthetic notes|Topic-organized `.md` per concept|AI (body) + CLI (frontmatter)|Incrementally updated|Reference, study, knowledge graph|

The CLI enforces raw-note immutability at the filesystem level during sessions: the AI runs inside an out-of-vault workspace (§4.5) and can reach only read-only copies of its inputs, never the originals. The instruction layer enforces it at the instruction level too. Neither alone is sufficient.

### 2.2 The Spine: AI Declares, CLI Executes

Every AI action splits along one axis: **content or structure.**

- **Content** — prose, embeds, block anchors, inline annotations, fact-checking asides — the AI writes **in place** into note bodies. Markdown-in-a-file is the medium it is best at.
- **Structure** — a new topic exists, a concept was atomized, CS601 reframed CS501, promote this atomic note to a topic — the AI **declares** in the session report (§5). It never mutates folder layout and never rewrites cross-vault links.
- **The CLI executes.** It reads declared structure, performs every mechanical mutation (create folders, move files, rewrite all inbound `[[wikilinks]]`/`![[embeds]]` vault-wide), stamps frontmatter, and rebuilds the index — transactionally, against a vault left untouched until merge-back (§4.5, §7), so any failure leaves the vault pristine.

This matches where each actor is trustworthy. The AI is reliable at _this paragraph means X_ and catastrophic at _rewrite 40 links without breaking 3_; the CLI is the reverse.

### 2.3 Four Artifacts, One Writer Each

|Artifact|Writer|Role|Durability|
|---|---|---|---|
|Note **body** (prose, embeds, anchors, annotations, asides)|**AI**|The synthesis itself|Permanent|
|Note **frontmatter** (`---` fence)|**CLI**|Canonical, self-describing provenance|Permanent|
|**Session report** (`_session_report.json`)|**AI**|Per-run changeset + narration + flags|Ephemeral (per run)|
|**Manifest** (`csnotes.json`)|**CLI**|Derived index over frontmatter + filesystem|Disposable (rebuildable)|

Plus one small store that is neither note nor index: the **flag store** (`_generated/flags.json`, §5.3) — durable, mutable, _semantic_ state (the AI's review queue). The AI proposes flags via the report; the user resolves them via `csnotes flags`; the CLI owns the state transitions.

**Precedence on conflict:** the filesystem is truth for what exists; frontmatter is truth for provenance; the session report is authoritative only for _intent_ (what to execute) and _narration_ (what to say); the manifest is never authoritative — if it disagrees with frontmatter, it is stale and gets rebuilt.

### 2.4 Topic-Primary Namespace

Synthetic notes are organized by **topic**, not by course. A note on inheritance begun in an intro OOP course may be updated by a systems-design course two years later. The organizing question is "what is this about," not "when was this covered." Course identity is preserved as metadata (frontmatter provenance), not structure.

### 2.5 Intellectual History as First-Class Data

When a later session reframes, contradicts, or substantially extends prior understanding, the prior framing is **not overwritten**. It moves into a Conceptual History section with a characterization of how the new material relates to it. _Current Understanding_ reflects best current synthesis; _Conceptual History_ reflects how understanding developed. In a graduate program this is both epistemically honest and pedagogically useful.

### 2.6 Granularity Target: Man-Page Level

Each synthetic note should be self-contained and useful standalone, at roughly the granularity of a man page. A concept with its own definition, its own syntax, and independent lookup value gets its own atomic note. Topic folders contain an index note that embeds atomic children via Obsidian block embeds, plus the atomic notes themselves.

```
_synthetic/
  inheritance/
    inheritance.md          ← index, embeds children
    polymorphism.md
    method-overriding.md
  method-declarations/
    method-declarations.md  ← index
    static.md
```

**Atomization heuristic for the AI:** if a concept has its own definition, its own syntax, and could plausibly be looked up independently, it gets its own page.

---

## 3. State Model

The state model is what makes "CLI owns the manifest" both true and non-fictional, given that the manifest is full of facts only the AI knows. The resolution: those facts do not _originate_ in the manifest. They live in note frontmatter (canonical), and the manifest is a rebuilt index over them.

- **Canonical:** per-note frontmatter, written by the CLI from the AI's declared provenance deltas. Each note self-describes its full current provenance, survives `rm csnotes.json`, and is greppable in Obsidian. (Snapshot model, not event-sourcing — each file is independently meaningful.)
- **Derived index:** the manifest. A cache over (filesystem ∪ frontmatter), rebuildable any time with `csnotes audit --reindex`. Disposable, which is a major robustness win.
- **Changeset:** the session report. A per-run record of what changed; lose it and you fall back to re-deriving from frontmatter. It is not a store.
- **Render:** `_session_workspace/_session.md`, the per-run briefing, derived from the manifest at launch and never read back. It cannot drift in a way that matters because nothing trusts it as state.

### 3.1 Per-Note Frontmatter (CLI-written, canonical)

The CLI is the sole writer of the YAML fence. It produces it by merging the AI's declared provenance deltas into whatever frontmatter already exists. The AI reads frontmatter (the briefing surfaces it) but never writes it.

**Atomic note:**

```yaml
---
csnotes_schema: 1
kind: atomic
topic: inheritance
title: Polymorphism
block_id: polymorphism-core          # primary anchor; CLI verifies ^polymorphism-core exists in body
contributing_sessions:
  - { course: CS501, date: 2026-07-30, relationship: introduced }
  - { course: CS601, date: 2027-03-12, relationship: reframed }
contributing_sources:
  - { source_id: SICP/SICP-ch01, location: { path: [1,1,2], label: "Linear Recursion and Iteration", raw: "1.1.2" }, relationship: introduced }
cross_embedded_in:
  - _synthetic/evaluation/evaluation.md
created:      2026-07-30T14:10:00Z
last_updated: 2027-03-12T09:30:00Z
---
```

**Index note:**

```yaml
---
csnotes_schema: 1
kind: index
topic: inheritance
title: Inheritance
embeds: [polymorphism, method-overriding]   # home-topic atomics embedded, in order
contributing_sessions:
  - { course: CS501, date: 2026-07-30, relationship: introduced }
contributing_sources: []
created:      2026-07-30T14:10:00Z
last_updated: 2027-03-12T09:30:00Z
---
```

**Enums:** `relationship` ∈ {introduced, extended, reframed, contradicted, nuanced}.

**Source locations** are a path through the source's own heading hierarchy (§6.3), stored three ways — `path` (canonical numeric coordinate, queryable/sortable), `label` (resolved heading text, stable across renumbering), `raw` (what the AI wrote).

---

## 4. Vault Structure

```
dir/
  raw/
    CS501/
      CS501-07-28.md                ← daily lecture notes, user-authored
      CS501-07-30.md
  plaud/
    CS501/
      CS501-07-28-transcript.md     ← Plaud transcript export, manually copied
      CS501-07-28-summary.md        ← Plaud summary export (optional)
      CS501-07-28-mindmap.md        ← Plaud mind map export (optional)
      CS501-07-30-a.md              ← multiple exports per day supported
      CS501-07-30-b.md
  artifacts/
    CS501/
      CS501-07-28/                  ← session-specific artifact folder
        slides.md                   ← slides, text-extracted to markdown (see §4.4)
        main.rs                     ← instructor example code
        utils.rs
  sources/
    SICP/                           ← book/resource namespace folder
      SICP-ch01.md                  ← chapter-level notes
      SICP-ch02.md
    dragon-book/
      dragon-ch01.md
    TAPL-notes.md                   ← flat file (allowed for short sources)
  _synthetic/
    inheritance/
      inheritance.md
      polymorphism.md
    method-declarations/
      method-declarations.md
      static.md
  _generated/
    action-items.md                 ← output of `csnotes extract`
    flags.json                      ← review-flag store (§5.3)
  _csnotes/
    instructions/
      claude.md                     ← static Claude instructions; CLI copies to workspace root as CLAUDE.md
      gemini.md                     ← static Antigravity instructions; CLI copies as GEMINI.md
      synthesis.skill               ← the wrap-up synthesis SKILL (§9)
  csnotes.json                      ← manifest (derived index)
  .csnotes                          ← vault configuration

# NOT in the vault — the AI runs here, cwd = the workspace (§4.5):
$XDG_RUNTIME_DIR/csnotes/<run_id>/  ← read-only input copies, writable _synthetic/ copy,
                                      _session.md, copied instruction file, _session_report.json,
                                      pre-merge snapshot
```

### 4.1 Inputs: sessions vs sources

There are two kinds of input. A **session** is a date-anchored bundle — a raw note plus its optional Plaud exports plus its optional lecture artifacts (slides, instructor code), all keyed to a course and date. A **source** is a date-agnostic reference — a textbook chapter, paper, or any standing material the user summarizes. Both flow through the same synthesis machinery and the same operation vocabulary; the only difference is whether a contribution lands in `contributing_sessions` or `contributing_sources`.

`sources/` supports both flat files (short papers) and nested folders (a book directory with per-chapter files). Source IDs are relative-path stems: a flat file is `TAPL-notes`; a nested file is `SICP/SICP-ch01`.

### 4.2 File Naming Convention

Filenames are generated from a configurable format string set during `csnotes init`, stored in `.csnotes`:

```toml
filename_format = "{course}-{mm}-{dd}"   # default
```

Available tokens: `{course}`, `{yyyy}`, `{mm}`, `{dd}`. The default omits the year **by deliberate choice**: course identity is the salient differentiator (courses do not span years and are taken in order), and `CS501-07-28` reads faster than `CS501-2026-07-28`. Internal correctness does not depend on the filename year — the manifest and frontmatter always store the full ISO date, so sorting and range queries are unambiguous regardless of what the filename shows. Year-inclusive (`{course}-{yyyy}-{mm}-{dd}`) and date-first formats are available for users who prefer them.

**Constraints enforced by the CLI:**

- `{course}` must be present (cross-course collision protection in Obsidian depends on it).
- At least one date token must be present (otherwise filenames are non-unique across sessions).
- Tokens must be `-`-delimited; adjacent variable-length tokens without a separator are rejected (the reconciler must parse filenames unambiguously).
- **No spaces** anywhere in configured names or in files dropped into the vault (§4.3).

The Plaud qualifier convention (`-transcript`, `-summary`, `-mindmap`, `-a`, `-b`) appends after the base filename regardless of format. Using the default format:

- **Raw note:** `CS501-07-28.md`
- **Plaud transcript:** `CS501-07-28-transcript.md`
- **Plaud summary:** `CS501-07-28-summary.md`
- **Plaud mind map:** `CS501-07-28-mindmap.md`
- **Multiple recordings same day:** `CS501-07-30-a.md`, `CS501-07-30-b.md`

The manifest stores the `filename_format` used per session entry, so `csnotes reconcile` parses filenames unambiguously even across a mid-program format change. `csnotes config --set filename_format=...` warns that existing files use the old format and offers `--migrate` to rename; without `--migrate`, old files remain recognized via their stored format and only new files use the updated one.

### 4.3 No-Spaces Invariant

Spaces fight the terminal-centric workflow, Rust argument/path handling, the `-`-delimited filename parser, and Obsidian wiki-link/embed resolution. The CLI enforces a no-spaces invariant at the boundary:

- `csnotes init` / `config` reject configured names containing spaces.
- `csnotes reconcile` **flags** (does not silently rename) any manually copied file containing a space, with a one-command fix offered. Auto-renaming a file Obsidian may have open is itself a hazard, so reconciliation reports rather than mutates.

Keep names alphanumeric with `-`/`_` delimiters.

### 4.4 Lecture Artifacts (slides, instructor code)

Artifacts live in `artifacts/<course>/<session-stem>/` and are ingested as session inputs. Text-bearing artifacts must be markdown by the time `csnotes process` runs: **the user pre-extracts** slide PDFs to `.md` (the CLI does not embed a PDF text extractor — that dependency is flaky and out of scope). `csnotes reconcile` flags an artifact folder that contains only non-text formats (e.g., a lone `slides.pdf` with no `slides.md`) so the gap is visible rather than silent. Instructor code files are copied and wrapped as-is.

### 4.5 The Session Workspace (sandbox + merge-back)

The AI session runs **entirely inside an out-of-vault workspace**, under `$XDG_RUNTIME_DIR/csnotes/<run_id>/` (falling back to `$XDG_CACHE_HOME` / a temp dir). The AI is launched with **cwd = the workspace** — it never has the vault as its working directory — and the CLI merges the results back into the vault on a clean exit. This is what makes the raw-note immutability guarantee real: the AI can only reach what the CLI placed in the workspace, so the read-only input copies are genuinely the only copies it can touch (cwd = vault root would defeat this entirely, since the originals would sit in the working directory).

The CLI assembles the workspace before launch and it contains:

- **Read-only, XML-wrapped copies of all session/source inputs** (raw notes, Plaud exports, artifacts, sources). The originals in the vault are never exposed to the AI.
- **A writable working copy of `_synthetic/`** — the AI edits note bodies here, not in the vault. Copy all of `_synthetic/` (markdown is cheap) so cross-links and embeds resolve. This working copy doubles as the pre-session baseline.
- **The rendered `_session.md` briefing** (existing block IDs, relevant open flags, manifest state).
- **The instruction file**, copied by the CLI to the workspace root as `CLAUDE.md` or `GEMINI.md` (per `skill_variant`) plus the synthesis SKILL — so backend auto-discovery works against the workspace root the CLI controls (§9). This is why the instruction sources can live in a tidy folder in the vault (§4, `_csnotes/`) rather than cluttering the vault root.
- **`_session_report.json`**, written by the AI on exit.

On a clean exit the CLI **merges modified synthetic notes back** into the real vault `_synthetic/` (and stamps frontmatter, updates the manifest — §7). The vault is untouched until that merge, so a crash mid-session needs no in-vault rollback: discard the workspace and the vault is already pristine. The only crash window that needs guarding is the few seconds of merge-back itself, covered by a snapshot taken immediately before it (§7, §10). An orphaned workspace lives in `tmpfs` and self-clears on reboot; `csnotes recover` handles it explicitly otherwise.

### 4.6 Assignments (separate work layer)

Homework has two halves with opposite lifecycles and they should not share a location. The **work layer** — code you submit, full of build artifacts (`.class`, etc.) — lives in its **own git repo**, outside the vault or `.gitignore`'d from it, and is never synced into Obsidian. The **reflection layer** — feedback received, "what I got wrong," post-mortems — lives in the vault as a date-agnostic source (e.g., `sources/assignments/...` with `type: assignment-feedback`) and _is_ AI-readable. So "can the AI see my homework" resolves to: the AI sees your _reflection notes_, not your build tree, unless you deliberately drop a specific file into a source path. The `sources/` type tag is a deliberate catch-all so that assignment logistics (unknown until the term is underway) can be slotted in without schema surgery — design the seam, specialize later.

### 4.7 Obsidian Syntax Requirements

The AI must emit valid Obsidian markdown:

- Wiki links: `[[note-name]]` or `[[note-name#section]]`
- Block anchors: `^block-id` at the end of the anchored line
- Block embeds: `![[note-name#^block-id]]`
- Block IDs: lowercase, no spaces, **unique within the vault**

Because block IDs must be vault-unique but the AI sees only the current session's context, `_session.md` surfaces existing block IDs for every note the session is about to touch, and the instruction layer tells the AI to reuse-or-disambiguate accordingly. The CLI's audit pass (§7) is the backstop that rejects any collision.

---

## 5. The Session Report (Sidecar) Contract

Written by the AI into the workspace at wrap-up. One per run. It is the keystone of the spine.

### 5.1 Top-level shape

```json
{
  "csnotes_report_schema": 1,
  "run_id": "9f2c-…",                 // MUST match manifest.session_in_progress.run_id
  "backend": "claude",                // or "agy" / "mock"
  "started_at":   "2027-03-12T08:55:00Z",
  "completed_at": "2027-03-12T09:30:00Z",
  "scope": {
    "kind": "session",                // "session" | "source" | "mixed"
    "sessions": ["CS601-03-12"],
    "sources":  []
  },
  "operations":   [ /* §5.2 */ ],
  "review_flags": [ /* §5.3 */ ]
}
```

`scope` unifies session-runs and source-runs (`process --source`): the operation vocabulary is identical; provenance deltas inside the ops carry `sessions` vs `sources`.

### 5.2 Operations

Two tiers: **indexing ops** (Phase 0) declare content the AI wrote in place; **structural ops** (mostly later phases, defined now so the seam exists) are mechanical, vault-global mutations the AI only _requests_.

**Indexing ops — Phase 0**

```json
{ "op": "create_note", "kind": "atomic",
  "path": "_synthetic/inheritance/polymorphism.md",
  "title": "Polymorphism", "topic": "inheritance",
  "block_id": "polymorphism-core",
  "embed_in": ["_synthetic/inheritance/inheritance.md"],
  "provenance": { "sessions": [{ "course": "CS601", "date": "2027-03-12", "relationship": "introduced" }], "sources": [] },
  "change_summary": "New atomic: polymorphism as dynamic dispatch." }
```

```json
{ "op": "update_note",
  "path": "_synthetic/inheritance/inheritance.md",
  "add_provenance": { "sessions": [{ "course": "CS601", "date": "2027-03-12", "relationship": "reframed" }], "sources": [] },
  "sections": ["Current Understanding", "Conceptual History"],
  "change_summary": "Moved Monday's definition into Conceptual History; CS601 reframes inheritance as interface-sharing." }
```

- The AI writes the body, including `![[…]]` embed lines, `^block-id` anchors, history headers, and asides. `embed_in` is a _declaration_ the CLI validates and indexes; the CLI inserts/rewrites embed lines mechanically only during structural ops.
- `create_note` on an existing path is a **precondition failure** (§7) — the guard rail that pushes a re-run toward `update_note` rather than clobbering.
- `update_note.add_provenance` is a _delta_; the CLI merges it into frontmatter and dedupes on `(course,date)` / `(source_id, location.path)`, so re-recording a session is caught, not accumulated.

**Structural ops — `rename_topic` in Phase 1; the rest Phase 4**

```json
{ "op": "rename_topic",  "from": "inheritence", "to": "inheritance", "reason": "typo" }
{ "op": "move_atomic",   "from_path": "_synthetic/inheritance/dispatch.md", "to_topic": "method-declarations", "reason": "…" }
{ "op": "promote_atomic","from_path": "_synthetic/inheritance/polymorphism.md", "to_topic": "polymorphism", "reason": "now stands alone" }
{ "op": "demote_topic",  "from_topic": "subclassing", "into_topic": "inheritance", "reason": "…" }
{ "op": "merge_topics",  "from": ["typing","type-systems"], "into": "type-systems", "reason": "…" }
{ "op": "split_topic",   "from": "memory", "into": [
    { "topic": "stack", "atomics": ["_synthetic/memory/stack.md"] },
    { "topic": "heap",  "atomics": ["_synthetic/memory/heap.md"] } ], "reason": "…" }
{ "op": "set_embed",     "atomic_path": "…", "index_path": "…", "present": true }
```

The CLI executes each by moving files and rewriting **every** inbound `[[wikilink]]`/`![[embed]]` vault-wide, reversibly against the snapshot. The AI never runs that find-replace. `rename_topic` is pulled into Phase 1 deliberately — a mistyped topic name is a week-one certainty and it is the lowest-risk structural op (pure rename, no merge/split semantics).

### 5.3 Review flags and their lifecycle

Flags are the AI's out-of-band channel to the user — the surfacing mechanism behind "never silently correct, always surface." The AI proposes a flag in the report; the CLI appends it to the **flag store** (`_generated/flags.json`) with an `id` and `open` state; the user resolves via `csnotes flags resolve <id>`. **Flags persist across runs** — a surfacing mechanism that evaporated after one run would barely surface at all across a multi-year program.

```json
{ "kind": "possible_misread",   "path": "…", "anchor": "^static-defn",
  "message": "Transcript 'static binding resolves at runtime' is contradictory; likely 'dynamic dispatch'. Flagged, raw note untouched." }
{ "kind": "needs_confirmation", "path": "…",
  "message": "Labeled CS601 'reframed' not 'contradicted'; affects Conceptual History. Confirm?" }
{ "kind": "unresolved_question","message": "You were fuzzy on covariance vs contravariance — worth revisiting." }
{ "kind": "ambiguity",          "path": "…",
  "message": "Chose dynamic/static split over early/late-binding split, FYI." }
```

Two behaviors make persistence pay off beyond nagging:

- **Resolution can change data.** Resolving a `needs_confirmation` about a `reframed`-vs-`contradicted` label feeds back into a frontmatter edit (a small CLI op or a next-session instruction). The flag must _wait for the user_, who may not know until they've asked the professor.
- **Open flags re-enter the next briefing.** `_session.md` includes open flags relevant to the upcoming session, so a `unresolved_question` about variance resurfaces when the next lecture touches it: the discuss/quiz half produces residue and the residue closes itself a session later.

**Tier the kinds** to keep the queue from becoming noise: `possible_misread` and `needs_confirmation` are _actionable_ → persist and nag in `status`; `ambiguity` is _informational_ → logged and auto-resolved, never nags; `unresolved_question` is a _thread_ → persists but surfaces only via briefing re-injection, not as a standing warning. Mental model: actionable flags are a to-do queue, the rest are a changelog.

### 5.4 Worked example (one reframe run)

AI body edit to `_synthetic/inheritance/inheritance.md` (frontmatter is CLI territory, omitted):

```markdown
# Inheritance

## Current Understanding
Inheritance is interface-sharing with implementation reuse as a side effect.
*(reframed CS601 03/12; see history)*
![[polymorphism#^polymorphism-core]]

## Conceptual History
### CS501 — 2026-07-30 (Introduced)
"A subclass is a kind of its superclass and gets its methods for free."

### CS601 — 2027-03-12 (Reframed)
> **[Claude: reframe note]** CS601 reframes inheritance around interface
> conformance; the CS501 "free methods" framing is the mechanism, not the point.
```

AI emits `_session_report.json`:

```json
{ "csnotes_report_schema": 1, "run_id": "9f2c", "backend": "claude",
  "started_at": "2027-03-12T08:55:00Z", "completed_at": "2027-03-12T09:30:00Z",
  "scope": { "kind": "session", "sessions": ["CS601-03-12"], "sources": [] },
  "operations": [
    { "op": "update_note", "path": "_synthetic/inheritance/inheritance.md",
      "add_provenance": { "sessions": [{ "course": "CS601", "date": "2027-03-12", "relationship": "reframed" }], "sources": [] },
      "sections": ["Current Understanding", "Conceptual History"],
      "change_summary": "Reframed inheritance as interface-sharing; prior definition moved to history." } ],
  "review_flags": [
    { "kind": "needs_confirmation", "path": "_synthetic/inheritance/inheritance.md",
      "message": "Called this 'reframed' not 'contradicted' — CS601 recasts, doesn't deny CS501. Confirm?" } ] }
```

The CLI merges the `CS601 reframed` entry into frontmatter, bumps `last_updated`, reindexes, runs the invariant suite (links resolve, block IDs unique), commits, and `status` later shows `1 open flag awaiting confirmation`.

---

## 6. Manifest Schema (derived index)

The manifest is a rebuildable cache. It is the convenient read surface for `status`/`diff`/`process` briefing generation, but it is never the source of truth — `csnotes audit --reindex` reconstructs it from frontmatter + filesystem.

### 6.1 Top-level

```json
{
  "version": "2",
  "vault_root": "/path/to/vault",
  "config": { "raw_dir": "raw", "plaud_dir": "plaud", "artifacts_dir": "artifacts",
              "sources_dir": "sources", "synthetic_dir": "_synthetic",
              "filename_format": "{course}-{mm}-{dd}" },
  "sessions": { ... },
  "sources":  { ... },
  "topics":   { ... },
  "session_in_progress": null,
  "flags_path": "_generated/flags.json"
}
```

### 6.2 Session entry

```json
"CS501-07-28": {
  "date": "2026-07-28", "course": "CS501", "filename_format": "{course}-{mm}-{dd}",
  "raw_note": "raw/CS501/CS501-07-28.md",
  "plaud_exports": [
    { "path": "plaud/CS501/CS501-07-28-transcript.md", "type": "transcript" },
    { "path": "plaud/CS501/CS501-07-28-summary.md", "type": "summary" } ],
  "artifacts": [
    { "path": "artifacts/CS501/CS501-07-28/slides.md", "type": "slides" },
    { "path": "artifacts/CS501/CS501-07-28/main.rs", "type": "code" } ],
  "plaud_missing": false,
  "status": "processed",
  "processed_at": "2026-07-29T14:23:00Z",
  "topics_updated": ["inheritance", "method-declarations"]
}
```

Status values: `unprocessed | in_progress | processed`. `plaud_exports` is a list (interrupted recordings, multiple export types); an empty list sets `plaud_missing: true`, triggering the no-transcript prompt. `in_progress` + top-level `session_in_progress` drives crash recovery.

### 6.3 Source entry and locations

```json
"SICP/SICP-ch01": {
  "path": "sources/SICP/SICP-ch01.md",
  "type": "textbook",                 // textbook | paper | assignment-feedback | other
  "status": "processed",
  "last_processed_at": "2026-05-29T19:30:00Z",
  "heading_scheme": ["section", "subsection"],   // derived from the file's heading tree
  "topics_updated": ["substitution-model"]
}
```

**Location granularity.** A source's `.md` heading hierarchy _is_ its locator scheme — derived for free by the `comrak` pass (Phase 1). A location is a coordinate in that tree, stored as `{ path, label, raw }` (§3.1). The AI may reference a location by number (`"1.1.2"`) or by heading text (`"Linear Recursion and Iteration"`); the CLI resolves either against the parsed tree and canonicalizes. This yields prefix queries ("everything in chapter 1" = `path[0]==1`), correct ordering (numeric-array compare, so `1.9 < 1.10`), and ranges. Non-numeric units (appendices, exercises) are string components under a natural-sort comparator; unstructured sources degrade to `["page"]`; spans use an optional `span_to`.

### 6.4 Topic entry

```json
"inheritance": {
  "index_note": "_synthetic/inheritance/inheritance.md",
  "atomic_notes": ["_synthetic/inheritance/polymorphism.md", "_synthetic/inheritance/method-overriding.md"],
  "contributing_sessions": [
    { "course": "CS501", "date": "2026-07-30", "relationship": "introduced" },
    { "course": "CS601", "date": "2027-03-12", "relationship": "reframed" } ],
  "contributing_sources": [
    { "source_id": "SICP/SICP-ch01", "location": { "path": [1,1,2], "label": "…", "raw": "1.1.2" }, "relationship": "introduced" } ],
  "pending_sessions": ["2026-08-04"],
  "last_updated": "2027-03-12T09:30:00Z",
  "open_flags": 1,
  "source_types": ["lecture", "transcript", "textbook"]
}
```

All semantic fields here are **re-derived from note frontmatter**, not written independently. `pending_sessions` is derived: sessions whose inputs reference the topic's domain but postdate `last_updated`. (Because the CLI cannot read meaning, "references the topic's domain" is resolved from the AI's declared `topics_updated` per session plus topic cross-links — never inferred from raw-note prose by the CLI.)

### 6.5 Invariants (checked by the audit pass)

- A topic's `contributing_sessions` may reference only sessions in `sessions`; `contributing_sources` only sources in `sources`.
- A session's/source's `topics_updated` may reference only topics in `topics`.
- `session_in_progress` is null unless some session/source has `status: in_progress`.
- Every atomic note in a topic's `atomic_notes` exists on disk with schema-valid frontmatter naming that topic.
- Block IDs are vault-unique; every `[[wikilink]]`/`![[embed]]` target resolves.

---

## 7. CLI Lifecycle: validate → execute → merge-back → commit / discard

Shared by `process` teardown, `recover`, and `audit`. The invariant that makes crash-safety real: **the vault is always in exactly one of two clean states — untouched (workspace discarded) or committed (merged). Never partial.** Because the AI works only on workspace copies (§4.5), the vault stays pristine throughout the session, so the entire validate/execute pipeline below runs against the _workspace_, and the vault is mutated only in the brief, snapshot-guarded merge-back step.

1. **Set in-progress at run start.** Before launching the AI, `process` records `session_in_progress = { run_id, started_at, workspace_path, phase: "synthesizing" }`. The pre-session baseline is the workspace's own writable copy of `_synthetic/` (and the untouched vault behind it) — no separate snapshot is needed yet.
2. **Locate + parse the report.** Absent at teardown → "no declared changes; run `audit --reindex` to reconcile" and stop (do not guess). Present → schema-validate and check `run_id` matches. A parse/schema failure **does not tear down**: preserve the workspace, report the error; the user re-enters the interactive session and has the AI rewrite the report, losing nothing.
3. **Precondition pass (dry, no mutation), in the workspace.** Validate every op's preconditions: `create_note` paths must not exist; structural-op sources must exist and targets be free; declared `block_id`s must appear as anchors in their bodies; `embed_in` targets must exist. **Any failure → abort, discard the workspace, report which op failed.** The vault was never touched.
4. **Execute structural ops** in declared order, against the workspace copy: move files, rewrite all inbound links/embeds vault-wide, update affected frontmatter.
5. **Index content ops:** for each `create_note`/`update_note`, open the workspace file, merge declared provenance into the frontmatter fence, bump `last_updated`, extract block IDs/links/embeds, build the new manifest index; stage any review flags for the flag store.
6. **Invariant suite (the audit), still in the workspace.** _Hard_ (violation → discard workspace, set `session_in_progress.error`, surface via `recover`): report parses and `run_id` matches; every declared note exists with schema-valid frontmatter; block IDs globally unique; every link/embed resolves; structural preconditions held; manifest referential integrity (§6.5). _Soft_ (warn in `status`, don't block): orphan atomic (no embedding index); `last_updated` predates a contributing session date; deduped duplicate provenance (idempotency smell); count of open flags.
7. **Merge-back + commit.** Take a pre-merge snapshot of the vault `_synthetic/` (guards the seconds-long write window), then copy the validated synthetic notes from the workspace into the vault, write the manifest, append flags to the flag store, optionally commit to history (§10 tier b/c), clear `session_in_progress`, delete the workspace and snapshot. A crash during the merge window → the snapshot restores the vault and `recover` re-runs the merge.
8. **Recover** reads `session_in_progress`: a workspace present with no successful commit → offer **Resume** (relaunch the interactive session against the same workspace) or **Discard** (delete the workspace, clear the flag — the vault is already untouched). If a pre-merge snapshot is present (crash mid-merge), recover first restores it. Because the merge-back is the only writer of the vault and is the sole snapshot-guarded step, there is no third state.

**Idempotency = observable convergence, not purity.** `create_note` preconditions block clobber; `update_note` is a content-replace against the briefing's current state, so applying twice converges rather than accumulates; provenance dedupes; an unmerged run is discarded wholesale by throwing away the workspace.

---

## 8. CLI Subcommand Reference

### `csnotes init`

Scaffolds the vault: prompts for vault root, subdirectory names (defaults offered), default course, and `filename_format` (validates `{course}` + ≥1 date token, rejects spaces). Writes `.csnotes`, an empty `csnotes.json`, an empty `_generated/flags.json`, and real (not stub) instruction sources under `_csnotes/instructions/` (`claude.md`, `gemini.md`, `synthesis.skill`).

### `csnotes process [--session DATE] [--course COURSE] [--source ID] [--topic TOPIC] [--dry-run] [--backend claude|agy|mock]`

The primary command. Lifecycle: read manifest → resolve scope (Course Resolution below, or `--source`) → check Plaud exports (no-export prompt if none) → build out-of-vault workspace (XML-wrapped read-only input copies + writable `_synthetic/` copy + rendered `_session.md` with existing block IDs and relevant open flags + the copied instruction file) → set `session_in_progress` → launch the interactive AI with cwd = the workspace → on exit run the §7 teardown (validate → execute → merge-back → commit/discard).

`--source ID` runs a source digest: skip Plaud checks, wrap the source as `<textbook_source>`, focus synthesis on mapping it into `_synthetic/`. `--dry-run` prints the plan and the would-be workspace contents without launching or mutating. `--backend mock` substitutes the test backend (§11).

**XML input wrapping** (model-agnostic, in the workspace):

```xml
<raw_student_notes course="CS501" date="2026-07-28"> …file contents… </raw_student_notes>
<plaud_transcript     course="CS501" date="2026-07-28"> … </plaud_transcript>
<plaud_summary        course="CS501" date="2026-07-28"> … </plaud_summary>
<lecture_slides       course="CS501" date="2026-07-28"> … </lecture_slides>
<instructor_code_sample file="main.rs" course="CS501" date="2026-07-28"> … </instructor_code_sample>
<textbook_source id="SICP/SICP-ch01" book="SICP" unit="Chapter 1"> … </textbook_source>
```

Tag names appear in the static instruction files as part of the interface contract.

**No-Plaud-export prompt** (`[p]ause / [c]ontinue / [q]uit`): `[c]` sets `plaud_missing: true` and notes the absence in `_session.md` so the AI does not expect transcript content.

**Course Resolution** (no `--course`): scan course folders for unprocessed sessions; one course with backlog → proceed and announce; multiple → interactive picker (single course / all / cancel); none → status summary and exit. `--course` overrides the prompt. `--session` without `--course`: proceed if the date is unique, prompt if it exists in multiple course folders (same-day multi-course sessions live in their respective folders as distinct entries). "Process all" runs one interactive session spanning all pending inputs across courses (and, if requested, unprocessed sources) for catch-up.

### `csnotes status`

Reads the manifest, prints a human summary: synthetic-note coverage per topic with pending markers, raw sessions, transcripts matched/missing, unprocessed sessions **and unprocessed sources**, stale-course nudges (§ below), and **open actionable flags**.

### `csnotes diff [--session DATE]`

Semantic (not line) summary of what changed last run, rendered from the report's `change_summary` fields, structural ops, and flags:

```
inheritance/inheritance.md
  ~ Restructured: CS601 reframes Monday's definition (moved to Conceptual History)
  ! 1 flag awaiting confirmation (reframed vs contradicted)
```

The structural half can also draw on git line-diff (§10) where available; the semantic narration always comes from the AI via the report.

### `csnotes flags [list|resolve <id>|show <id>]`

The review queue. `list` shows open actionable flags (and, with `--all`, threads and changelog entries); `resolve <id>` marks a flag resolved and, where the flag implies a data change, records the follow-up for the next session. The CLI owns flag state; the AI only proposes via the report.

### `csnotes extract [--session DATE] [--type actions|deadlines|questions] [--stdout]`

Read-only. Scans raw notes and transcripts for assignment mentions, deadline dates, and question-shaped lines. Output to `_generated/action-items.md` or stdout. Touches neither manifest nor sources.

### `csnotes reconcile [--notify]`

Replaces the v1 watch _daemon_ with **discrete, on-invocation reconciliation** (also run automatically at the start of `process`/`status`). Scans `raw/`, `plaud/`, `artifacts/`, `sources/`; registers new unprocessed sessions/sources and newly available exports; flags space-containing filenames and text-less artifact folders (does not auto-rename). No background process, so no self-concurrency and no watch↔process race. For proactive nudges, schedule `csnotes reconcile --notify` via launchd/systemd-timer — a periodic discrete run, not a persistent daemon.

### `csnotes recover`

Explicit crash recovery (§7 step 8). Detects an orphaned workspace + `session_in_progress`, offers Resume or Discard (and restores the pre-merge snapshot first if a crash interrupted merge-back). Idempotent; says so and exits if nothing to recover.

### `csnotes audit [--reindex] [--fix]`

Vault hygiene and the manifest's safety net. Without flags: runs the invariant suite read-only and reports violations (orphan atomics, broken links/embeds, stale frontmatter, block-ID collisions, schema drift). `--reindex` rebuilds `csnotes.json` from frontmatter + filesystem (the proof that the manifest is disposable). `--fix` applies mechanical, unambiguous repairs (e.g., re-embedding an orphan into its home index). The same engine runs as the post-session invariant pass in §7.

### `csnotes config [--set key=value] [--show] [--archive COURSE] [--migrate]`

Thin wrapper over `.csnotes`. Key fields: `ai_cli` (`claude`/`agy`), `active_courses`, `skill_variant` (which instruction file the CLI copies into the workspace root), `snapshot_mode` (`pre-merge`/`shadow-git`, §10), `archive_threshold_weeks`.

**Course Status and Archive Nudge.** `status` infers course activity from recent file timestamps; if the latest raw note in a course is older than the threshold (default 8 weeks) and the course is not in `active_courses`, it surfaces a non-blocking nudge to `--archive` it. Keeps stale courses from accumulating silently without manual end-of-term bookkeeping.

---

## 9. Instruction Layer

The static instruction sources live in the vault under `_csnotes/instructions/` (tidy, out of the vault root). Before each run the CLI copies the configured variant to the **workspace root** as `CLAUDE.md` or `GEMINI.md`, where the backend auto-discovers it via working-directory-upward search — discovery works against the workspace the CLI controls, not the vault, which is what lets the AI run sandboxed (§4.5) without losing native config discovery. Model-specific files let style differences be handled at authoring time, not in CLI transformation logic. The dynamic `_session.md` supplies per-run state; the static files reference the XML tag names so the model knows the input structure.

### 9.1 The tutor / synthesizer split

The two halves of the AI's job map to two homes by one test — _does it have a verifiable output contract?_

- **Tutor (discuss, quiz, surface confusions)** has no contract; it benefits from context but needs no rigid procedure. It lives as **always-on persona + context in `CLAUDE.md`/`GEMINI.md`**, so an interactive session opens in discussion mode by default.
- **Synthesizer (write the man-page notes + emit the report)** has a contract — valid Obsidian, atomization rules, provenance, and a schema-valid `_session_report.json`. It lives as a **SKILL invoked at wrap-up** ("okay, let's write it up"), so the structured-output discipline kicks in exactly when notes are produced.

This matches the natural session arc — talk and quiz, then converge — and gives the testable seam (§11): the SKILL instructs the model to write the report before exit.

### 9.2 Directives (carried by the static files)

**Source handling.** All inputs are in the workspace, wrapped in XML tags — read from there. Per-run state is in `_session.md`. Never suggest edits to raw notes, Plaud exports, or artifacts. The manifest and all frontmatter are CLI-owned and read-only to the AI. All body writes go to `_synthetic/`. If `plaud_missing: true`, synthesize from raw notes alone and note the absence.

**Tone matching.** Before synthesizing, read the raw notes for register (formality, shorthand, characteristic phrasing) and write in that voice cleaned up — not generic textbook prose.

**Synthesis rules.** Default: extend existing topic notes. Restructure only when new material contradicts or substantially reframes. Always label each section/block with the session(s) or source(s) that informed it.

**Note structure.** `# Topic` → `## Current Understanding` (best current synthesis, with `(synthesized from: …)` where a conclusion is cross-session) → `## Conceptual History` (per-contribution framing with relationship label and a labeled reframe note).

**Fact-checking asides.** Apply light domain knowledge to flag likely transcription errors or misremembers. Format `> **[Claude/Gemini: possible misread]** …`. Never silently correct; always label; never alter the raw record; place the aside adjacent to the content. Also declare it as a `possible_misread` review flag.

**Atomization.** A concept with its own definition, syntax, and independent lookup value gets an atomic note with a block anchor; the topic index embeds it via `![[note#^block-id]]`; cross-link freely. Reuse existing block IDs surfaced in `_session.md`; never collide.

**Intellectual history.** On reframe: move prior Current Understanding to Conceptual History, update Current Understanding, add a labeled reframe note, and declare the `relationship` in the report's provenance delta.

**The report.** Before exiting, write a schema-valid `_session_report.json` (§5): one op per touched note, structural requests where warranted, provenance deltas, change summaries, and review flags. This is the only channel by which the session's work becomes durable — an unwritten or malformed report means the CLI has nothing to commit.

---

## 10. Git / Snapshots

Snapshots exist to make rollback real — in v1 the AI edited `_synthetic/` in place with nothing to revert to. Three tiers, by appetite:

- **(c) Pre-merge snapshot, no history — Phase 0 default.** The sandbox already keeps the vault untouched for the whole session (§4.5, §7), so the only window needing protection is the seconds-long merge-back. Snapshot `_synthetic/` immediately before merging, restore from it if the merge is interrupted, delete it on success. No persistent repo, zero Obsidian-LiveSync interaction. This is the minimum that closes the rollback gap.
- **(b) Shadow git over `_synthetic/` only — graduation target.** A repo tracking just the synthetic notes (bare repo outside the vault, or `.git` excluded from LiveSync). Adds real history and free line-level diff (the _semantic_ diff still comes from the AI report). LiveSync never sees it. Selected via `config snapshot_mode=shadow-git`.
- **(a) Full vault git — optional.** Most provenance value, but now git and LiveSync must be refereed (exclude `.git/` from LiveSync; exclude `.obsidian/workspace*` from git). Livable but two ignore configs that break silently on an unconfigured device. Not required to fix anything.

Recommendation: ship (c), design toward (b). Tiers (b)/(c) sidestep the git-vs-LiveSync question entirely because git never touches files LiveSync syncs. Assignment _work_ repos (§4.6) are independent of this and use ordinary git.

---

## 11. Testing Strategy

The production loop launches a live interactive model, which cannot be unit-tested for synthesis quality — and shouldn't be. Everything _around_ the model is a deterministic seam, because the model communicates only through one file (`_session_report.json`) and through note-body edits.

- **`--backend mock`** replaces the AI launch: instead of spawning `claude`, it copies a fixture report and fixture body edits into the workspace/vault, making `process` runnable end-to-end headlessly.
- **CI** feeds recorded reports + a fixture vault and asserts the §7 lifecycle: precondition handling, structural-op link rewriting, frontmatter merge/dedupe, the invariant suite, merge-back, and the recovery paths (Resume, Discard, and snapshot-restore on a simulated mid-merge crash). No live model in tests.
- **Property targets:** idempotency (apply the same report twice → identical vault), transactionality (any precondition failure → byte-identical to pre-run), and reindex fidelity (`audit --reindex` from frontmatter reproduces the committed manifest).

---

## 12. Implementation Phases

Reordered so the safety substrate the spine depends on exists from the start.

**Phase 0 — Minimum viable core + safety**

- `csnotes init` (real `CLAUDE.md`/`GEMINI.md`, empty manifest + flag store)
- `csnotes process`: build out-of-vault workspace (XML-wrapped inputs + writable `_synthetic/` copy + copied instruction file), render thin `_session.md`, launch the interactive AI sandboxed (cwd = workspace), run §7 teardown
- **Session report ingestion**: parse/validate, precondition pass, content-op indexing, frontmatter merge, the **invariant suite** (block-ID uniqueness, link resolution, referential integrity), merge-back, commit-or-discard
- Pre-merge snapshot + `csnotes recover` (Resume/Discard)
- `csnotes status` (sessions only); `csnotes audit` (read-only invariant run) and `audit --reindex`
- `--backend mock` and the CI harness
- _Exit:_ a real interactive synthesis session commits via a valid report; a malformed report blocks teardown without losing state; a simulated crash recovers to a clean vault; `audit --reindex` reproduces the manifest.

**Phase 1 — Topic tracking, sources, markdown infra**

- `comrak` for heading structure, frontmatter, wiki-link extraction; block-anchor extraction via a regex pass over raw lines (Obsidian `^block-id` is not CommonMark and is not an AST node in `comrak`/`pulldown-cmark` — build a thin wrapper).
- Topic entries in the manifest, re-derived from frontmatter; richer `_session.md` (topics, coverage, pending, existing block IDs, open flags)
- `sources/` ingestion (`process --source`), heading-scheme derivation, `{path,label,raw}` locations
- `rename_topic` structural op (pulled early)
- `csnotes diff`; `csnotes flags`
- _Exit:_ manifest tracks session/source ↔ topic relationships derived from frontmatter; diff and flags are useful review surfaces; locations are queryable.

**Phase 2 — Extraction, reconciliation, artifacts**

- `csnotes extract`; `csnotes reconcile` (discrete, on-invocation; optional scheduled `--notify`); no-Plaud-export prompt; `artifacts/` ingestion and text-less-artifact flagging
- _Exit:_ passive manifest maintenance without a daemon; extraction is usable; missing inputs surface gracefully.

**Phase 3 — Crash-safety hardening**

- Full transactional teardown property tests; idempotency and reindex-fidelity properties; resume/discard and mid-merge snapshot-restore edge cases; idempotent teardown
- _Exit:_ an interrupted session never corrupts the manifest, never orphans a blocking workspace, and never partially applies a report.

**Phase 4 — Structural ops, polish, extension**

- Remaining structural ops (`move_atomic`, `promote_atomic`, `demote_topic`, `merge_topics`, `split_topic`, `set_embed`) with vault-wide link rewriting; shadow-git snapshot mode
- `config --archive` and stale-course nudges; `--dry-run`; cross-course reframe-detection heuristics; orphan detection in `status`; `audit --fix`
- _Exit:_ the knowledge graph can be refactored safely and reversibly; vault hygiene is automatable.

---

## 13. Design Decisions and Rationale

**Why the declare/execute spine.** The risk in this system is not the CLI (the compiler reviews that) but the AI, which writes free-form content and supplies semantic metadata. Splitting content (AI, in place) from structure (AI declares, CLI executes) puts each actor where it is reliable, makes the manifest's semantic fields have a real provenance, and turns refactoring and crash-safety into one consistent pattern (the AI proposes structured ops; the CLI applies them transactionally).

**Why frontmatter is canonical and CLI-written.** Provenance belongs with the note it describes (self-describing, survives manifest deletion, greppable). Making the CLI its sole writer gives single-writer discipline and removes YAML-authoring burden from the AI; the AI declares deltas, the CLI merges. The manifest then becomes a disposable index, eliminating the v1 tension where the CLI "owned" data only the AI could produce.

**Why Rust.** Type-level enforcement of manifest invariants and explicit state transitions matters most for the crash-recovery logic, where a permissive language could leave the vault corrupted. The portfolio framing is AI-directed development: the developer defines types, data structures, and invariants; the AI implements bodies; the compiler is the review surface. Rust makes that division tractable. (Note the complement: the AI's _output_ is validated at runtime by the audit pass, since the type system cannot constrain free-form markdown — which is why the audit is Phase 0, not "future.")

**Why the sandbox + merge-back (not in-place edits).** Running the AI with the vault as its working directory would defeat raw-note immutability — the read-only input copies are pointless if the originals sit in cwd — and would let hour-long sessions stream half-written synthetic notes to other devices via LiveSync. Running the AI entirely inside the out-of-vault workspace makes immutability real (the AI can reach only what the CLI placed there), keeps the vault pristine until an atomic merge-back, and shrinks the only crash-exposed window from the whole session to the seconds-long merge. The cost — losing vault-root config auto-discovery — is paid back by the CLI copying the instruction file into the workspace root, which is also why those files get a tidy `_csnotes/` home instead of cluttering the vault root.

**Why snapshots / git.** Under the sandbox the vault is untouched until merge-back, so most of the session needs no snapshot at all; a pre-merge snapshot of `_synthetic/` guards only the write window, and git is just the most boring way to persist it. Pre-merge snapshots close the gap with zero LiveSync interaction; shadow-git adds history when wanted.

**Why an out-of-vault workspace.** Temporary XML-wrapped copies inside the vault pollute Obsidian's graph and get replicated by LiveSync every session. Outside the vault, both problems vanish structurally rather than by fragile per-device ignore configs, an orphaned workspace self-clears from `tmpfs`, and — as above — it is the precondition for the sandbox model.

**Why persistent, tiered review flags.** "Never silently correct, always surface" is only real if the surface persists; a per-run flag barely surfaces across a multi-year program. Persistence also enables resolution-that-changes-data and re-injection of open threads into the next briefing, closing the loop between the tutoring and synthesis halves. Tiering keeps the queue from becoming noise.

**Why the tutor/synthesizer split.** The two halves divide cleanly on "is there a verifiable output contract." Tutoring has none → always-on context. Synthesis has one (Obsidian validity + the report schema) → an invoked SKILL at wrap-up. This also yields the deterministic test seam.

**Why topic-primary, intellectual-history-visible, synthetic/archival split, man-page granularity.** (Unchanged from v1.) Organizing by course fragments related knowledge; topic-primary lets understanding deepen and reframe across years with course identity preserved as metadata. Visible history makes the notes a record of _learning_, not just current state. The raw layer is the unmodified archival voice; the synthetic layer is the reference tool in the user's register — neither does double duty. Man-page granularity is the right unit for a cross-linked, embeddable reference returned to repeatedly.

**Why configurable filename format, year-less default.** Date components are known to the CLI (it parses them to build the manifest), so templating filenames is the inverse of an operation it already performs. The year-less default is deliberate: course identity is the salient differentiator and courses do not span years; the full ISO date lives in the manifest and frontmatter, so internal correctness is unaffected by what the filename shows.

**Why XML input wrapping, model-agnostic context.** Both backends parse XML-tagged content with structural accuracy, preventing instruction-data confusion without per-model branching in the CLI; the tag names are part of the interface contract and appear in the static instruction files. Keeping the session-context format model-agnostic preserves backend comparison experiments; model-specific instruction _files_ (not context transformation) absorb style differences.
