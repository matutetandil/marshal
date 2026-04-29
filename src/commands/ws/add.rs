//! `ws add <repo>` — capture the current `(branch, commit)` of a
//! child repo into `staged.toml`.
//!
//! First Phase 3 command. Mirrors `git add`'s mental model:
//!
//! * **Snapshot at stage time.** Reads the child repo's branch and
//!   commit *now* and stores them. Subsequent changes to the child
//!   working tree do not propagate to the staging entry.
//! * **Re-staging refreshes the snapshot.** Running `ws add <repo>`
//!   on an already-staged repo overwrites the previous snapshot,
//!   identical to `git add` re-staging a file's current content.
//!
//! `ws commit` (later slice) flushes every staged entry into
//! `state.toml` and clears `staged.toml`. The protection that buys
//! us — atomic deploy manifests — is the whole reason staging holds
//! a snapshot rather than a flag.
//!
//! No working-tree side-effects here: we read child state, validate
//! the repo against the manifest, and write `staged.toml`. Nothing
//! else changes on disk.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::cli::{Command, Renderable};
use crate::context;
use crate::git::porcelain::RepoState;
use crate::workspace::manifest::{Manifest, RepoEntry};
use crate::workspace::staged::{RepoStaged, StagedDeclaration};

pub struct WsAdd {
    pub explain: bool,
}

impl Command for WsAdd {
    type Output = WsAddOutput;

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

        // Validate the repo exists in the manifest. Same error shape
        // as `--on bogus`: list the known names so a typo is easy to
        // fix without leaving the terminal.
        let repo = manifest.find_repo(&repo_name).ok_or_else(|| {
            let known: Vec<&str> = manifest.repos.iter().map(|r| r.name.as_str()).collect();
            let known_list = if known.is_empty() {
                "(no repos declared in this workspace yet)".to_string()
            } else {
                format!("Known: {}.", known.join(", "))
            };
            anyhow!(
                "ws add: '{repo_name}' does not match any repo declared in the manifest.\n  {known_list}"
            )
        })?;

        let abs_path = child_repo_path(&ctx.root, repo);

        // --explain: list the shellouts and the staging file write,
        // without running anything. Honours Invariant 6. Porcelain v2
        // hands us branch + HEAD oid in a single shellout, so no
        // separate `rev-parse HEAD` is needed.
        if self.explain {
            let plan = vec![
                format!("git -C {} rev-parse --git-dir", abs_path.display()),
                format!(
                    "git -C {} status --porcelain=v2 --branch  (read branch + HEAD oid)",
                    abs_path.display()
                ),
                format!(
                    "load existing `{}/.workspace/local/staged.toml` (creating the dir if absent)",
                    ctx.root.display()
                ),
                format!("upsert entry for `{repo_name}` with the captured (branch, commit)"),
                format!(
                    "write `{}/.workspace/local/staged.toml` and seed `local/.gitignore` if missing",
                    ctx.root.display()
                ),
            ];
            return Ok(WsAddOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                repo_name,
                snapshot: None,
                previous_snapshot: None,
                explain_plan: Some(plan),
            });
        }

        // Read the child's current branch + commit. A repo missing
        // from disk (declared but never cloned) errors clearly: we
        // have nothing to stage.
        if !abs_path.is_dir() {
            bail!(
                "ws add: child repo `{repo_name}` is declared but missing on disk \
                 (expected at {}). Clone the workspace with `ws clone` or check the \
                 manifest's `path` setting.",
                abs_path.display()
            );
        }

        let state = RepoState::detect_at(&abs_path).with_context(|| {
            format!(
                "ws add: failed to read state of `{repo_name}` at {}",
                abs_path.display()
            )
        })?;

        // Three degenerate cases that staging cannot represent —
        // refuse explicitly with a pointer at how to recover. Each
        // is a state that does not produce a stable
        // `(branch, commit)` pair that a deploy manifest could pin.
        // Order matters: `is_initial` and `is_detached` both leave
        // HEAD without a resolvable commit, so we check them
        // *before* trying `git rev-parse HEAD`.
        if state.branch.is_initial {
            bail!(
                "ws add: `{repo_name}` has no commits yet. \
                 Make a first commit in the child repo, then \
                 re-run `ws add`."
            );
        }
        if state.branch.is_detached {
            bail!(
                "ws add: `{repo_name}` is on detached HEAD. \
                 Switch to a branch in the child repo first \
                 (`cd {} && git switch <branch>`), then re-run \
                 `ws add`.",
                abs_path.display()
            );
        }
        let branch = state.branch.name.clone().ok_or_else(|| {
            anyhow!(
                "ws add: `{repo_name}` has no current branch \
                 (porcelain reported neither initial nor detached). \
                 This is a marshal bug; please report it."
            )
        })?;
        // Porcelain v2 already gave us the HEAD oid (`# branch.oid …`);
        // no extra shellout needed. The same `is_initial` guard above
        // means `oid` is always populated by the time we reach here.
        let commit = state.branch.oid.clone().ok_or_else(|| {
            anyhow!(
                "ws add: porcelain reported no HEAD oid for `{repo_name}`. \
                 This is a marshal bug; please report it."
            )
        })?;

        let snapshot = RepoStaged { branch, commit };

        // Load existing staging (or empty), upsert, persist.
        let mut staged = StagedDeclaration::load_or_default(&ctx.root)
            .context("failed to load existing staging declaration")?;
        let previous = staged.stage(repo_name.clone(), snapshot.clone());
        staged
            .save_to_workspace(&ctx.root)
            .context("failed to persist staging declaration")?;

        Ok(WsAddOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            repo_name,
            snapshot: Some(snapshot),
            previous_snapshot: previous,
            explain_plan: None,
        })
    }
}

