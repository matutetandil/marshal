// Entry point for the workspace tool.
//
// The tool behaves in two modes:
//   1. Passthrough: if no workspace context is detected, forward to git unchanged.
//   2. Workspace-aware: if inside a workspace, apply coordination logic.
//
// Context detection happens first, before any command parsing.

use anyhow::Result;
use std::process::ExitCode;

mod cli;
mod commands;
mod context;
mod git;
mod workspace;

fn main() -> ExitCode {
    // Initialize tracing/logging from RUST_LOG env var
    init_logging();

    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            // Print chain of causes if present
            let mut source = err.source();
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    // Detect workspace context from current directory.
    // This walks up the filesystem looking for .workspace/ marker.
    let ctx = context::detect()?;

    // Parse arguments with awareness of whether we're in a workspace.
    // In passthrough mode, we forward verbatim. In workspace mode, we parse normally.
    cli::dispatch(ctx)
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
