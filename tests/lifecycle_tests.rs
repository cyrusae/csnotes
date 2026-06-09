/// End-to-end lifecycle tests using the mock backend.
///
/// Each test spins up a temp vault, runs `csnotes process --backend mock`,
/// and asserts invariants on the resulting vault state.
///
/// The binary is compiled by cargo and referenced via `CARGO_BIN_EXE_csnotes`.
/// Fixture files live in `tests/fixtures/<name>/`.
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

// ── Test vault helpers ────────────────────────────────────────────────────────

/// Write a minimal `.csnotes` config to `vault_root`.
/// Only sets `default_backend = "mock"` — all other fields fall back to
/// compiled-in defaults.
fn write_config(vault_root: &Path) {
    fs::write(
        vault_root.join(".csnotes"),
        "default_backend = \"mock\"\n",
    )
    .unwrap();
}

/// Write `csnotes.json` with a single unprocessed session `CPSC5001-09-03`.
/// `vault_root` must be the absolute path that will be used at runtime.
fn write_manifest(vault_root: &Path) {
    let vault_str = vault_root.to_string_lossy();
    let manifest = format!(
        r#"{{
  "version": "2",
  "vault_root": {vault_str_json},
  "config": {{
    "raw_dir": "notes",
    "plaud_dir": "plaud",
    "artifacts_dir": "artifacts",
    "sources_dir": "sources",
    "synthetic_dir": "_synthetic",
    "generated_dir": "_generated",
    "filename_format": "{{course}}-{{mm}}-{{dd}}",
    "default_backend": "mock",
    "skill_variant": "claude",
    "snapshot_mode": "pre_merge"
  }},
  "sessions": {{
    "CPSC5001-09-03": {{
      "date": "2026-09-03",
      "course": "CPSC5001",
      "filename_format": "{{course}}-{{mm}}-{{dd}}",
      "raw_note": "notes/CPSC5001-09-03.md",
      "plaud_exports": [],
      "artifacts": [],
      "plaud_missing": true,
      "status": "unprocessed",
      "processed_at": null,
      "topics_updated": []
    }}
  }},
  "sources": {{}},
  "topics": {{}},
  "session_in_progress": null,
  "flags_path": "_generated/flags.json"
}}"#,
        vault_str_json = serde_json::to_string(&vault_str).unwrap(),
    );
    fs::write(vault_root.join("csnotes.json"), manifest).unwrap();
}

/// Create a minimal test vault: config, manifest, one raw note, empty
/// `_synthetic/`.
fn setup_vault() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("_synthetic")).unwrap();

    write_config(root);
    write_manifest(root);

    fs::write(
        root.join("notes/CPSC5001-09-03.md"),
        "# CPSC5001 — Sep 3 2026\n\nIntro lecture: course overview and algorithm analysis.\n",
    )
    .unwrap();

    tmp
}

/// Run `csnotes process --backend mock [--fixture NAME]` in `vault_root`.
/// Returns the exit status.
fn run_process(vault_root: &Path, fixture: &str) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_csnotes"))
        .args(["process", "--backend", "mock", "--fixture", fixture])
        .current_dir(vault_root)
        .status()
        .expect("failed to spawn csnotes binary")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Happy path: mock AI creates an index note and one atomic note.
/// After teardown the vault must contain both notes with CLI-stamped
/// frontmatter, the session must be marked processed, and no in-progress
/// record should remain.
#[test]
fn happy_path_creates_two_notes() {
    let tmp = setup_vault();
    let root = tmp.path();

    let status = run_process(root, "default");
    assert!(status.success(), "process should succeed with default fixture");

    // Both synthetic notes must have been merged into the vault.
    let index_path = root.join("_synthetic/cpsc5001/cpsc5001.md");
    let atomic_path = root.join("_synthetic/cpsc5001/algorithm-analysis.md");
    assert!(index_path.exists(), "index note should exist after merge");
    assert!(atomic_path.exists(), "atomic note should exist after merge");

    // CLI must have stamped frontmatter on both.
    let index_content = fs::read_to_string(&index_path).unwrap();
    assert!(
        index_content.starts_with("---\n"),
        "index note should have YAML frontmatter"
    );
    assert!(
        index_content.contains("kind: index"),
        "index note frontmatter should declare kind: index"
    );

    let atomic_content = fs::read_to_string(&atomic_path).unwrap();
    assert!(
        atomic_content.starts_with("---\n"),
        "atomic note should have YAML frontmatter"
    );
    assert!(
        atomic_content.contains("block_id: algo-analysis-intro"),
        "atomic note frontmatter should record block_id"
    );

    // Session must be marked processed in the manifest.
    let manifest_raw = fs::read_to_string(root.join("csnotes.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    assert_eq!(
        manifest["sessions"]["CPSC5001-09-03"]["status"],
        "processed",
        "session should be marked processed"
    );

    // No in-progress record should remain.
    assert!(
        manifest["session_in_progress"].is_null(),
        "session_in_progress should be cleared after commit"
    );

    // Workspace should have been cleaned up (no stray snapshot dirs).
    for entry in fs::read_dir(root).unwrap() {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with("_synthetic_snapshot_"),
            "snapshot dir should have been cleaned up: {}",
            name
        );
    }
}

