//! State declaration parsing.
//!
//! state.toml declares the expected state of each child repo for the currently
//! active branch of the workspace repo. Different workspace branches have
//! different state.toml contents, versioned normally with Git.
//!
//! The state declaration is intent. It does not force reality; divergence
//! between declared and actual state is normal and handled elsewhere.
//!
//! Lit up in Phase 2 / Slice C. Consumed by the `ws` namespace to enrich
//! `git ws` output with declared-state info; future slices (`ws status`,
//! `ws diff`, `ws switch`) build on the same parsed structure.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::context::{STATE_FILE, WORKSPACE_MARKER};

/// A parsed state declaration. In-memory representation of state.toml.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StateDeclaration {
    /// Map from repo name to its declared state.
    #[serde(default)]
    pub repos: HashMap<String, RepoState>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepoState {
    /// The branch the repo should be on.
    pub branch: String,

    /// Optional last-known commit hash. If present, the tool can detect when
    /// the branch has advanced and offer to update the declaration.
    #[serde(default)]
    pub commit: Option<String>,
}

impl StateDeclaration {
    /// Parse a state declaration from a file path. Returns an empty declaration
    /// if the file doesn't exist (equivalent to "all repos on default").
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read state declaration at {}", path.display()))?;
        Self::parse(&content)
    }

    /// Try to load the state declaration from the standard location
    /// relative to a workspace root: `<root>/.workspace/state.toml`.
    ///
    /// Returns:
    /// * `Ok(None)` when the file does not exist — a workspace with
    ///   no state declaration is fully valid (equivalent to "all
    ///   repos on the manifest's default branch"). Callers that want
    ///   the empty-default semantics can use [`load`] instead.
    /// * `Ok(Some(state))` when the file is loaded and parsed cleanly.
    /// * `Err(_)` when the file exists but is unreadable or malformed.
    pub fn try_load_from_workspace(workspace_root: &Path) -> Result<Option<Self>> {
        let path = workspace_root.join(WORKSPACE_MARKER).join(STATE_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read state declaration at {}", path.display()))?;
        Self::parse(&content).map(Some)
    }

    /// Parse a state declaration from a string.
    pub fn parse(content: &str) -> Result<Self> {
        toml::from_str(content).context("failed to parse state declaration TOML")
    }

    /// Serialize to TOML string for writing.
    #[allow(dead_code)] // Consumed by `ws init` (Phase 2 / Slice D).
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize state declaration")
    }

    /// Get the declared state for a specific repo, or None if not declared.
    pub fn get(&self, repo_name: &str) -> Option<&RepoState> {
        self.repos.get(repo_name)
    }
}

/// One per-repo difference between two state declarations. JSON
/// shape uses an internally-tagged `kind` field
/// (`"added"` / `"removed"` / `"changed"`) so consumers can branch
/// with a single switch — same precedent as
/// `ws clone`'s `ChildResult` and `preflight::Obstacle`.
///
/// Consumed by `ws diff` (working tree vs HEAD) and `ws commit`
/// (about-to-write state.toml vs current state.toml). Living here
/// next to [`StateDeclaration`] keeps the diff logic with the data
/// shape it operates on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateChange {
    Added {
        name: String,
        branch: String,
    },
    Removed {
        name: String,
        branch: String,
    },
    Changed {
        name: String,
        from: String,
        to: String,
    },
}

impl StateChange {
    /// Repo name carried by every variant.
    pub fn name(&self) -> &str {
        match self {
            Self::Added { name, .. } => name,
            Self::Removed { name, .. } => name,
            Self::Changed { name, .. } => name,
        }
    }
}

/// Diff two state declarations into a list of per-repo changes.
/// Returns one [`StateChange`] per repo whose entry differs between
/// `from` and `to`, sorted alphabetically by repo name so the
/// output is predictable across invocations.
///
/// Currently only the `branch` field is compared — the optional
/// `commit` field is ignored because `state.toml` treats a missing
/// commit as "any tip of the branch", and we'd need a richer
/// `Changed` variant to express "branch unchanged but pinned commit
/// changed". A real use case will pull that in.
pub fn diff_states(from: &StateDeclaration, to: &StateDeclaration) -> Vec<StateChange> {
    use std::collections::BTreeSet;

    let mut names: BTreeSet<&String> = BTreeSet::new();
    names.extend(from.repos.keys());
    names.extend(to.repos.keys());

    let mut changes = Vec::new();
    for name in names {
        match (from.repos.get(name), to.repos.get(name)) {
            (None, Some(c)) => changes.push(StateChange::Added {
                name: name.clone(),
                branch: c.branch.clone(),
            }),
            (Some(h), None) => changes.push(StateChange::Removed {
                name: name.clone(),
                branch: h.branch.clone(),
            }),
            (Some(h), Some(c)) if h.branch != c.branch => changes.push(StateChange::Changed {
                name: name.clone(),
                from: h.branch.clone(),
                to: c.branch.clone(),
            }),
            // Same branch on both sides — not a change.
            (Some(_), Some(_)) | (None, None) => {}
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_state() {
        let state = StateDeclaration::parse("").unwrap();
        assert!(state.repos.is_empty());
    }

    #[test]
    fn parses_state_with_declarations() {
        let toml = r#"
            [repos."service-a"]
            branch = "feat/payment-api"
            commit = "abc123"

            [repos."service-b"]
            branch = "main"
        "#;
        let state = StateDeclaration::parse(toml).unwrap();
        assert_eq!(state.repos.len(), 2);
        assert_eq!(state.get("service-a").unwrap().branch, "feat/payment-api");
        assert_eq!(
            state.get("service-a").unwrap().commit.as_deref(),
            Some("abc123")
        );
        assert!(state.get("service-b").unwrap().commit.is_none());
    }

    #[test]
    fn round_trip_serialization() {
        let original = StateDeclaration {
            repos: [(
                "service-a".to_string(),
                RepoState {
                    branch: "main".to_string(),
                    commit: Some("abc".to_string()),
                },
            )]
            .into_iter()
            .collect(),
        };
        let serialized = original.to_toml().unwrap();
        let parsed = StateDeclaration::parse(&serialized).unwrap();
        assert_eq!(parsed.get("service-a").unwrap().branch, "main");
    }
}
