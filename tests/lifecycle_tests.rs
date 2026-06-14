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
        vault_root.join("csnotes.toml"),
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
    "recordings_dir": "recordings",
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
      "recording_exports": [],
      "artifacts": [],
      "recording_missing": true,
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
    assert!(
        status.success(),
        "process should succeed with default fixture"
    );

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
        manifest["sessions"]["CPSC5001-09-03"]["status"], "processed",
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
/// recording export to it.
#[test]
fn reconcile_registers_session_and_recording() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Minimal vault with no sessions yet
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("recordings")).unwrap();
    fs::create_dir_all(root.join("_synthetic")).unwrap();
    write_config(root);
    // Write a manifest with no sessions
    let vault_str = root.to_string_lossy();
    let manifest = format!(
        r#"{{"version":"2","vault_root":{vs},"config":{{"raw_dir":"notes","recordings_dir":"recordings","artifacts_dir":"artifacts","sources_dir":"sources","synthetic_dir":"_synthetic","generated_dir":"_generated","filename_format":"{{course}}-{{mm}}-{{dd}}","default_backend":"mock","skill_variant":"claude","snapshot_mode":"pre_merge"}},"sessions":{{}},"sources":{{}},"topics":{{}},"session_in_progress":null,"flags_path":"_generated/flags.json"}}"#,
        vs = serde_json::to_string(&vault_str).unwrap(),
    );
    fs::write(root.join("csnotes.json"), &manifest).unwrap();

    // Drop a raw note and a matching recording transcript
    fs::write(root.join("notes/CPSC5001-09-03.md"), "# Lecture 1\n").unwrap();
    fs::write(
        root.join("recordings/CPSC5001-09-03-transcript.md"),
        "Transcript text.\n",
    )
    .unwrap();

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
    let exports = &manifest["sessions"]["CPSC5001-09-03"]["recording_exports"];
    assert_eq!(
        exports.as_array().map(|a| a.len()),
        Some(1),
        "transcript should be attached"
    );
    assert!(
        exports[0]["path"]
            .as_str()
            .unwrap_or("")
            .contains("transcript"),
        "export path should reference the transcript file"
    );
}

/// Crash simulation: snapshot present + partial merge + workspace gone → `recover --discard`
/// restores `_synthetic/` from the snapshot and clears `session_in_progress`.
///
/// This exercises the path in `recover::run` where a pre-merge snapshot is detected,
/// `restore_snapshot` is called, and then the missing workspace causes the record to be
/// cleared without prompting (because `--discard` is set).
#[test]
fn recover_restores_from_mid_merge_snapshot() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    let run_id = "crash-sim-test";
    let fake_workspace = root.join(format!("_ws_{}", run_id));

    // Set up vault directories
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("_synthetic/cpsc5001")).unwrap();
    write_config(root);

    // Pre-merge snapshot with the known-good note content
    let good_content = "\
---
csnotes_schema: 1
kind: atomic
topic: cpsc5001
title: Good Note
block_id: good-note-id
contributing_sessions: []
contributing_sources: []
created: \"2026-09-03T00:00:00Z\"
last_updated: \"2026-09-03T00:00:00Z\"
---

