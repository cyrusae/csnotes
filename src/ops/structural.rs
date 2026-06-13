/// Structural ops: rename_topic (Phase 1) and the rest (Phase 4).
///
/// These ops perform vault-global mutations that the AI only *requests* via
/// the session report.  The CLI executes them here, transactionally against
/// the workspace copy.
///
/// **Phase 1:** `rename_topic`
/// **Phase 4:** `move_atomic`, `promote_atomic`, `demote_topic`,
///              `merge_topics`, `split_topic`, `set_embed`
use std::path::Path;

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::pathutil::safe_join;
use crate::report::*;

/// Execute a `rename_topic` op against the workspace.
///
/// Steps:
/// 1. Rename `_synthetic/{from}/` → `_synthetic/{to}/`
/// 2. Update the `topic` frontmatter field in every note in the renamed folder
/// 3. Rewrite any path-qualified wikilinks (`[[{from}/…]]`) across `_synthetic/`
pub fn execute_rename_topic(
    op: &RenameTopicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_dir = safe_join(&synthetic_root, &op.from)?;
    let to_dir = safe_join(&synthetic_root, &op.to)?;

    if !from_dir.exists() {
        bail!(
            "rename_topic: source topic folder '{}' does not exist in workspace",
            op.from
        );
    }
    if to_dir.exists() {
        bail!(
            "rename_topic: destination topic folder '{}' already exists \
             (use merge_topics to combine topics)",
            op.to
        );
    }

    // 1. Rename the folder.
    std::fs::rename(&from_dir, &to_dir).with_context(|| {
        format!(
            "rename_topic: renaming '{}' → '{}'",
            from_dir.display(),
            to_dir.display()
        )
    })?;

    // 2. Update `topic` in frontmatter for every note now in the new folder.
    for entry in WalkDir::new(&to_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        crate::frontmatter::update_frontmatter(entry.path(), |fm| {
            if fm.topic == op.from {
                fm.topic = op.to.clone();
            }
        })?;
    }

    // 3. Rewrite any path-qualified wikilinks across all of `_synthetic/`.
    //    Pure stem links (e.g. `[[polymorphism]]`) are unaffected because
    //    Obsidian resolves them by filename, not by folder.
    //    Path-qualified links (e.g. `[[inheritance/polymorphism]]`) need
    //    the folder prefix updated.
    rewrite_links(
        &synthetic_root,
        &format!("{}/", op.from),
        &format!("{}/", op.to),
    )?;

    Ok(())
}

/// Execute a `rename_atomic` op against the workspace.
///
/// Renames an atomic note's file (slug) within its current topic folder.
/// Updates the `title` frontmatter field and rewrites all wikilinks that
/// reference the old path stem across `_synthetic/`.  Aliases in links of the
/// form `[[topic/old-slug|Label]]` are preserved automatically.
pub fn execute_rename_atomic(
    op: &RenameAtomicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_abs = safe_join(workspace_root, &op.path)?;

    let from_p = std::path::Path::new(&op.path);
    let old_slug = from_p
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("rename_atomic: cannot parse slug from '{}'", op.path))?;
    let topic = from_p
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("rename_atomic: cannot parse topic from '{}'", op.path))?;

    if op.new_slug.is_empty() {
        bail!("rename_atomic: new_slug must not be empty");
    }

    let to_abs = safe_join(&synthetic_root, topic)?.join(format!("{}.md", op.new_slug));

    if !from_abs.exists() {
        bail!("rename_atomic: source note '{}' not found in workspace", op.path);
    }
    if to_abs.exists() {
        bail!(
            "rename_atomic: '{}' already exists in topic '{}'",
            op.new_slug, topic
        );
    }

    std::fs::rename(&from_abs, &to_abs).with_context(|| {
        format!(
            "rename_atomic: renaming '{}' → '{}.md'",
            from_abs.display(),
            op.new_slug
        )
    })?;

    crate::frontmatter::update_frontmatter(&to_abs, |fm| {
        fm.title = op.new_title.clone();
    })?;

    let old_link = format!("{}/{}", topic, old_slug);
    let new_link = format!("{}/{}", topic, op.new_slug);
    rewrite_note_links(&synthetic_root, &old_link, &new_link)?;

    Ok(())
}

/// Execute a `move_atomic` op against the workspace.
///
/// Moves an atomic note from its current topic folder to an **existing** topic
/// folder.  Updates frontmatter and rewrites path-qualified wikilinks.
///
/// Use `promote_atomic` when the destination topic is brand-new.
pub fn execute_move_atomic(
    op: &MoveAtomicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_abs = safe_join(workspace_root, &op.from_path)?;

    let from_p = std::path::Path::new(&op.from_path);
    let slug = from_p
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("move_atomic: cannot parse slug from '{}'", op.from_path))?;
    let old_topic = from_p
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("move_atomic: cannot parse topic from '{}'", op.from_path))?;

    let to_dir = safe_join(&synthetic_root, &op.to_topic)?;
    let to_abs = to_dir.join(format!("{}.md", slug));

    if !from_abs.exists() {
        bail!("move_atomic: source note '{}' not found in workspace", op.from_path);
    }
    if !to_dir.exists() {
        bail!(
            "move_atomic: target topic '{}' does not exist \
             (use promote_atomic to create a new topic)",
            op.to_topic
        );
    }
    if to_abs.exists() {
        bail!(
            "move_atomic: '{}' already exists in target topic '{}'",
            slug,
            op.to_topic
        );
    }

    std::fs::rename(&from_abs, &to_abs).with_context(|| {
        format!(
            "move_atomic: moving '{}' to '{}'",
            from_abs.display(),
            to_abs.display()
        )
    })?;

    crate::frontmatter::update_frontmatter(&to_abs, |fm| {
        fm.topic = op.to_topic.clone();
    })?;

    if old_topic != op.to_topic.as_str() {
        let old_link = format!("{}/{}", old_topic, slug);
        let new_link = format!("{}/{}", op.to_topic, slug);
        rewrite_note_links(&synthetic_root, &old_link, &new_link)?;
    }

    Ok(())
}

