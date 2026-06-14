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
    BLOCK_ANCHOR_RE
        .captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
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
        let files = vec![
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
}
