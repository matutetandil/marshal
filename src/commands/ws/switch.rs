//! `ws switch <name>` — switch the workspace to a different
//! workspace-branch, materialising its `state.toml` across child
//! repos.
//!
//! The workspace-level analogue of `git switch <branch>`. It does
//! two things, in order:
//!
//! 1. **Switch the workspace-repo** to `<name>` (or create + switch
//!    when `-c` / `--create` is set).
//! 2. **Materialise** the resulting `state.toml` in every declared
//!    child: each child is moved to the branch its entry pins, or
//!    to the manifest's default branch when it has no entry. Repos
//!    already on their target branch are skipped.
//!
//! Pre-flight gates the operation across the workspace **and**
//! every affected child. If any obstacle blocks a clean switch,
//! the operation aborts atomically — nothing in the workspace or
//! any child is mutated. The resolution flags (`--auto-stash`,
//! `--discard-changes`) apply to the workspace-repo and every
//! affected child uniformly.
//!
//! `--on <name>` filters which children get materialised. The
//! workspace-repo always switches. Useful for the equivalence-of-
//! flows pattern from the design brief
//! (`ws switch -c fix/bug-123 --on service-b`).

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

pub struct WsSwitch {
    pub on: Option<String>,
    pub explain: bool,
}

impl Command for WsSwitch {
    type Output = WsSwitchOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
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

        // Validate `--on` against the manifest before doing
        // anything else. Same shape as the other commands that
        // accept `--on`.
        if let Some(on_name) = &self.on {
            if manifest.find_repo(on_name).is_none() {
                let known: Vec<&str> = manifest.repos.iter().map(|r| r.name.as_str()).collect();
                let known_list = if known.is_empty() {
                    "(no repos declared in this workspace yet)".to_string()
                } else {
                    format!("Known: {}.", known.join(", "))
                };
                bail!(
                    "ws switch: '--on {on_name}' does not match any repo declared in the manifest.\n  {known_list}"
                );
            }
        }

