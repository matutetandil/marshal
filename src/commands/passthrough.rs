//! Forward the invocation to `git` verbatim.
//!
//! In 0.1.0 this is the entire product behavior.
//!
//! Portability contract:
//! - Arguments are `OsString`, not `String`. Non-UTF-8 bytes on Unix paths and
//!   wide-char arguments on Windows survive the round-trip into `git`.
//! - stdin, stdout, stderr are inherited directly. No colour stripping, no
//!   CRLF translation, no paginator interference.
//! - The exit code of `git` is propagated. On Unix, death by signal maps to
//!   the shell convention `128 + signum`.
//! - `git` is resolved via `PATH`. On Windows this picks up `git.exe`
//!   automatically; on any OS it honours whatever `git` the developer has.

use std::ffi::OsString;
use std::process::{Command, ExitCode, ExitStatus, Stdio};

/// The possible outcomes of attempting to spawn `git`.
///
/// Callers that just want Marshal's overall exit code use [`run`]; callers
/// that need to decide on post-passthrough behaviour (e.g. the
/// `--version` augmentation in `main`) use [`run_returning_outcome`] and
/// inspect the [`ExitStatus`] directly.
pub enum Outcome {
    /// `git` launched and ran to completion; carries its exit status.
    Ran(ExitStatus),
    /// `git` could not be launched (typically: not on `PATH`). The caller's
    /// error message has already been emitted to stderr.
    GitNotFound,
}

/// Forward `args` to `git` and return `git`'s exit code as our own.
pub fn run(args: &[OsString]) -> ExitCode {
    match run_returning_outcome(args) {
        Outcome::Ran(status) => exit_code_from(status),
        // 127 is the conventional shell exit code for "command not found".
        Outcome::GitNotFound => ExitCode::from(127),
    }
}

/// Forward `args` to `git` and return a structured outcome. Used by `main`
/// to act on success/failure after the fact (step 6's `--version`
/// augmentation is the first consumer).
pub fn run_returning_outcome(args: &[OsString]) -> Outcome {
    tracing::debug!(args_count = args.len(), "passthrough: invoking git");

    let status = Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(s) => Outcome::Ran(s),
        Err(err) => {
            eprintln!(
                "marshal: failed to execute `git`: {err}\n\
                 is `git` installed and on your PATH?"
            );
            Outcome::GitNotFound
        }
    }
}

#[cfg(unix)]
fn exit_code_from(status: ExitStatus) -> ExitCode {
    use std::os::unix::process::ExitStatusExt;

    if let Some(code) = status.code() {
        ExitCode::from(clamp_u8(code))
    } else if let Some(sig) = status.signal() {
        // POSIX shell convention: process killed by signal N exits 128 + N.
        ExitCode::from(clamp_u8(128_i32.saturating_add(sig)))
    } else {
        ExitCode::from(1)
    }
}

#[cfg(not(unix))]
fn exit_code_from(status: ExitStatus) -> ExitCode {
    ExitCode::from(clamp_u8(status.code().unwrap_or(1)))
}

fn clamp_u8(code: i32) -> u8 {
    code.clamp(0, 255) as u8
}
