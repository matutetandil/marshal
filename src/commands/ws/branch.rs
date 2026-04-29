//! `ws branch <name>` — create a new workspace branch.
//!
//! The workspace-level analogue of `git branch <name>`. Creates a
//! new branch in the workspace-repo from current HEAD. The new
//! branch's `state.toml` starts as a copy of the parent branch's
//! (because `git branch` copies the tree); the user populates the
//! per-child mapping later via the existing `ws add` / `ws commit`
//! workflow on the new branch (after `ws switch`).
//!
//! **Thin by design.** A workspace branch is a git branch on the
//! workspace-repo with its own versioned `state.toml`. Different
//! workspace branches map to arbitrary per-child branch combos —
//! one branch can have alpha on `main` + beta on `feat/payments`,
//! another can have alpha on `release/2.3` + beta on `release/2.4`.
//! This command does not pre-decide what those mappings should be;
//! it just makes a new workspace branch available. The user then
//! switches to it (Slice G) and uses `ws add` / `ws commit` to
//! declare the mapping that this workspace branch should record.
//!
//! A richer "granular scope" variant (auto-detect divergent
//! children, create branches in them, record in state.toml) is
//! described in the user's branching brief and lives in Slice F.5
//! — it depends on the parallel-execution framework arriving in
//! Slice H.
//!
//! `--on <name>` and any positional beyond `<name>` are rejected.
//! `ws branch` is a workspace-repo operation; per-child branching
//! is determined by state.toml content, not by this command's
//! flags.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

use crate::cli::{Command, Renderable};
use crate::context;
use crate::git::porcelain::RepoState;
use crate::workspace::manifest::Manifest;

pub struct WsBranch {
    pub on: Option<String>,
    pub explain: bool,
}

impl Command for WsBranch {
    type Output = WsBranchOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        if let Some(on_name) = &self.on {
            bail!(
                "ws branch: takes no `--on` filter. \
                 `ws branch` operates on the workspace-repo only — per-child \
                 branching is determined by state.toml content on each \
                 workspace branch. To create a child branch directly: \
                 `cd src/{on_name}` and use plain git."
            );
        }
        let name = parse_args(args)?;

        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here with `ws init`."
            )
        })?;

        // We only need the manifest to populate `workspace_name` in
        // the output — the operation does not consume it. Loading
        // it also asserts the workspace is at least minimally
        // initialised (consistent with the other ws commands).
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

        if self.explain {
            let plan = vec![
                format!(
                    "git -C {root} status --porcelain=v2 --branch  \
                     (read workspace HEAD for `from_branch` / `from_commit`)",
                    root = ctx.root.display()
                ),
                "abort if the workspace-repo is initial-empty (nothing to branch from)".to_string(),
                format!(
                    "git -C {root} branch {name}  \
                     (creates `{name}` at workspace HEAD; does not switch)",
                    root = ctx.root.display()
                ),
            ];
            return Ok(WsBranchOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                workspace_name: manifest.workspace.name.clone(),
                branch_name: name,
                from_branch: None,
                from_commit: None,
                explain_plan: Some(plan),
            });
        }

        // Read the workspace's current state. We need it for two
        // reasons: (1) populate `from_branch` / `from_commit` in the
        // output; (2) refuse cleanly if the repo has no commits
        // (initial-empty), with a hint clearer than git's terse
        // "fatal: not a valid object name" failure.
        let state = RepoState::detect_at(&ctx.root).with_context(|| {
            format!(
                "ws branch: failed to read workspace-repo state at {}",
                ctx.root.display()
            )
        })?;
        if state.branch.is_initial {
            bail!(
                "ws branch: workspace-repo at {} has no commits yet — \
                 there is nothing to branch from. \
                 Make a first commit (e.g. `git -C {} commit ...`) and re-run.",
                ctx.root.display(),
                ctx.root.display()
            );
        }
        let from_branch = state.branch.name.clone();
        let from_commit = state.branch.oid.clone();

        // git branch <name>: creates the branch from HEAD without
        // switching, exactly like plain `git branch <name>`. Errors
        // (branch exists, invalid name, etc) propagate verbatim from
        // git's stderr so the user gets the same diagnostic they
        // would running git directly.
        run_git(&ctx.root, &["branch", &name], &format!("git branch {name}"))?;

        Ok(WsBranchOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            workspace_name: manifest.workspace.name,
            branch_name: name,
            from_branch,
            from_commit,
            explain_plan: None,
        })
    }
}