/// Execute a `promote_atomic` op against the workspace.
///
/// Moves an atomic note into a **new** topic folder (which must not exist yet).
/// The AI typically pairs this with a `create_note` for the new topic's index.
///
/// Use `move_atomic` when the destination topic already exists.
pub fn execute_promote_atomic(
    op: &PromoteAtomicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_abs = safe_join(workspace_root, &op.from_path)?;

    let from_p = std::path::Path::new(&op.from_path);
    let slug = from_p
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("promote_atomic: cannot parse slug from '{}'", op.from_path))?;
    let old_topic = from_p
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("promote_atomic: cannot parse topic from '{}'", op.from_path))?;

    let to_dir = safe_join(&synthetic_root, &op.to_topic)?;
    let to_abs = to_dir.join(format!("{}.md", slug));

    if !from_abs.exists() {
        bail!("promote_atomic: source note '{}' not found in workspace", op.from_path);
    }
    if to_dir.exists() {
        bail!(
            "promote_atomic: target topic '{}' already exists \
             (use move_atomic to move into an existing topic)",
            op.to_topic
        );
    }

    std::fs::create_dir_all(&to_dir).with_context(|| {
        format!(
            "promote_atomic: creating topic folder '{}'",
            to_dir.display()
        )
    })?;

    std::fs::rename(&from_abs, &to_abs).with_context(|| {
        format!(
            "promote_atomic: moving '{}' to '{}'",
            from_abs.display(),
            to_abs.display()
        )
    })?;

    crate::frontmatter::update_frontmatter(&to_abs, |fm| {
        fm.topic = op.to_topic.clone();
    })?;

    let old_link = format!("{}/{}", old_topic, slug);
    let new_link = format!("{}/{}", op.to_topic, slug);
    rewrite_note_links(&synthetic_root, &old_link, &new_link)?;

    Ok(())
}

/// Execute a `demote_topic` op against the workspace.
///
/// Folds all notes from `from_topic` into an **existing** `into_topic`, then
/// deletes the now-empty source folder.
///
/// Use `merge_topics` when you want to fold several topics at once or when the
/// target topic doesn't exist yet.
pub fn execute_demote_topic(
    op: &DemoteTopicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_dir = safe_join(&synthetic_root, &op.from_topic)?;
    let into_dir = safe_join(&synthetic_root, &op.into_topic)?;

    if op.from_topic == op.into_topic {
        bail!("demote_topic: source and target are the same topic '{}'", op.from_topic);
    }
    if !from_dir.exists() {
        bail!("demote_topic: source topic '{}' does not exist", op.from_topic);
    }
    if !into_dir.exists() {
        bail!(
            "demote_topic: target topic '{}' does not exist \
             (use merge_topics with a new target to create it)",
            op.into_topic
        );
    }

    move_topic_notes(&from_dir, &into_dir, &op.from_topic, &op.into_topic)?;
    rewrite_links(
        &synthetic_root,
        &format!("{}/", op.from_topic),
        &format!("{}/", op.into_topic),
    )?;
    std::fs::remove_dir(&from_dir).with_context(|| {
        format!("demote_topic: removing source folder '{}'", op.from_topic)
    })?;

    Ok(())
}

/// Execute a `merge_topics` op against the workspace.
///
/// Folds all notes from every listed `from` topic into `into`, which is
/// created if it doesn't already exist.  Every source folder is deleted after
/// its notes have been moved.  `into` may be one of the `from` topics (keep
/// `into` in place while absorbing the others).
pub fn execute_merge_topics(
    op: &MergeTopicsOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let into_dir = safe_join(&synthetic_root, &op.into)?;

    if op.from.is_empty() {
        bail!("merge_topics: 'from' list is empty");
    }

    // Create the target if it doesn't yet exist (and isn't one of the sources).
    if !into_dir.exists() {
        std::fs::create_dir_all(&into_dir).with_context(|| {
            format!("merge_topics: creating target folder '{}'", op.into)
        })?;
    }

    // Pre-flight: collect all filenames that will land in `into` to detect conflicts.
    let mut landing: std::collections::HashSet<String> = std::fs::read_dir(&into_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    for from_topic in &op.from {
        if from_topic == &op.into {
            continue;
        }
        let from_dir = safe_join(&synthetic_root, from_topic)?;
        if !from_dir.exists() {
            bail!("merge_topics: source topic '{}' does not exist", from_topic);
        }
        for entry in WalkDir::new(&from_dir)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && e.path().extension().map_or(false, |x| x == "md"))
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if !landing.insert(name.clone()) {
                bail!(
                    "merge_topics: filename conflict — '{}' already exists in the merge target",
                    name
                );
            }
        }
    }

    // Execute: move notes, rewrite links, remove source folders.
    for from_topic in &op.from {
        if from_topic == &op.into {
            continue;
        }
        let from_dir = safe_join(&synthetic_root, from_topic)?;
        move_topic_notes(&from_dir, &into_dir, from_topic, &op.into)?;
        rewrite_links(
            &synthetic_root,
            &format!("{}/", from_topic),
            &format!("{}/", op.into),
        )?;
        std::fs::remove_dir(&from_dir).with_context(|| {
            format!("merge_topics: removing source folder '{}'", from_topic)
        })?;
    }

    Ok(())
}

/// Execute a `split_topic` op against the workspace.
///
/// Distributes atomics from `from` across new (or existing) `into` topics.
/// Each `SplitTarget` lists which note slugs move to that target.  Notes not
/// mentioned in any target are left in `from`.  If all notes are distributed,
/// the `from` folder is deleted.
pub fn execute_split_topic(
    op: &SplitTopicOp,
    workspace_root: &Path,
    synthetic_dir: &str,
) -> Result<()> {
    let synthetic_root = workspace_root.join(synthetic_dir);
    let from_dir = safe_join(&synthetic_root, &op.from)?;

    if !from_dir.exists() {
        bail!("split_topic: source topic '{}' does not exist", op.from);
    }
    if op.into.is_empty() {
        bail!("split_topic: 'into' list is empty");
    }

    for target in &op.into {
        if target.topic == op.from {
            continue; // some notes stay in source — valid, nothing to create
        }
        let target_dir = safe_join(&synthetic_root, &target.topic)?;
        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir).with_context(|| {
                format!("split_topic: creating folder '{}'", target.topic)
            })?;
        }

        for slug in &target.atomics {
            let from_note = from_dir.join(format!("{}.md", slug));
            let to_note = target_dir.join(format!("{}.md", slug));

            if !from_note.exists() {
                bail!(
                    "split_topic: '{}' not found in source topic '{}'",
                    slug,
                    op.from
                );
            }
            if to_note.exists() {
                bail!(
                    "split_topic: '{}' already exists in target topic '{}'",
                    slug,
                    target.topic
                );
            }

            std::fs::rename(&from_note, &to_note).with_context(|| {
                format!("split_topic: moving '{}' to '{}'", slug, target.topic)
            })?;

            crate::frontmatter::update_frontmatter(&to_note, |fm| {
                fm.topic = target.topic.clone();
            })?;

            let old_link = format!("{}/{}", op.from, slug);
            let new_link = format!("{}/{}", target.topic, slug);
            rewrite_note_links(&synthetic_root, &old_link, &new_link)?;
        }
    }

    // Remove source folder if it's now empty.
    let is_empty = std::fs::read_dir(&from_dir)
        .map(|mut rd| rd.next().is_none())
        .unwrap_or(false);
    if is_empty {
        std::fs::remove_dir(&from_dir).with_context(|| {
            format!("split_topic: removing emptied source folder '{}'", op.from)
        })?;
    }

    Ok(())
}