        if self.explain {
            let plan = build_explain_plan(
                &ctx.root,
                &parsed.target,
                parsed.create,
                parsed.auto_stash,
                parsed.discard,
                self.on.as_deref(),
            );
            return Ok(WsSwitchOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                workspace_name: manifest.workspace.name.clone(),
                target_branch: parsed.target,
                workspace: WorkspaceSwitchSummary::default(),
                children: Vec::new(),
                explain_plan: Some(plan),
            });
        }

        // ── 1. Read the target state.toml WITHOUT switching ──────
        //
        // `git show <branch>:.workspace/state.toml` lets us see the
        // file as it lives on the target branch right now — before
        // we move anything. Failure modes (target branch missing,
        // file absent on target) collapse to "empty state.toml on
        // target": every child defaults to the manifest's default
        // branch under the new workspace branch.
        let target_state = if parsed.create {
            // -c semantics: the target branch will be created from
            // current HEAD. Its state.toml therefore matches HEAD's
            // — same as the current state.toml.
            StateDeclaration::try_load_from_workspace(&ctx.root)
                .context("failed to read workspace state declaration")?
                .unwrap_or_default()
        } else {
            // Make sure the branch exists somewhere git can find it
            // (locally OR via origin/<name>). git's `--guess` on
            // switch creates the local branch automatically when an
            // upstream tracking reference matches; we delegate that
            // to git when the actual switch runs. Here we just need
            // to read the state.toml on whichever ref resolves.
            load_state_at_ref(&ctx.root, &parsed.target).with_context(|| {
                format!(
                    "ws switch: failed to read state.toml on `{}`. Verify the branch \
                     exists locally or in `origin` (run `git fetch` if you expect it \
                     to track a remote branch). To create a new workspace branch from \
                     here, use `ws switch -c {}`.",
                    parsed.target, parsed.target
                )
            })?
        };

        // ── 2. Compute per-repo plan: skip vs switch ─────────────
        let mut plan_children: Vec<ChildPlan> = Vec::new();
        for repo in &manifest.repos {
            if let Some(filter) = &self.on {
                if &repo.name != filter {
                    continue;
                }
            }
            let abs_path = child_repo_path(&ctx.root, repo);
            let target_branch = target_state
                .get(&repo.name)
                .map(|rs| rs.branch.clone())
                .unwrap_or_else(|| manifest.workspace.default_branch.clone());
            plan_children.push(ChildPlan {
                name: repo.name.clone(),
                path: abs_path,
                target_branch,
            });
        }

        // ── 3. Pre-flight: workspace + every affected child ──────
        //
        // We collect obstacles per repo first so the error message
        // can list them all together — a user with three dirty
        // repos sees all three at once and can resolve in one go.
        let workspace_state = RepoState::detect_at(&ctx.root).with_context(|| {
            format!(
                "ws switch: failed to read workspace-repo state at {}",
                ctx.root.display()
            )
        })?;
        if workspace_state.branch.is_initial {
            bail!(
                "ws switch: workspace-repo at {} has no commits yet. \
                 There is nothing to switch from. Make a first commit and \
                 declare some state with `ws add` + `ws commit`, then re-run.",
                ctx.root.display()
            );
        }

        let mut report = PreflightReport {
            workspace: preflight::obstacles(&workspace_state),
            ..Default::default()
        };

        let mut child_states: Vec<(ChildPlan, Option<RepoState>)> = Vec::new();
        for child in plan_children.into_iter() {
            if !child.path.is_dir() {
                // Missing on disk: not a hard blocker for the
                // SWITCH itself (we just can't materialise that
                // child). Surface it in the report and skip the
                // child during the materialise step.
                report.missing.push(child.name.clone());
                child_states.push((child, None));
                continue;
            }
            let st = RepoState::detect_at(&child.path).with_context(|| {
                format!(
                    "ws switch: failed to read state of `{}` at {}",
                    child.name,
                    child.path.display()
                )
            })?;
            // Only obstacles on children that need to change matter
            // for pre-flight. A child already on its target branch
            // is skipped entirely; uncommitted changes there are
            // not blocking.
            let needs_switch = st.branch.name.as_deref() != Some(child.target_branch.as_str());
            if needs_switch {
                let obs = preflight::obstacles(&st);
                if !obs.is_empty() {
                    report.children.push((child.name.clone(), obs));
                }
            }
            child_states.push((child, Some(st)));
        }

        let workspace_blocking =
            blocking_obstacles(&report.workspace, parsed.auto_stash, parsed.discard);
        let any_child_blocking = report.children.iter().any(|(_name, obs)| {
            !blocking_obstacles(obs, parsed.auto_stash, parsed.discard).is_empty()
        });
        if !workspace_blocking.is_empty() || any_child_blocking {
            bail!(
                "{}",
                format_blocking_error(&report, parsed.auto_stash, parsed.discard)
            );
        }

        // ── 4. Resolve workspace-repo obstacles ──────────────────
        let workspace_resolution = resolve_obstacles(
            &ctx.root,
            &report.workspace,
            parsed.auto_stash,
            parsed.discard,
            &format!("workspace `{}`", manifest.workspace.name),
        )?;

        // ── 5. Workspace switch (with optional -c) ───────────────
        if parsed.create {
            run_git(
                &ctx.root,
                &["branch", &parsed.target],
                &format!("git branch {}", parsed.target),
            )?;
        }
        run_git(
            &ctx.root,
            &["switch", &parsed.target],
            &format!("git switch {}", parsed.target),
        )?;

        let from_branch = workspace_state.branch.name.clone();
        let from_commit = workspace_state.branch.oid.clone();

        // ── 6. Per-child materialisation ─────────────────────────
        let mut children_summaries: Vec<ChildSwitchSummary> = Vec::new();
        for (child, state) in child_states {
            // Missing on disk: report and skip.
            let Some(state) = state else {
                children_summaries.push(ChildSwitchSummary {
                    name: child.name,
                    path: child.path.to_string_lossy().into_owned(),
                    from_branch: None,
                    to_branch: child.target_branch,
                    skipped: false,
                    missing_from_disk: true,
                    stashed: false,
                    discarded: false,
                });
                continue;
            };
            // Already on target.
            if state.branch.name.as_deref() == Some(child.target_branch.as_str()) {
                children_summaries.push(ChildSwitchSummary {
                    name: child.name,
                    path: child.path.to_string_lossy().into_owned(),
                    from_branch: state.branch.name,
                    to_branch: child.target_branch,
                    skipped: true,
                    missing_from_disk: false,
                    stashed: false,
                    discarded: false,
                });
                continue;
            }

            // Resolve obstacles for this child (if any).
            let obs = preflight::obstacles(&state);
            let resolution = resolve_obstacles(
                &child.path,
                &obs,
                parsed.auto_stash,
                parsed.discard,
                &format!("`{}`", child.name),
            )?;

            run_git(
                &child.path,
                &["switch", &child.target_branch],
                &format!("git switch {}", child.target_branch),
            )
            .with_context(|| {
                format!(
                    "while materialising child `{}` to branch `{}`",
                    child.name, child.target_branch
                )
            })?;

            children_summaries.push(ChildSwitchSummary {
                name: child.name,
                path: child.path.to_string_lossy().into_owned(),
                from_branch: state.branch.name,
                to_branch: child.target_branch,
                skipped: false,
                missing_from_disk: false,
                stashed: resolution.stashed,
                discarded: resolution.discarded,
            });
        }

        Ok(WsSwitchOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            workspace_name: manifest.workspace.name,
            target_branch: parsed.target,
            workspace: WorkspaceSwitchSummary {
                from_branch,
                from_commit,
                stashed: workspace_resolution.stashed,
                discarded: workspace_resolution.discarded,
                created_branch: parsed.create,
            },
            children: children_summaries,
            explain_plan: None,
        })
    }
}

