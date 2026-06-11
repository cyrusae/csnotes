# Antigravity CLI Code Review Feedback

This document contains detailed review feedback for the `csnotes` repository. It covers logic and correctness issues, performance bottlenecks, configurations, test coverage, and code quality improvements.

---

## 1. Logic and Correctness Issues

### A. Broken Wikilink Parsing for Adjacent Links

* **Target File:** [src/obsidian.rs](file:///Users/watcher/GitHere/csnotes/src/obsidian.rs#L35-L37) (`WIKILINK_RE`) and [extract_wikilinks](file:///Users/watcher/GitHere/csnotes/src/obsidian.rs#L68-L87)
* **High-Level Explanation:** 
  The regular expression [WIKILINK_RE](file:///Users/watcher/GitHere/csnotes/src/obsidian.rs#L35) matches `[[wikilinks]]` while avoiding `![[embeds]]` by using the pattern `(?:^|[^!])\[\[`. Because the preceding character is consumed as part of the match, adjacent wikilinks (e.g. `[[link1]][[link2]]` with no space between them) fail to match the second link. The `]` character closing the first link is consumed, meaning the parser cannot see a valid preceding character for the second link. This leads to incorrect link extractions and false positives in broken link audits.
* **Targeted Notes for AI Resolution:**
  Modify [WIKILINK_RE](file:///Users/watcher/GitHere/csnotes/src/obsidian.rs#L35) to match the optional `!` using a capturing group rather than a consuming lookbehind-equivalent, e.g. `(!?)\[\[([^\]|]+)(?:\|[^\]]*)?\]\]`. Update [extract_wikilinks](file:///Users/watcher/GitHere/csnotes/src/obsidian.rs#L68-L87) to filter out matches where the first group contains `!` and extract the inner link from group 2.

### B. Enforced Case Sensitivity for Case-Insensitive Obsidian Links

* **Target Files:**
  * [src/audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L517-L533) ([note_exists_in_tree](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L517))
  * [src/workspace.rs](file:///Users/watcher/GitHere/csnotes/src/workspace.rs#L627-L630) ([rebuild_cross_embedded_in](file:///Users/watcher/GitHere/csnotes/src/workspace.rs#L579))
* **High-Level Explanation:**
  Obsidian treats wikilinks and page targets case-insensitively. However, `csnotes` checks link existence and builds reverse embed maps using strict case-sensitive comparisons (e.g., `s == note_name`). If a user writes `[[sorting]]` but the file on disk is `Sorting.md`, `csnotes audit` throws a hard violation and blocks commits.
* **Targeted Notes for AI Resolution:**
  Update [note_exists_in_tree](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L517) to compare stems case-insensitively using `eq_ignore_ascii_case` or by converting both parts to lowercase. Similarly, update [rebuild_cross_embedded_in](file:///Users/watcher/GitHere/csnotes/src/workspace.rs#L579) to use a case-insensitive map key or lookup.

### C. Configured `synthetic_dir` is Ignored/Hardcoded

* **Target Files:**
  * [src/flags.rs](file:///Users/watcher/GitHere/csnotes/src/flags.rs#L102-L107) ([open_for_topic](file:///Users/watcher/GitHere/csnotes/src/flags.rs#L102))
  * [src/commands/process.rs](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L476-L496) ([topic_from_path](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L476))
* **High-Level Explanation:**
  The directory where synthetic notes reside is configurable via `csnotes.toml` (`synthetic_dir`). However, several key helper methods hardcode the default `"_synthetic"` path prefix. If a user sets `synthetic_dir` to something else, flag assignment and topic resolution from paths will fail.
* **Targeted Notes for AI Resolution:**
  * In [open_for_topic](file:///Users/watcher/GitHere/csnotes/src/flags.rs#L102), accept the `synthetic_dir` as a parameter or extract the topic prefix dynamically without hardcoding `_synthetic`.
  * Update [topic_from_path](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L476) to accept or dynamically resolve the configured `synthetic_dir` prefix instead of stripping components starting with `_`.

### D. Incomplete/Broken Date Parsing for CLI `--session` Parameter

* **Target Files:**
  * [src/commands/process.rs](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L373-L419) ([resolve_session_id](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L373))
  * [src/commands/extract.rs](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs#L29-L33)
* **High-Level Explanation:**
  The command documentation for `csnotes process --session` claims that users can specify partial dates like `07-28`. However, [resolve_session_id](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L373) compares the input date against `e.date.to_string()`, which produces `YYYY-MM-DD` (e.g. `2026-07-28`), making direct string equality fail. Additionally, `csnotes extract` does not support resolving partial dates/sessions at all, unlike `csnotes diff` which resolves them via `ends_with`.
* **Targeted Notes for AI Resolution:**
  * Change [resolve_session_id](file:///Users/watcher/GitHere/csnotes/src/commands/process.rs#L393) to check if the session date string ends with or contains the user's input, or parse the user's input as a partial date.
  * Align the session resolution logic in [src/commands/extract.rs](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs) with the helper function used in `diff` and `process`.

### E. Fragile CRLF Line Ending Handling (Direct `read_to_string` calls)

* **Target Files:**
  * [src/audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L288) & [src/audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L485) & [src/audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L641)
  * [src/commands/extract.rs](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs#L74)
* **High-Level Explanation:**
  The codebase has a custom [read_note](file:///Users/watcher/GitHere/csnotes/src/frontmatter.rs#L188) function designed to normalize Windows CRLF line endings to LF. However, several critical paths in the audit suite and extraction command bypass this helper and call `std::fs::read_to_string` directly. If any notes are saved with CRLF line endings, the frontmatter parser will fail to find the `---` fence splits, raising false parser/schema errors.
* **Targeted Notes for AI Resolution:**
  Replace all instances of `std::fs::read_to_string` in [src/audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs) and [src/commands/extract.rs](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs) that read markdown notes with `crate::frontmatter::read_note`.

### F. Course Name Constraints on Hyphens

* **Target File:** [src/config.rs](file:///Users/watcher/GitHere/csnotes/src/config.rs#L349-L362) ([FilenameFormat::build_regex](file:///Users/watcher/GitHere/csnotes/src/config.rs#L349))
* **High-Level Explanation:**
  The course name pattern compiler uses `(?P<course>[A-Za-z][A-Za-z0-9]*)`. If a user names a course `CS-101` or `CS_101` (which are valid according to configuration validators that only block spaces), the parsed regex fails or matches dates incorrectly because of the hyphens.
* **Targeted Notes for AI Resolution:**
  Update the course capture group expression in [FilenameFormat::build_regex](file:///Users/watcher/GitHere/csnotes/src/config.rs#L349) to allow hyphens and underscores, e.g., `(?P<course>[A-Za-z][A-Za-z0-9_-]*)`.

---

## 2. Performance and Scaling Issues

### A. Quadratic Directory Traversal in Wikilink Audit (O(N^2 * L))

* **Target File:** [src/audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L475-L515) ([check_links_resolve](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L475))
* **High-Level Explanation:**
  For every markdown file under `_synthetic/`, the validator extracts all wikilinks and embeds. For *each* link or embed, it calls [note_exists_in_tree](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L517), which performs a fresh recursive traversal of the entire synthetic directory tree using `WalkDir`. For large vaults, this produces a quadratic complexity of file-system operations (N files * L links * N files directory walk) that will cause severe slowdowns.
* **Targeted Notes for AI Resolution:**
  Pre-collect all valid note stems in the search root into a `HashSet<String>` in a single initial pass. Change [check_links_resolve](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L475) to verify link targets against this in-memory set in O(1) time.

---

## 3. General Quality Improvements & Architecture

### A. Walkdir Depth Constraints Silence Nested Files

* **Target File:** [src/commands/reconcile.rs](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs) ([scan_sources_dir](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs#L550), [scan_artifacts_dir](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs#L439))
* **High-Level Explanation:**
  `scan_sources_dir` uses `max_depth(2)` and `scan_artifacts_dir` uses `max_depth(1)`. If a user organizes textbook chapters or lab scripts in deeper subfolders (e.g. `sources/textbooks/SICP/ch1.md` or `artifacts/labs/week1/CPSC5001.py`), these files will be silently ignored during reconciliation without any warnings.
* **Targeted Notes for AI Resolution:**
  Remove or increase the `max_depth` restrictions in [src/commands/reconcile.rs](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs) to allow recursive folder scanning.

### B. Inaccurate Deadline Keyword Extraction

* **Target File:** [src/commands/extract.rs](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs#L187-L212) ([is_deadline](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs#L187))
* **High-Level Explanation:**
  The [is_deadline](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs#L187) check searches for `" by "` with spaces on both sides. This causes deadlines starting with "By" (e.g., `By Monday, submit project`) or formatted without extra whitespace to be ignored. Additionally, deadlines like `due Wednesday` or `due Oct 5` (which don't contain `due:` or `due date`) are completely missed.
* **Targeted Notes for AI Resolution:**
  Refactor [is_deadline](file:///Users/watcher/GitHere/csnotes/src/commands/extract.rs#L187) to match keywords case-insensitively and use regex or word boundaries (e.g., `\bby\b` and `\bdue\b`) to prevent whitespace sensitivity.

> Commentary: Would need to exclude "due to" in results.

---

## 4. Test Coverage Analysis

### Current Status

* Unit tests exist in `src/obsidian.rs`, `src/flags.rs`, and `src/commands/extract.rs`.
* Integration and property-based tests exist under `tests/lifecycle_tests.rs` and `tests/property_tests.rs`.
* **Overall Test Coverage:** Moderate. Test suites cover happy-path CLI execution, manifest idempotency, and basic workspace snapshots.

### Test Coverage Gaps & Recommendations

1. **No test coverage for `csnotes diff`:** Add integration tests checking diff outputs for session modifications and flags.
2. **Missing CLI command tests:** Commands like `config`, `status`, and `audit --fix` lack test cases. Add test suites executing these commands on a temporary vault.
3. **No tests for edge cases in regex parsers:** Add edge-case tests under `tests/` specifically focusing on adjacent wikilinks, case-insensitive links, and Windows CRLF line endings.
