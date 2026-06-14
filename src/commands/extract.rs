/// `csnotes extract` — pull action items, deadlines, and questions from raw
/// notes and write them to `_generated/extracts/<session-id>.md` (or stdout).
///
/// Recognises:
///   actions   — `- [ ] ...`, `TODO:`, `ACTION:`
///   deadlines — lines containing `due`, `deadline`, `overdue`, `submit`,
///               `turn in`, `hand in`, `by <date/day/ordinal>`, or
///               `by tomorrow`/`by tonight`
///   questions — lines ending with `?`, starting with `Q:` / `??`
use std::fs;
use std::path::{Path, PathBuf};

use std::sync::LazyLock;

use anyhow::{bail, Result};
use regex::Regex;

use crate::config::{find_vault_root, VaultConfig};
use crate::manifest::{Manifest, SessionStatus};

pub struct ExtractArgs {
    pub session: Option<String>,
    pub kind: Option<String>,
    pub stdout: bool,
}

pub fn run(args: ExtractArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let manifest = Manifest::load(&vault_root)?;

    // ── Resolve session(s) ────────────────────────────────────────────────────
    let session_ids: Vec<String> = if let Some(ref id) = args.session {
        if !manifest.sessions.contains_key(id.as_str()) {
            bail!("Session '{}' not found in manifest.", id);
        }
        vec![id.clone()]
    } else {
        // Default: all unprocessed sessions (most likely to have fresh actions)
        let mut ids: Vec<String> = manifest
            .sessions
            .iter()
            .filter(|(_, e)| e.status == SessionStatus::Unprocessed)
            .map(|(id, _)| id.clone())
            .collect();
        if ids.is_empty() {
            // Fall back to the most recently processed session
            ids = manifest
                .sessions
                .iter()
                .filter_map(|(id, e)| e.processed_at.map(|t| (id.clone(), t)))
                .max_by_key(|(_, t)| *t)
                .map(|(id, _)| vec![id])
                .unwrap_or_default();
        }
        ids
    };

    if session_ids.is_empty() {
        println!("No sessions to extract from.");
        return Ok(());
    }

    let filter = args.kind.as_deref();

    for session_id in &session_ids {
        let entry = match manifest.sessions.get(session_id.as_str()) {
            Some(e) => e,
            None => continue,
        };

        let raw_path = vault_root.join(&entry.raw_note);
        if !raw_path.exists() {
            eprintln!("  warn: raw note not found: {}", raw_path.display());
            continue;
        }

        let content = crate::frontmatter::read_note(&raw_path)?;
        let extracted = extract_from(&content, filter);

        if extracted.is_empty() {
            if args.stdout {
                println!("# {} — nothing extracted", session_id);
            }
            continue;
        }

        let output = render_extract(session_id, &entry.date.to_string(), &extracted);

        if args.stdout {
            print!("{}", output);
        } else {
            let out_dir = vault_root.join(&config.generated_dir).join("extracts");
            fs::create_dir_all(&out_dir)?;
            let out_path = out_dir.join(format!("{}.md", session_id));
            fs::write(&out_path, &output)?;
            println!("Extracted to {}", relative_path(&out_path, &vault_root));
        }
    }

    Ok(())
}

// ── Extraction ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExtractKind {
    Action,
    Deadline,
    Question,
}

impl ExtractKind {
    fn label(&self) -> &'static str {
        match self {
            ExtractKind::Action => "action",
            ExtractKind::Deadline => "deadline",
            ExtractKind::Question => "question",
        }
    }
}

struct ExtractedItem {
    kind: ExtractKind,
    text: String,
    line: usize,
}

fn extract_from(content: &str, filter: Option<&str>) -> Vec<ExtractedItem> {
    let want_actions = filter.is_none_or(|f| f == "actions" || f == "action");
    let want_deadlines = filter.is_none_or(|f| f == "deadlines" || f == "deadline");
    let want_questions = filter.is_none_or(|f| f == "questions" || f == "question");

    let mut items = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if want_actions && is_action(trimmed) {
            items.push(ExtractedItem {
                kind: ExtractKind::Action,
                text: clean_action(trimmed),
                line: i + 1,
            });
        } else if want_deadlines && is_deadline(trimmed) {
            items.push(ExtractedItem {
                kind: ExtractKind::Deadline,
                text: trimmed.to_string(),
                line: i + 1,
            });
        } else if want_questions && is_question(trimmed) {
            items.push(ExtractedItem {
                kind: ExtractKind::Question,
                text: clean_question(trimmed),
                line: i + 1,
            });
        }
    }

    items
}

