use crate::git::porcelain::RepoState;
use crate::workspace::preflight::{Obstacle, PreflightCheck};

/// Detects untracked files in the working tree. Soft blocker —
/// resolved by `--auto-stash` (which uses `--include-untracked`
/// so untracked files are preserved) or `--discard-changes`
/// (which runs `git clean -fd` to remove them).
pub struct UntrackedFilesCheck;

impl PreflightCheck for UntrackedFilesCheck {
    fn detect(&self, state: &RepoState) -> Option<Obstacle> {
        if state.working_tree.untracked > 0 {
            Some(Obstacle::UntrackedFiles {
                count: state.working_tree.untracked,
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
    fn zero_untracked_emits_nothing() {
        assert!(UntrackedFilesCheck.detect(&clean_state()).is_none());
    }

    #[test]
    fn untracked_emits_obstacle_with_count() {
        let mut s = clean_state();
        s.working_tree.untracked = 3;
        assert_eq!(
            UntrackedFilesCheck.detect(&s),
            Some(Obstacle::UntrackedFiles { count: 3 })
        );
    }
}