/// `csnotes reconcile` registers a raw note as a new session and matches a
/// Plaud export to it.
#[test]
fn reconcile_registers_session_and_plaud() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Minimal vault with no sessions yet
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("plaud")).unwrap();
    fs::create_dir_all(root.join("_synthetic")).unwrap();
    write_config(root);
    // Write a manifest with no sessions
    let vault_str = root.to_string_lossy();
    let manifest = format!(
        r#"{{"version":"2","vault_root":{vs},"config":{{"raw_dir":"notes","plaud_dir":"plaud","artifacts_dir":"artifacts","sources_dir":"sources","synthetic_dir":"_synthetic","generated_dir":"_generated","filename_format":"{{course}}-{{mm}}-{{dd}}","default_backend":"mock","skill_variant":"claude","snapshot_mode":"pre_merge"}},"sessions":{{}},"sources":{{}},"topics":{{}},"session_in_progress":null,"flags_path":"_generated/flags.json"}}"#,
        vs = serde_json::to_string(&vault_str).unwrap(),
    );
    fs::write(root.join("csnotes.json"), &manifest).unwrap();

    // Drop a raw note and a matching Plaud transcript
    fs::write(root.join("notes/CPSC5001-09-03.md"), "# Lecture 1\n").unwrap();
    fs::write(root.join("plaud/CPSC5001-09-03-transcript.md"), "Transcript text.\n").unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_csnotes"))
        .arg("reconcile")
        .current_dir(root)
        .status()
        .expect("failed to spawn csnotes");
    assert!(status.success(), "reconcile should succeed");

    let manifest_raw = fs::read_to_string(root.join("csnotes.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();

    assert!(
        manifest["sessions"]["CPSC5001-09-03"].is_object(),
        "session should have been registered"
    );
    assert_eq!(
        manifest["sessions"]["CPSC5001-09-03"]["status"],
        "unprocessed"
    );
    let exports = &manifest["sessions"]["CPSC5001-09-03"]["plaud_exports"];
    assert_eq!(exports.as_array().map(|a| a.len()), Some(1), "transcript should be attached");
    assert!(
        exports[0]["path"].as_str().unwrap_or("").contains("transcript"),
        "export path should reference the transcript file"
    );
}

/// Violation path: mock AI creates a note with a broken wikilink.
/// The invariant suite should catch it, discard the workspace, and leave
/// the vault untouched.
#[test]
fn broken_wikilink_discards_workspace() {
    let tmp = setup_vault();
    let root = tmp.path();

    let status = run_process(root, "broken-wikilink");
    assert!(
        !status.success(),
        "process should fail when invariant suite catches broken wikilink"
    );

    // The bad note must NOT have been merged into the vault.
    assert!(
        !root.join("_synthetic/cpsc5001/bad-note.md").exists(),
        "bad note should not be present in vault after invariant failure"
    );

    // Manifest must show no committed session and no in-progress record.
    let manifest_raw = fs::read_to_string(root.join("csnotes.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    assert_eq!(
        manifest["sessions"]["CPSC5001-09-03"]["status"],
        "unprocessed",
        "session should remain unprocessed after invariant failure"
    );
    assert!(
        manifest["session_in_progress"].is_null(),
        "session_in_progress should be cleared even after discard"
    );
}
