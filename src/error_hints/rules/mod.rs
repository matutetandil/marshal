//! Concrete error-hint rules.
//!
//! Organised loosely by failure domain so adding "another SSH hint" or
//! "another repository-state hint" lands next to its siblings. Each
//! file contains one or more rules and their unit tests.
//!
//! Current coverage (9 of the planned ~20 hints):
//!
//! * **repository.rs** — `not a git repository`.
//! * **ownership.rs** — `detected dubious ownership`.
//! * **ssh.rs** — `Permission denied (publickey)`.
//! * **push.rs** — non-fast-forward push rejection.
//! * **working_tree.rs** — `local changes would be overwritten`.
//! * **merge.rs** — `refusing to merge unrelated histories`.
//! * **pathspec.rs** — `pathspec '…' did not match any file`.
//! * **refs.rs** — `ambiguous argument: unknown revision`.
//! * **setup.rs** — empty author identity / `Author identity unknown`.

use super::Registry;

mod merge;
mod ownership;
mod pathspec;
mod push;
mod refs;
mod repository;
mod setup;
mod ssh;
mod working_tree;

/// Register the canonical hint rules with `registry`. Order is the search
/// order; when patterns overlap, the more specific rule must be
/// registered first so first-match-wins picks the narrower hit. The
/// current set is mutually exclusive (each rule keys off a distinct
/// substring or subcommand), so order is not yet load-bearing.
pub fn register_defaults(registry: &mut Registry) {
    registry.register(Box::new(repository::NotAGitRepository));
    registry.register(Box::new(ownership::DubiousOwnership));
    registry.register(Box::new(ssh::PublicKeyDenied));
    registry.register(Box::new(push::PushNonFastForward));
    registry.register(Box::new(working_tree::LocalChangesWouldBeOverwritten));
    registry.register(Box::new(merge::UnrelatedHistories));
    registry.register(Box::new(pathspec::PathspecNoMatch));
    registry.register(Box::new(refs::AmbiguousArgument));
    registry.register(Box::new(setup::EmptyIdent));
}
