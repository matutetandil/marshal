//! `ws restore <repo>` — bring a child repo back to the
//! workspace's declared state.
//!
//! First Phase 3 command that **writes to a child repo's working
//! tree**. Switches the named child to the branch declared for it
//! (state.toml override or manifest default).
//!
//! Pre-flight gates the operation:
//!
//! * **Hard blockers** (in-progress merge/rebase/cherry-pick/revert/
//!   bisect, working-tree conflicts, initial-empty repo): refused
//!   unconditionally — no flag at this layer can resolve them.
//!   The error explains how to clear each one before re-running.
//! * **Soft blockers** (staged / unstaged / untracked changes):
//!   refused by default (Invariant 8 — Conservative Defaults). The
//!   user opts in to one of two resolutions:
//!     - `--auto-stash`  →  `git stash push --include-untracked`
//!       (preserves the work; stash listed for later `git stash
//!       pop`).
//!     - `--discard-changes`  →  `git reset --hard` + `git clean
//!       -fd` (destroys uncommitted work — explicit opt-in).
//!
//! `--auto-stash` and `--discard-changes` are mutually exclusive
//! (the user has to pick a side). `--explain` prints the plan
//! without doing anything, including which obstacle resolution
//! step would run if the working tree is dirty.
//!
//! Single-repo only this slice. The multi-repo `ws restore --all`
//! variant waits for the parallel-execution framework in Slice H.
//! `--on <name>` — the namespace's declared-scope override — is
//! redundant when there's already a positional `<repo>`, so we
//! refuse it explicitly with a hint at the canonical form.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use crate::cli::{Command, Renderable};
use crate::context;
use crate::git::porcelain::RepoState;
use crate::workspace::manifest::{Manifest, RepoEntry};
use crate::workspace::preflight::{self, Obstacle};
use crate::workspace::state::StateDeclaration;

pub struct WsRestore {
    pub on: Option<String>,
    pub explain: bool,
}

impl Command for WsRestore {
    type Output = WsRestoreOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        // The positional `<repo>` and `--on <name>` overlap in
        // intent. Refuse before parsing args — otherwise a user who
        // typed `ws restore --on alpha` (no positional) gets the
        // less-helpful "missing <repo>" error before they see the
        // pointer at the canonical form.
        if let Some(on_name) = &self.on {
            bail!(
                "ws restore: takes a positional <repo>, not `--on`. \
                 Try `ws restore {on_name}` instead."
            );
        }
        let parsed = parse_args(args)?;

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

        let repo = manifest.find_repo(&parsed.repo_name).ok_or_else(|| {
            let known: Vec<&str> = manifest.repos.iter().map(|r| r.name.as_str()).collect();
            let known_list = if known.is_empty() {
                "(no repos declared in this workspace yet)".to_string()
            } else {
                format!("Known: {}.", known.join(", "))
            };
            anyhow!(
                "ws restore: '{}' does not match any repo declared in the manifest.\n  {known_list}",
                parsed.repo_name
            )
        })?;

        let abs_path = child_repo_path(&ctx.root, repo);

        let state = StateDeclaration::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace state declaration")?;
        let declared_branch = state
            .as_ref()
            .and_then(|s| s.get(&parsed.repo_name))
            .map(|rs| rs.branch.clone())
            .unwrap_or_else(|| manifest.workspace.default_branch.clone());

