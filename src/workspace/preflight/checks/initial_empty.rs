use crate::git::porcelain::RepoState;
use crate::workspace::preflight::{Obstacle, PreflightCheck};

/// Detects an initial-empty repo (no commits yet). Hard blocker
/// for restore/switch — there is nothing to switch from. The user
/// has to make a first commit manually.
pub struct InitialEmptyCheck;

impl PreflightCheck for InitialEmptyCheck {
    fn detect(&self, state: &RepoState) -> Option<Obstacle> {
        if state.branch.is_initial {
            Some(Obstacle::InitialEmpty)
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
    fn non_initial_branch_emits_nothing() {
        assert!(InitialEmptyCheck.detect(&clean_state()).is_none());
    }

    #[test]
    fn initial_branch_emits_obstacle() {
        let mut s = clean_state();
        s.branch.is_initial = true;
        s.branch.oid = None;
        assert_eq!(InitialEmptyCheck.detect(&s), Some(Obstacle::InitialEmpty));
    }
}
