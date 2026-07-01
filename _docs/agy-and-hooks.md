In **Google Antigravity (`agy`)**, controlling file management behavior centers around its **Agent Skills architecture**. While Claude Code relies on deterministic lifecycle hooks to block tool calls by force, Antigravity uses a declarative, capability-driven approach.

"Shadowing" or "wrapping" default file behaviors means providing Gemini with a highly specific, localized alternative skill that handles file architecture and instructing it that using raw shell tools (`mv`, `rm`, `mkdir`) for these actions is invalid.

Here is how you can technically implement this for your `csnotes` workspaces.

---

## 1. Create a Project-Scoped Workspace Skill

To intercept and shadow file management, create a local skill inside your vault or temporary session workspace:
`_csnotes/skills/vault-manager/SKILL.md` (or `.agents/skills/vault-manager/SKILL.md` if you want it automatically discovered by the project root).

Inside `SKILL.md`, define a frontmatter configuration and explicit procedural layout:

```markdown
---
name: vault-manager
description: Mandated workflow for creating, moving, renaming, or deleting files and topics within the csnotes workspace. Use this skill whenever a file structure or content change is required.
---

# Directives

- **NO MANUAL MUTATIONS:** You are STRICTLY FORBIDDEN from using raw shell tools (`Bash`) or default write operations (`mv`, `rm`, `mkdir`) to reorganize the vault.
- **THE STRUCTURAL PROTOCOL:** When notes need to be created, split, merged, or moved, you must append your intent to the `session_report.json` file inside the workspace. The underlying `csnotes` Rust engine handles the actual file execution during teardown.

# Capabilities
If you need to log a structural shift, map it to the JSON schema inside `_csnotes/instructions/report_schema.md`.

```

### Why this works

Unlike a monolithic system prompt that can suffer from context drift, Antigravity **dynamically equips** skills based on the user's request or the agent's intent. When Gemini thinks *"I need to split this sorting algorithm note into two files,"* it matches the intent against the `vault-manager` description and loads these instructions into its immediate context window.

---

## 2. Wrapping via Mock Scripts (Shadowing the Shell)

Because an Antigravity Skill can bundle custom scripts, you can completely "shadow" the system shell for localized actions by providing explicit alternative workflows.

If you want to ensure that even if Gemini slips up and tries to execute an external script or terminal action, it hits your wrapper instead, you can expose a custom script inside the skill folder:

```text
.agents/skills/vault-manager/
├── SKILL.md
└── file-ops-wrapper.sh

```

In your `SKILL.md`, you can bind execution commands directly to the skill:

```markdown
# Executables
If you feel forced to execute a directory operation, you must route it through:
`./file-ops-wrapper.sh <operation> <args>`

```

Inside `file-ops-wrapper.sh`, you can print a standard error block back to the agent:

```sh
#!/bin/bash
echo "ERROR: Direct filesystem mutation rejected by csnotes engine." >&2
echo "Please revert to editing session_report.json to register structural actions." >&2
exit 1

```

When `agy` reads the standard error output from your wrapper, the agent handles it as a tool failure and is forced to fall back to the compliant behavior outlined in your markdown directive.

---

## 3. The Ultimate Fail-Safe: Permission Routing

Because Antigravity CLI operates with a secure-by-default execution framework, the absolute insurance policy alongside your shadow skill is the user approval loop.

When initializing an interactive session via your Rust tool, ensure you don't run `agy` with global bypass flags. If Gemini bypasses the instructions in your Skill and tries to execute a standard shell mutation like `rm -rf _synthetic/sorting/`, the Antigravity runtime will halt execution and prompt you in the terminal:

```text
[agy] Agent wants to execute: rm -rf _synthetic/sorting/
Allow tool execution? (y/N)

```

By pairing a **Workspace Skill** (which shifts the agent's mental model toward generating `session_report.json`) with **strict permission enforcement**, you ensure Gemini remains fully isolated inside your rust-managed pipeline.