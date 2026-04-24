//! Marshal namespace dispatcher.
//!
//! When the user types `git marshal <…>` (the common case, with the alias to
//! git) or `marshal marshal <…>` (direct binary invocation), `main` routes
//! here instead of forwarding to git. This is marshal's own voice.
//!
//! For 0.2.0 the dispatcher is intentionally minimal:
//!   * no args         → print an overview and exit 0
//!   * unknown sub     → error on stderr, exit 2
//!
//! Concrete subcommands (`version`, `config`, …) land in later steps of the
//! 0.2.0 decomposition and are added here via Strategy + a command registry
//! when there is more than one to dispatch. Until then, an enum-less match
//! keeps the wiring honest — no pretending commands exist before they do.

use anyhow::Result;
use std::ffi::OsString;
use std::process::ExitCode;

/// Dispatch the argv that came *after* the literal `marshal` token.
pub fn dispatch(args: &[OsString]) -> Result<ExitCode> {
    match args.first().and_then(|s| s.to_str()) {
        None => {
            print_overview();
            Ok(ExitCode::from(0))
        }
        Some("config") => crate::commands::config::dispatch(&args[1..]),
        Some(sub) => {
            eprintln!("marshal: unknown subcommand '{sub}'. Run 'git marshal' for the list.");
            Ok(ExitCode::from(2))
        }
    }
}

fn print_overview() {
    println!("marshal {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("A transparent wrapper for git. When aliased to `git`, every");
    println!("invocation is forwarded verbatim unless the first subcommand is");
    println!("`marshal`, which routes to marshal's own namespace.");
    println!();
    println!("Marshal subcommands:");
    println!("  config    Manage Marshal configuration (get/set/unset/list)");
    println!();
    println!("More subcommands appear as they ship; see the project CHANGELOG.");
}
