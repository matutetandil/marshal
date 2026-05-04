use crate::git::porcelain::RepoState;
use crate::workspace::preflight::{Obstacle, PreflightCheck};

/// Detects unresolved merge conflicts in the working tree. Hard
/// blocker — independent of an in-progress op, since a manual
/// `git checkout --merge` can leave conflicts behind without one.
pub struct ConflictsCheck;

impl PreflightCheck for ConflictsCheck {
    fn detect(&self, state: &RepoState) -> Option<Obstacle> {
        if state.working_tree.unmerged > 0 {
            Some(Obstacle::Conflicts {
                count: state.working_tree.unmerged,
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
    fn zero_unmerged_emits_nothing() {
        assert!(ConflictsCheck.detect(&clean_state()).is_none());
    }

    #[test]
    fn unmerged_emits_obstacle_with_count() {
        let mut s = clean_state();
        s.working_tree.unmerged = 4;
        assert_eq!(
            ConflictsCheck.detect(&s),
            Some(Obstacle::Conflicts { count: 4 })
        );
    }
}
