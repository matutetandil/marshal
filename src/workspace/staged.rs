//! Per-developer staging declaration — the "index" of the workspace.
//!
//! Mirrors the role of git's index inside a repo: a snapshot of what
//! the user wants the next workspace commit (= next `state.toml`) to
//! contain, captured at stage time and held until commit. Stored at
//! `<root>/.workspace/local/staged.toml`. The `local/` directory is
//! gitignored at the workspace level so per-developer staging never
//! leaks into the workspace repo's history.
//!
//! Mental model — three zones (Phase 3):
//!
//! * **Working** — what every child repo's HEAD actually is right now.
//! * **Staged** — `staged.toml`, populated by `ws add`. Per-repo
//!   `(branch, commit)` snapshots taken at stage time.
//! * **Declared** — `state.toml`, the committed source-of-truth in
//!   the workspace repo. Updated by `ws commit` (flushes staged into
//!   declared and clears staged).
//!
//! The snapshot semantics match `git add` exactly: stage captures
//! the *current* values; running `ws add` again on the same repo
//! re-snapshots; modifying the working tree of a child after stage
//! does not change what is staged. This protects deploy-manifest
//! atomicity — what the user tested as a coordinated set is exactly
//! what `ws commit` will record.
//!
//! Unlike `RepoState` in `state.toml` (where `commit` is optional —
//! a state declaration may pin only the branch), `RepoStaged.commit`
//! is required. A stage entry must be reproducible.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::{LOCAL_DIR, WORKSPACE_MARKER};

/// File name of the staging declaration inside `.workspace/local/`.
pub const STAGED_FILE: &str = "staged.toml";

/// Header comment written when the staging file is created on disk
/// for the first time. Helps a user who opens the file directly
/// understand why it exists and what it contains.
const STAGED_FILE_HEADER: &str = "\
# staged.toml — per-developer staging area for the workspace.
#
# Populated by `ws add <repo>`; flushed into `.workspace/state.toml`
# by `ws commit`. Do not commit this file: the parent `.workspace/local/`
# directory is gitignored.
#
# Each entry pins the `(branch, commit)` of the child repo at stage time:
#   [repos.\"<repo-name>\"]
#   branch = \"<branch-name>\"
#   commit = \"<sha>\"
";

/// In-memory representation of `staged.toml`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StagedDeclaration {
    /// One entry per staged repo. Empty when nothing is staged.
    /// `skip_serializing_if` keeps the on-disk file clean after a
    /// `ws reset` (header + empty body, no `[repos]` empty table
    /// noise). Round-trip stays correct because `#[serde(default)]`
    /// re-creates the empty map on deserialize.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub repos: HashMap<String, RepoStaged>,
}

/// One staged child-repo snapshot. `commit` is required — a stage
/// entry must be reproducible (a state declaration in `state.toml`
/// can omit `commit` to mean "any tip of the branch", but a staged
/// snapshot is a precise pointer).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RepoStaged {
    pub branch: String,
    pub commit: String,
}

impl StagedDeclaration {
    /// Try to load the staging declaration from
    /// `<root>/.workspace/local/staged.toml`.
    ///
    /// Returns:
    /// * `Ok(None)` when the file does not exist — fresh workspace,
    ///   nothing staged. Distinct from "empty file" (which returns
    ///   `Ok(Some(default))`).
    /// * `Ok(Some(staged))` when loaded and parsed cleanly.
    /// * `Err(_)` when the file exists but is unreadable / malformed.
    ///
    /// Three-way semantics match the manifest and state loaders, so
    /// callers that already branch on those can branch on this the
    /// same way.
    pub fn try_load_from_workspace(workspace_root: &Path) -> Result<Option<Self>> {
        let path = staged_path(workspace_root);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read staging declaration at {}", path.display()))?;
        Self::parse(&content).map(Some)
    }

