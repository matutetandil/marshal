//! `ws restore [<repo> | --all]` — bring child repos back to the
//! workspace's declared state.
//!
//! Two modes:
//!
//! * **Single-repo** (`ws restore <repo>`) — switches the named
//!   child to its declared branch. The original Phase 3 / Slice C
//!   shape; behavior unchanged.
//! * **Multi-repo** (`ws restore --all`) — switches *every*
//!   declared child to its declared branch. Atomic pre-flight
//!   across the whole scope, then parallel execution via the
//!   `workspace::parallel` framework. Mirrors the pattern shipped
//!   by `ws switch` (Slice G).
//!
//! Both modes share the same pre-flight taxonomy:
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
//! In `--all` mode, the resolution flags apply uniformly across
//! every affected child. The decision "preserve" vs "destroy" is
//! workspace-wide; per-child selectivity can be expressed by
//! invoking the single-repo form once per repo.
//!
//! Atomic pre-flight in `--all` mode: every affected child's state
//! is read before any mutation. If any child has a blocking
//! obstacle, the operation aborts cleanly with a single aggregated
//! error listing every obstacle marked ✗ (blocking) / ✓ (cleared
//! by current flags). No child is mutated.
//!
//! Skip-when-aligned: a child already on its declared branch is
//! reported as skipped without invoking git, and is excluded from
//! the blocker computation entirely. Dirty state in those children
//! is irrelevant because we are not touching them.
//!
//! `--auto-stash` and `--discard-changes` are mutually exclusive
//! (the user has to pick a side). `--explain` prints the plan
//! without doing anything, including which obstacle resolution
//! step would run if the working tree is dirty.
//!
//! `--on <name>` — the namespace's declared-scope override — is
//! redundant when there's already a positional `<repo>`, so we
//! refuse it explicitly with a hint at the canonical form. In
//! `--all` mode it is also rejected: the whole point of `--all` is
//! "every declared child"; per-child filtering belongs to the
//! single-repo form.

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
use crate::workspace::parallel;
use crate::workspace::preflight::{self, Obstacle};
use crate::workspace::state::StateDeclaration;

pub struct WsRestore {
    /// `true` when the namespace's global `--all` is set. For
    /// `ws restore` this means "every declared child" rather than
    /// hide-boring expansion (which is meaningless here).
    pub all: bool,
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
                "ws restore: takes a positional <repo> (or --all), not `--on`. \
                 Try `ws restore {on_name}` instead."
            );
        }
        let parsed = parse_args(args)?;
        let target = match (self.all, parsed.repo_name) {
            (true, Some(name)) => bail!(
                "ws restore: '--all' and a positional <repo> are mutually exclusive. \
                 Use `ws restore {name}` to restore just one child, or `ws restore --all` \
                 to restore every declared child."
            ),
            (true, None) => Target::All,
            (false, Some(name)) => Target::Single(name),
            (false, None) => bail!(
                "ws restore: missing target. \
                 Usage: `ws restore <repo>` (single child) or `ws restore --all` (every declared child) \
                 [--auto-stash | --discard-changes]."
            ),
        };

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

        match target {
            Target::Single(repo_name) => run_single(
                &ctx.root,
                &manifest,
                &repo_name,
                parsed.auto_stash,
                parsed.discard,
                self.explain,
            ),
            Target::All => run_all(
                &ctx.root,
                &manifest,
                parsed.auto_stash,
                parsed.discard,
                self.explain,
            ),
        }
    }
}

// ── Single-repo execution ─────────────────────────────────────────

