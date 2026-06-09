# Session report schema

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