/// Execute a `set_embed` op against the workspace.
///
/// Adds or removes a transclusion link (`![[slug#^block-id]]`) from an index
/// note's body, and keeps the index note's `embeds` frontmatter list in sync.
///
/// `present: true`  → ensure the embed line exists (idempotent if already there).
/// `present: false` → remove the embed line (idempotent if already absent).
pub fn execute_set_embed(op: &SetEmbedOp, workspace_root: &Path) -> Result<()> {
    let index_abs = safe_join(workspace_root, &op.index_path)?;
    let atomic_abs = safe_join(workspace_root, &op.atomic_path)?;

    if !index_abs.exists() {
        bail!("set_embed: index note '{}' not found", op.index_path);
    }
    if !atomic_abs.exists() {
        bail!("set_embed: atomic note '{}' not found", op.atomic_path);
    }

    let atomic_slug = std::path::Path::new(&op.atomic_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("set_embed: cannot parse slug from '{}'", op.atomic_path))?
        .to_string();

    let atomic_fm = crate::frontmatter::read_frontmatter(&atomic_abs)?;
    let block_id = atomic_fm.block_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "set_embed: atomic '{}' has no block_id in frontmatter",
            op.atomic_path
        )
    })?;
    let embed_line = format!("![[{}#^{}]]", atomic_slug, block_id);

    // Read index note, split frontmatter from body.
    let raw = crate::frontmatter::read_note(&index_abs)
        .with_context(|| format!("set_embed: reading '{}'", op.index_path))?;
    let (yaml, body) = crate::frontmatter::split_frontmatter(&raw)
        .ok_or_else(|| anyhow::anyhow!("set_embed: '{}' has no frontmatter", op.index_path))?;
    let mut fm: crate::frontmatter::NoteFrontmatter =
        serde_yml::from_str(yaml).with_context(|| {
            format!("set_embed: parsing frontmatter of '{}'", op.index_path)
        })?;

    let embeds = fm.embeds.get_or_insert_with(Vec::new);

    let new_body = if op.present {
        if !embeds.contains(&atomic_slug) {
            embeds.push(atomic_slug.clone());
        }
        if body.contains(&embed_line) {
            body.to_string()
        } else {
            // Append after last non-empty line, ensuring trailing newline.
            let trimmed = body.trim_end_matches('\n');
            format!("{}\n{}\n", trimmed, embed_line)
        }
    } else {
        embeds.retain(|e| e != &atomic_slug);
        // Remove the embed line; preserve all other lines.
        let lines: Vec<&str> = body
            .lines()
            .filter(|l| *l != embed_line.as_str())
            .collect();
        if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        }
    };

    crate::frontmatter::write_frontmatter(&index_abs, &fm, &new_body)?;
    Ok(())
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Move all `.md` files from `from_dir` into `to_dir`, checking for filename
/// conflicts before touching anything, and updating the `topic` frontmatter
/// field in each moved note.
fn move_topic_notes(
    from_dir: &Path,
    to_dir: &Path,
    from_topic: &str,
    to_topic: &str,
) -> Result<()> {
    // Collect files to move first (check conflicts before touching anything).
    let mut to_move = Vec::new();
    for entry in WalkDir::new(from_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().map_or(false, |x| x == "md"))
    {
        let filename = entry.path().file_name().unwrap().to_owned();
        let dest = to_dir.join(&filename);
        if dest.exists() {
            bail!(
                "filename conflict — '{}' exists in both '{}' and '{}'",
                filename.to_string_lossy(),
                from_topic,
                to_topic
            );
        }
        to_move.push((entry.path().to_owned(), dest));
    }

    for (src, dest) in to_move {
        std::fs::rename(&src, &dest)
            .with_context(|| format!("moving '{}'", src.display()))?;
        crate::frontmatter::update_frontmatter(&dest, |fm| {
            if fm.topic == from_topic {
                fm.topic = to_topic.to_string();
            }
        })?;
    }
    Ok(())
}

// ── Link rewriters ─────────────────────────────────────────────────────────────

/// Replace all occurrences of `[[old_target` → `[[new_target` and
/// `![[old_target` → `![[new_target` across all `.md` files under `root`.
///
/// Designed to accept an arbitrary `root` so Phase 4 can pass the full vault
/// root while Phase 1 passes only `_synthetic/`.
pub fn rewrite_links(root: &Path, old_target: &str, new_target: &str) -> Result<usize> {
    use walkdir::WalkDir;

    let old_wiki = format!("[[{}", old_target);
    let new_wiki = format!("[[{}", new_target);
    let old_embed = format!("![[{}", old_target);
    let new_embed = format!("![[{}", new_target);

    let mut files_changed = 0;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = crate::frontmatter::read_note(entry.path())?;
        if content.contains(&old_wiki) || content.contains(&old_embed) {
            let updated = content
                .replace(&old_embed, &new_embed) // embeds first (longer prefix)
                .replace(&old_wiki, &new_wiki);
            std::fs::write(entry.path(), updated)?;
            files_changed += 1;
        }
    }

    Ok(files_changed)
}

/// Rewrite wikilinks that reference a **specific note** by its path stem.
///
/// More precise than `rewrite_links`: only rewrites when the matched text is
/// followed by `]]`, `#`, or `|` — preventing false matches like
/// `[[topic/sorting-redux]]` when rewriting `topic/sorting`.
///
/// Both plain wikilinks (`[[…]]`) and embed wikilinks (`![[…]]`) are rewritten.
fn rewrite_note_links(root: &Path, old_stem: &str, new_stem: &str) -> Result<usize> {
    let mut files_changed = 0;
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "md"))
    {
        let content = crate::frontmatter::read_note(entry.path())?;
        let updated = replace_note_links(&content, old_stem, new_stem);
        if updated != content {
            std::fs::write(entry.path(), &updated)?;
            files_changed += 1;
        }
    }
    Ok(files_changed)
}

