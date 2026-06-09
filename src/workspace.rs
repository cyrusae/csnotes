/// Workspace assembly and teardown.
///
/// The AI session runs entirely inside an out-of-vault workspace:
///
///   $XDG_RUNTIME_DIR/csnotes/<run_id>/
///     CLAUDE.md  (or GEMINI.md)            ← copied from _csnotes/instructions/
///     synthesis.md                          ← copied from _csnotes/instructions/
///     _session.md                           ← rendered briefing
///     _session_report.json                  ← written by AI on exit
///     <inputs>.md                           ← XML-wrapped, read-only copies
///     _synthetic/                           ← writable working copy
///
/// The vault is never touched until the brief, snapshot-guarded merge-back.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use uuid::Uuid;

use crate::config::{SkillVariant, VaultConfig};
use crate::error::CsnotesError;
use crate::flags::FlagStore;
use crate::manifest::{Manifest, SessionEntry, SourceEntry};
use crate::obsidian::collect_all_block_ids;

// ── Workspace paths ───────────────────────────────────────────────────────────

/// Determine the base directory for workspaces.
/// Priority: `$XDG_RUNTIME_DIR` → `$TMPDIR` → `/tmp`.
pub fn workspace_base_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(d).join("csnotes");
        fs::create_dir_all(&p).ok();
        return Ok(p);
    }
    if let Ok(d) = std::env::var("TMPDIR") {
        let p = PathBuf::from(d).join("csnotes");
        fs::create_dir_all(&p).ok();
        return Ok(p);
    }
    Ok(PathBuf::from("/tmp/csnotes"))
}

/// Generate a new run ID (UUIDv4, truncated to 8 hex chars for readability).
pub fn new_run_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

// ── Assembly ──────────────────────────────────────────────────────────────────

/// Parameters for assembling a workspace.
pub struct WorkspaceParams<'a> {
    pub vault_root: &'a Path,
    pub config: &'a VaultConfig,
    pub manifest: &'a Manifest,
    pub run_id: &'a str,
    pub scope: WorkspaceScope,
    /// If true, print what would happen but don't create anything.
    pub dry_run: bool,
}

pub enum WorkspaceScope {
    Session { session_id: String },
    Source { source_id: String },
    Topic { topic: String },
}

/// Assemble the workspace and return its root path.
pub fn assemble(params: &WorkspaceParams<'_>) -> Result<PathBuf> {
    let base = workspace_base_dir()?;
    let workspace_root = base.join(params.run_id);

    if workspace_root.exists() {
        bail!(CsnotesError::WorkspaceExists(workspace_root));
    }

    if params.dry_run {
        println!("dry-run: workspace would be created at {}", workspace_root.display());
        return Ok(workspace_root);
    }

    fs::create_dir_all(&workspace_root)
        .context("creating workspace directory")?;

    // 1. Copy instruction files
    copy_instruction_files(params.vault_root, params.config, &workspace_root)?;

    // 2. Copy and wrap inputs
    match &params.scope {
        WorkspaceScope::Session { session_id } => {
            let entry = params
                .manifest
                .sessions
                .get(session_id)
                .ok_or_else(|| CsnotesError::SessionNotFound(session_id.clone()))?;
            wrap_session_inputs(params.vault_root, entry, &workspace_root)?;
        }
        WorkspaceScope::Source { source_id } => {
            let entry = params
                .manifest
                .sources
                .get(source_id)
                .ok_or_else(|| CsnotesError::SourceNotFound(source_id.clone()))?;
            wrap_source_input(params.vault_root, source_id, entry, &workspace_root)?;
        }
        WorkspaceScope::Topic { topic: _ } => {
            // No external inputs — topic study session.
        }
    }

    // 3. Writable copy of _synthetic/
    let vault_synthetic = params.vault_root.join(&params.config.synthetic_dir);
    let ws_synthetic = workspace_root.join(&params.config.synthetic_dir);
    if vault_synthetic.exists() {
        copy_dir(&vault_synthetic, &ws_synthetic)?;
    } else {
        fs::create_dir_all(&ws_synthetic)?;
    }

    // 4. Render _session.md
    render_session_md(params, &workspace_root)?;

    Ok(workspace_root)
}

// ── Instruction files ─────────────────────────────────────────────────────────

