//! The Strategy interface for `marshal what-now` advice rules.
//!
//! Each rule inspects a [`RepoState`] snapshot and decides whether it
//! has a concrete next-step recommendation for the situation. When it
//! matches it returns an [`Advice`] — a one-line summary of what
//! marshal noticed plus a list of bullet suggestions the user can act
//! on.
//!
//! Rules are registered in priority order (most specific situation
//! first); the registry returns the first match. A `clean` fallback
//! rule sits at the end of the chain so every state produces some
//! advice.

use crate::cli::Renderable;
use serde::Serialize;
use std::io::{self, Write};

use super::state::RepoState;

/// Strategy: examine the current repo state and decide whether to
/// emit an advice.
pub trait AdviceRule: Send + Sync {
    /// Examine the snapshot. `Some(advice)` when the rule applies,
    /// `None` otherwise. Rules treat the state as read-only.
    fn examine(&self, state: &RepoState) -> Option<Advice>;
}

/// A user-facing advice produced by `marshal what-now`.
///
/// Doubles as the [`Command`] output for the what-now subcommand:
/// implements [`Renderable`] for the canonical Marshal bullet
/// format on stdout, and `serde::Serialize` for the JSON form
/// (see Invariant 10 in `docs/PRINCIPLES.md` — every command's
/// output speaks both formats so `--json` is a no-op switch in
/// the dispatcher).
///
/// [`Command`]: crate::cli::Command
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Advice {
    pub rule_id: &'static str,

    /// One-line summary of what marshal observed.
    pub title: String,

    /// Concrete next steps, rendered as bullets below the title.
    /// At least one suggestion is expected, but an empty list still
    /// renders cleanly (just the title).
    pub suggestions: Vec<String>,
}

impl Renderable for Advice {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "{}", self.title)?;
        for line in &self.suggestions {
            writeln!(w, "  • {line}")?;
        }
        Ok(())
    }
}
