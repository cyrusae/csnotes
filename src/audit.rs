/// The invariant suite.
///
/// Runs against the workspace (pre-merge) during teardown, and against the
/// vault (read-only) when `csnotes audit` is invoked directly.
///
/// Hard violations → discard workspace (vault untouched).
/// Soft warnings   → logged, never block commit.
use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::error::CsnotesError;
use crate::frontmatter::{parse_frontmatter, read_note, NoteKind};
use crate::manifest::{Manifest, SourceKind};
use crate::obsidian::{extract_block_ids, extract_embeds, extract_wikilinks, find_collisions};
use crate::pathutil::safe_join;
use crate::report::{Op, SessionReport};

// ── Result types ──────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct AuditResult {
    pub hard_violations: Vec<String>,
    pub soft_warnings: Vec<String>,
}

impl AuditResult {
    pub fn is_clean(&self) -> bool {
        self.hard_violations.is_empty()
    }

    pub fn print(&self) {
        if self.hard_violations.is_empty() && self.soft_warnings.is_empty() {
            println!("  audit: clean");
            return;
        }
        for v in &self.hard_violations {
            eprintln!("  ERROR: {}", v);
        }
        for w in &self.soft_warnings {
            println!("  WARN:  {}", w);
        }
    }
}

// ── Pre-merge precondition pass (Phase 0) ─────────────────────────────────────

/// Check all op preconditions.  Pure read — no mutations.
/// Returns `Ok(())` if all preconditions hold; `Err(...)` on the first
/// failure.
pub fn precondition_pass(report: &SessionReport, workspace_root: &Path) -> Result<()> {
    for op in &report.operations {
        match op {
            Op::CreateNote(op) => {
                let path = safe_join(workspace_root, &op.path)?;
                // The file must ALREADY exist — the AI wrote it.
                // `create_note` on an existing csnotes-frontmatter path is the
                // precondition failure (would clobber an existing note).
                if path.exists() {
                    // Check if it already has csnotes frontmatter
                    if let Ok(content) = crate::frontmatter::read_note(&path) {
                        if crate::frontmatter::split_frontmatter(&content).is_some() {
                            return Err(CsnotesError::CreateNotePathExists(op.path.clone()).into());
                        }
                    }
                } else {
                    return Err(CsnotesError::UpdateNotePathMissing(op.path.clone()).into());
                }

                // block_id anchor must be present in the body
                if let Some(block_id) = &op.block_id {
                    check_block_id_anchor(workspace_root, &op.path, block_id)?;
                }

                // embed_in targets must exist
                for target in &op.embed_in {
                    let target_path = safe_join(workspace_root, target)?;
                    if !target_path.exists() {
                        return Err(CsnotesError::EmbedInTargetMissing(target.clone()).into());
                    }
                    // The ![[...]] embed line must be present in the target index
                    if let Some(block_id) = &op.block_id {
                        let note_stem = Path::new(&op.path)
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        check_embed_line_present(workspace_root, target, &note_stem, block_id)?;
                    }
                }
            }

            Op::UpdateNote(op) => {
                let path = safe_join(workspace_root, &op.path)?;
                if !path.exists() {
                    return Err(CsnotesError::UpdateNotePathMissing(op.path.clone()).into());
                }
                // Must have parseable csnotes frontmatter
                let content = crate::frontmatter::read_note(&path)?;
                parse_frontmatter(&content, &path)?;
            }

            Op::RenameTopic(op) => {
                // The source topic folder must exist in the workspace.
                // (The config synthetic_dir is not available here, so we scan
                // for the folder under any direct child of the workspace root
                // that looks like a synthetic dir.)
                let synthetic = workspace_root.join("_synthetic");
                let possible_dirs = [
                    safe_join(&synthetic, &op.from)?,
                    safe_join(workspace_root, &op.from)?,
                ];
                let from_exists = possible_dirs.iter().any(|d| d.exists());
                if !from_exists {
                    return Err(CsnotesError::RenameTopicSourceMissing(op.from.clone()).into());
                }
                let possible_dests = [
                    safe_join(&synthetic, &op.to)?,
                    safe_join(workspace_root, &op.to)?,
                ];
                if possible_dests.iter().any(|d| d.exists()) {
                    return Err(CsnotesError::RenameTopicDestExists(op.to.clone()).into());
                }
            }

            // Phase 4 structural ops — no precondition checks yet.
            _ => {}
        }
    }
    Ok(())
}

// ── Post-execution invariant suite (Phase 0) ──────────────────────────────────

