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
//!    `marshal` token. It is the only place the active output
//!    format is selected: `--json` (anywhere in argv) flips the
//!    [`OutputFormat`] to `Json`; the format then flows through
//!    `run_command` to the dispatcher's chosen `Command`, which
//!    is invariant to it. Concrete commands never see `--json`.

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

// ── Dispatchers ───────────────────────────────────────────────────

/// Dispatch the argv that came *after* the literal `marshal` token.
///
/// `--json` is a global flag accepted anywhere in argv. It is
/// stripped here and the active [`OutputFormat`] is threaded into
/// every subcommand. Per Invariant 10, this is the *only* place in
/// the marshal namespace where the format is selected — concrete
/// `Command` impls and their sub-dispatchers receive a format and
/// obey it; they do not know about `--json` itself.
pub fn dispatch_marshal(args: &[OsString]) -> Result<ExitCode> {
    let (format, args) = extract_json_flag(args);
    let args = args.as_slice();
    match args.first().and_then(|s| s.to_str()) {
        None => {
            print_overview();
            Ok(ExitCode::from(0))
        }
        Some("config") => crate::commands::config::dispatch(&args[1..], format),
        Some("help") => run_command(crate::commands::help::Help, &args[1..], format),
        Some("what-now") => run_command(crate::commands::what_now::WhatNow, &args[1..], format),
        Some(sub) => {
            eprintln!("marshal: unknown subcommand '{sub}'. Run 'git marshal' for the list.");
            Ok(ExitCode::from(2))
        }
    }
}

/// Dispatch the argv that came *after* the literal `ws` token.
///
/// Sibling to [`dispatch_marshal`] — separate top-level namespace
/// for workspace operations. `--json`, `--all`, and `--on <name>`
/// are extracted here and threaded to every workspace subcommand.
/// `--on` is the "declared scope" override from ARCHITECTURE.md's
/// scope-inference model: the user explicitly names the repo to
/// operate on. Without it, each command falls back to its own
/// scope policy (spatial for `ws log`, full-workspace for `ws
/// status` and `ws diff`).
pub fn dispatch_ws(args: &[OsString]) -> Result<ExitCode> {
    let (format, args) = extract_json_flag(args);
    let (all, args) = extract_all_flag(&args);
    let (on, args) = extract_on_flag(&args)?;
    crate::commands::ws::dispatch(args.as_slice(), all, on, format)
}

/// Strip `--all` from `args` (anywhere in the slice — global flag
/// semantics, same shape as `--json`). Used by [`dispatch_ws`].
/// Workspace commands read the resulting flag from a field on
/// their own `Command` struct; the trait surface stays unchanged.
fn extract_all_flag(args: &[OsString]) -> (bool, Vec<OsString>) {
    use std::ffi::OsStr;
    let target = OsStr::new("--all");
    let mut filtered = Vec::with_capacity(args.len());
    let mut all = false;
    for arg in args {
        if arg.as_os_str() == target {
            all = true;
        } else {
            filtered.push(arg.clone());
        }
    }
    (all, filtered)
}

/// Strip `--on <name>` (or `--on=<name>`) from `args`, returning
/// the optional repo name plus the filtered argv. Used by
/// [`dispatch_ws`] for declared-scope override per ARCHITECTURE.md.
///
/// Errors when the flag is repeated or its value is missing /
/// not UTF-8. The flag's value is required to be a `&str` because
/// it has to round-trip through manifest lookups; non-UTF-8 names
/// would be a configuration error in the manifest itself.
fn extract_on_flag(args: &[OsString]) -> Result<(Option<String>, Vec<OsString>)> {
    use std::ffi::OsStr;
    let mut filtered = Vec::with_capacity(args.len());
    let mut on: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let bytes = arg.as_os_str();
        if bytes == OsStr::new("--on") {
            // `--on <value>` separated form.
            let value = args
                .get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("'--on' expects a value (e.g. `--on service-a`)"))?;
            let s = value
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("'--on' value is not valid UTF-8"))?;
            assign_on(&mut on, s)?;
            i += 2;
            continue;
        }
        if let Some(s) = arg.to_str() {
            if let Some(value) = s.strip_prefix("--on=") {
                assign_on(&mut on, value)?;
                i += 1;
                continue;
            }
        }
        filtered.push(arg.clone());
        i += 1;
    }
    Ok((on, filtered))
}

fn assign_on(slot: &mut Option<String>, value: &str) -> Result<()> {
    if slot.is_some() {
        anyhow::bail!("'--on' specified more than once");
    }
    if value.is_empty() {
        anyhow::bail!("'--on' expects a non-empty repo name");
    }
    *slot = Some(value.to_string());
    Ok(())
}

/// Strip `--json` from `args` (anywhere in the slice — global flag
/// semantics), and return the active output format alongside the
/// filtered argv. Accepting `--json` in any position lets the user
/// place it where it reads best for them
/// (`marshal --json config list` or `marshal config list --json`)
/// without forcing a positional convention.
fn extract_json_flag(args: &[OsString]) -> (OutputFormat, Vec<OsString>) {
    use std::ffi::OsStr;
    let target = OsStr::new("--json");
    let mut filtered = Vec::with_capacity(args.len());
    let mut json = false;
    for arg in args {
        if arg.as_os_str() == target {
            json = true;
        } else {
            filtered.push(arg.clone());
        }
    }
    let format = if json {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    (format, filtered)
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
    println!("  help       Print this overview, or `help <topic>` for details");
    println!();
    println!("Workspace operations live under their own namespace:");
    println!("  git ws     Show current workspace context (init/status/log/… land in Phase 2)");
    println!();
    println!("Run `marshal help` for a comprehensive reference (topic-by-topic).");
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
