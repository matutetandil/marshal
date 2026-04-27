//! Unresolved merge conflicts — highest-priority advice.

use crate::commands::what_now::rule::{Advice, AdviceRule};
use crate::git::porcelain::{InProgressOp, RepoState};

/// Anything else marshal might suggest is moot until conflicts are
/// resolved — `git commit` and `git switch` will refuse, every push
/// will block, and the user can lose work by glossing over them. So
/// this rule is registered first.
///
/// The advice branches on the in-progress operation when one is
/// active so the abort command is the right one (`git rebase --abort`
/// vs `git merge --abort`, etc.).
pub struct MergeConflict;

impl AdviceRule for MergeConflict {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if !state.working_tree.has_unmerged() {
            return None;
        }
        let abort_cmd = match state.in_progress {
            InProgressOp::Rebase => "`git rebase --abort`",
            InProgressOp::CherryPick => "`git cherry-pick --abort`",
            InProgressOp::Revert => "`git revert --abort`",
            // Plain merges, and the rare case of unmerged paths
            // outside an active op (manual `git update-index`),
            // both back out via `git merge --abort`.
            _ => "`git merge --abort`",
        };
        let n = state.working_tree.unmerged;
        let plural = if n == 1 { "" } else { "s" };
        Some(Advice {
            rule_id: "merge-conflict",
            title: format!("Unresolved merge conflict{plural} in {n} file{plural}."),
            suggestions: vec![
                "Open each conflicted file and resolve the \
                 `<<<<<<<` / `=======` / `>>>>>>>` markers."
                    .to_string(),
                "Mark each one resolved with `git add <file>`. \
                 Run `git status` along the way to see what's left."
                    .to_string(),
                format!(
                    "Finish the operation: `git commit` (or \
                     `git rebase --continue` / `git cherry-pick --continue` \
                     / `git revert --continue` if you were in one of those)."
                ),
                format!("Back out without resolving: {abort_cmd}."),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::WorkingTreeInfo;

    fn state_with_unmerged(n: usize, op: InProgressOp) -> RepoState {
        RepoState {
            working_tree: WorkingTreeInfo {
                unmerged: n,
                ..Default::default()
            },
            in_progress: op,
            ..Default::default()
        }
    }

    #[test]
    fn matches_when_unmerged_files_present() {
        let s = state_with_unmerged(2, InProgressOp::Merge);
        let advice = MergeConflict.examine(&s).unwrap();
        assert_eq!(advice.rule_id, "merge-conflict");
        assert!(advice.title.contains("2 files"));
        assert!(advice.suggestions.iter().any(|a| a.contains("git add")));
    }

    #[test]
    fn singular_phrasing_for_one_file() {
        let s = state_with_unmerged(1, InProgressOp::Merge);
        let advice = MergeConflict.examine(&s).unwrap();
        assert!(advice.title.contains("1 file."));
        assert!(advice.title.contains("conflict in"));
    }

    #[test]
    fn abort_command_matches_active_operation() {
        for (op, expected) in [
            (InProgressOp::Merge, "merge --abort"),
            (InProgressOp::Rebase, "rebase --abort"),
            (InProgressOp::CherryPick, "cherry-pick --abort"),
            (InProgressOp::Revert, "revert --abort"),
            (InProgressOp::None, "merge --abort"),
        ] {
            let advice = MergeConflict.examine(&state_with_unmerged(1, op)).unwrap();
            assert!(
                advice.suggestions.iter().any(|s| s.contains(expected)),
                "op {op:?} should suggest `{expected}`, got {:?}",
                advice.suggestions
            );
        }
    }

    #[test]
    fn does_not_fire_on_clean_state() {
        assert!(MergeConflict.examine(&RepoState::default()).is_none());
    }
}
