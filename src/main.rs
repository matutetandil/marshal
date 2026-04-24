//! Marshal entry point.
//!
//! Routing rules:
//! 1. Parse argv into a `ParsedGitInvocation`.
//! 2. If the subcommand is literally `marshal` (typical usage:
//!    `git marshal <sub>` when aliased), dispatch to our own namespace.
//! 3. Otherwise forward byte-exact to `git`.
//!
//! Passthrough remains the default for every command the user already knows
//! from Git. The `marshal` namespace is the only place marshal speaks in its
//! own voice in 0.2.0; later steps add `version` augmentation and
//! modernization tips layered on top.

use std::ffi::OsString;
use std::process::ExitCode;

mod cli;
mod commands;
mod context;
mod git;
mod modernize;
mod workspace;

fn main() -> ExitCode {
    init_logging();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let parsed = git::parser::parse(&args);

    if parsed.subcommand_is("marshal") {
        return match cli::dispatch(&parsed.subcommand_args) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("marshal: {err}");
                let mut source = err.source();
                while let Some(cause) = source {
                    eprintln!("  caused by: {cause}");
                    source = cause.source();
                }
                ExitCode::from(1)
            }
        };
    }

    // Modernization hook. Registry is empty in step 3 (behavior unchanged);
    // step 4 fills it with the 12 canonical rules, step 5 adds config to
    // toggle tips/rewrite. Wiring it now keeps step 4 purely additive.
    let registry = modernize::Registry::default();
    if let Some(opinion) = registry.first_opinion(&parsed) {
        opinion.suggestion.emit_to_stderr();
        // Rewrite mode comes with config in step 5; step 3 always forwards
        // the original args.
    }

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
