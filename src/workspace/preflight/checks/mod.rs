//! Default pre-flight check implementations.
//!
//! Each submodule houses one [`PreflightCheck`](super::PreflightCheck)
//! impl. [`register_defaults`] composes them in rendering order:
//! in-progress operations first (they dominate the user's mental
//! model — a rebase active means nothing else matters until they
//! handle it), then conflicts, then initial-empty, then the soft
//! blockers in `staged → unstaged → untracked` order.
//!
//! Adding a new check: drop a new file in this directory with an
//! `impl PreflightCheck`, then add one `.register(...)` line below.

mod conflicts;
mod in_progress;
mod initial_empty;
mod staged;
mod unstaged;
mod untracked;

use super::Registry;

pub fn register_defaults(registry: &mut Registry) {
    registry
        .register(in_progress::InProgressCheck)
        .register(conflicts::ConflictsCheck)
        .register(initial_empty::InitialEmptyCheck)
        .register(staged::StagedChangesCheck)
        .register(unstaged::UnstagedChangesCheck)
        .register(untracked::UntrackedFilesCheck);
}