#[derive(Serialize)]
pub struct WsBranchOutput {
    pub root: String,
    pub workspace_name: String,
    pub branch_name: String,
    /// Workspace branch the new one was created from. `None` when
    /// HEAD was detached at branch-creation time (`git branch`
    /// still works from detached, branching off the commit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,
    /// Workspace HEAD commit at branch-creation time. `None` under
    /// `--explain`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

impl Renderable for WsBranchOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws branch", plan);
        }
        let from_label = self
            .from_branch
            .as_deref()
            .map(|b| format!("`{b}`"))
            .unwrap_or_else(|| "(detached HEAD)".to_string());
        let sha = self
            .from_commit
            .as_deref()
            .map(short_sha)
            .unwrap_or("??????");
        writeln!(
            w,
            "Created workspace branch `{}` from {} ({}).",
            self.branch_name, from_label, sha
        )?;
        writeln!(w)?;
        writeln!(
            w,
            "Switch to it with `ws switch {}` to materialise its state across child repos.",
            self.branch_name
        )
    }
}

// ── Argument parsing ──────────────────────────────────────────────

fn parse_args(args: &[OsString]) -> Result<String> {
    let mut name: Option<String> = None;
    for arg in args {
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws branch: argument is not valid UTF-8: {:?}",
                arg.to_string_lossy()
            )
        })?;
        if s.starts_with("--") || s.starts_with('-') {
            bail!(
                "ws branch: unexpected flag '{s}'. \
                 Expected: `ws branch <name>` (with optional --explain)."
            );
        }
        if name.is_some() {
            bail!(
                "ws branch: too many positional arguments. \
                 `ws branch` takes exactly one branch name: `ws branch <name>`."
            );
        }
        name = Some(s.to_string());
    }
    name.ok_or_else(|| {
        anyhow!(
            "ws branch: missing required <name> argument. \
             Usage: `ws branch <name>`."
        )
    })
}

// ── Helpers ───────────────────────────────────────────────────────

fn run_git(repo_path: &Path, args: &[&str], label: &str) -> Result<()> {
    let out = ProcessCommand::new("git")
        .current_dir(repo_path)
        .args(args)
        .output()
        .with_context(|| format!("failed to invoke `{label}` in {}", repo_path.display()))?;
    if !out.status.success() {
        let code = out
            .status
            .code()
            .map(|c| format!("{c}"))
            .unwrap_or_else(|| "?".to_string());
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!(
            "ws branch: `{label}` exited with status {code} in {}: {}",
            repo_path.display(),
            stderr.trim()
        );
    }
    Ok(())
}

fn short_sha(sha: &str) -> &str {
    let n = sha.len().min(7);
    &sha[..n]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn parse_args_accepts_one_name() {
        let n = parse_args(&os(&["feature/payments"])).unwrap();
        assert_eq!(n, "feature/payments");
    }

    #[test]
    fn parse_args_rejects_missing() {
        let err = parse_args(&[]).unwrap_err();
        assert!(err.to_string().contains("missing required <name>"));
    }

    #[test]
    fn parse_args_rejects_two_positionals() {
        let err = parse_args(&os(&["a", "b"])).unwrap_err();
        assert!(err.to_string().contains("too many positional"));
    }

    #[test]
    fn parse_args_rejects_unknown_flag() {
        let err = parse_args(&os(&["--bogus"])).unwrap_err();
        assert!(err.to_string().contains("unexpected flag"));
    }

    #[test]
    fn short_sha_truncates_to_seven() {
        assert_eq!(short_sha("abcdef1234567890"), "abcdef1");
    }

    #[test]
    fn short_sha_handles_short_input() {
        assert_eq!(short_sha("abc"), "abc");
    }
}