        // --explain: describe the plan without running it. The
        // plan is conservative — we cannot read the working tree
        // under --explain (that would be a side-effect-shaped
        // shellout that the user might not want), so we say the
        // pre-flight will run + the resolution step that *would*
        // execute given the requested flags.
        if self.explain {
            let plan = build_explain_plan(
                &abs_path,
                &declared_branch,
                parsed.auto_stash,
                parsed.discard,
            );
            return Ok(WsRestoreOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                repo_name: parsed.repo_name,
                path: abs_path.to_string_lossy().into_owned(),
                declared_branch,
                from_branch: None,
                from_commit: None,
                to_branch: String::new(),
                to_commit: None,
                stashed: false,
                discarded: false,
                obstacles: Vec::new(),
                explain_plan: Some(plan),
            });
        }

        if !abs_path.is_dir() {
            bail!(
                "ws restore: child repo `{}` is missing on disk \
                 (declared, expected at {}). \
                 Clone the workspace with `ws clone` first.",
                parsed.repo_name,
                abs_path.display()
            );
        }

        // Capture pre-restore state.
        let before = RepoState::detect_at(&abs_path).with_context(|| {
            format!(
                "ws restore: failed to read state of `{}` at {}",
                parsed.repo_name,
                abs_path.display()
            )
        })?;

        let from_branch = before.branch.name.clone();
        let from_commit = before.branch.oid.clone();

        // Pre-flight: collect every obstacle, then decide whether
        // any are blocking given the user's flags.
        let obs = preflight::obstacles(&before);
        let blocking = blocking_obstacles(&obs, parsed.auto_stash, parsed.discard);
        if !blocking.is_empty() {
            bail!(
                "{}",
                format_blocking_error(&parsed.repo_name, &abs_path, &blocking, &obs)
            );
        }

        // Resolve soft blockers under --auto-stash / --discard. Hard
        // blockers were caught above. The presence of any soft
        // blocker that survived pre-flight means a resolution flag
        // is set and there's actually something to resolve.
        let needs_resolution = obs.iter().any(|o| !o.is_hard_blocker());

        let mut stashed = false;
        let mut discarded = false;
        if needs_resolution && parsed.discard {
            run_git(&abs_path, &["reset", "--hard"], "git reset --hard")?;
            run_git(&abs_path, &["clean", "-fd"], "git clean -fd")?;
            discarded = true;
        } else if needs_resolution && parsed.auto_stash {
            // Build a stash message that identifies the operation
            // and the repo. Useful when listing later.
            let msg = format!(
                "marshal/ws-restore: stashed before switching `{}` to `{}`",
                parsed.repo_name, declared_branch
            );
            run_git(
                &abs_path,
                &["stash", "push", "--include-untracked", "-m", &msg],
                "git stash push",
            )?;
            stashed = true;
        }

        // Switch to the declared branch. `git switch` (Git ≥ 2.23)
        // is the modern surface; if the branch does not exist
        // locally but tracks origin/<branch>, git creates it
        // automatically with `--guess` (the default). We rely on
        // that — if neither exists, switch errors and we surface
        // its message verbatim.
        run_git(
            &abs_path,
            &["switch", &declared_branch],
            &format!("git switch {declared_branch}"),
        )?;

        let after = RepoState::detect_at(&abs_path).with_context(|| {
            format!(
                "ws restore: failed to re-read state of `{}` after switch",
                parsed.repo_name
            )
        })?;

        Ok(WsRestoreOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            repo_name: parsed.repo_name,
            path: abs_path.to_string_lossy().into_owned(),
            declared_branch: declared_branch.clone(),
            from_branch,
            from_commit,
            to_branch: after.branch.name.unwrap_or(declared_branch),
            to_commit: after.branch.oid,
            stashed,
            discarded,
            obstacles: obs,
            explain_plan: None,
        })
    }
}

// ── Pre-flight evaluation ─────────────────────────────────────────

/// Filter the obstacle list to those that *block* given the user's
/// resolution flags. Hard blockers always block. Soft blockers
/// block unless one of the resolution flags addresses them.
fn blocking_obstacles(obstacles: &[Obstacle], auto_stash: bool, discard: bool) -> Vec<&Obstacle> {
    obstacles
        .iter()
        .filter(|o| {
            if o.is_hard_blocker() {
                return true;
            }
            // Soft blocker: clears under either resolution flag.
            !(auto_stash && o.cleared_by_auto_stash() || discard && o.cleared_by_discard())
        })
        .collect()
}

fn format_blocking_error(
    repo_name: &str,
    abs_path: &Path,
    blocking: &[&Obstacle],
    all_obs: &[Obstacle],
) -> String {
    let mut msg = format!(
        "ws restore: cannot restore `{repo_name}` — working state has issues that block the switch.\n\n"
    );
    msg.push_str("Detected:\n");
    for o in all_obs {
        let marker = if blocking.contains(&o) {
            "  ✗"
        } else {
            "  ✓"
        };
        msg.push_str(&format!("{marker} {}\n", o.description()));
    }
    msg.push('\n');

    // Tailor the suggestions: if every blocker is soft, point at
    // the resolution flags. If any is hard, point at the manual
    // recovery path.
    let any_hard = blocking.iter().any(|o| o.is_hard_blocker());
    let any_soft = blocking.iter().any(|o| !o.is_hard_blocker());

    if any_hard {
        msg.push_str(&format!(
            "Resolve manually in the child repo first ({}):\n",
            abs_path.display()
        ));
        msg.push_str(
            "  - For an in-progress op: complete it (`git rebase --continue`/etc) \
             or abort it (`git rebase --abort`/etc).\n",
        );
        msg.push_str(
            "  - For unresolved conflicts: edit the conflicted files, \
             then `git add` + complete the operation.\n",
        );
        msg.push_str("  - For an empty repo: make a first commit before restoring.\n");
    }
    if any_soft {
        if any_hard {
            msg.push('\n');
        }
        msg.push_str("For uncommitted local changes, choose one:\n");
        msg.push_str(
            "  - `ws restore <repo> --auto-stash`     stashes everything (recoverable via \
             `git stash pop`).\n",
        );
        msg.push_str(
            "  - `ws restore <repo> --discard-changes` resets and cleans \
             (destructive — opt-in).\n",
        );
        msg.push_str("  - Or commit/stash by hand inside the child first.\n");
    }
    msg
}

