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
use std::process::Command;

#[cfg(unix)]
use crate::config::{AiBackend, SkillVariant};
use crate::error::CsnotesError;
use anyhow::{bail, Result};

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait BackendLauncher {
    /// Launch the AI session against the given workspace root.
    /// Blocks until the session exits.
    fn launch(&self, workspace: &Path) -> Result<()>;

    fn backend_name(&self) -> &'static str;
}

// ── Signal-safe interactive runner ────────────────────────────────────────────

/// Run a command that owns the terminal (TUI/interactive session), blocking until
/// it exits.
///
/// On Unix, SIGINT from Ctrl+C is delivered to the entire foreground process
/// group, which includes this parent process.  We ignore SIGINT in the parent
/// for the duration of the child's run so the child TUI can handle it
/// internally (clearing a prompt, cancelling a generation, etc.) without also
/// killing `csnotes` and leaving the workspace dangling.  The default handler
/// is restored immediately when the child exits.
///
/// ## Why the `#[cfg(unix)]` signal lines have no unit tests
///
/// `libc::signal()` mutates **process-wide** signal disposition.  The Rust test
/// harness runs tests on multiple threads inside the same process, so any test
/// that flips `SIGINT` to `SIG_IGN` and back would race with every other
/// concurrently executing test and could silence `Ctrl+C` for the entire suite.
/// The only meaningful verification is an end-to-end integration test that
/// spawns a real child process, sends `SIGINT` to the process group, and
/// confirms the parent survives — which is not practical as an automated unit
/// test.  Coverage on these two lines is therefore intentionally absent.
fn run_interactive(cmd: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    #[cfg(unix)]
    // SAFETY: signal() is async-signal-safe; we restore SIG_DFL on exit.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
    }
    let result = cmd.status();
    #[cfg(unix)]
    // SAFETY: restoring SIG_DFL is always safe; no allocations or locks held.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
    result
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Construct the appropriate backend from the configured/overridden backend.
///
/// `agy_model` is forwarded to `AgyBackend` only; ignored for other backends.
/// `resume` causes the backend to re-enter the most recent conversation (`-c`)
/// rather than starting a fresh one.
pub fn make_backend(
    backend: AiBackend,
    skill_variant: SkillVariant,
    fixture: Option<String>,
    agy_model: Option<String>,
    resume: bool,
) -> Box<dyn BackendLauncher> {
    match backend {
        AiBackend::Claude => Box::new(ClaudeBackend {
            skill_variant,
            resume,
        }),
        AiBackend::Agy => Box::new(AgyBackend {
            skill_variant,
            model: agy_model,
            resume,
        }),
        AiBackend::Mock => Box::new(MockBackend { fixture }),
    }
}

// ── ClaudeBackend ─────────────────────────────────────────────────────────────

pub struct ClaudeBackend {
    pub skill_variant: SkillVariant,
    pub resume: bool,
}

impl BackendLauncher for ClaudeBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        // `--system-prompt "."` scopes the system prompt to the workspace's
        // own CLAUDE.md only, preventing global ~/.claude/CLAUDE.md
        // interference.  `-c` re-enters the most recent conversation when
        // resuming an interrupted session.
        let mut args = vec!["--system-prompt", "."];
        if self.resume {
            args.push("-c");
        }
        let mut cmd = Command::new("claude");
        cmd.args(&args).current_dir(workspace);
        let status = run_interactive(&mut cmd)
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
    /// Optional Gemini model override passed to `agy --model`.
    /// When `None`, `agy` uses its own default (currently gemini-2.5-pro).
    pub model: Option<String>,
    pub resume: bool,
}

impl BackendLauncher for AgyBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        // `agy` does not auto-load GEMINI.md from cwd (unlike Claude Code,
        // which auto-discovers CLAUDE.md).  We bootstrap with `-i` to
        // instruct the model to read the instruction file immediately, and
        // `--add-dir` to make all workspace files accessible as context.
        let workspace_str = workspace.to_string_lossy();

        let mut cmd = Command::new("agy");

        // Model selection — omit entirely if not set so `agy` uses its default.
        if let Some(ref m) = self.model {
            cmd.args(["--model", m]);
        }

        // `-c` re-enters the most recent agy conversation (scoped to cwd by agy).
        if self.resume {
            cmd.arg("-c");
        }

        cmd.args([
            "-i",
            "Read GEMINI.md in this workspace — it contains your full instructions for this session.",
            "--add-dir",
            &workspace_str,
        ])
        .current_dir(workspace);

        let status = run_interactive(&mut cmd)
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

/// Test backend: copies fixture files into the workspace instead of launching
/// a real AI.  Used with `--backend mock [--fixture NAME]`.
///
/// Fixtures live in `tests/fixtures/<name>/`.  A fixture set contains:
/// - `_session_report.json` — the report the "AI" emits
/// - `_synthetic/` — note body edits (copied into the workspace's writable
///   `_synthetic/` copy, overwriting existing files as the AI would)
pub struct MockBackend {
    /// Name of the fixture set to use.  If `None`, uses "default".
    pub fixture: Option<String>,
}

impl BackendLauncher for MockBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        let fixture_name = self.fixture.as_deref().unwrap_or("default");

        // Locate the fixtures directory relative to the binary's source tree.
        // In tests this is resolved via CARGO_MANIFEST_DIR; in production use
        // `--backend mock` is only meaningful during development.
        let fixture_dir = locate_fixture_dir(fixture_name)?;

        // Derive the actual run_id from the workspace directory name.
        // workspace_root = $base/csnotes/<run_id>, so file_name() == run_id.
        let run_id = workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Read, patch run_id, and write _session_report.json.
        // The fixture file stores a placeholder; the real run_id is only known
        // at launch time and must match what the teardown will check.
        let report_src = fixture_dir.join("_session_report.json");
        if report_src.exists() {
            let content = std::fs::read_to_string(&report_src)
                .map_err(|e| CsnotesError::BackendFailed(format!("reading fixture report: {e}")))?;
            let mut report: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| CsnotesError::BackendFailed(format!("parsing fixture report: {e}")))?;
            report["run_id"] = serde_json::Value::String(run_id);
            let patched = serde_json::to_string_pretty(&report).map_err(|e| {
                CsnotesError::BackendFailed(format!("serialising patched report: {e}"))
            })?;
            std::fs::write(workspace.join("_session_report.json"), patched)
                .map_err(|e| CsnotesError::BackendFailed(format!("writing fixture report: {e}")))?;
        }

        // Copy any fixture synthetic note edits into the workspace.
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
    // Check CSNOTES_FIXTURES env var first (useful in integration tests).
    if let Ok(base) = std::env::var("CSNOTES_FIXTURES") {
        let p = std::path::PathBuf::from(base).join(name);
        if p.exists() {
            return Ok(p);
        }
    }

    // Fall back to tests/fixtures/ relative to CARGO_MANIFEST_DIR.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = std::path::PathBuf::from(manifest)
            .join("tests")
            .join("fixtures")
            .join(name);
        if p.exists() {
            return Ok(p);
        }
    }

    bail!(
        "fixture '{}' not found (set CSNOTES_FIXTURES or run from the project root)",
        name
    );
}

/// Recursively copy `src/` into `dst/`, overwriting files that exist.
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
