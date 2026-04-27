//! `marshal what-now` — analyse repo state and suggest the next action.
//!
//! Reactive counterpart to the actionable error hints in
//! `crate::error_hints`: instead of waiting for a failed git command
//! and matching its stderr, `what-now` reads the cold state of the
//! current repository (branch identity, remote relationship, working
//! tree counters, ongoing multi-step operations) and recommends a
//! concrete next step the user can take.
//!
//! State extraction lives in [`crate::git::porcelain`] (shared with
//! `ws status` from Phase 2 / Slice E). This module orchestrates:
//! a state snapshot of the cwd, the canonical rule registry, and
//! the resulting advice.
//!
//! SOLID applied (mirrors `error_hints/` and `modernize/`):
//! * **SRP** — one rule per situation; the registry aggregates;
//!   `git::porcelain` extracts state; `rule.rs` defines the contract.
//! * **OCP** — adding rule N+1 is `impl AdviceRule` + one line in
//!   `register_defaults`. No existing code changes.
//! * **DIP** — the entry point depends on the registry trait, never
//!   on concrete rules.

pub mod rule;
pub mod rules;

pub use rule::{Advice, AdviceRule};

use anyhow::{anyhow, Result};
use std::ffi::OsString;

use crate::cli::Command;

/// `Command` impl for `marshal what-now`.
///
/// Reads the current repo state once, runs it through the canonical
/// rule registry, and returns the first matching advice. Rendering
/// (human / JSON) is the dispatcher's job; this command stays
/// invariant to output format per Invariant 10.
///
/// The catch-all `clean` rule in the registry guarantees every
/// successful detection produces an advice, so the `Option` from
/// `first_advice` is converted to an `Err` only as a safety net
/// against registry-construction bugs.
pub struct WhatNow;

impl Command for WhatNow {
    type Output = Advice;

    fn run(&self, _args: &[OsString]) -> Result<Self::Output> {
        let state = crate::git::porcelain::RepoState::detect()?;
        let registry = Registry::default();
        registry
            .first_advice(&state)
            .ok_or_else(|| anyhow!("the advice registry produced no advice (this is a bug)"))
    }
}

pub struct Registry {
    rules: Vec<Box<dyn AdviceRule>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Append `rule` to the registry. Called by
    /// [`rules::register_defaults`] once concrete rules ship.
    pub fn register(&mut self, rule: Box<dyn AdviceRule>) {
        self.rules.push(rule);
    }

    /// Walk every rule in registration order; return the first
    /// matching advice. `None` only when the registry is empty —
    /// the canonical set will include a `clean` fallback so this
    /// returns `Some` for every real state.
    pub fn first_advice(&self, state: &crate::git::porcelain::RepoState) -> Option<Advice> {
        self.rules.iter().find_map(|r| r.examine(state))
    }
}

impl Default for Registry {
    /// The registry seeded with the canonical Marshal advice rules.
    /// Empty in W2; populated in W3.
    fn default() -> Self {
        let mut registry = Self::new();
        rules::register_defaults(&mut registry);
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::RepoState;

    // Test doubles so we can exercise the registry plumbing without
    // depending on the production rules (which arrive in W3).
    struct AlwaysMatches;
    impl AdviceRule for AlwaysMatches {
        fn examine(&self, _state: &RepoState) -> Option<Advice> {
            Some(Advice {
                rule_id: "test-always",
                title: "matched".to_string(),
                suggestions: vec!["do the thing".to_string()],
            })
        }
    }

    struct NeverMatches;
    impl AdviceRule for NeverMatches {
        fn examine(&self, _state: &RepoState) -> Option<Advice> {
            None
        }
    }

    #[test]
    fn empty_registry_yields_no_advice() {
        let reg = Registry::new();
        assert!(reg.first_advice(&RepoState::default()).is_none());
    }

    #[test]
    fn default_registry_dispatches_through_canonical_rules() {
        // Smoke-test that `register_defaults` wired the canonical
        // chain. Per-rule matching is covered by each rule module's
        // own tests; this asserts dispatch reaches them and the
        // priority order is respected.
        use crate::git::porcelain::{BranchInfo, InProgressOp, WorkingTreeInfo};
        let reg = Registry::default();

        // Conflicts win even when other conditions also apply.
        let s = RepoState {
            working_tree: WorkingTreeInfo {
                unmerged: 1,
                unstaged: 1,
                ..Default::default()
            },
            in_progress: InProgressOp::Rebase,
            ..Default::default()
        };
        assert_eq!(reg.first_advice(&s).unwrap().rule_id, "merge-conflict");

        // In-progress beats uncommitted-changes when no conflicts.
        let s = RepoState {
            working_tree: WorkingTreeInfo {
                unstaged: 1,
                ..Default::default()
            },
            in_progress: InProgressOp::Rebase,
            ..Default::default()
        };
        assert_eq!(reg.first_advice(&s).unwrap().rule_id, "rebase-in-progress");

        // Initial wins over uncommitted (untracked files in fresh repo).
        let s = RepoState {
            branch: BranchInfo {
                is_initial: true,
                name: Some("main".to_string()),
                ..Default::default()
            },
            working_tree: WorkingTreeInfo {
                untracked: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(reg.first_advice(&s).unwrap().rule_id, "initial-state");

        // Clean repo gets the catch-all.
        let s = RepoState {
            branch: BranchInfo {
                name: Some("main".to_string()),
                upstream: Some("origin/main".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(reg.first_advice(&s).unwrap().rule_id, "clean");
    }

    #[test]
    fn matching_rule_produces_advice() {
        let mut reg = Registry::new();
        reg.register(Box::new(AlwaysMatches));
        let advice = reg.first_advice(&RepoState::default()).expect("matches");
        assert_eq!(advice.rule_id, "test-always");
        assert_eq!(advice.suggestions, vec!["do the thing".to_string()]);
    }

    #[test]
    fn non_matching_rule_is_skipped() {
        let mut reg = Registry::new();
        reg.register(Box::new(NeverMatches));
        assert!(reg.first_advice(&RepoState::default()).is_none());
    }

    #[test]
    fn first_matching_rule_wins() {
        let mut reg = Registry::new();
        reg.register(Box::new(NeverMatches));
        reg.register(Box::new(AlwaysMatches));
        let advice = reg
            .first_advice(&RepoState::default())
            .expect("one matches");
        assert_eq!(advice.rule_id, "test-always");
    }
}