fn copy_instruction_files(
    vault_root: &Path,
    config: &VaultConfig,
    workspace_root: &Path,
) -> Result<()> {
    let instruction_src = config.instruction_source_path(vault_root);
    let synthesis_src = config.synthesis_md_path(vault_root);
    let report_schema_src = config.report_schema_path(vault_root);

    let dest_name = match config.skill_variant {
        SkillVariant::Claude => "CLAUDE.md",
        SkillVariant::Gemini => "GEMINI.md",
    };

    if instruction_src.exists() {
        fs::copy(&instruction_src, workspace_root.join(dest_name))
            .with_context(|| format!("copying {}", instruction_src.display()))?;
    } else {
        // Write a minimal stub so the session can proceed
        fs::write(
            workspace_root.join(dest_name),
            "# csnotes session\n\nNo instruction file found. \
             Run `csnotes init` to create one.\n",
        )?;
    }

    if synthesis_src.exists() {
        fs::copy(&synthesis_src, workspace_root.join("synthesis.md"))
            .with_context(|| format!("copying {}", synthesis_src.display()))?;
    }

    if report_schema_src.exists() {
        fs::copy(&report_schema_src, workspace_root.join("report_schema.md"))
            .with_context(|| format!("copying {}", report_schema_src.display()))?;
    }

    Ok(())
}

// ── XML wrapping ──────────────────────────────────────────────────────────────

/// Wrap file content in an XML tag with attributes.
pub fn xml_wrap(content: &str, tag: &str, attrs: &[(&str, &str)]) -> String {
    let attr_str: String = attrs
        .iter()
        .map(|(k, v)| format!(" {}=\"{}\"", k, v))
        .collect();
    format!("<{}{}>\n{}\n</{}>", tag, attr_str, content, tag)
}

fn wrap_session_inputs(
    vault_root: &Path,
    entry: &SessionEntry,
    workspace_root: &Path,
) -> Result<()> {
    let course = &entry.course;
    let date = entry.date.to_string();

    // Raw notes
    let raw_path = vault_root.join(&entry.raw_note);
    if raw_path.exists() {
        let content = fs::read_to_string(&raw_path)?;
        let wrapped = xml_wrap(&content, "raw_student_notes", &[("course", course), ("date", &date)]);
        let dest = workspace_root.join(format!("input_raw_{}.md", sanitise(&entry.raw_note)));
        fs::write(&dest, wrapped)?;
        make_readonly(&dest)?;
    }

    // Plaud exports
    for export in &entry.plaud_exports {
        let src = vault_root.join(&export.path);
        if !src.exists() {
            continue;
        }
        let content = fs::read_to_string(&src)?;
        let tag = match export.kind {
            crate::manifest::PlaudKind::Transcript => "plaud_transcript",
            crate::manifest::PlaudKind::Summary => "plaud_summary",
            crate::manifest::PlaudKind::Mindmap => "plaud_mindmap",
            _ => "plaud_export",
        };
        let wrapped = xml_wrap(&content, tag, &[("course", course), ("date", &date)]);
        let dest = workspace_root.join(format!("input_{}.md", sanitise(&export.path)));
        fs::write(&dest, wrapped)?;
        make_readonly(&dest)?;
    }

    // Artifacts
    for artifact in &entry.artifacts {
        let src = vault_root.join(&artifact.path);
        if !src.exists() {
            continue;
        }
        let content = fs::read_to_string(&src)?;
        let (tag, extra_attrs);
        let file_name = Path::new(&artifact.path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        match artifact.kind {
            crate::manifest::ArtifactKind::Slides => {
                tag = "lecture_slides";
                extra_attrs = vec![("course", course.as_str()), ("date", date.as_str())];
            }
            crate::manifest::ArtifactKind::Code => {
                tag = "instructor_code_sample";
                extra_attrs = vec![
                    ("file", &file_name),
                    ("course", course),
                    ("date", &date),
                ];
            }
            _ => {
                tag = "artifact";
                extra_attrs = vec![("course", course.as_str()), ("date", date.as_str())];
            }
        }
        let wrapped = xml_wrap(&content, tag, &extra_attrs);
        let dest = workspace_root.join(format!("input_{}.md", sanitise(&artifact.path)));
        fs::write(&dest, wrapped)?;
        make_readonly(&dest)?;
    }

    Ok(())
}

fn wrap_source_input(
    vault_root: &Path,
    source_id: &str,
    entry: &SourceEntry,
    workspace_root: &Path,
) -> Result<()> {
    let src = vault_root.join(&entry.path);
    if !src.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&src)?;
    // book and unit are derived from the source_id path structure
    let (book, unit) = parse_source_id(source_id);
    let wrapped = xml_wrap(
        &content,
        "textbook_source",
        &[("id", source_id), ("book", &book), ("unit", &unit)],
    );
    let dest = workspace_root.join(format!("input_source_{}.md", sanitise(source_id)));
    fs::write(&dest, wrapped)?;
    make_readonly(&dest)?;
    Ok(())
}

