//! Concrete help topics.
//!
//! Each file is one topic. Adding a topic = `impl HelpTopic` +
//! one line in [`register_defaults`] (Invariant 10).
//!
//! Current set:
//!   * `overview`  — the `marshal help` (no arg) landing screen,
//!     context-aware (in-repo vs outside).
//!   * `config`    — the configuration system in depth.
//!   * `hints`     — actionable error hints reference.
//!   * `modernize` — modernization tips reference.
//!   * `what-now`  — the what-now command in detail.

use super::Registry;

mod config;
mod hints;
mod modernize;
mod overview;
mod what_now;

/// Register every canonical help topic with `registry`. Order does
/// not matter for help (lookups are by name, not first-match), but
/// keeping it stable makes the topic list deterministic.
pub fn register_defaults(registry: &mut Registry) {
    registry.register(Box::new(overview::Overview));
    registry.register(Box::new(config::Config));
    registry.register(Box::new(hints::Hints));
    registry.register(Box::new(modernize::Modernize));
    registry.register(Box::new(what_now::WhatNow));
}
