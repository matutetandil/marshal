//! CLI parsing and dispatch.
//!
//! Not consumed by `main` in 0.1.0 (pure passthrough). Kept as scaffolding for
//! Phase 1+ so the path to wiring command interception is a one-line change in
//! `main.rs`.

#![allow(dead_code)]

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::ffi::OsString;
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
    let raw_args: Vec<OsString> = std::env::args_os().collect();

    // Outside a workspace, or with --raw, skip clap entirely and forward to git.
    let raw_requested = raw_args.iter().any(|a| a == "--raw");
    if ctx.is_none() || raw_requested {
        let forward: Vec<OsString> = raw_args.iter().skip(1).cloned().collect();
        return Ok(crate::commands::passthrough::run(&forward));
    }

    let cli = Cli::parse();

    match cli.command {
        None => {
            println!("Usage: marshal <command> [options]");
            println!("Run 'marshal --help' for details.");
            Ok(ExitCode::from(0))
        }
        Some(Command::Status) => crate::commands::status::run(ctx.unwrap(), cli.explain, cli.json),
        Some(Command::Init) => crate::commands::init::run(),
        Some(Command::Clone { url }) => crate::commands::clone::run(&url),
        Some(Command::Log) => crate::commands::log::run(ctx.unwrap()),
        Some(Command::External(args)) => {
            let forward: Vec<OsString> = args.into_iter().map(OsString::from).collect();
            Ok(crate::commands::passthrough::run(&forward))
        }
    }
}
