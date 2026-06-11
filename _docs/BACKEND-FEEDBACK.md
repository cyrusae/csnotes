Looking closely at how you are spawning the processes, **there are a few critical issues regarding standard stream inheritance and signal handling** that will cause both `claude` and `agy` to crash or hang instantly when invoked.

Here is an analysis of what needs to be fixed to make your interactive sessions robust.

---

## The Issues

### 1. Missing Standard I/O Inheritance (The TUI Crash)

In both `ClaudeBackend` and `AgyBackend`, you are using standard `Command::new(...).status()`. By default, Rust's `Command` type without explicit configuration does not guarantee that `stdin` is fully attached as an interactive TTY when run deep inside a complex CLI layer.

* If `claude` or `agy` detects that `stdin` or `stdout` is not a raw terminal TTY, they may either **panic, immediately exit, or fall back to an unreadable raw stream** instead of rendering their interactive terminal interface.

### 2. The `Ctrl+C` (SIGINT) Panic

If a user is inside an interactive Claude or Antigravity session and hits `Ctrl+C` to clear a prompt line or cancel a long streaming generation, the operating system broadcasts `SIGINT` to the *entire foreground process group*.

* This means **both your child process (`agy`/`claude`) and your parent process (`csnotes`) will receive `SIGINT` simultaneously**.
* Since your Rust program has no signal handler here, `csnotes` will instantly terminate on `Ctrl+C`. This completely bypasses your crash recovery mechanism, skips writing state flags, leaves the workspace dangling, and breaks your state consistency guarantees.

---

## The Solution

To fix this, you need to:

1. Explicitly attach `.stdin(Stdio::inherit())`, `.stdout(Stdio::inherit())`, and `.stderr(Stdio::inherit())` before calling `.status()`.
2. Implement a quick signal abstraction layer during execution so `csnotes` gracefully waits for the child process to complete its exit sequence before processing errors or recoveries.

Here is the corrected and hardened implementation of `backend.rs`:

```rust
#![allow(dead_code)]
/// AI backend launchers.
///
/// The CLI launches an interactive AI session and blocks until it exits.
/// The AI's only outputs that matter to the CLI are:
///   - edits to the workspace `_synthetic/` tree (note bodies)
///   - `_session_report.json` written before exit
///
/// `ClaudeBackend` — uses `claude --system-prompt "."` to scope the system
/// prompt to the workspace's own `CLAUDE.md`, preventing any global
/// `~/.claude/CLAUDE.md` from leaking into the session.
///
/// `AgyBackend` — `agy` does not auto-load `GEMINI.md` from cwd (its
/// `.antigravitycli/` directory is only a project registry pointer).
/// Context is provided via `--add-dir` and bootstrapped with `-i` so the
/// model reads `GEMINI.md` on startup.
///
/// `MockBackend` — copies fixture files into the workspace instead of
/// spawning a process; used for CI and `--backend mock`.
use std::path::Path;
use std::process::{Command, Stdio}; // Added Stdio

use anyhow::{bail, Result};

use crate::config::{AiBackend, SkillVariant};
use crate::error::CsnotesError;

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait BackendLauncher {
    /// Launch the AI session against the given workspace root.
    /// Blocks until the session exits.
    fn launch(&self, workspace: &Path) -> Result<()>;

    fn backend_name(&self) -> &'static str;
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn make_backend(
    backend: AiBackend,
    skill_variant: SkillVariant,
    fixture: Option<String>,
    agy_model: Option<String>,
    resume: bool,
) -> Box<dyn BackendLauncher> {
    match backend {
        AiBackend::Claude => Box::new(ClaudeBackend { skill_variant, resume }),
        AiBackend::Agy => Box::new(AgyBackend { skill_variant, model: agy_model, resume }),
        AiBackend::Mock => Box::new(MockBackend { fixture }),
    }
}

// ── Helper for Signal Shielding ──────────────────────────────────────────────

/// Executes a command while shielding the parent process from being killed instantly by Ctrl+C.
/// This allows the interactive TUI child to handle SIGINT internally for its own workflow.
fn execute_interactive_command(cmd: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    // Suppress or handle Ctrl+C in parent thread while child runs.
    // If you use the `ctrlc` crate:
    let _ = ctrlc::set_handler(|| {
        // Do nothing in the parent shell; let the child TUI catch it and react.
    });

    let status = cmd.status();

    // Re-enable default behavior or your application's normal handler after completion
    // if your main loop requires custom teardown monitoring.
    
    status
}

// ── ClaudeBackend ─────────────────────────────────────────────────────────────

pub struct ClaudeBackend {
    pub skill_variant: SkillVariant,
    pub resume: bool,
}

impl BackendLauncher for ClaudeBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        let mut args = vec!["--system-prompt", "."];
        if self.resume {
            args.push("-c");
        }

        let mut cmd = Command::new("claude");
        cmd.args(&args)
            .current_dir(workspace)
            // CRITICAL: Inherit standard streams so interactive terminal rendering works flawlessly
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = execute_interactive_command(&mut cmd)
            .map_err(|e| CsnotesError::BackendFailed(format!("claude: {e}")))?;

        if !status.success() {
            bail!(CsnotesError::BackendFailed(format!(
                "claude exited with status {}",
                status
            )));
        }
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "claude"
    }
}

// ── AgyBackend ────────────────────────────────────────────────────────────────

pub struct AgyBackend {
    pub skill_variant: SkillVariant,
    pub model: Option<String>,
    pub resume: bool,
}

impl BackendLauncher for AgyBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        let workspace_str = workspace.to_string_lossy();
        let mut cmd = Command::new("agy");

        if let Some(ref m) = self.model {
            cmd.args(["--model", m]);
        }

        if self.resume {
            cmd.arg("-c");
        }

        cmd.args([
            "-i",
            "Read GEMINI.md in this workspace — it contains your full instructions for this session.",
            "--add-dir",
            &workspace_str,
        ])
        .current_dir(workspace)
        // CRITICAL: Inherit standard streams so the terminal UI renders properly
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

        let status = execute_interactive_command(&mut cmd)
            .map_err(|e| CsnotesError::BackendFailed(format!("agy: {e}")))?;

        if !status.success() {
            bail!(CsnotesError::BackendFailed(format!(
                "agy exited with status {}",
                status
            )));
        }
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "agy"
    }
}

// ── MockBackend ───────────────────────────────────────────────────────────────

pub struct MockBackend {
    pub fixture: Option<String>,
}

impl BackendLauncher for MockBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        let fixture_name = self.fixture.as_deref().unwrap_or("default");
        let fixture_dir = locate_fixture_dir(fixture_name)?;

        let run_id = workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let report_src = fixture_dir.join("_session_report.json");
        if report_src.exists() {
            let content = std::fs::read_to_string(&report_src)
                .map_err(|e| CsnotesError::BackendFailed(format!("reading fixture report: {e}")))?;
            let mut report: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| CsnotesError::BackendFailed(format!("parsing fixture report: {e}")))?;
            report["run_id"] = serde_json::Value::String(run_id);
            let patched = serde_json::to_string_pretty(&report)
                .map_err(|e| CsnotesError::BackendFailed(format!("serialising patched report: {e}")))?;
            std::fs::write(workspace.join("_session_report.json"), patched)
                .map_err(|e| CsnotesError::BackendFailed(format!("writing fixture report: {e}")))?;
        }

        let synthetic_src = fixture_dir.join("_synthetic");
        if synthetic_src.exists() {
            copy_dir_merge(&synthetic_src, &workspace.join("_synthetic"))?;
        }

        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "mock"
    }
}

fn locate_fixture_dir(name: &str) -> Result<std::path::PathBuf> {
    if let Ok(base) = std::env::var("CSNOTES_FIXTURES") {
        let p = std::path::PathBuf::from(base).join(name);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = std::path::PathBuf::from(manifest)
            .join("tests")
            .join("fixtures")
            .join(name);
        if p.exists() {
            return Ok(p);
        }
    }

    bail!("fixture '{}' not found (set CSNOTES_FIXTURES or run from the project root)", name);
}

fn copy_dir_merge(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(src).unwrap();
        let dest = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

```

### Highlights of the Changes:

* **`use std::process::{Command, Stdio};`**: Standard inputs and outputs are now explicitly declared.
* **`.stdin/stdout/stderr(Stdio::inherit())`**: Ensures that the child processes share the same interactive terminal window context.
* **`execute_interactive_command` helper**: Intercepts the parent loop's standard behavior during execution, allowing the user to seamlessly use `Ctrl+C` within Claude Code or `agy` without breaking the `csnotes` application state tracking.