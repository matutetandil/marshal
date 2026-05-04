//! Obstacle taxonomy and the [`PreflightCheck`] trait.
//!
//! An [`Obstacle`] is one reason a write-side operation cannot
//! proceed cleanly on a given repo. Two tiers (see
//! [`Obstacle::is_hard_blocker`]) drive the resolution UX: hard
//! blockers require manual intervention; soft blockers can be
//! cleared by `--auto-stash` or `--discard-changes`.
//!
//! A [`PreflightCheck`] is one obstacle detector. Each check
//! inspects a [`RepoState`] and emits at most one obstacle. The
//! registry composes them — adding a new obstacle is a new
//! `impl PreflightCheck` plus one line in `checks::register_defaults`.

use serde::Serialize;

use crate::git::porcelain::{InProgressOp, RepoState};

/// One obstacle that prevents a clean operation. Carries enough
/// context for the renderer to compose a human-readable message
/// without having to re-inspect the [`RepoState`]. Tagged-enum
/// JSON shape (`{ "kind": "...", ... }`) for single-switch
/// consumption from machine consumers.
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

/// One pre-flight check. Stateless inspector that emits at most
/// one [`Obstacle`] for a given [`RepoState`]. The registry
/// iterates every registered check; the resulting `Vec<Obstacle>`
/// preserves registration order.
pub trait PreflightCheck: Send + Sync {
    fn detect(&self, state: &RepoState) -> Option<Obstacle>;
}