    /// Load the staging declaration, returning an empty declaration
    /// when the file does not exist. Convenience for callers that
    /// don't need to distinguish "no file" from "empty file".
    pub fn load_or_default(workspace_root: &Path) -> Result<Self> {
        Ok(Self::try_load_from_workspace(workspace_root)?.unwrap_or_default())
    }

    /// Parse a staging declaration from a string.
    pub fn parse(content: &str) -> Result<Self> {
        toml::from_str(content).context("failed to parse staging declaration TOML")
    }

    /// Serialise to TOML string. Used by `ws add` / `ws unstage`
    /// to persist the in-memory state back to disk.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialise staging declaration")
    }

    /// Persist the staging declaration to disk. Creates the
    /// `.workspace/local/` directory if missing and seeds it with a
    /// `.gitignore` (`*` — ignore everything inside) on first write
    /// so per-developer staging never accidentally leaks into the
    /// workspace repo's commit history.
    ///
    /// When the declaration is empty (`repos` is empty), writes the
    /// file anyway — preserving the empty body keeps the file
    /// available as documentation of what `local/` is for. Callers
    /// that prefer to remove the file on empty can do so explicitly.
    pub fn save_to_workspace(&self, workspace_root: &Path) -> Result<()> {
        let local_dir = workspace_root.join(WORKSPACE_MARKER).join(LOCAL_DIR);
        std::fs::create_dir_all(&local_dir).with_context(|| {
            format!(
                "failed to create local staging directory {}",
                local_dir.display()
            )
        })?;

        // Seed `.gitignore` once. Idempotent — only writes when the
        // file does not already exist, so a user who edits theirs is
        // never overwritten.
        let gitignore = local_dir.join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(&gitignore, "*\n")
                .with_context(|| format!("failed to write {}", gitignore.display()))?;
        }

        let body = self.to_toml()?;
        let mut content = String::with_capacity(STAGED_FILE_HEADER.len() + body.len());
        content.push_str(STAGED_FILE_HEADER);
        content.push_str(&body);

        let path = local_dir.join(STAGED_FILE);
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write staging declaration at {}", path.display()))
    }

    /// Look up the staged snapshot for a repo by name.
    #[allow(dead_code)] // Consumed by `ws status` integration (Slice B).
    pub fn get(&self, repo_name: &str) -> Option<&RepoStaged> {
        self.repos.get(repo_name)
    }

    /// Insert or overwrite the snapshot for `repo_name`. Returns the
    /// previous entry, if any — useful for the renderer to surface
    /// "staged X (was Y)" when re-staging.
    pub fn stage(&mut self, repo_name: String, snapshot: RepoStaged) -> Option<RepoStaged> {
        self.repos.insert(repo_name, snapshot)
    }

    /// Remove a staged entry. Returns the previous entry, if any.
    pub fn unstage(&mut self, repo_name: &str) -> Option<RepoStaged> {
        self.repos.remove(repo_name)
    }

    /// `true` when nothing is staged.
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
    }
}

