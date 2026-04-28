//! `ws` namespace — workspace operations.
//!
//! Sibling to the `marshal` namespace. With marshal aliased to git,
//! the user invokes workspace operations as `git ws <…>`. The
//! choice of a separate top-level namespace (rather than nesting
//! under `marshal` or intercepting plain `git` commands) is dictated
//! by Invariant 9 (Developer Flow Preserved): workspace features are
//! additive, opt-in, and live behind a recognisable prefix. A user
//! who never types `ws` keeps git's exact behaviour.
//!
//! Phase 2 ships read-only operations into this namespace. Today the
//! bare `ws` (no arg) is the only reachable command — it prints the
//! current workspace context (root + manifest summary + state
//! declarations + current child repo, all reconciled). More commands
//! (`ws init`, `ws status`, `ws log`, `ws diff`, `ws clone`) land in
//! subsequent slices. Adding one is `impl Command` plus one arm in
//! [`dispatch`] (Invariant 10).
//!
//! Hide-boring presentation: workspace commands aim to be scannable
//! even with hundreds of child repos. Repos on the manifest's
//! default branch are summarised as a count; only "interesting"
//! repos (pinned to a non-default branch by state.toml) are listed
//! individually. The global `--all` flag overrides the abbreviation
//! and shows every repo in full.

use anyhow::{anyhow, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use crate::cli::{run_command, Command, OutputFormat, Renderable};
use crate::context;
use crate::workspace::manifest::Manifest;
use crate::workspace::state::StateDeclaration;

mod diff;
mod init;
mod log;
mod status;

/// Threshold for "show full list inline" vs "show count + interesting only".
/// Below or equal to this, the human form expands unconditionally; above it,
/// the abbreviation kicks in unless `--all` is set.
const ABBREVIATE_THRESHOLD: usize = 5;

/// Entry point invoked by `cli::dispatch_ws` when the user types
/// `git ws <…>`. Receives every arg past the literal `ws` token, plus
/// the global `--all` flag and the active output format.
pub fn dispatch(args: &[OsString], all: bool, format: OutputFormat) -> Result<ExitCode> {
    match args.first().and_then(|s| s.to_str()) {
        // Bare `git ws` — print the current workspace context.
        None => run_command(WsContextInfo { all }, args, format),
        // `git ws init` — create a `.workspace/` directory here.
        Some("init") => run_command(init::WsInit, &args[1..], format),
        // `git ws status` — aggregated read-only view across child repos.
        Some("status") => run_command(status::WsStatus { all }, &args[1..], format),
        // `git ws log` — aggregated commit log across child repos.
        Some("log") => run_command(log::WsLog { all }, &args[1..], format),
        // `git ws diff` — semantic interpretation of state.toml changes.
        Some("diff") => run_command(diff::WsDiff, &args[1..], format),
        Some(other) => {
            eprintln!(
                "ws: unknown subcommand '{other}'. \
                 Run `git ws` for the workspace context, `git ws init` \
                 to create one, `git ws status` for aggregated state, \
                 `git ws log` for cross-repo activity, or `git ws diff` \
                 for state-declaration changes. More subcommands arrive \
                 in Phase 2 — see ROADMAP.md."
            );
            Ok(ExitCode::from(2))
        }
    }
}

/// `git ws` (no arg) — show the current workspace context. The
/// `all` field encodes the global `--all` flag and toggles the
/// hide-boring vs full-expansion rendering modes.
struct WsContextInfo {
    all: bool,
}

impl Command for WsContextInfo {
    type Output = WsContextOutput;

    fn run(&self, _args: &[OsString]) -> Result<Self::Output> {
        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here once `ws init` ships in a future slice."
            )
        })?;

        let manifest = Manifest::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace manifest")?;
        let state = StateDeclaration::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace state declaration")?;

        let manifest_summary = manifest.as_ref().map(|m| ManifestSummary {
            name: m.workspace.name.clone(),
            default_branch: m.workspace.default_branch.clone(),
            repos: m.repos.iter().map(|r| r.name.clone()).collect(),
        });

        // Build state entries one per declared repo, with an
        // `on_default` flag for the renderer's hide-boring logic.
        // Without a manifest there are no repos to enumerate, so
        // state info collapses to None.
        let state_summary = manifest.as_ref().map(|m| {
            let default_branch = &m.workspace.default_branch;
            let entries: Vec<RepoStateEntry> = m
                .repos
                .iter()
                .map(|repo| {
                    let branch = state
                        .as_ref()
                        .and_then(|s| s.get(&repo.name))
                        .map(|rs| rs.branch.clone())
                        .unwrap_or_else(|| default_branch.clone());
                    let on_default = branch == *default_branch;
                    RepoStateEntry {
                        name: repo.name.clone(),
                        branch,
                        on_default,
                    }
                })
                .collect();
            StateSummary {
                has_state_file: state.is_some(),
                entries,
            }
        });

        // current_repo carries its effective declared branch when the
        // manifest knows about it; useful for the user to see at a
        // glance what state.toml expects for the repo they are in.
        let current_repo = ctx.current_repo.map(|name| {
            let declared = manifest
                .as_ref()
                .map(|m| m.find_repo(&name).is_some())
                .unwrap_or(false);
            let declared_branch = if declared {
                state_summary
                    .as_ref()
                    .and_then(|s| s.entries.iter().find(|e| e.name == name))
                    .map(|e| e.branch.clone())
            } else {
                None
            };
            CurrentRepo {
                name,
                declared,
                declared_branch,
            }
        });

        Ok(WsContextOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            current_repo,
            manifest: manifest_summary,
            state: state_summary,
            all: self.all,
        })
    }
}