fn run_single(
    root: &Path,
    manifest: &Manifest,
    repo_name: &str,
    auto_stash: bool,
    discard: bool,
    explain: bool,
) -> Result<WsRestoreOutput> {
    let repo = manifest.find_repo(repo_name).ok_or_else(|| {
        let known: Vec<&str> = manifest.repos.iter().map(|r| r.name.as_str()).collect();
        let known_list = if known.is_empty() {
            "(no repos declared in this workspace yet)".to_string()
        } else {
            format!("Known: {}.", known.join(", "))
        };
        anyhow!(
            "ws restore: '{repo_name}' does not match any repo declared in the manifest.\n  {known_list}"
        )
    })?;

    let abs_path = child_repo_path(root, repo);

    let state = StateDeclaration::try_load_from_workspace(root)
        .context("failed to read workspace state declaration")?;
    let declared_branch = state
        .as_ref()
        .and_then(|s| s.get(repo_name))
        .map(|rs| rs.branch.clone())
        .unwrap_or_else(|| manifest.workspace.default_branch.clone());

    // --explain: describe the plan without running it. The
    // plan is conservative — we cannot read the working tree
    // under --explain (that would be a side-effect-shaped
    // shellout that the user might not want), so we say the
    // pre-flight will run + the resolution step that *would*
    // execute given the requested flags.
    if explain {
        let plan = build_explain_plan(&abs_path, &declared_branch, auto_stash, discard);
        return Ok(WsRestoreOutput::Single(WsRestoreSingleOutput {
            root: root.to_string_lossy().into_owned(),
            repo_name: repo_name.to_string(),
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
        }));
    }

    if !abs_path.is_dir() {
        bail!(
            "ws restore: child repo `{repo_name}` is missing on disk \
             (declared, expected at {}). \
             Clone the workspace with `ws clone` first.",
            abs_path.display()
        );
    }

    // Capture pre-restore state.
    let before = RepoState::detect_at(&abs_path).with_context(|| {
        format!(
            "ws restore: failed to read state of `{repo_name}` at {}",
            abs_path.display()
        )
    })?;

    let from_branch = before.branch.name.clone();
    let from_commit = before.branch.oid.clone();

    // Pre-flight: collect every obstacle, then decide whether
    // any are blocking given the user's flags.
    let obs = preflight::obstacles(&before);
    let blocking = blocking_obstacles(&obs, auto_stash, discard);
    if !blocking.is_empty() {
        bail!(
            "{}",
            format_blocking_error(repo_name, &abs_path, &blocking, &obs)
        );
    }

    // Resolve soft blockers under --auto-stash / --discard. Hard
    // blockers were caught above. The presence of any soft
    // blocker that survived pre-flight means a resolution flag
    // is set and there's actually something to resolve.
    let needs_resolution = obs.iter().any(|o| !o.is_hard_blocker());

    let mut stashed = false;
    let mut discarded = false;
    if needs_resolution && discard {
        run_git(&abs_path, &["reset", "--hard"], "git reset --hard")?;
        run_git(&abs_path, &["clean", "-fd"], "git clean -fd")?;
        discarded = true;
    } else if needs_resolution && auto_stash {
        // Build a stash message that identifies the operation
        // and the repo. Useful when listing later.
        let msg = format!(
            "marshal/ws-restore: stashed before switching `{repo_name}` to `{declared_branch}`"
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
        format!("ws restore: failed to re-read state of `{repo_name}` after switch")
    })?;

    Ok(WsRestoreOutput::Single(WsRestoreSingleOutput {
        root: root.to_string_lossy().into_owned(),
        repo_name: repo_name.to_string(),
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
    }))
}

// ── Multi-repo (--all) execution ──────────────────────────────────

