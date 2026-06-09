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

/// Construct the appropriate backend from the configured/overridden backend.
pub fn make_backend(
    backend: AiBackend,
    skill_variant: SkillVariant,
    fixture: Option<String>,
) -> Box<dyn BackendLauncher> {
    match backend {
        AiBackend::Claude => Box::new(ClaudeBackend { skill_variant }),
        AiBackend::Agy => Box::new(AgyBackend { skill_variant }),
        AiBackend::Mock => Box::new(MockBackend { fixture }),
    }
}

// ── ClaudeBackend ─────────────────────────────────────────────────────────────

pub struct ClaudeBackend {
    pub skill_variant: SkillVariant,
}

impl BackendLauncher for ClaudeBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        // `--system-prompt "."` scopes the system prompt to the workspace's
        // own CLAUDE.md only, preventing global ~/.claude/CLAUDE.md
        // interference.
        let status = Command::new("claude")
            .args(["--system-prompt", "."])
            .current_dir(workspace)
            .status()
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
}

impl BackendLauncher for AgyBackend {
    fn launch(&self, workspace: &Path) -> Result<()> {
        // agy does not auto-load GEMINI.md from cwd; we bootstrap with `-i`
        // to explicitly instruct the model to read it, and `--add-dir` to
        // make the workspace files accessible.
        let workspace_str = workspace.to_string_lossy();
        let status = Command::new("agy")
            .args([
                "-i",
                "Read GEMINI.md in this workspace for your instructions.",
                "--add-dir",
                &workspace_str,
            ])
            .current_dir(workspace)
            .status()
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

        // Copy _session_report.json
        let report_src = fixture_dir.join("_session_report.json");
        if report_src.exists() {
            std::fs::copy(&report_src, workspace.join("_session_report.json"))
                .map_err(|e| CsnotesError::BackendFailed(format!("copying fixture report: {e}")))?;
        }

        // Copy any fixture synthetic note edits
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

    bail!("fixture '{}' not found (set CSNOTES_FIXTURES or run from the project root)", name);
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
