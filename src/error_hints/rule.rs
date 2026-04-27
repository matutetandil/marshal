//! The Strategy interface for actionable error hint rules.
//!
//! Each rule inspects what git wrote to stderr together with the user's
//! invocation and the exit code, and decides whether the failure has a
//! known shape it can offer concrete remediations for. When it matches it
//! returns a [`Hint`] — a short title and a list of bullet actions the
//! user can take to recover.
//!
//! Rules are deliberately small (SRP). Adding a new hint means
//! implementing this trait and registering in
//! [`super::rules::register_defaults`]. Hints fire only when git exited
//! non-zero — successful runs never produce hints, by design.

use crate::git::parser::ParsedGitInvocation;

/// What a rule sees: the captured stderr text (lossy UTF-8 — non-UTF-8
/// bytes are replaced so rules can use plain `&str` matching), the parsed
/// invocation, and git's exit code.
///
/// `'a` ties the borrow lifetimes to the slot the registry calls from in
/// `main`. Rules treat the context as read-only.
///
/// The first batch of rules only inspects `stderr`; `parsed` and
/// `exit_code` are populated by `main` and exposed here for rules that
/// need them (the next batch covers e.g. push-specific failures, where
/// gating on `parsed.subcommand` keeps the rule from firing on similar
/// stderr from unrelated subcommands).
pub struct HintContext<'a> {
    pub stderr: &'a str,
    #[allow(dead_code)] // First read once non-stderr-only rules land in step 4.
    pub parsed: &'a ParsedGitInvocation,
    #[allow(dead_code)] // Same reason as `parsed` above.
    pub exit_code: i32,
}

/// Strategy: examine an invocation that just failed and decide whether to
/// emit a hint.
pub trait ErrorHintRule: Send + Sync {
    /// Examine the failure. `Some(hint)` when the rule recognises the
    /// shape and has actionable advice, `None` otherwise.
    ///
    /// Rules run only on git failures (exit code ≠ 0). They must be fast
    /// — substring match against the captured stderr is the typical
    /// implementation — and should allocate only when they match.
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint>;
}

/// A user-facing hint. Renders to stderr in the canonical Marshal format
/// — same `marshal: <namespace>: …` shape as modernization tips and
/// configuration warnings, so the augmentation is always recognisable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub rule_id: &'static str,

    /// One-line summary of what marshal noticed. Printed after
    /// `marshal: hint:`.
    pub title: String,

    /// Concrete next steps the user can take, rendered as bullets below
    /// the title. At least one action is expected; an empty list still
    /// renders cleanly (just the title).
    pub actions: Vec<String>,
}

impl Hint {
    /// Emit the hint on stderr in the canonical Marshal format.
    pub fn emit_to_stderr(&self) {
        eprintln!("marshal: hint: {}", self.title);
        for action in &self.actions {
            eprintln!("  • {action}");
        }
    }
}
