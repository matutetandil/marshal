use crate::git::porcelain::RepoState;
use crate::workspace::preflight::{Obstacle, PreflightCheck};

/// Detects an active git operation (merge, rebase, cherry-pick,
/// revert, bisect). Hard blocker — the user has to abort or
/// complete it before any switch/restore can run.
pub struct InProgressCheck;

impl PreflightCheck for InProgressCheck {
    fn detect(&self, state: &RepoState) -> Option<Obstacle> {
        if state.in_progress.is_active() {
            Some(Obstacle::InProgress {
                op: state.in_progress,
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
    fn no_in_progress_emits_nothing() {
        assert!(InProgressCheck.detect(&clean_state()).is_none());
    }

    #[test]
    fn rebase_emits_obstacle() {
        let mut s = clean_state();
        s.in_progress = InProgressOp::Rebase;
        assert!(matches!(
            InProgressCheck.detect(&s),
            Some(Obstacle::InProgress {
                op: InProgressOp::Rebase
            })
        ));
    }

    #[test]
    fn cherry_pick_emits_obstacle() {
        let mut s = clean_state();
        s.in_progress = InProgressOp::CherryPick;
        assert!(matches!(
            InProgressCheck.detect(&s),
            Some(Obstacle::InProgress {
                op: InProgressOp::CherryPick
            })
        ));
    }
}