/// Absolute path of `staged.toml` for a given workspace root.
fn staged_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(WORKSPACE_MARKER)
        .join(LOCAL_DIR)
        .join(STAGED_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rs(branch: &str, commit: &str) -> RepoStaged {
        RepoStaged {
            branch: branch.to_string(),
            commit: commit.to_string(),
        }
    }

    #[test]
    fn parses_empty_staged() {
        let staged = StagedDeclaration::parse("").unwrap();
        assert!(staged.is_empty());
    }

    #[test]
    fn parses_staged_entries() {
        let toml = r#"
            [repos."service-a"]
            branch = "feat/payment"
            commit = "abc123"

            [repos."service-b"]
            branch = "main"
            commit = "def456"
        "#;
        let staged = StagedDeclaration::parse(toml).unwrap();
        assert_eq!(staged.repos.len(), 2);
        assert_eq!(staged.get("service-a"), Some(&rs("feat/payment", "abc123")));
        assert_eq!(staged.get("service-b"), Some(&rs("main", "def456")));
    }

    #[test]
    fn missing_commit_is_a_parse_error() {
        // Unlike `state.toml`, a staged entry MUST carry a commit —
        // the snapshot has to be reproducible.
        let toml = r#"
            [repos."service-a"]
            branch = "main"
        "#;
        let err = StagedDeclaration::parse(toml).unwrap_err();
        // The top-level message comes from our `.context(…)` wrap; the
        // serde "missing field `commit`" lives in the cause chain.
        let chain: String = err
            .chain()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" / ")
            .to_lowercase();
        assert!(
            chain.contains("commit"),
            "expected the error chain to mention the missing commit field, got: {chain}"
        );
    }

    #[test]
    fn round_trip_serialisation() {
        let mut original = StagedDeclaration::default();
        original.stage("service-a".to_string(), rs("feat/x", "abc"));
        original.stage("service-b".to_string(), rs("main", "def"));

        let body = original.to_toml().unwrap();
        let parsed = StagedDeclaration::parse(&body).unwrap();
        assert_eq!(parsed.repos.len(), 2);
        assert_eq!(parsed.get("service-a"), Some(&rs("feat/x", "abc")));
    }

    #[test]
    fn stage_returns_previous_entry_on_overwrite() {
        let mut staged = StagedDeclaration::default();
        assert!(staged.stage("svc".to_string(), rs("main", "abc")).is_none());
        let prev = staged
            .stage("svc".to_string(), rs("feat/x", "def"))
            .unwrap();
        assert_eq!(prev, rs("main", "abc"));
        assert_eq!(staged.get("svc"), Some(&rs("feat/x", "def")));
    }

    #[test]
    fn unstage_returns_removed_entry() {
        let mut staged = StagedDeclaration::default();
        staged.stage("svc".to_string(), rs("main", "abc"));
        let removed = staged.unstage("svc").unwrap();
        assert_eq!(removed, rs("main", "abc"));
        assert!(staged.is_empty());
        assert!(staged.unstage("svc").is_none());
    }

    #[test]
    fn try_load_from_missing_returns_ok_none() {
        let tmp = TempDir::new().unwrap();
        let result = StagedDeclaration::try_load_from_workspace(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn save_seeds_gitignore_on_first_write() {
        let tmp = TempDir::new().unwrap();
        let staged = StagedDeclaration::default();
        staged.save_to_workspace(tmp.path()).unwrap();

        let gitignore = tmp
            .path()
            .join(WORKSPACE_MARKER)
            .join(LOCAL_DIR)
            .join(".gitignore");
        assert!(gitignore.is_file());
        assert_eq!(std::fs::read_to_string(&gitignore).unwrap(), "*\n");
    }

    #[test]
    fn save_does_not_overwrite_existing_gitignore() {
        let tmp = TempDir::new().unwrap();
        // Pre-populate a customised .gitignore.
        let local = tmp.path().join(WORKSPACE_MARKER).join(LOCAL_DIR);
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join(".gitignore"), "# custom\n").unwrap();

        StagedDeclaration::default()
            .save_to_workspace(tmp.path())
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(local.join(".gitignore")).unwrap(),
            "# custom\n"
        );
    }

    #[test]
    fn save_then_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let mut staged = StagedDeclaration::default();
        staged.stage("svc".to_string(), rs("feat/x", "abc123"));
        staged.save_to_workspace(tmp.path()).unwrap();

        let loaded = StagedDeclaration::try_load_from_workspace(tmp.path())
            .unwrap()
            .unwrap();
        assert_eq!(loaded.get("svc"), Some(&rs("feat/x", "abc123")));
    }

    #[test]
    fn load_or_default_returns_empty_when_missing() {
        let tmp = TempDir::new().unwrap();
        let staged = StagedDeclaration::load_or_default(tmp.path()).unwrap();
        assert!(staged.is_empty());
    }
}