fn run_all(
    root: &Path,
    manifest: &Manifest,
    auto_stash: bool,
    discard: bool,
    explain: bool,
) -> Result<WsRestoreOutput> {
    // --explain: describe the plan without running it. The plan
    // does not enumerate repos because that would require reading
    // state.toml (a side-effect-free op, but explain is
    // intentionally a pure dry-run).
    if explain {
        let plan = build_explain_plan_all(root, auto_stash, discard);
        return Ok(WsRestoreOutput::All(WsRestoreAllOutput {
            root: root.to_string_lossy().into_owned(),
            workspace_name: manifest.workspace.name.clone(),
            children: Vec::new(),
            explain_plan: Some(plan),
        }));
    }

    // 1. Compute per-child plan: declared branch + abs path.
    let state = StateDeclaration::try_load_from_workspace(root)
        .context("failed to read workspace state declaration")?;
    let mut plan_children: Vec<ChildPlan> = Vec::with_capacity(manifest.repos.len());
    for repo in &manifest.repos {
        let declared_branch = state
            .as_ref()
            .and_then(|s| s.get(&repo.name))
            .map(|rs| rs.branch.clone())
            .unwrap_or_else(|| manifest.workspace.default_branch.clone());
        plan_children.push(ChildPlan {
            name: repo.name.clone(),
            path: child_repo_path(root, repo),
            declared_branch,
        });
    }

    // 2. Atomic pre-flight: read state of every child that's on
    //    disk. Skip-when-aligned: a child already on its declared
    //    branch is excluded from the blocker computation entirely.
    //    Children missing on disk are surfaced (not blocking).
    let mut obstacle_report: Vec<(String, Vec<Obstacle>)> = Vec::new();
    let mut child_states: Vec<(ChildPlan, ChildPreflight)> =
        Vec::with_capacity(plan_children.len());
    for child in plan_children.into_iter() {
        if !child.path.is_dir() {
            child_states.push((child, ChildPreflight::Missing));
            continue;
        }
        let st = RepoState::detect_at(&child.path).with_context(|| {
            format!(
                "ws restore: failed to read state of `{}` at {}",
                child.name,
                child.path.display()
            )
        })?;
        let needs_restore = st.branch.name.as_deref() != Some(child.declared_branch.as_str());
        if needs_restore {
            let obs = preflight::obstacles(&st);
            if !obs.is_empty() {
                obstacle_report.push((child.name.clone(), obs.clone()));
            }
            child_states.push((child, ChildPreflight::NeedsRestore { state: st }));
        } else {
            child_states.push((child, ChildPreflight::Aligned { state: st }));
        }
    }

    let any_blocking = obstacle_report
        .iter()
        .any(|(_n, obs)| !blocking_obstacles(obs, auto_stash, discard).is_empty());
    if any_blocking {
        bail!(
            "{}",
            format_all_blocking_error(&obstacle_report, auto_stash, discard)
        );
    }

    // 3. Per-child execution — parallel.
    let outcomes = parallel::execute(
        &child_states,
        |(plan, _)| plan.name.clone(),
        |(child, pre), bar| -> Result<ChildRestoreSummary> {
            let started = std::time::Instant::now();

            match pre {
                ChildPreflight::Missing => {
                    bar.finish_with_message("missing on disk");
                    Ok(ChildRestoreSummary {
                        name: child.name.clone(),
                        path: child.path.to_string_lossy().into_owned(),
                        declared_branch: child.declared_branch.clone(),
                        from_branch: None,
                        to_branch: child.declared_branch.clone(),
                        skipped: false,
                        missing_from_disk: true,
                        stashed: false,
                        discarded: false,
                    })
                }
                ChildPreflight::Aligned { state } => {
                    bar.finish_with_message("✓ already on declared");
                    Ok(ChildRestoreSummary {
                        name: child.name.clone(),
                        path: child.path.to_string_lossy().into_owned(),
                        declared_branch: child.declared_branch.clone(),
                        from_branch: state.branch.name.clone(),
                        to_branch: child.declared_branch.clone(),
                        skipped: true,
                        missing_from_disk: false,
                        stashed: false,
                        discarded: false,
                    })
                }
                ChildPreflight::NeedsRestore { state } => {
                    let from_branch = state.branch.name.clone();

                    let obs = preflight::obstacles(state);
                    let needs_resolution = obs.iter().any(|o| !o.is_hard_blocker());
                    let mut stashed = false;
                    let mut discarded = false;
                    if needs_resolution && discard {
                        bar.set_message("discarding local changes");
                        run_git(&child.path, &["reset", "--hard"], "git reset --hard")?;
                        run_git(&child.path, &["clean", "-fd"], "git clean -fd")?;
                        discarded = true;
                    } else if needs_resolution && auto_stash {
                        bar.set_message("stashing local changes");
                        let msg = format!(
                            "marshal/ws-restore: stashed before restoring `{}` to `{}`",
                            child.name, child.declared_branch
                        );
                        run_git(
                            &child.path,
                            &["stash", "push", "--include-untracked", "-m", &msg],
                            "git stash push",
                        )?;
                        stashed = true;
                    }

                    bar.set_message(format!("switching to {}", child.declared_branch));
                    run_git(
                        &child.path,
                        &["switch", &child.declared_branch],
                        &format!("git switch {}", child.declared_branch),
                    )
                    .with_context(|| {
                        format!(
                            "while restoring child `{}` to branch `{}`",
                            child.name, child.declared_branch
                        )
                    })?;

                    bar.finish_with_message(format!(
                        "✓ restored to {} in {}",
                        child.declared_branch,
                        parallel::format_ms(started.elapsed().as_millis())
                    ));

                    Ok(ChildRestoreSummary {
                        name: child.name.clone(),
                        path: child.path.to_string_lossy().into_owned(),
                        declared_branch: child.declared_branch.clone(),
                        from_branch,
                        to_branch: child.declared_branch.clone(),
                        skipped: false,
                        missing_from_disk: false,
                        stashed,
                        discarded,
                    })
                }
            }
        },
    );

    let children: Vec<ChildRestoreSummary> = outcomes.into_iter().collect::<Result<Vec<_>>>()?;

    Ok(WsRestoreOutput::All(WsRestoreAllOutput {
        root: root.to_string_lossy().into_owned(),
        workspace_name: manifest.workspace.name.clone(),
        children,
        explain_plan: None,
    }))
}

