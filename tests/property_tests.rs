/// Phase 3 property-based tests: idempotency, transactionality, reindex fidelity.
///
/// Three categories:
///
/// 1. **proptest idempotency** — `reconcile` is a stable fixed point: running it
///    twice on the same vault produces byte-identical manifests regardless of
///    which sessions, recording exports, or sources are present.
///
/// 2. **proptest completeness** — every file on disk after `reconcile` is
///    registered; no entries are double-counted or silently dropped.
///
/// 3. **lifecycle / transactionality** — deterministic crash-and-recovery and
///    reindex-fidelity scenarios that don't need varied random inputs.
use proptest::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const BINARY: &str = env!("CARGO_BIN_EXE_csnotes");

// ── Shared vault helpers ──────────────────────────────────────────────────────

fn write_minimal_config(root: &Path) {
    fs::write(root.join("csnotes.toml"), "default_backend = \"mock\"\n").unwrap();
}

fn read_manifest(root: &Path) -> serde_json::Value {
    let raw = fs::read_to_string(root.join("csnotes.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// Write an empty `csnotes.json` (no sessions, sources, or topics).
fn write_empty_manifest(root: &Path) {
    let vs = serde_json::to_string(&root.to_string_lossy()).unwrap();
    let content = format!(
        r#"{{"version":"2","vault_root":{vs},"config":{{"raw_dir":"notes","recordings_dir":"recordings","artifacts_dir":"artifacts","sources_dir":"sources","synthetic_dir":"_synthetic","generated_dir":"_generated","filename_format":"{{course}}-{{mm}}-{{dd}}","default_backend":"mock","skill_variant":"claude","snapshot_mode":"pre_merge"}},"sessions":{{}},"sources":{{}},"topics":{{}},"session_in_progress":null,"flags_path":"_generated/flags.json"}}"#,
    );
    fs::write(root.join("csnotes.json"), content).unwrap();
}

/// Write a `csnotes.json` with one unprocessed session (`CPSC5001-09-03`).
fn write_single_session_manifest(root: &Path) {
    let vs = serde_json::to_string(&root.to_string_lossy()).unwrap();
    let manifest = format!(
        r#"{{"version":"2","vault_root":{vs},"config":{{"raw_dir":"notes","recordings_dir":"recordings","artifacts_dir":"artifacts","sources_dir":"sources","synthetic_dir":"_synthetic","generated_dir":"_generated","filename_format":"{{course}}-{{mm}}-{{dd}}","default_backend":"mock","skill_variant":"claude","snapshot_mode":"pre_merge"}},"sessions":{{"CPSC5001-09-03":{{"date":"2026-09-03","course":"CPSC5001","filename_format":"{{course}}-{{mm}}-{{dd}}","raw_note":"notes/CPSC5001-09-03.md","recording_exports":[],"artifacts":[],"recording_missing":true,"status":"unprocessed","processed_at":null,"topics_updated":[]}}}},"sources":{{}},"topics":{{}},"session_in_progress":null,"flags_path":"_generated/flags.json"}}"#,
    );
    fs::write(root.join("csnotes.json"), manifest).unwrap();
}

/// Create a minimal vault suitable for reconcile proptest runs.
fn setup_reconcile_vault() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("recordings")).unwrap();
    fs::create_dir_all(root.join("sources")).unwrap();
    fs::create_dir_all(root.join("_synthetic")).unwrap();
    write_minimal_config(root);
    write_empty_manifest(root);
    tmp
}

/// Create the vault used by `happy_path` and `reindex_is_stable_after_clean_process`.
fn setup_process_vault() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("_synthetic")).unwrap();
    write_minimal_config(root);
    write_single_session_manifest(root);
    fs::write(
        root.join("notes/CPSC5001-09-03.md"),
        "# CPSC5001 — Sep 3 2026\n\nIntro lecture: course overview and algorithm analysis.\n",
    )
    .unwrap();
    tmp
}

fn run_reconcile(root: &Path) -> bool {
    Command::new(BINARY)
        .arg("reconcile")
        .current_dir(root)
        .status()
        .expect("spawn reconcile")
        .success()
}

