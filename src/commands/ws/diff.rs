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
use crate::workspace::state::{self, StateChange, StateDeclaration};

pub struct WsDiff {
    /// `--on <name>` from the dispatcher. When set, the change
    /// list is filtered to only entries whose repo name matches.
    /// Validated against the manifest's declared repos before
    /// running so a typo errors out clearly.
    pub on: Option<String>,
    /// `--explain` from the dispatcher — list the `git show`
    /// invocation we would run plus the comparison we would do,
    /// without doing it.
    pub explain: bool,
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

        // Validate `--on` against the manifest. We do this even
        // under `--explain` so a typo in the override surfaces
        // before the user reads the (irrelevant) plan.
        let _scope = scope::resolve(
            self.on.as_deref(),
            &manifest,
            &current,
            ctx.current_repo.as_deref(),
            ScopePolicy::full_workspace(),
        )?;

        // --explain: describe the comparison we would perform.
        if self.explain {
            let mut plan = vec![
                format!(
                    "read working-tree state at `{}/.workspace/state.toml`",
                    ctx.root.display()
                ),
                format!(
                    "git -C {} show HEAD:.workspace/state.toml",
                    ctx.root.display()
                ),
                "compare the two state declarations and emit per-repo \
                 added/removed/changed entries"
                    .to_string(),
            ];
            if self.on.is_some() {
                plan.push(format!(
                    "filter the change list to entries matching --on (`{}`)",
                    self.on.as_deref().unwrap_or("")
                ));
            }
            return Ok(WsDiffOutput {
                workspace: WorkspaceInfo {
                    root: ctx.root.to_string_lossy().into_owned(),
                    name: manifest.workspace.name.clone(),
                },
                changes: Vec::new(),
                explain_plan: Some(plan),
            });
        }

        // HEAD's state.toml via `git show`. Any failure (no commits,
        // file didn't exist at HEAD, not a git repo) collapses to
        // "empty at HEAD" — current entries then read as additions.
        let head = load_state_at_head(&ctx.root)?;

        let scope_for_filter = scope::resolve(
            self.on.as_deref(),
            &manifest,
            &current,
            ctx.current_repo.as_deref(),
            ScopePolicy::full_workspace(),
        )?;

        let mut changes = state::diff_states(&head, &current);
        if self.on.is_some() {
            changes.retain(|c| scope_for_filter.contains(&c.name().to_string()));
        }

        Ok(WsDiffOutput {
            workspace: WorkspaceInfo {
                root: ctx.root.to_string_lossy().into_owned(),
                name: manifest.workspace.name.clone(),
            },
            changes,
            explain_plan: None,
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

#[derive(Serialize)]
pub struct WsDiffOutput {
    pub workspace: WorkspaceInfo,
    pub changes: Vec<StateChange>,

    /// `Some(plan)` when `--explain` was set. Other fields are
    /// empty in that case; the renderer surfaces the plan instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub root: String,
    pub name: String,
}

impl Renderable for WsDiffOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws diff", plan);
        }

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
        assert!(state::diff_states(&head, &current).is_empty());
    }

    #[test]
    fn additions_when_repo_only_in_current() {
        let head = state_with(&[]);
        let current = state_with(&[("svc", "feat/x")]);
        let changes = state::diff_states(&head, &current);
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
        let changes = state::diff_states(&head, &current);
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
        let changes = state::diff_states(&head, &current);
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
        let changes = state::diff_states(&head, &current);
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
        let changes = state::diff_states(&head, &current);
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