fn is_action(line: &str) -> bool {
    line.starts_with("- [ ]") || line.starts_with("* [ ]") || {
        let up = line.to_ascii_uppercase();
        up.starts_with("TODO:") || up.starts_with("ACTION:") || up.starts_with("- TODO")
    }
}

fn clean_action(line: &str) -> String {
    let s = line
        .trim_start_matches("- [ ]")
        .trim_start_matches("* [ ]")
        .trim();
    // Strip TODO:/ACTION: prefix case-insensitively
    let up = s.to_ascii_uppercase();
    if up.starts_with("TODO:") {
        s[5..].trim().to_string()
    } else if up.starts_with("ACTION:") {
        s[7..].trim().to_string()
    } else {
        s.to_string()
    }
}

/// Matches lines that look like deadlines.
///
/// Uses word boundaries (`\b`) so that:
/// - `\bdue\b`        catches "due:", "due date", "Due Friday" but NOT "residue"
/// - `\bdeadline`     catches "deadline", "deadlines"
/// - `\boverdue\b`    catches "overdue assignment"
/// - `\bsubmit`       catches "submit", "submitted", "submission"
/// - `\bturn\s+in\b`  catches "turn in the assignment"
/// - `\bhand\s+in\b`  catches "hand in the project"
/// - `\bby\s+...`     catches "by Monday", "by January", "by the 15th",
///   "by tomorrow", "by tonight" but NOT "maybe thursday"
///   or "standby friday" (word boundary before "by")
static DEADLINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \bdeadline
        | \bdue\b
        | \boverdue\b
        | \bsubmit
        | \bturn\s+in\b
        | \bhand\s+in\b
        | \bby\s+(?:the\s+)?
          (?:
              mon(?:day)?   | tue(?:sday)?  | wed(?:nesday)? | thu(?:rsday)?
            | fri(?:day)?   | sat(?:urday)? | sun(?:day)?
            | jan(?:uary)?  | feb(?:ruary)? | mar(?:ch)?     | apr(?:il)?
            | may           | jun(?:e)?     | jul(?:y)?      | aug(?:ust)?
            | sep(?:tember)?| oct(?:ober)?  | nov(?:ember)?  | dec(?:ember)?
            | tomorrow | tonight
            | \d{1,2}(?:st|nd|rd|th)
          )\b
        ",
    )
    .unwrap()
});

fn is_deadline(line: &str) -> bool {
    DEADLINE_RE.is_match(line)
}

fn is_question(line: &str) -> bool {
    let trimmed = line.trim_end_matches(|c: char| c.is_whitespace() || c == '*' || c == '_');
    let up = line.trim().to_ascii_uppercase();
    trimmed.ends_with('?')
        || up.starts_with("Q:")
        || up.starts_with("??")
        || up.starts_with("QUESTION:")
}

