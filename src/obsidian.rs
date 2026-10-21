#![allow(dead_code)]
/// Regex-based extraction of Obsidian-specific markdown syntax.
///
/// `comrak` parses CommonMark (+ GFM extensions).  Obsidian wiki-links
/// (`[[...]]`), block anchors (`^block-id`), and embed syntax (`![[...]]`)
/// are **not** CommonMark — they do not appear as AST nodes in comrak.
/// All Obsidian-specific extraction is done here with plain regex passes.
/// This is intentional and documented so future contributors don't try to
/// configure comrak to handle these.
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use anyhow::Result;
use walkdir::WalkDir;

// ── Regex definitions ─────────────────────────────────────────────────────────

/// Matches Obsidian block anchors: `^block-id` at the end of a line.
/// Block IDs are lowercase, start with a letter or digit, and contain only
/// letters, digits, and hyphens.
static BLOCK_ANCHOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)\^([a-z0-9][a-z0-9-]*)$").unwrap());

/// Matches `[[wikilink]]`, `[[wikilink#section]]`, and `[[wikilink|alias]]`
/// (but NOT `![[...]]`).
///
/// Group 1 captures an optional leading `!` (the embed sigil).
/// Group 2 captures the link target.  Callers filter out matches where
/// group 1 == `"!"` and split group 2 on `#` to extract the anchor.
///
/// Using a capturing group instead of a consuming `(?:^|[^!])` prefix
/// fixes adjacent-wikilink parsing: `[[a]][[b]]` — the old prefix consumed
/// the `]` after `[[a]]`, making `[[b]]` invisible to the next match.
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!?)\[\[([^\]|]+)(?:\|[^\]]*)?\]\]").unwrap());

/// Matches `![[embed]]` and `![[embed#^block-id]]`.
/// Captures the full inner content (file name + optional anchor).
static EMBED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[\[([^\]]+)\]\]").unwrap());

// ── Extraction functions ──────────────────────────────────────────────────────

/// Extract all block anchor IDs from a markdown string.
///
/// Returns a `Vec` of the ID strings (without the leading `^`), in the order
/// they appear in the file.
pub fn extract_block_ids(content: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(cap) = BLOCK_ANCHOR_RE.captures(line) {
            ids.push(cap[1].to_string());
        }
    }
    ids
}

/// A parsed wiki-link target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// The note name (no `#section`, no `|alias`).
    pub target: String,
    /// Optional section/anchor portion (everything after `#`).
    pub anchor: Option<String>,
}

/// Extract all `[[wikilinks]]` from a markdown string (excludes `![[embeds]]`).
pub fn extract_wikilinks(content: &str) -> Vec<WikiLink> {
    WIKILINK_RE
        .captures_iter(content)
        .filter(|cap| &cap[1] != "!")
        .map(|cap| {
            let inner = &cap[2];
            if let Some((target, anchor)) = inner.split_once('#') {
                WikiLink {
                    target: target.trim().to_string(),
                    anchor: Some(anchor.trim().to_string()),
                }
            } else {
                WikiLink {
                    target: inner.trim().to_string(),
                    anchor: None,
                }
            }
        })
        .collect()
}

/// A parsed embed target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedTarget {
    /// The note name (without `#`).
    pub file: String,
    /// Optional anchor, which may be a block-anchor (e.g. `^poly-core`).
    pub anchor: Option<String>,
}

impl EmbedTarget {
    /// True if the anchor is a block anchor (starts with `^`).
    pub fn is_block_anchor(&self) -> bool {
        self.anchor.as_deref().is_some_and(|a| a.starts_with('^'))
    }

    /// The block ID without the leading `^` (if this is a block anchor).
    pub fn block_id(&self) -> Option<&str> {
        self.anchor.as_deref().and_then(|a| a.strip_prefix('^'))
    }
}

/// Extract all `![[embeds]]` from a markdown string.
pub fn extract_embeds(content: &str) -> Vec<EmbedTarget> {
    EMBED_RE
        .captures_iter(content)
        .map(|cap| {
            let inner = &cap[1];
            if let Some((file, anchor)) = inner.split_once('#') {
                EmbedTarget {
                    file: file.trim().to_string(),
                    anchor: Some(anchor.trim().to_string()),
                }
            } else {
                EmbedTarget {
                    file: inner.trim().to_string(),
                    anchor: None,
                }
            }
        })
        .collect()
}

