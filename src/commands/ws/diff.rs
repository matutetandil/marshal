//! `ws diff` — semantic interpretation of state.toml changes.
//!
//! Compares the working-tree `state.toml` against the version at
//! HEAD of the workspace repo and translates the difference into
//! per-repo "added / removed / changed" entries. Where `git diff`
//! would show a raw TOML hunk ("`branch = "feat/x"` replaced
//! `branch = "main"`"), `ws diff` renders the change in the
//! workspace's domain language ("`service-a`'s declared branch
//! changed from `main` to `feat/x`").
//!
//! Marshal does not reimplement plain `git diff` — for raw file
//! diffs of manifest.toml or other tracked workspace files, the
//! user can `cd` to the workspace root and run `git diff`. This
//! command focuses on the value Marshal adds: domain-aware
//! interpretation.
//!
//! Edge cases:
//!   * No HEAD (workspace repo has no commits yet): treated as
//!     "empty state at HEAD" — every entry in current `state.toml`
//!     becomes an addition.
//!   * `state.toml` did not exist at HEAD: same as above.
//!   * `state.toml` does not exist in the working tree: empty on
//!     the current side; entries from HEAD become removals.
//!   * Workspace repo is not a git repo: same as "no HEAD" —
//!     `git show` fails, current state is the only signal.
//!
//! No spatial inference yet (consistent with `ws log`). Scope
//! inference engine in Slice H.

use anyhow::{anyhow, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

use crate::cli::{Command, Renderable};
use crate::context::{self, STATE_FILE, WORKSPACE_MARKER};
use crate::workspace::manifest::Manifest;
use crate::workspace::scope::{self, ScopePolicy};
use crate::workspace::state::StateDeclaration;

pub struct WsDiff {
    /// `--on <name>` from the dispatcher. When set, the change
    /// list is filtered to only entries whose repo name matches.
    /// Validated against the manifest's declared repos before
    /// running so a typo errors out clearly.
    pub on: Option<String>,
}

impl Command for WsDiff {
    type Output = WsDiffOutput;

    fn run(&self, _args: &[OsString]) -> Result<Self::Output> {
        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here with `ws init`."
            )
        })?;

        // Manifest is required so the output can name the workspace
        // (and so we surface the "no manifest yet" case the same way
        // `ws status` and `ws log` do — consistency across read-only
        // commands).
        let manifest = Manifest::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace manifest")?
            .ok_or_else(|| {
                anyhow!(
                    "workspace at {} has no manifest yet.\n  \
                     Run `ws init` here to create one, or edit \
                     `.workspace/manifest.toml` directly.",
                    ctx.root.display()
                )
            })?;

        // Working-tree state.toml. Missing file = empty declaration
        // (every repo defaults; no entries).
        let current = StateDeclaration::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace state declaration")?
            .unwrap_or_default();

        // HEAD's state.toml via `git show`. Any failure (no commits,
        // file didn't exist at HEAD, not a git repo) collapses to
        // "empty at HEAD" — current entries then read as additions.
        let head = load_state_at_head(&ctx.root)?;

        // Validate `--on` against the manifest. We don't need
        // `infer()` here — the diff is naturally "across all
        // declared state", and `--on` filters the result list.
        let scope = scope::resolve(
            self.on.as_deref(),
            &manifest,
            &current,
            ctx.current_repo.as_deref(),
            ScopePolicy::full_workspace(),
        )?;

        let mut changes = compute_changes(&head, &current);
        if self.on.is_some() {
            changes.retain(|c| scope.contains(&c.name().to_string()));
        }

        Ok(WsDiffOutput {
            workspace: WorkspaceInfo {
                root: ctx.root.to_string_lossy().into_owned(),
                name: manifest.workspace.name.clone(),
            },
            changes,
        })
    }
}

/// Read `state.toml` as of `HEAD` in the workspace repo. Returns an
/// empty `StateDeclaration` whenever the lookup fails for any
/// reason — no HEAD, no file at HEAD, not a git repo. The diff
/// then reads as "everything in the working tree is new".
fn load_state_at_head(workspace_root: &Path) -> Result<StateDeclaration> {
    let object = format!("HEAD:{WORKSPACE_MARKER}/{STATE_FILE}");
    let output = ProcessCommand::new("git")
        .current_dir(workspace_root)
        .args(["show", &object])
        .output()
        .with_context(|| {
            format!(
                "failed to invoke `git show` in {}",
                workspace_root.display()
            )
        })?;
    if !output.status.success() {
        // Most common cases — repo has no HEAD, or the file did
        // not exist at HEAD. Both legitimate; degrade to empty.
        return Ok(StateDeclaration::default());
    }
    let content = String::from_utf8_lossy(&output.stdout);
    StateDeclaration::parse(&content).context("failed to parse the version of state.toml at HEAD")
}

/// Walk both versions, emit one [`StateChange`] per repo name that
/// differs. Sorted alphabetically by repo name so the output is
/// predictable across invocations.
fn compute_changes(head: &StateDeclaration, current: &StateDeclaration) -> Vec<StateChange> {
    use std::collections::BTreeSet;

    let mut names: BTreeSet<&String> = BTreeSet::new();
    names.extend(head.repos.keys());
    names.extend(current.repos.keys());

    let mut changes = Vec::new();
    for name in names {
        match (head.repos.get(name), current.repos.get(name)) {
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
            // Same branch on both sides — not a change. (We don't
            // diff the optional `commit` field yet; that's a
            // refinement once a use case shows up.)
            (Some(_), Some(_)) | (None, None) => {}
        }
    }
    changes
}

