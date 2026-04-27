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
//!   * step W1 — `state.rs`: the [`state::RepoState`] snapshot (this commit).
//!   * step W2 — `rule.rs` + `Registry`: Strategy plumbing for advice rules.
//!   * step W3 — `rules/`: the canonical rules (merge-conflict,
//!     uncommitted-changes, unpushed-commits, …).
//!   * step W4 — `cli.rs` wiring: `marshal what-now` reachable end-to-end.
//!
//! Silenced for now — the entry point lands in W4 alongside the CLI dispatch.

#![allow(dead_code)]

pub mod state;