// ── --explain plan ────────────────────────────────────────────────

fn build_explain_plan(
    abs_path: &Path,
    declared_branch: &str,
    auto_stash: bool,
    discard: bool,
) -> Vec<String> {
    let mut plan = vec![
        format!(
            "git -C {} status --porcelain=v2 --branch  (read working state for pre-flight)",
            abs_path.display()
        ),
        "evaluate pre-flight obstacles (in-progress op, conflicts, initial-empty, \
         staged/unstaged/untracked changes)"
            .to_string(),
        "abort if any blocking obstacle remains given the requested flags".to_string(),
    ];
    if discard {
        plan.push(format!(
            "if the working tree is dirty: \
             git -C {p} reset --hard  +  git -C {p} clean -fd  (--discard-changes)",
            p = abs_path.display()
        ));
    } else if auto_stash {
        plan.push(format!(
            "if the working tree is dirty: \
             git -C {p} stash push --include-untracked -m \"marshal/ws-restore: …\"  (--auto-stash)",
            p = abs_path.display()
        ));
    }
    plan.push(format!(
        "git -C {} switch {}",
        abs_path.display(),
        declared_branch
    ));
    plan
}

// ── Output ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WsRestoreOutput {
    pub root: String,
    pub repo_name: String,
    pub path: String,
    pub declared_branch: String,

    /// Branch the repo was on before the switch. `None` if it was
    /// detached, initial-empty, or unreadable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,

    /// HEAD oid before the switch. Present whenever a commit
    /// existed (so always except `initial-empty`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_commit: Option<String>,

    /// Branch the repo is on after the switch — typically equals
    /// `declared_branch`. Empty under `--explain`.
    pub to_branch: String,

    /// HEAD oid after the switch. Empty / `None` under `--explain`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_commit: Option<String>,

    /// `true` when `--auto-stash` actually pushed a stash.
    pub stashed: bool,

    /// `true` when `--discard-changes` ran reset + clean.
    pub discarded: bool,

    /// Every obstacle the pre-flight detected, blocking or not.
    /// Empty under `--explain`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub obstacles: Vec<Obstacle>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

impl Renderable for WsRestoreOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws restore", plan);
        }

        let from = self
            .from_branch
            .as_deref()
            .map(|b| format!("`{b}`"))
            .unwrap_or_else(|| "(no branch)".to_string());

        if self.from_branch.as_deref() == Some(&self.to_branch) {
            writeln!(
                w,
                "`{}` already on declared branch `{}` — nothing to do.",
                self.repo_name, self.to_branch
            )?;
        } else {
            writeln!(
                w,
                "Restored `{}`: {from} → `{}`.",
                self.repo_name, self.to_branch
            )?;
        }
        if self.stashed {
            writeln!(
                w,
                "  Local changes were stashed. Run `cd {} && git stash pop` to recover.",
                self.path
            )?;
        }
        if self.discarded {
            writeln!(w, "  Local changes were discarded (--discard-changes).")?;
        }
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Where a child repo lives within the workspace tree. Mirrors the
/// resolution used by `ws status` / `ws add` / `ws clone`.
fn child_repo_path(workspace_root: &Path, repo: &RepoEntry) -> PathBuf {
    let rel = match &repo.path {
        Some(p) if !p.is_empty() => p.clone(),
        _ => format!("src/{}", repo.name),
    };
    workspace_root.join(rel)
}

/// Run a git subcommand inside `repo_path`, surfacing both the
/// failed exit status and stderr in a single anyhow error.
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
            "ws restore: `{label}` exited with status {code} in {}: {}",
            repo_path.display(),
            stderr.trim()
        );
    }
    Ok(())
}

// ── Argument parsing ──────────────────────────────────────────────

#[derive(Debug)]
struct ParsedArgs {
    repo_name: String,
    auto_stash: bool,
    discard: bool,
}

