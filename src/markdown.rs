#![allow(dead_code)]
//! comrak-based Markdown parsing for Phase 1+.
//!
//! Phase 0 uses only the regex passes in `obsidian.rs`.  This module is a
//! Phase 1 stub — compiled but not called.
//!
//! Phase 1 adds:
//! - `parse_headings`: heading text + level + byte offset
//! - `derive_heading_scheme`: infer ["chapter", "section", ...] from depth
//! - `resolve_location`: "1.1.2" or heading text → {path, label, raw}

#[derive(Debug, Clone)]
pub struct Heading {
    pub level: u32,
    pub text: String,
    pub byte_offset: usize,
}

/// Parse headings from a Markdown string.
/// Phase 1 — not yet implemented.
pub fn parse_headings(_content: &str) -> Vec<Heading> {
    vec![]
}

/// Derive a human-readable heading scheme from a heading list.
/// Phase 1 — not yet implemented.
pub fn derive_heading_scheme(_headings: &[Heading]) -> Vec<String> {
    vec![]
}
