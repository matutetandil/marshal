//! In-progress multi-step operations (rebase, cherry-pick, revert,
//! bisect, paused merge).
//!
//! Fires only when no unresolved conflicts exist — the conflict rule
//! handles those with sharper advice. When this rule fires, git is
//! waiting on the user to either finish the operation or abort it.

use crate::commands::what_now::rule::{Advice, AdviceRule};
use crate::git::porcelain::{InProgressOp, RepoState};

pub struct InProgressOperation;

impl AdviceRule for InProgressOperation {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if state.working_tree.has_unmerged() {
            return None; // conflict rule already covers this
        }
        if !state.in_progress.is_active() {
            return None;
        }

        let (rule_id, title, suggestions) = match state.in_progress {
            InProgressOp::Rebase => (
                "rebase-in-progress",
                "Rebase in progress.".to_string(),
                vec![
                    "Continue: `git rebase --continue`.".to_string(),
                    "Skip the current commit: `git rebase --skip`.".to_string(),
                    "Back out: `git rebase --abort`.".to_string(),
                ],
            ),
            InProgressOp::CherryPick => (
                "cherry-pick-in-progress",
                "Cherry-pick in progress.".to_string(),
                vec![
                    "Continue: `git cherry-pick --continue` (after staging fixes if any)."
                        .to_string(),
                    "Skip the current commit: `git cherry-pick --skip`.".to_string(),
                    "Back out: `git cherry-pick --abort`.".to_string(),
                ],
            ),
            InProgressOp::Revert => (
                "revert-in-progress",
                "Revert in progress.".to_string(),
                vec![
                    "Continue: `git revert --continue`.".to_string(),
                    "Skip the current commit: `git revert --skip`.".to_string(),
                    "Back out: `git revert --abort`.".to_string(),
                ],
            ),
            InProgressOp::Bisect => (
                "bisect-in-progress",
                "Bisect in progress.".to_string(),
                vec![
                    "Mark the current commit: `git bisect good` or `git bisect bad`.".to_string(),
                    "Stop without recording a result: `git bisect reset`.".to_string(),
                ],
            ),
            InProgressOp::Merge => (
                "merge-paused",
                "Merge ready to commit (no conflicts).".to_string(),
                vec![
                    "Finish: `git commit` (uses the merge message git prepared).".to_string(),
                    "Back out: `git merge --abort`.".to_string(),
                ],
            ),
            InProgressOp::None => unreachable!("guarded by is_active() above"),
        };
        Some(Advice {
            rule_id,
            title,
            suggestions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::WorkingTreeInfo;

    fn state_for(op: InProgressOp) -> RepoState {
        RepoState {
            in_progress: op,
            ..Default::default()
        }
    }

    #[test]
    fn rebase_yields_continue_skip_abort() {
        let advice = InProgressOperation
            .examine(&state_for(InProgressOp::Rebase))
            .unwrap();
        assert_eq!(advice.rule_id, "rebase-in-progress");
        assert!(advice.suggestions.iter().any(|s| s.contains("--continue")));
        assert!(advice.suggestions.iter().any(|s| s.contains("--skip")));
        assert!(advice.suggestions.iter().any(|s| s.contains("--abort")));
    }

    #[test]
    fn each_op_has_its_own_rule_id() {
        let mut ids = vec![];
        for op in [
            InProgressOp::Rebase,
            InProgressOp::CherryPick,
            InProgressOp::Revert,
            InProgressOp::Bisect,
            InProgressOp::Merge,
        ] {
            ids.push(InProgressOperation.examine(&state_for(op)).unwrap().rule_id);
        }
        // Every rule_id must be unique — same rule, different
        // states should not collide.
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "rule_ids collided: {ids:?}");
    }

    #[test]
    fn does_not_fire_when_conflicts_exist() {
        let s = RepoState {
            in_progress: InProgressOp::Rebase,
            working_tree: WorkingTreeInfo {
                unmerged: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(InProgressOperation.examine(&s).is_none());
    }

    #[test]
    fn does_not_fire_on_clean_state() {
        assert!(InProgressOperation
            .examine(&state_for(InProgressOp::None))
            .is_none());
    }
}
