/// `csnotes report-schema` — print the authoritative `_session_report.json`
/// schema as Markdown, derived from the actual Rust serde types.
///
/// The output is what `csnotes init` writes as `report_schema.md`.  Run this
/// from inside a workspace when you want to check the current schema without
/// leaving the session.
use anyhow::Result;

use crate::frontmatter::NoteKind;
use crate::manifest::Relationship;
use crate::report::{FlagKind, ScopeKind, REPORT_SCHEMA_VERSION};

pub fn run() -> Result<()> {
    print!("{}", generate());
    Ok(())
}

/// Generate the `report_schema.md` Markdown string from live Rust types.
///
/// Called both by `csnotes report-schema` (prints to stdout) and by
/// `csnotes init` / `csnotes init --instructions-only` (writes the file).
pub fn generate() -> String {
    let rel_values = enum_values(&[
        Relationship::Introduced,
        Relationship::Extended,
        Relationship::Reframed,
        Relationship::Contradicted,
        Relationship::Nuanced,
    ]);

    let note_kind_values = enum_values(&[NoteKind::Atomic, NoteKind::Index, NoteKind::Journal]);

    let scope_kind_values = enum_values(&[
        ScopeKind::Session,
        ScopeKind::Source,
        ScopeKind::Topic,
        ScopeKind::Mixed,
    ]);

    let flag_kind_values = enum_values(&[
        FlagKind::PossibleMisread,
        FlagKind::NeedsConfirmation,
        FlagKind::UnresolvedQuestion,
        FlagKind::Ambiguity,
    ]);

    // Op tag values are derived from the serde `#[serde(tag = "op")]` attribute.
    let indexing_ops = r#""create_note", "update_note""#;
    let structural_ops = r#""rename_topic", "rename_atomic", "move_atomic", "promote_atomic", "demote_topic", "merge_topics", "split_topic", "set_embed""#;

    format!(
        r##"# Session report schema

> **Auto-generated** from Rust types — run `csnotes report-schema` for the
> current version.  Enum values below are derived directly from serde and can
> be trusted even if an older copy of this file is present.

Write `_session_report.json` using the schema below.  This is a metadata
manifest — do NOT include note bodies.  The CLI reads the files you wrote
directly; the report tells it what you did and why.

---

## Top-level structure

```json
{{
  "csnotes_report_schema": {schema_version},
  "run_id": "<copy from _session.md exactly>",
  "backend": "<claude | gemini>",
  "started_at": "<ISO 8601 UTC>",
  "completed_at": "<ISO 8601 UTC>",
  "scope": {{
    "kind": "session",
    "sessions": ["<session-id>"],
    "sources": []
  }},
  "operations": [ ... ],
  "review_flags": [ ... ]
}}
```

**`run_id`** must be copied verbatim from `_session.md`.  A mismatch causes
the CLI to discard the workspace.

**`backend`** — use `"claude"` for Claude Code sessions, `"gemini"` for
Gemini/Agy sessions.

**`scope.kind`** — one of: {scope_kind_values}

---

## `create_note` operation

One entry for every note file you created.

```json
{{
  "op": "create_note",
  "kind": "atomic",
  "path": "_synthetic/algorithm-analysis/red-black-trees.md",
  "title": "Red-Black Trees",
  "topic": "algorithm-analysis",
  "block_id": "red-black-trees",
  "embed_in": ["_synthetic/algorithm-analysis/algorithm-analysis.md"],
  "provenance": {{
    "sessions": [
      {{
        "course": "CPSC5001",
        "date": "YYYY-MM-DD",
        "relationship": "introduced"
      }}
    ],
    "sources": []
  }},
  "change_summary": "One sentence: what this note captures."
}}
```

| Field | Notes |
|---|---|
| `kind` | {note_kind_values} |
| `path` | workspace-relative; matches the file you wrote |
| `block_id` | required for atomic; omit for index |
| `embed_in` | index notes this atomic should appear in; `[]` if none |
| `relationship` | {rel_values} |

**`provenance.sources` is auto-harvested.**  The CLI scans the note body for
`[[wikilinks]]` that resolve to registered source files and adds them to
`contributing_sources` automatically (relationship: `introduced`).  You only
need to list a source explicitly if you want a non-`introduced` relationship
(e.g. `extended`, `nuanced`).  Leave `"sources": []` in all other cases.

---

## `update_note` operation

One entry for every existing note you edited.

```json
{{
  "op": "update_note",
  "path": "_synthetic/algorithm-analysis/sorting.md",
  "add_provenance": {{
    "sessions": [
      {{
        "course": "CPSC5001",
        "date": "YYYY-MM-DD",
        "relationship": "extended"
      }}
    ],
    "sources": []
  }},
  "sections": ["Comparison with heapsort"],
  "change_summary": "One sentence: what changed and why."
}}
```

`sections` is an informal list of headings or concepts you touched — used by
`csnotes diff`, informational only.

---

## Review flags

```json
{{
  "kind": "possible_misread",
  "path": "_synthetic/algorithm-analysis/red-black-trees.md",
  "message": "Raw notes say 'n log n' but rotation analysis is O(log n) — kept O(log n), please confirm."
}}
```

| Kind | Behaviour |
|---|---|
| `"possible_misread"` | Persists; shown in `csnotes status` until resolved |
| `"needs_confirmation"` | Persists; shown in `csnotes status` until resolved |
| `"unresolved_question"` | Surfaces in future session briefings; doesn't nag |
| `"ambiguity"` | Logged only; auto-resolved |

Valid `kind` values: {flag_kind_values}

Flags do not block the commit.

---

## Structural ops

These ops mutate the vault's topic/note layout.  Include them in `operations`
alongside `create_note` / `update_note`.

Indexing ops: {indexing_ops}
Structural ops: {structural_ops}

### `rename_topic`

```json
{{ "op": "rename_topic", "from": "old-topic", "to": "new-topic",
  "reason": "clearer name" }}
```

### `rename_atomic`

Rename a note's slug and title without changing its topic.

```json
{{ "op": "rename_atomic",
  "path": "_synthetic/algorithms/sort.md",
  "new_slug": "comparison-sorting",
  "new_title": "Comparison Sorting",
  "reason": "old name was too terse" }}
```

### `move_atomic`

Move a note into an **existing** topic folder.

```json
{{ "op": "move_atomic",
  "from_path": "_synthetic/algorithms/sorting.md",
  "to_topic": "data-structures",
  "reason": "better conceptual fit" }}
```

### `promote_atomic`

Move a note into a **new** topic folder (you must also `create_note` the index).

```json
{{ "op": "promote_atomic",
  "from_path": "_synthetic/algorithms/red-black-trees.md",
  "to_topic": "balanced-trees",
  "reason": "topic large enough to stand alone" }}
```

### `demote_topic`

Fold all notes from one topic into an **existing** target topic.

```json
{{ "op": "demote_topic", "from_topic": "graphs",
  "into_topic": "algorithms", "reason": "too small to stand alone" }}
```

### `merge_topics`

Fold multiple source topics into one target (created if absent).  `into` may be
one of the `from` entries (keep it, absorb the others).

```json
{{ "op": "merge_topics",
  "from": ["red-black-trees", "avl-trees"],
  "into": "balanced-trees",
  "reason": "unified concept" }}
```

### `split_topic`

Distribute atomics from one topic into several targets.  Notes not listed in
any target remain in `from`.  New topic folders are created automatically.

```json
{{ "op": "split_topic", "from": "algorithms",
  "into": [
    {{ "topic": "sorting",   "atomics": ["quicksort", "mergesort"] }},
    {{ "topic": "searching", "atomics": ["binary-search"] }}
  ],
  "reason": "topic grew too broad" }}
```

### `set_embed`

Add or remove a transclusion link from an index note.

```json
{{ "op": "set_embed",
  "atomic_path": "_synthetic/algorithms/mergesort.md",
  "index_path":  "_synthetic/algorithms/algorithms.md",
  "present": true }}
```

`present: false` removes the embed.  Both operations are idempotent.
"##,
        schema_version = REPORT_SCHEMA_VERSION,
        rel_values = rel_values,
        note_kind_values = note_kind_values,
        scope_kind_values = scope_kind_values,
        flag_kind_values = flag_kind_values,
        indexing_ops = indexing_ops,
        structural_ops = structural_ops,
    )
}

fn enum_values<T: serde::Serialize>(variants: &[T]) -> String {
    variants
        .iter()
        .map(|v| {
            let s = serde_json::to_string(v).unwrap_or_default();
            // serde serializes unit enum variants as JSON strings like `"introduced"`
            s
        })
        .collect::<Vec<_>>()
        .join(" / ")
}
