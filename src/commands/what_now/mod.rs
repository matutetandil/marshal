//! `marshal what-now` — analyse repo state and suggest the next action.
//!
//! Reactive counterpart to the actionable error hints in
//! `crate::error_hints`: instead of waiting for a failed git command
//! and matching its stderr, `what-now` reads the cold state of the
//! current repository (branch identity, remote relationship, working
//! tree counters, ongoing multi-step operations) and recommends a
//! concrete next step the user can take.
//!
//! The module ships incrementally:
//!
//!   * step W1 — `state.rs`: the [`state::RepoState`] snapshot.
//!   * step W2 — `rule.rs` + `Registry`: Strategy plumbing (this commit).
//!   * step W3 — `rules/`: the canonical rules (merge-conflict,
//!     uncommitted-changes, unpushed-commits, …).
//!   * step W4 — `cli.rs` wiring: `marshal what-now` reachable end-to-end.
//!
//! SOLID applied (mirrors `error_hints/` and `modernize/`):
//! * **SRP** — one rule per situation; the registry aggregates;
//!   `state.rs` extracts state; `rule.rs` defines the contract.
//! * **OCP** — adding rule N+1 is `impl AdviceRule` + one line in
//!   `register_defaults`. No existing code changes.
//! * **DIP** — the entry point depends on the registry trait, never
//!   on concrete rules.

#![allow(dead_code)] // Entry point and registry are wired up in W4 (CLI dispatch).

pub mod rule;
pub mod rules;
pub mod state;

pub use rule::{Advice, AdviceRule};

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
    pub fn first_advice(&self, state: &state::RepoState) -> Option<Advice> {
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
    use super::state::RepoState;
    use super::*;

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
    fn default_registry_starts_empty_until_rules_ship() {
        // Guards that `register_defaults` does not silently start
        // returning advice before rule modules land. When the first
        // rules arrive, this test is rewritten to assert specific
        // rule_ids fire.
        let reg = Registry::default();
        assert!(reg.first_advice(&RepoState::default()).is_none());
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
