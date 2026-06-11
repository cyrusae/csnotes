# csnotes — Build Status

> Last updated: 2026-06-10
> All 106 tests passing (97 unit + 4 lifecycle + 5 property).

---

## Summary

The CLI is **feature-complete for Phase 0 through Phase 4** (all items). Every planned command and structural op is implemented. The vault is not yet bootstrapped with real data; first real use is the next milestone.

---

## Command Status

| Command | Status | Notes |
|---------|--------|-------|
| `init` | ✅ Full | Scaffolds dirs, writes `.csnotes`, creates `csnotes.json`. `--instructions-only` refreshes instruction files in an existing vault without touching structure. Embedded real content for `claude.md`, `gemini.md`, `synthesis.md`, `report_schema.md`. |
| `reconcile` | ✅ Full | Course-root pattern (`active_courses`) wired. Scans `{course}/{raw_dir}/` and `{course}/{plaud_dir}/` per course. `Manifest::load_or_create` bootstraps on first run. `--rename-spaces` renames files with spaces. **Phase 1:** scans `sources_dir/` (flat and one-level subdirs), derives heading schemes via comrak, registers `SourceEntry` records. **Phase 2:** scans `{course}/{artifacts_dir}/` for text-readable files, matches by session-ID filename prefix, classifies as Slides/Code/Other. Desktop notifications: macOS (osascript) + Linux (notify-send). |
| `process` | ✅ Full | Full §7 teardown pipeline. `backend`/`skill_variant` stored in `InProgressRecord`. Per-session reports saved to `_generated/reports/<session-id>.json`. `--backend` flag overrides config. **Phase 1:** `rename_topic` structural op fully executed; source status updated; `topics_updated` populated. **Phase 2:** auto-reconcile runs silently before every `process` invocation — new sessions/artifacts/sources are picked up automatically. |
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

### Phase 3 — Completed

| Test | Coverage |
|------|----------|
| `reconcile_is_idempotent` | proptest (32 cases): 1–5 random (course, month, day) triples → reconcile twice → byte-identical manifest |
| `reconcile_registers_all_raw_notes` | proptest (32 cases): 1–5 random session files → reconcile once → all appear in manifest, exact count match |
| `reconcile_source_registration_is_idempotent` | proptest (32 cases): 1–3 random source stems → reconcile twice → sources section byte-identical |
| `recover_discard_clears_stale_in_progress` | lifecycle: dangling `session_in_progress` (workspace gone) → `recover --discard` → `session_in_progress` null |
| `reindex_is_stable_after_clean_process` | lifecycle: `process` → `audit --reindex` → sessions/topics unchanged, processed status preserved |

### Phase 4 — Completed

| Op | Implementation |
|----|----------------|
| `move_atomic` | Moves note to existing topic; updates frontmatter + precise wikilink rewrite (terminator-checked, no false prefix matches) |
| `promote_atomic` | Creates new topic folder, moves note; same frontmatter/link updates |
| `demote_topic` | Folds all notes from source into existing target; conflict-checked, source folder deleted |
| `merge_topics` | Folds N source topics into one target (created if absent); per-source conflict check |
| `split_topic` | Distributes listed atomics to new targets; unlisted notes remain in source; source removed if empty |
| `set_embed` | Adds/removes `![[slug#^block-id]]` from index body and keeps `embeds` frontmatter in sync; idempotent |

All six ops wired into the `process` teardown pipeline (step 5).  16 unit tests added (7 op-level, 2 for precise `replace_note_links`).  `report_schema.md` updated with JSON examples for all ops.

| Feature | Implementation |
|---------|---------------|
| `cross_embedded_in` rebuild | `workspace::rebuild_cross_embedded_in` walks the synthetic dir after every `merge_back`. Builds an embedder map from all `![[stem#^id]]` links in index notes, then updates only atomic notes whose `cross_embedded_in` differs. 4 unit tests. |
| `process --dry-run` scope output | Prints resolved scope (session ID + raw note / plaud count / artifact count, or source path/kind, or topic atomic count), backend, and workspace path before exiting without launching AI. |

**Known limitations:**
- **LiveSync + merge-back window**: If Obsidian LiveSync is active during `process`, it may sync `_synthetic/` mid-merge (between the snapshot and the final cleanup). The window is short (seconds), but a sync at that moment could push a partially-merged tree. Mitigation: pause LiveSync before running `process`, or accept the risk given the snapshot makes recovery straightforward.

### Security & Quality — Completed (from CSN feedback)

| Item | Implementation |
|------|---------------|
| Path traversal (CSN-001) | `src/pathutil.rs` — `safe_join(root, unsafe_path)` rejects absolute paths, `..` components, and NUL bytes. Applied at every AI-produced path join in `content.rs`, `structural.rs`, and `audit.rs`. 6 unit tests. |
| CRLF normalization (CSN-004) | `frontmatter::read_note(path)` normalizes `\r\n` → `\n` on every disk read. Replaces all `std::fs::read_to_string` calls on `.md` files across `frontmatter.rs`, `content.rs`, `structural.rs`, `audit.rs`, `workspace.rs`, `obsidian.rs`. 3 unit tests. |
| AppleScript injection (CSN-002) | `reconcile::notify` now passes the message as argv to `osascript` instead of interpolating it into the script string. |
| Proptest for markdown parsers (CSN test rec.) | 4 proptest cases in `obsidian.rs`: `extract_block_ids`, `extract_wikilinks`, `extract_embeds` never panic on arbitrary input; returned values are consistent with the input string. |
| Concurrent locking (CSN-003) | Tracked as backlog (L5). `session_in_progress` guard already covers the worst case; `reconcile` is idempotent per proptest. |

### Agy (Antigravity) backend — Completed

| Feature | Implementation |
|---------|---------------|
| `AgyBackend::launch` | `agy [--model <model>] -i "Read GEMINI.md…" --add-dir <workspace>`. Blocks until session exits. |
| Model selection | `agy_model` config key + `--agy-model` per-run CLI override. When absent, `agy` uses its default (gemini-2.5-pro). `config --set agy_model=<model>` persists it. |
| `GEMINI.md` instruction file | Full standalone file: workspace layout, four phases (Orient → Debrief → Write notes → Write report), format rules, exit checklist. No longer a redirect to `claude.md`. |
| `report_schema.md` | `"backend"` field now documents both `"claude"` and `"gemini"` values. Template placeholder updated from hardcoded `"claude"`. |
| `recover --resume` | Passes `config.agy_model` to `make_backend` so re-launched Agy sessions use the correct model. |

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

1. **Bootstrap the real vault.** Write `.csnotes` for the CPSC5001/CPSC5002/CPSC5005 layout, run `csnotes init --instructions-only`, run `csnotes reconcile`. Validate manifest looks correct including source and artifact registration.
2. **First real session.** Run `csnotes process` against a CPSC5001 session. This is the first live test of the instruction files and the full teardown pipeline.
3. **Bootstrap and first real session** — everything is implemented; real use is the remaining validation step.
