//! The Strategy interface for modernization rules.
//!
//! Each rule inspects a [`ParsedGitInvocation`] and, if it recognises a legacy
//! form it knows about, returns a [`RuleOpinion`] carrying both a user-facing
//! [`Suggestion`] and the argv the rule would rewrite the invocation to (for
//! opt-in rewrite mode in a later step).
//!
//! Rules are deliberately small — one legacy→modern mapping per rule (SRP).
//! Adding a new modernization means implementing this trait and registering
//! in [`super::rules::register_defaults`].

use crate::git::parser::ParsedGitInvocation;
use std::ffi::OsString;

/// Strategy: inspect a Git invocation and decide whether it has a modern
/// equivalent worth suggesting.
///
/// Rule identity lives on [`Suggestion::rule_id`] (a `&'static str`
/// kebab-case key, e.g. `"checkout-create-branch"`). Carrying it on the
/// suggestion rather than as a trait method keeps the trait minimal in step
/// 3 and lets the id ride along with its data. A `fn id(&self)` can be added
/// on the trait later (step 5 needs it for config-based rule opt-outs) if
/// that proves useful.
pub trait ModernizationRule: Send + Sync {
    /// Examine the invocation. `Some(opinion)` when the legacy form matches,
    /// `None` otherwise. This runs on every Marshal invocation — rules must
    /// be fast and allocate only when they match.
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion>;
}

/// What a matching rule says about an invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleOpinion {
    pub suggestion: Suggestion,

    /// The argv to forward when rewrite mode is enabled. `Some` for rules
    /// with a safe 1:1 mapping (all 12 canonical rules in step 4 qualify);
    /// `None` reserved for future rules that can *detect* a legacy form but
    /// cannot mechanically translate it (e.g. ambiguous user intent).
    pub rewrite: Option<Vec<OsString>>,
}

/// A single, human-readable suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub rule_id: &'static str,

    /// A printable rendering of the command the user actually typed, enough
    /// for the user to see themselves in the tip.
    pub legacy_form: String,

    /// The modern equivalent, in the same form as `legacy_form`.
    pub modern_form: String,

    /// Optional one-line extra context. Most rules leave this `None`; use it
    /// when the substitution is subtle enough that a short explanation helps.
    pub note: Option<&'static str>,
}

impl Suggestion {
    /// Emit the tip on stderr in the canonical Marshal format. All rules
    /// route through here so tone and format stay consistent.
    pub fn emit_to_stderr(&self) {
        eprintln!(
            "marshal: tip: try `{}` instead of `{}`",
            self.modern_form, self.legacy_form
        );
        if let Some(note) = self.note {
            eprintln!("             {note}");
        }
    }
}