fn parse_source_id(id: &str) -> (String, String) {
    // "SICP/SICP-ch01" → book = "SICP", unit = "SICP-ch01"
    // "TAPL-notes"     → book = "TAPL-notes", unit = ""
    if let Some((book, unit)) = id.split_once('/') {
        (book.to_string(), unit.to_string())
    } else {
        (id.to_string(), String::new())
    }
}

// ── _session.md rendering ─────────────────────────────────────────────────────

fn render_session_md(params: &WorkspaceParams<'_>, workspace_root: &Path) -> Result<()> {
    let mut out = String::new();

    // Header
    match &params.scope {
        WorkspaceScope::Session { session_id } => {
            out.push_str(&format!("# Session Briefing — {}\n\n", session_id));
            let entry = params.manifest.sessions.get(session_id);
            out.push_str("## Scope\n");
            if let Some(e) = entry {
                out.push_str(&format!("- Course: {}\n", e.course));
                out.push_str(&format!("- Date: {}\n", e.date));
            }
            out.push_str(&format!("- Sessions being processed: {}\n", session_id));
            out.push_str(&format!("- run_id: `{}`\n\n", params.run_id));

            // Inputs
            out.push_str("## Inputs in This Workspace\n");
            if let Some(e) = entry {
                out.push_str(&format!("- Raw notes: `<raw_student_notes>` tag\n"));
                if e.plaud_missing {
                    out.push_str(
                        "- Plaud: **not available** — synthesise from raw notes only\n",
                    );
                } else if e.plaud_exports.is_empty() {
                    out.push_str("- Plaud: none\n");
                } else {
                    let kinds: Vec<_> = e
                        .plaud_exports
                        .iter()
                        .map(|p| format!("{:?}", p.kind).to_lowercase())
                        .collect();
                    out.push_str(&format!("- Plaud: {}\n", kinds.join(", ")));
                }
                if e.artifacts.is_empty() {
                    out.push_str("- Artifacts: none\n");
                } else {
                    let names: Vec<_> = e
                        .artifacts
                        .iter()
                        .map(|a| Path::new(&a.path).file_name().unwrap_or_default().to_string_lossy().to_string())
                        .collect();
                    out.push_str(&format!("- Artifacts: {}\n", names.join(", ")));
                }
            }
            out.push('\n');
        }
        WorkspaceScope::Source { source_id } => {
            out.push_str(&format!("# Source Briefing — {}\n\n", source_id));
            out.push_str("## Scope\n");
            out.push_str(&format!("- Source: {}\n", source_id));
            out.push_str(&format!("- run_id: `{}`\n\n", params.run_id));
        }
        WorkspaceScope::Topic { topic } => {
            out.push_str(&format!("# Study Session — {}\n\n", topic));
            out.push_str("## Scope\n");
            out.push_str(&format!("- Topic: {}\n", topic));
            out.push_str("- Mode: study/review (no new session input)\n");
            out.push_str(&format!("- run_id: `{}`\n\n", params.run_id));
        }
    }

    // Existing synthetic notes
    out.push_str("## Existing Synthetic Notes\n");
    let synthetic_root = params.vault_root.join(&params.config.synthetic_dir);
    if synthetic_root.exists() {
        for (topic_name, topic) in &params.manifest.topics {
            out.push_str(&format!("\n### {}\n", topic_name));
            out.push_str(&format!("- Index: `{}`\n", topic.index_note));
            for atomic in &topic.atomic_notes {
                out.push_str(&format!("- Atomic: `{}`\n", atomic));
            }
            out.push_str(&format!("- Last updated: {}\n", topic.last_updated.format("%Y-%m-%d")));
        }
    } else {
        out.push_str("_No synthetic notes yet._\n");
    }

    // All known block IDs (for collision avoidance)
    out.push_str("\n## All Known Block IDs (vault-wide)\n");
    out.push_str("_Reuse existing IDs where appropriate; never create a duplicate._\n\n");
    let ws_synthetic = workspace_root.join(&params.config.synthetic_dir);
    if ws_synthetic.exists() {
        let block_ids = collect_all_block_ids(&ws_synthetic)?;
        if block_ids.is_empty() {
            out.push_str("_None yet._\n");
        } else {
            let mut sorted: Vec<_> = block_ids.iter().collect();
            sorted.sort_by_key(|(id, _)| (*id).clone());
            for (id, path) in sorted {
                out.push_str(&format!("- `^{}` → `{}`\n", id, path));
            }
        }
    }

    // Open flags
    out.push_str("\n## Open Flags\n");
    if let Ok(flag_store) =
        FlagStore::load(&params.manifest.flags_path_absolute())
    {
        let actionable: Vec<_> = flag_store.open_actionable().collect();
        let threads: Vec<_> = flag_store.open_threads().collect();
        if actionable.is_empty() && threads.is_empty() {
            out.push_str("_None._\n");
        }
        for flag in actionable {
            out.push_str(&format!(
                "- [{}] **{}**: {}\n",
                flag.id,
                flag.display_kind(),
                flag.message
            ));
        }
        for flag in threads {
            out.push_str(&format!(
                "- [{}] *{}*: {}\n",
                flag.id,
                flag.display_kind(),
                flag.message
            ));
        }
    }

    out.push('\n');

    fs::write(workspace_root.join("_session.md"), out)
        .context("writing _session.md")?;
    Ok(())
}

