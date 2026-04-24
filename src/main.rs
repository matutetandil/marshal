//! Marshal entry point.
//!
//! 0.1.0 scope: pure passthrough. Every invocation is forwarded to `git`
//! verbatim, with stdin/stdout/stderr inherited and the exit code propagated
//! exactly. Workspace logic, context detection, and command interception all
//! arrive in later releases (see `docs/ROADMAP.md`).
//!
//! The `cli`, `context`, `workspace`, and most of `commands` modules are
//! scaffolded for those later releases. Their unit tests keep running so the
//! scaffold stays honest, but `main` does not call them in this version.
//! Wiring them in will be a localized change here once Phase 1+ starts.

use std::ffi::OsString;
use std::process::ExitCode;

mod cli;
mod commands;
mod context;
mod git;
mod workspace;

fn main() -> ExitCode {
    init_logging();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    commands::passthrough::run(&args)
}

fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .compact()
        .init();
}
