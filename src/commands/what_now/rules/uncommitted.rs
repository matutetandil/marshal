//! Working-tree changes the user hasn't committed yet.
//!
//! Fires only when the higher-priority rules (conflict, in-progress,
//! initial, detached) don't apply. The title summarises the three
//! buckets (staged / unstaged / untracked) and the suggestions cover
//! the round-trip: review → stage → commit, plus stash for "save for
//! later".

use crate::commands::what_now::rule::{Advice, AdviceRule};
use crate::git::porcelain::RepoState;

pub struct UncommittedChanges;

impl AdviceRule for UncommittedChanges {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        let wt = state.working_tree;
        if !wt.has_any_changes() || wt.has_unmerged() {
            return None;
        }

        let title = build_title(wt.staged, wt.unstaged, wt.untracked);
        let mut suggestions = Vec::new();

        // Review section — depends on what's there to review.
        if wt.unstaged > 0 && wt.staged > 0 {
            suggestions.push(
                "Review changes: `git diff` (unstaged) and \
                 `git diff --cached` (staged)."
                    .to_string(),
            );
        } else if wt.unstaged > 0 {
            suggestions.push("Review changes: `git diff`.".to_string());
        } else if wt.staged > 0 {
            suggestions.push("Review staged changes: `git diff --cached`.".to_string());
        }

        // Staging section — only relevant if there is anything still
        // to stage.
        if wt.unstaged > 0 || wt.untracked > 0 {
            suggestions.push(
                "Stage all of it: `git add -A`. \
                 Stage selectively: `git add <path>`."
                    .to_string(),
            );
        }

        // Commit section — useful both when there is something staged
        // already and as the natural follow-up after staging.
        suggestions.push("Commit the staged set: `git commit -m \"<message>\"`.".to_string());

        // Stash escape hatch — always relevant.
        suggestions.push(
            "Save for later instead: `git stash push -m \"wip\"` \
             (re-apply with `git stash pop`)."
                .to_string(),
        );

        Some(Advice {
            rule_id: "uncommitted-changes",
            title,
            suggestions,
        })
    }
}

fn build_title(staged: usize, unstaged: usize, untracked: usize) -> String {
    // Compose only the parts that apply, in the order the user
    // typically wants to act on them.
    let mut parts = Vec::with_capacity(3);
    if staged > 0 {
        parts.push(format!("{staged} staged"));
    }
    if unstaged > 0 {
        parts.push(format!("{unstaged} unstaged"));
    }
    if untracked > 0 {
        parts.push(format!("{untracked} untracked"));
    }
    let joined = match parts.len() {
        1 => parts.remove(0),
        2 => format!("{} and {}", parts[0], parts[1]),
        // Three parts: "A, B, and C" — Oxford-comma style.
        _ => format!("{}, {}, and {}", parts[0], parts[1], parts[2]),
    };
    let total: usize = staged + unstaged + untracked;
    let plural = if total == 1 { "" } else { "s" };
    format!("Working tree has {joined} change{plural}.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::WorkingTreeInfo;

    fn state_with(staged: usize, unstaged: usize, untracked: usize) -> RepoState {
        RepoState {
            working_tree: WorkingTreeInfo {
                staged,
                unstaged,
                untracked,
                unmerged: 0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn does_not_fire_on_clean() {
        assert!(UncommittedChanges.examine(&RepoState::default()).is_none());
    }

    #[test]
    fn does_not_fire_when_conflicts_present() {
        let s = RepoState {
            working_tree: WorkingTreeInfo {
                staged: 1,
                unstaged: 0,
                untracked: 0,
                unmerged: 1,
            },
            ..Default::default()
        };
        assert!(UncommittedChanges.examine(&s).is_none());
    }

    #[test]
    fn title_reflects_only_buckets_that_have_files() {
        let advice = UncommittedChanges.examine(&state_with(2, 0, 0)).unwrap();
        assert_eq!(advice.title, "Working tree has 2 staged changes.");
        let advice = UncommittedChanges.examine(&state_with(0, 1, 0)).unwrap();
        assert_eq!(advice.title, "Working tree has 1 unstaged change.");
        let advice = UncommittedChanges.examine(&state_with(2, 1, 3)).unwrap();
        assert_eq!(
            advice.title,
            "Working tree has 2 staged, 1 unstaged, and 3 untracked changes."
        );
    }

    #[test]
    fn suggestions_drop_review_when_only_untracked() {
        // Pure untracked → no `git diff` line, only stage + commit + stash.
        let advice = UncommittedChanges.examine(&state_with(0, 0, 2)).unwrap();
        assert!(!advice.suggestions.iter().any(|s| s.contains("git diff")));
        assert!(advice.suggestions.iter().any(|s| s.contains("git add")));
    }

    #[test]
    fn suggestions_drop_staging_when_only_staged() {
        // Pure staged → no `git add` line; user is ready to commit.
        let advice = UncommittedChanges.examine(&state_with(1, 0, 0)).unwrap();
        assert!(!advice.suggestions.iter().any(|s| s.contains("git add")));
        assert!(advice.suggestions.iter().any(|s| s.contains("git commit")));
    }
}