// ── Merge-back + snapshot ─────────────────────────────────────────────────────

/// Take a pre-merge snapshot of vault `_synthetic/`.
/// Returns the snapshot path.
pub fn take_snapshot(vault_root: &Path, synthetic_dir: &str, run_id: &str) -> Result<PathBuf> {
    let src = vault_root.join(synthetic_dir);
    let snapshot = vault_root.join(format!("_synthetic_snapshot_{}", run_id));
    if src.exists() {
        copy_dir(&src, &snapshot)?;
    } else {
        fs::create_dir_all(&snapshot)?;
    }
    Ok(snapshot)
}

/// Merge the workspace's `_synthetic/` into the vault, then clean up.
/// Called only after the invariant suite passes.
pub fn merge_back(
    workspace_root: &Path,
    vault_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let ws_synthetic = workspace_root.join(synthetic_dir);
    let vault_synthetic = vault_root.join(synthetic_dir);

    // Ensure vault synthetic dir exists
    fs::create_dir_all(&vault_synthetic)?;

    // Copy each file from the workspace into the vault
    for entry in walkdir::WalkDir::new(&ws_synthetic)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(&ws_synthetic).unwrap();
        let dest = vault_synthetic.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dest)?;
        }
    }

    Ok(())
}

/// Restore a pre-merge snapshot (called by `recover` if a crash interrupted
/// the merge).
pub fn restore_snapshot(vault_root: &Path, synthetic_dir: &str, run_id: &str) -> Result<()> {
    let snapshot = vault_root.join(format!("_synthetic_snapshot_{}", run_id));
    if !snapshot.exists() {
        return Ok(()); // nothing to restore
    }

    let current = vault_root.join(synthetic_dir);
    let broken = vault_root.join(format!("_synthetic_broken_{}", run_id));

    // Move current → broken (in case we need to inspect it)
    if current.exists() {
        fs::rename(&current, &broken)?;
    }

    // Move snapshot → current
    fs::rename(&snapshot, &current)?;

    // Clean up broken dir
    if broken.exists() {
        fs::remove_dir_all(&broken).ok();
    }

    Ok(())
}

/// Delete workspace and snapshot directories after a successful merge.
pub fn cleanup(workspace_root: &Path, vault_root: &Path, run_id: &str) -> Result<()> {
    let snapshot = vault_root.join(format!("_synthetic_snapshot_{}", run_id));
    if workspace_root.exists() {
        fs::remove_dir_all(workspace_root).ok();
    }
    if snapshot.exists() {
        fs::remove_dir_all(snapshot).ok();
    }
    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(src).unwrap();
        let dest = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn make_readonly(path: &Path) -> Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_readonly(true);
    fs::set_permissions(path, perms)?;
    Ok(())
}

fn sanitise(path: &str) -> String {
    path.replace(['/', '\\', '.', ' '], "_")
}
