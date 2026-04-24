// CLI parsing and dispatch.
//
// Design: when no workspace context exists, we pass arguments through to git
// unchanged. When a workspace context exists, we parse with clap and dispatch
// to workspace-aware handlers.
//
// This module is deliberately thin. Actual logic lives in `commands/`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::process::ExitCode;

use crate::context::Context;

/// A Git workspace manager.
///
/// Provides monorepo-like ergonomics over multi-repo storage.
/// When not inside a workspace, behaves as a transparent Git wrapper.
#[derive(Parser, Debug)]
#[command(name = "marshal", version, about, long_about = None)]
#[command(disable_help_subcommand = true)]
struct Cli {
    /// Explain what the operation would do without executing
    #[arg(long, global = true)]
    explain: bool,

    /// Output in machine-readable JSON format
    #[arg(long, global = true)]
    json: bool,

    /// Skip workspace logic, forward directly to git
    #[arg(long, global = true)]
    raw: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show workspace or repo status
    Status,

    /// Initialize a workspace in the current directory
    Init,

    /// Clone a workspace and all its child repos
    Clone {
        /// URL of the workspace manifest or workspace repo
        url: String,
    },

    /// Show workspace log (commits to workspace state)
    Log,

    /// Catch-all: anything we don't recognize passes through to git
    #[command(external_subcommand)]
    External(Vec<String>),
}

pub fn dispatch(ctx: Option<Context>) -> Result<ExitCode> {
    let raw_args: Vec<String> = std::env::args().collect();

    // If we're not in a workspace, or --raw was passed, go straight to passthrough.
    // We detect --raw by scanning args; clap parsing happens only when we know
    // we're operating in workspace mode.
    if ctx.is_none() || raw_args.iter().any(|a| a == "--raw") {
        return crate::commands::passthrough::run(&raw_args[1..]);
    }

    // We're in a workspace. Parse with clap.
    let cli = Cli::parse();

    match cli.command {
        None => {
            // No subcommand given. Show help.
            println!("Usage: marshal <command> [options]");
            println!("Run 'marshal --help' for details.");
            Ok(ExitCode::from(0))
        }
        Some(Command::Status) => crate::commands::status::run(ctx.unwrap(), cli.explain, cli.json),
        Some(Command::Init) => crate::commands::init::run(),
        Some(Command::Clone { url }) => crate::commands::clone::run(&url),
        Some(Command::Log) => crate::commands::log::run(ctx.unwrap()),
        Some(Command::External(args)) => crate::commands::passthrough::run(&args),
    }
}
