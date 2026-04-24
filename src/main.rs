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
mod config;
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

    // Modernization hook gated by config.
    //
    // A malformed config file must not break Git commands — the user is
    // trying to run `git status`, not fix their config. On load error we
    // warn once to stderr and fall back to defaults so the command still
    // completes.
    let effective_config = match config::ConfigResolver::current_user() {
        Ok(resolver) => resolver.effective().unwrap_or_else(|err| {
            eprintln!("marshal: warning: using defaults — {err}");
            config::Config::default()
        }),
        Err(err) => {
            eprintln!("marshal: warning: using defaults — {err}");
            config::Config::default()
        }
    };

    let registry = modernize::Registry::default();
    let forward: Vec<OsString> = if let Some(opinion) = registry.first_opinion(&parsed) {
        if effective_config.modernize_tips() {
            opinion.suggestion.emit_to_stderr();
        }
        if effective_config.modernize_rewrite() {
            opinion.rewrite.unwrap_or_else(|| args.clone())
        } else {
            args.clone()
        }
    } else {
        args.clone()
    };

    commands::passthrough::run(&forward)
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
