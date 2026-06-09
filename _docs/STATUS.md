# csnotes — Build Status

> Last updated: 2026-06-09
> All 58 tests passing (54 unit + 3 lifecycle + 1 property placeholder).

---

## Summary

The CLI is **feature-complete for Phase 0 through Phase 2, plus all Phase 1 items**. Every command that isn't explicitly gated on a future phase is functional. The vault is not yet bootstrapped with real data; first real use is the next milestone.

---

## Command Status

| Command | Status | Notes |
|---------|--------|-------|
| `init` | ✅ Full | Scaffolds dirs, writes `.csnotes`, creates `csnotes.json`. `--instructions-only` refreshes instruction files in an existing vault without touching structure. Embedded real content for `claude.md`, `gemini.md`, `synthesis.md`, `report_schema.md`. |
| `reconcile` | ✅ Full | Course-root pattern (`active_courses`) wired. Scans `{course}/{raw_dir}/` and `{course}/{plaud_dir}/` per course. `Manifest::load_or_create` bootstraps on first run. `--rename-spaces` renames files with spaces. **Phase 1:** scans `sources_dir/` (flat and one-level subdirs), derives heading schemes via comrak, registers `SourceEntry` records. |
| `process` | ✅ Full (Phase 0+1) | Full §7 teardown pipeline. `backend`/`skill_variant` stored in `InProgressRecord`. Per-session reports saved to `_generated/reports/<session-id>.json`. `--backend` flag overrides config. **Phase 1:** `rename_topic` structural op fully executed (folder rename + frontmatter update + link rewrite). Source status updated in teardown. `topics_updated` populated on both session and source entries after every teardown. |
| `recover` | ✅ Full | Three-path logic: workspace gone → clear; report present → teardown; report absent + `--resume` → re-launch AI using stored `backend`/`skill_variant`. `--discard` cleans up without asking. |
| `status` | ✅ Full (Phase 0) | Sessions, sources, topics, flags, in-progress warning. |
| `diff` | ✅ Full | `--session` resolves partial IDs (e.g. `09-03` matches `CPSC5001-09-03`). Reads per-session reports from `_generated/reports/`. Shows creates, updates, structural ops, actionable flags, open questions. |
| `flags list` | ✅ Full | `--all` includes threads and changelog. Fixed: `FlagStore::load` no longer hard-errors on missing file. |
| `flags resolve` | ✅ Full | Marks resolved, records `follow_up`. |
| `flags show` | ✅ Full | Full flag detail. |
| `extract` | ✅ Full | Detects actions (`- [ ]`, `TODO:`, `ACTION:`), deadlines (due/deadline/submit + `by <weekday/month>`), questions (`?`, `Q:`, `??`). `--type` filter. `--stdout` or writes to `_generated/extracts/<session-id>.md`. 6 unit tests. |
| `audit` | ✅ Full | Read-only invariant check. `--reindex` rebuilds manifest from frontmatter. `--fix` shows repair plan (dry-run); `--fix --apply` executes. Fixes: `block_id` declared in frontmatter but `^id` anchor absent from body. **Phase 1:** `rename_topic` precondition check (source folder exists, dest doesn't). `pending_sessions` computed during reindex. |
| `config --show` | ✅ Full | |
| `config --set` | ✅ Full | Keys: `filename_format`, `raw_dir`, `plaud_dir`, `artifacts_dir`, `sources_dir`, `default_backend`, `archive_threshold_weeks`. |
| `config --add-course` | ✅ Full | Deduplicates. |
| `config --archive` | ✅ Full | Errors if course not found. |
| `config --migrate` | ✅ Full | Shows rename plan (dry-run); `--migrate --apply` executes. Pre-flight enforces reconcile-before-migrate. Renames raw notes, plaud exports, and artifacts. Updates manifest session keys and all path fields. |

---

## Phase 1 — Completed

| Feature | Implementation |
|---------|---------------|
| `comrak` heading parsing | `markdown.rs` — `parse_headings` + `derive_heading_scheme`. Levels collapsed by rank, not absolute depth. 6 unit tests. |
| Source registration in `reconcile` | `scan_sources_dir` walks `sources_dir/` (max depth 2). Source IDs: `{stem}` or `{subdir}/{stem}`. Heading scheme derived on registration. |
| `topics_updated` tracking | Populated during teardown from op list (`create_note` → `op.topic`; `update_note` → first non-synthetic path component; `rename_topic` → both `from` and `to`). |
| `pending_sessions` | Computed during `reindex`: sessions whose `processed_at > topic.last_updated` AND `topics_updated` contains the topic. Catches genuine teardown/reindex desync. |
| Richer `_session.md` | Block IDs listed per-topic (not flat). Pending sessions shown inline per topic. Topic-scoped open flags shown per-topic. Separate "Resolved Follow-ups" section injects `follow_up` text from resolved flags. Vault-wide unscoped flags section at end. |
| `rename_topic` structural op | `execute_rename_topic`: rename folder → update `topic` frontmatter in all notes → rewrite path-qualified wikilinks. Precondition checks in `audit::precondition_pass`. 4 unit tests. |
| Source teardown | `update_source_status` mirrors `update_session_status`: sets `Processed`, stamps `last_processed_at`, writes `topics_updated`. |
| `follow_up` re-injection | `FlagStore::resolved_with_follow_up()` surfaces resolved flags with notes; injected as "Resolved Follow-ups" section in briefing. |

---

## Architecture Deviations from PLAN.md

These are intentional decisions made during implementation; PLAN.md has not been retroactively updated.

| Plan | Actual | Rationale |
|------|--------|-----------|
| `InProgressRecord` had no backend/skill fields | Added `backend: Option<AiBackend>` and `skill_variant: Option<SkillVariant>` | `recover --resume` needs them to re-launch the correct backend |
| `skill_variant` read from config at launch | Derived from `backend_kind` at launch; config value only used as fallback for mock | Prevents `--backend agy` from using CLAUDE.md when config has `skill_variant = "claude"` |
| `_session.md` did not include `run_id` | `run_id` added to scope section | `report_schema.md` instructs the AI to copy it into the report; it wasn't reachable otherwise |
| Single instruction file (`claude.md` / `gemini.md`) | Three files: `claude.md` (workflow phases), `synthesis.md` (note-writing philosophy), `report_schema.md` (JSON schema reference) | Phased reading model — AI reads each reference just-in-time rather than holding all context upfront |
| `--system-prompt` used to load CLAUDE.md | `--system-prompt "."` blocks global CLAUDE.md; workspace CLAUDE.md is auto-discovered | Prevents double-loading if user has a global `~/.claude/CLAUDE.md` |
| `audit --fix` was Phase 4 stub | Implemented | Only mechanical repair currently available: missing `^id` body anchor |
| `config --migrate` was Phase 4 stub | Implemented | Dry-run by default, `--apply` to execute; pre-flight check for unregistered files |
| Report `session_id` in `scope.sessions` list | Working as designed | Allows multi-session reports; `diff --session` copies all declared session IDs during teardown |
| `pending_sessions` = "unprocessed sessions touching this topic" | Implemented as: processed sessions whose `processed_at > topic.last_updated` AND `topics_updated` contains this topic | Can't know which unprocessed sessions will touch a topic; this catches genuine teardown desync instead |
| Flat block ID list in `_session.md` | Per-topic block IDs, inline in the notes section | AI gets spatial context (which IDs belong to which topic) rather than a vault-wide dump |

---

## What's Not Implemented

### Phase 3

- `proptest` idempotency and transactionality suites (placeholder test exists)
- Mid-merge crash simulation tests

### Phase 4 (beyond the three commands already done)

- Full structural op suite: `move_atomic`, `promote_atomic`, `demote_topic`, `merge_topics`, `split_topic`, `set_embed`
- Shadow-git snapshot mode (`snapshot_mode = shadow_git`)
- `cross_embedded_in` rebuild during merge-back (field exists in frontmatter, rebuild step is stubbed)

### Agy (Antigravity) backend

- `AgyBackend` is stubbed (`todo!()`). Launch signature is `agy -i "Read GEMINI.md..." --add-dir <workspace>`.

---

## Instruction Files

Written and embedded in `init.rs` as raw string constants. Installed to `_csnotes/instructions/` by `csnotes init` or `csnotes init --instructions-only`.

| File | Purpose |
|------|---------|
| `claude.md` | Workspace entry point. Four phases: Orient (read `_session.md`), Debrief (quiz before writing), Write notes (read `synthesis.md` first), Write report (read `report_schema.md` first). Exit checklist. |
| `gemini.md` | Equivalent for Agy/Gemini backend. |
| `synthesis.md` | Note-writing philosophy: voice, atomization thresholds, index note structure, wikilinks, uncertainty handling, textbook vs. lecture synthesis. |
| `report_schema.md` | JSON schema reference: `create_note`, `update_note`, `rename_topic` op fields, review flag kinds. Read immediately before writing the report. |

---

## Next Steps (Suggested Order)

1. **Bootstrap the real vault.** Write `.csnotes` for the CPSC5001/CPSC5002/CPSC5005 layout, run `csnotes init --instructions-only`, run `csnotes reconcile`. Validate manifest looks correct including source registration.
2. **First real session.** Run `csnotes process` against a CPSC5001 session. This is the first live test of the instruction files and the full teardown pipeline.
3. **Phase 3 tests.** `proptest` idempotency suite — best done now that Phase 1 structural ops exist and the rename path is real.
4. **Agy backend.** Implement `AgyBackend` for Gemini/Antigravity sessions.
5. **Phase 4 structural ops** if real sessions reveal need: `move_atomic`, `promote_atomic`, `demote_topic`, `merge_topics`, `split_topic`, `set_embed`.
