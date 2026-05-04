//! Pre-flight checks for write-side workspace operations.
//!
//! Phase 3 commands that mutate a child repo's working tree
//! (`ws restore`, `ws switch`, soon `ws restore --all`) all need
//! to detect the same set of "are we safe to proceed?" conditions
//! before doing anything. This module hosts the obstacle taxonomy
//! and the [`Registry`] of checks that produce them.
//!
//! Two tiers of obstacles:
//!
//! * **Hard blockers** — conditions that no flag can resolve at
//!   the time of invocation. The user has to abort/complete the
//!   in-progress operation, resolve the conflicts, or make a
//!   first commit themselves. Examples: an active rebase, merge
//!   conflicts in the working tree, an initial-empty repo with
//!   no commits to switch from.
//!
//! * **Soft blockers** — uncommitted local changes (staged,
//!   unstaged, untracked). Refused by default (Invariant 8:
//!   Conservative Defaults — never silently destroy work), but
//!   the user can opt in to `--auto-stash` or `--discard-changes`
//!   to resolve them.
//!
//! Strategy + Registry per Invariant 10. Adding a new obstacle is
//! `impl PreflightCheck` for the new type + one registration line
//! in [`checks::register_defaults`]; nothing else changes.

mod check;
mod checks;

pub use check::{Obstacle, PreflightCheck};

use crate::git::porcelain::RepoState;

/// A composed sequence of [`PreflightCheck`] implementations. Each
/// check is invoked in registration order; the resulting
/// `Vec<Obstacle>` preserves that order, which is meaningful (the
/// renderer surfaces in-progress operations first because they
/// dominate the user's mental model).
pub struct Registry {
    checks: Vec<Box<dyn PreflightCheck>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
        }
    }

    pub fn register<C: PreflightCheck + 'static>(&mut self, check: C) -> &mut Self {
        self.checks.push(Box::new(check));
        self
    }

    pub fn detect_all(&self, state: &RepoState) -> Vec<Obstacle> {
        self.checks.iter().filter_map(|c| c.detect(state)).collect()
    }
}

impl Default for Registry {
    fn default() -> Self {
        let mut reg = Self::new();
        checks::register_defaults(&mut reg);
        reg
    }
}

/// Detect every obstacle present in `state` using the default
/// registry. The result is in rendering order: in-progress ops
/// first, then conflicts, then initial-empty, then the soft
/// blockers in `staged → unstaged → untracked` order.
pub fn obstacles(state: &RepoState) -> Vec<Obstacle> {
    Registry::default().detect_all(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::{BranchInfo, InProgressOp, WorkingTreeInfo};

    fn clean_state() -> RepoState {
        RepoState {
            branch: BranchInfo {
                name: Some("main".to_string()),
                oid: Some("abc".to_string()),
                ..Default::default()
            },
            working_tree: WorkingTreeInfo::default(),
            in_progress: InProgressOp::None,
        }
    }

    #[test]
    fn clean_state_has_no_obstacles() {
        assert!(obstacles(&clean_state()).is_empty());
    }

    #[test]
    fn detects_in_progress_rebase_as_hard_blocker() {
        let mut s = clean_state();
        s.in_progress = InProgressOp::Rebase;
        let o = obstacles(&s);
        assert_eq!(o.len(), 1);
        assert!(o[0].is_hard_blocker());
        assert_eq!(o[0].description(), "rebase in progress");
    }

    #[test]
    fn detects_conflicts_as_hard_blocker() {
        let mut s = clean_state();
        s.working_tree.unmerged = 3;
        let o = obstacles(&s);
        assert_eq!(o.len(), 1);
        assert!(o[0].is_hard_blocker());
        assert!(o[0].description().contains("3 conflicts"));
    }

    #[test]
    fn detects_initial_empty_as_hard_blocker() {
        let mut s = clean_state();
        s.branch.is_initial = true;
        s.branch.oid = None;
        let o = obstacles(&s);
        assert_eq!(o.len(), 1);
        assert!(o[0].is_hard_blocker());
    }

    #[test]
    fn detects_uncommitted_changes_as_soft_blockers() {
        let mut s = clean_state();
        s.working_tree.staged = 2;
        s.working_tree.unstaged = 1;
        s.working_tree.untracked = 4;
        let o = obstacles(&s);
        assert_eq!(o.len(), 3);
        for ob in &o {
            assert!(!ob.is_hard_blocker());
            assert!(ob.cleared_by_auto_stash());
            assert!(ob.cleared_by_discard());
        }
    }

    #[test]
    fn ordering_puts_in_progress_first() {
        let mut s = clean_state();
        s.in_progress = InProgressOp::Merge;
        s.working_tree.unstaged = 1;
        let o = obstacles(&s);
        assert_eq!(o.len(), 2);
        assert!(matches!(o[0], Obstacle::InProgress { .. }));
        assert!(matches!(o[1], Obstacle::UnstagedChanges { .. }));
    }

    #[test]
    fn singular_plural_descriptions() {
        assert_eq!(
            Obstacle::StagedChanges { count: 1 }.description(),
            "1 staged change"
        );
        assert_eq!(
            Obstacle::StagedChanges { count: 2 }.description(),
            "2 staged changes"
        );
        assert_eq!(
            Obstacle::UntrackedFiles { count: 1 }.description(),
            "1 untracked file"
        );
        assert_eq!(
            Obstacle::UntrackedFiles { count: 7 }.description(),
            "7 untracked files"
        );
    }

    #[test]
    fn registry_supports_custom_composition() {
        // Hand-roll a Registry with a synthetic check to confirm
        // composition is mechanical: any `PreflightCheck` impl plus
        // one `register` call is the entire surface for adding a
        // member. No modification of existing impls or dispatch.
        struct AlwaysFlagsInitialEmpty;
        impl PreflightCheck for AlwaysFlagsInitialEmpty {
            fn detect(&self, _: &RepoState) -> Option<Obstacle> {
                Some(Obstacle::InitialEmpty)
            }
        }
        let mut reg = Registry::new();
        reg.register(AlwaysFlagsInitialEmpty);
        let o = reg.detect_all(&clean_state());
        assert_eq!(o, vec![Obstacle::InitialEmpty]);
    }
}
