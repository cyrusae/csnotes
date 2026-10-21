//! `csnotes sources` — manage registered source files.
//!
//! `sources` (no subcommand) is the inventory: every registered source,
//! grouped by course tag, with how many notes credit it.  `status` only
//! shows a one-line summary from the same numbers ([`summarize`]).
//!
//! `sources tag` writes course tags.  For Markdown sources the tag goes into
//! the file's YAML frontmatter (`courses:`), which is what `reconcile` reads,
//! so it survives `reconcile --reset`.  The manifest entry is updated at the
//! same time, so the change takes effect without a reconcile.  PDF/PPTX/DOCX
//! sources have no frontmatter; their tags live in the manifest only.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use crate::ui::Paint;
use anyhow::{bail, Context, Result};

use crate::config::{find_vault_root, VaultConfig};
use crate::manifest::{Manifest, ManifestLock, SourceEntry, SourceStatus};

pub enum SourcesSubcommand {
    List(ListArgs),
    Tag(TagArgs),
}

#[derive(Default)]
pub struct ListArgs {
    /// Only sources visible to this course (tagged with it, or untagged).
    pub course: Option<String>,
    /// Only unused sources (see [`Usage::Unused`]).
    pub unused: bool,
    /// Only dormant sources (listed in full instead of collapsed).
    pub dormant: bool,
    /// Only untagged sources.
    pub untagged: bool,
    pub json: bool,
}

pub struct TagArgs {
    /// Source IDs, directory prefixes, or `*` globs.
    pub patterns: Vec<String>,
    pub courses: Vec<String>,
    pub mode: TagMode,
    pub dry_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagMode {
    /// Add the courses to whatever is already there.
    Add,
    /// Replace the existing course list.
    Replace,
    /// Remove the courses (an empty result untags the source).
    Remove,
}

pub fn run(sub: SourcesSubcommand) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    match sub {
        SourcesSubcommand::List(args) => run_list(&vault_root, &config, &args),
        SourcesSubcommand::Tag(args) => run_tag(&vault_root, &config, &args),
    }
}

// ── inventory ─────────────────────────────────────────────────────────────────

/// How a source stands, from the student's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Usage {
    /// Marked "not read yet" in its frontmatter; kept out of workspaces.
    Staged,
    /// Tagged only with courses that are no longer active.
    Dormant,
    /// Credited by at least one note, or ingested by a `--source` run.
    Used,
    /// Available to sessions but nothing has drawn on it yet.
    Unused,
}

pub fn usage(entry: &SourceEntry, credits: usize, active_courses: &[String]) -> Usage {
    if entry.status == SourceStatus::Staged {
        Usage::Staged
    } else if entry.is_dormant(active_courses) {
        Usage::Dormant
    } else if credits > 0 || entry.status == SourceStatus::Processed {
        Usage::Used
    } else {
        Usage::Unused
    }
}

/// Number of notes under `synthetic_dir` whose frontmatter credits each
/// source ID.  Computed on demand, so it can't drift from the notes.
pub fn source_credits(vault_root: &Path, synthetic_dir: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    let root = vault_root.join(synthetic_dir);
    for entry in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let Ok(content) = crate::frontmatter::read_note(entry.path()) else {
            continue;
        };
        let Ok(fm) = crate::frontmatter::parse_frontmatter(&content, entry.path()) else {
            continue;
        };
        let ids: BTreeSet<&str> = fm
            .contributing_sources
            .iter()
            .map(|c| c.source_id.as_str())
            .collect();
        for id in ids {
            *counts.entry(id.to_string()).or_insert(0) += 1;
        }
    }
    counts
}

/// Totals for the one-line `status` summary.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct Summary {
    pub total: usize,
    pub used: usize,
    pub unused: usize,
    pub dormant: usize,
    pub staged: usize,
}

pub fn summarize(
    manifest: &Manifest,
    credits: &HashMap<String, usize>,
    active_courses: &[String],
) -> Summary {
    let mut s = Summary {
        total: manifest.sources.len(),
        ..Default::default()
    };
    for (id, e) in &manifest.sources {
        match usage(e, credits.get(id).copied().unwrap_or(0), active_courses) {
            Usage::Used => s.used += 1,
            Usage::Unused => s.unused += 1,
            Usage::Dormant => s.dormant += 1,
            Usage::Staged => s.staged += 1,
        }
    }
    s
}