// ── Pre-flight aggregation ────────────────────────────────────────

#[derive(Default)]
struct PreflightReport {
    workspace: Vec<Obstacle>,
    /// `(repo_name, obstacles)` for each child that has at least
    /// one obstacle AND would have its branch changed by the switch.
    children: Vec<(String, Vec<Obstacle>)>,
    /// Children declared in the manifest but missing on disk.
    /// Surfaced in the human form of the error/result, not blocking.
    missing: Vec<String>,
}

fn blocking_obstacles(obstacles: &[Obstacle], auto_stash: bool, discard: bool) -> Vec<&Obstacle> {
    obstacles
        .iter()
        .filter(|o| {
            if o.is_hard_blocker() {
                return true;
            }
            !(auto_stash && o.cleared_by_auto_stash() || discard && o.cleared_by_discard())
        })
        .collect()
}

fn format_blocking_error(report: &PreflightReport, auto_stash: bool, discard: bool) -> String {
    let mut msg = String::from(
        "ws switch: cannot switch — pre-flight detected obstacles in one or more repos.\n",
    );

    let workspace_blocking = blocking_obstacles(&report.workspace, auto_stash, discard);
    if !workspace_blocking.is_empty() {
        msg.push_str("\nWorkspace-repo:\n");
        for o in &report.workspace {
            let marker = if workspace_blocking.contains(&o) {
                "  ✗"
            } else {
                "  ✓"
            };
            msg.push_str(&format!("{marker} {}\n", o.description()));
        }
    }
    for (name, obs) in &report.children {
        let blocking = blocking_obstacles(obs, auto_stash, discard);
        if blocking.is_empty() {
            continue;
        }
        msg.push_str(&format!("\nChild `{name}`:\n"));
        for o in obs {
            let marker = if blocking.contains(&o) {
                "  ✗"
            } else {
                "  ✓"
            };
            msg.push_str(&format!("{marker} {}\n", o.description()));
        }
    }

    // Aggregate "any hard / any soft" across workspace + every
    // affected child. Explicit loop keeps the types simple — chaining
    // borrowed iterators across owned `Vec` results from
    // `blocking_obstacles` would force lifetime gymnastics for no
    // gain.
    let mut any_hard = workspace_blocking.iter().any(|o| o.is_hard_blocker());
    let mut any_soft_left = workspace_blocking.iter().any(|o| !o.is_hard_blocker());
    for (_, obs) in &report.children {
        let blocking = blocking_obstacles(obs, auto_stash, discard);
        if blocking.iter().any(|o| o.is_hard_blocker()) {
            any_hard = true;
        }
        if blocking.iter().any(|o| !o.is_hard_blocker()) {
            any_soft_left = true;
        }
    }

    msg.push('\n');
    if any_hard {
        msg.push_str(
            "Hard blockers must be resolved manually before switching:\n  \
             - For an in-progress op: complete it (`git rebase --continue` / etc) \
             or abort it.\n  \
             - For unresolved conflicts: edit the conflicted files, then `git add` \
             + complete the operation.\n  \
             - For an empty repo: make a first commit before switching.\n",
        );
    }
    if any_soft_left {
        if any_hard {
            msg.push('\n');
        }
        msg.push_str(
            "For uncommitted local changes, choose one (the flag applies to every \
             affected repo uniformly):\n  \
             - `ws switch <name> --auto-stash`     stashes everywhere (recoverable \
             via `git stash pop` per repo).\n  \
             - `ws switch <name> --discard-changes` resets and cleans every affected \
             repo (destructive — opt-in).\n  \
             - Or commit/stash by hand inside each affected repo first.\n",
        );
    }
    msg
}

