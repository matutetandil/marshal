//! Forward the invocation to `git` verbatim.
//!
//! Two modes:
//!
//! * **Inherit** (default in 0.1.0–0.2.x): stdin/stdout/stderr inherited
//!   directly. Marshal is invisible; git is byte-for-byte indistinguishable.
//! * **Capture stderr** (added in 0.3.0): stdout/stdin still inherited;
//!   stderr is piped through a worker thread that writes each chunk to our
//!   stderr live and retains a bounded copy in memory. The retained bytes
//!   feed the actionable error hints registry — when git fails, the
//!   registry pattern-matches on the captured text and emits a tip.
//!
//! Capture mode is opt-out: a user who wants pure passthrough sets
//! `errors.actionable_hints = false` and gets back the inherit path with
//! zero augmentation.
//!
//! Portability contract:
//! - Arguments are `OsString`, not `String`. Non-UTF-8 bytes on Unix paths and
//!   wide-char arguments on Windows survive the round-trip into `git`.
//! - In inherit mode, no colour stripping, no CRLF translation, no
//!   paginator interference. In capture mode the same holds for stdout;
//!   stderr passes through us byte-for-byte (we forward chunks unchanged).
//! - Streaming is preserved in capture mode: long-running commands (clone,
//!   push, fetch) print progress live because the tee thread flushes after
//!   every chunk.
//! - The exit code of `git` is propagated. On Unix, death by signal maps to
//!   the shell convention `128 + signum`.
//! - `git` is resolved via `PATH`. On Windows this picks up `git.exe`
//!   automatically; on any OS it honours whatever `git` the developer has.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Command, ExitCode, ExitStatus, Stdio};

/// Maximum bytes of stderr retained in the capture buffer. Beyond this cap,
/// bytes are still forwarded live to our stderr (the user sees everything
/// git wrote), but they are dropped from the in-memory buffer so a runaway
/// stderr cannot blow our memory. 256 KiB is well above any plausible real
/// git error message — the typical hit is a few hundred bytes.
const STDERR_CAPTURE_CAP: usize = 256 * 1024;

/// The possible outcomes of attempting to spawn `git`.
///
/// Callers that just want Marshal's overall exit code use [`run`]; callers
/// that need to act on success/failure (the `--version` augmentation, the
/// actionable error hints) use [`run_returning_outcome`] and inspect the
/// fields directly.
pub enum Outcome {
    /// `git` launched and ran to completion.
    Ran {
        status: ExitStatus,
        /// `Some(buf)` when the caller asked for capture; `None` otherwise.
        /// Buffer is capped at [`STDERR_CAPTURE_CAP`]; truncation is silent
        /// (consumers match against substrings, not byte counts).
        ///
        /// Silenced until the actionable error hints registry consumes it —
        /// added in the next step of the 0.3.0 cycle.
        #[allow(dead_code)]
        captured_stderr: Option<Vec<u8>>,
    },
    /// `git` could not be launched (typically: not on `PATH`). The caller's
    /// error message has already been emitted to stderr.
    GitNotFound,
}

/// Forward `args` to `git` and return `git`'s exit code as our own. Inherits
/// stderr — no capture, no augmentation. Used by simple call sites that do
/// not need to react to the outcome.
pub fn run(args: &[OsString]) -> ExitCode {
    match run_returning_outcome(args, false) {
        Outcome::Ran { status, .. } => exit_code_from(status),
        // 127 is the conventional shell exit code for "command not found".
        Outcome::GitNotFound => ExitCode::from(127),
    }
}

/// Forward `args` to `git` and return a structured outcome.
///
/// `capture_stderr` selects the stderr handling:
/// * `false` — `Stdio::inherit()`. Identical to plain `git`. No capture
///   buffer is produced (`captured_stderr` is `None`).
/// * `true` — pipe stderr from git, forward every chunk to our stderr
///   immediately, and retain a bounded copy in the returned buffer. Used
///   by callers that pattern-match on stderr after the fact.
pub fn run_returning_outcome(args: &[OsString], capture_stderr: bool) -> Outcome {
    tracing::debug!(
        args_count = args.len(),
        capture_stderr,
        "passthrough: invoking git"
    );

    if capture_stderr {
        run_with_captured_stderr(args)
    } else {
        run_with_inherited_stderr(args)
    }
}