#[derive(serde::Serialize)]
struct Row<'a> {
    id: &'a str,
    kind: &'static str,
    courses: &'a [String],
    usage: Usage,
    credited_by: usize,
}

fn run_list(vault_root: &Path, config: &VaultConfig, args: &ListArgs) -> Result<()> {
    let manifest = Manifest::load(vault_root)?;
    let credits = source_credits(vault_root, &config.synthetic_dir);
    let active = &config.active_courses;

    let rows: Vec<Row> = manifest
        .sources
        .iter()
        .map(|(id, e)| {
            let n = credits.get(id).copied().unwrap_or(0);
            Row {
                id,
                kind: e.kind.as_str(),
                courses: &e.courses,
                usage: usage(e, n, active),
                credited_by: n,
            }
        })
        .filter(|r| {
            args.course
                .as_deref()
                .is_none_or(|c| r.courses.is_empty() || r.courses.iter().any(|x| x == c))
                && (!args.unused || r.usage == Usage::Unused)
                && (!args.dormant || r.usage == Usage::Dormant)
                && (!args.untagged || r.courses.is_empty())
        })
        .collect();

    // Credited in notes but not registered (renamed/deleted source files).
    let mut orphans: Vec<(&String, &usize)> = credits
        .iter()
        .filter(|(id, _)| !manifest.sources.contains_key(id.as_str()))
        .collect();
    orphans.sort();

    if args.json {
        #[derive(serde::Serialize)]
        struct Out<'a> {
            active_courses: &'a [String],
            summary: Summary,
            sources: Vec<Row<'a>>,
            credited_but_unregistered: Vec<&'a String>,
        }
        let out = Out {
            active_courses: active,
            summary: summarize(&manifest, &credits, active),
            sources: rows,
            credited_but_unregistered: orphans.iter().map(|(id, _)| *id).collect(),
        };
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    let sum = summarize(&manifest, &credits, active);
    println!(
        "{}  {} registered · {} used · {} unused · {} dormant · {} staged{}",
        "Sources".bold(),
        sum.total,
        sum.used.to_string().green(),
        sum.unused.to_string().yellow(),
        sum.dormant.to_string().dimmed(),
        sum.staged.to_string().dimmed(),
        if active.is_empty() {
            String::new()
        } else {
            format!("   {}", format!("active: {}", active.join(", ")).dimmed())
        }
    );

    if rows.is_empty() {
        println!(
            "
No sources match those filters."
        );
        return Ok(());
    }

    // Group by course tag set.  Order: groups touching an active course,
    // then untagged, then dormant groups.
    let mut groups: BTreeMap<(u8, String), Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        let rank = if r.courses.is_empty() {
            1
        } else if r.courses.iter().any(|c| active.contains(c)) || active.is_empty() {
            0
        } else {
            2
        };
        groups
            .entry((rank, r.courses.join(", ")))
            .or_default()
            .push(r);
    }

    let width = rows.iter().map(|r| r.id.len()).max().unwrap_or(0).min(60);
    for ((rank, label), mut members) in groups {
        members.sort_by_key(|r| r.id);
        let title = match rank {
            1 => "Untagged (visible to every course)".to_string(),
            2 => format!("{label}  (dormant)"),
            _ => label,
        };
        println!(
            "
{}",
            title.bold()
        );
        if rank == 2 && !args.dormant {
            let used = members.iter().filter(|r| r.credited_by > 0).count();
            let staged = members.iter().filter(|r| r.usage == Usage::Staged).count();
            let staged_note = if staged > 0 {
                format!(" ({staged} of them staged)")
            } else {
                String::new()
            };
            println!(
                "  {}",
                format!(
                    "{} source(s){}, {} credited in notes — `csnotes sources --dormant` to list",
                    members.len(),
                    staged_note,
                    used
                )
                .dimmed()
            );
            continue;
        }
        for r in members {
            let (mark, note) = match r.usage {
                Usage::Used if r.credited_by > 0 => (
                    "✓".green().to_string(),
                    format!(
                        "{} note{}",
                        r.credited_by,
                        if r.credited_by == 1 { "" } else { "s" }
                    ),
                ),
                Usage::Used => ("✓".green().to_string(), "ingested".to_string()),
                Usage::Unused => ("·".yellow().to_string(), "unused".yellow().to_string()),
                Usage::Staged => (
                    "○".dimmed().to_string(),
                    "staged (unread)".dimmed().to_string(),
                ),
                Usage::Dormant if r.credited_by > 0 => (
                    "✓".dimmed().to_string(),
                    format!("{} notes", r.credited_by).dimmed().to_string(),
                ),
                Usage::Dormant => ("·".dimmed().to_string(), "unused".dimmed().to_string()),
            };
            println!(
                "  {} {:<width$}  {:<16} {}",
                mark,
                r.id,
                r.kind.dimmed(),
                note,
                width = width
            );
        }
    }

    if !orphans.is_empty() {
        println!(
            "
{}",
            "Credited in notes but not registered".bold()
        );
        for (id, n) in orphans {
            println!("  {} {}  {}", "?".red(), id, format!("{n} notes").dimmed());
        }
        println!(
            "  {}",
            "(renamed or deleted source files — `csnotes audit` / `reconcile` to check)".dimmed()
        );
    }
    Ok(())
}