// ── Obstacle resolution ───────────────────────────────────────────

#[derive(Default, Debug, Clone, Copy)]
struct ResolutionResult {
    stashed: bool,
    discarded: bool,
}

fn resolve_obstacles(
    repo: &Path,
    obstacles: &[Obstacle],
    auto_stash: bool,
    discard: bool,
    repo_label: &str,
) -> Result<ResolutionResult> {
    let needs_resolution = obstacles.iter().any(|o| !o.is_hard_blocker());
    if !needs_resolution {
        return Ok(ResolutionResult::default());
    }
    if discard {
        run_git(repo, &["reset", "--hard"], "git reset --hard")?;
        run_git(repo, &["clean", "-fd"], "git clean -fd")?;
        Ok(ResolutionResult {
            stashed: false,
            discarded: true,
        })
    } else if auto_stash {
        let msg = format!("marshal/ws-switch: stashed before switching {repo_label}");
        run_git(
            repo,
            &["stash", "push", "--include-untracked", "-m", &msg],
            "git stash push",
        )?;
        Ok(ResolutionResult {
            stashed: true,
            discarded: false,
        })
    } else {
        // Pre-flight should have rejected this combination already;
        // the only way to reach this branch is a logic bug.
        bail!(
            "ws switch: internal error — obstacles needing resolution \
             reached the resolve step without a flag set"
        );
    }
}

// ── Reading state.toml from a non-checked-out branch ──────────────

