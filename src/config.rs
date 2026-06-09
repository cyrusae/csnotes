#![allow(dead_code)]
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use std::sync::OnceLock;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::CsnotesError;

// ── Enums ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AiBackend {
    Claude,
    Agy,
    Mock,
}

impl std::fmt::Display for AiBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiBackend::Claude => write!(f, "claude"),
            AiBackend::Agy => write!(f, "agy"),
            AiBackend::Mock => write!(f, "mock"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillVariant {
    Claude,
    Gemini,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    PreMerge,
    ShadowGit,
}

// ── VaultConfig ───────────────────────────────────────────────────────────────

/// Contents of `.csnotes` — the TOML config file at the vault root.
/// `vault_root` is NOT stored here; it is derived from the file's location.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VaultConfig {
    // Directory names (relative to vault root)
    #[serde(default = "default_raw_dir")]
    pub raw_dir: String,
    #[serde(default = "default_plaud_dir")]
    pub plaud_dir: String,
    #[serde(default = "default_artifacts_dir")]
    pub artifacts_dir: String,
    #[serde(default = "default_sources_dir")]
    pub sources_dir: String,
    #[serde(default = "default_synthetic_dir")]
    pub synthetic_dir: String,
    #[serde(default = "default_generated_dir")]
    pub generated_dir: String,
    #[serde(default = "default_csnotes_dir")]
    pub csnotes_dir: String,

    /// Filename template for raw notes and Plaud exports.
    /// Tokens: `{course}`, `{yyyy}`, `{mm}`, `{dd}`.  Must contain `{course}`
    /// and at least one date token; all tokens separated by `-`.
    #[serde(default = "default_filename_format")]
    pub filename_format: String,

    #[serde(default)]
    pub active_courses: Vec<String>,

    /// Default AI backend; overridden per-run by `--backend` / `-m`.
    #[serde(default = "default_backend")]
    pub default_backend: AiBackend,

    #[serde(default = "default_skill_variant")]
    pub skill_variant: SkillVariant,

    #[serde(default = "default_snapshot_mode")]
    pub snapshot_mode: SnapshotMode,

    /// Weeks of inactivity before `status` nudges to archive a course.
    #[serde(default = "default_archive_threshold_weeks")]
    pub archive_threshold_weeks: u32,

    /// Recognized Plaud export qualifiers (beyond always-recognized single
    /// letters a–z).  Extend as you discover new Plaud output formats.
    #[serde(default = "default_plaud_qualifiers")]
    pub plaud_qualifiers: Vec<String>,
}

fn default_raw_dir() -> String { "notes".into() }
fn default_plaud_dir() -> String { "plaud".into() }
fn default_artifacts_dir() -> String { "artifacts".into() }
fn default_sources_dir() -> String { "sources".into() }
fn default_synthetic_dir() -> String { "_synthetic".into() }
fn default_generated_dir() -> String { "_generated".into() }
fn default_csnotes_dir() -> String { "_csnotes".into() }
fn default_filename_format() -> String { "{course}-{mm}-{dd}".into() }
fn default_backend() -> AiBackend { AiBackend::Claude }
fn default_skill_variant() -> SkillVariant { SkillVariant::Claude }
fn default_snapshot_mode() -> SnapshotMode { SnapshotMode::PreMerge }
fn default_archive_threshold_weeks() -> u32 { 8 }
fn default_plaud_qualifiers() -> Vec<String> {
    vec!["transcript".into(), "summary".into(), "mindmap".into()]
}

impl VaultConfig {
    pub fn load(vault_root: &Path) -> Result<Self> {
        let path = vault_root.join(".csnotes");
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: VaultConfig = toml::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self, vault_root: &Path) -> Result<()> {
        let path = vault_root.join(".csnotes");
        let content = toml::to_string_pretty(self)
            .context("serializing .csnotes")?;
        std::fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Validate a new config (called by `init` and `config --set`).
    pub fn validate(&self) -> Result<()> {
        FilenameFormat::parse(&self.filename_format)?;
        for course in &self.active_courses {
            ensure_no_spaces(course, "active_courses entry")?;
        }
        Ok(())
    }

    /// Does this qualifier string (after the base stem) count as a Plaud export?
    /// Always-recognized: single lowercase ASCII letter.
    /// Also recognized: anything in `plaud_qualifiers`.
    pub fn is_plaud_qualifier(&self, qualifier: &str) -> bool {
        if qualifier.len() == 1
            && qualifier.chars().next().map_or(false, |c| c.is_ascii_lowercase())
        {
            return true;
        }
        self.plaud_qualifiers.iter().any(|q| q == qualifier)
    }

    /// Instruction file path in the vault (to be copied into the workspace).
    pub fn instruction_source_path(&self, vault_root: &Path) -> PathBuf {
        let filename = match self.skill_variant {
            SkillVariant::Claude => "claude.md",
            SkillVariant::Gemini => "gemini.md",
        };
        vault_root.join(&self.csnotes_dir).join("instructions").join(filename)
    }

    pub fn synthesis_md_path(&self, vault_root: &Path) -> PathBuf {
        vault_root
            .join(&self.csnotes_dir)
            .join("instructions")
            .join("synthesis.md")
    }

    pub fn report_schema_path(&self, vault_root: &Path) -> PathBuf {
        vault_root
            .join(&self.csnotes_dir)
            .join("instructions")
            .join("report_schema.md")
    }
}

// ── Vault root discovery ──────────────────────────────────────────────────────

/// Walk upward from `start` until we find a directory containing `.csnotes`.
/// Returns the vault root (the directory containing `.csnotes`).
pub fn find_vault_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".csnotes").exists() {
            return Ok(current);
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => {
                return Err(
                    CsnotesError::VaultNotFound(start.to_path_buf()).into()
                )
            }
        }
    }
}

