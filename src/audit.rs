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
use crate::frontmatter::{parse_frontmatter, NoteKind};
use crate::manifest::Manifest;
use crate::obsidian::{collect_all_block_ids, extract_block_ids, extract_embeds, extract_wikilinks};
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
                let path = workspace_root.join(&op.path);
                // The file must ALREADY exist — the AI wrote it.
                // `create_note` on an existing csnotes-frontmatter path is the
                // precondition failure (would clobber an existing note).
                if path.exists() {
                    // Check if it already has csnotes frontmatter
                    if let Ok(content) = std::fs::read_to_string(&path) {
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
                    let target_path = workspace_root.join(target);
                    if !target_path.exists() {
                        return Err(CsnotesError::EmbedInTargetMissing(target.clone()).into());
                    }
                    // The ![[...]] embed line must be present in the target index
                    if let Some(block_id) = &op.block_id {
                        let note_stem = Path::new(&op.path)
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        check_embed_line_present(
                            workspace_root,
                            target,
                            &note_stem,
                            block_id,
                        )?;
                    }
                }
            }

            Op::UpdateNote(op) => {
                let path = workspace_root.join(&op.path);
                if !path.exists() {
                    return Err(CsnotesError::UpdateNotePathMissing(op.path.clone()).into());
                }
                // Must have parseable csnotes frontmatter
                let content = std::fs::read_to_string(&path)?;
                parse_frontmatter(&content, &path)?;
            }

            // Structural ops — precondition checks are Phase 1+ responsibilities.
            // For Phase 0 we just verify they won't fire unexpectedly.
            _ => {
                // Structural ops are not yet executed; they'll fail gracefully
                // in ops/structural.rs if the AI emits one prematurely.
            }
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

    // 1. Collect all block IDs and check uniqueness
    let block_id_index = collect_all_block_ids(&synthetic_root)?;
    let mut id_counts: HashMap<String, Vec<String>> = HashMap::new();
    for (id, path) in &block_id_index {
        id_counts.entry(id.clone()).or_default().push(path.clone());
    }
    for (id, paths) in &id_counts {
        if paths.len() > 1 {
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
            let path = workspace_root.join(&op.path);
            if let Err(e) = parse_frontmatter_from_path(&path) {
                result.hard_violations.push(format!(
                    "note '{}' has invalid frontmatter after create_note: {}",
                    op.path, e
                ));
            }
        }
        if let Op::UpdateNote(op) = op {
            let path = workspace_root.join(&op.path);
            if let Err(e) = parse_frontmatter_from_path(&path) {
                result.hard_violations.push(format!(
                    "note '{}' has invalid frontmatter after update_note: {}",
                    op.path, e
                ));
            }
        }
    }

    // 3. Every atomic note must have block_id in frontmatter AND matching anchor
    if synthetic_root.exists() {
        for entry in walkdir::WalkDir::new(&synthetic_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
        {
            let content = match std::fs::read_to_string(entry.path()) {
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

/// Run the invariant suite against the vault directly (for `csnotes audit`).
/// Does not require a session report — checks structural consistency only.
pub fn audit_vault(vault_root: &Path, config: &crate::config::VaultConfig) -> Result<AuditResult> {
    let mut result = AuditResult::default();
    let synthetic_root = vault_root.join(&config.synthetic_dir);

    if !synthetic_root.exists() {
        result.soft_warnings.push(format!(
            "{} does not exist — no synthetic notes yet",
            config.synthetic_dir
        ));
        return Ok(result);
    }

    // Block ID uniqueness
    let block_id_index = collect_all_block_ids(&synthetic_root)?;
    let mut id_counts: HashMap<String, Vec<String>> = HashMap::new();
    for (id, path) in &block_id_index {
        id_counts.entry(id.clone()).or_default().push(path.clone());
    }
    for (id, paths) in &id_counts {
        if paths.len() > 1 {
            result.hard_violations.push(format!(
                "block ID '^{}' appears in multiple files: {}",
                id,
                paths.join(", ")
            ));
        }
    }

    // Frontmatter validity + atomic anchor check
    for entry in walkdir::WalkDir::new(&synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        match parse_frontmatter(&content, entry.path()) {
            Err(e) => {
                result.hard_violations.push(format!(
                    "invalid frontmatter in '{}': {}",
                    entry.path().display(), e
                ));
            }
            Ok(fm) => {
                if fm.kind == NoteKind::Atomic {
                    if let Some(id) = &fm.block_id {
                        if !extract_block_ids(&content).contains(id) {
                            result.hard_violations.push(format!(
                                "'{}': block_id '{}' in frontmatter but '^{}' not in body",
                                entry.path().display(), id, id
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
            .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
        {
            let content = match std::fs::read_to_string(entry.path()) {
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
                if !topic.contributing_sessions.iter().any(|c| {
                    c.course == contrib.course && c.date == contrib.date
                }) {
                    topic.contributing_sessions.push(contrib);
                }
            }

            // Update last_updated
            if fm.last_updated > topic.last_updated {
                topic.last_updated = fm.last_updated;
            }
        }
    }

    Ok(manifest)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_frontmatter_from_path(path: &Path) -> Result<crate::frontmatter::NoteFrontmatter> {
    let content = std::fs::read_to_string(path)?;
    parse_frontmatter(&content, path)
}

fn check_block_id_anchor(workspace_root: &Path, note_path: &str, block_id: &str) -> Result<()> {
    let path = workspace_root.join(note_path);
    let content = std::fs::read_to_string(&path)?;
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
    let content = std::fs::read_to_string(&path)?;
    let embeds = extract_embeds(&content);
    let found = embeds.iter().any(|e| {
        e.file == atomic_stem && e.block_id() == Some(block_id)
    });
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
    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = match std::fs::read_to_string(entry.path()) {
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
            if !note_exists_in_tree(search_root, &link.target) {
                result.hard_violations.push(format!(
                    "broken wikilink [[{}]] in '{}'",
                    link.target, source_rel
                ));
            }
        }

        for embed in extract_embeds(&content) {
            if !note_exists_in_tree(search_root, &embed.file) {
                result.hard_violations.push(format!(
                    "broken embed ![[{}]] in '{}'",
                    embed.file, source_rel
                ));
            }
        }
    }
    Ok(())
}

fn note_exists_in_tree(root: &Path, note_name: &str) -> bool {
    // Search for a .md file whose stem matches `note_name`.
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        if entry
            .path()
            .file_stem()
            .map_or(false, |s| s == note_name)
        {
            return true;
        }
    }
    false
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
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = match std::fs::read_to_string(entry.path()) {
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
                let content = std::fs::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                // Append anchor on its own line, preceded by a blank line if
                // the body doesn't already end with one.
                let trimmed = content.trim_end_matches('\n');
                let gap = if trimmed.ends_with('\n') { "\n" } else { "\n\n" };
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
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for embed in extract_embeds(&content) {
            embedded.insert(embed.file);
        }
    }

    // Find atomics not embedded anywhere
    for entry in walkdir::WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = match std::fs::read_to_string(entry.path()) {
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
                if !embedded.contains(&stem) {
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