fn load_state_at_ref(workspace_root: &Path, branch: &str) -> Result<StateDeclaration> {
    let object = format!("{branch}:.workspace/state.toml");
    let out = ProcessCommand::new("git")
        .current_dir(workspace_root)
        .args(["show", &object])
        .output()
        .with_context(|| {
            format!(
                "failed to invoke `git show` in {}",
                workspace_root.display()
            )
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Distinguish "file not present on this branch" (legitimate
        // → empty state) from "branch does not exist" (real error).
        // Git's actual error strings vary by version; we look for
        // patterns that all point at file-absence:
        //   - "does not exist in <ref>"
        //   - "did not match any file(s) known to git"
        //   - "exists on disk, but not in <ref>"
        // Anything else (unknown revision, ambiguous argument) is
        // a branch-resolution failure and bubbles up.
        let lower = stderr.to_lowercase();
        let file_absent = lower.contains("does not exist in")
            || lower.contains("did not match")
            || lower.contains("does not match")
            || lower.contains("exists on disk, but not in");
        if file_absent {
            return Ok(StateDeclaration::default());
        }
        bail!("git show {object} failed: {}", stderr.trim());
    }
    let content = String::from_utf8_lossy(&out.stdout);
    StateDeclaration::parse(&content)
        .with_context(|| format!("failed to parse state.toml on `{branch}`"))
}

// ── --explain plan ────────────────────────────────────────────────

fn build_explain_plan(
    workspace_root: &Path,
    target: &str,
    create: bool,
    auto_stash: bool,
    discard: bool,
    on: Option<&str>,
) -> Vec<String> {
    let mut plan = vec![format!(
        "git -C {root} show {target}:.workspace/state.toml  \
         (read state.toml on the target branch — collapses to empty \
         when the file does not exist there)",
        root = workspace_root.display()
    )];
    plan.push(
        "compute per-child target branch (state.toml entry, falling back to manifest default)"
            .to_string(),
    );
    plan.push(
        "read porcelain status of the workspace-repo + each affected child (in-scope per --on)"
            .to_string(),
    );
    plan.push(
        "evaluate pre-flight obstacles uniformly; abort if any blocker remains given the flags"
            .to_string(),
    );
    if discard {
        plan.push(
            "if any affected repo is dirty: git reset --hard + git clean -fd (--discard-changes)"
                .to_string(),
        );
    } else if auto_stash {
        plan.push(
            "if any affected repo is dirty: git stash push --include-untracked (--auto-stash)"
                .to_string(),
        );
    }
    if create {
        plan.push(format!(
            "git -C {root} branch {target}  (create the workspace branch from current HEAD)",
            root = workspace_root.display()
        ));
    }
    plan.push(format!(
        "git -C {root} switch {target}",
        root = workspace_root.display()
    ));
    plan.push(if let Some(filter) = on {
        format!(
            "for the child matching --on `{filter}` only: \
             git -C <child> switch <declared-branch>"
        )
    } else {
        "for every affected declared child: git -C <child> switch <declared-branch>".to_string()
    });
    plan
}

// ── Output ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WsSwitchOutput {
    pub root: String,
    pub workspace_name: String,
    pub target_branch: String,
    pub workspace: WorkspaceSwitchSummary,
    pub children: Vec<ChildSwitchSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

#[derive(Serialize, Default)]
pub struct WorkspaceSwitchSummary {
    /// Workspace branch we were on before the switch. `None` if
    /// detached or empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_commit: Option<String>,
    pub stashed: bool,
    pub discarded: bool,
    pub created_branch: bool,
}

#[derive(Serialize)]
pub struct ChildSwitchSummary {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,
    pub to_branch: String,
    /// `true` when the child was already on its target branch.
    pub skipped: bool,
    /// `true` when the child is declared but missing on disk.
    pub missing_from_disk: bool,
    pub stashed: bool,
    pub discarded: bool,
}

impl Renderable for WsSwitchOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws switch", plan);
        }

        let from = self
            .workspace
            .from_branch
            .as_deref()
            .map(|b| format!("`{b}`"))
            .unwrap_or_else(|| "(detached HEAD)".to_string());

        if self.workspace.created_branch {
            writeln!(
                w,
                "Created and switched workspace `{}` to `{}` (from {}).",
                self.workspace_name, self.target_branch, from
            )?;
        } else {
            writeln!(
                w,
                "Switched workspace `{}` to `{}` (from {}).",
                self.workspace_name, self.target_branch, from
            )?;
        }
        if self.workspace.stashed {
            writeln!(
                w,
                "  Workspace-repo: local changes stashed; recover with \
                 `cd {} && git stash pop`.",
                self.root
            )?;
        }
        if self.workspace.discarded {
            writeln!(w, "  Workspace-repo: local changes discarded.")?;
        }

        if self.children.is_empty() {
            writeln!(w)?;
            writeln!(
                w,
                "(No declared children to materialise — manifest declares zero repos.)"
            )?;
            return Ok(());
        }

        let switched: Vec<&ChildSwitchSummary> = self
            .children
            .iter()
            .filter(|c| !c.skipped && !c.missing_from_disk)
            .collect();
        let skipped: Vec<&ChildSwitchSummary> =
            self.children.iter().filter(|c| c.skipped).collect();
        let missing: Vec<&ChildSwitchSummary> = self
            .children
            .iter()
            .filter(|c| c.missing_from_disk)
            .collect();

        writeln!(w)?;
        if !switched.is_empty() {
            writeln!(w, "Children switched:")?;
            let pad = switched.iter().map(|c| c.name.len()).max().unwrap_or(0);
            for child in &switched {
                let from = child
                    .from_branch
                    .as_deref()
                    .map(|b| format!("`{b}`"))
                    .unwrap_or_else(|| "(detached)".to_string());
                let mut line = format!(
                    "  {:<pad$}  {} → `{}`",
                    child.name,
                    from,
                    child.to_branch,
                    pad = pad
                );
                if child.stashed {
                    line.push_str(" — stashed");
                }
                if child.discarded {
                    line.push_str(" — discarded");
                }
                writeln!(w, "{line}")?;
            }
        }
        if !skipped.is_empty() {
            if !switched.is_empty() {
                writeln!(w)?;
            }
            writeln!(w, "Children already on target ({}):", skipped.len())?;
            for child in &skipped {
                writeln!(w, "  {} (on `{}`)", child.name, child.to_branch)?;
            }
        }
        if !missing.is_empty() {
            writeln!(w)?;
            writeln!(w, "Children missing on disk:")?;
            for child in &missing {
                writeln!(
                    w,
                    "  {} (declared `{}`, expected at {})",
                    child.name, child.to_branch, child.path
                )?;
            }
            writeln!(w)?;
            writeln!(
                w,
                "Run `ws clone <url>` against a workspace URL, or clone the missing \
                 child by hand, to materialise the new state."
            )?;
        }

        Ok(())
    }
}

