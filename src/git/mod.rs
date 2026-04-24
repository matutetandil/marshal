// Git interaction layer.
//
// Initially, we shell out to the `git` binary. This is the simplest approach
// and gives us 100% compatibility with whatever git version the user has.
//
// Later, for performance-critical paths or when shelling out becomes fragile,
// we may switch to libgit2 via the `git2` crate. The abstractions here are
// designed to allow that swap without leaking implementation details.

pub mod parser;

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, Output};

/// Execute a git command in the given directory, capturing output.
///
/// Primary building block for helpers that need to parse git's output. For
/// forwarding invocations directly to the user, see `commands::passthrough`.
pub fn run(cwd: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .context("failed to execute git")?;
    Ok(output)
}

/// Get the current branch name of a repo, or None if in detached HEAD.
#[allow(dead_code)] // Consumed by workspace-aware commands starting in Phase 2.
pub fn current_branch(repo: &Path) -> Result<Option<String>> {
    let out = run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    if out.status.success() {
        let branch = String::from_utf8(out.stdout)
            .context("git output is not valid UTF-8")?
            .trim()
            .to_string();
        Ok(Some(branch))
    } else {
        // Non-zero exit with --quiet means detached HEAD; that's not an error.
        Ok(None)
    }
}

/// Check if a repo has any uncommitted changes in the working tree or index.
#[allow(dead_code)] // Consumed by workspace-aware commands starting in Phase 2.
pub fn is_dirty(repo: &Path) -> Result<bool> {
    let out = run(repo, &["status", "--porcelain"])?;
    if !out.status.success() {
        bail!("git status failed in {}", repo.display());
    }
    Ok(!out.stdout.is_empty())
}

/// Get the commit hash that a ref points to.
#[allow(dead_code)] // Consumed by workspace-aware commands starting in Phase 2.
pub fn rev_parse(repo: &Path, reference: &str) -> Result<String> {
    let out = run(repo, &["rev-parse", reference])?;
    if !out.status.success() {
        bail!(
            "failed to resolve ref '{}' in {}: {}",
            reference,
            repo.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8(out.stdout)
        .context("git output is not valid UTF-8")?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_test_repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path();
        Command::new("git")
            .current_dir(path)
            .args(["init", "--quiet", "--initial-branch=main"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(path)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        tmp
    }

    #[test]
    fn detects_clean_repo() {
        let tmp = init_test_repo();
        // Fresh repo with no commits is still "clean" (empty status).
        assert!(!is_dirty(tmp.path()).unwrap());
    }

    #[test]
    fn detects_dirty_repo() {
        let tmp = init_test_repo();
        std::fs::write(tmp.path().join("file.txt"), "content").unwrap();
        assert!(is_dirty(tmp.path()).unwrap());
    }
}
