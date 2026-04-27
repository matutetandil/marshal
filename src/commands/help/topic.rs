//! The Strategy interface for `marshal help` topics.
//!
//! Each topic is a unit struct implementing [`HelpTopic`]. The
//! registry looks one up by name when `Help::run` resolves the
//! topic the user asked for; the command then renders the produced
//! [`HelpOutput`] through the dispatcher's chosen format (Human or
//! Json) — same pipeline every other marshal-namespace command uses.
//!
//! Adding a topic is a new `impl HelpTopic` + one registration
//! line in `topics::register_defaults` (Invariant 10).

use crate::cli::Renderable;
use serde::Serialize;
use std::io::{self, Write};
use std::process::{Command, Stdio};

/// Snapshot of the cwd context that context-aware topics consult.
///
/// Cheap to detect (one `git rev-parse` shellout); produced once per
/// `marshal help` invocation by topics that need it. Topics whose
/// content does not depend on context simply skip detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpContext {
    pub in_git_repo: bool,
}

impl HelpContext {
    /// Detect the context. Wraps `git rev-parse --is-inside-work-tree`
    /// with stdio silenced so a "fatal: not a git repository" message
    /// from git does not leak into our output. Any failure (git not
    /// installed, exit non-zero) is interpreted as "outside a repo" —
    /// the worst-case interpretation never makes the help wrong, only
    /// less specific.
    pub fn detect() -> Self {
        let in_git_repo = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        Self { in_git_repo }
    }
}

/// One section of help — a heading and its body lines. Sections are
/// composable so the human renderer can blank-line between them and
/// the JSON form preserves the structure for tooling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HelpSection {
    pub heading: String,
    pub body: Vec<String>,
}

/// The output of every help topic. Doubles as the [`Command`]
/// output for `marshal help` — implements `Renderable` for the
/// human form on stdout and `serde::Serialize` for the JSON form,
/// per Invariant 10.
///
/// [`Command`]: crate::cli::Command
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HelpOutput {
    /// The topic's stable identifier (e.g. `"overview"`,
    /// `"config"`). Useful for tooling that branches on which
    /// help screen is being rendered.
    pub topic: String,

    /// One-line title above the sections.
    pub title: String,

    /// Body of the help, in the order the user reads it.
    pub sections: Vec<HelpSection>,
}

impl Renderable for HelpOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "{}", self.title)?;
        for section in &self.sections {
            // Blank line before each section's heading so the
            // output has visible breathing room.
            writeln!(w)?;
            writeln!(w, "{}", section.heading)?;
            for line in &section.body {
                writeln!(w, "  {line}")?;
            }
        }
        Ok(())
    }
}

/// Strategy: a help topic the user can ask for by name.
pub trait HelpTopic: Send + Sync {
    /// The stable identifier the user types after `marshal help`
    /// (e.g. `"overview"`, `"config"`). Must be lowercase and
    /// unique within the registry.
    fn name(&self) -> &'static str;

    /// Produce the help body for this topic.
    fn produce(&self) -> HelpOutput;
}