fn clean_question(line: &str) -> String {
    let s = line.trim();
    let up = s.to_ascii_uppercase();
    if up.starts_with("Q:") {
        s[2..].trim().to_string()
    } else if up.starts_with("QUESTION:") {
        s[9..].trim().to_string()
    } else if up.starts_with("??") {
        s[2..].trim().to_string()
    } else {
        s.to_string()
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_extract(session_id: &str, date: &str, items: &[ExtractedItem]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Extracts — {} ({})\n\n", session_id, date));

    for kind in [
        ExtractKind::Action,
        ExtractKind::Deadline,
        ExtractKind::Question,
    ] {
        let section: Vec<_> = items.iter().filter(|i| i.kind == kind).collect();
        if section.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}s\n\n", capitalise(kind.label())));
        for item in section {
            out.push_str(&format!("- {} _(line {})_\n", item.text, item.line));
        }
        out.push('\n');
    }

    out
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

fn relative_path(path: &Path, base: &PathBuf) -> String {
    path.strip_prefix(base)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_checkbox_action() {
        let items = extract_from("- [ ] email prof about project\nsome other line\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Action);
        assert_eq!(items[0].text, "email prof about project");
    }

    #[test]
    fn detects_todo_action() {
        let items = extract_from("TODO: read chapter 4\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Action);
        assert_eq!(items[0].text, "read chapter 4");
    }

    #[test]
    fn detects_question() {
        let items = extract_from("why does this work?\nQ: is this O(n log n)?\n", None);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.kind == ExtractKind::Question));
    }

    #[test]
    fn detects_deadline() {
        let items = extract_from("assignment due: friday\nsubmit by thursday\n", None);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.kind == ExtractKind::Deadline));
    }

    #[test]
    fn filter_by_kind() {
        let content = "- [ ] do thing\nwhy?\ndue: monday\n";
        let actions = extract_from(content, Some("actions"));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, ExtractKind::Action);

        let questions = extract_from(content, Some("questions"));
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].kind, ExtractKind::Question);
    }

    #[test]
    fn deadline_bare_due_without_colon() {
        // "due" without a colon was previously missed
        let items = extract_from("assignment due friday\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_uppercase_keywords() {
        let items = extract_from("DEADLINE: submit project\nDUE: Oct 15\n", None);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.kind == ExtractKind::Deadline));
    }

    #[test]
    fn deadline_submission_variant() {
        let items = extract_from("homework submission by friday\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn no_false_positive_residue() {
        // "residue" contains "due" but should not trigger — word boundary fix
        let items = extract_from("the residue was measured\n", None);
        assert!(items.is_empty(), "residue should not match due");
    }

    #[test]
    fn no_false_positive_maybe() {
        // "maybe thursday" — "maybe" contains "by" but no word boundary before it
        let items = extract_from("maybe thursday works for everyone\n", None);
        assert!(items.is_empty(), "maybe should not match by <day>");
    }

    #[test]
    fn no_false_positive_standby() {
        let items = extract_from("put it on standby friday\n", None);
        assert!(items.is_empty(), "standby should not match by <day>");
    }

    #[test]
    fn no_false_positives_on_normal_prose() {
        let content = "Today we covered sorting algorithms.\nMerge sort is O(n log n).\n";
        let items = extract_from(content, None);
        assert!(items.is_empty());
    }

    // ── New keyword coverage ──────────────────────────────────────────────────

    #[test]
    fn deadline_turn_in() {
        let items = extract_from("turn in the assignment by friday\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_hand_in() {
        let items = extract_from("hand in the problem set\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_overdue() {
        let items = extract_from("this assignment is overdue\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_by_full_month_name() {
        let items = extract_from("project due by January 15\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_by_ordinal() {
        let items = extract_from("submit by the 15th\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_by_ordinal_no_the() {
        let items = extract_from("due by 3rd\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_by_tomorrow() {
        let items = extract_from("submit by tomorrow\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_by_tonight() {
        let items = extract_from("finish by tonight\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn deadline_by_full_day_name() {
        // Full day names like "by Monday" must match (mon(?:day)? covers both)
        let items = extract_from("finish by Monday\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn no_false_positive_tomorrow_without_by() {
        // "tomorrow" alone (no "by") should not trigger
        let items = extract_from("we will cover this tomorrow\n", None);
        assert!(
            items.is_empty(),
            "bare 'tomorrow' without 'by' should not match"
        );
    }

    #[test]
    fn deadline_all_full_month_names_by() {
        let months = [
            "january",
            "february",
            "march",
            "april",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        for month in months {
            let line = format!("submit by {}\n", month);
            let items = extract_from(&line, None);
            assert_eq!(items.len(), 1, "expected match for 'by {}'", month);
            assert_eq!(items[0].kind, ExtractKind::Deadline);
        }
    }

    // ── extract_from: filter + line numbers ───────────────────────────────────

    #[test]
    fn filter_by_deadlines() {
        // Catches `replace || with && in extract_from` line 131: with &&,
        // `f == "deadlines" && f == "deadline"` is always false → filter skips deadlines.
        let items = extract_from("due by monday\n", Some("deadlines"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Deadline);
    }

    #[test]
    fn filter_by_deadline_singular() {
        let items = extract_from("assignment due friday\n", Some("deadline"));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn action_line_number_is_one_indexed() {
        // Catches `replace + with * in extract_from` on `i + 1`: first line (i=0)
        // would give line=0 with *, but correct is line=1.
        let items = extract_from("- [ ] first line task\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].line, 1, "first-line action must report line 1");
    }

    #[test]
    fn deadline_line_number_is_one_indexed() {
        let items = extract_from("assignment due friday\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].line, 1);
    }

    #[test]
    fn question_line_number_is_one_indexed() {
        let items = extract_from("what is recursion?\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].line, 1);
    }

    // ── is_action: star checkbox ──────────────────────────────────────────────

    #[test]
    fn is_action_detects_star_checkbox() {
        // Catches `replace || with && in is_action` at column 62: with &&, the star
        // checkbox requires also matching the TODO block, which is impossible.
        let items = extract_from("* [ ] read the paper\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Action);
        assert_eq!(items[0].text, "read the paper");
    }

    // ── is_question: individual || branches ──────────────────────────────────

    #[test]
    fn is_question_q_colon_without_trailing_mark() {
        // Catches `replace || with && in is_question` at line 234: Q: prefix alone
        // would fail if `ends_with('?') && starts_with("Q:")` is required.
        let items = extract_from("Q: what does polymorphism mean\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Question);
    }

    #[test]
    fn is_question_double_question_prefix() {
        // Catches `replace || with && in is_question` at line 235.
        let items = extract_from("?? is this on the exam\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Question);
    }

    #[test]
    fn is_question_question_colon_prefix() {
        let items = extract_from("QUESTION: explain big-O\n", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ExtractKind::Question);
    }

    #[test]
    fn is_question_trims_trailing_space() {
        // Catches one of the `|| with &&` in trim_end_matches closure (line 230).
        assert!(is_question("why does this work? "));
    }

    #[test]
    fn is_question_trims_trailing_asterisk() {
        // Catches the other `|| with &&` in trim_end_matches closure (line 230).
        assert!(is_question("why does this work?*"));
    }

    // ── clean_question ────────────────────────────────────────────────────────

    #[test]
    fn clean_question_strips_q_colon() {
        assert_eq!(clean_question("Q: what is recursion"), "what is recursion");
    }

    #[test]
    fn clean_question_strips_question_colon() {
        assert_eq!(clean_question("QUESTION: explain big-O"), "explain big-O");
    }

    #[test]
    fn clean_question_strips_double_question() {
        assert_eq!(clean_question("?? is this tested"), "is this tested");
    }

    #[test]
    fn clean_question_preserves_bare_question() {
        assert_eq!(clean_question("why does this work?"), "why does this work?");
    }

    // ── render_extract ────────────────────────────────────────────────────────

    #[test]
    fn render_extract_groups_by_kind() {
        // Catches `replace == with != in render_extract`: with !=, each section's
        // filter would collect items of OTHER kinds, scrambling the output.
        let items = vec![
            ExtractedItem {
                kind: ExtractKind::Action,
                text: "do thing".into(),
                line: 1,
            },
            ExtractedItem {
                kind: ExtractKind::Question,
                text: "why?".into(),
                line: 2,
            },
        ];
        let out = render_extract("CS101-01-10", "2026-01-10", &items);
        assert!(
            out.contains("## Actions\n"),
            "actions section should appear"
        );
        assert!(
            out.contains("## Questions\n"),
            "questions section should appear"
        );
        assert!(
            !out.contains("## Deadlines\n"),
            "empty deadlines section should be omitted"
        );
        assert!(
            out.contains("do thing"),
            "action text should appear in actions section"
        );
    }

    #[test]
    fn render_extract_includes_header() {
        let out = render_extract("CS101-01-10", "2026-01-10", &[]);
        assert!(
            out.contains("CS101-01-10"),
            "session id should appear in header"
        );
        assert!(out.contains("2026-01-10"), "date should appear in header");
    }

    // ── capitalise ───────────────────────────────────────────────────────────

    #[test]
    fn capitalise_uppercases_first_char() {
        assert_eq!(capitalise("action"), "Action");
        assert_eq!(capitalise("question"), "Question");
        assert_eq!(capitalise(""), "");
    }

    // ── relative_path ─────────────────────────────────────────────────────────

    #[test]
    fn relative_path_strips_vault_prefix() {
        let base = std::path::PathBuf::from("/vault");
        let path = std::path::Path::new("/vault/notes/cs101.md");
        assert_eq!(relative_path(path, &base), "notes/cs101.md");
    }

    #[test]
    fn relative_path_falls_back_to_absolute_when_no_prefix() {
        let base = std::path::PathBuf::from("/other");
        let path = std::path::Path::new("/vault/notes/cs101.md");
        assert_eq!(relative_path(path, &base), "/vault/notes/cs101.md");
    }

    // ── ExtractKind::label ────────────────────────────────────────────────────

    #[test]
    fn extract_kind_label() {
        assert_eq!(ExtractKind::Action.label(), "action");
        assert_eq!(ExtractKind::Deadline.label(), "deadline");
        assert_eq!(ExtractKind::Question.label(), "question");
    }
}
