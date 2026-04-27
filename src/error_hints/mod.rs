//! Actionable error hints — Strategy registry.
//!
//! When git exits non-zero, `main` consults this registry over the
//! captured stderr and the parsed invocation. The first matching rule's
//! [`Hint`] is appended to stderr after git's own output, namespaced as
//! `marshal: hint: …` so the augmentation is always recognisable.
//!
//! Hints are gated by `errors.actionable_hints` — disabling the flag
//! turns off both stderr capture (in `main`) and the registry walk, so
//! the user gets back unaltered passthrough.
//!
//! SOLID applied (mirrors the `modernize/` registry):
//! * **SRP** — one rule per failure shape; the registry aggregates.
//! * **OCP** — adding rule N+1 is `impl ErrorHintRule` + one line in
//!   `register_defaults`. No existing code changes.
//! * **LSP** — any rule is interchangeable through the trait.
//! * **DIP** — `main` depends on the registry, never on concrete rules.

pub mod rule;
pub mod rules;

pub use rule::{ErrorHintRule, Hint, HintContext};

pub struct Registry {
    rules: Vec<Box<dyn ErrorHintRule>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Append `rule` to the registry. Called by
    /// [`rules::register_defaults`] once concrete rules ship.
    #[allow(dead_code)] // Consumed by `register_defaults` from the next step on.
    pub fn register(&mut self, rule: Box<dyn ErrorHintRule>) {
        self.rules.push(rule);
    }

    /// Ask every rule in registration order; return the first matching
    /// hint. `None` when no rule fires (or when the registry is empty,
    /// as in this step).
    pub fn first_hint(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        self.rules.iter().find_map(|r| r.examine(ctx))
    }
}

impl Default for Registry {
    /// The registry seeded with the canonical Marshal hint rules. All of
    /// `main`'s production paths use this.
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

    // Test doubles so we can exercise the registry plumbing without
    // depending on the production rules (which arrive in the next step).
    struct AlwaysMatches;
    impl ErrorHintRule for AlwaysMatches {
        fn examine(&self, _ctx: &HintContext<'_>) -> Option<Hint> {
            Some(Hint {
                rule_id: "test-always",
                title: "matched".to_string(),
                actions: vec!["do the thing".to_string()],
            })
        }
    }

    struct NeverMatches;
    impl ErrorHintRule for NeverMatches {
        fn examine(&self, _ctx: &HintContext<'_>) -> Option<Hint> {
            None
        }
    }

    fn ctx_for<'a>(stderr: &'a str, parsed: &'a crate::git::parser::ParsedGitInvocation) -> HintContext<'a> {
        HintContext {
            stderr,
            parsed,
            exit_code: 128,
        }
    }

    #[test]
    fn empty_registry_yields_no_hint() {
        let reg = Registry::new();
        let parsed = parse(&[]);
        assert!(reg.first_hint(&ctx_for("", &parsed)).is_none());
    }

    #[test]
    fn default_registry_starts_empty_until_rules_ship() {
        // Guards that `register_defaults` does not silently start firing
        // hints before rule modules land. When the first batch arrives,
        // this test is rewritten to assert specific rule_ids fire.
        let reg = Registry::default();
        let parsed = parse(&[]);
        assert!(reg.first_hint(&ctx_for("anything", &parsed)).is_none());
    }

    #[test]
    fn matching_rule_produces_hint() {
        let mut reg = Registry::new();
        reg.register(Box::new(AlwaysMatches));
        let parsed = parse(&[]);
        let hint = reg.first_hint(&ctx_for("", &parsed)).expect("matches");
        assert_eq!(hint.rule_id, "test-always");
        assert_eq!(hint.actions, vec!["do the thing".to_string()]);
    }

    #[test]
    fn non_matching_rule_is_skipped() {
        let mut reg = Registry::new();
        reg.register(Box::new(NeverMatches));
        let parsed = parse(&[]);
        assert!(reg.first_hint(&ctx_for("", &parsed)).is_none());
    }

    #[test]
    fn first_matching_rule_wins() {
        let mut reg = Registry::new();
        reg.register(Box::new(NeverMatches));
        reg.register(Box::new(AlwaysMatches));
        let parsed = parse(&[]);
        let hint = reg.first_hint(&ctx_for("", &parsed)).expect("one matches");
        assert_eq!(hint.rule_id, "test-always");
    }
}