/// Run the full invariant suite against the (post-execution) workspace.
/// Hard violations mean discard; soft warnings mean log-and-commit.
pub fn invariant_suite(
    workspace_root: &Path,
    synthetic_dir: &str,
    report: &SessionReport,
    _manifest: &Manifest,
) -> Result<AuditResult> {
    let mut result = AuditResult::default();
    let synthetic_root = workspace_root.join(synthetic_dir);

    // 1. Check block ID uniqueness across all notes
    if synthetic_root.exists() {
        for (id, paths) in block_id_collisions(&synthetic_root)? {
            result.hard_violations.push(format!(
                "block ID '^{}' appears in multiple files: {}",
                id,
                paths.join(", ")
            ));
        }
    }

    // 2. For each note declared in ops, verify schema-valid frontmatter
    for op in &report.operations {
        if let Op::CreateNote(op) = op {
            match safe_join(workspace_root, &op.path) {
                Ok(path) => {
                    if let Err(e) = parse_frontmatter_from_path(&path) {
                        result.hard_violations.push(format!(
                            "note '{}' has invalid frontmatter after create_note: {}",
                            op.path, e
                        ));
                    }
                }
                Err(e) => result.hard_violations.push(format!(
                    "unsafe path in create_note op '{}': {}",
                    op.path, e
                )),
            }
        }
        if let Op::UpdateNote(op) = op {
            match safe_join(workspace_root, &op.path) {
                Ok(path) => {
                    if let Err(e) = parse_frontmatter_from_path(&path) {
                        result.hard_violations.push(format!(
                            "note '{}' has invalid frontmatter after update_note: {}",
                            op.path, e
                        ));
                    }
                }
                Err(e) => result.hard_violations.push(format!(
                    "unsafe path in update_note op '{}': {}",
                    op.path, e
                )),
            }
        }
    }

    // 3. Every atomic note must have block_id in frontmatter AND matching anchor
    if synthetic_root.exists() {
        for entry in walkdir::WalkDir::new(&synthetic_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        {
            let content = match crate::frontmatter::read_note(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Ok(fm) = parse_frontmatter(&content, entry.path()) {
                if fm.kind == NoteKind::Atomic {
                    match &fm.block_id {
                        None => {
                            result.hard_violations.push(format!(
                                "atomic note '{}' has no block_id in frontmatter",
                                entry.path().display()
                            ));
                        }
                        Some(id) => {
                            let ids_in_body = extract_block_ids(&content);
                            if !ids_in_body.contains(id) {
                                result.hard_violations.push(format!(
                                    "atomic note '{}': block_id '{}' declared in frontmatter \
                                     but anchor '^{}' not found in body",
                                    entry.path().display(),
                                    id,
                                    id
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Every [[wikilink]] in _synthetic/ must resolve
    if synthetic_root.exists() {
        check_links_resolve(&synthetic_root, &synthetic_root, &mut result)?;
    }

    // 5. Soft: orphan atomics (not embedded by any index in their topic folder)
    check_orphan_atomics(&synthetic_root, &mut result)?;

    Ok(result)
}

// ── Direct vault audit (no report context) ────────────────────────────────────

const AI_CONVERSATION_SIDECAR_WORD_THRESHOLD: usize = 4_500;

/// Run the invariant suite against the vault directly (for `csnotes audit`).
/// Does not require a session report — checks structural consistency only.
pub fn audit_vault(
    vault_root: &Path,
    config: &crate::config::VaultConfig,
    manifest: &Manifest,
) -> Result<AuditResult> {
    let mut result = AuditResult::default();
    let synthetic_root = vault_root.join(&config.synthetic_dir);

    if !synthetic_root.exists() {
        result.soft_warnings.push(format!(
            "{} does not exist — no synthetic notes yet",
            config.synthetic_dir
        ));
    }

    // Block ID uniqueness
    for (id, paths) in block_id_collisions(&synthetic_root)? {
        result.hard_violations.push(format!(
            "block ID '^{}' appears in multiple files: {}",
            id,
            paths.join(", ")
        ));
    }

    // Frontmatter validity + atomic anchor check
    for entry in walkdir::WalkDir::new(&synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        match parse_frontmatter(&content, entry.path()) {
            Err(e) => {
                result.hard_violations.push(format!(
                    "invalid frontmatter in '{}': {}",
                    entry.path().display(),
                    e
                ));
            }
            Ok(fm) => {
                if fm.kind == NoteKind::Atomic {
                    if let Some(id) = &fm.block_id {
                        if !extract_block_ids(&content).contains(id) {
                            result.hard_violations.push(format!(
                                "'{}': block_id '{}' in frontmatter but '^{}' not in body",
                                entry.path().display(),
                                id,
                                id
                            ));
                        }
                    }
                }
            }
        }
    }

    // Link resolution
    check_links_resolve(&synthetic_root, &synthetic_root, &mut result)?;

    // Orphan atomics (soft)
    check_orphan_atomics(&synthetic_root, &mut result)?;

    // Sidecar nudge: AI conversation sources that are long but have no .json
    for (source_id, entry) in &manifest.sources {
        if entry.kind != SourceKind::AiConversation {
            continue;
        }
        let full_path = vault_root.join(&entry.path);
        let json_path = full_path.with_extension("json");
        if json_path.exists() {
            continue;
        }
        if let Ok(content) = read_note(&full_path) {
            let word_count = content.split_whitespace().count();
            if word_count >= AI_CONVERSATION_SIDECAR_WORD_THRESHOLD {
                result.soft_warnings.push(format!(
                    "AI conversation '{}' is ~{} words but has no sidecar — \
                     generate a .json alongside the .md to enable workspace indexing",
                    source_id, word_count
                ));
            }
        }
    }

    Ok(result)
}

// ── Reindex ───────────────────────────────────────────────────────────────────

/// Rebuild the manifest from frontmatter + filesystem.
/// This is the proof that the manifest is disposable — `audit --reindex`
/// must produce an identical manifest to the committed one.
pub fn reindex(vault_root: &Path, config: &crate::config::VaultConfig) -> Result<Manifest> {
    use crate::manifest::{ManifestConfig, TopicEntry};

    let synthetic_root = vault_root.join(&config.synthetic_dir);
    let manifest_config = ManifestConfig::from_vault_config(config);
    let mut manifest = Manifest::empty(vault_root.to_path_buf(), manifest_config);

    // Load existing sessions and sources from the old manifest if present
    // (reindex only rebuilds the topics map from frontmatter; sessions and
    // sources are registered by reconcile, not derivable from _synthetic/).
    if let Ok(old) = Manifest::load(vault_root) {
        manifest.sessions = old.sessions;
        manifest.sources = old.sources;
    }

    // Walk _synthetic/ and rebuild topics from frontmatter.
    if synthetic_root.exists() {
        for entry in walkdir::WalkDir::new(&synthetic_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        {
            let content = match crate::frontmatter::read_note(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let fm = match parse_frontmatter(&content, entry.path()) {
                Ok(fm) => fm,
                Err(_) => continue,
            };

            let rel_path = entry
                .path()
                .strip_prefix(vault_root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();

            let topic = manifest
                .topics
                .entry(fm.topic.clone())
                .or_insert_with(|| TopicEntry {
                    index_note: String::new(),
                    atomic_notes: vec![],
                    contributing_sessions: vec![],
                    contributing_sources: vec![],
                    pending_sessions: vec![],
                    last_updated: fm.last_updated,
                    open_flags: 0,
                    source_types: vec![],
                });

            match fm.kind {
                NoteKind::Index => topic.index_note = rel_path,
                NoteKind::Atomic => {
                    if !topic.atomic_notes.contains(&rel_path) {
                        topic.atomic_notes.push(rel_path);
                    }
                }
            }

            // Merge contributing sessions
            for contrib in fm.contributing_sessions {
                if !topic
                    .contributing_sessions
                    .iter()
                    .any(|c| c.course == contrib.course && c.date == contrib.date)
                {
                    topic.contributing_sessions.push(contrib);
                }
            }

            // Update last_updated
            if fm.last_updated > topic.last_updated {
                topic.last_updated = fm.last_updated;
            }
        }
    }

    // Compute pending_sessions: processed sessions whose processed_at is after
    // the topic's last_updated AND whose topics_updated lists this topic.
    // This detects genuine desync — e.g., a session was processed but the
    // topic's notes weren't updated (crash, manual edit, etc.).
    for (topic_name, topic_entry) in manifest.topics.iter_mut() {
        topic_entry.pending_sessions = manifest
            .sessions
            .iter()
            .filter(|(_, s)| {
                if let Some(processed_at) = s.processed_at {
                    processed_at > topic_entry.last_updated && s.topics_updated.contains(topic_name)
                } else {
                    false
                }
            })
            .map(|(id, _)| id.clone())
            .collect();
    }

    Ok(manifest)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn block_id_collisions(synthetic_root: &Path) -> Result<HashMap<String, Vec<String>>> {
    let mut files: Vec<(String, String)> = Vec::new();
    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        if let Ok(content) = crate::frontmatter::read_note(entry.path()) {
            let rel = entry
                .path()
                .strip_prefix(synthetic_root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            files.push((content, rel));
        }
    }
    Ok(find_collisions(
        files.iter().map(|(c, p)| (c.as_str(), p.as_str())),
    ))
}

fn parse_frontmatter_from_path(path: &Path) -> Result<crate::frontmatter::NoteFrontmatter> {
    let content = crate::frontmatter::read_note(path)?;
    parse_frontmatter(&content, path)
}

fn check_block_id_anchor(workspace_root: &Path, note_path: &str, block_id: &str) -> Result<()> {
    let path = workspace_root.join(note_path);
    let content = crate::frontmatter::read_note(&path)?;
    let ids = extract_block_ids(&content);
    if !ids.contains(&block_id.to_string()) {
        return Err(CsnotesError::BlockIdAnchorMissing {
            id: block_id.to_string(),
            path: note_path.to_string(),
        }
        .into());
    }
    Ok(())
}

fn check_embed_line_present(
    workspace_root: &Path,
    index_path: &str,
    atomic_stem: &str,
    block_id: &str,
) -> Result<()> {
    let path = workspace_root.join(index_path);
    if !path.exists() {
        return Ok(()); // target missing is caught separately
    }
    let content = crate::frontmatter::read_note(&path)?;
    let embeds = extract_embeds(&content);
    let found = embeds
        .iter()
        .any(|e| e.file == atomic_stem && e.block_id() == Some(block_id));
    if !found {
        return Err(CsnotesError::EmbedLineMissing {
            atomic: atomic_stem.to_string(),
            block_id: block_id.to_string(),
            index: index_path.to_string(),
        }
        .into());
    }
    Ok(())
}

fn check_links_resolve(
    synthetic_root: &Path,
    search_root: &Path,
    result: &mut AuditResult,
) -> Result<()> {
    // Pre-collect all note stems once so each link lookup is O(1) rather than
    // triggering a full WalkDir per link (which would be O(N²) across N notes).
    let known_stems = collect_note_stems(search_root);

    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let source_rel = entry
            .path()
            .strip_prefix(search_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .to_string();

        for link in extract_wikilinks(&content) {
            if !known_stems.contains(&link.target.to_lowercase()) {
                result.hard_violations.push(format!(
                    "broken wikilink [[{}]] in '{}'",
                    link.target, source_rel
                ));
            }
        }

        for embed in extract_embeds(&content) {
            if !known_stems.contains(&embed.file.to_lowercase()) {
                result.hard_violations.push(format!(
                    "broken embed ![[{}]] in '{}'",
                    embed.file, source_rel
                ));
            }
        }
    }
    Ok(())
}

/// Collect the lowercased file stems of all `.md` files under `root`.
/// Used to build a lookup set for link-resolution checks.
fn collect_note_stems(root: &Path) -> std::collections::HashSet<String> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase())
        })
        .collect()
}

// ── Fix plan ──────────────────────────────────────────────────────────────────

use std::path::PathBuf;

/// A single mechanical repair that `audit --fix --apply` can execute.
pub struct FixItem {
    /// Human-readable description for the dry-run preview.
    pub description: String,
    pub action: FixAction,
}

pub enum FixAction {
    /// Append a `^block_id` anchor to the end of the note body.
    AppendAnchor { path: PathBuf, block_id: String },
}

/// Collect all auto-repairable issues in `_synthetic/`.
///
/// Currently detects: atomic notes whose frontmatter declares a `block_id`
/// but whose body is missing the corresponding `^id` anchor.
pub fn collect_fixes(
    vault_root: &Path,
    config: &crate::config::VaultConfig,
) -> Result<Vec<FixItem>> {
    let synthetic_root = vault_root.join(&config.synthetic_dir);
    let mut fixes = Vec::new();

    if !synthetic_root.exists() {
        return Ok(fixes);
    }

    for entry in walkdir::WalkDir::new(&synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Unparseable frontmatter can't be auto-repaired
        let fm = match parse_frontmatter(&content, entry.path()) {
            Ok(fm) => fm,
            Err(_) => continue,
        };
        if fm.kind == NoteKind::Atomic {
            if let Some(id) = &fm.block_id {
                if !extract_block_ids(&content).contains(id) {
                    let rel = entry
                        .path()
                        .strip_prefix(vault_root)
                        .unwrap_or(entry.path());
                    fixes.push(FixItem {
                        description: format!(
                            "'{}': append '^{}' anchor to body",
                            rel.display(),
                            id
                        ),
                        action: FixAction::AppendAnchor {
                            path: entry.path().to_path_buf(),
                            block_id: id.clone(),
                        },
                    });
                }
            }
        }
    }

    Ok(fixes)
}

/// Execute a collected fix plan.  Returns the number of repairs applied.
pub fn apply_fixes(fixes: &[FixItem]) -> Result<usize> {
    use anyhow::Context;
    let mut applied = 0;
    for fix in fixes {
        match &fix.action {
            FixAction::AppendAnchor { path, block_id } => {
                let content = crate::frontmatter::read_note(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                // Append anchor on its own line, preceded by a blank line if
                // the body doesn't already end with one.
                let trimmed = content.trim_end_matches('\n');
                let gap = if trimmed.ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                let new_content = format!("{}{}\n^{}\n", trimmed, gap, block_id);
                std::fs::write(path, new_content)
                    .with_context(|| format!("writing {}", path.display()))?;
                applied += 1;
            }
        }
    }
    Ok(applied)
}

fn check_orphan_atomics(synthetic_root: &Path, result: &mut AuditResult) -> Result<()> {
    if !synthetic_root.exists() {
        return Ok(());
    }

    // Collect all embed targets across all notes
    let mut embedded: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for embed in extract_embeds(&content) {
            embedded.insert(embed.file.to_lowercase());
        }
    }

    // Find atomics not embedded anywhere
    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Ok(fm) = parse_frontmatter(&content, entry.path()) {
            if fm.kind == NoteKind::Atomic {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !embedded.contains(&stem.to_lowercase()) {
                    result.soft_warnings.push(format!(
                        "orphan atomic '{}' — not embedded by any index note",
                        entry.path().display()
                    ));
                }
            }
        }
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiBackend, SkillVariant, SnapshotMode, VaultConfig};
    use crate::frontmatter::ProvenanceDelta;
    use crate::manifest::{ManifestConfig, SessionEntry, SessionStatus};
    use crate::report::{
        CreateNoteOp, RenameTopicOp, ReportScope, ScopeKind, SessionReport, UpdateNoteOp,
    };
    use chrono::{DateTime, Utc};
    use tempfile::TempDir;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        if let Some(parent) = dir.join(name).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn make_vault_config() -> VaultConfig {
        serde_json::from_str("{}").unwrap()
    }

    fn make_empty_manifest(vault_root: &Path) -> Manifest {
        Manifest::empty(
            vault_root.to_path_buf(),
            ManifestConfig {
                raw_dir: "notes".into(),
                recordings_dir: "recordings".into(),
                artifacts_dir: "artifacts".into(),
                sources_dir: "sources".into(),
                synthetic_dir: "_synthetic".into(),
                generated_dir: "_generated".into(),
                filename_format: "{course}-{mm}-{dd}".into(),
                default_backend: AiBackend::Mock,
                skill_variant: SkillVariant::Claude,
                snapshot_mode: SnapshotMode::PreMerge,
            },
        )
    }

    fn make_report(ops: Vec<Op>) -> SessionReport {
        SessionReport {
            csnotes_report_schema: 1,
            run_id: "test-run".into(),
            backend: "mock".into(),
            started_at: Utc::now(),
            completed_at: Utc::now(),
            scope: ReportScope {
                kind: ScopeKind::Session,
                sessions: vec![],
                sources: vec![],
                topic: None,
            },
            operations: ops,
            review_flags: vec![],
        }
    }

    fn csnotes_note() -> &'static str {
        "---\ncsnotes_schema: 1\nkind: atomic\ntopic: test\ntitle: Test\n\
         block_id: test-01\ncontributing_sessions: []\ncontributing_sources: []\n\
         created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
         ---\nContent.\n"
    }

    fn create_op(path: &str) -> Op {
        Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: path.into(),
            title: "Test".into(),
            topic: "test".into(),
            block_id: None,
            embed_in: vec![],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })
    }

    fn update_op(path: &str) -> Op {
        Op::UpdateNote(UpdateNoteOp {
            path: path.into(),
            add_provenance: ProvenanceDelta::default(),
            sections: vec![],
            change_summary: "test".into(),
        })
    }

    fn rename_op(from: &str, to: &str) -> Op {
        Op::RenameTopic(RenameTopicOp {
            from: from.into(),
            to: to.into(),
            reason: "test".into(),
        })
    }

    // ── precondition_pass tests ───────────────────────────────────────────────

    #[test]
    fn precondition_pass_empty_report_ok() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_create_note_file_missing_errors() {
        let tmp = TempDir::new().unwrap();
        // AI is supposed to have written the file, but it's absent.
        let report = make_report(vec![create_op("note.md")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("note.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_create_note_existing_frontmatter_errors() {
        let tmp = TempDir::new().unwrap();
        // File already has csnotes frontmatter → would clobber an existing note.
        write(tmp.path(), "note.md", csnotes_note());
        let report = make_report(vec![create_op("note.md")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_create_note_fresh_file_passes() {
        let tmp = TempDir::new().unwrap();
        // File exists but has no frontmatter — AI wrote a fresh file, that's correct.
        write(tmp.path(), "note.md", "# Raw content, no frontmatter\n");
        let report = make_report(vec![create_op("note.md")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_create_note_embed_in_missing_errors() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n");
        let report = make_report(vec![Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: "note.md".into(),
            title: "Test".into(),
            topic: "test".into(),
            block_id: None,
            embed_in: vec!["index.md".into()],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("index.md"), "{err}");
    }

    #[test]
    fn precondition_update_note_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![update_op("note.md")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("note.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_update_note_exists_passes() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", csnotes_note());
        let report = make_report(vec![update_op("note.md")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_update_note_invalid_frontmatter_errors() {
        let tmp = TempDir::new().unwrap();
        // File exists but has no csnotes frontmatter — parse_frontmatter must fail.
        write(tmp.path(), "note.md", "# Just prose, no frontmatter\n");
        let report = make_report(vec![update_op("note.md")]);
        assert!(
            precondition_pass(&report, tmp.path()).is_err(),
            "expected error for update_note targeting file without valid csnotes frontmatter"
        );
    }

    #[test]
    fn precondition_rename_topic_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![rename_op("sorting", "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("sorting"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_rename_topic_source_exists_passes() {
        let tmp = TempDir::new().unwrap();
        // Source exists under _synthetic/, destination does not → should pass.
        std::fs::create_dir_all(tmp.path().join("_synthetic/sorting")).unwrap();
        let report = make_report(vec![rename_op("sorting", "algorithms")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_rename_topic_dest_exists_errors() {
        let tmp = TempDir::new().unwrap();
        // Source exists under _synthetic/; dest also exists → collision.
        std::fs::create_dir_all(tmp.path().join("_synthetic/sorting")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/algorithms")).unwrap();
        let report = make_report(vec![rename_op("sorting", "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("algorithms"), "{err}");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn crlf_frontmatter_parsed_correctly_in_audit() {
        // check_orphan_atomics reads file content; before the read_note fix,
        // CRLF line endings caused parse_frontmatter to fail, silently skipping
        // notes and producing no orphan warnings for unembedded atomics.
        let tmp = TempDir::new().unwrap();
        let crlf_atomic = "---\r\ncsnotes_schema: 1\r\nkind: atomic\r\ntopic: cs\r\n\
            title: Sorting\r\nblock_id: sort-01\r\ncontributing_sessions: []\r\n\
            contributing_sources: []\r\ncreated: \"2026-01-01T00:00:00Z\"\r\n\
            last_updated: \"2026-01-01T00:00:00Z\"\r\n---\r\nSorting content.\r\n";
        write(tmp.path(), "sorting.md", crlf_atomic);

        let mut result = AuditResult::default();
        check_orphan_atomics(tmp.path(), &mut result).unwrap();
        // The atomic is not embedded by anything, so it must appear as an orphan.
        // If CRLF broke parse_frontmatter, kind would be unknown and no warning
        // would be emitted — the assertion would fail.
        assert_eq!(result.soft_warnings.len(), 1);
        assert!(result.soft_warnings[0].contains("orphan atomic"));
    }

    #[test]
    fn collect_note_stems_lowercases_all() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "Sorting.md", "");
        write(tmp.path(), "BFS.md", "");
        write(tmp.path(), "not-a-note.txt", "");
        let stems = collect_note_stems(tmp.path());
        assert!(stems.contains("sorting"));
        assert!(stems.contains("bfs"));
        assert!(!stems.contains("not-a-note"));
    }

    #[test]
    fn check_links_resolve_accepts_case_insensitive_links() {
        let tmp = TempDir::new().unwrap();
        // Index note links to "Sorting" (capital S); the file on disk is "sorting.md".
        write(tmp.path(), "sorting.md", "# Sorting\n");
        write(tmp.path(), "index.md", "See [[Sorting]].\n");
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &mut result).unwrap();
        assert!(
            result.hard_violations.is_empty(),
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn check_links_resolve_flags_broken_link() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.md", "See [[nonexistent]].\n");
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &mut result).unwrap();
        assert_eq!(result.hard_violations.len(), 1);
        assert!(result.hard_violations[0].contains("nonexistent"));
    }

    #[test]
    fn check_links_resolve_flags_broken_embed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.md", "![[ghost-atomic#^id]]\n");
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &mut result).unwrap();
        assert_eq!(result.hard_violations.len(), 1);
        assert!(result.hard_violations[0].contains("ghost-atomic"));
    }

    #[test]
    fn audit_vault_broken_embed_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/index.md", "![[ghost-atomic#^id]]\n");
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result
                .hard_violations
                .iter()
                .any(|v| v.contains("ghost-atomic")),
            "expected hard violation for broken embed, got: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn invariant_suite_broken_embed_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/index.md", "![[ghost-atomic#^id]]\n");
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        assert!(
            result
                .hard_violations
                .iter()
                .any(|v| v.contains("ghost-atomic")),
            "expected hard violation for broken embed, got: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn check_orphan_atomics_embedded_atomic_not_warned() {
        let tmp = TempDir::new().unwrap();
        // An atomic embedded by an index note must NOT be flagged as an orphan.
        write(tmp.path(), "sorting.md", csnotes_note());
        write(tmp.path(), "index.md", "![[sorting#^test-01]]\n");
        let mut result = AuditResult::default();
        check_orphan_atomics(tmp.path(), &mut result).unwrap();
        assert!(
            result.soft_warnings.is_empty(),
            "embedded atomic should not be flagged as orphan: {:?}",
            result.soft_warnings
        );
    }

    // ── audit_vault caller tests ──────────────────────────────────────────────

    #[test]
    fn audit_vault_warns_when_synthetic_dir_absent() {
        let tmp = TempDir::new().unwrap();
        // No _synthetic/ directory created.
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(result.hard_violations.is_empty());
        assert_eq!(result.soft_warnings.len(), 1);
        assert!(
            result.soft_warnings[0].contains("does not exist"),
            "{:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn audit_vault_broken_wikilink_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        // _synthetic/ exists but the link target does not.
        write(tmp.path(), "_synthetic/index.md", "See [[missing-note]].\n");
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result
                .hard_violations
                .iter()
                .any(|v| v.contains("missing-note")),
            "expected hard violation for missing-note, got: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn audit_vault_orphan_atomic_is_soft_warning() {
        let tmp = TempDir::new().unwrap();
        // Atomic note not embedded by any index → orphan warning.
        write(tmp.path(), "_synthetic/cs/sorting.md", csnotes_note());
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result.soft_warnings.iter().any(|w| w.contains("orphan")),
            "expected orphan warning, got: {:?}",
            result.soft_warnings
        );
    }

    // ── invariant_suite caller tests ──────────────────────────────────────────

    #[test]
    fn invariant_suite_clean_when_no_synthetic_dir() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        // Both check_links_resolve and check_orphan_atomics guard on dir existence.
        assert!(
            result.is_clean(),
            "expected clean result, got: {:?}",
            result
        );
    }

    #[test]
    fn invariant_suite_broken_wikilink_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/index.md", "See [[ghost-note]].\n");
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        assert!(
            result
                .hard_violations
                .iter()
                .any(|v| v.contains("ghost-note")),
            "expected hard violation for ghost-note, got: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn invariant_suite_orphan_atomic_is_soft_warning() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/cs/sorting.md", csnotes_note());
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        assert!(
            result.soft_warnings.iter().any(|w| w.contains("orphan")),
            "expected orphan warning, got: {:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn invariant_suite_index_note_without_block_id_is_not_flagged() {
        let tmp = TempDir::new().unwrap();
        // Index notes never carry block_id.  The block_id check in invariant_suite
        // must be guarded by `fm.kind == NoteKind::Atomic`, not `!=`.
        write(
            tmp.path(),
            "_synthetic/cs/index.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: cs\ntitle: CS\n\
             contributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\n# CS\n",
        );
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        assert!(
            !result
                .hard_violations
                .iter()
                .any(|v| v.contains("block_id")),
            "index note must not trigger block_id violation: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn collect_note_stems_lowercases_mixed_case_files() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "Sorting.md", "# Sorting\n");
        let stems = collect_note_stems(tmp.path());
        // collect_note_stems lowercases everything so case-insensitive lookups work.
        assert!(stems.contains("sorting"));
        assert!(!stems.contains("mergesort"));
    }

    // Helpers for the tests below -------------------------------------------------

    fn clean_note(block_id: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nContent.\n\n^{block_id}\n"
        )
    }

    fn note_missing_anchor(block_id: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nContent.\n"
        )
    }

    fn create_op_with_block_id(path: &str, block_id: &str) -> Op {
        Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: path.into(),
            title: "Test".into(),
            topic: "test".into(),
            block_id: Some(block_id.into()),
            embed_in: vec![],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })
    }

    // ── duplicate block IDs ───────────────────────────────────────────────────

    #[test]
    fn invariant_suite_duplicate_block_ids_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/cs/note-a.md", &clean_note("dup-01"));
        write(tmp.path(), "_synthetic/cs/note-b.md", &clean_note("dup-01"));
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        assert!(
            result.hard_violations.iter().any(|v| v.contains("dup-01")),
            "expected duplicate block_id violation, got: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn audit_vault_duplicate_block_ids_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/cs/note-a.md", &clean_note("dup-01"));
        write(tmp.path(), "_synthetic/cs/note-b.md", &clean_note("dup-01"));
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result.hard_violations.iter().any(|v| v.contains("dup-01")),
            "expected duplicate block_id violation, got: {:?}",
            result.hard_violations
        );
    }

    // ── audit_vault atomic anchor check ──────────────────────────────────────

    #[test]
    fn audit_vault_atomic_missing_anchor_is_hard_violation() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &note_missing_anchor("sort-01"),
        );
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result.hard_violations.iter().any(|v| v.contains("sort-01")),
            "expected hard violation for missing anchor, got: {:?}",
            result.hard_violations
        );
    }

    #[test]
    fn audit_vault_atomic_with_anchor_is_clean() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        write(
            tmp.path(),
            "_synthetic/cs/index.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: cs\ntitle: CS\n\
             contributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\n![[sorting#^sort-01]]\n",
        );
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result.hard_violations.is_empty(),
            "{:?}",
            result.hard_violations
        );
    }

    // ── check_block_id_anchor (via precondition_pass) ─────────────────────────

    #[test]
    fn precondition_create_note_block_id_anchor_present_passes() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n\n^my-id\n");
        let report = make_report(vec![create_op_with_block_id("note.md", "my-id")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_create_note_block_id_anchor_missing_errors() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content without anchor\n");
        let report = make_report(vec![create_op_with_block_id("note.md", "my-id")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("my-id"), "{err}");
    }

    // ── check_embed_line_present (via precondition_pass) ─────────────────────

    #[test]
    fn precondition_embed_line_present_passes_when_embed_in_index() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n\n^my-id\n");
        write(tmp.path(), "index.md", "![[note#^my-id]]\n");
        let report = make_report(vec![Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: "note.md".into(),
            title: "Test".into(),
            topic: "test".into(),
            block_id: Some("my-id".into()),
            embed_in: vec!["index.md".into()],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_embed_line_missing_from_index_errors() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n\n^my-id\n");
        // Index exists but doesn't contain the ![[note#^my-id]] line.
        write(tmp.path(), "index.md", "Some unrelated content.\n");
        let report = make_report(vec![Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: "note.md".into(),
            title: "Test".into(),
            topic: "test".into(),
            block_id: Some("my-id".into()),
            embed_in: vec!["index.md".into()],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("my-id") || err.to_string().contains("note"),
            "{err}"
        );
    }

    #[test]
    fn precondition_embed_wrong_file_same_block_id_errors() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n\n^my-id\n");
        // Index has the correct block_id but the wrong file stem.
        // check_embed_line_present uses `file == stem && block_id() == id`;
        // if `&&` were `||`, the block_id match alone would falsely satisfy it.
        write(tmp.path(), "index.md", "![[other-note#^my-id]]\n");
        let report = make_report(vec![Op::CreateNote(CreateNoteOp {
            kind: NoteKind::Atomic,
            path: "note.md".into(),
            title: "Test".into(),
            topic: "test".into(),
            block_id: Some("my-id".into()),
            embed_in: vec!["index.md".into()],
            provenance: ProvenanceDelta::default(),
            change_summary: "test".into(),
        })]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("my-id") || err.to_string().contains("note"),
            "expected embed-line-missing error, got: {err}"
        );
    }

    // ── collect_fixes / apply_fixes ───────────────────────────────────────────

    #[test]
    fn collect_fixes_returns_empty_when_no_synthetic_dir() {
        let tmp = TempDir::new().unwrap();
        let fixes = collect_fixes(tmp.path(), &make_vault_config()).unwrap();
        assert!(fixes.is_empty());
    }

    #[test]
    fn collect_fixes_returns_fix_for_atomic_missing_anchor() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &note_missing_anchor("sort-01"),
        );
        let fixes = collect_fixes(tmp.path(), &make_vault_config()).unwrap();
        assert_eq!(
            fixes.len(),
            1,
            "expected one fix, got: {:?}",
            fixes.iter().map(|f| &f.description).collect::<Vec<_>>()
        );
        assert!(
            fixes[0].description.contains("sort-01"),
            "{}",
            fixes[0].description
        );
    }

    #[test]
    fn collect_fixes_returns_no_fix_when_anchor_present() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let fixes = collect_fixes(tmp.path(), &make_vault_config()).unwrap();
        assert!(
            fixes.is_empty(),
            "no fix needed for note with anchor present"
        );
    }

    #[test]
    fn apply_fixes_appends_anchor_and_reports_count() {
        let tmp = TempDir::new().unwrap();
        let note_path = tmp.path().join("note.md");
        std::fs::write(&note_path, "Content.\n").unwrap();
        let fixes = vec![FixItem {
            description: "test fix".into(),
            action: FixAction::AppendAnchor {
                path: note_path.clone(),
                block_id: "sort-01".into(),
            },
        }];
        let count = apply_fixes(&fixes).unwrap();
        assert_eq!(count, 1);
        let content = std::fs::read_to_string(&note_path).unwrap();
        assert!(
            content.contains("^sort-01"),
            "anchor not appended:\n{content}"
        );
    }

    #[test]
    fn apply_fixes_counts_all_applied() {
        let tmp = TempDir::new().unwrap();
        let path_a = tmp.path().join("a.md");
        let path_b = tmp.path().join("b.md");
        std::fs::write(&path_a, "Content A.\n").unwrap();
        std::fs::write(&path_b, "Content B.\n").unwrap();
        let fixes = vec![
            FixItem {
                description: "fix a".into(),
                action: FixAction::AppendAnchor {
                    path: path_a.clone(),
                    block_id: "id-a".into(),
                },
            },
            FixItem {
                description: "fix b".into(),
                action: FixAction::AppendAnchor {
                    path: path_b.clone(),
                    block_id: "id-b".into(),
                },
            },
        ];
        let count = apply_fixes(&fixes).unwrap();
        assert_eq!(count, 2);
        assert!(std::fs::read_to_string(&path_a).unwrap().contains("^id-a"));
        assert!(std::fs::read_to_string(&path_b).unwrap().contains("^id-b"));
    }

    // ── reindex ───────────────────────────────────────────────────────────────

    #[test]
    fn reindex_collects_atomic_notes_for_topic() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest
            .topics
            .get("cs")
            .expect("cs topic should exist after reindex");
        assert_eq!(topic.atomic_notes.len(), 1);
        assert!(
            topic.atomic_notes[0].contains("sorting.md"),
            "{:?}",
            topic.atomic_notes
        );
    }

    // ── reindex contributing session deduplication ────────────────────────────

    fn note_with_contrib(block_id: &str, course: &str, date: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Note\n\
             block_id: {block_id}\n\
             contributing_sessions:\n- course: {course}\n  date: \"{date}\"\n  relationship: introduced\n\
             contributing_sources: []\ncreated: \"2026-01-01T00:00:00Z\"\n\
             last_updated: \"2026-01-01T00:00:00Z\"\n---\nContent.\n\n^{block_id}\n"
        )
    }

    #[test]
    fn reindex_deduplicates_identical_contributing_sessions() {
        let tmp = TempDir::new().unwrap();
        // Two notes both carrying the same (course, date) contrib — must appear once.
        write(
            tmp.path(),
            "_synthetic/cs/note-a.md",
            &note_with_contrib("id-a", "CS101", "2026-01-10"),
        );
        write(
            tmp.path(),
            "_synthetic/cs/note-b.md",
            &note_with_contrib("id-b", "CS101", "2026-01-10"),
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert_eq!(
            topic.contributing_sessions.len(),
            1,
            "identical session must appear only once: {:?}",
            topic.contributing_sessions
        );
    }

    #[test]
    fn reindex_keeps_distinct_contributing_sessions_by_date() {
        let tmp = TempDir::new().unwrap();
        // Same course, different dates — both must be kept.
        // If the dedup predicate used `||` instead of `&&`, the date-only
        // difference would be ignored and one entry would be dropped.
        write(
            tmp.path(),
            "_synthetic/cs/note-a.md",
            &note_with_contrib("id-a", "CS101", "2026-01-10"),
        );
        write(
            tmp.path(),
            "_synthetic/cs/note-b.md",
            &note_with_contrib("id-b", "CS101", "2026-01-17"),
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert_eq!(
            topic.contributing_sessions.len(),
            2,
            "sessions with same course but different date must both be kept: {:?}",
            topic.contributing_sessions
        );
    }

    // ── reindex pending_sessions ──────────────────────────────────────────────

    fn write_manifest_with_session(
        vault_root: &Path,
        session_id: &str,
        processed_at: Option<DateTime<Utc>>,
        topics_updated: Vec<String>,
    ) {
        let mut m = make_empty_manifest(vault_root);
        m.sessions.insert(
            session_id.to_string(),
            SessionEntry {
                date: chrono::NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
                course: "CS101".into(),
                filename_format: "{course}-{mm}-{dd}".into(),
                raw_note: "notes/cs101-01-10.md".into(),
                recording_exports: vec![],
                artifacts: vec![],
                recording_missing: false,
                status: SessionStatus::Processed,
                processed_at,
                topics_updated,
            },
        );
        m.save(vault_root).unwrap();
    }

    #[test]
    fn reindex_pending_sessions_includes_post_topic_session() {
        let tmp = TempDir::new().unwrap();
        // Note last_updated Jan 01; session processed Jun 01 (after).  Session
        // lists "cs" → must appear in pending_sessions.
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let processed = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        write_manifest_with_session(tmp.path(), "sess-1", Some(processed), vec!["cs".into()]);
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert!(
            topic.pending_sessions.contains(&"sess-1".to_string()),
            "session processed after last_updated must be pending: {:?}",
            topic.pending_sessions
        );
    }

    #[test]
    fn reindex_pending_sessions_excludes_pre_topic_session() {
        let tmp = TempDir::new().unwrap();
        // Note last_updated Jun 01; session processed Jan 01 (before).
        // Must NOT be in pending_sessions.
        let late_note = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Late\n\
                         block_id: late-01\ncontributing_sessions: []\ncontributing_sources: []\n\
                         created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-06-01T00:00:00Z\"\n\
                         ---\nContent.\n\n^late-01\n";
        write(tmp.path(), "_synthetic/cs/late.md", late_note);
        let processed = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        write_manifest_with_session(tmp.path(), "old-sess", Some(processed), vec!["cs".into()]);
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert!(
            !topic.pending_sessions.contains(&"old-sess".to_string()),
            "session processed before last_updated must not be pending: {:?}",
            topic.pending_sessions
        );
    }

    #[test]
    fn reindex_pending_sessions_excludes_session_not_for_topic() {
        let tmp = TempDir::new().unwrap();
        // Session processed after the topic's last_updated, but topics_updated
        // lists a different topic.  Must NOT appear in cs's pending_sessions.
        write(
            tmp.path(),
            "_synthetic/cs/sorting.md",
            &clean_note("sort-01"),
        );
        let processed = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        write_manifest_with_session(
            tmp.path(),
            "wrong-topic-sess",
            Some(processed),
            vec!["other-topic".into()],
        );
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        assert!(
            !topic
                .pending_sessions
                .contains(&"wrong-topic-sess".to_string()),
            "session for different topic must not appear in cs pending_sessions: {:?}",
            topic.pending_sessions
        );
    }

    // ── sidecar nudge (audit_vault) ───────────────────────────────────────────

    fn make_ai_source_manifest(vault_root: &Path, source_id: &str, path: &str) -> Manifest {
        use crate::manifest::{SourceEntry, SourceStatus};
        let mut m = make_empty_manifest(vault_root);
        m.sources.insert(
            source_id.to_string(),
            SourceEntry {
                path: path.to_string(),
                kind: SourceKind::AiConversation,
                status: SourceStatus::Unprocessed,
                last_processed_at: None,
                heading_scheme: vec![],
                topics_updated: vec![],
                summary: None,
                tags: vec![],
                courses: vec![],
            },
        );
        m
    }

    #[test]
    fn audit_vault_warns_when_long_ai_conversation_has_no_sidecar() {
        let tmp = TempDir::new().unwrap();
        // Write a long AI conversation (well over the 4 500-word threshold).
        let body = "word ".repeat(4_600);
        write(tmp.path(), "sources/AI-Conversations/Gemini/chat.md", &body);
        // No chat.json alongside it.
        let manifest = make_ai_source_manifest(
            tmp.path(),
            "AI-Conversations/Gemini/chat",
            "sources/AI-Conversations/Gemini/chat.md",
        );
        let result = audit_vault(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert!(
            result
                .soft_warnings
                .iter()
                .any(|w| w.contains("AI-Conversations/Gemini/chat")),
            "expected sidecar nudge warning, got: {:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn audit_vault_no_sidecar_warn_when_json_present() {
        let tmp = TempDir::new().unwrap();
        let body = "word ".repeat(2_100);
        write(tmp.path(), "sources/AI-Conversations/Gemini/chat.md", &body);
        // Sidecar exists — no warning expected.
        write(
            tmp.path(),
            "sources/AI-Conversations/Gemini/chat.json",
            "{}",
        );
        let manifest = make_ai_source_manifest(
            tmp.path(),
            "AI-Conversations/Gemini/chat",
            "sources/AI-Conversations/Gemini/chat.md",
        );
        let result = audit_vault(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert!(
            !result.soft_warnings.iter().any(|w| w.contains("sidecar")),
            "no sidecar warning expected when .json present: {:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn audit_vault_no_sidecar_warn_for_short_ai_conversation() {
        let tmp = TempDir::new().unwrap();
        // Short conversation — under threshold, no nudge.
        let body = "word ".repeat(500);
        write(tmp.path(), "sources/AI-Conversations/Gemini/chat.md", &body);
        let manifest = make_ai_source_manifest(
            tmp.path(),
            "AI-Conversations/Gemini/chat",
            "sources/AI-Conversations/Gemini/chat.md",
        );
        let result = audit_vault(tmp.path(), &make_vault_config(), &manifest).unwrap();
        assert!(
            !result.soft_warnings.iter().any(|w| w.contains("sidecar")),
            "no sidecar warning expected for short conversation: {:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn reindex_tracks_last_updated_as_max() {
        let tmp = TempDir::new().unwrap();
        // Two notes in the same topic with different last_updated timestamps.
        let early = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Early\n\
                     block_id: early-01\ncontributing_sessions: []\ncontributing_sources: []\n\
                     created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
                     ---\nContent.\n\n^early-01\n";
        let late = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: Late\n\
                     block_id: late-01\ncontributing_sessions: []\ncontributing_sources: []\n\
                     created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-06-01T00:00:00Z\"\n\
                     ---\nContent.\n\n^late-01\n";
        write(tmp.path(), "_synthetic/cs/early.md", early);
        write(tmp.path(), "_synthetic/cs/late.md", late);
        let manifest = reindex(tmp.path(), &make_vault_config()).unwrap();
        let topic = manifest.topics.get("cs").expect("cs topic should exist");
        // last_updated must be the later of the two timestamps.
        assert_eq!(
            topic.last_updated.format("%Y-%m-%d").to_string(),
            "2026-06-01",
            "expected max last_updated, got: {}",
            topic.last_updated
        );
    }
}