#[derive(Debug)]
struct ChildPlan {
    name: String,
    path: PathBuf,
    declared_branch: String,
}

enum ChildPreflight {
    Missing,
    Aligned { state: RepoState },
    NeedsRestore { state: RepoState },
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

/// `ws restore` returns one of two shapes depending on the mode:
/// the single-repo form (positional `<repo>`) carries per-repo
/// fields directly; the multi-repo form (`--all`) wraps a vec of
/// per-child summaries. `#[serde(untagged)]` so JSON consumers
/// receive the inner shape directly without a wrapper key.
#[derive(Serialize)]
#[serde(untagged)]
pub enum WsRestoreOutput {
    Single(WsRestoreSingleOutput),
    All(WsRestoreAllOutput),
}

#[derive(Serialize)]
pub struct WsRestoreSingleOutput {
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

#[derive(Serialize)]
pub struct WsRestoreAllOutput {
    pub root: String,
    pub workspace_name: String,
    pub children: Vec<ChildRestoreSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChildRestoreSummary {
    pub name: String,
    pub path: String,
    pub declared_branch: String,
    /// Branch the child was on before the restore. `None` if
    /// missing on disk or detached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,
    pub to_branch: String,
    /// `true` when the child was already on its declared branch.
    pub skipped: bool,
    /// `true` when the child is declared but missing on disk.
    pub missing_from_disk: bool,
    pub stashed: bool,
    pub discarded: bool,
}

impl Renderable for WsRestoreOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        match self {
            WsRestoreOutput::Single(s) => render_single(s, w),
            WsRestoreOutput::All(a) => render_all(a, w),
        }
    }
}

fn render_single(s: &WsRestoreSingleOutput, w: &mut dyn Write) -> io::Result<()> {
    if let Some(plan) = &s.explain_plan {
        return super::render_explain_plan(w, "ws restore", plan);
    }

    let from = s
        .from_branch
        .as_deref()
        .map(|b| format!("`{b}`"))
        .unwrap_or_else(|| "(no branch)".to_string());

    if s.from_branch.as_deref() == Some(&s.to_branch) {
        writeln!(
            w,
            "`{}` already on declared branch `{}` — nothing to do.",
            s.repo_name, s.to_branch
        )?;
    } else {
        writeln!(w, "Restored `{}`: {from} → `{}`.", s.repo_name, s.to_branch)?;
    }
    if s.stashed {
        writeln!(
            w,
            "  Local changes were stashed. Run `cd {} && git stash pop` to recover.",
            s.path
        )?;
    }
    if s.discarded {
        writeln!(w, "  Local changes were discarded (--discard-changes).")?;
    }
    Ok(())
}

fn render_all(a: &WsRestoreAllOutput, w: &mut dyn Write) -> io::Result<()> {
    if let Some(plan) = &a.explain_plan {
        return super::render_explain_plan(w, "ws restore --all", plan);
    }

    let restored: Vec<&ChildRestoreSummary> = a
        .children
        .iter()
        .filter(|c| !c.skipped && !c.missing_from_disk)
        .collect();
    let already: Vec<&ChildRestoreSummary> = a.children.iter().filter(|c| c.skipped).collect();
    let missing: Vec<&ChildRestoreSummary> =
        a.children.iter().filter(|c| c.missing_from_disk).collect();

    if restored.is_empty() && already.is_empty() && missing.is_empty() {
        writeln!(
            w,
            "Workspace `{}` has no declared children to restore.",
            a.workspace_name
        )?;
        return Ok(());
    }

    if !restored.is_empty() {
        for c in &restored {
            let from = c
                .from_branch
                .as_deref()
                .map(|b| format!("`{b}`"))
                .unwrap_or_else(|| "(no branch)".to_string());
            writeln!(w, "Restored `{}`: {from} → `{}`.", c.name, c.to_branch)?;
            if c.stashed {
                writeln!(
                    w,
                    "  Local changes were stashed. Run `cd {} && git stash pop` to recover.",
                    c.path
                )?;
            }
            if c.discarded {
                writeln!(w, "  Local changes were discarded (--discard-changes).")?;
            }
        }
    }
    if !already.is_empty() {
        for c in &already {
            writeln!(
                w,
                "`{}` already on declared branch `{}` — skipped.",
                c.name, c.to_branch
            )?;
        }
    }
    if !missing.is_empty() {
        writeln!(w)?;
        writeln!(
            w,
            "Note: {} declared {} missing on disk:",
            missing.len(),
            if missing.len() == 1 {
                "child"
            } else {
                "children"
            }
        )?;
        for c in &missing {
            writeln!(w, "  - `{}` (expected at {})", c.name, c.path)?;
        }
        writeln!(
            w,
            "Run `ws clone` to populate them, or remove them from the manifest."
        )?;
    }
    Ok(())
}

fn build_explain_plan_all(workspace_root: &Path, auto_stash: bool, discard: bool) -> Vec<String> {
    let mut plan = vec![
        format!(
            "read `{}/.workspace/manifest.toml` to enumerate declared children",
            workspace_root.display()
        ),
        format!(
            "read `{}/.workspace/state.toml` to compute each child's declared branch",
            workspace_root.display()
        ),
        "for every child on disk: \
         git -C <child> status --porcelain=v2 --branch  (read working state for pre-flight)"
            .to_string(),
        "evaluate pre-flight obstacles per child; abort the entire op if any blocking \
         obstacle remains given the requested flags (atomic refusal — no child mutated)"
            .to_string(),
        "skip-when-aligned: a child already on its declared branch is excluded from \
         the blocker computation and contributes a `skipped: true` summary"
            .to_string(),
    ];
    if discard {
        plan.push(
            "for each affected child with a dirty tree: \
             git -C <child> reset --hard  +  git -C <child> clean -fd  (--discard-changes)"
                .to_string(),
        );
    } else if auto_stash {
        plan.push(
            "for each affected child with a dirty tree: \
             git -C <child> stash push --include-untracked -m \"marshal/ws-restore: …\"  (--auto-stash)"
                .to_string(),
        );
    }
    plan.push(
        "for every affected child, in parallel: \
         git -C <child> switch <declared-branch>"
            .to_string(),
    );
    plan
}

fn format_all_blocking_error(
    obstacle_report: &[(String, Vec<Obstacle>)],
    auto_stash: bool,
    discard: bool,
) -> String {
    let mut msg = String::from(
        "ws restore: cannot restore — pre-flight detected obstacles in one or more children.\n",
    );
    for (name, obs) in obstacle_report {
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

    let mut any_hard = false;
    let mut any_soft_left = false;
    for (_, obs) in obstacle_report {
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
            "Hard blockers must be resolved manually before restoring:\n  \
             - For an in-progress op: complete it (`git rebase --continue` / etc) or abort it.\n  \
             - For unresolved conflicts: edit the conflicted files, then `git add` + complete the operation.\n  \
             - For an empty repo: make a first commit before restoring.\n",
        );
    }
    if any_soft_left {
        if any_hard {
            msg.push('\n');
        }
        msg.push_str(
            "For uncommitted local changes, choose one (the flag applies to every \
             affected child uniformly):\n  \
             - `ws restore --all --auto-stash`     stashes everywhere (recoverable \
             via `git stash pop` per child).\n  \
             - `ws restore --all --discard-changes` resets and cleans every affected \
             child (destructive — opt-in).\n  \
             - Or commit/stash by hand inside each affected child first.\n",
        );
    }
    msg
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
//
// `--all` is intentionally NOT parsed here: it is the namespace's
// global hide-boring flag, consumed by `dispatch_ws` and threaded
// to this command via `WsRestore::all`. parse_args only sees
// per-command flags (`--auto-stash`, `--discard-changes`) and the
// optional positional `<repo>`. The combination of the two is
// resolved into a `Target` in the Command's `run` method.

#[derive(Debug)]
struct ParsedArgs {
    repo_name: Option<String>,
    auto_stash: bool,
    discard: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Target {
    Single(String),
    All,
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
                 Expected: `ws restore (<repo> | --all) [--auto-stash | --discard-changes]`."
            );
        } else if repo_name.is_none() {
            repo_name = Some(s.to_string());
        } else {
            bail!(
                "ws restore: too many positional arguments. \
                 Restore one repo per invocation: `ws restore <repo>`, \
                 or every declared child with `ws restore --all`."
            );
        }
    }

