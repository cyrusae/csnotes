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

// ── Pre-merge precondition pass ───────────────────────────────────────────────

/// Check all op preconditions.  Pure read — no mutations.
/// Returns `Ok(())` if all preconditions hold; `Err(...)` on the first failure.
#[allow(dead_code)]
pub fn precondition_pass(report: &SessionReport, workspace_root: &Path) -> Result<()> {
    precondition_pass_ops(&report.operations, workspace_root)
}

/// Same as `precondition_pass` but operates on an arbitrary slice of ops.
/// Used by `csnotes commit` to check only the not-yet-committed tail.
pub fn precondition_pass_ops(ops: &[Op], workspace_root: &Path) -> Result<()> {
    for op in ops {
        match op {
            Op::CreateNote(op) => {
                let path = safe_join(workspace_root, &op.path)?;
                if op.kind == crate::frontmatter::NoteKind::Journal {
                    // Journal entries: AI writes the full file; CLI just copies
                    // it to the vault during merge-back without stamping frontmatter.
                    if !path.exists() {
                        return Err(CsnotesError::CreateNotePathMissing(op.path.clone()).into());
                    }
                } else {
                    // Atomic/index: AI writes the body (no frontmatter); CLI stamps it.
                    if path.exists() {
                        if let Ok(content) = crate::frontmatter::read_note(&path) {
                            if crate::frontmatter::split_frontmatter(&content).is_some() {
                                return Err(
                                    CsnotesError::CreateNotePathExists(op.path.clone()).into()
                                );
                            }
                        }
                    } else {
                        return Err(CsnotesError::CreateNotePathMissing(op.path.clone()).into());
                    }

                    if let Some(block_id) = &op.block_id {
                        check_block_id_anchor(workspace_root, &op.path, block_id)?;
                    }

                    for target in &op.embed_in {
                        let target_path = safe_join(workspace_root, target)?;
                        if !target_path.exists() {
                            return Err(CsnotesError::EmbedInTargetMissing(target.clone()).into());
                        }
                    }
                }
            }

            Op::UpdateNote(op) => {
                let path = safe_join(workspace_root, &op.path)?;
                if !path.exists() {
                    return Err(CsnotesError::UpdateNotePathMissing(op.path.clone()).into());
                }
                let content = crate::frontmatter::read_note(&path)?;
                parse_frontmatter(&content, &path)?;
            }

            Op::RenameTopic(op) => {
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

            Op::RenameAtomic(op) => {
                let from_abs = safe_join(workspace_root, &op.path)?;
                if !from_abs.exists() {
                    return Err(CsnotesError::RenameAtomicSourceMissing(op.path.clone()).into());
                }
                let from_p = std::path::Path::new(&op.path);
                let topic = from_p
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let synthetic = workspace_root.join("_synthetic");
                let to_abs = safe_join(&synthetic, topic)?.join(format!("{}.md", op.new_slug));
                if to_abs.exists() {
                    return Err(CsnotesError::RenameAtomicDestExists {
                        slug: op.new_slug.clone(),
                        topic: topic.to_string(),
                    }
                    .into());
                }
            }

            Op::MoveAtomic(op) => {
                let from_abs = safe_join(workspace_root, &op.from_path)?;
                if !from_abs.exists() {
                    return Err(CsnotesError::MoveAtomicSourceMissing(op.from_path.clone()).into());
                }
                let synthetic = workspace_root.join("_synthetic");
                let to_dir = safe_join(&synthetic, &op.to_topic)?;
                if !to_dir.exists() {
                    return Err(CsnotesError::MoveAtomicTargetMissing(op.to_topic.clone()).into());
                }
                let slug = std::path::Path::new(&op.from_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let to_abs = to_dir.join(format!("{}.md", slug));
                if to_abs.exists() {
                    return Err(CsnotesError::MoveAtomicDestExists {
                        slug: slug.to_string(),
                        topic: op.to_topic.clone(),
                    }
                    .into());
                }
            }

            Op::PromoteAtomic(op) => {
                let from_abs = safe_join(workspace_root, &op.from_path)?;
                if !from_abs.exists() {
                    return Err(
                        CsnotesError::PromoteAtomicSourceMissing(op.from_path.clone()).into(),
                    );
                }
                let synthetic = workspace_root.join("_synthetic");
                let to_dir = safe_join(&synthetic, &op.to_topic)?;
                if to_dir.exists() {
                    return Err(CsnotesError::PromoteAtomicTargetExists(op.to_topic.clone()).into());
                }
            }

            Op::DemoteTopic(op) => {
                if op.from_topic == op.into_topic {
                    return Err(CsnotesError::DemoteTopicSameTarget(op.from_topic.clone()).into());
                }
                let synthetic = workspace_root.join("_synthetic");
                let from_dir = safe_join(&synthetic, &op.from_topic)?;
                if !from_dir.exists() {
                    return Err(
                        CsnotesError::DemoteTopicSourceMissing(op.from_topic.clone()).into(),
                    );
                }
                let into_dir = safe_join(&synthetic, &op.into_topic)?;
                if !into_dir.exists() {
                    return Err(
                        CsnotesError::DemoteTopicTargetMissing(op.into_topic.clone()).into(),
                    );
                }
            }

            Op::MergeTopics(op) => {
                let synthetic = workspace_root.join("_synthetic");
                let into_dir = safe_join(&synthetic, &op.into)?;

                let mut landing: std::collections::HashSet<String> = if into_dir.exists() {
                    std::fs::read_dir(&into_dir)
                        .map(|rd| {
                            rd.filter_map(|e| e.ok())
                                .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
                                .map(|e| e.file_name().to_string_lossy().to_string())
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    std::collections::HashSet::new()
                };

                for from_topic in &op.from {
                    if from_topic == &op.into {
                        continue;
                    }
                    let from_dir = safe_join(&synthetic, from_topic)?;
                    if !from_dir.exists() {
                        return Err(
                            CsnotesError::MergeTopicsSourceMissing(from_topic.clone()).into()
                        );
                    }
                    for entry in std::fs::read_dir(&from_dir)
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
                    {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if !landing.insert(name.clone()) {
                            return Err(CsnotesError::MergeTopicsFileConflict(name).into());
                        }
                    }
                }
            }

            Op::SplitTopic(op) => {
                let synthetic = workspace_root.join("_synthetic");
                let from_dir = safe_join(&synthetic, &op.from)?;
                if !from_dir.exists() {
                    return Err(CsnotesError::SplitTopicSourceMissing(op.from.clone()).into());
                }
                for target in &op.into {
                    if target.topic == op.from {
                        continue;
                    }
                    let target_dir = safe_join(&synthetic, &target.topic)?;
                    for slug in &target.atomics {
                        let from_note = from_dir.join(format!("{}.md", slug));
                        if !from_note.exists() {
                            return Err(CsnotesError::SplitTopicAtomicMissing {
                                slug: slug.clone(),
                                topic: op.from.clone(),
                            }
                            .into());
                        }
                        if target_dir.exists() {
                            let to_note = target_dir.join(format!("{}.md", slug));
                            if to_note.exists() {
                                return Err(CsnotesError::SplitTopicDestExists {
                                    slug: slug.clone(),
                                    topic: target.topic.clone(),
                                }
                                .into());
                            }
                        }
                    }
                }
            }

            Op::SetEmbed(op) => {
                let index_abs = safe_join(workspace_root, &op.index_path)?;
                if !index_abs.exists() {
                    return Err(CsnotesError::SetEmbedIndexMissing(op.index_path.clone()).into());
                }
                let atomic_abs = safe_join(workspace_root, &op.atomic_path)?;
                if !atomic_abs.exists() {
                    return Err(CsnotesError::SetEmbedAtomicMissing(op.atomic_path.clone()).into());
                }
                let raw = std::fs::read_to_string(&atomic_abs)?;
                if let Some((yaml, _)) = crate::frontmatter::split_frontmatter(&raw) {
                    let fm: Result<crate::frontmatter::NoteFrontmatter, _> =
                        serde_yml::from_str(yaml);
                    if fm.map(|f| f.block_id.is_none()).unwrap_or(false) {
                        return Err(
                            CsnotesError::SetEmbedAtomicNoBlockId(op.atomic_path.clone()).into(),
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

// ── Post-execution invariant suite ────────────────────────────────────────────

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

    if synthetic_root.exists() {
        for (id, paths) in block_id_collisions(&synthetic_root)? {
            result.hard_violations.push(format!(
                "block ID '^{}' appears in multiple files: {}",
                id,
                paths.join(", ")
            ));
        }
    }

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

    if synthetic_root.exists() {
        let vault_stems = load_vault_stems(workspace_root);
        check_links_resolve(&synthetic_root, &synthetic_root, &vault_stems, &mut result)?;
    }

    check_orphan_atomics(&synthetic_root, &mut result)?;

    Ok(result)
}

// ── In-workspace check ────────────────────────────────────────────────────────

/// Run the structural subset of the invariant suite against a workspace
/// `_synthetic/` directory without needing a session report.
///
/// This is the function backing `csnotes check`, which Claude can invoke from
/// inside the workspace before exiting to surface violations early.
pub fn check_workspace(workspace_root: &Path, synthetic_dir: &str) -> Result<AuditResult> {
    let mut result = AuditResult::default();
    let synthetic_root = workspace_root.join(synthetic_dir);

    if !synthetic_root.exists() {
        result.soft_warnings.push(format!(
            "synthetic directory '{}' not found — nothing to check",
            synthetic_dir
        ));
        return Ok(result);
    }

    for (id, paths) in block_id_collisions(&synthetic_root)? {
        result.hard_violations.push(format!(
            "block ID '^{}' appears in multiple files: {}",
            id,
            paths.join(", ")
        ));
    }

    for entry in walkdir::WalkDir::new(&synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match crate::frontmatter::read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Ok(fm) = crate::frontmatter::parse_frontmatter(&content, entry.path()) {
            if fm.kind == crate::frontmatter::NoteKind::Atomic {
                match &fm.block_id {
                    None => {
                        result.hard_violations.push(format!(
                            "atomic note '{}' has no block_id in frontmatter",
                            entry.path().display()
                        ));
                    }
                    Some(id) => {
                        let ids_in_body = crate::obsidian::extract_block_ids(&content);
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

    let vault_stems = load_vault_stems(workspace_root);
    check_links_resolve(&synthetic_root, &synthetic_root, &vault_stems, &mut result)?;

    check_orphan_atomics(&synthetic_root, &mut result)?;

    Ok(result)
}

// ── Direct vault audit ────────────────────────────────────────────────────────

const AI_CONVERSATION_SIDECAR_WORD_THRESHOLD: usize = 4_500;

/// Run the invariant suite against the vault directly (for `csnotes audit`).
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

    for (id, paths) in block_id_collisions(&synthetic_root)? {
        result.hard_violations.push(format!(
            "block ID '^{}' appears in multiple files: {}",
            id,
            paths.join(", ")
        ));
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

    let ignore_names: Vec<&str> = config
        .sources_ignore_dirs
        .iter()
        .map(|s| s.as_str())
        .collect();
    let vault_stems = collect_vault_stems(
        vault_root,
        &[
            &config.synthetic_dir,
            &config.generated_dir,
            &config.csnotes_dir,
        ],
        &ignore_names,
    );
    check_links_resolve(&synthetic_root, &synthetic_root, &vault_stems, &mut result)?;

    check_orphan_atomics(&synthetic_root, &mut result)?;

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

// ── Vault stem index ──────────────────────────────────────────────────────────

/// Collect lowercased stem and root-relative-path identifiers for all `.md`
/// files in `vault_root`, skipping top-level directories in `excluded_dirs`.
pub fn collect_vault_stems(
    vault_root: &Path,
    excluded_dirs: &[&str],
    ignore_names: &[&str],
) -> std::collections::HashSet<String> {
    walkdir::WalkDir::new(vault_root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_str().unwrap_or("");
                if e.depth() == 1
                    && (excluded_dirs.contains(&name) || name.starts_with("_synthetic_"))
                {
                    return false;
                }
                if ignore_names.contains(&name) {
                    return false;
                }
            }
            true
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .flat_map(|e| {
            let stem = e
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            let rel = e
                .path()
                .strip_prefix(vault_root)
                .ok()
                .map(|p| p.with_extension(""))
                .and_then(|p| p.to_str().map(|s| s.to_lowercase()));
            [stem, rel].into_iter().flatten()
        })
        .collect()
}

/// Load the vault-wide stem index written to `_vault_stems.json` during
/// workspace assembly.  Returns an empty set if the file is absent.
pub fn load_vault_stems(workspace_root: &Path) -> std::collections::HashSet<String> {
    let path = workspace_root.join("_vault_stems.json");
    if !path.exists() {
        return std::collections::HashSet::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return std::collections::HashSet::new(),
    };
    serde_json::from_str::<Vec<String>>(&content)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

// ── Private helpers ───────────────────────────────────────────────────────────

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

fn check_links_resolve(
    synthetic_root: &Path,
    search_root: &Path,
    extra: &std::collections::HashSet<String>,
    result: &mut AuditResult,
) -> Result<()> {
    let mut known_stems = collect_note_stems(search_root);
    known_stems.extend(extra.iter().cloned());

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

fn collect_note_stems(root: &Path) -> std::collections::HashSet<String> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .flat_map(|e| {
            let stem = e
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            let rel = e
                .path()
                .strip_prefix(root)
                .ok()
                .map(|p| p.with_extension(""))
                .and_then(|p| p.to_str().map(|s| s.to_lowercase()));
            [stem, rel].into_iter().flatten()
        })
        .collect()
}

fn check_orphan_atomics(synthetic_root: &Path, result: &mut AuditResult) -> Result<()> {
    if !synthetic_root.exists() {
        return Ok(());
    }

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
    use crate::manifest::ManifestConfig;
    use crate::report::{
        CreateNoteOp, DemoteTopicOp, MergeTopicsOp, MoveAtomicOp, PromoteAtomicOp, RenameAtomicOp,
        RenameTopicOp, ReportScope, ScopeKind, SessionReport, SetEmbedOp, SplitTarget,
        SplitTopicOp, UpdateNoteOp,
    };
    use chrono::Utc;
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

    fn rename_atomic_op(path: &str, new_slug: &str) -> Op {
        Op::RenameAtomic(RenameAtomicOp {
            path: path.into(),
            new_slug: new_slug.into(),
            new_title: "New Title".into(),
            reason: "test".into(),
        })
    }

    fn move_atomic_op(from_path: &str, to_topic: &str) -> Op {
        Op::MoveAtomic(MoveAtomicOp {
            from_path: from_path.into(),
            to_topic: to_topic.into(),
            reason: "test".into(),
        })
    }

    fn promote_atomic_op(from_path: &str, to_topic: &str) -> Op {
        Op::PromoteAtomic(PromoteAtomicOp {
            from_path: from_path.into(),
            to_topic: to_topic.into(),
            reason: "test".into(),
        })
    }

    fn demote_topic_op(from: &str, into: &str) -> Op {
        Op::DemoteTopic(DemoteTopicOp {
            from_topic: from.into(),
            into_topic: into.into(),
            reason: "test".into(),
        })
    }

    fn merge_topics_op(from: Vec<&str>, into: &str) -> Op {
        Op::MergeTopics(MergeTopicsOp {
            from: from.into_iter().map(String::from).collect(),
            into: into.into(),
            reason: "test".into(),
        })
    }

    fn split_topic_op(from: &str, targets: Vec<(&str, Vec<&str>)>) -> Op {
        Op::SplitTopic(SplitTopicOp {
            from: from.into(),
            into: targets
                .into_iter()
                .map(|(topic, atomics)| SplitTarget {
                    topic: topic.into(),
                    atomics: atomics.into_iter().map(String::from).collect(),
                })
                .collect(),
            reason: "test".into(),
        })
    }

    fn set_embed_op(atomic_path: &str, index_path: &str, present: bool) -> Op {
        Op::SetEmbed(SetEmbedOp {
            atomic_path: atomic_path.into(),
            index_path: index_path.into(),
            present,
        })
    }

    fn index_note() -> &'static str {
        "---\ncsnotes_schema: 1\nkind: index\ntopic: test\ntitle: Test Index\n\
         contributing_sessions: []\ncontributing_sources: []\n\
         created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
         ---\nIndex content.\n"
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
        let report = make_report(vec![create_op("note.md")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("note.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_create_note_existing_frontmatter_errors() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", csnotes_note());
        let report = make_report(vec![create_op("note.md")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_create_note_fresh_file_passes() {
        let tmp = TempDir::new().unwrap();
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
        std::fs::create_dir_all(tmp.path().join("_synthetic/sorting")).unwrap();
        let report = make_report(vec![rename_op("sorting", "algorithms")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_rename_topic_dest_exists_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/sorting")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/algorithms")).unwrap();
        let report = make_report(vec![rename_op("sorting", "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("algorithms"), "{err}");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_rename_atomic_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![rename_atomic_op("_synthetic/cs/old.md", "new")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("old.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_rename_atomic_dest_exists_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/old.md", csnotes_note());
        write(tmp.path(), "_synthetic/cs/new.md", csnotes_note());
        let report = make_report(vec![rename_atomic_op("_synthetic/cs/old.md", "new")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("new"), "{err}");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_rename_atomic_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/old.md", csnotes_note());
        let report = make_report(vec![rename_atomic_op("_synthetic/cs/old.md", "new")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_move_atomic_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        let report = make_report(vec![move_atomic_op("_synthetic/cs/bfs.md", "graphs")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_move_atomic_target_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        let report = make_report(vec![move_atomic_op("_synthetic/cs/bfs.md", "graphs")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("graphs"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn precondition_move_atomic_dest_conflict_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        write(tmp.path(), "_synthetic/graphs/bfs.md", csnotes_note());
        let report = make_report(vec![move_atomic_op("_synthetic/cs/bfs.md", "graphs")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs"), "{err}");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_move_atomic_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        let report = make_report(vec![move_atomic_op("_synthetic/cs/bfs.md", "graphs")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_promote_atomic_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![promote_atomic_op("_synthetic/cs/bfs.md", "new-topic")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_promote_atomic_target_exists_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/new-topic")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        let report = make_report(vec![promote_atomic_op("_synthetic/cs/bfs.md", "new-topic")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("new-topic"), "{err}");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_promote_atomic_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        let report = make_report(vec![promote_atomic_op("_synthetic/cs/bfs.md", "new-topic")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_demote_topic_same_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        let report = make_report(vec![demote_topic_op("cs", "cs")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("same topic"), "{err}");
    }

    #[test]
    fn precondition_demote_topic_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/algorithms")).unwrap();
        let report = make_report(vec![demote_topic_op("cs", "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("cs"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn precondition_demote_topic_target_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        let report = make_report(vec![demote_topic_op("cs", "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("algorithms"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn precondition_demote_topic_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/algorithms")).unwrap();
        let report = make_report(vec![demote_topic_op("cs", "algorithms")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_merge_topics_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        let report = make_report(vec![merge_topics_op(vec!["cs", "graphs"], "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("cs"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn precondition_merge_topics_filename_conflict_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        write(tmp.path(), "_synthetic/graphs/bfs.md", csnotes_note());
        let report = make_report(vec![merge_topics_op(vec!["cs", "graphs"], "algorithms")]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs.md"), "{err}");
        assert!(err.to_string().contains("conflict"), "{err}");
    }

    #[test]
    fn precondition_merge_topics_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        write(tmp.path(), "_synthetic/cs/dfs.md", csnotes_note());
        write(tmp.path(), "_synthetic/graphs/bfs.md", csnotes_note());
        let report = make_report(vec![merge_topics_op(vec!["cs", "graphs"], "algorithms")]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_split_topic_source_missing_errors() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![split_topic_op("cs", vec![("graphs", vec!["bfs"])])]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("cs"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn precondition_split_topic_atomic_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        let report = make_report(vec![split_topic_op("cs", vec![("graphs", vec!["bfs"])])]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_split_topic_dest_conflict_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/graphs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        write(tmp.path(), "_synthetic/graphs/bfs.md", csnotes_note());
        let report = make_report(vec![split_topic_op("cs", vec![("graphs", vec!["bfs"])])]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs"), "{err}");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn precondition_split_topic_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        write(tmp.path(), "_synthetic/cs/dfs.md", csnotes_note());
        let report = make_report(vec![split_topic_op(
            "cs",
            vec![("graphs", vec!["bfs"]), ("search", vec!["dfs"])],
        )]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_set_embed_index_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        let report = make_report(vec![set_embed_op(
            "_synthetic/cs/bfs.md",
            "_synthetic/cs/index.md",
            true,
        )]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("index.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_set_embed_atomic_missing_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/index.md", index_note());
        let report = make_report(vec![set_embed_op(
            "_synthetic/cs/bfs.md",
            "_synthetic/cs/index.md",
            true,
        )]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs.md"), "{err}");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn precondition_set_embed_no_block_id_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/index.md", index_note());
        let no_block_id = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: cs\ntitle: BFS\n\
             contributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n\
             ---\nContent.\n";
        write(tmp.path(), "_synthetic/cs/bfs.md", no_block_id);
        let report = make_report(vec![set_embed_op(
            "_synthetic/cs/bfs.md",
            "_synthetic/cs/index.md",
            true,
        )]);
        let err = precondition_pass(&report, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bfs.md"), "{err}");
        assert!(err.to_string().contains("no block_id"), "{err}");
    }

    #[test]
    fn precondition_set_embed_passes() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();
        write(tmp.path(), "_synthetic/cs/index.md", index_note());
        write(tmp.path(), "_synthetic/cs/bfs.md", csnotes_note());
        let report = make_report(vec![set_embed_op(
            "_synthetic/cs/bfs.md",
            "_synthetic/cs/index.md",
            true,
        )]);
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn crlf_frontmatter_parsed_correctly_in_audit() {
        let tmp = TempDir::new().unwrap();
        let crlf_atomic = "---\r\ncsnotes_schema: 1\r\nkind: atomic\r\ntopic: cs\r\n\
            title: Sorting\r\nblock_id: sort-01\r\ncontributing_sessions: []\r\n\
            contributing_sources: []\r\ncreated: \"2026-01-01T00:00:00Z\"\r\n\
            last_updated: \"2026-01-01T00:00:00Z\"\r\n---\r\nSorting content.\r\n";
        write(tmp.path(), "sorting.md", crlf_atomic);

        let mut result = AuditResult::default();
        check_orphan_atomics(tmp.path(), &mut result).unwrap();
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
        write(tmp.path(), "sorting.md", "# Sorting\n");
        write(tmp.path(), "index.md", "See [[Sorting]].\n");
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &Default::default(), &mut result).unwrap();
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
        check_links_resolve(tmp.path(), tmp.path(), &Default::default(), &mut result).unwrap();
        assert_eq!(result.hard_violations.len(), 1);
        assert!(result.hard_violations[0].contains("nonexistent"));
    }

    #[test]
    fn check_links_resolve_flags_broken_embed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.md", "![[ghost-atomic#^id]]\n");
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &Default::default(), &mut result).unwrap();
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
            "{:?}",
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
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn check_orphan_atomics_embedded_atomic_not_warned() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "sorting.md", csnotes_note());
        write(tmp.path(), "index.md", "![[sorting#^test-01]]\n");
        let mut result = AuditResult::default();
        check_orphan_atomics(tmp.path(), &mut result).unwrap();
        assert!(
            result.soft_warnings.is_empty(),
            "{:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn audit_vault_warns_when_synthetic_dir_absent() {
        let tmp = TempDir::new().unwrap();
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
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn audit_vault_orphan_atomic_is_soft_warning() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/cs/sorting.md", csnotes_note());
        let result = audit_vault(
            tmp.path(),
            &make_vault_config(),
            &make_empty_manifest(tmp.path()),
        )
        .unwrap();
        assert!(
            result.soft_warnings.iter().any(|w| w.contains("orphan")),
            "{:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn invariant_suite_clean_when_no_synthetic_dir() {
        let tmp = TempDir::new().unwrap();
        let report = make_report(vec![]);
        let manifest = make_empty_manifest(tmp.path());
        let result = invariant_suite(tmp.path(), "_synthetic", &report, &manifest).unwrap();
        assert!(result.is_clean(), "{:?}", result);
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
            "{:?}",
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
            "{:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn invariant_suite_index_note_without_block_id_is_not_flagged() {
        let tmp = TempDir::new().unwrap();
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
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn collect_note_stems_lowercases_mixed_case_files() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "Sorting.md", "# Sorting\n");
        let stems = collect_note_stems(tmp.path());
        assert!(stems.contains("sorting"));
        assert!(!stems.contains("mergesort"));
    }

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
            "{:?}",
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
            "{:?}",
            result.hard_violations
        );
    }

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
            "{:?}",
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
    fn precondition_embed_line_missing_from_index_passes() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n\n^my-id\n");
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
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn precondition_embed_wrong_file_same_block_id_passes() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "# Content\n\n^my-id\n");
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
        assert!(precondition_pass(&report, tmp.path()).is_ok());
    }

    #[test]
    fn collect_note_stems_includes_relative_path() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "java/java-boxed-primitives.md",
            "# Java Boxed Primitives\n",
        );
        let stems = collect_note_stems(tmp.path());
        assert!(stems.contains("java-boxed-primitives"), "{:?}", stems);
        assert!(stems.contains("java/java-boxed-primitives"), "{:?}", stems);
    }

    #[test]
    fn collect_note_stems_rel_path_is_lowercased() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "Java/Java-Boxed-Primitives.md", "body\n");
        let stems = collect_note_stems(tmp.path());
        assert!(stems.contains("java-boxed-primitives"));
        assert!(stems.contains("java/java-boxed-primitives"));
        assert!(!stems.contains("Java-Boxed-Primitives"));
        assert!(!stems.contains("Java/Java-Boxed-Primitives"));
    }

    #[test]
    fn check_links_resolve_accepts_topic_prefixed_wikilink() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "java/java-boxed-primitives.md",
            "# Java Boxed Primitives\n",
        );
        write(
            tmp.path(),
            "java/index.md",
            "See [[java/java-boxed-primitives]].\n",
        );
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &Default::default(), &mut result).unwrap();
        assert!(
            result.hard_violations.is_empty(),
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn check_links_resolve_accepts_mixed_case_topic_prefixed_wikilink() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "java/java-boxed-primitives.md",
            "# Java Boxed Primitives\n",
        );
        write(
            tmp.path(),
            "java/index.md",
            "See [[Java/Java-Boxed-Primitives]].\n",
        );
        let mut result = AuditResult::default();
        check_links_resolve(tmp.path(), tmp.path(), &Default::default(), &mut result).unwrap();
        assert!(
            result.hard_violations.is_empty(),
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn collect_vault_stems_excludes_specified_dirs() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "_synthetic/topic/note.md", "body\n");
        write(tmp.path(), "_synthetic_broken_abc/topic/note.md", "body\n");
        write(
            tmp.path(),
            "AI-Conversations/Claude/Java-boxed-primitives.md",
            "body\n",
        );
        let stems = collect_vault_stems(tmp.path(), &["_synthetic"], &[]);
        assert!(stems.contains("java-boxed-primitives"), "{:?}", stems);
        assert!(
            stems.contains("ai-conversations/claude/java-boxed-primitives"),
            "{:?}",
            stems
        );
        assert!(!stems.contains("note"), "{:?}", stems);
    }

    #[test]
    fn collect_vault_stems_respects_ignore_names_at_any_depth() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "AI-Conversations/Claude/java-notes.md",
            "body\n",
        );
        write(tmp.path(), "sources/_tools/generator.md", "body\n");
        write(tmp.path(), "sources/SICP/_exercises/ch01.md", "body\n");
        let stems = collect_vault_stems(tmp.path(), &[], &["_tools", "_exercises"]);
        assert!(stems.contains("java-notes"));
        assert!(!stems.contains("generator"), "{:?}", stems);
        assert!(!stems.contains("ch01"), "{:?}", stems);
    }

    #[test]
    fn load_vault_stems_returns_empty_when_file_absent() {
        let tmp = TempDir::new().unwrap();
        let stems = load_vault_stems(tmp.path());
        assert!(stems.is_empty());
    }

    #[test]
    fn load_vault_stems_round_trips_json() {
        let tmp = TempDir::new().unwrap();
        let data = serde_json::to_string(&vec![
            "java-boxed-primitives",
            "ai-conversations/claude/java-boxed-primitives",
        ])
        .unwrap();
        std::fs::write(tmp.path().join("_vault_stems.json"), data).unwrap();
        let stems = load_vault_stems(tmp.path());
        assert!(stems.contains("java-boxed-primitives"));
        assert!(stems.contains("ai-conversations/claude/java-boxed-primitives"));
    }

    #[test]
    fn check_links_resolve_accepts_cross_vault_link_via_extra_stems() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "_synthetic/java/overview.md",
            "See [[Java-boxed-primitives]] for details.\n",
        );
        let mut extra = std::collections::HashSet::new();
        extra.insert("java-boxed-primitives".to_string());
        extra.insert("ai-conversations/claude/java-boxed-primitives".to_string());
        let synthetic_root = tmp.path().join("_synthetic");
        let mut result = AuditResult::default();
        check_links_resolve(&synthetic_root, &synthetic_root, &extra, &mut result).unwrap();
        assert!(
            result.hard_violations.is_empty(),
            "{:?}",
            result.hard_violations
        );
    }

    #[test]
    fn check_workspace_accepts_cross_vault_link_when_vault_stems_file_present() {
        let tmp = TempDir::new().unwrap();
        let note = "---\ncsnotes_schema: 1\nkind: atomic\ntopic: java\ntitle: Overview\n\
                    block_id: java-overview\ncontributing_sessions: []\n\
                    contributing_sources: []\ncreated: \"2026-01-01T00:00:00Z\"\n\
                    last_updated: \"2026-01-01T00:00:00Z\"\n---\n\
                    See [[Java-boxed-primitives]].\n\n^java-overview\n";
        write(tmp.path(), "_synthetic/java/overview.md", note);
        let stems = serde_json::to_string(&vec![
            "java-boxed-primitives",
            "ai-conversations/claude/java-boxed-primitives",
        ])
        .unwrap();
        std::fs::write(tmp.path().join("_vault_stems.json"), stems).unwrap();
        let result = check_workspace(tmp.path(), "_synthetic").unwrap();
        assert!(
            result.hard_violations.is_empty(),
            "{:?}",
            result.hard_violations
        );
    }

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
        let body = "word ".repeat(4_600);
        write(tmp.path(), "sources/AI-Conversations/Gemini/chat.md", &body);
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
            "{:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn audit_vault_no_sidecar_warn_when_json_present() {
        let tmp = TempDir::new().unwrap();
        let body = "word ".repeat(2_100);
        write(tmp.path(), "sources/AI-Conversations/Gemini/chat.md", &body);
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
            "{:?}",
            result.soft_warnings
        );
    }

    #[test]
    fn audit_vault_no_sidecar_warn_for_short_ai_conversation() {
        let tmp = TempDir::new().unwrap();
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
            "{:?}",
            result.soft_warnings
        );
    }
}