fn run_with_inherited_stderr(args: &[OsString]) -> Outcome {
    let status = Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(status) => Outcome::Ran {
            status,
            captured_stderr: None,
        },
        Err(err) => {
            emit_git_not_found(&err);
            Outcome::GitNotFound
        }
    }
}

fn run_with_captured_stderr(args: &[OsString]) -> Outcome {
    let spawn = Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawn {
        Ok(c) => c,
        Err(err) => {
            emit_git_not_found(&err);
            return Outcome::GitNotFound;
        }
    };

    // We configured stderr as `piped()`, so `take()` is guaranteed to find
    // the pipe. Express that as `expect` rather than a runtime branch we
    // cannot exercise.
    let mut child_stderr = child
        .stderr
        .take()
        .expect("stderr is piped on capture mode");

    let tee = std::thread::spawn(move || tee_stderr(&mut child_stderr));

    let status = match child.wait() {
        Ok(s) => s,
        Err(err) => {
            // wait() failing post-spawn is a kernel-level oddity; log it
            // and degrade to GitNotFound for symmetry. The tee thread is
            // detached but will exit on its own when the pipe closes.
            eprintln!("marshal: failed to wait on `git`: {err}");
            return Outcome::GitNotFound;
        }
    };

    // The tee thread reads until EOF and returns its buffer. EOF arrives
    // when the child closes its stderr — guaranteed by `wait()` above.
    let captured = tee.join().unwrap_or_default();

    Outcome::Ran {
        status,
        captured_stderr: Some(captured),
    }
}

/// Read from `child_stderr` in chunks; write each chunk to our stderr
/// immediately (preserving streaming) while retaining a copy up to
/// [`STDERR_CAPTURE_CAP`] bytes. Returns the captured buffer.
///
/// Generic over `R: Read` so unit tests can drive it with a `Cursor`
/// without spawning a process.
fn tee_stderr<R: Read>(child_stderr: &mut R) -> Vec<u8> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut our_stderr = std::io::stderr();
    loop {
        match child_stderr.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let _ = our_stderr.write_all(&chunk[..n]);
                let _ = our_stderr.flush();
                if buffer.len() < STDERR_CAPTURE_CAP {
                    let take = (STDERR_CAPTURE_CAP - buffer.len()).min(n);
                    buffer.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    buffer
}

fn emit_git_not_found(err: &std::io::Error) {
    eprintln!(
        "marshal: failed to execute `git`: {err}\n\
         is `git` installed and on your PATH?"
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A typical-sized input fits entirely in the buffer; nothing is
    /// dropped on the floor.
    #[test]
    fn tee_returns_full_input_under_cap() {
        let input = b"fatal: not a git repository (or any parent up to mount point /)\n";
        let mut reader = Cursor::new(input.to_vec());
        let captured = tee_stderr(&mut reader);
        assert_eq!(captured, input);
    }

    /// Once the buffer hits the cap, additional bytes are still consumed
    /// from the reader (so the child never blocks on a full pipe) but no
    /// longer accumulated. The retained prefix is the head of the input —
    /// which is what hint rules will match against.
    #[test]
    fn tee_caps_buffer_but_consumes_input_fully() {
        let huge: Vec<u8> = (0..(STDERR_CAPTURE_CAP + 1024))
            .map(|i| (i % 256) as u8)
            .collect();
        let mut reader = Cursor::new(huge.clone());
        let captured = tee_stderr(&mut reader);
        assert_eq!(captured.len(), STDERR_CAPTURE_CAP);
        assert_eq!(&captured[..], &huge[..STDERR_CAPTURE_CAP]);
    }

    /// An empty input produces an empty buffer — no panics, no allocations
    /// beyond the initial `Vec::new()`.
    #[test]
    fn tee_handles_empty_input() {
        let mut reader = Cursor::new(Vec::<u8>::new());
        let captured = tee_stderr(&mut reader);
        assert!(captured.is_empty());
    }
}