#[derive(Serialize)]
pub struct WsDiffOutput {
    pub workspace: WorkspaceInfo,
    pub changes: Vec<StateChange>,
}

#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub root: String,
    pub name: String,
}

/// One per-repo state change. JSON shape uses an externally-tagged
/// `kind` field (`"added"` / `"removed"` / `"changed"`) so consumers
/// can branch with a single switch.
#[derive(Serialize, Debug, PartialEq, Eq)]
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

impl Renderable for WsDiffOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(
            w,
            "Workspace `{}` — state declaration changes (working tree vs HEAD)",
            self.workspace.name
        )?;
        writeln!(w, "Root: {}", self.workspace.root)?;
        writeln!(w)?;

        if self.changes.is_empty() {
            writeln!(w, "No state declarations changed since HEAD.")?;
            writeln!(w)?;
            writeln!(
                w,
                "(For raw file diffs of manifest.toml or other workspace \
                 files, run `git diff` in the workspace root.)"
            )?;
            return Ok(());
        }

        // Pad repo names so the change descriptions align across
        // rows. The `kind` symbol is a single character followed by
        // a space; pad doesn't include it.
        let pad = self
            .changes
            .iter()
            .map(|c| c.name().len())
            .max()
            .unwrap_or(0);

        for change in &self.changes {
            match change {
                StateChange::Added { name, branch } => {
                    writeln!(w, "  + {:<pad$}  declared on `{branch}`", name, pad = pad)?;
                }
                StateChange::Removed { name, branch } => {
                    writeln!(
                        w,
                        "  - {:<pad$}  declaration removed (was `{branch}`)",
                        name,
                        pad = pad
                    )?;
                }
                StateChange::Changed { name, from, to } => {
                    writeln!(w, "  ~ {:<pad$}  `{from}` → `{to}`", name, pad = pad)?;
                }
            }
        }

        writeln!(w)?;
        writeln!(
            w,
            "(For raw file diffs of manifest.toml or other workspace files, \
             run `git diff` in the workspace root.)"
        )
    }
}

impl StateChange {
    fn name(&self) -> &str {
        match self {
            Self::Added { name, .. } => name,
            Self::Removed { name, .. } => name,
            Self::Changed { name, .. } => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::state::RepoState;
    use std::collections::HashMap;

    fn state_with(entries: &[(&str, &str)]) -> StateDeclaration {
        let repos: HashMap<String, RepoState> = entries
            .iter()
            .map(|(name, branch)| {
                (
                    (*name).to_string(),
                    RepoState {
                        branch: (*branch).to_string(),
                        commit: None,
                    },
                )
            })
            .collect();
        StateDeclaration { repos }
    }

    #[test]
    fn no_changes_yields_empty_vec() {
        let head = state_with(&[("a", "main")]);
        let current = state_with(&[("a", "main")]);
        assert!(compute_changes(&head, &current).is_empty());
    }

    #[test]
    fn additions_when_repo_only_in_current() {
        let head = state_with(&[]);
        let current = state_with(&[("svc", "feat/x")]);
        let changes = compute_changes(&head, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0],
            StateChange::Added {
                name: "svc".to_string(),
                branch: "feat/x".to_string(),
            }
        );
    }

    #[test]
    fn removals_when_repo_only_in_head() {
        let head = state_with(&[("svc", "main")]);
        let current = state_with(&[]);
        let changes = compute_changes(&head, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0],
            StateChange::Removed {
                name: "svc".to_string(),
                branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn changed_when_branch_differs() {
        let head = state_with(&[("svc", "main")]);
        let current = state_with(&[("svc", "feat/payment")]);
        let changes = compute_changes(&head, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0],
            StateChange::Changed {
                name: "svc".to_string(),
                from: "main".to_string(),
                to: "feat/payment".to_string(),
            }
        );
    }

    #[test]
    fn changes_are_sorted_alphabetically() {
        let head = state_with(&[("zeta", "main"), ("alpha", "main")]);
        let current = state_with(&[("zeta", "feat/y"), ("alpha", "feat/x")]);
        let changes = compute_changes(&head, &current);
        assert_eq!(changes.len(), 2);
        // Alpha first, then zeta, regardless of HEAD insertion order.
        match &changes[0] {
            StateChange::Changed { name, .. } => assert_eq!(name, "alpha"),
            _ => panic!("expected Changed alpha"),
        }
        match &changes[1] {
            StateChange::Changed { name, .. } => assert_eq!(name, "zeta"),
            _ => panic!("expected Changed zeta"),
        }
    }

    #[test]
    fn multiple_change_kinds_in_one_diff() {
        let head = state_with(&[("a", "main"), ("b", "main")]);
        let current = state_with(&[("a", "feat/x"), ("c", "main")]);
        // `a` changed, `b` removed, `c` added.
        let changes = compute_changes(&head, &current);
        assert_eq!(changes.len(), 3);
        // Sorted: a, b, c.
        match &changes[0] {
            StateChange::Changed { name, .. } => assert_eq!(name, "a"),
            _ => panic!("expected Changed for a"),
        }
        match &changes[1] {
            StateChange::Removed { name, .. } => assert_eq!(name, "b"),
            _ => panic!("expected Removed for b"),
        }
        match &changes[2] {
            StateChange::Added { name, .. } => assert_eq!(name, "c"),
            _ => panic!("expected Added for c"),
        }
    }
}