// ── Vault-wide block ID index ─────────────────────────────────────────────────

/// Scan an entire `_synthetic/` directory tree and return a map of
/// `block_id → relative_path` (relative to `synthetic_root`).
///
/// This is called before workspace assembly to populate the "all known block
/// IDs" section of `_session.md`, so the AI can avoid collisions.
pub fn collect_all_block_ids(synthetic_root: &Path) -> Result<HashMap<String, String>> {
    let mut index: HashMap<String, String> = HashMap::new();

    for entry in WalkDir::new(synthetic_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
    {
        let content = match crate::frontmatter::read_note(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = entry
            .path()
            .strip_prefix(synthetic_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .to_string();
        for id in extract_block_ids(&content) {
            // First occurrence wins for collision reporting purposes.
            index.entry(id).or_insert(rel.clone());
        }
    }

    Ok(index)
}

/// Check for block ID collisions within a set of `(content, path)` pairs.
/// Returns a map of `block_id → Vec<path>` for IDs that appear more than once.
pub fn find_collisions<'a>(
    files: impl Iterator<Item = (&'a str, &'a str)>,
) -> HashMap<String, Vec<String>> {
    let mut seen: HashMap<String, Vec<String>> = HashMap::new();
    for (content, path) in files {
        for id in extract_block_ids(content) {
            seen.entry(id).or_default().push(path.to_string());
        }
    }
    seen.into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// True when an atomic note's `^block_id` anchor sits at the very end of a
/// note that has more than a heading and an intro paragraph.
///
/// Obsidian attaches a block ID to the block immediately before it, so the
/// index's `![[note#^id]]` embed would transclude the note's last list or
/// paragraph instead of its intro summary.  The anchor belongs right after
/// the intro paragraph.  Short notes (heading + one paragraph) are exempt.
pub fn anchor_is_trailing(content: &str, block_id: &str) -> bool {
    let body = crate::frontmatter::split_frontmatter(content)
        .map(|(_, body)| body)
        .unwrap_or(content);
    let blocks: Vec<&str> = body
        .split("\n\n")
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .collect();
    let Some(last) = blocks.last() else {
        return false;
    };
    let anchor = format!("^{block_id}");
    let content_blocks = if *last == anchor {
        blocks.len() - 1
    } else if last.ends_with(&anchor)
        && last[..last.len() - anchor.len()].ends_with(char::is_whitespace)
    {
        blocks.len()
    } else {
        return false;
    };
    content_blocks > 2
}

/// Index (into `lines`) one past the end of the note's intro paragraph: the
/// first block after any leading headings/blank lines.  `None` when there is
/// no such block, or it opens a code fence (blank lines inside the fence
/// would make "end of paragraph" ambiguous — leave those for a human).
fn intro_end(lines: &[&str]) -> Option<usize> {
    let start = lines
        .iter()
        .position(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))?;
    let first = lines[start].trim_start();
    if first.starts_with("```") || first.starts_with("~~~") {
        return None;
    }
    Some(
        lines[start..]
            .iter()
            .position(|l| l.trim().is_empty())
            .map_or(lines.len(), |off| start + off),
    )
}

/// Insert `^block_id` on its own line directly after the intro paragraph,
/// with blank lines on both sides.  Works on a note body (no frontmatter).
fn insert_after_intro(lines: &[&str], block_id: &str) -> Option<String> {
    let end = intro_end(lines)?;
    let anchor = format!("^{block_id}");
    let mut out: Vec<&str> = lines[..end].to_vec();
    out.push("");
    out.push(&anchor);
    if end < lines.len() {
        out.extend_from_slice(&lines[end..]); // starts with the blank separator
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    Some(joined)
}

/// Split `content` into (frontmatter prefix, body) without re-serialising
/// the YAML, so a rewrite leaves the frontmatter byte-for-byte unchanged.
fn split_prefix(content: &str) -> (&str, &str) {
    match crate::frontmatter::split_frontmatter(content) {
        Some((_, body)) => content.split_at(content.len() - body.len()),
        None => ("", content),
    }
}

/// Rewrite a note whose `^block_id` anchor is trailing (see
/// [`anchor_is_trailing`]) so the anchor sits right after the intro
/// paragraph.  Handles both a standalone `^id` last line and an inline
/// `... ^id` at the end of the last paragraph.  Returns `None` when the note
/// can't be fixed mechanically (no trailing anchor, or no plain intro
/// paragraph to attach to).
pub fn move_anchor_after_intro(content: &str, block_id: &str) -> Option<String> {
    if !anchor_is_trailing(content, block_id) {
        return None;
    }
    let (prefix, body) = split_prefix(content);
    let anchor = format!("^{block_id}");
    let mut lines: Vec<&str> = body.lines().collect();
    let last = lines.iter().rposition(|l| !l.trim().is_empty())?;
    let last_line = lines[last].trim_end();
    if last_line.trim_start() == anchor {
        lines.truncate(last);
    } else if let Some(rest) = last_line.strip_suffix(&anchor) {
        if !rest.ends_with(char::is_whitespace) {
            return None;
        }
        lines[last] = rest.trim_end();
        lines.truncate(last + 1);
    } else {
        return None;
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    // Already anchored after the intro as well?  Then just drop the duplicate.
    if lines.iter().any(|l| l.trim() == anchor) {
        let mut joined = lines.join("\n");
        joined.push('\n');
        return Some(format!("{prefix}{joined}"));
    }
    Some(format!("{prefix}{}", insert_after_intro(&lines, block_id)?))
}

/// Add a missing `^block_id` anchor after the intro paragraph, falling back
/// to the end of the note when there's no plain intro paragraph.
pub fn insert_anchor(content: &str, block_id: &str) -> String {
    let (prefix, body) = split_prefix(content);
    let lines: Vec<&str> = body.lines().collect();
    if let Some(new_body) = insert_after_intro(&lines, block_id) {
        return format!("{prefix}{new_body}");
    }
    let trimmed = content.trim_end_matches('\n');
    format!("{trimmed}\n\n^{block_id}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_block_ids_basic() {
        let content = "Some text. ^my-anchor\nAnother line.\nMore content. ^second-id\n";
        let ids = extract_block_ids(content);
        assert_eq!(ids, vec!["my-anchor", "second-id"]);
    }

    #[test]
    fn block_id_must_be_at_line_end() {
        // `^foo` in the middle of a line is NOT a block anchor;
        // `^bar` at the true end of the line IS.
        let content = "The ^foo in the middle.\nLine ending with ^bar\n";
        let ids = extract_block_ids(content);
        assert_eq!(ids, vec!["bar"]);
    }

    #[test]
    fn block_id_uppercase_not_matched() {
        let content = "Some text. ^MyAnchor\n";
        let ids = extract_block_ids(content);
        assert!(ids.is_empty(), "block IDs must be lowercase");
    }

    #[test]
    fn block_ids_inside_fenced_code_are_ignored() {
        let content = "\
Real anchor. ^real-id
```java
int max = 2^31-1;
radius^2 = r * r;
```
Another real anchor. ^also-real
";
        let ids = extract_block_ids(content);
        assert_eq!(
            ids,
            vec!["real-id", "also-real"],
            "carets inside code fences should not be extracted as block IDs"
        );
    }

    #[test]
    fn block_ids_inside_tilde_fence_are_ignored() {
        let content = "Before. ^before-id\n~~~\n^inside-fence\n~~~\nAfter. ^after-id\n";
        let ids = extract_block_ids(content);
        assert_eq!(ids, vec!["before-id", "after-id"]);
    }

    #[test]
    fn extract_wikilinks_basic() {
        let content = "See [[inheritance]] and [[polymorphism#^poly-core]].";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "inheritance");
        assert_eq!(links[0].anchor, None);
        assert_eq!(links[1].target, "polymorphism");
        assert_eq!(links[1].anchor.as_deref(), Some("^poly-core"));
    }

    #[test]
    fn wikilinks_dont_match_embeds() {
        let content = "![[inheritance#^block]] and [[inheritance]].";
        let links = extract_wikilinks(content);
        // Only the plain wikilink, not the embed
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "inheritance");
    }

    #[test]
    fn extract_embeds_basic() {
        let content = "![[polymorphism#^poly-core]]\n![[method-declarations]]\n";
        let embeds = extract_embeds(content);
        assert_eq!(embeds.len(), 2);
        assert_eq!(embeds[0].file, "polymorphism");
        assert_eq!(embeds[0].anchor.as_deref(), Some("^poly-core"));
        assert!(embeds[0].is_block_anchor());
        assert_eq!(embeds[0].block_id(), Some("poly-core"));
        assert_eq!(embeds[1].file, "method-declarations");
        assert_eq!(embeds[1].anchor, None);
    }

    #[test]
    fn adjacent_wikilinks_both_matched() {
        let content = "[[link1]][[link2]]";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "link1");
        assert_eq!(links[1].target, "link2");
    }

    #[test]
    fn wikilink_with_alias_stripped() {
        let content = "[[inheritance|OOP Inheritance]]";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "inheritance");
    }

    #[test]
    fn collision_detection() {
        let files = [
            ("Line one ^foo\n", "file-a.md"),
            ("Line one ^foo\nLine two ^bar\n", "file-b.md"),
        ];
        let collisions = find_collisions(files.iter().map(|(c, p)| (*c, *p)));
        assert!(collisions.contains_key("foo"));
        assert!(!collisions.contains_key("bar"));
    }

    // ── Proptest: panic safety for arbitrary markdown input ───────────────────

    proptest::proptest! {
        /// `extract_block_ids` must never panic on arbitrary input and must
        /// only return IDs that actually appear in the input as `^<id>`.
        #[test]
        fn block_ids_no_panic_and_consistent(s in ".*") {
            let ids = extract_block_ids(&s);
            for id in &ids {
                // Every returned ID must appear in the original string
                // preceded by `^`.
                assert!(s.contains(&format!("^{}", id)));
            }
        }

        /// `extract_wikilinks` must never panic on arbitrary input and must
        /// only return targets whose text (with `[[`) appears in the input.
        #[test]
        fn wikilinks_no_panic_and_consistent(s in ".*") {
            let links = extract_wikilinks(&s);
            for link in &links {
                // The target (possibly with a path qualifier) must appear
                // somewhere in the original string.
                assert!(s.contains(&link.target) || link.target.is_empty());
            }
        }

        /// `extract_embeds` must never panic on arbitrary input and must only
        /// return file names whose text (inside `![[`) appears in the input.
        #[test]
        fn embeds_no_panic_and_consistent(s in ".*") {
            let embeds = extract_embeds(&s);
            for embed in &embeds {
                assert!(s.contains(&embed.file) || embed.file.is_empty());
            }
        }

        /// All three parsers together must handle extremely long inputs
        /// (e.g. unclosed brackets) without panicking or hanging.
        #[test]
        fn parsers_handle_long_malformed_input(
            prefix in "[\\[!]{0,4}",
            body in ".{0,200}",
        ) {
            let s = format!("{}{}", prefix, body);
            let _ = extract_block_ids(&s);
            let _ = extract_wikilinks(&s);
            let _ = extract_embeds(&s);
        }
    }

    // ── EmbedTarget::is_block_anchor ─────────────────────────────────────────

    #[test]
    fn is_block_anchor_true_for_caret_anchor() {
        let e = EmbedTarget {
            file: "note".into(),
            anchor: Some("^poly-core".into()),
        };
        assert!(e.is_block_anchor());
    }

    #[test]
    fn is_block_anchor_false_for_section_anchor() {
        let e = EmbedTarget {
            file: "note".into(),
            anchor: Some("Introduction".into()),
        };
        assert!(!e.is_block_anchor());
    }

    #[test]
    fn is_block_anchor_false_when_no_anchor() {
        let e = EmbedTarget {
            file: "note".into(),
            anchor: None,
        };
        assert!(!e.is_block_anchor());
    }

    // ── collect_all_block_ids ─────────────────────────────────────────────────

    #[test]
    fn collect_all_block_ids_returns_ids_from_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "Content.\n\n^id-one\n").unwrap();
        std::fs::write(tmp.path().join("b.md"), "Content.\n\n^id-two\n").unwrap();
        std::fs::write(tmp.path().join("ignore.txt"), "^id-txt\n").unwrap();

        let ids = collect_all_block_ids(tmp.path()).unwrap();
        assert!(ids.contains_key("id-one"), "id-one should be found");
        assert!(ids.contains_key("id-two"), "id-two should be found");
        assert!(!ids.contains_key("id-txt"), "non-md file should be ignored");
    }

    // ── anchor_is_trailing ───────────────────────────────────────────────────

    const FM: &str = "---\nkind: atomic\nblock_id: foo\n---\n";

    #[test]
    fn anchor_after_intro_is_fine() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n### More\n\n- a\n- b\n");
        assert!(!anchor_is_trailing(&c, "foo"));
    }

    #[test]
    fn standalone_anchor_at_end_is_trailing() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n### More\n\n- a\n- b\n\n^foo\n");
        assert!(anchor_is_trailing(&c, "foo"));
    }

    #[test]
    fn inline_anchor_on_last_block_is_trailing() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n### More\n\nLast para. ^foo\n");
        assert!(anchor_is_trailing(&c, "foo"));
    }

    #[test]
    fn short_note_with_anchor_at_end_is_fine() {
        let c = format!("{FM}\n## Foo\n\nIntro only.\n\n^foo\n");
        assert!(!anchor_is_trailing(&c, "foo"));
    }

    #[test]
    fn other_anchor_ids_do_not_count() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n### More\n\n- a ^foobar\n");
        assert!(!anchor_is_trailing(&c, "foo"));
    }

    // ── move_anchor_after_intro / insert_anchor ──────────────────────────────

    #[test]
    fn move_standalone_trailing_anchor() {
        let c =
            format!("{FM}\n## Foo\n\nIntro line one.\nline two.\n\n### More\n\n- a\n- b\n\n^foo\n");
        let fixed = move_anchor_after_intro(&c, "foo").unwrap();
        assert_eq!(
            fixed,
            format!("{FM}\n## Foo\n\nIntro line one.\nline two.\n\n^foo\n\n### More\n\n- a\n- b\n")
        );
        assert!(!anchor_is_trailing(&fixed, "foo"));
        assert!(fixed.starts_with(FM), "frontmatter must be untouched");
    }

    #[test]
    fn move_inline_trailing_anchor() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n### More\n\nLast para. ^foo\n");
        let fixed = move_anchor_after_intro(&c, "foo").unwrap();
        assert_eq!(
            fixed,
            format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n### More\n\nLast para.\n")
        );
    }

    #[test]
    fn move_drops_duplicate_when_intro_already_anchored() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n- a\n\n- b\n\n^foo\n");
        let fixed = move_anchor_after_intro(&c, "foo").unwrap();
        assert_eq!(
            fixed,
            format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n- a\n\n- b\n")
        );
    }

    #[test]
    fn move_refuses_when_intro_is_code_fence() {
        let c = format!("{FM}\n## Foo\n\n```java\nint x;\n\nint y;\n```\n\n- a\n\n- b\n\n^foo\n");
        assert!(move_anchor_after_intro(&c, "foo").is_none());
    }

    #[test]
    fn move_refuses_when_anchor_not_trailing() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n- a\n");
        assert!(move_anchor_after_intro(&c, "foo").is_none());
    }

    #[test]
    fn insert_anchor_goes_after_intro() {
        let c = format!("{FM}\n## Foo\n\nIntro.\n\n- a\n");
        assert_eq!(
            insert_anchor(&c, "foo"),
            format!("{FM}\n## Foo\n\nIntro.\n\n^foo\n\n- a\n")
        );
    }

    #[test]
    fn insert_anchor_falls_back_to_end() {
        let c = format!("{FM}\n```\ncode\n```\n");
        assert_eq!(
            insert_anchor(&c, "foo"),
            format!("{FM}\n```\ncode\n```\n\n^foo\n")
        );
    }
}
