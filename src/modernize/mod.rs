//! Modernization rules — Strategy registry.
//!
//! The registry aggregates [`ModernizationRule`] strategies. `main` consults
//! it once per invocation, after routing: if any rule claims the invocation
//! is legacy, its suggestion is emitted to stderr before Git runs.
//!
//! Behavior is "first opinion wins". The canonical rules registered by
//! [`rules::register_defaults`] are mutually exclusive by construction
//! (they match disjoint subcommand patterns), so this ordering is safe.
//!
//! SOLID applied:
//! * **SRP** — each rule = one legacy→modern mapping; the registry
//!   aggregates; the parser parses; nothing mixes responsibilities.
//! * **OCP** — adding rule N+1 is `impl ModernizationRule` + one line in
//!   `register_defaults`. No existing code changes.
//! * **LSP** — any rule is interchangeable; the trait's contract is uniform.
//! * **ISP** — the trait has two methods, both load-bearing.
//! * **DIP** — the registry (and therefore `main`) depends on the trait, not
//!   on concrete rules.

pub mod rule;
pub mod rules;

use crate::git::parser::ParsedGitInvocation;
use rule::{ModernizationRule, RuleOpinion};
use std::ffi::OsString;

pub struct Registry {
    rules: Vec<Box<dyn ModernizationRule>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the registry. Called by [`rules::register_defaults`] in
    /// step 4 when concrete rules ship. Silenced for now so the empty
    /// registry still compiles under `clippy -D warnings`.
    #[allow(dead_code)] // Consumed by `rules::register_defaults` from step 4 onward.
    pub fn register(&mut self, rule: Box<dyn ModernizationRule>) {
        self.rules.push(rule);
    }

    /// Ask every rule in registration order; return the first matching
    /// opinion. `None` when no rule fires.
    pub fn first_opinion(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        self.rules.iter().find_map(|r| r.examine(parsed))
    }

    /// Shortcut: the rewritten argv from the first matching rule, if any.
    /// Callers use this only when rewrite mode is enabled by config, which
    /// arrives in step 5. Silenced until then.
    #[allow(dead_code)] // Consumed from `main` once config gates rewrite mode in step 5.
    pub fn rewrite_argv(&self, parsed: &ParsedGitInvocation) -> Option<Vec<OsString>> {
        self.first_opinion(parsed).and_then(|op| op.rewrite)
    }
}

impl Default for Registry {
    /// The registry seeded with the canonical Marshal rules. All of `main`'s
    /// production paths use this.
    fn default() -> Self {
        let mut registry = Self::new();
        rules::register_defaults(&mut registry);
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::parser::parse;
    use rule::Suggestion;

    // Test doubles: concrete rules that deterministically match or don't,
    // so we can exercise the registry plumbing without depending on the
    // real rules (which do not exist yet in step 3).
    struct AlwaysMatches;
    impl ModernizationRule for AlwaysMatches {
        fn examine(&self, _parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
            Some(RuleOpinion {
                suggestion: Suggestion {
                    rule_id: "test-always",
                    legacy_form: "legacy".to_string(),
                    modern_form: "modern".to_string(),
                    note: None,
                },
                rewrite: Some(vec![OsString::from("modernized")]),
            })
        }
    }

    struct NeverMatches;
    impl ModernizationRule for NeverMatches {
        fn examine(&self, _parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
            None
        }
    }

    fn empty_parsed() -> ParsedGitInvocation {
        parse(&[])
    }

    #[test]
    fn empty_registry_has_no_opinion() {
        let reg = Registry::new();
        assert!(reg.first_opinion(&empty_parsed()).is_none());
    }

    #[test]
    fn default_registry_contains_the_canonical_rules() {
        // Step 4 contract: `Default` now seeds the 11 canonical rules
        // (12 patterns — `stash save` and `stash save -u` share one impl).
        // Pick a handful of representative legacy invocations to confirm
        // the registry wires them up. Per-rule matching is covered in each
        // rule module's own tests.
        let reg = Registry::default();
        assert!(
            reg.first_opinion(&parse(&[
                OsString::from("checkout"),
                OsString::from("-b"),
                OsString::from("feat"),
            ]))
            .is_some(),
            "checkout -b should match via default registry"
        );
        assert!(
            reg.first_opinion(&parse(&[
                OsString::from("stash"),
                OsString::from("save"),
                OsString::from("wip"),
            ]))
            .is_some(),
            "stash save should match via default registry"
        );
        assert!(
            reg.first_opinion(&parse(&[
                OsString::from("remote"),
                OsString::from("rm"),
                OsString::from("origin"),
            ]))
            .is_some(),
            "remote rm should match via default registry"
        );
    }

    #[test]
    fn matching_rule_produces_opinion() {
        let mut reg = Registry::new();
        reg.register(Box::new(AlwaysMatches));
        let op = reg.first_opinion(&empty_parsed()).expect("should match");
        assert_eq!(op.suggestion.rule_id, "test-always");
        assert_eq!(op.rewrite, Some(vec![OsString::from("modernized")]));
    }

    #[test]
    fn non_matching_rule_is_skipped() {
        let mut reg = Registry::new();
        reg.register(Box::new(NeverMatches));
        assert!(reg.first_opinion(&empty_parsed()).is_none());
    }

    #[test]
    fn first_matching_rule_wins() {
        let mut reg = Registry::new();
        reg.register(Box::new(NeverMatches));
        reg.register(Box::new(AlwaysMatches));
        let op = reg.first_opinion(&empty_parsed()).expect("one matches");
        assert_eq!(op.suggestion.rule_id, "test-always");
    }

    #[test]
    fn rewrite_argv_extracts_from_first_opinion() {
        let mut reg = Registry::new();
        reg.register(Box::new(AlwaysMatches));
        let rewritten = reg.rewrite_argv(&empty_parsed()).expect("rewrite");
        assert_eq!(rewritten, vec![OsString::from("modernized")]);
    }
}
