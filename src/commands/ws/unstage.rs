//! `ws unstage <repo>` — drop a previously-staged repo's snapshot
//! from `staged.toml`.
//!
//! Counterpart to `ws add`: it never touches the child repo's
//! working tree, only the per-developer staging file. Idempotent —
//! unstaging a repo that was never staged succeeds with a "nothing
//! to do" output rather than erroring, mirroring how `git restore
//! --staged <unstaged-path>` behaves benignly when there's nothing
//! to undo.
//!
//! Validates the repo against the manifest with the same shape as
//! `ws add`: a typo errors out cleanly with the list of known
//! names. The validation is a usability call, not a correctness
//! one — unstaging an unknown repo is technically harmless (the
//! key would simply not be in `staged.toml`) but a typo here almost
//! always means the user is confused, and we should say so.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};

use crate::cli::{Command, Renderable};
use crate::context;
use crate::workspace::manifest::Manifest;
use crate::workspace::staged::{RepoStaged, StagedDeclaration};

pub struct WsUnstage {
    pub explain: bool,
}

impl Command for WsUnstage {
    type Output = WsUnstageOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let repo_name = parse_args(args)?;

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

        if manifest.find_repo(&repo_name).is_none() {
            let known: Vec<&str> = manifest.repos.iter().map(|r| r.name.as_str()).collect();
            let known_list = if known.is_empty() {
                "(no repos declared in this workspace yet)".to_string()
            } else {
                format!("Known: {}.", known.join(", "))
            };
            bail!(
                "ws unstage: '{repo_name}' does not match any repo declared in the manifest.\n  {known_list}"
            );
        }

        if self.explain {
            let plan = vec![
                format!(
                    "load `{}/.workspace/local/staged.toml` (treat missing as empty)",
                    ctx.root.display()
                ),
                format!("remove the entry for `{repo_name}`, if present"),
                format!(
                    "write `{}/.workspace/local/staged.toml` (header + body, even when body is empty)",
                    ctx.root.display()
                ),
            ];
            return Ok(WsUnstageOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                repo_name,
                removed: None,
                was_staged: false,
                explain_plan: Some(plan),
            });
        }

        let mut staged = StagedDeclaration::load_or_default(&ctx.root)
            .context("failed to load existing staging declaration")?;
        let removed = staged.unstage(&repo_name);

        // Only persist if we actually mutated. Keeps the staging
        // file's mtime stable when unstaging a repo that was not
        // staged — small touch, but it makes "did anything change?"
        // observable from the filesystem.
        if removed.is_some() {
            staged
                .save_to_workspace(&ctx.root)
                .context("failed to persist staging declaration")?;
        }

        Ok(WsUnstageOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            repo_name,
            was_staged: removed.is_some(),
            removed,
            explain_plan: None,
        })
    }
}

#[derive(Serialize)]
pub struct WsUnstageOutput {
    pub root: String,
    pub repo_name: String,
    /// `true` when the repo had a staging entry that we just
    /// removed. `false` when it was already not staged (no-op).
    /// Distinct from `removed.is_some()` only for the explain path,
    /// where both are unset.
    pub was_staged: bool,
    /// The snapshot that was just removed, if any. Lets JSON
    /// consumers see exactly what the user dropped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<RepoStaged>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

impl Renderable for WsUnstageOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws unstage", plan);
        }
        match &self.removed {
            Some(prev) => writeln!(
                w,
                "Unstaged `{}` (was `{}`@{}).",
                self.repo_name,
                prev.branch,
                short_sha(&prev.commit)
            ),
            None => writeln!(w, "`{}` was not staged — nothing to do.", self.repo_name),
        }
    }
}

// ── Argument parsing ──────────────────────────────────────────────

fn parse_args(args: &[OsString]) -> Result<String> {
    let mut repo: Option<String> = None;
    for arg in args {
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws unstage: argument is not valid UTF-8: {:?}",
                arg.to_string_lossy()
            )
        })?;
        if s.starts_with("--") {
            bail!(
                "ws unstage: unexpected flag '{s}'. \
                 Expected: `ws unstage <repo>` (with optional --explain)."
            );
        }
        if repo.is_some() {
            bail!(
                "ws unstage: too many positional arguments. \
                 Unstage one repo per invocation: `ws unstage <repo>`."
            );
        }
        repo = Some(s.to_string());
    }
    repo.ok_or_else(|| {
        anyhow!(
            "ws unstage: missing required <repo> argument. \
             Usage: `ws unstage <repo>`."
        )
    })
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
    fn parse_args_accepts_one_repo() {
        assert_eq!(parse_args(&os(&["alpha"])).unwrap(), "alpha");
    }

    #[test]
    fn parse_args_rejects_missing() {
        let err = parse_args(&[]).unwrap_err();
        assert!(err.to_string().contains("missing required <repo>"));
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
}
