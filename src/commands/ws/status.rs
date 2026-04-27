//! `ws status` — aggregated read-only view of every child repo in
//! the workspace.
//!
//! For each repo declared in the manifest, resolves the on-disk
//! path (`<root>/<repo.path or default "src/<name>">`), invokes
//! [`crate::git::porcelain::RepoState::detect_at`] to get the
//! per-repo snapshot, and reconciles against the declared branch
//! (state.toml override or manifest default). The result is a
//! [`WsStatusOutput`] that the dispatcher renders in either the
//! human form (hide-boring; clean+on-declared collapsed into a
//! count) or JSON (full per-repo data).
//!
//! Slice E2a: state-fetcher + categorization. The hide-boring
//! rendering and the dispatch wire-up land in Slice E2b.

use anyhow::{anyhow, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;

use crate::cli::{Command, Renderable};
use crate::context;
use crate::git::porcelain::{InProgressOp, RepoState};
use crate::workspace::manifest::Manifest;
use crate::workspace::state::StateDeclaration;

/// Threshold for "show full list inline" vs "list interesting + count
/// boring". Mirrors the threshold in `commands/ws/mod.rs`. Below or
/// equal to this many repos, the human form lists every entry; above
/// it, hide-boring kicks in unless `--all` is set.
const ABBREVIATE_THRESHOLD: usize = 5;

/// `git ws status` — aggregated workspace status. The `all` field
/// encodes the global `--all` flag and toggles hide-boring vs
/// full-expansion in the renderer (Slice E2b).
pub struct WsStatus {
    pub all: bool,
}

impl Command for WsStatus {
    type Output = WsStatusOutput;

    fn run(&self, _args: &[OsString]) -> Result<Self::Output> {
        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here with `ws init`."
            )
        })?;

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

        // state.toml is optional — every repo defaults to the
        // manifest's default branch when there's no entry.
        let state = StateDeclaration::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace state declaration")?;

        let workspace = WorkspaceInfo {
            root: ctx.root.to_string_lossy().into_owned(),
            name: manifest.workspace.name.clone(),
            default_branch: manifest.workspace.default_branch.clone(),
        };

        let repos = manifest
            .repos
            .iter()
            .map(|repo| {
                let rel_path = repo
                    .path
                    .clone()
                    .unwrap_or_else(|| format!("src/{}", repo.name));
                let abs_path = ctx.root.join(&rel_path);

                // Effective declared branch = state.toml override
                // or manifest default.
                let declared_branch = state
                    .as_ref()
                    .and_then(|s| s.get(&repo.name))
                    .map(|rs| rs.branch.clone())
                    .unwrap_or_else(|| manifest.workspace.default_branch.clone());

                let (repo_state, missing_from_disk) = inspect_repo(&abs_path);
                let clean_on_declared = is_boring(&repo_state, &declared_branch);

                RepoStatus {
                    name: repo.name.clone(),
                    path: rel_path,
                    declared_branch,
                    clean_on_declared,
                    missing_from_disk,
                    state: repo_state,
                }
            })
            .collect();

        Ok(WsStatusOutput {
            workspace,
            repos,
            all: self.all,
        })
    }
}

/// Try to read the per-repo state. Returns `(state, missing)`:
/// `state` is `Some` when the path is a readable git repo, `None`
/// when reading fails for any reason; `missing` is `true` when the
/// path itself does not exist on disk (the most common reason for
/// a repo to be uninspectable in a fresh workspace where children
/// have not been cloned yet).
fn inspect_repo(abs_path: &Path) -> (Option<RepoState>, bool) {
    if !abs_path.exists() {
        return (None, true);
    }
    // Path exists but may not be a git repo, or git might fail for
    // other reasons (permissions, corrupted .git, …). Treat any
    // detect_at failure as "uninspectable" — the renderer will
    // surface the repo without a state block.
    match RepoState::detect_at(abs_path) {
        Ok(rs) => (Some(rs), false),
        Err(_) => (None, false),
    }
}