// ── FilenameFormat ────────────────────────────────────────────────────────────

/// Tokens recognised in a filename format string.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Course,
    Year,
    Month,
    Day,
    Literal(String),
}

impl Token {
    fn is_variable_length(&self) -> bool {
        matches!(self, Token::Course | Token::Year)
    }
}

/// A validated, compiled filename format.
///
/// ```
/// # use csnotes::config::FilenameFormat;
/// let fmt = FilenameFormat::parse("{course}-{mm}-{dd}").unwrap();
/// # let date = chrono::NaiveDate::from_ymd_opt(2026, 7, 28).unwrap();
/// # assert_eq!(fmt.render("CS501", date), "CS501-07-28");
/// ```
pub struct FilenameFormat {
    raw: String,
    tokens: Vec<Token>,
    /// Regex for parsing a filename stem back to components.
    parser: OnceLock<Regex>,
}

impl std::fmt::Debug for FilenameFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilenameFormat").field("raw", &self.raw).finish()
    }
}

impl Clone for FilenameFormat {
    fn clone(&self) -> Self {
        FilenameFormat {
            raw: self.raw.clone(),
            tokens: self.tokens.clone(),
            parser: OnceLock::new(),
        }
    }
}

impl FilenameFormat {
    /// Parse and validate a format string.
    pub fn parse(s: &str) -> Result<Self> {
        if s.contains(' ') {
            bail!(CsnotesError::InvalidFilenameFormat {
                format: s.to_string(),
                reason: "spaces are not allowed".to_string(),
            });
        }

        let tokens = tokenise(s)?;

        // Must contain {course}
        if !tokens.iter().any(|t| *t == Token::Course) {
            bail!(CsnotesError::InvalidFilenameFormat {
                format: s.to_string(),
                reason: "must contain {course}".to_string(),
            });
        }

        // Must contain at least one date token
        let has_date = tokens.iter().any(|t| {
            matches!(t, Token::Year | Token::Month | Token::Day)
        });
        if !has_date {
            bail!(CsnotesError::InvalidFilenameFormat {
                format: s.to_string(),
                reason: "must contain at least one date token ({yyyy}, {mm}, or {dd})".to_string(),
            });
        }

        // Two adjacent variable-length tokens are ambiguous
        for window in tokens.windows(2) {
            if window[0].is_variable_length() && window[1].is_variable_length() {
                bail!(CsnotesError::InvalidFilenameFormat {
                    format: s.to_string(),
                    reason: format!(
                        "two adjacent variable-length tokens would be ambiguous to parse"
                    ),
                });
            }
        }

        Ok(FilenameFormat { raw: s.to_string(), tokens, parser: OnceLock::new() })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Render a filename stem (no extension) from a course name and date.
    pub fn render(&self, course: &str, date: NaiveDate) -> String {
        let mut out = String::new();
        for token in &self.tokens {
            match token {
                Token::Course => out.push_str(course),
                Token::Year => out.push_str(&format!("{:04}", date.format("%Y"))),
                Token::Month => out.push_str(&format!("{:02}", date.format("%m"))),
                Token::Day => out.push_str(&format!("{:02}", date.format("%d"))),
                Token::Literal(s) => out.push_str(s),
            }
        }
        out
    }

    /// Try to parse a filename stem into (course, year?, month?, day?).
    ///
    /// Returns `None` if the stem doesn't match this format.
    pub fn try_parse_stem(&self, stem: &str) -> Option<ParsedStem> {
        let re = self.parser.get_or_init(|| self.build_regex());
        let caps = re.captures(stem)?;

        Some(ParsedStem {
            course: caps.name("course").map(|m| m.as_str().to_string())?,
            year: caps.name("yyyy").and_then(|m| m.as_str().parse().ok()),
            month: caps.name("mm").and_then(|m| m.as_str().parse().ok()),
            day: caps.name("dd").and_then(|m| m.as_str().parse().ok()),
        })
    }

    fn build_regex(&self) -> Regex {
        let mut pattern = "^".to_string();
        for token in &self.tokens {
            match token {
                Token::Course => pattern.push_str(r"(?P<course>[A-Za-z][A-Za-z0-9]*)"),
                Token::Year => pattern.push_str(r"(?P<yyyy>\d{4})"),
                Token::Month => pattern.push_str(r"(?P<mm>\d{2})"),
                Token::Day => pattern.push_str(r"(?P<dd>\d{2})"),
                Token::Literal(s) => pattern.push_str(&regex::escape(s)),
            }
        }
        pattern.push('$');
        Regex::new(&pattern).expect("FilenameFormat::build_regex produced invalid regex")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStem {
    pub course: String,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

/// Convert a format string like `{course}-{mm}-{dd}` into a `Vec<Token>`.
fn tokenise(s: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = s.chars().peekable();
    let mut literal_buf = String::new();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Flush any accumulated literal
            if !literal_buf.is_empty() {
                tokens.push(Token::Literal(literal_buf.clone()));
                literal_buf.clear();
            }
            // Read until '}'
            let mut name = String::new();
            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(c) => name.push(c),
                    None => bail!(CsnotesError::InvalidFilenameFormat {
                        format: s.to_string(),
                        reason: "unclosed '{'".to_string(),
                    }),
                }
            }
            match name.as_str() {
                "course" => tokens.push(Token::Course),
                "yyyy" => tokens.push(Token::Year),
                "mm" => tokens.push(Token::Month),
                "dd" => tokens.push(Token::Day),
                other => bail!(CsnotesError::InvalidFilenameFormat {
                    format: s.to_string(),
                    reason: format!("unknown token '{{{}}}'", other),
                }),
            }
        } else {
            literal_buf.push(ch);
        }
    }

