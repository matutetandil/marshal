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

/// Forward `args` to `git` and return `git`'s exit code as our own.
pub fn run(args: &[OsString]) -> ExitCode {
    tracing::debug!(args_count = args.len(), "passthrough: invoking git");

    let status = Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(s) => exit_code_from(s),
        Err(err) => {
            eprintln!(
                "marshal: failed to execute `git`: {err}\n\
                 is `git` installed and on your PATH?"
            );
            // 127 is the conventional shell exit code for "command not found".
            ExitCode::from(127)
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