/// In-memory helper: replace `[[old_stem]]`, `[[old_stem#…`, `[[old_stem|…`
/// (and embed variants) without matching longer stems that share a prefix.
fn replace_note_links(content: &str, old_stem: &str, new_stem: &str) -> String {
    // A valid wikilink terminator after the stem path.
    const TERMINATORS: &[char] = &[']', '#', '|'];

    let mut result = String::with_capacity(content.len());
    let mut remaining = content;
    let pattern = format!("[[{}", old_stem);

    while let Some(idx) = remaining.find(&pattern) {
        let after = idx + pattern.len();
        if remaining[after..].starts_with(|c: char| TERMINATORS.contains(&c)) {
            result.push_str(&remaining[..idx]);
            result.push_str("[[");
            result.push_str(new_stem);
            remaining = &remaining[after..];
        } else {
            // Not a valid wikilink boundary — advance one char to avoid re-matching.
            result.push_str(&remaining[..idx + 1]);
            remaining = &remaining[idx + 1..];
        }
    }
    result.push_str(remaining);
    result
}

// ── Raw-note relinking ────────────────────────────────────────────────────────

/// After structural ops have been applied to `_synthetic/`, propagate the same
/// link rewrites to raw notes so that user-authored notes stay in sync.
///
/// `raw_note_roots` is a list of directories to scan (e.g. one per active
/// course, or just `vault_root/notes/` for flat vaults).  Errors on individual
/// files are swallowed — relink is best-effort after a committed session.
///
/// Returns the total number of files that had at least one link rewritten.
pub fn relink_raw_notes(ops: &[Op], raw_note_roots: &[&Path]) -> Result<usize> {
    if raw_note_roots.is_empty() {
        return Ok(0);
    }

    // Represent each rewrite as either a prefix rewrite ("inheritance/" →
    // "oop-basics/") or an exact-stem rewrite ("cs/old-slug" → "cs/new-slug").
    enum Rw {
        Prefix { old: String, new: String },
        Exact  { old: String, new: String },
    }

    let mut total = 0usize;

    for op in ops {
        let rewrites: Vec<Rw> = match op {
            Op::RenameTopic(o) => vec![Rw::Prefix {
                old: format!("{}/", o.from),
                new: format!("{}/", o.to),
            }],
            Op::RenameAtomic(o) => {
                let p = std::path::Path::new(&o.path);
                let old_slug = match p.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                let topic = match p.parent().and_then(|pp| pp.file_name()).and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                vec![Rw::Exact {
                    old: format!("{}/{}", topic, old_slug),
                    new: format!("{}/{}", topic, o.new_slug),
                }]
            }
            Op::MoveAtomic(o) => {
                let p = std::path::Path::new(&o.from_path);
                let slug = match p.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                let old_topic = match p.parent().and_then(|pp| pp.file_name()).and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                vec![Rw::Exact {
                    old: format!("{}/{}", old_topic, slug),
                    new: format!("{}/{}", o.to_topic, slug),
                }]
            }
            Op::PromoteAtomic(o) => {
                let p = std::path::Path::new(&o.from_path);
                let slug = match p.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                let old_topic = match p.parent().and_then(|pp| pp.file_name()).and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                vec![Rw::Exact {
                    old: format!("{}/{}", old_topic, slug),
                    new: format!("{}/{}", o.to_topic, slug),
                }]
            }
            Op::DemoteTopic(o) => vec![Rw::Prefix {
                old: format!("{}/", o.from_topic),
                new: format!("{}/", o.into_topic),
            }],
            Op::MergeTopics(o) => o.from.iter()
                .filter(|from| *from != &o.into)
                .map(|from| Rw::Prefix {
                    old: format!("{}/", from),
                    new: format!("{}/", o.into),
                })
                .collect(),
            Op::SplitTopic(o) => o.into.iter()
                .filter(|t| t.topic != o.from)
                .flat_map(|t| t.atomics.iter().map(move |slug| Rw::Exact {
                    old: format!("{}/{}", o.from, slug),
                    new: format!("{}/{}", t.topic, slug),
                }))
                .collect(),
            Op::CreateNote(_) | Op::UpdateNote(_) | Op::SetEmbed(_) => vec![],
        };

        for rw in &rewrites {
            for &root in raw_note_roots {
                let n = match rw {
                    Rw::Prefix { old, new } => rewrite_links(root, old, new).unwrap_or(0),
                    Rw::Exact  { old, new } => rewrite_note_links(root, old, new).unwrap_or(0),
                };
                total += n;
            }
        }
    }

    Ok(total)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_note(dir: &std::path::Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn minimal_note(topic: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: {topic}\ntitle: Test\n\
             block_id: test-id\nembeds: ~\ncontributing_sessions: []\n\
             contributing_sources: []\ncreated: \"2026-01-01T00:00:00Z\"\n\
             last_updated: \"2026-01-01T00:00:00Z\"\n---\n\nBody text.\n\n^test-id\n"
        )
    }

    #[test]
    fn rename_topic_renames_folder_and_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "inheritance/index.md", &minimal_note("inheritance"));
        write_note(&synthetic, "inheritance/poly.md", &minimal_note("inheritance"));

        let op = RenameTopicOp {
            from: "inheritance".to_string(),
            to: "oop-basics".to_string(),
            reason: "clearer name".to_string(),
        };
        execute_rename_topic(&op, tmp.path(), "_synthetic").unwrap();

        // Old folder gone, new folder present
        assert!(!synthetic.join("inheritance").exists());
        assert!(synthetic.join("oop-basics").exists());
        assert!(synthetic.join("oop-basics/index.md").exists());
        assert!(synthetic.join("oop-basics/poly.md").exists());

        // Frontmatter topic field updated
        let content = std::fs::read_to_string(synthetic.join("oop-basics/index.md")).unwrap();
        assert!(content.contains("topic: oop-basics"), "frontmatter topic not updated");
        assert!(!content.contains("topic: inheritance"), "old topic name still present");
    }

    #[test]
    fn rename_topic_rewrites_path_qualified_links() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "inheritance/index.md", &minimal_note("inheritance"));
        // Another note that uses a path-qualified link to the old topic
        write_note(
            &synthetic,
            "other/other.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: other\ntitle: Other\n\
             embeds: []\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nSee [[inheritance/poly]] and ![[inheritance/poly#^test-id]].\n",
        );

        let op = RenameTopicOp {
            from: "inheritance".to_string(),
            to: "oop-basics".to_string(),
            reason: "test".to_string(),
        };
        execute_rename_topic(&op, tmp.path(), "_synthetic").unwrap();

        let content = std::fs::read_to_string(synthetic.join("other/other.md")).unwrap();
        assert!(content.contains("[[oop-basics/poly]]"));
        assert!(content.contains("![[oop-basics/poly#^test-id]]"));
        assert!(!content.contains("[[inheritance/"));
    }

    #[test]
    fn rename_topic_fails_if_source_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic")).unwrap();
        let op = RenameTopicOp {
            from: "nonexistent".to_string(),
            to: "new-name".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_topic(&op, tmp.path(), "_synthetic").is_err());
    }

    #[test]
    fn rename_topic_fails_if_dest_exists() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "old-name/note.md", &minimal_note("old-name"));
        write_note(&synthetic, "new-name/note.md", &minimal_note("new-name"));

        let op = RenameTopicOp {
            from: "old-name".to_string(),
            to: "new-name".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_topic(&op, tmp.path(), "_synthetic").is_err());
    }

    // ── rename_atomic ────────────────────────────────────────────────────────

    fn atomic_note_with_title(topic: &str, title: &str) -> String {
        format!(
            "---\ncsnotes_schema: 1\nkind: atomic\ntopic: {topic}\ntitle: {title}\n\
             block_id: test-id\nembeds: ~\ncontributing_sessions: []\n\
             contributing_sources: []\ncreated: \"2026-01-01T00:00:00Z\"\n\
             last_updated: \"2026-01-01T00:00:00Z\"\n---\n\nBody.\n\n^test-id\n"
        )
    }

    #[test]
    fn rename_atomic_renames_file_and_updates_title() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "cs/old-slug.md", &atomic_note_with_title("cs", "Old Title"));

        let op = RenameAtomicOp {
            path: "_synthetic/cs/old-slug.md".to_string(),
            new_slug: "new-slug".to_string(),
            new_title: "New Title".to_string(),
            reason: "better name".to_string(),
        };
        execute_rename_atomic(&op, tmp.path(), "_synthetic").unwrap();

        assert!(!synthetic.join("cs/old-slug.md").exists(), "old file should be gone");
        assert!(synthetic.join("cs/new-slug.md").exists(), "new file should exist");
        let content = std::fs::read_to_string(synthetic.join("cs/new-slug.md")).unwrap();
        assert!(content.contains("title: New Title"), "title not updated in frontmatter");
        assert!(content.contains("topic: cs"), "topic should be unchanged");
    }

    #[test]
    fn rename_atomic_rewrites_links_in_other_notes() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "cs/old-slug.md", &atomic_note_with_title("cs", "Old Title"));
        write_note(
            &synthetic,
            "other/index.md",
            "See [[cs/old-slug]] and ![[cs/old-slug#^test-id]] and [[cs/old-slug|My Label]].\n",
        );

        let op = RenameAtomicOp {
            path: "_synthetic/cs/old-slug.md".to_string(),
            new_slug: "new-slug".to_string(),
            new_title: "New Title".to_string(),
            reason: "test".to_string(),
        };
        execute_rename_atomic(&op, tmp.path(), "_synthetic").unwrap();

        let content = std::fs::read_to_string(synthetic.join("other/index.md")).unwrap();
        assert!(content.contains("[[cs/new-slug]]"), "plain link not rewritten");
        assert!(content.contains("![[cs/new-slug#^test-id]]"), "embed link not rewritten");
        // Alias label preserved, only the slug updated
        assert!(content.contains("[[cs/new-slug|My Label]]"), "alias not preserved");
        assert!(!content.contains("old-slug"), "old slug should be gone from links");
    }

    #[test]
    fn rename_atomic_fails_if_source_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("_synthetic/cs")).unwrap();

        let op = RenameAtomicOp {
            path: "_synthetic/cs/missing.md".to_string(),
            new_slug: "new-name".to_string(),
            new_title: "New".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_atomic(&op, tmp.path(), "_synthetic").is_err());
    }

    #[test]
    fn rename_atomic_fails_if_dest_exists() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "cs/old-slug.md", &atomic_note_with_title("cs", "Old"));
        write_note(&synthetic, "cs/new-slug.md", &atomic_note_with_title("cs", "Existing"));

        let op = RenameAtomicOp {
            path: "_synthetic/cs/old-slug.md".to_string(),
            new_slug: "new-slug".to_string(),
            new_title: "New".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_atomic(&op, tmp.path(), "_synthetic").is_err());
    }

    #[test]
    fn rename_atomic_fails_if_new_slug_empty() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "cs/old-slug.md", &atomic_note_with_title("cs", "Old"));

        let op = RenameAtomicOp {
            path: "_synthetic/cs/old-slug.md".to_string(),
            new_slug: "".to_string(),
            new_title: "New".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_rename_atomic(&op, tmp.path(), "_synthetic").is_err());
    }

    // ── move_atomic ──────────────────────────────────────────────────────────

    #[test]
    fn move_atomic_moves_note_and_updates_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));
        write_note(&synthetic, "data-structures/index.md", &minimal_note("data-structures"));

        let op = MoveAtomicOp {
            from_path: "_synthetic/algorithms/sorting.md".to_string(),
            to_topic: "data-structures".to_string(),
            reason: "better fit".to_string(),
        };
        execute_move_atomic(&op, tmp.path(), "_synthetic").unwrap();

        assert!(!synthetic.join("algorithms/sorting.md").exists(), "source should be gone");
        assert!(synthetic.join("data-structures/sorting.md").exists(), "dest should exist");
        let content = std::fs::read_to_string(synthetic.join("data-structures/sorting.md")).unwrap();
        assert!(content.contains("topic: data-structures"), "frontmatter topic not updated");
    }

    #[test]
    fn move_atomic_rewrites_path_qualified_links() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));
        write_note(&synthetic, "data-structures/index.md", &minimal_note("data-structures"));
        // A third note that links to the moving note
        write_note(
            &synthetic,
            "other/other.md",
            "Body with [[algorithms/sorting]] and ![[algorithms/sorting#^test-id]].\n",
        );

        let op = MoveAtomicOp {
            from_path: "_synthetic/algorithms/sorting.md".to_string(),
            to_topic: "data-structures".to_string(),
            reason: "test".to_string(),
        };
        execute_move_atomic(&op, tmp.path(), "_synthetic").unwrap();

        let content = std::fs::read_to_string(synthetic.join("other/other.md")).unwrap();
        assert!(content.contains("[[data-structures/sorting]]"));
        assert!(content.contains("![[data-structures/sorting#^test-id]]"));
        assert!(!content.contains("[[algorithms/sorting"));
    }

    #[test]
    fn move_atomic_fails_if_source_missing() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        std::fs::create_dir_all(synthetic.join("dst-topic")).unwrap();

        let op = MoveAtomicOp {
            from_path: "_synthetic/algorithms/missing.md".to_string(),
            to_topic: "dst-topic".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_move_atomic(&op, tmp.path(), "_synthetic").is_err());
    }

    #[test]
    fn move_atomic_fails_if_dest_topic_missing() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));

        let op = MoveAtomicOp {
            from_path: "_synthetic/algorithms/sorting.md".to_string(),
            to_topic: "nonexistent-topic".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_move_atomic(&op, tmp.path(), "_synthetic").is_err());
    }

    // ── promote_atomic ───────────────────────────────────────────────────────

    #[test]
    fn promote_atomic_creates_new_topic_folder() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));

        let op = PromoteAtomicOp {
            from_path: "_synthetic/algorithms/sorting.md".to_string(),
            to_topic: "sorting-algorithms".to_string(),
            reason: "earned its own topic".to_string(),
        };
        execute_promote_atomic(&op, tmp.path(), "_synthetic").unwrap();

        assert!(synthetic.join("sorting-algorithms").is_dir());
        assert!(synthetic.join("sorting-algorithms/sorting.md").exists());
        let content = std::fs::read_to_string(synthetic.join("sorting-algorithms/sorting.md")).unwrap();
        assert!(content.contains("topic: sorting-algorithms"));
    }

    #[test]
    fn promote_atomic_fails_if_dest_topic_exists() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));
        write_note(&synthetic, "sorting-algorithms/index.md", &minimal_note("sorting-algorithms"));

        let op = PromoteAtomicOp {
            from_path: "_synthetic/algorithms/sorting.md".to_string(),
            to_topic: "sorting-algorithms".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_promote_atomic(&op, tmp.path(), "_synthetic").is_err());
    }

    // ── demote_topic ─────────────────────────────────────────────────────────

    #[test]
    fn demote_topic_moves_all_notes_and_removes_folder() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        // graphs uses distinct filenames to avoid conflict with algorithms/index.md
        write_note(&synthetic, "graphs/bfs.md", &minimal_note("graphs"));
        write_note(&synthetic, "graphs/dfs.md", &minimal_note("graphs"));
        write_note(&synthetic, "algorithms/index.md", &minimal_note("algorithms"));

        let op = DemoteTopicOp {
            from_topic: "graphs".to_string(),
            into_topic: "algorithms".to_string(),
            reason: "graphs is a subtopic".to_string(),
        };
        execute_demote_topic(&op, tmp.path(), "_synthetic").unwrap();

        assert!(!synthetic.join("graphs").exists(), "source folder should be deleted");
        assert!(synthetic.join("algorithms/index.md").exists()); // original untouched
        assert!(synthetic.join("algorithms/bfs.md").exists());
        assert!(synthetic.join("algorithms/dfs.md").exists());
        let bfs = std::fs::read_to_string(synthetic.join("algorithms/bfs.md")).unwrap();
        assert!(bfs.contains("topic: algorithms"));
    }

    #[test]
    fn demote_topic_fails_on_filename_conflict() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        // Both topics have an "index.md" — conflict
        write_note(&synthetic, "graphs/index.md", &minimal_note("graphs"));
        write_note(&synthetic, "algorithms/index.md", &minimal_note("algorithms"));

        let op = DemoteTopicOp {
            from_topic: "graphs".to_string(),
            into_topic: "algorithms".to_string(),
            reason: "test".to_string(),
        };
        assert!(execute_demote_topic(&op, tmp.path(), "_synthetic").is_err());
    }

    // ── merge_topics ─────────────────────────────────────────────────────────

    #[test]
    fn merge_topics_combines_multiple_sources() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "red-black/rb.md", &minimal_note("red-black"));
        write_note(&synthetic, "avl/avl.md", &minimal_note("avl"));
        write_note(&synthetic, "balanced-trees/index.md", &minimal_note("balanced-trees"));

        let op = MergeTopicsOp {
            from: vec!["red-black".to_string(), "avl".to_string()],
            into: "balanced-trees".to_string(),
            reason: "unify".to_string(),
        };
        execute_merge_topics(&op, tmp.path(), "_synthetic").unwrap();

        assert!(!synthetic.join("red-black").exists());
        assert!(!synthetic.join("avl").exists());
        assert!(synthetic.join("balanced-trees/rb.md").exists());
        assert!(synthetic.join("balanced-trees/avl.md").exists());
        assert!(synthetic.join("balanced-trees/index.md").exists());
    }

    #[test]
    fn merge_topics_creates_new_target_if_absent() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "topic-a/a.md", &minimal_note("topic-a"));
        write_note(&synthetic, "topic-b/b.md", &minimal_note("topic-b"));

        let op = MergeTopicsOp {
            from: vec!["topic-a".to_string(), "topic-b".to_string()],
            into: "combined".to_string(),
            reason: "new target".to_string(),
        };
        execute_merge_topics(&op, tmp.path(), "_synthetic").unwrap();

        assert!(synthetic.join("combined").is_dir());
        assert!(synthetic.join("combined/a.md").exists());
        assert!(synthetic.join("combined/b.md").exists());
    }

    // ── split_topic ──────────────────────────────────────────────────────────

    #[test]
    fn split_topic_distributes_atomics_to_new_topics() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));
        write_note(&synthetic, "algorithms/searching.md", &minimal_note("algorithms"));
        write_note(&synthetic, "algorithms/graphs.md", &minimal_note("algorithms"));

        let op = SplitTopicOp {
            from: "algorithms".to_string(),
            into: vec![
                crate::report::SplitTarget {
                    topic: "sorting".to_string(),
                    atomics: vec!["sorting".to_string()],
                },
                crate::report::SplitTarget {
                    topic: "searching".to_string(),
                    atomics: vec!["searching".to_string()],
                },
                crate::report::SplitTarget {
                    topic: "graphs".to_string(),
                    atomics: vec!["graphs".to_string()],
                },
            ],
            reason: "split into specific areas".to_string(),
        };
        execute_split_topic(&op, tmp.path(), "_synthetic").unwrap();

        // All three distributed — source folder removed
        assert!(!synthetic.join("algorithms").exists(), "source should be gone when emptied");
        assert!(synthetic.join("sorting/sorting.md").exists());
        assert!(synthetic.join("searching/searching.md").exists());
        assert!(synthetic.join("graphs/graphs.md").exists());
    }

    #[test]
    fn split_topic_leaves_remainder_in_source() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "algorithms/sorting.md", &minimal_note("algorithms"));
        write_note(&synthetic, "algorithms/searching.md", &minimal_note("algorithms"));

        // Only move sorting; searching stays in algorithms
        let op = SplitTopicOp {
            from: "algorithms".to_string(),
            into: vec![crate::report::SplitTarget {
                topic: "sorting".to_string(),
                atomics: vec!["sorting".to_string()],
            }],
            reason: "partial split".to_string(),
        };
        execute_split_topic(&op, tmp.path(), "_synthetic").unwrap();

        assert!(synthetic.join("algorithms/searching.md").exists(), "remainder should stay");
        assert!(synthetic.join("sorting/sorting.md").exists());
    }

    // ── set_embed ────────────────────────────────────────────────────────────

    #[test]
    fn set_embed_adds_embed_line_and_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");

        // Index note with no embeds
        write_note(
            &synthetic,
            "cs/cs.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: cs\ntitle: CS\n\
             embeds: []\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nIndex body.\n",
        );
        // Atomic note with block_id
        write_note(&synthetic, "cs/sorting.md", &minimal_note("cs"));

        let op = SetEmbedOp {
            atomic_path: "_synthetic/cs/sorting.md".to_string(),
            index_path: "_synthetic/cs/cs.md".to_string(),
            present: true,
        };
        execute_set_embed(&op, tmp.path()).unwrap();

        let content = std::fs::read_to_string(synthetic.join("cs/cs.md")).unwrap();
        assert!(content.contains("![[sorting#^test-id]]"), "embed line should be present");
        assert!(content.contains("embeds:"), "embeds frontmatter should be updated");
        assert!(content.contains("- sorting"), "sorting should be in embeds list");
    }

    #[test]
    fn set_embed_remove_is_idempotent_when_absent() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(
            &synthetic,
            "cs/cs.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: cs\ntitle: CS\n\
             embeds: []\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nIndex body.\n",
        );
        write_note(&synthetic, "cs/sorting.md", &minimal_note("cs"));

        let op = SetEmbedOp {
            atomic_path: "_synthetic/cs/sorting.md".to_string(),
            index_path: "_synthetic/cs/cs.md".to_string(),
            present: false,
        };
        // Should succeed even though embed doesn't exist
        assert!(execute_set_embed(&op, tmp.path()).is_ok());
        let content = std::fs::read_to_string(synthetic.join("cs/cs.md")).unwrap();
        assert!(!content.contains("![[sorting#^"));
    }

    // ── replace_note_links (precise matching) ────────────────────────────────

    #[test]
    fn replace_note_links_does_not_match_longer_prefix() {
        let content = "See [[topic/sorting]] and [[topic/sorting-redux]].";
        let result = replace_note_links(content, "topic/sorting", "topic/new-sorting");
        assert!(result.contains("[[topic/new-sorting]]"), "exact match should be replaced");
        assert!(result.contains("[[topic/sorting-redux]]"), "longer stem must not be replaced");
    }

    #[test]
    fn replace_note_links_handles_anchor_and_alias() {
        let content = "![[topic/note#^anchor]] and [[topic/note|alias]].";
        let result = replace_note_links(content, "topic/note", "other/note");
        assert!(result.contains("![[other/note#^anchor]]"));
        assert!(result.contains("[[other/note|alias]]"));
    }

    #[test]
    fn rewrite_links_only_touches_matching_files() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a.md");
        let b = tmp.path().join("b.md");
        std::fs::write(&a, "See [[old/thing]] and ![[old/embed]].\n").unwrap();
        std::fs::write(&b, "No matching links here.\n").unwrap();

        let changed = rewrite_links(tmp.path(), "old/", "new/").unwrap();
        assert_eq!(changed, 1);
        let content_a = std::fs::read_to_string(&a).unwrap();
        assert!(content_a.contains("[[new/thing]]"));
        assert!(content_a.contains("![[new/embed]]"));
        let content_b = std::fs::read_to_string(&b).unwrap();
        assert_eq!(content_b, "No matching links here.\n");
    }

    // ── execute_merge_topics (self-skip and conflict detection) ───────────────

    #[test]
    fn merge_topics_skips_into_topic_when_listed_in_from() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        write_note(&synthetic, "sorting/sorting.md", &minimal_note("sorting"));
        write_note(&synthetic, "algorithms/algo.md", &minimal_note("algorithms"));

        // "algorithms" appears in both `from` and `into`; it should be skipped.
        let op = MergeTopicsOp {
            from: vec!["sorting".into(), "algorithms".into()],
            into: "algorithms".into(),
            reason: "merge sorting into algorithms".into(),
        };
        execute_merge_topics(&op, tmp.path(), "_synthetic").unwrap();

        // sorting should be merged in; algorithms' own file stays untouched.
        assert!(!synthetic.join("sorting").exists(), "source topic should be removed");
        assert!(synthetic.join("algorithms/sorting.md").exists(), "sorting.md moved to algorithms");
        assert!(synthetic.join("algorithms/algo.md").exists(), "algo.md should be unchanged");
    }

    #[test]
    fn merge_topics_detects_filename_conflict_between_sources() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");
        // Both source topics have a file named "shared.md" → conflict.
        write_note(&synthetic, "topic-a/shared.md", &minimal_note("topic-a"));
        write_note(&synthetic, "topic-b/shared.md", &minimal_note("topic-b"));

        let op = MergeTopicsOp {
            from: vec!["topic-a".into(), "topic-b".into()],
            into: "merged".into(),
            reason: "conflict test".into(),
        };
        let err = execute_merge_topics(&op, tmp.path(), "_synthetic").unwrap_err();
        assert!(
            err.to_string().contains("shared.md") || err.to_string().contains("conflict"),
            "should report filename conflict: {err}"
        );
    }

    // ── move_topic_notes (.md filter) ─────────────────────────────────────────

    #[test]
    fn move_topic_notes_only_moves_md_files() {
        let tmp = TempDir::new().unwrap();
        let from = tmp.path().join("from");
        let to = tmp.path().join("to");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();

        std::fs::write(from.join("note.md"), &minimal_note("from")).unwrap();
        std::fs::write(from.join("readme.txt"), "not a note").unwrap();

        move_topic_notes(&from, &to, "from", "to").unwrap();

        assert!(to.join("note.md").exists(), "md file should be moved to target");
        assert!(!to.join("readme.txt").exists(), "non-md file must not be moved");
        assert!(from.join("readme.txt").exists(), "non-md file must stay in source");
    }

    // ── execute_set_embed (present: false removes existing embed) ─────────────

    #[test]
    fn set_embed_remove_clears_embed_from_index() {
        let tmp = TempDir::new().unwrap();
        let synthetic = tmp.path().join("_synthetic");

        // Index note that already contains the embed line and slug in frontmatter.
        write_note(
            &synthetic,
            "cs/cs.md",
            "---\ncsnotes_schema: 1\nkind: index\ntopic: cs\ntitle: CS\n\
             embeds:\n  - sorting\ncontributing_sessions: []\ncontributing_sources: []\n\
             created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\n\
             \nIndex body.\n\n![[sorting#^test-id]]\n",
        );
        write_note(&synthetic, "cs/sorting.md", &minimal_note("cs"));

        let op = SetEmbedOp {
            atomic_path: "_synthetic/cs/sorting.md".to_string(),
            index_path: "_synthetic/cs/cs.md".to_string(),
            present: false,
        };
        execute_set_embed(&op, tmp.path()).unwrap();

        let content = std::fs::read_to_string(synthetic.join("cs/cs.md")).unwrap();
        assert!(
            !content.contains("![[sorting#^test-id]]"),
            "embed line should be removed"
        );
        assert!(
            !content.contains("- sorting"),
            "sorting should be removed from embeds list in frontmatter"
        );
        assert!(content.contains("Index body."), "non-embed body content must be preserved");
    }

    // ── rewrite_links (|| vs &&) ──────────────────────────────────────────────

    #[test]
    fn rewrite_links_processes_file_with_wikilink_only() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("f.md");
        // File has only a plain wikilink — no embed.
        std::fs::write(&f, "See [[old/note]].\n").unwrap();

        let changed = rewrite_links(tmp.path(), "old/", "new/").unwrap();
        assert_eq!(changed, 1, "file with only a wikilink (no embed) must be updated");
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.contains("[[new/note]]"), "wikilink should be rewritten");
    }

    // ── rewrite_note_links (count) ────────────────────────────────────────────

    #[test]
    fn rewrite_note_links_returns_count_of_changed_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "See [[topic/sorting]].\n").unwrap();
        std::fs::write(tmp.path().join("b.md"), "No relevant links.\n").unwrap();

        let count = rewrite_note_links(tmp.path(), "topic/sorting", "topic/new-sorting").unwrap();
        assert_eq!(count, 1, "exactly one file should be reported as changed");

        let content = std::fs::read_to_string(tmp.path().join("a.md")).unwrap();
        assert!(content.contains("[[topic/new-sorting]]"), "link should be rewritten");
    }

    // ── relink_raw_notes ─────────────────────────────────────────────────────

    #[test]
    fn relink_raw_notes_rename_topic_rewrites_raw_notes() {
        let tmp = TempDir::new().unwrap();
        let raw = tmp.path().join("notes");
        std::fs::create_dir_all(&raw).unwrap();
        std::fs::write(raw.join("lecture.md"), "See [[inheritance/poly]].\n").unwrap();

        let ops = vec![Op::RenameTopic(RenameTopicOp {
            from: "inheritance".to_string(),
            to: "oop-basics".to_string(),
            reason: "test".to_string(),
        })];
        let roots = [raw.as_path()];
        let n = relink_raw_notes(&ops, &roots).unwrap();
        assert_eq!(n, 1);
        let content = std::fs::read_to_string(raw.join("lecture.md")).unwrap();
        assert!(content.contains("[[oop-basics/poly]]"));
        assert!(!content.contains("[[inheritance/"));
    }

    #[test]
    fn relink_raw_notes_rename_atomic_rewrites_raw_notes() {
        let tmp = TempDir::new().unwrap();
        let raw = tmp.path().join("notes");
        std::fs::create_dir_all(&raw).unwrap();
        std::fs::write(raw.join("lecture.md"), "See [[cs/old-slug]] and [[cs/old-slug|Label]].\n").unwrap();

        let ops = vec![Op::RenameAtomic(RenameAtomicOp {
            path: "_synthetic/cs/old-slug.md".to_string(),
            new_slug: "new-slug".to_string(),
            new_title: "New".to_string(),
            reason: "test".to_string(),
        })];
        let roots = [raw.as_path()];
        let n = relink_raw_notes(&ops, &roots).unwrap();
        assert_eq!(n, 1);
        let content = std::fs::read_to_string(raw.join("lecture.md")).unwrap();
        assert!(content.contains("[[cs/new-slug]]"));
        assert!(content.contains("[[cs/new-slug|Label]]"), "alias preserved");
        assert!(!content.contains("old-slug"));
    }

    #[test]
    fn relink_raw_notes_no_roots_returns_zero() {
        let ops = vec![Op::RenameTopic(RenameTopicOp {
            from: "a".to_string(),
            to: "b".to_string(),
            reason: "test".to_string(),
        })];
        let n = relink_raw_notes(&ops, &[]).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn relink_raw_notes_non_structural_ops_are_no_ops() {
        let tmp = TempDir::new().unwrap();
        let raw = tmp.path().join("notes");
        std::fs::create_dir_all(&raw).unwrap();
        std::fs::write(raw.join("lecture.md"), "Some content [[cs/note]].\n").unwrap();

        let ops = vec![
            Op::CreateNote(crate::report::CreateNoteOp {
                kind: crate::frontmatter::NoteKind::Atomic,
                path: "_synthetic/cs/note.md".to_string(),
                title: "Note".to_string(),
                topic: "cs".to_string(),
                block_id: Some("note-id".to_string()),
                embed_in: vec![],
                provenance: crate::frontmatter::ProvenanceDelta::default(),
                change_summary: "test".to_string(),
            }),
        ];
        let roots = [raw.as_path()];
        let n = relink_raw_notes(&ops, &roots).unwrap();
        assert_eq!(n, 0, "create_note should cause no rewrites");
        let content = std::fs::read_to_string(raw.join("lecture.md")).unwrap();
        assert!(content.contains("[[cs/note]]"), "content should be unchanged");
    }
}