    if !literal_buf.is_empty() {
        tokens.push(Token::Literal(literal_buf));
    }

    Ok(tokens)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn ensure_no_spaces(value: &str, context: &str) -> Result<()> {
    if value.contains(' ') {
        bail!("{context} '{value}' must not contain spaces");
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn render_default_format() {
        let fmt = FilenameFormat::parse("{course}-{mm}-{dd}").unwrap();
        assert_eq!(fmt.render("CS501", date(2026, 7, 28)), "CS501-07-28");
    }

    #[test]
    fn render_with_year() {
        let fmt = FilenameFormat::parse("{course}-{yyyy}-{mm}-{dd}").unwrap();
        assert_eq!(fmt.render("CS601", date(2027, 3, 12)), "CS601-2027-03-12");
    }

    #[test]
    fn parse_stem_default() {
        let fmt = FilenameFormat::parse("{course}-{mm}-{dd}").unwrap();
        let parsed = fmt.try_parse_stem("CS501-07-28").unwrap();
        assert_eq!(parsed.course, "CS501");
        assert_eq!(parsed.month, Some(7));
        assert_eq!(parsed.day, Some(28));
        assert_eq!(parsed.year, None);
    }

    #[test]
    fn parse_stem_with_year() {
        let fmt = FilenameFormat::parse("{course}-{yyyy}-{mm}-{dd}").unwrap();
        let parsed = fmt.try_parse_stem("CS601-2027-03-12").unwrap();
        assert_eq!(parsed.course, "CS601");
        assert_eq!(parsed.year, Some(2027));
        assert_eq!(parsed.month, Some(3));
        assert_eq!(parsed.day, Some(12));
    }

    #[test]
    fn parse_stem_no_match() {
        let fmt = FilenameFormat::parse("{course}-{mm}-{dd}").unwrap();
        assert!(fmt.try_parse_stem("random-file").is_none());
    }

    #[test]
    fn rejects_missing_course() {
        assert!(FilenameFormat::parse("{mm}-{dd}").is_err());
    }

    #[test]
    fn rejects_missing_date() {
        assert!(FilenameFormat::parse("{course}").is_err());
    }

    #[test]
    fn rejects_spaces() {
        assert!(FilenameFormat::parse("{course} {mm}-{dd}").is_err());
    }

    #[test]
    fn rejects_adjacent_variable_tokens() {
        // {course} and {yyyy} are both variable-length — ambiguous
        assert!(FilenameFormat::parse("{course}{yyyy}-{mm}-{dd}").is_err());
    }

    #[test]
    fn plaud_qualifier_recognition() {
        let cfg = VaultConfig {
            raw_dir: default_raw_dir(),
            plaud_dir: default_plaud_dir(),
            artifacts_dir: default_artifacts_dir(),
            sources_dir: default_sources_dir(),
            synthetic_dir: default_synthetic_dir(),
            generated_dir: default_generated_dir(),
            csnotes_dir: default_csnotes_dir(),
            filename_format: default_filename_format(),
            active_courses: vec![],
            default_backend: default_backend(),
            skill_variant: default_skill_variant(),
            snapshot_mode: default_snapshot_mode(),
            archive_threshold_weeks: default_archive_threshold_weeks(),
            plaud_qualifiers: default_plaud_qualifiers(),
        };
        assert!(cfg.is_plaud_qualifier("transcript"));
        assert!(cfg.is_plaud_qualifier("summary"));
        assert!(cfg.is_plaud_qualifier("a"));
        assert!(cfg.is_plaud_qualifier("b"));
        assert!(!cfg.is_plaud_qualifier("notes"));
        assert!(!cfg.is_plaud_qualifier("v2"));
        assert!(!cfg.is_plaud_qualifier("ab")); // two letters, not single
    }
}
