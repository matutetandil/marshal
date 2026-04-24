//! Manifest parsing and validation.
//!
//! The manifest describes the structural definition of a workspace: which repos
//! compose it, their URLs, and their affinities. It lives at
//! `.workspace/manifest.toml` and is versioned in the workspace repo.
//!
//! Changes to the manifest should be rare and deliberate, going through normal
//! Git review workflows.
//!
//! Scaffolded for Phase 2; not consumed by `main` in 0.1.0.

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A parsed manifest. This is the in-memory representation of manifest.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub workspace: WorkspaceMeta,

    #[serde(default)]
    pub repos: Vec<RepoEntry>,

    #[serde(default)]
    pub affinities: HashMap<String, RepoAffinity>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceMeta {
    pub name: String,

    #[serde(default = "default_branch")]
    pub default_branch: String,
}

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepoEntry {
    /// Logical name of the repo within this workspace.
    pub name: String,

    /// Remote URL used to clone the repo.
    pub url: String,

    /// Optional classification. Used for targeting (e.g., `on kind:service`).
    #[serde(default)]
    pub kind: Option<String>,

    /// Optional path within the workspace. Defaults to `src/<name>`.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RepoAffinity {
    #[serde(default)]
    pub depends_on: Vec<String>,

    #[serde(default)]
    pub groups: Vec<String>,
}

impl Manifest {
    /// Parse a manifest from a file path.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read manifest at {}", path.display()))?;
        Self::parse(&content)
    }

    /// Parse a manifest from a string.
    pub fn parse(content: &str) -> Result<Self> {
        let manifest: Manifest =
            toml::from_str(content).context("failed to parse manifest TOML")?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate internal consistency of the manifest.
    ///
    /// Currently checks:
    /// - No duplicate repo names
    /// - Affinity references point to existing repos
    ///
    /// TODO: validate URLs, detect cycles in depends_on, etc.
    pub fn validate(&self) -> Result<()> {
        // Check for duplicate repo names
        let mut names = std::collections::HashSet::new();
        for repo in &self.repos {
            if !names.insert(&repo.name) {
                anyhow::bail!("duplicate repo name in manifest: '{}'", repo.name);
            }
        }

        // Validate affinity references
        for (repo_name, affinity) in &self.affinities {
            if !names.contains(repo_name) {
                anyhow::bail!("affinity declared for unknown repo '{}'", repo_name);
            }
            for dep in &affinity.depends_on {
                if !names.contains(dep) {
                    anyhow::bail!("repo '{}' depends on unknown repo '{}'", repo_name, dep);
                }
            }
        }

        Ok(())
    }

    /// Look up a repo entry by name.
    pub fn find_repo(&self, name: &str) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let toml = r#"
            [workspace]
            name = "test"
        "#;
        let m = Manifest::parse(toml).unwrap();
        assert_eq!(m.workspace.name, "test");
        assert_eq!(m.workspace.default_branch, "main");
        assert!(m.repos.is_empty());
    }

    #[test]
    fn parses_full_manifest() {
        let toml = r#"
            [workspace]
            name = "my-project"
            default_branch = "develop"

            [[repos]]
            name = "service-a"
            url = "git@github.com:org/service-a.git"
            kind = "service"

            [[repos]]
            name = "shared-lib"
            url = "git@github.com:org/shared-lib.git"
            kind = "library"

            [affinities]
            "service-a" = { depends_on = ["shared-lib"] }
        "#;
        let m = Manifest::parse(toml).unwrap();
        assert_eq!(m.repos.len(), 2);
        assert_eq!(m.affinities.len(), 1);
        assert_eq!(
            m.affinities.get("service-a").unwrap().depends_on,
            vec!["shared-lib"]
        );
    }

    #[test]
    fn rejects_duplicate_repo_names() {
        let toml = r#"
            [workspace]
            name = "test"

            [[repos]]
            name = "dup"
            url = "url1"

            [[repos]]
            name = "dup"
            url = "url2"
        "#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_unknown_affinity_target() {
        let toml = r#"
            [workspace]
            name = "test"

            [[repos]]
            name = "a"
            url = "url"

            [affinities]
            "a" = { depends_on = ["nonexistent"] }
        "#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(err.to_string().contains("unknown repo"));
    }
}