#[derive(Serialize)]
pub struct WsAddOutput {
    pub root: String,
    pub repo_name: String,

    /// `Some` after a successful stage; `None` under `--explain`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<RepoStaged>,

    /// `Some` when this stage replaced a previous snapshot of the
    /// same repo (re-stage). The renderer surfaces it as
    /// "(was branch@commit)" so the user sees the replacement
    /// happen rather than a silent overwrite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_snapshot: Option<RepoStaged>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

impl Renderable for WsAddOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws add", plan);
        }
        let snapshot = match &self.snapshot {
            Some(s) => s,
            // Defensive: the only branch that yields snapshot=None
            // outside --explain would be a logic error, so fall
            // through with a generic line rather than panicking.
            None => {
                return writeln!(w, "Staged `{}`.", self.repo_name);
            }
        };
        let short = short_sha(&snapshot.commit);
        match &self.previous_snapshot {
            None => writeln!(
                w,
                "Staged `{}` at `{}`@{} ({}).",
                self.repo_name, snapshot.branch, short, snapshot.commit
            ),
            Some(prev) => writeln!(
                w,
                "Re-staged `{}` at `{}`@{} (was `{}`@{}).",
                self.repo_name,
                snapshot.branch,
                short,
                prev.branch,
                short_sha(&prev.commit)
            ),
        }
    }
}

// ── Argument parsing ──────────────────────────────────────────────

/// `ws add` takes exactly one positional `<repo>`. Multi-repo
/// staging in one invocation is a worthwhile follow-up but waits for
/// a real use case — for now, one repo per command keeps the error
/// reporting and the rendering simple.
fn parse_args(args: &[OsString]) -> Result<String> {
    let mut repo: Option<String> = None;
    for arg in args {
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws add: argument is not valid UTF-8: {:?}",
                arg.to_string_lossy()
            )
        })?;
        if s.starts_with("--") {
            bail!(
                "ws add: unexpected flag '{s}'. \
                 Expected: `ws add <repo>` (with optional --explain)."
            );
        }
        if repo.is_some() {
            bail!(
                "ws add: too many positional arguments. \
                 Stage one repo per invocation: `ws add <repo>`."
            );
        }
        repo = Some(s.to_string());
    }
    repo.ok_or_else(|| {
        anyhow!(
            "ws add: missing required <repo> argument. \
             Usage: `ws add <repo>`."
        )
    })
}

// ── Path + display helpers ────────────────────────────────────────

/// Where a child repo lives within the workspace tree. Mirrors the
/// resolution used by `ws status` and `ws clone`. Worth extracting
/// to a shared workspace helper once a fourth consumer shows up; for
/// now a three-line inline match is clearer than the indirection.
pub(super) fn child_repo_path(workspace_root: &std::path::Path, repo: &RepoEntry) -> PathBuf {
    let rel = match &repo.path {
        Some(p) if !p.is_empty() => p.clone(),
        _ => format!("src/{}", repo.name),
    };
    workspace_root.join(rel)
}

/// Truncate a sha to seven characters for the human form. Matches
/// git's default short-hash length; long-form stays available in JSON.
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
        let r = parse_args(&os(&["service-a"])).unwrap();
        assert_eq!(r, "service-a");
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

    #[test]
    fn short_sha_truncates_to_seven() {
        assert_eq!(short_sha("abc123def456"), "abc123d");
    }

    #[test]
    fn short_sha_handles_already_short_input() {
        assert_eq!(short_sha("abc"), "abc");
    }

    #[test]
    fn child_repo_path_defaults_to_src_layout() {
        let repo = RepoEntry {
            name: "alpha".to_string(),
            url: "x".to_string(),
            kind: None,
            path: None,
        };
        let p = child_repo_path(std::path::Path::new("/ws"), &repo);
        assert_eq!(p, std::path::PathBuf::from("/ws").join("src").join("alpha"));
    }

    #[test]
    fn child_repo_path_honours_explicit_path() {
        let repo = RepoEntry {
            name: "alpha".to_string(),
            url: "x".to_string(),
            kind: None,
            path: Some("services/alpha".to_string()),
        };
        let p = child_repo_path(std::path::Path::new("/ws"), &repo);
        assert_eq!(p, std::path::PathBuf::from("/ws/services/alpha"));
    }
}
