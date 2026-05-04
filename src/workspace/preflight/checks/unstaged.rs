use crate::git::porcelain::RepoState;
use crate::workspace::preflight::{Obstacle, PreflightCheck};

/// Detects working-tree modifications not yet staged. Soft
/// blocker — same resolution flags as [`StagedChangesCheck`].
pub struct UnstagedChangesCheck;

impl PreflightCheck for UnstagedChangesCheck {
    fn detect(&self, state: &RepoState) -> Option<Obstacle> {
        if state.working_tree.unstaged > 0 {
            Some(Obstacle::UnstagedChanges {
                count: state.working_tree.unstaged,
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
    fn zero_unstaged_emits_nothing() {
        assert!(UnstagedChangesCheck.detect(&clean_state()).is_none());
    }

    #[test]
    fn unstaged_emits_obstacle_with_count() {
        let mut s = clean_state();
        s.working_tree.unstaged = 7;
        assert_eq!(
            UnstagedChangesCheck.detect(&s),
            Some(Obstacle::UnstagedChanges { count: 7 })
        );
    }
}
