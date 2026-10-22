#!/usr/bin/env python3
"""
Run a crosslink hook with Pi's tool calls translated to Claude Code's names.

Under Pi (claude-bridge), Claude's tools arrive as MCP tools named
mcp__custom-tools__<tool>, and file tools take `path` instead of `file_path`.
The crosslink hooks only recognise the native names, so without this they
silently allow everything. Native tool calls pass through unchanged.

Usage: pi-tool-shim.py <hook-script>
The hook's stdout, stderr and exit code are passed through. On a blocking
exit (2) stdout is also copied to stderr: work-check prints its reason to
stdout, but Claude Code only shows stderr to the agent for exit 2.
"""

import json
import os
import subprocess
import sys

PI_TOOL_PREFIX = "mcp__custom-tools__"
PI_TOOL_NAMES = {"bash": "Bash", "write": "Write", "edit": "Edit", "read": "Read"}


def translate(input_data):
    tool_name = input_data.get("tool_name", "")
    if not tool_name.startswith(PI_TOOL_PREFIX):
        return input_data
    native = PI_TOOL_NAMES.get(tool_name[len(PI_TOOL_PREFIX):])
    if native is None:
        return input_data

    input_data["tool_name"] = native
    tool_input = input_data.get("tool_input")
    if isinstance(tool_input, dict) and "path" in tool_input and "file_path" not in tool_input:
        path = os.path.expanduser(str(tool_input["path"]))
        if not os.path.isabs(path):
            path = os.path.join(input_data.get("cwd") or os.getcwd(), path)
        tool_input["file_path"] = os.path.normpath(path)
    return input_data


def main():
    if len(sys.argv) != 2:
        print("pi-tool-shim: usage: pi-tool-shim.py <hook-script>", file=sys.stderr)
        sys.exit(2)
    hook = sys.argv[1]

    raw = sys.stdin.read()
    try:
        payload = json.dumps(translate(json.loads(raw)))
    except (json.JSONDecodeError, ValueError, TypeError, AttributeError):
        # Leave malformed input for the hook itself to reject (work-check
        # fails closed on unparseable stdin).
        payload = raw

    result = subprocess.run(
        [sys.executable, hook], input=payload, text=True, stdout=subprocess.PIPE
    )
    sys.stdout.write(result.stdout)
    if result.returncode == 2 and result.stdout:
        sys.stderr.write(result.stdout)
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()
