//! Pre-flight checks for write-side workspace operations.
//!
//! Phase 3 commands that mutate a child repo's working tree
//! (`ws restore`, future `ws switch`, `ws branch`) all need to
//! detect the same set of "are we safe to proceed?" conditions
//! before doing anything. This module hosts the obstacle taxonomy
//! and the per-state classification function.
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
//! Slice H of Phase 3 will refactor this into a `PreflightCheck`
//! Strategy + Registry shared by every coordinated multi-repo
//! operation. For now the per-state function is enough — `ws
//! restore` is the single consumer.

use serde::Serialize;

use crate::git::porcelain::{InProgressOp, RepoState};

/// One obstacle that prevents a clean restore. Carries enough
/// context for the renderer to compose a human-readable message
/// without having to re-inspect the [`RepoState`]. Tagged-enum
/// JSON shape (`{ "kind": "...", ... }`) for single-switch
/// consumption — same precedent as `ws diff`'s `StateChange` and
/// `ws clone`'s `ChildResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Obstacle {
    /// A git operation is in progress (merge, rebase, etc).
    /// Resolution is the user's: abort or complete the op.
    InProgress { op: InProgressOp },
    /// Merge conflicts present in the working tree. Independent
    /// of an in-progress op (a manual `git checkout --merge` can
    /// leave conflicts without one).
    Conflicts { count: usize },
    /// Working-tree files staged for commit. Soft blocker.
    StagedChanges { count: usize },
    /// Working-tree modifications not yet staged. Soft blocker.
    UnstagedChanges { count: usize },
    /// Untracked files in the working tree. Soft blocker.
    UntrackedFiles { count: usize },
    /// Repository has no commits yet — nothing to restore from
    /// or to. Hard blocker that requires a first commit.
    InitialEmpty,
}

impl Obstacle {
    /// Human-friendly one-line description, suitable for inclusion
    /// in an error message body or a `--explain` plan step.
    pub fn description(&self) -> String {
        let plural = |n: usize, singular: &str, plural: &str| -> String {
            if n == 1 {
                format!("{n} {singular}")
            } else {
                format!("{n} {plural}")
            }
        };
        match self {
            Self::InProgress {
                op: InProgressOp::Merge,
            } => "merge in progress".to_string(),
            Self::InProgress {
                op: InProgressOp::Rebase,
            } => "rebase in progress".to_string(),
            Self::InProgress {
                op: InProgressOp::CherryPick,
            } => "cherry-pick in progress".to_string(),
            Self::InProgress {
                op: InProgressOp::Revert,
            } => "revert in progress".to_string(),
            Self::InProgress {
                op: InProgressOp::Bisect,
            } => "bisect in progress".to_string(),
            // Defensive — `obstacles()` only emits InProgress when
            // the op is active, but the type itself permits None.
            Self::InProgress {
                op: InProgressOp::None,
            } => "in-progress operation".to_string(),
            Self::Conflicts { count } => {
                format!("{} unresolved", plural(*count, "conflict", "conflicts"))
            }
            Self::StagedChanges { count } => plural(*count, "staged change", "staged changes"),
            Self::UnstagedChanges { count } => {
                plural(*count, "unstaged change", "unstaged changes")
            }
            Self::UntrackedFiles { count } => plural(*count, "untracked file", "untracked files"),
            Self::InitialEmpty => "no commits in this repo yet".to_string(),
        }
    }

    /// `true` for obstacles that cannot be cleared by any flag
    /// available at invocation time. Any hard blocker present in
    /// the obstacle list aborts the operation immediately, before
    /// `--auto-stash` or `--discard-changes` are even considered.
    pub fn is_hard_blocker(&self) -> bool {
        matches!(
            self,
            Self::InProgress { .. } | Self::Conflicts { .. } | Self::InitialEmpty
        )
    }

    /// `true` when `--auto-stash` would resolve this obstacle by
    /// `git stash push --include-untracked`.
    pub fn cleared_by_auto_stash(&self) -> bool {
        matches!(
            self,
            Self::StagedChanges { .. } | Self::UnstagedChanges { .. } | Self::UntrackedFiles { .. }
        )
    }

    /// `true` when `--discard-changes` would resolve this obstacle
    /// by `git reset --hard` + `git clean -fd`. Same set as
    /// `cleared_by_auto_stash` for now — the difference is intent
    /// (preserve vs destroy), not which obstacles each flag
    /// addresses.
    pub fn cleared_by_discard(&self) -> bool {
        matches!(
            self,
            Self::StagedChanges { .. } | Self::UnstagedChanges { .. } | Self::UntrackedFiles { .. }
        )
    }
}

/// Compute every obstacle present in `state`. The result is in
/// rendering order (in-progress first because it dominates the
/// user's mental model: if a rebase is active, nothing else
/// matters until they handle it).
pub fn obstacles(state: &RepoState) -> Vec<Obstacle> {
    let mut out = Vec::new();
    if state.in_progress.is_active() {
        out.push(Obstacle::InProgress {
            op: state.in_progress,
        });
    }
    if state.working_tree.unmerged > 0 {
        out.push(Obstacle::Conflicts {
            count: state.working_tree.unmerged,
        });
    }
    if state.branch.is_initial {
        out.push(Obstacle::InitialEmpty);
    }
    if state.working_tree.staged > 0 {
        out.push(Obstacle::StagedChanges {
            count: state.working_tree.staged,
        });
    }
    if state.working_tree.unstaged > 0 {
        out.push(Obstacle::UnstagedChanges {
            count: state.working_tree.unstaged,
        });
    }
    if state.working_tree.untracked > 0 {
        out.push(Obstacle::UntrackedFiles {
            count: state.working_tree.untracked,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::{BranchInfo, WorkingTreeInfo};

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
}
