//! Workspace context detection.
//!
//! On every invocation, we walk up the filesystem from the current directory
//! looking for a `.workspace/` marker directory. If found, we've identified
//! a workspace root and can determine which child repo (if any) we're inside.
//!
//! This mirrors how git finds the `.git/` directory of the current repo.
//!
//! Scaffolded for Phase 2; not consumed by `main` in 0.1.0 (pure passthrough).
//! The unit tests below keep the module honest.

#![allow(dead_code)]

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

/// The marker directory that identifies a workspace root.
pub const WORKSPACE_MARKER: &str = ".workspace";

/// The manifest file inside the workspace marker directory.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// The state declaration file.
pub const STATE_FILE: &str = "state.toml";

/// The local (gitignored) subdirectory for per-developer state.
pub const LOCAL_DIR: &str = "local";

/// A detected workspace context.
#[derive(Debug, Clone)]
pub struct Context {
    /// Absolute path to the workspace root (directory containing `.workspace/`).
    pub root: PathBuf,

    /// If the current directory is inside a child repo, the name of that repo
    /// as declared in the manifest. `None` if we're at the workspace root or
    /// in a workspace-level subdirectory that isn't a child repo.
    pub current_repo: Option<String>,
}

/// Detect workspace context starting from the current directory.
///
/// Returns `Ok(Some(ctx))` if a workspace is detected, `Ok(None)` if not
/// (we're in a plain Git repo or a non-Git directory), or `Err` on IO errors.
pub fn detect() -> Result<Option<Context>> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    Ok(find_workspace_root(&cwd).map(|root| {
        let current_repo = identify_child_repo(&root, &cwd);
        Context { root, current_repo }
    }))
}

/// Walk up the filesystem from `start` looking for a directory containing
/// the workspace marker. Returns the workspace root if found.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        let marker = dir.join(WORKSPACE_MARKER);
        if marker.is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// Given a workspace root and a path somewhere inside it, determine which
/// child repo (if any) the path belongs to.
///
/// By convention, child repos live under `src/` but this may become
/// configurable via the manifest. For now, we use the convention.
fn identify_child_repo(workspace_root: &Path, path: &Path) -> Option<String> {
    // TODO: read manifest to know actual child repo locations.
    // For now, use the convention: src/<repo-name>/
    let src = workspace_root.join("src");
    let relative = path.strip_prefix(&src).ok()?;
    let first_component = relative.components().next()?;
    Some(first_component.as_os_str().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detects_workspace_from_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(WORKSPACE_MARKER)).unwrap();

        let found = find_workspace_root(root);
        assert_eq!(found.as_deref(), Some(root));
    }

    #[test]
    fn detects_workspace_from_child_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(WORKSPACE_MARKER)).unwrap();
        let nested = root.join("src").join("service-a").join("deep").join("path");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_workspace_root(&nested);
        assert_eq!(found.as_deref(), Some(root));
    }

    #[test]
    fn returns_none_when_no_workspace() {
        let tmp = TempDir::new().unwrap();
        let found = find_workspace_root(tmp.path());
        assert!(found.is_none());
    }

    #[test]
    fn identifies_child_repo_by_convention() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let repo_path = root.join("src").join("my-service");
        std::fs::create_dir_all(&repo_path).unwrap();

        let repo = identify_child_repo(root, &repo_path);
        assert_eq!(repo, Some("my-service".to_string()));
    }

    #[test]
    fn no_child_repo_at_workspace_root() {
        let tmp = TempDir::new().unwrap();
        let repo = identify_child_repo(tmp.path(), tmp.path());
        assert!(repo.is_none());
    }
}