// ── proptest strategies ───────────────────────────────────────────────────────

fn course_str() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("CPSC5001"), Just("CPSC5002"), Just("CPSC5005"),]
}

/// (course, month 1..=12, day 1..=28) — stays within unambiguous date range.
fn session_triple() -> impl Strategy<Value = (&'static str, u32, u32)> {
    (course_str(), 1u32..=12u32, 1u32..=28u32)
}

/// Valid source filename stems: lowercase letter, then alphanumerics/hyphens.
fn source_stem() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{2,7}(-[a-z][a-z0-9]{1,5}){0,2}".prop_map(|s| s)
}

// ── Property tests ────────────────────────────────────────────────────────────

proptest! {
    // 32 cases per test keeps CI fast while still exercising meaningful variety.
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Running `reconcile` twice on the same vault produces a byte-identical
    /// manifest.  Catches any non-idempotent write: double-registration,
    /// timestamp churn on unmodified entries, etc.
    #[test]
    fn reconcile_is_idempotent(
        triples in proptest::collection::vec(session_triple(), 1..=5)
    ) {
        let tmp = setup_reconcile_vault();
        let root = tmp.path();

        // Deduplicate: same course+date = same session ID.
        let mut written: HashSet<String> = HashSet::new();
        for (course, mm, dd) in &triples {
            let name = format!("{}-{:02}-{:02}.md", course, mm, dd);
            if written.insert(name.clone()) {
                fs::write(root.join("notes").join(&name), "# Lecture\n").unwrap();
            }
        }

        prop_assert!(run_reconcile(root), "first reconcile should succeed");
        let after_first = fs::read_to_string(root.join("csnotes.json")).unwrap();

        prop_assert!(run_reconcile(root), "second reconcile should succeed");
        let after_second = fs::read_to_string(root.join("csnotes.json")).unwrap();

        prop_assert_eq!(
            after_first,
            after_second,
            "manifest must be byte-identical after a second reconcile on unchanged vault"
        );
    }

    /// Every raw note file created before `reconcile` must appear in
    /// `manifest.sessions` with exactly one entry.
    #[test]
    fn reconcile_registers_all_raw_notes(
        triples in proptest::collection::vec(session_triple(), 1..=5)
    ) {
        let tmp = setup_reconcile_vault();
        let root = tmp.path();

        let mut expected: HashSet<String> = HashSet::new();
        for (course, mm, dd) in &triples {
            let name = format!("{}-{:02}-{:02}.md", course, mm, dd);
            let id   = format!("{}-{:02}-{:02}", course, mm, dd);
            let path = root.join("notes").join(&name);
            if !path.exists() {
                fs::write(&path, "# Lecture\n").unwrap();
            }
            expected.insert(id);
        }

        prop_assert!(run_reconcile(root), "reconcile should succeed");

        let manifest = read_manifest(root);
        let sessions = manifest["sessions"].as_object().unwrap();

        prop_assert_eq!(
            sessions.len(),
            expected.len(),
            "manifest session count should equal number of unique raw note files"
        );
        for id in &expected {
            prop_assert!(
                sessions.contains_key(id.as_str()),
                "session '{}' must be registered in manifest",
                id
            );
        }
    }

    /// Source registration is idempotent: a second `reconcile` run must not
    /// add duplicate source entries or mutate existing ones.
    #[test]
    fn reconcile_source_registration_is_idempotent(
        stems in proptest::collection::vec(source_stem(), 1..=3)
    ) {
        let tmp = setup_reconcile_vault();
        let root = tmp.path();

        // One raw note gives reconcile a session to register.
        fs::write(root.join("notes/CPSC5001-09-03.md"), "# Lecture\n").unwrap();

        let mut written: HashSet<String> = HashSet::new();
        for stem in &stems {
            if written.insert(stem.clone()) {
                let name = format!("{}.md", stem);
                fs::write(
                    root.join("sources").join(&name),
                    "# Source\n\n## Chapter 1\n\nContent.\n",
                )
                .unwrap();
            }
        }

        prop_assert!(run_reconcile(root), "first reconcile should succeed");
        let after_first = fs::read_to_string(root.join("csnotes.json")).unwrap();

        prop_assert!(run_reconcile(root), "second reconcile should succeed");
        let after_second = fs::read_to_string(root.join("csnotes.json")).unwrap();

        prop_assert_eq!(
            after_first,
            after_second,
            "sources section must be byte-identical after a second reconcile"
        );
    }
}

