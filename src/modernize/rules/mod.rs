//! Concrete modernization rules.
//!
//! Organised by the Git subcommand each family operates on. The registry
//! order is "most specific first" — a rule that matches a strictly smaller
//! class of invocations is registered before the rule that matches its
//! superset, so the first-match-wins semantics of [`super::Registry`] is
//! deterministic.
//!
//! Current coverage (12 canonical Git modernizations, 11 rule impls —
//! `stash save` and `stash save -u` are one rule that preserves the `-u`
//! flag through the rewrite):
//!
//! * **checkout** (8 impls, `checkout.rs`) — the split of `git checkout` into
//!   `git switch` + `git restore` in Git 2.23.
//! * **reset** (1 impl, `reset.rs`) — the file-mode split into `restore
//!   --staged`.
//! * **stash** (1 impl, `stash.rs`) — the deprecation of `stash save` in
//!   favour of `stash push` (Git 2.16).
//! * **remote** (1 impl, `remote.rs`) — the deprecation of `remote rm` in
//!   favour of `remote remove`.

use super::Registry;

mod checkout;
mod remote;
mod reset;
mod stash;

/// Register the full canonical rule set with `registry`. Ordering matters:
/// more specific patterns are inserted before their supersets, so that
/// `Registry::first_opinion` picks the narrowest matching rule.
pub fn register_defaults(registry: &mut Registry) {
    // Checkout family — ordered specific→general so the catch-all branch
    // rule is considered last.
    registry.register(Box::new(checkout::CheckoutCreateBranch));
    registry.register(Box::new(checkout::CheckoutCreateBranchForce));
    registry.register(Box::new(checkout::CheckoutOrphan));
    registry.register(Box::new(checkout::CheckoutDetach));
    registry.register(Box::new(checkout::CheckoutRestoreFromCommit));
    registry.register(Box::new(checkout::CheckoutRestoreFromHead));
    registry.register(Box::new(checkout::CheckoutRestoreFile));
    registry.register(Box::new(checkout::CheckoutSwitchBranch));

    // Other families — each has a single rule.
    registry.register(Box::new(reset::ResetRestoreStaged));
    registry.register(Box::new(stash::StashSaveToPush));
    registry.register(Box::new(remote::RemoteRmToRemove));
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::git::parser::{parse, ParsedGitInvocation};
    use std::ffi::OsString;

    /// Terse builder for rule tests.
    pub fn parsed(argv: &[&str]) -> ParsedGitInvocation {
        let v: Vec<OsString> = argv.iter().map(OsString::from).collect();
        parse(&v)
    }

    pub fn osv(strs: &[&str]) -> Vec<OsString> {
        strs.iter().map(OsString::from).collect()
    }
}
