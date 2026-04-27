//! Marshal namespace dispatcher.
//!
//! Two layers live here:
//!
//! 1. The [`Command`] / [`Renderable`] / [`OutputFormat`] contract —
//!    the Strategy substrate every marshal namespace command builds
//!    on. Mandated by Invariant 10 (Open/Closed via Strategy):
//!    adding a command is `impl Command` plus one registration
//!    line, never a modification of existing impls or the dispatch
//!    body.
//!
//! 2. The [`dispatch`] entry point routing argv after the literal
//!    `marshal` token to either the marshal-namespace overview or a
//!    concrete command. As of step S1 the existing
//!    `commands::config::dispatch` and `commands::what_now::run`
//!    paths still own their own argv parsing — the migration to
//!    `Command` happens command-by-command in S2 and S3, then
//!    `--json` lights up centrally in S4 with no further per-command
//!    changes.

use anyhow::Result;
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

// ── Strategy: Command + Renderable + OutputFormat ─────────────────

/// Output format the user asked for. Set once by the dispatcher
/// (default `Human`; `Json` when the global `--json` flag is
/// present in S4). Concrete commands never see this — the dispatcher
/// switches on it after `Command::run` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    /// Constructed in S4 once `--json` lights up in the dispatcher.
    /// Already plumbed through `write_output` in S1 so the JSON
    /// path is exercised by unit tests from day one.
    #[allow(dead_code)]
    Json,
}

impl Default for OutputFormat {
    /// Human is the default everywhere — stdout to a terminal,
    /// stdout to a pipe (no machine-readable assumptions),
    /// integration tests that don't pass `--json`.
    fn default() -> Self {
        Self::Human
    }
}

/// Renderable: produce the human-readable form of an output value.
/// Sibling to `serde::Serialize` (used for the JSON form). Outputs
/// of `Command` impls always have both — that is what makes
/// `--json` a no-op switch in the dispatcher rather than a
/// per-command feature.
pub trait Renderable {
    /// Write the human-readable form to `w`. Implementations must
    /// not panic on write errors — propagate them so the dispatcher
    /// can react (typically: an I/O error on stdout terminates the
    /// command with a non-zero exit).
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()>;
}

/// Strategy: a marshal-namespace command. Each implementor parses
/// its own subcommand-specific argv and produces a typed `Output`.
/// The dispatcher renders the output to either human or JSON based
/// on the active [`OutputFormat`]; commands never branch on format.
///
/// This shape is required by Invariant 10 in `docs/PRINCIPLES.md`:
/// adding a new command is `impl Command` plus one registration line
/// in the dispatcher; never modifying existing impls or the dispatch
/// body.
pub trait Command {
    /// The structured value the command produces. Carries both
    /// `Renderable` (for the human form) and `serde::Serialize`
    /// (for JSON) — the dispatcher routes between them based on
    /// the active output format.
    type Output: Renderable + Serialize;

    /// Run the command. `args` is everything *after* the
    /// subcommand token (so a `Command` for `config get` sees
    /// `["modernize.tips"]`, not `["get", "modernize.tips"]`).
    fn run(&self, args: &[OsString]) -> Result<Self::Output>;
}

/// Run a command and emit its output in the requested format on
/// stdout. The dispatcher uses this once a concrete `Command`
/// impl is selected. Errors from `Command::run` and I/O errors
/// from rendering both propagate.
///
/// Silenced until S2 migrates the first command to use it. The
/// scaffolding lands in S1 so the contract and tests are in place.
#[allow(dead_code)]
pub fn run_command<C: Command>(
    cmd: C,
    args: &[OsString],
    format: OutputFormat,
) -> Result<ExitCode> {
    let output = cmd.run(args)?;
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    write_output(&output, format, &mut handle)?;
    Ok(ExitCode::from(0))
}

