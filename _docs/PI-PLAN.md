# Pi backend — plan

> Drafted 2026-09-29. Status: proposal, nothing implemented.
> Pi = the `earendil-works/pi` coding-agent harness (formerly `badlogic/pi-mono`),
> npm `@earendil-works/pi-coding-agent`, docs at https://pi.dev/docs/latest.

---

## Goals

1. **Pi becomes the default launched interface** for `process` / `recover --resume`,
   with in-session model switching (`/model`, Ctrl+P over a scoped list).
2. **Lean context.** The tutor sees only csnotes instructions and csnotes tools — no
   coding-agent preamble, no stray tool descriptions, no user-level connectors/MCP.
3. **Maximally restrictive tool surface.** No bash. Writes confined to the paths the
   CLI owns the merge-back for. `check` / `commit` / `report-schema` become tools.
4. **A panel, not a single voice.** Claude via the existing OAuth bridge plus
   OpenRouter models for cheap helper roles and cross-family review.
5. Workflow upgrades that Pi makes cheap: session agenda HUD, style review pass,
   scout subagents over prior material.

## Decisions already made

| Decision | Choice |
|---|---|
| Claude Code backend | **Kept** as the original/fallback. Untouched except where shared code changes. |
| Agy backend | **Retired**, superseded by Pi. (Pi itself dropped its Antigravity provider in 0.71.0, so there's no path to keep it via Pi either.) |
| Codex | **No dedicated backend.** Pi ships OpenAI ChatGPT Plus/Pro OAuth and OpenRouter covers GPT models; Codex-family models arrive for free if ever wanted. |
| OS-level sandbox | **Not now.** Tool-level lockdown instead (see Security). Revisit only if bash ever comes back. |
| MCP | Not supported, not wanted. |
| Where the TS lives | In this repo under `pi/`, embedded into the binary with `include_str!` and written into each workspace — same pattern as the instruction files. No npm publish. |

## Non-goals

- Building a custom app on Pi's SDK/RPC mode. The CLI + project `.pi/` dir covers
  everything here with far less exposure to API churn (most breaking changes land
  in SDK/session/provider internals — see "Living with Pi's churn").
- Changing the teardown pipeline. The contract stays: `_synthetic/` edits +
  `_session_report.json`, validated and merged by Rust.

---

## Architecture

```text
csnotes process
  └─ assemble workspace  (unchanged)
       + write .pi/SYSTEM.md          ← instruction file for the scope, Pi-flavoured
       + write .pi/settings.json      ← tool allowlist, scoped models, quiet startup
       + write .pi/extensions/csnotes.ts
       + write .pi/agenda.json        ← seeded from the same data as _session.md
  └─ PiBackend::launch
       pi --tools <allowlist> -nc [--model M] [--models "<scope>"] [-c] -e <ext>
  └─ teardown (unchanged)
```

### What goes where

| Concern | Mechanism | Why there |
|---|---|---|
| No bash, fixed tool set | `--tools` allowlist on the command line (built-ins **and** extension tools by name, since 0.68) | Enforced by Pi core, not by our extension. If the extension fails to load, we **lose capabilities, not restrictions**. |
| Path confinement for `edit` / `write` | `tool_call` handler in `csnotes.ts`, returns `{block, reason}` | Handlers that throw fail closed. |
| Path confinement for `read` / `grep` / `find` / `ls` | Same handler, confined to the workspace root | Stops a prompt-injected source file from steering the model into `~/.pi/agent/auth.json` or `~/.ssh`. |
| `csnotes check`, `csnotes commit`, `csnotes report-schema` | `registerTool`s whose `execute` shells out to fixed argv (no model-controlled strings) | Replaces "please run this in bash". |
| No context-file leakage | `-nc` (skip AGENTS.md/CLAUDE.md discovery up the parent chain) + everything in `SYSTEM.md` | Workspace lives under a temp dir whose parents we don't control. |
| No global-extension leakage | TBD — see spike checklist (agent-dir override, or `settings.json` resource filtering) | Otherwise anything in `~/.pi/agent/extensions/` rides along, recreating the "arXiv connector" problem. |

The Python `csnotes_guard.py` hook stays for the Claude backend only.

### Rust changes

- `config.rs`
  - `AiBackend::Pi` added. `AiBackend::Agy` removed; keep deserialising
    `"agy"` → error pointing at `pi`, so old `csnotes.toml` files get a readable
    message rather than a serde failure.
  - `SkillVariant::Gemini` replaced by `SkillVariant::Pi`. The review variants
    (`CourseReview`, `ResourceReview`) keep their own files; see "Instructions"
    for how the tool-vs-shell wording differences are handled.
  - New keys: `pi_model` (default model), `pi_models` (Ctrl+P scope string),
    `pi_version` (expected version, see churn section).
    `agy_model` removed. `--agy-model` → `--model` (a generic per-run override; the
    Claude backend's `claude_model` can fold into it too).
- `backend.rs`: `PiBackend` next to `ClaudeBackend`. Same `run_interactive` wrapper
  (SIGINT handling applies equally). `AgyBackend` deleted.
- `workspace.rs`: `write_workspace_pi()` alongside `write_workspace_hooks()`;
  called by backend kind.
- `init.rs`: backend prompt becomes `claude/pi`; `GEMINI_MD` deleted.
- `main.rs` / `config_cmd.rs` help text and `apply_set` updated.
- New `csnotes doctor` (small): checks `pi --version` against `pi_version`, Node ≥ 22.19,
  and that `pi` can load the embedded extension headlessly. Run on demand and
  automatically when the installed Pi version differs from the last-seen one.

---

## Security posture

**Threat model:** the realistic attacker is text inside a lecture transcript, slide
deck, or pasted AI conversation that talks the model into doing something. csnotes
already defends the teardown against it (`safe_join`, CSN-001). The session is the
remaining surface.

With the design above, a hijacked model can:
- read files **inside the workspace** (fine — it's all copies),
- write files **inside `_synthetic/`, `_journal/`, `_session_report.json`**
  (fine — teardown validates),
- call `check` / `commit` (fine — idempotent, validated),
- talk to you.

It cannot run commands, reach the network, read outside the workspace, or write
outside the allowed paths. That covers what an OS sandbox would buy here.
Continuing a session isn't a problem either way: Pi keeps session JSONL under
`~/.pi/agent/sessions`, outside the workspace.

**Residual risks, accepted:**
- The extension itself runs with full user permissions (all Pi extensions do).
  Mitigated by it being ours, small, and embedded in the binary.
- Third-party packages (subagents, todo) run with full permissions too. Pin exact
  versions, read the source before adopting, prefer the official `examples/`
  as the base for anything small.
- `!cmd` in the Pi editor still runs bash **as you**. That's the human, not the
  model — leave it.

---

## Living with Pi's churn

Pi has shipped ~40 releases June–Sept 2026, most minor versions with a "Breaking
Changes" section. Going through the changelog since January, the large majority
land in surfaces we don't touch: SDK session internals, provider/stream types,
RPC framing, auth storage, custom-provider refresh. The ones that **would** have
hit a csnotes-shaped setup:

| Version | Change | How it would have shown up |
|---|---|---|
| 0.51.0 | `execute(toolCallId, params, signal, onUpdate, ctx)` param order swapped | Silent if you use neither `signal` nor `onUpdate`; wrong-object bugs if you do |
| 0.59.0 | Custom tools only listed in the system prompt if they set `promptSnippet` | **Silent**: tools still callable but vanish from the prompt's tool list |
| 0.65.0 | Unknown single-dash CLI flags now error | Loud: launch fails |
| 0.69.0 / 0.83.0 | TypeBox import moves / removed APIs | Loud: extension fails to load |
| 0.72.0 | `models.json` `reasoningEffortMap` → `thinkingLevelMap` | Thinking-level config silently ignored for custom model entries |
| 0.43.0 / 0.52.6 | `/branch` → `/fork`, `/exit` removed | Muscle memory |

**Damage profile:** worst plausible case is "the session launches missing tools or
with a stale prompt section, and you notice in the first minute". The tool
allowlist and path confinement don't depend on the extension loading (allowlist
is core; path guard fails closed), teardown is Rust, and session transcripts
persist, so no work is lost.

**Strategy:**
1. Pin `pi_version`. `csnotes doctor` warns on mismatch; upgrade deliberately.
2. On upgrade: skim the changelog's Breaking Changes for the six surfaces we use —
   `registerTool`, `tool_call`, `setWidget`/`ui`, `SYSTEM.md` assembly, CLI flags,
   settings keys. Then run `doctor` and one mock-scope session.
3. Keep `csnotes.ts` small and boring. Anything clever (subagents, agenda UI)
   lives in separate files so one breaking change doesn't take everything down.
4. Always set `promptSnippet` on our tools, and make `doctor` assert the tool
   names appear in the assembled system prompt (catches the 0.59-class silent break).

---

## Phases

### Phase 0 — Instructions hygiene (independent of Pi, do first)

**Why:** the repo's `_csnotes/instructions/synthesis.md` still has the old
"conversational prose" voice section. The bullets-not-prose rules exist only in
`SYNTHESIS_MD` in `init.rs`. Two copies drifted, and a vault seeded from the stale
one is instructed to write paragraphs.

- [ ] Make `_csnotes/instructions/*.md` the **single source of truth**: replace the
      `const *_MD: &str = r##"..."##` blocks in `init.rs` with `include_str!` of
      those files. Drift becomes impossible.
- [ ] Diff each embedded constant against its file before switching, and keep
      whichever is newer. Confirmed 2026-09-29: `SYNTHESIS_MD` is newer than the
      repo file (it has the voice/bullets section, the file doesn't). Check the rest.
      `COURSE_REVIEW_MD`, `RESOURCE_REVIEW_MD` and `CSNOTES_REFERENCE_MD` have no
      repo file yet, so extract them.
- [ ] Refresh the live vault: `csnotes init --instructions-only`.
- [ ] Cross-family rewrite pass: run the instructions through one or two
      non-Claude models (via OpenRouter) for tightening and voice. You pick the
      winner per file. That way the prompt's own voice isn't inherited from one
      model family.
- [ ] Add a unit test asserting `synthesis.md` contains the "Bullets, not prose" header,
      so a future edit that drops it fails CI.

### Phase 1 — Spike (manual, no Rust)

Take a preserved workspace (`process` then quit before teardown, or
`--backend mock` + copy), hand-write `.pi/`, and run Pi against it.

Must confirm before writing Rust:
- [ ] `--tools read,grep,find,ls,edit,write,csnotes_check,csnotes_commit,csnotes_report_schema`
      yields exactly that tool set, with bash absent from prompt and runtime.
- [ ] `.pi/SYSTEM.md` + `-nc` produces a prompt with nothing coding-agent-shaped
      left in it. Dump it with the `prompt-customizer` example or a `before_agent_start` log.
- [ ] **Global leakage:** how to stop `~/.pi/agent/extensions|skills|prompts` loading
      (agent-dir env override? settings resource filters?). Decide the mechanism.
- [ ] Project-trust prompt: does `-e <path>` + `.pi/` trigger it on a fresh temp dir
      every run? If so, `--approve` or equivalent.
- [ ] `write` creates parent directories (replaces the one legit `mkdir`).
- [ ] Resume: does `pi -c` scope "most recent session" by cwd? `recover --resume`
      depends on it. If not, capture the session file path at launch and pass it explicitly.
- [ ] Claude via the OAuth bridge and an OpenRouter model both work; Ctrl+P cycles
      between them mid-session without losing context.
- [ ] Path guard blocks `write ../../x`, `read ~/.ssh/...` and symlink escapes (resolve
      real paths before comparing).
- [ ] What happens when the extension throws at load: is it fatal, a warning, or silent?
      This decides how loud `doctor` needs to be.

Output: a working `.pi/` template + notes appended to this file.

### Phase 2 — `PiBackend` in Rust

- [ ] Changes listed under "Rust changes" above.
- [ ] `pi/csnotes.ts` + `pi/SYSTEM.*.md` embedded via `include_str!`.
- [ ] Instruction variants: rather than a fourth hand-maintained copy, generate the
      Pi flavour from `claude.md` with a small substitution table ("run `csnotes check`"
      → "call the `csnotes_check` tool", drop the "hooks block these commands"
      paragraph, etc.), with a test that no `csnotes <subcommand>` shell instruction
      survives in the Pi variant. Same for the review variants.
- [ ] Lifecycle test with a fake `pi` on `PATH` (a script that writes a fixture report),
      mirroring how `MockBackend` exercises teardown.
- [ ] `csnotes doctor`.
- [ ] Remove `AgyBackend`, `GEMINI_MD`, `agy_model`, `--agy-model`, `SkillVariant::Gemini`;
      update README + STATUS + CHANGELOG.
- [ ] Flip `default_backend` default to `pi` only after a few real sessions.

### Phase 3 — Style lint in `csnotes check` (Rust, no LLM)

This makes most of the "rewrite this in the terse style" requests mechanical.
It's also useful under the Claude backend.

- [ ] Per atomic note, outside the embed-intro paragraph:
  - flag paragraphs over N sentences (start at 3),
  - flag notes whose non-code prose lines outnumber bullet lines,
  - flag a configurable banned-phrase list (`csnotes.toml` `style.banned_phrases`;
    seed with "the whole ballgame", "it's worth noting", "at the end of the day", …).
- [ ] Warnings by default; a `style.strict = true` config makes them block exit like
      invariant violations.
- [ ] Report violations with file + line so the model can fix them in place.

### Phase 4 — Session agenda HUD

- [ ] CLI seeds `.pi/agenda.json` at assemble time from the data it already has for
      `_session.md`: inputs to debrief, open flags, pending topics, resolved follow-ups.
- [ ] Extension renders it with `ctx.ui.setWidget` (above the editor), starting from the
      official `todo.ts` example rather than a third-party package. It's small, and
      it avoids a pinned dependency on our most visible UI.
- [ ] `agenda_update` tool (add / check off / annotate). The model maintains it; you see it.
- [ ] Persist to the workspace so `recover --resume` restores it.
- [ ] Optional: dump the final agenda into the journal entry.
- Fallback if hand-rolling stalls: `@juicesharp/rpiv-todo` (most mature todo overlay, ~840★ monorepo).

### Phase 5 — Style reviewer (cross-family)

- [ ] Adopt `nicobailon/pi-subagents` (pinned). It's the clear leader (~3.8k★) and
      supports per-role model overrides.
- [ ] A `note-reviewer` agent: prompt = `synthesis.md` + "return concrete rewrite
      suggestions, do not edit"; model = a **different family** from the writer,
      cheap tier, via OpenRouter. A different family won't share the writer's tics.
- [ ] Triggers: `/review [path]` on demand, and run automatically inside
      `csnotes_check` over notes changed since the last review. Results come back
      as tool output the writer then acts on.
- [ ] Runs after the Phase 3 lint, so the LLM only sees what the regexes can't catch.

### Phase 6 — Scouts over prior material

- [ ] **Cheap layer first (Rust):** at assemble time, keyword-match the session's
      inputs against conversation index files (`global_tags`, `concepts_discussed`),
      resources, and existing note slugs. Emit a "Possibly relevant prior material"
      section in `_session.md` / the agenda.
- [ ] **Agentic layer:** a `scout(query)` tool backed by a pi-subagents agent on a cheap
      model (fan-out/parallel mode). It searches `sources/` + index files and returns
      ≤10 pointers (`file`, turn/heading, one-line why). The main context only sees
      the pointers, so looking stops being expensive.
- [ ] Nudge in `SYSTEM.md`: during Orient, scout each major concept before the debrief.

### Phase 7 — QoL

- [ ] Footer: model + context % + session cost (official `status-line` example, or
      `pi-powerline-footer` if it earns a pin).
- [ ] Official `preset` example: `debrief` (strong model, high thinking), `drafting`
      (mid model), `review` (cross-family). One keystroke each.
- [ ] Official `notify` example for "turn finished" while you're in another window.
- [ ] `pi-btw` for side questions that shouldn't pollute the session. Evaluate it first.
- [ ] Already built in and worth learning: `/tree` for tangents, `/export` to HTML.

---

## Open questions

- Model roster defaults: which OpenRouter models for (a) cheap scout, (b) cross-family
  reviewer, (c) alternate primary. Decide after Phase 1 by feel. Budget target:
  "a couple of dollars a month" for the non-Claude slots.
- Should the Claude backend also get the Phase 3 lint and Phase 6 cheap layer?
  (Yes, they're backend-agnostic; noting it so they don't get built Pi-only by accident.)
- Is the scoped model list per vault (`csnotes.toml`) or per user (`~/.pi/agent`)?
  Leaning per vault, passed via `--models`.
- Does the tutor ever legitimately need a shell? Current answer is no. If one
  appears, add a single allowlisted-argv tool for it; don't bring bash back.
