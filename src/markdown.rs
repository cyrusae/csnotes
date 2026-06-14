use comrak::nodes::{AstNode, NodeValue};
/// comrak-based Markdown heading parsing for source ingestion.
///
/// Used by `reconcile` when registering a new source file to derive the
/// heading scheme (e.g. ["chapter", "section", "subsection"]) that will be
/// stored in `SourceEntry::heading_scheme`.  The scheme is later surfaced in
/// the `_session.md` briefing so the AI can reference textbook locations
/// precisely.
use comrak::{parse_document, Arena, Options};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u32,
    pub text: String,
    /// Byte offset of the heading node within the original source string.
    pub byte_offset: usize,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse all ATX and setext headings from a Markdown string.
///
/// Returns headings in document order.
pub fn parse_headings(content: &str) -> Vec<Heading> {
    let arena = Arena::new();
    let root = parse_document(&arena, content, &Options::default());
    let mut headings = Vec::new();
    collect_headings(root, content, &mut headings);
    headings
}

/// Infer a human-readable heading scheme from a heading list.
///
/// Finds the distinct heading levels that appear in the document, sorts them,
/// and maps them to conventional academic labels:
///
/// | Rank | Label        |
/// |------|-------------|
/// | 1st  | `chapter`   |
/// | 2nd  | `section`   |
/// | 3rd  | `subsection`|
/// | 4th+ | `level{n}`  |
///
/// Returns an empty vec when there are no headings.  Skipped levels are
/// collapsed — a document using only h1 and h3 yields `["chapter", "section"]`.
pub fn derive_heading_scheme(headings: &[Heading]) -> Vec<String> {
    use std::collections::BTreeSet;

    let levels: BTreeSet<u32> = headings.iter().map(|h| h.level).collect();
    levels
        .into_iter()
        .enumerate()
        .map(|(rank, _level)| match rank {
            0 => "chapter".to_string(),
            1 => "section".to_string(),
            2 => "subsection".to_string(),
            n => format!("level{}", n + 1),
        })
        .collect()
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn collect_headings<'a>(node: &'a AstNode<'a>, source: &str, out: &mut Vec<Heading>) {
    if let NodeValue::Heading(ref h) = node.data.borrow().value {
        let level = h.level as u32;
        let byte_offset = node.data.borrow().sourcepos.start.line.saturating_sub(1);
        // Collect the plain text of all inline children.
        let text = collect_text(node);
        // Convert 1-based line number to a rough byte offset by counting newlines.
        let byte_offset = line_to_byte_offset(source, byte_offset);
        out.push(Heading {
            level,
            text,
            byte_offset,
        });
    }
    for child in node.children() {
        collect_headings(child, source, out);
    }
}

/// Recursively collect plain text from an AST node's inline children.
fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Text(t) => buf.push_str(t),
            NodeValue::Code(c) => buf.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
            _ => buf.push_str(&collect_text(child)),
        }
    }
    buf
}

/// Convert a 0-based line index to the byte offset of that line's start.
fn line_to_byte_offset(source: &str, line_idx: usize) -> usize {
    source
        .char_indices()
        .filter(|&(_, c)| c == '\n')
        .map(|(i, _)| i + 1)
        .nth(line_idx.saturating_sub(1))
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_atx_headings() {
        let md = "# Chapter 1\n\nSome text.\n\n## Section 1.1\n\n### Sub 1.1.1\n";
        let headings = parse_headings(md);
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "Chapter 1");
        assert_eq!(headings[1].level, 2);
        assert_eq!(headings[1].text, "Section 1.1");
        assert_eq!(headings[2].level, 3);
        assert_eq!(headings[2].text, "Sub 1.1.1");
    }

    #[test]
    fn derive_scheme_three_levels() {
        let md = "# Chapter 1\n## Section 1.1\n### Sub 1.1.1\n";
        let h = parse_headings(md);
        assert_eq!(
            derive_heading_scheme(&h),
            vec!["chapter", "section", "subsection"]
        );
    }

    #[test]
    fn derive_scheme_two_levels() {
        let md = "# Part I\n## Chapter 1\n## Chapter 2\n";
        let h = parse_headings(md);
        assert_eq!(derive_heading_scheme(&h), vec!["chapter", "section"]);
    }

    #[test]
    fn derive_scheme_skipped_level() {
        // Document uses h1 and h3 only — still gets chapter/section labels
        // based on rank, not absolute level.
        let md = "# Top\n### Deep\n";
        let h = parse_headings(md);
        assert_eq!(derive_heading_scheme(&h), vec!["chapter", "section"]);
    }

    #[test]
    fn derive_scheme_empty() {
        let h = parse_headings("Just a paragraph.\n");
        assert!(derive_heading_scheme(&h).is_empty());
    }

    #[test]
    fn heading_text_with_inline_code() {
        let md = "## The `Option` Type\n";
        let h = parse_headings(md);
        assert_eq!(h[0].text, "The Option Type");
    }

    // ── line_to_byte_offset ───────────────────────────────────────────────────

    #[test]
    fn line_to_byte_offset_returns_correct_position() {
        // "abc\ndef\nghi\n": '\n' at byte 3, 7, 11.
        // map(i → i+1): [4, 8, 12].
        // line_idx uses saturating_sub(1), so idx 0 and 1 both resolve to nth(0)=4.
        let source = "abc\ndef\nghi\n";
        assert_eq!(
            line_to_byte_offset(source, 1),
            4,
            "idx 1 → start of second line"
        );
        assert_eq!(
            line_to_byte_offset(source, 2),
            8,
            "idx 2 → start of third line"
        );
        assert_eq!(
            line_to_byte_offset(source, 99),
            0,
            "out-of-range → unwrap_or(0)"
        );
    }
}
