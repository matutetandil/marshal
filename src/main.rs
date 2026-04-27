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
mod error_hints;
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

    // Two augmentations layer on top of the passthrough:
    //
    // * `--version`: after `git --version` succeeds, marshal appends its own
    //   version line on stdout (the node+npm / php+xdebug pattern).
    // * Actionable error hints: when git fails and the feature is enabled,
    //   stderr is teed (so the hint registry can pattern-match it after the
    //   fact) and a hint is printed below git's own message.
    //
    // The hints feature gates the stderr capture mode itself: with
    // `errors.actionable_hints = false` we revert to inherit stderr and skip
    // the registry entirely, restoring pure passthrough.
    let hint_registry = error_hints::Registry::default();
    let actionable_hints = effective_config.actionable_hints();
    let is_version_query = is_version_only_query(&parsed);

    match commands::passthrough::run_returning_outcome(&forward, actionable_hints) {
        commands::passthrough::Outcome::Ran {
            status,
            captured_stderr,
        } => {
            if !status.success() {
                if let Some(buf) = captured_stderr.as_ref() {
                    let stderr_text = String::from_utf8_lossy(buf);
                    let ctx = error_hints::HintContext {
                        stderr: stderr_text.as_ref(),
                        parsed: &parsed,
                        exit_code: status.code().unwrap_or(-1),
                    };
                    if let Some(hint) = hint_registry.first_hint(&ctx) {
                        hint.emit_to_stderr();
                    }
                }
            }
            if is_version_query && status.success() {
                println!("marshal version {}", env!("CARGO_PKG_VERSION"));
            }
            exit_code_from(status)
        }
        commands::passthrough::Outcome::GitNotFound => ExitCode::from(127),
    }
}

/// `true` when the invocation is a pure version query: no subcommand, and
/// `--version` is one of the global flags. This matches `git --version`,
/// `git -C /tmp --version`, etc.
fn is_version_only_query(parsed: &git::parser::ParsedGitInvocation) -> bool {
    use std::ffi::OsStr;
    parsed.subcommand.is_none()
        && parsed
            .global_flags
            .iter()
            .any(|a| a == OsStr::new("--version"))
}

/// Bridge `ExitStatus` → `ExitCode` so `main` can return directly.
/// Duplicated from the passthrough module to avoid exposing the mapping as
/// public API surface — the passthrough owns the canonical version.
#[cfg(unix)]
fn exit_code_from(status: std::process::ExitStatus) -> ExitCode {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        ExitCode::from(code.clamp(0, 255) as u8)
    } else if let Some(sig) = status.signal() {
        ExitCode::from(128_i32.saturating_add(sig).clamp(0, 255) as u8)
    } else {
        ExitCode::from(1)
    }
}

#[cfg(not(unix))]
fn exit_code_from(status: std::process::ExitStatus) -> ExitCode {
    ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8)
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
