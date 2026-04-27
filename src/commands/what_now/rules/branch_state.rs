//! Special branch states that need their own advice: a fresh repo
//! before any commits, and a detached HEAD where the user could
//! lose their position by switching branches.

use crate::commands::what_now::rule::{Advice, AdviceRule};
use crate::git::porcelain::RepoState;

/// `git init` happened, the user has files, but no commits yet. Even
/// if the working tree is "dirty" (untracked files), the right
/// next step is the first commit, not the generic "stage and commit"
/// flow — this rule fires before `uncommitted-changes`.
pub struct InitialState;

impl AdviceRule for InitialState {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if !state.branch.is_initial {
            return None;
        }
        Some(Advice {
            rule_id: "initial-state",
            title: "Fresh repository — no commits yet.".to_string(),
            suggestions: vec![
                "Stage what you want to keep: `git add <path>` \
                 (or `git add -A` for everything)."
                    .to_string(),
                "Make the first commit: `git commit -m \"initial\"`.".to_string(),
            ],
        })
    }
}

/// HEAD is detached — pointing at a commit, not a branch. Anything
/// the user does here that creates new commits will be unreachable
/// once they switch away. The hint to "name where you are" first.
pub struct DetachedHead;

impl AdviceRule for DetachedHead {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if !state.branch.is_detached {
            return None;
        }
        Some(Advice {
            rule_id: "detached-head",
            title: "HEAD is detached — you are not on a branch.".to_string(),
            suggestions: vec![
                "Save your position as a branch: `git switch -c <name>` \
                 (any new commits stick around once you switch away)."
                    .to_string(),
                "Or return to where you were: `git switch -` \
                 jumps back to the previous branch."
                    .to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::BranchInfo;

    #[test]
    fn initial_state_fires_only_when_is_initial() {
        let mut s = RepoState::default();
        s.branch.is_initial = true;
        let advice = InitialState.examine(&s).unwrap();
        assert_eq!(advice.rule_id, "initial-state");
        assert!(advice.suggestions[0].contains("git add"));
        assert!(advice.suggestions[1].contains("git commit"));

        s.branch.is_initial = false;
        assert!(InitialState.examine(&s).is_none());
    }

    #[test]
    fn detached_head_fires_only_when_detached() {
        let s = RepoState {
            branch: BranchInfo {
                is_detached: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let advice = DetachedHead.examine(&s).unwrap();
        assert_eq!(advice.rule_id, "detached-head");
        assert!(advice.suggestions[0].contains("git switch -c"));
        assert!(advice.suggestions[1].contains("git switch -"));

        assert!(DetachedHead.examine(&RepoState::default()).is_none());
    }
}
