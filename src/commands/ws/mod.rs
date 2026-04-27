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
//! current workspace context (root + manifest summary + current
//! child repo, reconciled against the manifest). More commands
//! (`ws init`, `ws status`, `ws log`, `ws diff`, `ws clone`) land in
//! subsequent slices. Adding one is `impl Command` plus one arm in
//! [`dispatch`] (Invariant 10).

use anyhow::{anyhow, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use crate::cli::{run_command, Command, OutputFormat, Renderable};
use crate::context;
use crate::workspace::manifest::Manifest;

/// Entry point invoked by `cli::dispatch_ws` when the user types
/// `git ws <…>`. Receives every arg past the literal `ws` token plus
/// the active output format.
pub fn dispatch(args: &[OsString], format: OutputFormat) -> Result<ExitCode> {
    match args.first().and_then(|s| s.to_str()) {
        // Bare `git ws` — print the current workspace context.
        // Future subcommands (init, status, log, diff, clone) land
        // as additional arms in Phase 2 / Slices D–J.
        None => run_command(WsContextInfo, args, format),
        Some(other) => {
            eprintln!(
                "ws: unknown subcommand '{other}'. \
                 Run `git ws` for the workspace context. \
                 More subcommands arrive in Phase 2 — see ROADMAP.md."
            );
            Ok(ExitCode::from(2))
        }
    }
}

/// `git ws` (no arg) — show the current workspace context.
struct WsContextInfo;

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

        // Manifest is optional: a freshly-marked workspace dir
        // (`mkdir .workspace`) may not have a manifest yet, and we
        // want `git ws` to succeed and report that. A *malformed*
        // manifest, on the other hand, propagates the parse error.
        let manifest = Manifest::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace manifest")?;

        let manifest_summary = manifest.as_ref().map(|m| ManifestSummary {
            name: m.workspace.name.clone(),
            default_branch: m.workspace.default_branch.clone(),
            repos: m.repos.iter().map(|r| r.name.clone()).collect(),
        });

        let current_repo = ctx.current_repo.map(|name| {
            let declared = manifest
                .as_ref()
                .map(|m| m.find_repo(&name).is_some())
                .unwrap_or(false);
            CurrentRepo { name, declared }
        });

        Ok(WsContextOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            current_repo,
            manifest: manifest_summary,
        })
    }
}

#[derive(Serialize)]
pub struct WsContextOutput {
    /// Absolute path to the workspace root (the directory containing
    /// `.workspace/`).
    pub root: String,

    /// `Some` when the cwd is inside a child repo (by the
    /// `<root>/src/<name>/…` convention); `None` at the workspace
    /// root or in workspace-level subdirectories that aren't a child
    /// repo. Skipped from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_repo: Option<CurrentRepo>,

    /// `Some` when `<root>/.workspace/manifest.toml` exists and
    /// parses cleanly; `None` when the manifest file does not exist
    /// (a workspace can be partially initialised). Skipped from JSON
    /// when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ManifestSummary>,
}

#[derive(Serialize)]
pub struct CurrentRepo {
    pub name: String,
    /// `true` when the manifest declares a repo with this name. When
    /// `false`, the cwd matches the convention path (`<root>/src/<name>/`)
    /// but the manifest does not list it — likely a typo, an
    /// undeclared repo, or a manifest that has fallen behind reality.
    pub declared: bool,
}

#[derive(Serialize)]
pub struct ManifestSummary {
    pub name: String,
    pub default_branch: String,
    /// Names of every repo declared in the manifest, in declaration
    /// order. The full `RepoEntry` lives in `Manifest`; this output
    /// type stays slim because `git ws` is an orientation command,
    /// not a repo-detail dump.
    pub repos: Vec<String>,
}

impl Renderable for WsContextOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "Workspace at: {}", self.root)?;

        match &self.manifest {
            Some(m) => {
                writeln!(
                    w,
                    "Workspace name: {} (default branch: {})",
                    m.name, m.default_branch
                )?;
                if m.repos.is_empty() {
                    writeln!(w, "Declared repos: (none)")?;
                } else {
                    writeln!(
                        w,
                        "Declared repos ({}): {}",
                        m.repos.len(),
                        m.repos.join(", ")
                    )?;
                }
            }
            None => {
                writeln!(
                    w,
                    "(No manifest yet — `.workspace/manifest.toml` does not exist.)"
                )?;
            }
        }

        match &self.current_repo {
            Some(repo) => {
                let label = if repo.declared {
                    "declared"
                } else {
                    "NOT declared in manifest"
                };
                writeln!(w, "Current repo: {} ({label})", repo.name)
            }
            None => writeln!(w, "Current repo: (workspace root)"),
        }
    }
}
