// Scope inference.
//
// Operations apply to a set of child repos. Which repos? That's the scope.
// The scope is inferred from context unless explicitly overridden with --on.
//
// Each operation has a declared "scope policy" describing how to infer its
// default scope. This is part of the design, not implementation detail.
//
// See ARCHITECTURE.md § "Scope Inference" for the conceptual model.

use crate::workspace::manifest::Manifest;

/// The dimensions along which scope can be inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    /// Where the developer's current directory is.
    Spatial,

    /// Which repos have actual file modifications.
    Material,

    /// What the current workspace branch declares.
    Temporal,

    /// Affinities from the manifest.
    Structural,
}

/// The scope policy for a specific operation.
#[derive(Debug, Clone)]
pub struct ScopePolicy {
    /// Primary dimensions used to infer scope.
    pub dimensions: Vec<Dimension>,

    /// Whether the policy is restrictive (more dimensions narrow the scope)
    /// or permissive (any matching dimension includes the repo).
    pub restrictive: bool,
}

impl ScopePolicy {
    /// Policy for `status`, `fetch`: always full workspace.
    pub fn full_workspace() -> Self {
        Self {
            dimensions: vec![],
            restrictive: false,
        }
    }

    /// Policy for `log`, `diff`: spatial (workspace if at root, repo if inside).
    pub fn spatial_fallback() -> Self {
        Self {
            dimensions: vec![Dimension::Spatial],
            restrictive: true,
        }
    }

    /// Policy for `commit`: material, limited by spatial.
    pub fn material_limited_by_spatial() -> Self {
        Self {
            dimensions: vec![Dimension::Material, Dimension::Spatial],
            restrictive: true,
        }
    }

    /// Policy for `switch`: temporal (what the branch declares).
    pub fn temporal() -> Self {
        Self {
            dimensions: vec![Dimension::Temporal],
            restrictive: false,
        }
    }

    /// Policy for `push`: material and temporal (repos with unpushed commits).
    pub fn material_and_temporal() -> Self {
        Self {
            dimensions: vec![Dimension::Material, Dimension::Temporal],
            restrictive: true,
        }
    }

    /// Policy for `pull`: full workspace with structural ordering.
    pub fn full_with_structural_ordering() -> Self {
        Self {
            dimensions: vec![Dimension::Structural],
            restrictive: false,
        }
    }

    /// Policy for `test`: material plus structural dependents.
    pub fn material_plus_dependents() -> Self {
        Self {
            dimensions: vec![Dimension::Material, Dimension::Structural],
            restrictive: false,
        }
    }
}

/// Inputs available to scope inference.
pub struct InferenceContext<'a> {
    pub manifest: &'a Manifest,
    pub current_repo: Option<&'a str>,
    pub dirty_repos: &'a [String],
    pub declared_state: &'a crate::workspace::state::StateDeclaration,
}

/// Infer the scope of an operation given its policy and context.
///
/// Returns the names of repos that should be included in the operation.
pub fn infer(policy: &ScopePolicy, ctx: &InferenceContext) -> Vec<String> {
    // If the policy has no dimensions, it's "full workspace": include all repos.
    if policy.dimensions.is_empty() {
        return ctx.manifest.repos.iter().map(|r| r.name.clone()).collect();
    }

    let mut candidates: Vec<String> =
        ctx.manifest.repos.iter().map(|r| r.name.clone()).collect();

    for dim in &policy.dimensions {
        candidates = apply_dimension(*dim, candidates, ctx);
    }

    candidates
}

fn apply_dimension(
    dim: Dimension,
    candidates: Vec<String>,
    ctx: &InferenceContext,
) -> Vec<String> {
    match dim {
        Dimension::Spatial => {
            // If we're inside a specific repo, restrict to just that repo.
            // Otherwise, leave candidates unchanged.
            if let Some(repo) = ctx.current_repo {
                candidates.into_iter().filter(|r| r == repo).collect()
            } else {
                candidates
            }
        }
        Dimension::Material => {
            // Keep only repos that have modifications.
            candidates
                .into_iter()
                .filter(|r| ctx.dirty_repos.iter().any(|d| d == r))
                .collect()
        }
        Dimension::Temporal => {
            // Keep only repos that are declared in the state (non-default state).
            candidates
                .into_iter()
                .filter(|r| ctx.declared_state.repos.contains_key(r))
                .collect()
        }
        Dimension::Structural => {
            // Expand candidates to include repos that depend on any candidate.
            let mut expanded = candidates.clone();
            for candidate in &candidates {
                for (dependent, affinity) in &ctx.manifest.affinities {
                    if affinity.depends_on.iter().any(|d| d == candidate)
                        && !expanded.contains(dependent)
                    {
                        expanded.push(dependent.clone());
                    }
                }
            }
            expanded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::manifest::{RepoEntry, WorkspaceMeta};
    use crate::workspace::state::StateDeclaration;

    fn sample_manifest() -> Manifest {
        Manifest {
            workspace: WorkspaceMeta {
                name: "test".to_string(),
                default_branch: "main".to_string(),
            },
            repos: vec![
                RepoEntry {
                    name: "service-a".to_string(),
                    url: "url-a".to_string(),
                    kind: None,
                    path: None,
                },
                RepoEntry {
                    name: "service-b".to_string(),
                    url: "url-b".to_string(),
                    kind: None,
                    path: None,
                },
                RepoEntry {
                    name: "shared-lib".to_string(),
                    url: "url-shared".to_string(),
                    kind: None,
                    path: None,
                },
            ],
            affinities: Default::default(),
        }
    }

    #[test]
    fn full_workspace_returns_all_repos() {
        let manifest = sample_manifest();
        let state = StateDeclaration::default();
        let ctx = InferenceContext {
            manifest: &manifest,
            current_repo: None,
            dirty_repos: &[],
            declared_state: &state,
        };
        let policy = ScopePolicy::full_workspace();
        let scope = infer(&policy, &ctx);
        assert_eq!(scope.len(), 3);
    }

    #[test]
    fn spatial_dimension_restricts_to_current_repo() {
        let manifest = sample_manifest();
        let state = StateDeclaration::default();
        let ctx = InferenceContext {
            manifest: &manifest,
            current_repo: Some("service-a"),
            dirty_repos: &[],
            declared_state: &state,
        };
        let policy = ScopePolicy::spatial_fallback();
        let scope = infer(&policy, &ctx);
        assert_eq!(scope, vec!["service-a".to_string()]);
    }

    #[test]
    fn material_dimension_filters_to_dirty_repos() {
        let manifest = sample_manifest();
        let state = StateDeclaration::default();
        let dirty = vec!["service-a".to_string(), "shared-lib".to_string()];
        let ctx = InferenceContext {
            manifest: &manifest,
            current_repo: None,
            dirty_repos: &dirty,
            declared_state: &state,
        };
        let policy = ScopePolicy::material_limited_by_spatial();
        let scope = infer(&policy, &ctx);
        assert_eq!(scope.len(), 2);
        assert!(scope.contains(&"service-a".to_string()));
        assert!(scope.contains(&"shared-lib".to_string()));
    }
}
