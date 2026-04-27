//! Concrete advice rules for `marshal what-now`.
//!
//! Registration order is the search order. Most specific situation
//! first; the catch-all `clean` rule lands last so every repo state
//! produces some advice.
//!
//! Current set (9 rules, by priority):
//!
//!   1. `merge-conflict` — unresolved conflicts block everything else.
//!   2. `*-in-progress` (rebase / cherry-pick / revert / bisect /
//!      paused-merge) — git is waiting on the user to finish or
//!      abort. One rule, branches by op.
//!   3. `initial-state` — `git init` happened, no commits yet.
//!   4. `detached-head` — HEAD not on a branch; new commits would be
//!      unreachable after a switch.
//!   5. `uncommitted-changes` — staged/unstaged/untracked, no
//!      conflicts, no in-progress op.
//!   6. `diverged` — both ahead and behind upstream.
//!   7. `behind-upstream` — behind only.
//!   8. `unpushed-commits` / `unpushed-commits-no-upstream` —
//!      ahead only, with branch shape varying by upstream presence.
//!   9. `clean` — catch-all, always returns advice.

use super::Registry;

mod branch_state;
mod clean;
mod conflict;
mod in_progress;
mod sync;
mod uncommitted;

/// Register the canonical advice rules with `registry` in priority
/// order.
pub fn register_defaults(registry: &mut Registry) {
    registry.register(Box::new(conflict::MergeConflict));
    registry.register(Box::new(in_progress::InProgressOperation));
    registry.register(Box::new(branch_state::InitialState));
    registry.register(Box::new(branch_state::DetachedHead));
    registry.register(Box::new(uncommitted::UncommittedChanges));
    registry.register(Box::new(sync::Diverged));
    registry.register(Box::new(sync::Behind));
    registry.register(Box::new(sync::Ahead));
    registry.register(Box::new(clean::Clean));
}
