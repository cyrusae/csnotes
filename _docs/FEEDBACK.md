# Adversarial Code Review & Quality Recommendations: csnotes

This document presents a comprehensive review of the `csnotes` codebase, covering security/adversarial findings, structural recommendations, and opportunities to expand test coverage.

---

## Summary of Findings & Action Items

| ID | Title | Component | Severity | Status |
| :--- | :--- | :--- | :--- | :--- |
| **CSN-001** | Arbitrary File Write & Overwrite via Path Traversal in Report Operations | [audit.rs](file:///Users/watcher/GitHere/csnotes/src/audit.rs), [content.rs](file:///Users/watcher/GitHere/csnotes/src/ops/content.rs), [structural.rs](file:///Users/watcher/GitHere/csnotes/src/ops/structural.rs) | **CRITICAL** | Action Required |
| **CSN-002** | Potential AppleScript/Shell Command Injection in macOS Notifications | [reconcile.rs](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs) | **MEDIUM** | Action Required |
| **CSN-003** | Lack of Concurrent Vault/Manifest Locking (Race Conditions) | [main.rs](file:///Users/watcher/GitHere/csnotes/src/main.rs), [manifest.rs](file:///Users/watcher/GitHere/csnotes/src/manifest.rs) | **MEDIUM** | Design Nudge |
| **CSN-004** | Windows CRLF Line Endings Break Frontmatter Parsing | [frontmatter.rs](file:///Users/watcher/GitHere/csnotes/src/frontmatter.rs) | **LOW** | Design Nudge |

---

## Detailed Findings

### CSN-001: Arbitrary File Write & Overwrite via Path Traversal (CRITICAL)

> [!CAUTION]
> This is a high-severity security vulnerability that allows an attacker to write, overwrite, or corrupt arbitrary files on the local filesystem of the user running `csnotes`.

#### Root Cause
In the session teardown pipeline, the CLI reads `_session_report.json` (produced by the AI session) and executes operations declared in the report. Paths specified by the AI (e.g., `op.path` in `CreateNote` and `UpdateNote` or `from_path` in structural operations) are resolved using `workspace_root.join(&op.path)` or `synthetic_root.join(...)`. 

In Rust, `Path::join` behaves in a way that:
1. If the joined path is absolute (e.g., `/Users/watcher/.ssh/authorized_keys`), the existing prefix is completely discarded, and the path resolves directly to the absolute path.
2. If the joined path contains parent directory segments (e.g., `_synthetic/../../../../Users/watcher/.ssh/authorized_keys`), it successfully escapes the workspace directory.

No validation or canonicalization is performed to verify that the target path remains inside `workspace_root` or `_synthetic/`.

#### Vulnerable Code Example
In [src/ops/content.rs:L40](file:///Users/watcher/GitHere/csnotes/src/ops/content.rs#L40):
```rust
pub fn execute_create_note(
    op: &CreateNoteOp,
    workspace_root: &Path,
    now: DateTime<Utc>,
) -> Result<()> {
    let note_path = workspace_root.join(&op.path); // Traversal point
    ...
```

And in [src/audit.rs:L55](file:///Users/watcher/GitHere/csnotes/src/audit.rs#L55):
```rust
pub fn precondition_pass(report: &SessionReport, workspace_root: &Path) -> Result<()> {
    for op in &report.operations {
        match op {
            Op::CreateNote(op) => {
                let path = workspace_root.join(&op.path); // Traversal point
```

#### Attack Scenario
Since `csnotes` processes raw, unverified lecture notes and Plaud transcripts, a malicious actor could embed a **prompt injection payload** in a raw note or lecture slide. When the AI processes this session, it reads the raw content, gets hijacked by the prompt injection, and is instructed to write a malicious `_session_report.json` with an absolute path or a `../` sequence:

```json
{
  "op": "create_note",
  "path": "/Users/watcher/.bash_profile",
  "title": "Malicious Note",
  "topic": "system",
  ...
}
```

When the user's `csnotes` process completes the session, it will silently overwrite the user's local shell configuration or SSH keys, leading to arbitrary code execution.

#### Remediation
Implement strict path validation for all path arguments received from `_session_report.json`. Create a utility function to ensure paths are sub-directories of the intended root and contain no relative directory components:

```rust
pub fn secure_join(root: &Path, unsafe_path: &str) -> Result<PathBuf> {
    let unsafe_path = Path::new(unsafe_path);
    if unsafe_path.is_absolute() {
        bail!("Absolute paths are not allowed");
    }
    
    let resolved = root.join(unsafe_path);
    
    // Canonicalize to resolve any `..` or symlinks, then verify prefix
    let canonical_root = root.canonicalize()?;
    let canonical_resolved = resolved.canonicalize()
        .map_err(|_| anyhow::anyhow!("Path does not exist or is invalid"))?;
        
    if !canonical_resolved.starts_with(&canonical_root) {
        bail!("Directory traversal detected: path escapes root");
    }
    Ok(canonical_resolved)
}
```

---

### CSN-002: Potential AppleScript/Shell Command Injection in macOS Notifications (MEDIUM)

> [!WARNING]
> While the current notification payload only contains integer lengths of vectors, any future development extending the notification to include user-supplied text (such as file names or session dates) will lead to AppleScript injection.

#### Root Cause
In [src/commands/reconcile.rs:L770-781](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs#L770-781), notifications on macOS are executed using `osascript` with string-formatted code:

```rust
fn notify(message: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"csnotes\"",
                    message
                ),
            ])
            .status();
    }
```

If `message` contains double quotes (`"`) or backslashes (`\`), it corrupts the AppleScript string literal boundary, potentially allowing command execution or syntax errors.

#### Remediation
Avoid formatting strings into script execution commands. Instead, pass the message as an argument to the AppleScript using `argv`:

```rust
fn notify(message: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 1 of argv) with title \"csnotes\"",
                "-e",
                "end run",
                message,
            ])
            .status();
    }
}
```

---

### CSN-003: Lack of Concurrent Vault/Manifest Locking (Race Conditions) (MEDIUM)

> [!NOTE]
> Running multiple instances of `csnotes` concurrently in the same vault can lead to manifest corruption, partial session commit overwrites, or broken snapshots.

#### Root Cause
`csnotes` performs multi-step file movements, workspace merging, and manifest updates. There is no file locking (e.g. advisory lock) or verification on `csnotes.json` to prevent concurrent executions.
If a user runs `csnotes process` in two separate terminals or has an automated file watcher (e.g. `entr` or `watchexec`) running `csnotes reconcile` in the background while they process a session, they can overlap:
1. Two processes load the manifest, make independent changes, and write back, clobbering each other's changes.
2. Two processes write to the same snapshot path (`_synthetic_snapshot_<run_id>`) or try to clean up the same workspace, leading to runtime failures.

#### Remediation
Implement advisory file locking on `csnotes.json` during operations that mutate vault files (reconcile, process, recover, migrate). 
You can use the `fs2` crate to acquire an exclusive lock on the manifest file:

```rust
use fs2::FileExt;

let file = std::fs::File::open(vault_root.join("csnotes.json"))?;
file.lock_exclusive()?; // Blocks or returns error if another instance is running
```

---

### CSN-004: Windows CRLF Line Endings Break Frontmatter Parsing (LOW)

> [!TIP]
> Standardizing line ending handling improves cross-platform robustness when notes are edited on Windows or synced via Git without auto-CRLF configuration.

#### Root Cause
The frontmatter parser splits the note contents using strict `\n` line endings in [src/frontmatter.rs:L169-183](file:///Users/watcher/GitHere/csnotes/src/frontmatter.rs#L169-183):

```rust
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content.strip_prefix("---\n")?;
    // Find the closing fence (must be `---` on its own line)
    let close = content.find("\n---\n").or_else(|| {
        content.strip_suffix("\n---").map(|_| content.len() - 4)
    })?;
    ...
```

If a note contains Windows-style CRLF (`\r\n`), the prefix check `strip_prefix("---\n")` fails (since it expects `\n` but finds `\r\n`), causing the note to be treated as having no frontmatter (`NoFrontmatter` error).

#### Remediation
Update the parser to handle both LF and CRLF sequences:

```rust
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let has_crlf = content.contains("\r\n");
    let normalized = if has_crlf {
        content.replace("\r\n", "\n")
    } else {
        content.to_string()
    };
    
    let content_ref = if has_crlf { &normalized } else { content };
    let content_stripped = content_ref.strip_prefix("---\n")?;
    let close = content_stripped.find("\n---\n").or_else(|| {
        content_stripped.strip_suffix("\n---").map(|_| content_stripped.len() - 4)
    })?;
    
    // Slice appropriately based on normalized structure or return normalized slices
    ...
}
```
Alternatively, strip `\r` from the start of the file or normalize the line endings of files read from disk prior to processing.

---

## General Recommendations for Code Quality

### 1. Unified Directory Scanner
Currently, the directory scanning functions (`scan_raw_dir`, `scan_plaud_dir`, `scan_artifacts_dir`, and `scan_sources_dir`) in [reconcile.rs](file:///Users/watcher/GitHere/csnotes/src/commands/reconcile.rs) duplicate the boilerplate logic for:
- Initializing walkdir loops
- Handling file extensions
- Checking and warning/renaming space-containing names
- Prefix stripping relative to the vault root

**Recommendation**: Extract a common directory traversal wrapper or helper struct that handles filename space validation and path normalization, yielding structured file descriptors to specific scanner sub-functions.

### 2. Custom Domain Error Types
While the codebase makes extensive use of `anyhow::Result`, it maps custom errors in `src/error.rs` through `CsnotesError`. However, many error paths in the CLI dispatching and command execution still use ad-hoc `anyhow::bail!` or `anyhow::anyhow!` statements with unstructured strings.

**Recommendation**: Ensure all internal failures (such as structural operation mismatches, parser desyncs, and path validations) define distinct enum variants inside [error.rs](file:///Users/watcher/GitHere/csnotes/src/error.rs). Reserve `anyhow` strictly for the top-level application CLI layer.

### 3. Decoupling Manifest & Vault Configurations
`ManifestConfig` (in [manifest.rs](file:///Users/watcher/GitHere/csnotes/src/manifest.rs#L105)) copies fields from `VaultConfig` manually. This is prone to drifts when new configuration parameters (like backend model mappings or formatting constraints) are added.

**Recommendation**: Deriving or nesting the configuration context, or using a macro to automatically generate matching fields, would prevent synchronization issues.

---

## Opportunities for Test Coverage

### 1. Adversarial/Vulnerability Testing
To prevent regression on path traversal vulnerabilities (like **CSN-001**), add tests to the suite that feed invalid inputs (absolute paths, relative parent directories, `\0` null bytes) to:
- Operation parsing (`CreateNoteOp`, `UpdateNoteOp`, `RenameTopicOp`, etc.)
- Path resolution functions
- Workspace assembly

Assert that these attempts return validation errors and do not read or write outside of the sandbox directory.

### 2. Multi-threaded and Concurrency Mock Tests
Simulate concurrency race conditions in integration tests:
- Launch multiple `csnotes reconcile` threads concurrently in a temporary vault.
- Attempt to start `csnotes process` while another process has an active in-progress lock.
- Assert that locks prevent manifest write collisions and yield clean warning outputs.

### 3. Cross-Platform Line Ending Tests
Ensure robustness against Windows-style line endings:
- Create unit tests that run frontmatter parsing and wikilink extraction against files populated with CRLF (`\r\n`) sequences.
- Verify that `csnotes audit` and `csnotes process` handle CRLF files seamlessly.

### 4. Property Testing for Markdown Parsers
Expand the scope of the `proptest` harness in [property_tests.rs](file:///Users/watcher/GitHere/csnotes/tests/property_tests.rs). Use property testing to generate arbitrary markdown strings and feed them into:
- `extract_wikilinks`
- `extract_embeds`
- `extract_block_ids`

Verify that the regex-based parsing routines do not crash or consume excessive resources on malformed markup (e.g. extremely long, unclosed wikilinks or nested anchors).