/// A repo is "boring" (clean and on declared) iff every dimension
/// matches the user's intent: branch matches, working tree clean,
/// no in-progress op, no ahead/behind. Anything else makes it
/// "interesting" and the renderer surfaces it individually.
fn is_boring(state: &Option<RepoState>, declared_branch: &str) -> bool {
    let Some(rs) = state else {
        // Missing / unreadable = interesting by definition.
        return false;
    };
    if rs.branch.is_detached || rs.branch.is_initial {
        return false;
    }
    if rs.in_progress.is_active() {
        return false;
    }
    if rs.branch.ahead != 0 || rs.branch.behind != 0 {
        return false;
    }
    if !rs.working_tree.is_clean() {
        return false;
    }
    rs.branch.name.as_deref() == Some(declared_branch)
}

#[derive(Serialize)]
pub struct WsStatusOutput {
    pub workspace: WorkspaceInfo,
    pub repos: Vec<RepoStatus>,

    /// Render-time only. Excluded from JSON: machine consumers
    /// always see the full data, the flag only affects how the
    /// human form abbreviates.
    #[serde(skip)]
    pub all: bool,
}

#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub root: String,
    pub name: String,
    pub default_branch: String,
}

#[derive(Serialize)]
pub struct RepoStatus {
    pub name: String,
    /// Path relative to the workspace root (`src/<name>` by
    /// default, or whatever the manifest's `path` field says).
    pub path: String,
    /// Effective declared branch for this repo (state.toml
    /// override or manifest default).
    pub declared_branch: String,
    /// `true` when the repo's on-disk state matches the user's
    /// declared intent in every dimension. The renderer collapses
    /// these into a single count line under hide-boring.
    pub clean_on_declared: bool,
    /// `true` when the repo's on-disk path does not exist (typical
    /// for a freshly-`ws init`-ed workspace where children have
    /// not been cloned yet — `ws clone` arrives in a later slice).
    pub missing_from_disk: bool,
    /// Detailed per-repo snapshot. `None` when the path is missing
    /// or the repo is unreadable; the renderer treats both cases
    /// as "interesting" and surfaces a one-line note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<RepoState>,
}

impl Renderable for WsStatusOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(
            w,
            "Workspace `{}` (default branch: {}) — {} repos declared",
            self.workspace.name,
            self.workspace.default_branch,
            self.repos.len()
        )?;
        writeln!(w, "Root: {}", self.workspace.root)?;
        writeln!(w)?;

        if self.repos.is_empty() {
            writeln!(w, "(No repos declared in the manifest yet.)")?;
            return Ok(());
        }

        let interesting: Vec<&RepoStatus> =
            self.repos.iter().filter(|r| !r.clean_on_declared).collect();
        let boring_count = self.repos.len() - interesting.len();
        let total = self.repos.len();
        let expand_all = self.all || total <= ABBREVIATE_THRESHOLD;

        // Choose the row set: with --all (or small N) we render every
        // repo; otherwise only the interesting ones.
        let rows: Vec<&RepoStatus> = if expand_all {
            self.repos.iter().collect()
        } else {
            interesting.clone()
        };

        if rows.is_empty() {
            // All boring + abbreviation kicked in: nothing to list,
            // just the summary.
            writeln!(w, "All {total} repos clean and on declared branch. ✓")?;
            return Ok(());
        }

        let pad = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
        for repo in &rows {
            let detail = describe_repo(repo);
            writeln!(w, "  {:<pad$}  {detail}", repo.name, pad = pad)?;
        }

        if !expand_all && boring_count > 0 {
            writeln!(w)?;
            let plural = if boring_count == 1 { "repo" } else { "repos" };
            writeln!(
                w,
                "{boring_count} other {plural} clean and on declared branch. \
                 Run with `--all` to list them."
            )?;
        }

        Ok(())
    }
}