// ── Lifecycle / transactionality tests ───────────────────────────────────────

/// `recover --discard` must clear a stale `session_in_progress` record even
/// when the referenced workspace directory no longer exists (simulates a crash
/// after workspace teardown but before the manifest was updated).
#[test]
fn recover_discard_clears_stale_in_progress() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("_synthetic")).unwrap();
    write_minimal_config(root);

    // Inject an in-progress record whose workspace directory does NOT exist.
    let vs = serde_json::to_string(&root.to_string_lossy()).unwrap();
    let ghost_ws = root.join("_workspace_deadbeef");
    let ws_json = serde_json::to_string(&ghost_ws.to_string_lossy()).unwrap();
    let manifest = format!(
        r#"{{"version":"2","vault_root":{vs},"config":{{"raw_dir":"notes","recordings_dir":"recordings","artifacts_dir":"artifacts","sources_dir":"sources","synthetic_dir":"_synthetic","generated_dir":"_generated","filename_format":"{{course}}-{{mm}}-{{dd}}","default_backend":"mock","skill_variant":"claude","snapshot_mode":"pre_merge"}},"sessions":{{}},"sources":{{}},"topics":{{}},"session_in_progress":{{"run_id":"deadbeef-0000-0000-0000-000000000000","started_at":"2026-09-03T18:00:00Z","workspace_path":{ws_json},"phase":"synthesizing","error":null,"backend":"mock","skill_variant":"claude"}},"flags_path":"_generated/flags.json"}}"#,
    );
    fs::write(root.join("csnotes.json"), &manifest).unwrap();

    let status = Command::new(BINARY)
        .args(["recover", "--discard"])
        .current_dir(root)
        .status()
        .expect("spawn recover --discard");
    assert!(
        status.success(),
        "recover --discard should succeed with a ghost workspace"
    );

    let after = read_manifest(root);
    assert!(
        after["session_in_progress"].is_null(),
        "session_in_progress must be null after recover --discard"
    );
}

/// After a clean `process` run, `audit --reindex` must be a stable fixed
/// point: the sessions and topics sets must be unchanged, and the processed
/// session status must be preserved.
#[test]
fn reindex_is_stable_after_clean_process() {
    let tmp = setup_process_vault();
    let root = tmp.path();

    let status = Command::new(BINARY)
        .args(["process", "--backend", "mock", "--fixture", "default"])
        .current_dir(root)
        .status()
        .expect("spawn process");
    assert!(
        status.success(),
        "process should succeed with default fixture"
    );

    let before = read_manifest(root);

    let status = Command::new(BINARY)
        .args(["audit", "--reindex"])
        .current_dir(root)
        .status()
        .expect("spawn audit --reindex");
    assert!(
        status.success(),
        "audit --reindex should succeed after clean process"
    );

    let after = read_manifest(root);

    // Sessions set must be unchanged.
    let before_sessions: Vec<&str> = before["sessions"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let after_sessions: Vec<&str> = after["sessions"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        before_sessions, after_sessions,
        "sessions keys must be unchanged after reindex"
    );

    // Topics set must be unchanged.
    let before_topics: Vec<&str> = before["topics"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let after_topics: Vec<&str> = after["topics"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        before_topics, after_topics,
        "topics keys must be unchanged after reindex"
    );

    // Processed status must survive reindex.
    assert_eq!(
        after["sessions"]["CPSC5001-09-03"]["status"], "processed",
        "reindex must preserve the processed status for committed sessions"
    );

    // No in-progress record should appear after reindex.
    assert!(
        after["session_in_progress"].is_null(),
        "reindex must not introduce a stale session_in_progress"
    );
}
