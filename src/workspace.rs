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
        let content = crate::frontmatter::read_note(&raw_path)?;
        let wrapped = xml_wrap(&content, "raw_student_notes", &[("course", course), ("date", &date)]);
        let dest = workspace_root.join(format!("input_raw_{}.md", sanitise(&entry.raw_note)));
        fs::write(&dest, wrapped)?;
        make_readonly(&dest)?;
    }

    // Recording exports
    for export in &entry.recording_exports {
        let src = vault_root.join(&export.path);
        if !src.exists() {
            continue;
        }
        let content = crate::frontmatter::read_note(&src)?;
        let tag = match export.kind {
            crate::manifest::RecordingKind::Transcript => "recording_transcript",
            crate::manifest::RecordingKind::Summary => "recording_summary",
            crate::manifest::RecordingKind::Mindmap => "recording_mindmap",
            _ => "recording_export",
        };
        let wrapped = xml_wrap(&content, tag, &[("course", course), ("date", &date)]);
        let dest = workspace_root.join(format!("input_{}.md", sanitise(&export.path)));
        fs::write(&dest, wrapped)?;
        make_readonly(&dest)?;
    }

    // Artifacts — text-readable formats only; binary files (PDF, images, etc.)
    // are silently skipped because the AI cannot consume them.
    for artifact in &entry.artifacts {
        let src = vault_root.join(&artifact.path);
        if !src.exists() {
            continue;
        }
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !is_text_artifact_ext(&ext) {
            continue;
        }
        let content = crate::frontmatter::read_note(&src)?;
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
    let content = crate::frontmatter::read_note(&src)?;
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
                if e.recording_missing {
                    out.push_str(
                        "- Recording: **not available** — synthesise from raw notes only\n",
                    );
                } else if e.recording_exports.is_empty() {
                    out.push_str("- Recording: none\n");
                } else {
                    let kinds: Vec<_> = e
                        .recording_exports
                        .iter()
                        .map(|p| format!("{:?}", p.kind).to_lowercase())
                        .collect();
                    out.push_str(&format!("- Recording: {}\n", kinds.join(", ")));
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

    // Existing synthetic notes — per-topic with block IDs, pending sessions,
    // and topic-scoped flags inline.
    out.push_str("## Existing Synthetic Notes\n");
    out.push_str("_Block IDs listed per topic — reuse existing IDs; never duplicate._\n");

    let ws_synthetic = workspace_root.join(&params.config.synthetic_dir);
    // Collect all block IDs once, then bucket by topic for inline display.
    let all_block_ids = if ws_synthetic.exists() {
        collect_all_block_ids(&ws_synthetic)?
    } else {
        std::collections::HashMap::new()
    };

    let flag_store = FlagStore::load(&params.manifest.flags_path_absolute())
        .unwrap_or_default();

    if params.manifest.topics.is_empty() {
        out.push_str("\n_No synthetic notes yet._\n");
    } else {
        for (topic_name, topic) in &params.manifest.topics {
            out.push_str(&format!("\n### {}\n", topic_name));
            out.push_str(&format!("- Index: `{}`\n", topic.index_note));
            for atomic in &topic.atomic_notes {
                out.push_str(&format!("- Atomic: `{}`\n", atomic));
            }
            out.push_str(&format!(
                "- Last updated: {}\n",
                topic.last_updated.format("%Y-%m-%d")
            ));

            // Pending sessions (processed after this topic's last_updated).
            if !topic.pending_sessions.is_empty() {
                out.push_str(&format!(
                    "- ⚠ Pending sessions (processed after last update): {}\n",
                    topic.pending_sessions.join(", ")
                ));
            }

            // Block IDs belonging to this topic's folder.
            let topic_prefix = format!("{}/", topic_name);
            let mut topic_ids: Vec<(&String, &String)> = all_block_ids
                .iter()
                .filter(|(_, path)| {
                    // path is relative to _synthetic/, so strip the synthetic
                    // dir prefix to check the topic component.
                    let stripped = path
                        .trim_start_matches(&format!("{}/", params.config.synthetic_dir));
                    stripped.starts_with(&topic_prefix)
                })
                .collect();
            topic_ids.sort_by_key(|(id, _)| *id);
            if !topic_ids.is_empty() {
                out.push_str("- Block IDs:\n");
                for (id, path) in &topic_ids {
                    out.push_str(&format!("  - `^{}` in `{}`\n", id, path));
                }
            }

            // Open flags scoped to this topic.
            let topic_flags: Vec<_> = flag_store.open_for_topic(topic_name, &params.config.synthetic_dir).collect();
            if !topic_flags.is_empty() {
                out.push_str("- Open flags:\n");
                for flag in topic_flags {
                    let anchor_suffix = flag
                        .anchor
                        .as_deref()
                        .map(|a| format!(" `^{}`", a))
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "  - [{}] **{}**{}: {}\n",
                        flag.id,
                        flag.display_kind(),
                        anchor_suffix,
                        flag.message
                    ));
                }
            }
        }
    }

    // Resolved follow-ups — re-inject so the AI knows how past questions
    // were answered and can apply that context when writing notes.
    let follow_ups: Vec<_> = flag_store.resolved_with_follow_up().collect();
    if !follow_ups.is_empty() {
        out.push_str("\n## Resolved Follow-ups\n");
        out.push_str("_These flags were resolved by the user; apply any corrections noted below._\n\n");
        for flag in follow_ups {
            let path_note = flag
                .path
                .as_deref()
                .map(|p| format!(" (`{}`)", p))
                .unwrap_or_default();
            out.push_str(&format!(
                "- **{}**{}: {}\n",
                flag.display_kind(),
                path_note,
                flag.message
            ));
            if let Some(fu) = &flag.follow_up {
                out.push_str(&format!("  → Follow-up: {}\n", fu));
            }
        }
    }

    // Open flags not scoped to any specific topic (vault-wide actionable +
    // threads that don't belong to a known topic folder).
    out.push_str("\n## Open Flags (vault-wide)\n");
    {
        // Collect flags NOT already shown in the per-topic sections above.
        let known_topic_prefixes: Vec<String> = params
            .manifest
            .topics
            .keys()
            .map(|t| format!("_synthetic/{}/", t))
            .collect();
        let unscoped_actionable: Vec<_> = flag_store
            .open_actionable()
            .filter(|f| {
                f.path.as_deref().map_or(true, |p| {
                    !known_topic_prefixes.iter().any(|prefix| p.starts_with(prefix))
                })
            })
            .collect();
        let unscoped_threads: Vec<_> = flag_store
            .open_threads()
            .filter(|f| {
                f.path.as_deref().map_or(true, |p| {
                    !known_topic_prefixes.iter().any(|prefix| p.starts_with(prefix))
                })
            })
            .collect();
        if unscoped_actionable.is_empty() && unscoped_threads.is_empty() {
            out.push_str("_None._\n");
        }
        for flag in unscoped_actionable {
            out.push_str(&format!(
                "- [{}] **{}**: {}\n",
                flag.id,
                flag.display_kind(),
                flag.message
            ));
        }
        for flag in unscoped_threads {
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

    // After all files are in place, rebuild the `cross_embedded_in` reverse
    // index so every atomic note knows which index notes embed it.
    rebuild_cross_embedded_in(&vault_synthetic)?;

    Ok(())
}

/// Rebuild the `cross_embedded_in` frontmatter field for every atomic note in
/// `synthetic_root`.
///
/// Scans all `.md` files for `![[stem#^block-id]]` embed links, builds a
/// reverse map `atomic_stem → [index_stems_that_embed_it]`, then updates only
/// the notes whose stored value differs from the computed one.
pub fn rebuild_cross_embedded_in(synthetic_root: &Path) -> Result<()> {
    use std::collections::HashMap;

    // Phase 1: forward scan — which index notes embed which atomics?
    let mut embedded_by: HashMap<String, Vec<String>> = HashMap::new();

    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = match crate::frontmatter::read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let embeds = crate::obsidian::extract_embeds(&content);
        if embeds.is_empty() {
            continue;
        }

        let index_stem = match entry.path().file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        for embed in embeds {
            if embed.is_block_anchor() {
                // Normalize to lowercase so case-insensitive Obsidian links
                // (e.g. ![[Sorting]] → sorting.md) are matched correctly.
                embedded_by
                    .entry(embed.file.to_lowercase())
                    .or_default()
                    .push(index_stem.clone());
            }
        }
    }

    // Deduplicate and sort for stable output.
    for list in embedded_by.values_mut() {
        list.sort();
        list.dedup();
    }

    // Phase 2: update atomic notes whose stored value differs.
    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let stem = match entry.path().file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let content = match crate::frontmatter::read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (yaml, body) = match crate::frontmatter::split_frontmatter(&content) {
            Some(pair) => pair,
            None => continue,
        };

        let mut fm: crate::frontmatter::NoteFrontmatter = match serde_yml::from_str(yaml) {
            Ok(fm) => fm,
            Err(_) => continue,
        };

        // Only atomic notes carry cross_embedded_in.
        if fm.block_id.is_none() {
            continue;
        }

        let new_value: Option<Vec<String>> = {
            let v = embedded_by.get(&stem.to_lowercase()).cloned().unwrap_or_default();
            if v.is_empty() { None } else { Some(v) }
        };

        // Only write when the value actually changes to avoid unnecessary
        // file rewrites.
        let unchanged = match (&fm.cross_embedded_in, &new_value) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };

        if !unchanged {
            fm.cross_embedded_in = new_value;
            crate::frontmatter::write_frontmatter(entry.path(), &fm, body)?;
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

/// Returns true for file extensions that are safe to read as UTF-8 text and
/// pass to the AI.  Binary formats (PDF, PPTX, images, …) return false and
/// are silently skipped during workspace assembly.
fn is_text_artifact_ext(ext: &str) -> bool {
    matches!(
        ext,
        "md" | "txt" | "html" | "htm" | "tex"
            | "py" | "java" | "rs" | "js" | "ts" | "jsx" | "tsx"
            | "c" | "cpp" | "h" | "hpp" | "cc" | "cxx"
            | "go" | "rb" | "swift" | "kt" | "kts"
            | "cs" | "fs" | "ml" | "mli" | "hs" | "lhs"
            | "r" | "rmd" | "sql" | "sh" | "bash" | "zsh" | "fish"
            | "yaml" | "yml" | "toml" | "json" | "xml" | "csv"
            | "ipynb"
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(root: &std::path::Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn atomic_note(topic: &str, slug: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: {topic}\ntitle: {slug}\n\
             block_id: {slug}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nBody text.\n\n^{slug}\n"
        )
    }

    fn index_note_with_embeds(topic: &str, embeds: &[&str]) -> String {
        let embed_lines: String = embeds
            .iter()
            .map(|s| format!("![[{}#^{}]]\n", s, s))
            .collect();
        format!(
            "---\ncsnotes_schema: 1\nkind: index\ntopic: {topic}\ntitle: {topic}\n\
             embeds: [{}]\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \n{embed_lines}",
            embeds
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", "),
            embed_lines = embed_lines,
        )
    }

    /// Basic rebuild: one index embeds two atomics.
    #[test]
    fn rebuild_cross_embedded_in_sets_embedders() {
        let tmp = TempDir::new().unwrap();
        let syn = tmp.path();

        write_file(syn, "cs/sorting.md", &atomic_note("cs", "sorting"));
        write_file(syn, "cs/searching.md", &atomic_note("cs", "searching"));
        write_file(syn, "cs/cs.md", &index_note_with_embeds("cs", &["sorting", "searching"]));

        rebuild_cross_embedded_in(syn).unwrap();

        let sorting = std::fs::read_to_string(syn.join("cs/sorting.md")).unwrap();
        assert!(
            sorting.contains("cross_embedded_in:"),
            "cross_embedded_in should be set on sorting"
        );
        assert!(sorting.contains("cs"), "sorting should list cs as embedder");

        let searching = std::fs::read_to_string(syn.join("cs/searching.md")).unwrap();
        assert!(searching.contains("cross_embedded_in:"));
        assert!(searching.contains("cs"));
    }

    /// Atomic not embedded anywhere should have cross_embedded_in: null / absent.
    #[test]
    fn rebuild_clears_cross_embedded_in_when_no_embedders() {
        let tmp = TempDir::new().unwrap();
        let syn = tmp.path();

        // Atomic with a stale cross_embedded_in value (e.g. from a previous session)
        write_file(
            syn,
            "cs/orphan.md",
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: orphan\n\
             block_id: orphan\ncontributing_sessions: []\ncontributing_sources: []\n\
             cross_embedded_in:\n  - old-index\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nBody.\n\n^orphan\n",
        );
        // No index note that embeds it
        write_file(syn, "cs/cs.md", &index_note_with_embeds("cs", &[]));

        rebuild_cross_embedded_in(syn).unwrap();

        let orphan = std::fs::read_to_string(syn.join("cs/orphan.md")).unwrap();
        // After rebuild, stale value should be cleared (None → not serialised)
        assert!(
            !orphan.contains("old-index"),
            "stale embedder should have been removed"
        );
    }

    /// Rebuild is idempotent: running it twice gives the same result.
    #[test]
    fn rebuild_cross_embedded_in_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let syn = tmp.path();

        write_file(syn, "cs/sorting.md", &atomic_note("cs", "sorting"));
        write_file(syn, "cs/cs.md", &index_note_with_embeds("cs", &["sorting"]));

        rebuild_cross_embedded_in(syn).unwrap();
        let after_first = std::fs::read_to_string(syn.join("cs/sorting.md")).unwrap();

        rebuild_cross_embedded_in(syn).unwrap();
        let after_second = std::fs::read_to_string(syn.join("cs/sorting.md")).unwrap();

        assert_eq!(
            after_first, after_second,
            "rebuild should be idempotent"
        );
    }

    /// Cross-topic embed: index in topic A embeds atomic from topic B.
    #[test]
    fn rebuild_handles_cross_topic_embeds() {
        let tmp = TempDir::new().unwrap();
        let syn = tmp.path();

        write_file(syn, "algorithms/sorting.md", &atomic_note("algorithms", "sorting"));
        // Index in a different topic embeds the atomic
        write_file(syn, "overview/overview.md", &index_note_with_embeds("overview", &["sorting"]));

        rebuild_cross_embedded_in(syn).unwrap();

        let sorting = std::fs::read_to_string(syn.join("algorithms/sorting.md")).unwrap();
        assert!(sorting.contains("cross_embedded_in:"));
        assert!(sorting.contains("overview"));
    }
}