/// One-line description of a single repo's state for the human form.
/// Composes the salient signals: declared branch, current branch (if
/// different), in-progress op, ahead/behind, working-tree counters.
fn describe_repo(repo: &RepoStatus) -> String {
    if repo.missing_from_disk {
        return format!(
            "missing on disk (declared `{}`, expected at `{}`)",
            repo.declared_branch, repo.path
        );
    }
    let Some(state) = &repo.state else {
        return format!(
            "unreadable (declared `{}`, expected at `{}`)",
            repo.declared_branch, repo.path
        );
    };

    let mut parts: Vec<String> = Vec::new();

    // Branch identity.
    if state.branch.is_initial {
        parts.push(format!("fresh repo, declared `{}`", repo.declared_branch));
    } else if state.branch.is_detached {
        parts.push(format!(
            "detached HEAD, declared `{}`",
            repo.declared_branch
        ));
    } else if let Some(current) = &state.branch.name {
        if current == &repo.declared_branch {
            parts.push(format!("on `{current}`"));
        } else {
            parts.push(format!(
                "on `{current}` (declared `{}`)",
                repo.declared_branch
            ));
        }
    } else {
        parts.push(format!("declared `{}`", repo.declared_branch));
    }

    // In-progress op.
    if let Some(label) = in_progress_label(state.in_progress) {
        parts.push(label.to_string());
    }

    // Ahead / behind.
    if state.branch.ahead > 0 || state.branch.behind > 0 {
        let mut ab = String::new();
        if state.branch.ahead > 0 {
            ab.push_str(&format!("↑{}", state.branch.ahead));
        }
        if state.branch.behind > 0 {
            if !ab.is_empty() {
                ab.push(' ');
            }
            ab.push_str(&format!("↓{}", state.branch.behind));
        }
        parts.push(ab);
    }

    // Working-tree counters.
    let wt = &state.working_tree;
    let mut wt_parts: Vec<String> = Vec::new();
    if wt.unmerged > 0 {
        wt_parts.push(format!("{} conflict{}", wt.unmerged, plural_s(wt.unmerged)));
    }
    if wt.staged > 0 {
        wt_parts.push(format!("{} staged", wt.staged));
    }
    if wt.unstaged > 0 {
        wt_parts.push(format!("{} unstaged", wt.unstaged));
    }
    if wt.untracked > 0 {
        wt_parts.push(format!("{} untracked", wt.untracked));
    }
    if !wt_parts.is_empty() {
        parts.push(wt_parts.join(", "));
    }

    if parts.is_empty() {
        "clean".to_string()
    } else {
        parts.join(" — ")
    }
}

fn in_progress_label(op: InProgressOp) -> Option<&'static str> {
    match op {
        InProgressOp::None => None,
        InProgressOp::Merge => Some("merge in progress"),
        InProgressOp::Rebase => Some("rebase in progress"),
        InProgressOp::CherryPick => Some("cherry-pick in progress"),
        InProgressOp::Revert => Some("revert in progress"),
        InProgressOp::Bisect => Some("bisect in progress"),
    }
}

fn plural_s(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::{BranchInfo, InProgressOp, WorkingTreeInfo};

    fn clean_state_on(branch: &str) -> RepoState {
        RepoState {
            branch: BranchInfo {
                name: Some(branch.to_string()),
                upstream: Some(format!("origin/{branch}")),
                ahead: 0,
                behind: 0,
                ..Default::default()
            },
            working_tree: WorkingTreeInfo::default(),
            in_progress: InProgressOp::None,
        }
    }

    #[test]
    fn boring_when_clean_on_declared_branch() {
        let s = Some(clean_state_on("main"));
        assert!(is_boring(&s, "main"));
    }

    #[test]
    fn interesting_when_on_wrong_branch() {
        let s = Some(clean_state_on("feat/x"));
        assert!(!is_boring(&s, "main"));
    }

    #[test]
    fn interesting_when_dirty() {
        let mut rs = clean_state_on("main");
        rs.working_tree.unstaged = 1;
        assert!(!is_boring(&Some(rs), "main"));
    }

    #[test]
    fn interesting_when_in_progress() {
        let mut rs = clean_state_on("main");
        rs.in_progress = InProgressOp::Rebase;
        assert!(!is_boring(&Some(rs), "main"));
    }

    #[test]
    fn interesting_when_ahead_or_behind() {
        let mut rs = clean_state_on("main");
        rs.branch.ahead = 1;
        assert!(!is_boring(&Some(rs), "main"));

        let mut rs2 = clean_state_on("main");
        rs2.branch.behind = 1;
        assert!(!is_boring(&Some(rs2), "main"));
    }

    #[test]
    fn interesting_when_detached_or_initial() {
        let mut rs = clean_state_on("main");
        rs.branch.is_detached = true;
        rs.branch.name = None;
        assert!(!is_boring(&Some(rs), "main"));

        let mut rs2 = clean_state_on("main");
        rs2.branch.is_initial = true;
        assert!(!is_boring(&Some(rs2), "main"));
    }

    #[test]
    fn interesting_when_state_is_missing() {
        // No state at all = repo isn't on disk or is unreadable.
        // Always interesting — the user needs to know.
        assert!(!is_boring(&None, "main"));
    }
}
