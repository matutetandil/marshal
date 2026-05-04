use crate::git::porcelain::RepoState;
use crate::workspace::preflight::{Obstacle, PreflightCheck};

/// Detects working-tree files staged for commit. Soft blocker —
/// resolved by `--auto-stash` (preserve via `git stash push
/// --include-untracked`) or `--discard-changes` (destructive
/// `git reset --hard`).
pub struct StagedChangesCheck;

impl PreflightCheck for StagedChangesCheck {
    fn detect(&self, state: &RepoState) -> Option<Obstacle> {
        if state.working_tree.staged > 0 {
            Some(Obstacle::StagedChanges {
                count: state.working_tree.staged,
            })
        } else {
            None
        }
    }
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
    fn zero_staged_emits_nothing() {
        assert!(StagedChangesCheck.detect(&clean_state()).is_none());
    }

    #[test]
    fn staged_emits_obstacle_with_count() {
        let mut s = clean_state();
        s.working_tree.staged = 2;
        assert_eq!(
            StagedChangesCheck.detect(&s),
            Some(Obstacle::StagedChanges { count: 2 })
        );
    }
}
