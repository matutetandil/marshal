// Git interaction layer.
//
// Initially, we shell out to the `git` binary. This is the simplest approach
// and gives us 100% compatibility with whatever git version the user has.
//
// Later, for performance-critical paths or when shelling out becomes fragile,
// we may switch to libgit2 via the `git2` crate. The abstractions here are
// designed to allow that swap without leaking implementation details.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Execute a git command in the given directory, capturing output.
///
/// This is the primary building block for all git interactions.
pub fn run(cwd: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .context("failed to execute git")?;
    Ok(output)
}

/// Execute a git command passing stdin/stdout/stderr through to the user.
///
/// Used for passthrough mode where we want git's output to reach the user
/// verbatim, including colors and interactive prompts.
pub fn run_interactive(cwd: &Path, args: &[&str]) -> Result<std::process::ExitStatus> {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to execute git")?;
    Ok(status)
}

/// Get the current branch name of a repo, or None if in detached HEAD.
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
pub fn is_dirty(repo: &Path) -> Result<bool> {
    let out = run(repo, &["status", "--porcelain"])?;
    if !out.status.success() {
        bail!("git status failed in {}", repo.display());
    }
    Ok(!out.stdout.is_empty())
}

/// Get the commit hash that a ref points to.
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
