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
//! current workspace context (root + current child repo). More
//! commands (`ws init`, `ws status`, `ws log`, `ws diff`, `ws clone`)
//! land in subsequent slices. Adding one is `impl Command` plus one
//! arm in [`dispatch`] (Invariant 10).

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use crate::cli::{run_command, Command, OutputFormat, Renderable};
use crate::context;

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
        Ok(WsContextOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            current_repo: ctx.current_repo,
        })
    }
}

#[derive(Serialize)]
pub struct WsContextOutput {
    /// Absolute path to the workspace root (the directory containing
    /// `.workspace/`).
    pub root: String,

    /// `Some(name)` when the cwd is inside a child repo (by the
    /// `<root>/src/<name>/…` convention); `None` at the workspace
    /// root or in workspace-level subdirectories that aren't a child
    /// repo. Skipped from JSON when `None` so machine consumers can
    /// use `parsed.get("current_repo").is_some()` as the predicate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_repo: Option<String>,
}

impl Renderable for WsContextOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "Workspace at: {}", self.root)?;
        match &self.current_repo {
            Some(repo) => writeln!(w, "Current repo: {repo}"),
            None => writeln!(w, "Current repo: (workspace root)"),
        }
    }
}