// ── tag ───────────────────────────────────────────────────────────────────────

/// One planned change.
#[derive(Debug, PartialEq)]
pub struct TagChange {
    pub source_id: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    /// True when the source has no frontmatter to write (PDF/PPTX/DOCX).
    pub manifest_only: bool,
}

fn run_tag(vault_root: &Path, config: &VaultConfig, args: &TagArgs) -> Result<()> {
    if args.courses.is_empty() && args.mode != TagMode::Replace {
        bail!("give at least one --course (or use --replace with none to untag)");
    }
    let _lock = ManifestLock::acquire(vault_root)?;
    let mut manifest = Manifest::load(vault_root)?;

    let ids = match_sources(&args.patterns, &manifest)?;
    let changes = plan_tags(&ids, &manifest, &args.courses, args.mode);

    for course in unknown_courses(&args.courses, config, &manifest) {
        eprintln!(
            "  {} '{}' isn't an active course or a course seen in sessions/sources — typo?",
            "warn:".yellow(),
            course
        );
    }

    let unchanged = ids.len() - changes.len();
    if changes.is_empty() {
        println!(
            "Nothing to change ({} source(s) already tagged that way).",
            unchanged
        );
        return Ok(());
    }

    for c in &changes {
        println!(
            "  {} {}  {} {} {}{}",
            "~".yellow().bold(),
            c.source_id,
            fmt_courses(&c.before).dimmed(),
            "→".dimmed(),
            fmt_courses(&c.after),
            if c.manifest_only {
                "  (manifest only)".dimmed().to_string()
            } else {
                String::new()
            }
        );
    }
    if unchanged > 0 {
        println!(
            "  {}",
            format!("{} source(s) already tagged that way", unchanged).dimmed()
        );
    }

    if args.dry_run {
        println!("\nDry run — nothing written.  Re-run without --dry-run to apply.");
        return Ok(());
    }

    let mut manifest_only = 0;
    for c in &changes {
        let entry = manifest
            .sources
            .get_mut(&c.source_id)
            .expect("planned source exists in manifest");
        if !c.manifest_only {
            let path = vault_root.join(&entry.path);
            let content = crate::frontmatter::read_note(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let updated = set_frontmatter_courses(&content, &c.after)
                .with_context(|| format!("updating frontmatter of {}", path.display()))?;
            std::fs::write(&path, updated)
                .with_context(|| format!("writing {}", path.display()))?;
        } else {
            manifest_only += 1;
        }
        entry.courses = c.after.clone();
    }
    manifest.save(vault_root)?;

    println!("\nTagged {} source(s).", changes.len());
    if manifest_only > 0 {
        println!(
            "  {} of them are PDF/PPTX/DOCX: the tag lives in csnotes.json only, so \
             `reconcile --reset` would drop it.",
            manifest_only
        );
    }
    Ok(())
}

fn fmt_courses(c: &[String]) -> String {
    if c.is_empty() {
        "(untagged)".to_string()
    } else {
        format!("[{}]", c.join(", "))
    }
}

/// Resolve patterns to source IDs.  Each pattern is an exact ID, a directory
/// prefix (`Textbooks/SICP` → everything under it), or a glob where `*`
/// matches within one path segment (`AI-Conversations/*/Java-*`).
/// A pattern that matches nothing is an error, so typos don't silently no-op.
pub fn match_sources(patterns: &[String], manifest: &Manifest) -> Result<Vec<String>> {
    let mut out = BTreeSet::new();
    for raw in patterns {
        let pat = raw.trim_end_matches('/');
        let matched: Vec<&String> = if pat.contains('*') {
            manifest
                .sources
                .keys()
                .filter(|id| glob_match(pat, id))
                .collect()
        } else if let Some((id, _)) = manifest.sources.get_key_value(pat) {
            vec![id]
        } else {
            let prefix = format!("{pat}/");
            manifest
                .sources
                .keys()
                .filter(|id| id.starts_with(&prefix))
                .collect()
        };
        if matched.is_empty() {
            bail!(
                "'{}' matches no registered source (exact ID, folder prefix, or * glob). \
                 Run `csnotes reconcile` if the file is new.",
                raw
            );
        }
        out.extend(matched.into_iter().cloned());
    }
    Ok(out.into_iter().collect())
}

/// Glob match where `*` matches any run of characters except `/`, so
/// `Folder/*` matches direct children only and `*/Java-*` stays one level deep.
fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => {
                // Try every split point that doesn't cross a '/'.
                let mut i = 0;
                loop {
                    if go(rest, &t[i..]) {
                        return true;
                    }
                    if i == t.len() || t[i] == b'/' {
                        return false;
                    }
                    i += 1;
                }
            }
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

/// Compute the changes for `ids`, skipping sources whose tags wouldn't change.
pub fn plan_tags(
    ids: &[String],
    manifest: &Manifest,
    courses: &[String],
    mode: TagMode,
) -> Vec<TagChange> {
    ids.iter()
        .filter_map(|id| {
            let entry = manifest.sources.get(id)?;
            let before = entry.courses.clone();
            let after = apply_mode(&before, courses, mode);
            if after == before {
                return None;
            }
            Some(TagChange {
                source_id: id.clone(),
                before,
                after,
                manifest_only: !entry.path.ends_with(".md"),
            })
        })
        .collect()
}

fn apply_mode(existing: &[String], courses: &[String], mode: TagMode) -> Vec<String> {
    let mut out: Vec<String> = match mode {
        TagMode::Replace => vec![],
        TagMode::Add | TagMode::Remove => existing.to_vec(),
    };
    match mode {
        TagMode::Add | TagMode::Replace => {
            for c in courses {
                if !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        TagMode::Remove => out.retain(|c| !courses.contains(c)),
    }
    out
}

/// Courses that don't appear anywhere the vault knows about.
fn unknown_courses(courses: &[String], config: &VaultConfig, manifest: &Manifest) -> Vec<String> {
    let mut known: BTreeSet<&str> = config.active_courses.iter().map(String::as_str).collect();
    known.extend(manifest.sessions.values().map(|s| s.course.as_str()));
    known.extend(
        manifest
            .sources
            .values()
            .flat_map(|s| s.courses.iter().map(String::as_str)),
    );
    courses
        .iter()
        .filter(|c| !known.contains(c.as_str()))
        .cloned()
        .collect()
}

/// Rewrite (or add, or remove) the top-level `courses:` key in a Markdown
/// file's YAML frontmatter, leaving every other line byte-for-byte intact.
/// Adds a frontmatter block if the file has none.  An empty `courses` list
/// removes the key.
///
/// The result is re-parsed and must read back as exactly `courses`; any
/// mismatch is an error and nothing should be written.
pub fn set_frontmatter_courses(content: &str, courses: &[String]) -> Result<String> {
    let block: String = if courses.is_empty() {
        String::new()
    } else {
        let mut b = String::from("courses:\n");
        for c in courses {
            b.push_str(&format!("  - {}\n", yaml_scalar(c)));
        }
        b
    };

    let updated = match crate::frontmatter::split_frontmatter(content) {
        None => {
            if courses.is_empty() {
                content.to_string()
            } else {
                format!("---\n{block}---\n{content}")
            }
        }
        Some((yaml, body)) => {
            let lines: Vec<&str> = yaml.lines().collect();
            let key_line = lines
                .iter()
                .position(|l| l.starts_with("courses:") && is_key(l, "courses"));
            let mut new_yaml = String::new();
            match key_line {
                Some(k) => {
                    // The key's value runs until the next line that starts a
                    // new top-level key (column 0, not a "- " list item, not
                    // blank/comment).
                    let end = lines[k + 1..]
                        .iter()
                        .position(|l| starts_top_level_key(l))
                        .map_or(lines.len(), |off| k + 1 + off);
                    for l in &lines[..k] {
                        new_yaml.push_str(l);
                        new_yaml.push('\n');
                    }
                    new_yaml.push_str(&block);
                    for l in &lines[end..] {
                        new_yaml.push_str(l);
                        new_yaml.push('\n');
                    }
                }
                None => {
                    for l in &lines {
                        new_yaml.push_str(l);
                        new_yaml.push('\n');
                    }
                    new_yaml.push_str(&block);
                }
            }
            format!("---\n{new_yaml}---\n{body}")
        }
    };

    let read_back = read_courses(&updated);
    if read_back != courses {
        bail!(
            "frontmatter edit didn't round-trip (wanted {:?}, read back {:?}); \
             file left unchanged — edit its `courses:` by hand",
            courses,
            read_back
        );
    }
    Ok(updated)
}

/// `courses` as reconcile would read it.
fn read_courses(content: &str) -> Vec<String> {
    let Some((yaml, _)) = crate::frontmatter::split_frontmatter(content) else {
        return vec![];
    };
    let Ok(serde_yml::Value::Mapping(map)) = serde_yml::from_str::<serde_yml::Value>(yaml) else {
        return vec![];
    };
    match map.get("courses") {
        Some(serde_yml::Value::Sequence(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .collect(),
        Some(serde_yml::Value::String(s)) => vec![s.trim().to_string()],
        _ => vec![],
    }
}

fn is_key(line: &str, key: &str) -> bool {
    line.strip_prefix(key)
        .is_some_and(|rest| rest.starts_with(':'))
}

fn starts_top_level_key(line: &str) -> bool {
    let first = line.chars().next();
    match first {
        None => false,
        Some(c) if c.is_whitespace() => false,
        Some('-' | '#') => false,
        _ => line.contains(':'),
    }
}

/// Quote a course name if YAML would otherwise misread it.
fn yaml_scalar(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !s.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "on" | "off" | "~"
        );
    if plain {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestConfig, SourceEntry, SourceKind, SourceStatus};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn manifest_with(ids: &[(&str, &str, &[&str])]) -> Manifest {
        let cfg: ManifestConfig = serde_json::from_str(
            r#"{"raw_dir":"notes","recordings_dir":"recordings","artifacts_dir":"artifacts",
                "sources_dir":"sources","synthetic_dir":"_synthetic","generated_dir":"_generated",
                "filename_format":"{course}-{mm}-{dd}","default_backend":"mock",
                "skill_variant":"claude","snapshot_mode":"pre_merge"}"#,
        )
        .unwrap();
        let mut m = Manifest::empty(std::path::PathBuf::from("/v"), cfg);
        for (id, path, courses) in ids {
            m.sources.insert(
                id.to_string(),
                SourceEntry {
                    path: path.to_string(),
                    kind: SourceKind::Textbook,
                    status: SourceStatus::Unprocessed,
                    last_processed_at: None,
                    heading_scheme: vec![],
                    topics_updated: vec![],
                    summary: None,
                    tags: vec![],
                    courses: s(courses),
                },
            );
        }
        m
    }

    // ── matching ──

    #[test]
    fn glob_star_stays_within_segment() {
        assert!(glob_match("AI/*/Java-*", "AI/Claude/Java-boxed"));
        assert!(glob_match("AI/*/Java-*", "AI/Gemini/Java-x"));
        assert!(!glob_match("AI/*/Java-*", "AI/Claude/Cpp-x"));
        assert!(
            !glob_match("AI/*", "AI/Claude/Java-x"),
            "* must not cross '/'"
        );
        assert!(glob_match("*", "Other"));
        assert!(!glob_match("Java-*", "AI/Claude/Java-x"));
    }

    #[test]
    fn match_sources_exact_prefix_glob_and_dedup() {
        let m = manifest_with(&[
            (
                "Textbooks/SOJ/Chapter-01",
                "sources/Textbooks/SOJ/Chapter-01.md",
                &[],
            ),
            (
                "Textbooks/SOJ/Chapter-02",
                "sources/Textbooks/SOJ/Chapter-02.md",
                &[],
            ),
            ("AI/Claude/Java-a", "sources/AI/Claude/Java-a.md", &[]),
            ("AI/Claude/Cpp-b", "sources/AI/Claude/Cpp-b.md", &[]),
        ]);
        let got = match_sources(
            &s(&["Textbooks/SOJ/", "AI/*/Java-*", "Textbooks/SOJ/Chapter-01"]),
            &m,
        )
        .unwrap();
        assert_eq!(
            got,
            s(&[
                "AI/Claude/Java-a",
                "Textbooks/SOJ/Chapter-01",
                "Textbooks/SOJ/Chapter-02"
            ])
        );
    }

    #[test]
    fn match_sources_errors_on_no_match() {
        let m = manifest_with(&[("A/b", "sources/A/b.md", &[])]);
        let err = match_sources(&s(&["A/nope*"]), &m).unwrap_err().to_string();
        assert!(err.contains("matches no registered source"), "{err}");
        // "A" as a prefix must not match "AB/..." style siblings.
        let m = manifest_with(&[("AB/x", "sources/AB/x.md", &[])]);
        assert!(match_sources(&s(&["A"]), &m).is_err());
    }

    // ── planning ──

    #[test]
    fn plan_add_replace_remove() {
        let m = manifest_with(&[("a", "sources/a.md", &["X"]), ("b", "sources/b.pdf", &[])]);
        let ids = s(&["a", "b"]);

        let add = plan_tags(&ids, &m, &s(&["X", "Y"]), TagMode::Add);
        assert_eq!(add[0].after, s(&["X", "Y"]));
        assert_eq!(add[1].after, s(&["X", "Y"]));
        assert!(add[1].manifest_only, "pdf has no frontmatter");

        let rep = plan_tags(&ids, &m, &s(&["Z"]), TagMode::Replace);
        assert_eq!(rep[0].after, s(&["Z"]));

        let rem = plan_tags(&ids, &m, &s(&["X"]), TagMode::Remove);
        assert_eq!(rem.len(), 1, "b has nothing to remove → no change");
        assert!(rem[0].after.is_empty());
    }

    #[test]
    fn plan_skips_already_tagged() {
        let m = manifest_with(&[("a", "sources/a.md", &["X"])]);
        assert!(plan_tags(&s(&["a"]), &m, &s(&["X"]), TagMode::Add).is_empty());
    }

    // ── frontmatter editing ──

    #[test]
    fn set_courses_replaces_block_list_and_keeps_other_lines() {
        let before = "---\ntitle: \"Intro\"\ncourses:\n  - CPSC5001\n  - CPSC5002\nstatus: \"completed\"\n---\nBody\n";
        let after = set_frontmatter_courses(before, &s(&["CPSC5005"])).unwrap();
        assert_eq!(
            after,
            "---\ntitle: \"Intro\"\ncourses:\n  - CPSC5005\nstatus: \"completed\"\n---\nBody\n"
        );
    }

    #[test]
    fn set_courses_handles_flow_and_unindented_lists() {
        let flow = "---\ncourses: [A, B]\ntags: [x]\n---\nB\n";
        assert_eq!(
            set_frontmatter_courses(flow, &s(&["C"])).unwrap(),
            "---\ncourses:\n  - C\ntags: [x]\n---\nB\n"
        );
        let unindented = "---\ncourses:\n- A\n- B\ntags: [x]\n---\nB\n";
        assert_eq!(
            set_frontmatter_courses(unindented, &s(&["C"])).unwrap(),
            "---\ncourses:\n  - C\ntags: [x]\n---\nB\n"
        );
    }

    #[test]
    fn set_courses_appends_when_absent_and_removes_when_empty() {
        let fm = "---\ncreated: 2026-06-06\nsummary: \"x: y\"\n---\n> body\n";
        let added = set_frontmatter_courses(fm, &s(&["CPSC5005"])).unwrap();
        assert_eq!(
            added,
            "---\ncreated: 2026-06-06\nsummary: \"x: y\"\ncourses:\n  - CPSC5005\n---\n> body\n"
        );
        assert_eq!(set_frontmatter_courses(&added, &[]).unwrap(), fm);
    }

    #[test]
    fn set_courses_adds_frontmatter_to_bare_file() {
        assert_eq!(
            set_frontmatter_courses("# Title\n", &s(&["A"])).unwrap(),
            "---\ncourses:\n  - A\n---\n# Title\n"
        );
        assert_eq!(
            set_frontmatter_courses("# Title\n", &[]).unwrap(),
            "# Title\n"
        );
    }

    #[test]
    fn set_courses_does_not_touch_similar_keys() {
        let fm = "---\ncourses_note: keep\ncourses: [A]\n---\n";
        assert_eq!(
            set_frontmatter_courses(fm, &s(&["B"])).unwrap(),
            "---\ncourses_note: keep\ncourses:\n  - B\n---\n"
        );
    }

    #[test]
    fn yaml_scalar_quotes_risky_names() {
        assert_eq!(yaml_scalar("CPSC5005"), "CPSC5005");
        assert_eq!(yaml_scalar("5005"), "\"5005\"");
        assert_eq!(yaml_scalar("yes"), "\"yes\"");
        assert_eq!(yaml_scalar("CS 101"), "\"CS 101\"");
        let fm = set_frontmatter_courses("---\ntitle: x\n---\n", &s(&["5005", "no"])).unwrap();
        assert_eq!(read_courses(&fm), s(&["5005", "no"]));
    }

    // ── inventory ──

    #[test]
    fn usage_classification() {
        let m = manifest_with(&[
            ("old", "sources/old.md", &["CPSC5001"]),
            ("new", "sources/new.md", &["CPSC5005"]),
            ("free", "sources/free.md", &[]),
        ]);
        let active = s(&["CPSC5005"]);
        assert_eq!(usage(&m.sources["old"], 3, &active), Usage::Dormant);
        assert_eq!(usage(&m.sources["new"], 0, &active), Usage::Unused);
        assert_eq!(usage(&m.sources["new"], 1, &active), Usage::Used);
        let mut staged = m.sources["free"].clone();
        staged.status = SourceStatus::Staged;
        assert_eq!(usage(&staged, 5, &active), Usage::Staged);
        let mut ingested = m.sources["free"].clone();
        ingested.status = SourceStatus::Processed;
        assert_eq!(usage(&ingested, 0, &active), Usage::Used);
    }

    #[test]
    fn summarize_counts_each_bucket() {
        let m = manifest_with(&[
            ("old", "sources/old.md", &["CPSC5001"]),
            ("new", "sources/new.md", &["CPSC5005"]),
            ("free", "sources/free.md", &[]),
        ]);
        let credits: HashMap<String, usize> = [("free".to_string(), 2)].into();
        let sum = summarize(&m, &credits, &s(&["CPSC5005"]));
        assert_eq!(
            sum,
            Summary {
                total: 3,
                used: 1,
                unused: 1,
                dormant: 1,
                staged: 0
            }
        );
    }

    #[test]
    fn source_credits_counts_notes_once_each() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("_synthetic/java");
        std::fs::create_dir_all(&dir).unwrap();
        let note = |srcs: &[&str]| {
            let mut fm = String::from(
                "---\ncsnotes_schema: 1\nkind: atomic\ntopic: java\ntitle: T\nblock_id: t\n\
                 contributing_sessions: []\ncontributing_sources:\n",
            );
            for id in srcs {
                fm.push_str(&format!(
                    "  - source_id: {id}\n    relationship: introduced\n"
                ));
            }
            fm.push_str("created: \"2026-01-01T00:00:00Z\"\nlast_updated: \"2026-01-01T00:00:00Z\"\n---\nBody\n");
            fm
        };
        std::fs::write(dir.join("a.md"), note(&["Books/X/Ch1", "Books/X/Ch1"])).unwrap();
        std::fs::write(dir.join("b.md"), note(&["Books/X/Ch1", "AI/c"])).unwrap();
        std::fs::write(dir.join("broken.md"), "no frontmatter").unwrap();
        let c = source_credits(tmp.path(), "_synthetic");
        assert_eq!(
            c.get("Books/X/Ch1"),
            Some(&2),
            "duplicate credit in one note counts once"
        );
        assert_eq!(c.get("AI/c"), Some(&1));
    }
}