/// Test seam: render `output` to an arbitrary writer rather than
/// stdout. Used by [`run_command`] internally and by unit tests
/// that need to assert against the bytes produced.
#[allow(dead_code)]
fn write_output<R: Renderable + Serialize>(
    output: &R,
    format: OutputFormat,
    w: &mut dyn Write,
) -> Result<()> {
    match format {
        OutputFormat::Human => output.render_human(w)?,
        OutputFormat::Json => {
            serde_json::to_writer_pretty(&mut *w, output)?;
            // Pretty-printed JSON does not include a trailing
            // newline; add one so the output composes nicely with
            // shell pipes and CLI conventions.
            w.write_all(b"\n")?;
        }
    }
    Ok(())
}

// ── Dispatcher ────────────────────────────────────────────────────

/// Dispatch the argv that came *after* the literal `marshal` token.
pub fn dispatch(args: &[OsString]) -> Result<ExitCode> {
    match args.first().and_then(|s| s.to_str()) {
        None => {
            print_overview();
            Ok(ExitCode::from(0))
        }
        Some("config") => {
            // Output format is `Human` until S4 wires the global
            // `--json` flag; per Invariant 10, this is the only
            // place the format is selected.
            crate::commands::config::dispatch(&args[1..], OutputFormat::default())
        }
        Some("what-now") => crate::commands::what_now::run(),
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
    println!("  config     Manage Marshal configuration (get/set/unset/list)");
    println!("  what-now   Analyse repo state and suggest the next action");
    println!();
    println!("More subcommands appear as they ship; see the project CHANGELOG.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    /// A representative output type: small, has structure, exercises
    /// both human rendering and JSON serialisation.
    #[derive(Serialize)]
    struct Greeting {
        who: String,
        excited: bool,
    }

    impl Renderable for Greeting {
        fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
            let punct = if self.excited { "!" } else { "." };
            writeln!(w, "Hello, {}{}", self.who, punct)
        }
    }

    /// A `Command` test double that returns a fixed `Greeting`.
    /// Lets us drive `run_command` end-to-end without touching real
    /// commands (which migrate in S2/S3).
    struct GreetCommand;

    impl Command for GreetCommand {
        type Output = Greeting;

        fn run(&self, _args: &[OsString]) -> Result<Self::Output> {
            Ok(Greeting {
                who: "world".to_string(),
                excited: true,
            })
        }
    }

    #[test]
    fn human_format_uses_renderable() {
        let g = Greeting {
            who: "marshal".to_string(),
            excited: false,
        };
        let mut buf = Vec::new();
        write_output(&g, OutputFormat::Human, &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "Hello, marshal.\n");
    }

    #[test]
    fn json_format_uses_serialize() {
        let g = Greeting {
            who: "marshal".to_string(),
            excited: true,
        };
        let mut buf = Vec::new();
        write_output(&g, OutputFormat::Json, &mut buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed["who"], "marshal");
        assert_eq!(parsed["excited"], true);
        // The trailing newline matters for shell composition.
        assert!(buf.ends_with(b"\n"));
    }

    #[test]
    fn json_format_is_pretty_printed() {
        // Pretty-printing keeps `marshal config list --json | jq …`
        // ergonomic when the user pipes to a viewer rather than
        // straight into another program.
        let g = Greeting {
            who: "x".to_string(),
            excited: false,
        };
        let mut buf = Vec::new();
        write_output(&g, OutputFormat::Json, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains('\n') && s.contains("  "),
            "expected pretty-printed JSON (newlines + indent), got: {s:?}"
        );
    }

    #[test]
    fn run_command_returns_zero_exit_code_on_success() {
        // We can't easily intercept stdout from `run_command` in a
        // unit test; the format-routing logic is covered by
        // `write_output` tests above. Here we just assert the
        // success contract: a Command that runs cleanly yields exit 0.
        let code = run_command(GreetCommand, &[], OutputFormat::Human).unwrap();
        // ExitCode does not impl PartialEq; compare via the Debug
        // representation which is stable for `ExitCode::from(N)`.
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(0)));
    }

    #[test]
    fn output_format_default_is_human() {
        assert_eq!(OutputFormat::default(), OutputFormat::Human);
    }
}
