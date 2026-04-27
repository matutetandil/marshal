//! `marshal config` — manage Marshal's configuration.
//!
//! Each operation (`get`, `set`, `unset`, `list`) is its own
//! [`Command`] (Strategy) impl in its own file. The sub-dispatcher
//! [`dispatch`] routes the user's sub-subcommand to the right impl
//! and forwards the remaining argv plus the active output format.
//!
//! This file owns:
//!   * `dispatch` — the routing match (one arm per operation;
//!     adding a fifth operation = `impl Command` + one arm).
//!   * `print_help` — the `marshal config help` body, which is the
//!     one place that lists every known operation by name.
//!
//! Argv helpers shared across operations live in `helpers.rs` so
//! the sub-files import only what they need (Invariant 10:
//! adding an operation should not require touching unrelated
//! files).

use anyhow::Result;
use std::ffi::OsString;
use std::process::ExitCode;

use crate::cli::{run_command, OutputFormat};
use crate::config::ConfigKey;

mod get;
mod helpers;
mod list;
mod set;
mod unset;

/// Entry point invoked by `cli::dispatch` when the user types
/// `git marshal config <…>`. Receives every arg past the literal
/// `config` token plus the active output format.
pub fn dispatch(args: &[OsString], format: OutputFormat) -> Result<ExitCode> {
    match args.first().and_then(|s| s.to_str()) {
        None => {
            print_help();
            Ok(ExitCode::from(2))
        }
        Some("get") => run_command(get::ConfigGet, &args[1..], format),
        Some("set") => run_command(set::ConfigSet, &args[1..], format),
        Some("unset") => run_command(unset::ConfigUnset, &args[1..], format),
        Some("list") => run_command(list::ConfigList, &args[1..], format),
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(ExitCode::from(0))
        }
        Some(other) => {
            eprintln!(
                "marshal config: unknown subcommand '{other}'. \
                 Expected get, set, unset, or list."
            );
            Ok(ExitCode::from(2))
        }
    }
}

fn print_help() {
    println!("marshal config — manage Marshal's configuration.");
    println!();
    println!("Usage:");
    println!("  git marshal config get [--show-origin] <key>");
    println!("  git marshal config set   [--system|--global|--local] <key> <value>");
    println!("  git marshal config unset [--system|--global|--local] <key>");
    println!("  git marshal config list");
    println!();
    println!("Known keys:");
    for key in ConfigKey::all() {
        println!("  {:<25}  {}", key.as_dotted(), key.description());
    }
    println!();
    println!("Levels (precedence: system < global < local):");
    println!("  --global (default)  per-user config ($XDG_CONFIG_HOME/marshal/config.toml)");
    println!("  --system            machine-wide config (/etc/marshal/config.toml on Unix)");
    println!("  --local             per-repo config (<git-dir>/marshal/config.toml)");
}
