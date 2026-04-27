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

use super::state::RepoState;

/// Strategy: examine the current repo state and decide whether to
/// emit an advice.
pub trait AdviceRule: Send + Sync {
    /// Examine the snapshot. `Some(advice)` when the rule applies,
    /// `None` otherwise. Rules treat the state as read-only.
    fn examine(&self, state: &RepoState) -> Option<Advice>;
}

/// A user-facing advice rendered by `marshal what-now`.
///
/// Renders to **stdout** (not stderr): unlike modernization tips and
/// error hints, `what-now` is a user-invoked command and the advice
/// *is* its output. Stderr stays available for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    pub rule_id: &'static str,

    /// One-line summary of what marshal observed.
    pub title: String,

    /// Concrete next steps, rendered as bullets below the title.
    /// At least one suggestion is expected, but an empty list still
    /// renders cleanly (just the title).
    pub suggestions: Vec<String>,
}

impl Advice {
    /// Emit the advice on stdout in the canonical Marshal bullet
    /// format, matching the indentation used by error hints.
    pub fn render_to_stdout(&self) {
        println!("{}", self.title);
        for line in &self.suggestions {
            println!("  • {line}");
        }
    }
}