// ── Argument parsing ──────────────────────────────────────────────

#[derive(Debug)]
struct ParsedArgs {
    target: String,
    create: bool,
    auto_stash: bool,
    discard: bool,
}

fn parse_args(args: &[OsString]) -> Result<ParsedArgs> {
    let mut target: Option<String> = None;
    let mut create = false;
    let mut auto_stash = false;
    let mut discard = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws switch: argument {} is not valid UTF-8: {:?}",
                i,
                arg.to_string_lossy()
            )
        })?;
        if s == "--auto-stash" {
            if auto_stash {
                bail!("ws switch: '--auto-stash' specified more than once");
            }
            auto_stash = true;
            i += 1;
        } else if s == "--discard-changes" {
            if discard {
                bail!("ws switch: '--discard-changes' specified more than once");
            }
            discard = true;
            i += 1;
        } else if s == "-c" || s == "--create" {
            if create {
                bail!("ws switch: '{s}' specified more than once");
            }
            create = true;
            i += 1;
        } else if s.starts_with("--") || s.starts_with('-') {
            bail!(
                "ws switch: unexpected flag '{s}'. \
                 Expected: `ws switch [-c] <name> [--auto-stash | --discard-changes]`."
            );
        } else if target.is_none() {
            target = Some(s.to_string());
            i += 1;
        } else {
            bail!(
                "ws switch: too many positional arguments. \
                 Switch one branch per invocation: `ws switch <name>`."
            );
        }
    }

    if auto_stash && discard {
        bail!(
            "ws switch: '--auto-stash' and '--discard-changes' are mutually exclusive. \
             Pick one — preserve work or destroy it."
        );
    }
    let target = target.ok_or_else(|| {
        anyhow!(
            "ws switch: missing required <name> argument. \
             Usage: `ws switch [-c] <name> [--auto-stash | --discard-changes]`."
        )
    })?;
    Ok(ParsedArgs {
        target,
        create,
        auto_stash,
        discard,
    })
}

// ── Helpers ───────────────────────────────────────────────────────

#[derive(Debug)]
struct ChildPlan {
    name: String,
    path: PathBuf,
    target_branch: String,
}

fn child_repo_path(workspace_root: &Path, repo: &RepoEntry) -> PathBuf {
    let rel = match &repo.path {
        Some(p) if !p.is_empty() => p.clone(),
        _ => format!("src/{}", repo.name),
    };
    workspace_root.join(rel)
}

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
            "ws switch: `{label}` exited with status {code} in {}: {}",
            repo_path.display(),
            stderr.trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn parse_args_minimal() {
        let p = parse_args(&os(&["main"])).unwrap();
        assert_eq!(p.target, "main");
        assert!(!p.create);
        assert!(!p.auto_stash);
        assert!(!p.discard);
    }

    #[test]
    fn parse_args_create_short() {
        let p = parse_args(&os(&["-c", "feature/x"])).unwrap();
        assert!(p.create);
        assert_eq!(p.target, "feature/x");
    }

    #[test]
    fn parse_args_create_long() {
        let p = parse_args(&os(&["--create", "feature/x"])).unwrap();
        assert!(p.create);
    }

    #[test]
    fn parse_args_auto_stash() {
        let p = parse_args(&os(&["main", "--auto-stash"])).unwrap();
        assert!(p.auto_stash);
    }

    #[test]
    fn parse_args_discard() {
        let p = parse_args(&os(&["main", "--discard-changes"])).unwrap();
        assert!(p.discard);
    }

    #[test]
    fn parse_args_rejects_both_resolution_flags() {
        let err = parse_args(&os(&["main", "--auto-stash", "--discard-changes"])).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
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
        let err = parse_args(&os(&["main", "--bogus"])).unwrap_err();
        assert!(err.to_string().contains("unexpected flag"));
    }

    #[test]
    fn parse_args_rejects_duplicate_create() {
        let err = parse_args(&os(&["-c", "foo", "--create"])).unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }
}