fn parse_args(args: &[OsString]) -> Result<ParsedArgs> {
    let mut repo_name: Option<String> = None;
    let mut auto_stash = false;
    let mut discard = false;

    for arg in args {
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws restore: argument is not valid UTF-8: {:?}",
                arg.to_string_lossy()
            )
        })?;
        if s == "--auto-stash" {
            if auto_stash {
                bail!("ws restore: '--auto-stash' specified more than once");
            }
            auto_stash = true;
        } else if s == "--discard-changes" {
            if discard {
                bail!("ws restore: '--discard-changes' specified more than once");
            }
            discard = true;
        } else if s.starts_with("--") {
            bail!(
                "ws restore: unexpected flag '{s}'. \
                 Expected: `ws restore <repo> [--auto-stash | --discard-changes]`."
            );
        } else if repo_name.is_none() {
            repo_name = Some(s.to_string());
        } else {
            bail!(
                "ws restore: too many positional arguments. \
                 Restore one repo per invocation: `ws restore <repo>`."
            );
        }
    }

    if auto_stash && discard {
        bail!(
            "ws restore: '--auto-stash' and '--discard-changes' are mutually exclusive. \
             Pick one — preserve work or destroy it."
        );
    }

    let repo_name = repo_name.ok_or_else(|| {
        anyhow!(
            "ws restore: missing required <repo> argument. \
             Usage: `ws restore <repo> [--auto-stash | --discard-changes]`."
        )
    })?;
    Ok(ParsedArgs {
        repo_name,
        auto_stash,
        discard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::{BranchInfo, InProgressOp, WorkingTreeInfo};

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn parse_args_minimal() {
        let p = parse_args(&os(&["alpha"])).unwrap();
        assert_eq!(p.repo_name, "alpha");
        assert!(!p.auto_stash);
        assert!(!p.discard);
    }

    #[test]
    fn parse_args_auto_stash() {
        let p = parse_args(&os(&["alpha", "--auto-stash"])).unwrap();
        assert!(p.auto_stash);
    }

    #[test]
    fn parse_args_discard() {
        let p = parse_args(&os(&["alpha", "--discard-changes"])).unwrap();
        assert!(p.discard);
    }

    #[test]
    fn parse_args_rejects_both_resolution_flags() {
        let err = parse_args(&os(&["alpha", "--auto-stash", "--discard-changes"])).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn parse_args_rejects_missing_repo() {
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
        let err = parse_args(&os(&["alpha", "--bogus"])).unwrap_err();
        assert!(err.to_string().contains("unexpected flag"));
    }

    fn clean_state() -> RepoState {
        RepoState {
            branch: BranchInfo {
                name: Some("feat/x".to_string()),
                oid: Some("abc".to_string()),
                ..Default::default()
            },
            working_tree: WorkingTreeInfo::default(),
            in_progress: InProgressOp::None,
        }
    }

    #[test]
    fn blocking_obstacles_passes_clean_state() {
        let obs = preflight::obstacles(&clean_state());
        assert!(blocking_obstacles(&obs, false, false).is_empty());
    }

    #[test]
    fn blocking_obstacles_blocks_dirty_without_flags() {
        let mut s = clean_state();
        s.working_tree.unstaged = 2;
        let obs = preflight::obstacles(&s);
        assert_eq!(blocking_obstacles(&obs, false, false).len(), 1);
    }

    #[test]
    fn blocking_obstacles_clears_dirty_under_auto_stash() {
        let mut s = clean_state();
        s.working_tree.unstaged = 2;
        s.working_tree.untracked = 1;
        let obs = preflight::obstacles(&s);
        assert!(blocking_obstacles(&obs, true, false).is_empty());
    }

    #[test]
    fn blocking_obstacles_clears_dirty_under_discard() {
        let mut s = clean_state();
        s.working_tree.staged = 3;
        let obs = preflight::obstacles(&s);
        assert!(blocking_obstacles(&obs, false, true).is_empty());
    }

    #[test]
    fn blocking_obstacles_keeps_hard_blocker_even_with_flags() {
        let mut s = clean_state();
        s.in_progress = InProgressOp::Rebase;
        s.working_tree.unstaged = 1;
        let obs = preflight::obstacles(&s);
        // --auto-stash clears the soft blocker but the hard one
        // survives.
        let blocking = blocking_obstacles(&obs, true, false);
        assert_eq!(blocking.len(), 1);
        assert!(blocking[0].is_hard_blocker());
    }
}
