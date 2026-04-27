//! Concrete help topics.
//!
//! Each file is one topic. Adding a topic = `impl HelpTopic` +
//! one line in [`register_defaults`] (Invariant 10).
//!
//! Current set:
//!   * `overview` — the `marshal help` (no arg) landing screen.
//!
//! H2 adds: `config`, `hints`, `modernize`, `what-now`.

use super::Registry;

mod overview;

/// Register every canonical help topic with `registry`. Order does
/// not matter for help (lookups are by name, not first-match), but
/// keeping it stable makes the topic list deterministic.
pub fn register_defaults(registry: &mut Registry) {
    registry.register(Box::new(overview::Overview));
}
