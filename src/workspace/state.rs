// State declaration parsing.
//
// state.toml declares the expected state of each child repo for the currently
// active branch of the workspace repo. Different workspace branches have
// different state.toml contents, versioned normally with Git.
//
// The state declaration is intent. It does not force reality; divergence
// between declared and actual state is normal and handled elsewhere.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

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

    /// Parse a state declaration from a string.
    pub fn parse(content: &str) -> Result<Self> {
        toml::from_str(content).context("failed to parse state declaration TOML")
    }

    /// Serialize to TOML string for writing.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize state declaration")
    }

    /// Get the declared state for a specific repo, or None if not declared.
    pub fn get(&self, repo_name: &str) -> Option<&RepoState> {
        self.repos.get(repo_name)
    }
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