#[derive(Serialize)]
pub struct WsContextOutput {
    pub root: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_repo: Option<CurrentRepo>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ManifestSummary>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<StateSummary>,

    /// Render-time only: forces full expansion in the human form.
    /// Excluded from JSON — machine consumers always get the
    /// complete data, so the flag is irrelevant for them.
    #[serde(skip)]
    pub all: bool,
}

#[derive(Serialize)]
pub struct CurrentRepo {
    pub name: String,
    pub declared: bool,
    /// Effective declared branch for this repo: the value from
    /// state.toml when it overrides, otherwise the manifest's
    /// default branch. `None` when there is no manifest, or when
    /// the cwd-derived `name` is not in the manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_branch: Option<String>,
}

#[derive(Serialize)]
pub struct ManifestSummary {
    pub name: String,
    pub default_branch: String,
    pub repos: Vec<String>,
}

#[derive(Serialize)]
pub struct StateSummary {
    /// `true` when `<root>/.workspace/state.toml` exists — even an
    /// empty file counts. `false` when there is no state file at
    /// all (every repo defaults by definition).
    pub has_state_file: bool,
    /// One entry per repo in the manifest, with the effective
    /// declared branch and an `on_default` flag. Always full —
    /// the human renderer abbreviates by skipping `on_default`
    /// entries unless `all` is set; JSON consumers always see the
    /// complete list.
    pub entries: Vec<RepoStateEntry>,
}

#[derive(Serialize)]
pub struct RepoStateEntry {
    pub name: String,
    pub branch: String,
    /// `true` when `branch` equals the manifest's default branch
    /// (either implicitly — no entry in state.toml — or explicitly
    /// — entry matches default). The renderer collapses these
    /// into a count when `all` is `false` and the total is large.
    pub on_default: bool,
}

impl Renderable for WsContextOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "Workspace at: {}", self.root)?;

        // ── Manifest summary ───────────────────────────────────
        match &self.manifest {
            Some(m) => {
                writeln!(
                    w,
                    "Workspace name: {} (default branch: {})",
                    m.name, m.default_branch
                )?;
                let n = m.repos.len();
                if n == 0 {
                    writeln!(w, "Declared repos: (none)")?;
                } else if self.all || n <= ABBREVIATE_THRESHOLD {
                    writeln!(w, "Declared repos ({}): {}", n, m.repos.join(", "))?;
                } else {
                    writeln!(w, "Declared repos: {n}")?;
                }
            }
            None => {
                writeln!(
                    w,
                    "(No manifest yet — `.workspace/manifest.toml` does not exist.)"
                )?;
            }
        }

        // ── State declarations ─────────────────────────────────
        if let Some(state) = &self.state {
            render_state(w, state, self.all)?;
        }

        // ── Current repo ───────────────────────────────────────
        match &self.current_repo {
            Some(repo) => {
                let declared_label = if repo.declared {
                    "declared"
                } else {
                    "NOT declared in manifest"
                };
                match &repo.declared_branch {
                    Some(branch) => writeln!(
                        w,
                        "Current repo: {} ({}, state declares `{}`)",
                        repo.name, declared_label, branch
                    ),
                    None => writeln!(w, "Current repo: {} ({})", repo.name, declared_label),
                }
            }
            None => writeln!(w, "Current repo: (workspace root)"),
        }
    }
}

/// Render the state-declarations block using the hide-boring rule.
/// Pulled into its own function because the branching matters: the
/// data shape is uniform but the rendering has four cases (empty,
/// all-default, abbreviated, expanded).
fn render_state(w: &mut dyn Write, state: &StateSummary, all: bool) -> io::Result<()> {
    let total = state.entries.len();
    if total == 0 {
        // Manifest has no repos; nothing to declare state for.
        return Ok(());
    }

    let pinned: Vec<&RepoStateEntry> = state.entries.iter().filter(|e| !e.on_default).collect();
    let default_count = total - pinned.len();

    let prefix = if state.has_state_file {
        "State declarations"
    } else {
        "State (implicit — no state.toml; all repos on default)"
    };

    // Fast path: every repo is on the default branch.
    if pinned.is_empty() {
        writeln!(w, "{prefix}: all {total} repos on default branch.")?;
        return Ok(());
    }

    writeln!(w, "{prefix}:")?;

    if all || total <= ABBREVIATE_THRESHOLD {
        // Expanded: list every repo with its effective branch.
        let pad = state
            .entries
            .iter()
            .map(|e| e.name.len())
            .max()
            .unwrap_or(0);
        for entry in &state.entries {
            if entry.on_default {
                writeln!(w, "  {:<pad$}  default", entry.name, pad = pad)?;
            } else {
                writeln!(
                    w,
                    "  {:<pad$}  on `{}`",
                    entry.name,
                    entry.branch,
                    pad = pad
                )?;
            }
        }
    } else if pinned.len() > ABBREVIATE_THRESHOLD {
        // Many pinned and many defaulted — counts only.
        writeln!(
            w,
            "  {} repos pinned to non-default branches.",
            pinned.len()
        )?;
        writeln!(w, "  {default_count} default to manifest's default branch.")?;
    } else {
        // Few pinned, many defaulted — list pinned, count defaulted.
        let pad = pinned.iter().map(|e| e.name.len()).max().unwrap_or(0);
        for entry in &pinned {
            writeln!(
                w,
                "  {:<pad$}  on `{}`",
                entry.name,
                entry.branch,
                pad = pad
            )?;
        }
        if default_count > 0 {
            let plural = if default_count == 1 { "repo" } else { "others" };
            writeln!(
                w,
                "  {default_count} {plural} default to manifest's default branch."
            )?;
        }
    }

    Ok(())
}
