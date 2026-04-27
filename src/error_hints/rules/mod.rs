//! Concrete error-hint rules.
//!
//! Organised loosely by failure domain so adding "another SSH hint" or
//! "another repository-state hint" lands next to its siblings. Each
//! file contains one or more rules and their unit tests.
//!
//! Current coverage (3 of the planned ~20 hints):
//!
//! * **repository.rs** — `not a git repository`.
//! * **ownership.rs** — `detected dubious ownership`.
//! * **ssh.rs** — `Permission denied (publickey)`.

use super::Registry;

mod ownership;
mod repository;
mod ssh;

/// Register the canonical hint rules with `registry`. Order is the search
/// order; when patterns overlap, the more specific rule must be
/// registered first so first-match-wins picks the narrower hit. The
/// current set is mutually exclusive (each rule keys off a distinct
/// substring), so order is not yet load-bearing.
pub fn register_defaults(registry: &mut Registry) {
    registry.register(Box::new(repository::NotAGitRepository));
    registry.register(Box::new(ownership::DubiousOwnership));
    registry.register(Box::new(ssh::PublicKeyDenied));
}
