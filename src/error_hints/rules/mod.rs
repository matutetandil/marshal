//! Concrete error-hint rules.
//!
//! The first batch ships in the next step of the 0.3.0 cycle, covering
//! `not a git repository`, `dubious ownership`, and SSH key issues. This
//! module exists so the registry has an `register_defaults` symbol to
//! call from day one — adding a rule then becomes "implement
//! `ErrorHintRule` + one line here".
//!
//! Ordering: registration order is the search order. As with
//! modernization rules, when patterns overlap the more specific rule
//! must be registered first so first-match-wins picks the narrower hit.

use super::Registry;

/// Register the canonical hint rules with `registry`. Empty in this
/// step; rule files arrive next.
pub fn register_defaults(_registry: &mut Registry) {
    // Intentionally empty — rules are added in the next step of the
    // 0.3.0 cycle.
}
