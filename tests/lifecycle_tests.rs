/// End-to-end §7 lifecycle tests using the mock backend.
///
/// Each test initialises a temp vault, runs `csnotes process --backend mock`,
/// and asserts invariants about the resulting vault state.
///
/// Run with: cargo test --test lifecycle_tests

// Tests live here — populated in Phase 0 CI work.
// The fixture vault is at tests/fixtures/default/.

#[test]
fn placeholder_compiles() {
    // Remove once real lifecycle tests are added.
    assert!(true);
}