    if auto_stash && discard {
        bail!(
            "ws restore: '--auto-stash' and '--discard-changes' are mutually exclusive. \
             Pick one — preserve work or destroy it."
        );
    }

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
        assert_eq!(p.repo_name.as_deref(), Some("alpha"));
        assert!(!p.auto_stash);
        assert!(!p.discard);
    }

    #[test]
    fn parse_args_auto_stash() {
        let p = parse_args(&os(&["alpha", "--auto-stash"])).unwrap();
        assert!(p.auto_stash);
        assert_eq!(p.repo_name.as_deref(), Some("alpha"));
    }

    #[test]
    fn parse_args_discard() {
        let p = parse_args(&os(&["alpha", "--discard-changes"])).unwrap();
        assert!(p.discard);
    }

    #[test]
    fn parse_args_no_positional_is_ok() {
        // `--all` lives at the namespace level and is consumed by
        // the dispatcher; parse_args sees args without it. With no
        // positional and no per-command flags, parse_args succeeds
        // and the Command's run resolves the missing target into
        // either All (if dispatcher set self.all = true) or a clear
        // "missing target" error otherwise.
        let p = parse_args(&[]).unwrap();
        assert!(p.repo_name.is_none());
        assert!(!p.auto_stash);
        assert!(!p.discard);
    }

    #[test]
    fn parse_args_no_positional_with_auto_stash() {
        let p = parse_args(&os(&["--auto-stash"])).unwrap();
        assert!(p.repo_name.is_none());
        assert!(p.auto_stash);
    }

    #[test]
    fn parse_args_rejects_both_resolution_flags() {
        let err = parse_args(&os(&["alpha", "--auto-stash", "--discard-changes"])).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
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

    #[test]
    fn parse_args_does_not_recognise_all_flag() {
        // `--all` is a namespace-global flag stripped by the
        // dispatcher before parse_args runs. If it ever reaches
        // parse_args, it should be rejected as unknown — the
        // dispatcher is the sole owner.
        let err = parse_args(&os(&["--all"])).unwrap_err();
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
