/// Path safety utilities for AI-produced report paths.
///
/// All paths written into `_session_report.json` by an AI session are
/// untrusted.  Before joining any such string onto a root directory, call
/// `safe_join` so that absolute paths and parent-directory traversal are
/// rejected regardless of what the AI wrote.
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};

/// Join `root` with an AI-provided `unsafe_path`, refusing:
/// - absolute paths (`/etc/passwd`)
/// - parent-directory traversal (`../../etc/passwd`)
/// - embedded NUL bytes
///
/// Does not require the target to exist on disk, so it is safe to use for
/// `create_note` paths that have not been written yet.
pub fn safe_join(root: &Path, unsafe_path: &str) -> Result<PathBuf> {
    if unsafe_path.contains('\0') {
        bail!("report path contains NUL byte: {:?}", unsafe_path);
    }
    let p = Path::new(unsafe_path);
    if p.is_absolute() {
        bail!("absolute path not allowed in report: {:?}", unsafe_path);
    }
    for component in p.components() {
        if matches!(component, Component::ParentDir) {
            bail!(
                "'..' component not allowed in report path: {:?}",
                unsafe_path
            );
        }
    }
    Ok(root.join(p))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/tmp/vault";

    fn root() -> &'static Path {
        Path::new(ROOT)
    }

    #[test]
    fn rejects_absolute_path() {
        assert!(safe_join(root(), "/etc/passwd").is_err());
        assert!(safe_join(root(), "/Users/watcher/.ssh/authorized_keys").is_err());
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(safe_join(root(), "../../etc/passwd").is_err());
        assert!(safe_join(root(), "_synthetic/../../../etc/passwd").is_err());
        assert!(safe_join(root(), "good/../../bad").is_err());
    }

    #[test]
    fn rejects_nul_byte() {
        assert!(safe_join(root(), "foo\0bar").is_err());
    }

    #[test]
    fn accepts_normal_relative_path() {
        let result = safe_join(root(), "_synthetic/sorting/quicksort.md").unwrap();
        assert_eq!(
            result,
            Path::new("/tmp/vault/_synthetic/sorting/quicksort.md")
        );
    }

    #[test]
    fn accepts_single_component() {
        let result = safe_join(root(), "sorting").unwrap();
        assert_eq!(result, Path::new("/tmp/vault/sorting"));
    }

    #[test]
    fn accepts_current_dir_prefix() {
        // `./foo` is allowed (CurDir component, not ParentDir)
        let result = safe_join(root(), "./sorting/quicksort.md").unwrap();
        assert_eq!(result, Path::new("/tmp/vault/sorting/quicksort.md"));
    }
}
