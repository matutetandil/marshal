//! Concrete advice rules.
//!
//! Empty in this step; the canonical rules ship in W3. The function
//! exists so the registry has a `register_defaults` symbol to call
//! from day one — adding rules then becomes "implement
//! `AdviceRule` + one line here".
//!
//! Ordering: registration order is the search order. Most specific
//! situation first; the catch-all `clean` rule lands last so every
//! repo state produces some advice.

use super::Registry;

pub fn register_defaults(_registry: &mut Registry) {
    // Intentionally empty — rules are added in the next step.
}
