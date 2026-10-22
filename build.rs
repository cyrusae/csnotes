//! Embeds the git commit and commit date into `csnotes --version`, so a binary
//! installed with `cargo install --git` reports exactly which commit it is.
//! Builds outside a git checkout (e.g. a source tarball) report the bare
//! Cargo version.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap();

    let version = match git_info(&manifest_dir) {
        Some((sha, date)) => format!("{pkg_version} ({sha} {date})"),
        None => pkg_version,
    };
    println!("cargo:rustc-env=CSNOTES_VERSION={version}");
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Returns `(short_sha, commit_date)` if `dir` is the root of a git checkout.
fn git_info(dir: &Path) -> Option<(String, String)> {
    // Only trust git when this crate *is* the repo root; otherwise we could
    // pick up the commit of some enclosing repository.
    let toplevel = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?);
    if toplevel.canonicalize().ok()? != dir.canonicalize().ok()? {
        return None;
    }

    // Rebuild when HEAD moves: HEAD itself (branch switches, detached
    // checkouts), the branch ref it points to, and packed refs.
    let mut watch = vec![git(dir, &["rev-parse", "--git-path", "HEAD"])?];
    if let Some(head_ref) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
        watch.extend(git(dir, &["rev-parse", "--git-path", &head_ref]));
    }
    watch.extend(git(dir, &["rev-parse", "--git-path", "packed-refs"]));
    for path in watch {
        let path = dir.join(path);
        // A missing path would make Cargo rerun this script on every build.
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    println!("cargo:rerun-if-changed=build.rs");

    let sha = git(dir, &["rev-parse", "--short=7", "HEAD"])?;
    let date = git(dir, &["log", "-1", "--format=%cs", "HEAD"])?;
    Some((sha, date))
}