Good content. ^good-note-id
";
    let snapshot_dir = root.join(format!("_synthetic_snapshot_{}", run_id));
    fs::create_dir_all(snapshot_dir.join("cpsc5001")).unwrap();
    fs::write(snapshot_dir.join("cpsc5001/good-note.md"), good_content).unwrap();

    // Simulate a partial merge: _synthetic/ has garbage (merge started but crashed)
    fs::write(
        root.join("_synthetic/cpsc5001/good-note.md"),
        "PARTIAL GARBAGE — merge was interrupted\n",
    )
    .unwrap();

    // Write a manifest with session_in_progress pointing to a nonexistent workspace
    let vault_str = root.to_string_lossy();
    let workspace_str = fake_workspace.to_string_lossy();
    let manifest = format!(
        r#"{{
  "version": "2",
  "vault_root": {vault_json},
  "config": {{
    "raw_dir": "notes",
    "recordings_dir": "recordings",
    "artifacts_dir": "artifacts",
    "sources_dir": "sources",
    "synthetic_dir": "_synthetic",
    "generated_dir": "_generated",
    "filename_format": "{{course}}-{{mm}}-{{dd}}",
    "default_backend": "mock",
    "skill_variant": "claude",
    "snapshot_mode": "pre_merge"
  }},
  "sessions": {{}},
  "sources": {{}},
  "topics": {{}},
  "session_in_progress": {{
    "run_id": {run_id_json},
    "started_at": "2026-09-03T10:00:00Z",
    "workspace_path": {workspace_json},
    "phase": "merge_back",
    "error": null,
    "backend": "mock",
    "skill_variant": "claude"
  }},
  "flags_path": "_generated/flags.json"
}}"#,
        vault_json = serde_json::to_string(&vault_str).unwrap(),
        run_id_json = serde_json::to_string(run_id).unwrap(),
        workspace_json = serde_json::to_string(&workspace_str).unwrap(),
    );
    fs::write(root.join("csnotes.json"), &manifest).unwrap();

    // Run recover --discard (non-interactive)
    let status = Command::new(env!("CARGO_BIN_EXE_csnotes"))
        .args(["recover", "--discard"])
        .current_dir(root)
        .status()
        .expect("failed to spawn csnotes");
    assert!(status.success(), "recover --discard should succeed");

    // _synthetic/ must contain the restored note, not the garbage
    let restored = fs::read_to_string(root.join("_synthetic/cpsc5001/good-note.md")).unwrap();
    assert_eq!(
        restored, good_content,
        "_synthetic/ should be restored from snapshot"
    );

    // session_in_progress must be cleared
    let manifest_raw = fs::read_to_string(root.join("csnotes.json")).unwrap();
    let manifest_val: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    assert!(
        manifest_val["session_in_progress"].is_null(),
        "session_in_progress should be cleared after recover"
    );

    // Snapshot dir is consumed by restore_snapshot (renamed to _synthetic/)
    assert!(
        !snapshot_dir.exists(),
        "snapshot dir should be gone after restore"
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
        manifest["sessions"]["CPSC5001-09-03"]["status"], "unprocessed",
        "session should remain unprocessed after invariant failure"
    );
    assert!(
        manifest["session_in_progress"].is_null(),
        "session_in_progress should be cleared even after discard"
    );
}

/// rename_topic op: the topic folder is renamed and notes updated.
/// Verifies the Op::RenameTopic dispatch arm in run_teardown.
#[test]
fn rename_topic_op_renames_folder_in_vault() {
    let tmp = setup_vault();
    let root = tmp.path();

    let status = run_process(root, "rename-topic");
    assert!(
        status.success(),
        "process should succeed with rename-topic fixture"
    );

    assert!(
        root.join("_synthetic/algorithms").exists(),
        "algorithms/ folder should exist after rename"
    );
    assert!(
        !root.join("_synthetic/cpsc5001").exists(),
        "cpsc5001/ folder should be gone after rename"
    );
    let index = fs::read_to_string(root.join("_synthetic/algorithms/cpsc5001.md")).unwrap();
    assert!(
        index.contains("topic: algorithms"),
        "index frontmatter should reflect renamed topic"
    );
}

/// set_embed op: embed line is added to the index note.
/// Verifies the Op::SetEmbed dispatch arm in run_teardown.
#[test]
fn set_embed_op_adds_embed_line_to_index() {
    let tmp = setup_vault();
    let root = tmp.path();

    let status = run_process(root, "set-embed");
    assert!(
        status.success(),
        "process should succeed with set-embed fixture"
    );

    let index = fs::read_to_string(root.join("_synthetic/cs/cs.md")).unwrap();
    assert!(
        index.contains("![[sorting#^sort-id]]"),
        "index should contain the embed line after set_embed: {index}"
    );
}

/// update_note op: provenance is merged into the note's frontmatter.
/// Verifies the Op::UpdateNote dispatch arm in run_teardown.
#[test]
fn update_note_op_merges_provenance_into_note() {
    let tmp = setup_vault();
    let root = tmp.path();

    let status = run_process(root, "update-note");
    assert!(
        status.success(),
        "process should succeed with update-note fixture"
    );

    let note = fs::read_to_string(root.join("_synthetic/cs/sorting.md")).unwrap();
    assert!(
        note.contains("CPSC5001"),
        "note frontmatter should contain the added contributing session: {note}"
    );
}
