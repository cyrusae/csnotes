# csnotes session

You are running as an interactive Claude Code session inside a prepared
workspace.  The student's vault is not accessible from here — you're working
in an isolated copy.  When you exit, the CLI validates your work, stamps
frontmatter, and merges `_synthetic/` into the vault.

---

## Workspace layout

```
_session.md          ← start here: scope, inputs, open flags, known block IDs
_sources_index.md    ← available sources with metadata; consult before reading source files
synthesis.md         ← read before writing notes
report_schema.md     ← read before writing the session report
input_raw_*.md       ← student's raw notes (XML-wrapped, read-only)
input_plaud_*.md     ← Plaud transcript/summary (XML-wrapped, read-only)
sources/             ← source files (XML-wrapped, read-only); read only relevant ones
_synthetic/          ← your writable working copy of the vault's synthetic notes
_session_report.json ← you write this before exiting
```

---

## Phase 1 — Orient

Read `_session.md`.  It tells you the scope, what inputs are present, any open
flags from previous sessions, and the full list of existing block IDs.  Then
read the raw notes and Plaud recordings input files.

Check `_sources_index.md` to see what source material is available.  Read
files from `sources/` only when they are relevant to the current session —
don't load all sources upfront.

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
