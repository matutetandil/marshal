//! Branch-vs-upstream relationship rules: ahead, behind, diverged.
//!
//! All three fire only when the working tree is clean and no
//! higher-priority condition (conflict, in-progress, initial,
//! detached, uncommitted) is active. In that order they live last in
//! the chain just before the `clean` fallback.

use crate::commands::what_now::rule::{Advice, AdviceRule};
use crate::git::porcelain::RepoState;

/// Both ahead and behind — local commits exist that the remote does
/// not, and the remote has commits the local branch does not. Sync
/// before pushing.
pub struct Diverged;

impl AdviceRule for Diverged {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if !is_clean_and_idle(state) {
            return None;
        }
        let b = &state.branch;
        if b.ahead == 0 || b.behind == 0 {
            return None;
        }
        let upstream = upstream_label(state);
        Some(Advice {
            rule_id: "diverged",
            title: format!(
                "Diverged from {upstream}: {ahead} ahead, {behind} behind.",
                ahead = b.ahead,
                behind = b.behind,
            ),
            suggestions: vec![
                "Bring the remote commits in first: \
                 `git pull --rebase` (replays your local commits on top)."
                    .to_string(),
                "After any conflicts are resolved, push: `git push`.".to_string(),
            ],
        })
    }
}

/// Behind only — remote moved, local hasn't. Catch up.
pub struct Behind;

impl AdviceRule for Behind {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if !is_clean_and_idle(state) {
            return None;
        }
        let b = &state.branch;
        if b.behind == 0 || b.ahead != 0 {
            return None;
        }
        let upstream = upstream_label(state);
        let n = b.behind;
        let plural = if n == 1 { "" } else { "s" };
        Some(Advice {
            rule_id: "behind-upstream",
            title: format!("Behind {upstream} by {n} commit{plural}."),
            suggestions: vec![
                "Fast-forward catch up: `git pull --rebase`.".to_string(),
                "Or pull with a merge commit: `git pull --no-rebase`.".to_string(),
            ],
        })
    }
}

/// Ahead only — local commits not yet on the remote. Push.
pub struct Ahead;

impl AdviceRule for Ahead {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        if !is_clean_and_idle(state) {
            return None;
        }
        let b = &state.branch;
        if b.ahead == 0 || b.behind != 0 {
            return None;
        }
        // Two distinct shapes: tracking (have an upstream) versus
        // first push (no upstream configured at all).
        let n = b.ahead;
        let plural = if n == 1 { "" } else { "s" };
        if state.branch.upstream.is_some() {
            let upstream = upstream_label(state);
            Some(Advice {
                rule_id: "unpushed-commits",
                title: format!("Ahead of {upstream} by {n} commit{plural}."),
                suggestions: vec!["Push them: `git push`.".to_string()],
            })
        } else {
            // No upstream: the user must declare one with -u on first
            // push. Use the local branch name; fall back to a
            // placeholder when even that is missing (rare — initial
            // repo with commits but no branch.head, basically only
            // detached, which is caught earlier).
            let branch = state.branch.name.as_deref().unwrap_or("<branch>");
            Some(Advice {
                rule_id: "unpushed-commits-no-upstream",
                title: format!("{n} local commit{plural} on `{branch}` — no upstream configured."),
                suggestions: vec![format!(
                    "Push and set tracking: `git push -u origin {branch}` \
                         (the `-u` is `--set-upstream`)."
                )],
            })
        }
    }
}

fn is_clean_and_idle(state: &RepoState) -> bool {
    state.working_tree.is_clean() && !state.in_progress.is_active()
}

fn upstream_label(state: &RepoState) -> String {
    state
        .branch
        .upstream
        .clone()
        .unwrap_or_else(|| "upstream".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::{BranchInfo, WorkingTreeInfo};

    fn clean_state(ahead: usize, behind: usize, upstream: Option<&str>) -> RepoState {
        RepoState {
            branch: BranchInfo {
                name: Some("main".to_string()),
                ahead,
                behind,
                upstream: upstream.map(str::to_string),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    // ── Diverged ───────────────────────────────────────────────────

    #[test]
    fn diverged_fires_when_both_ahead_and_behind() {
        let s = clean_state(2, 3, Some("origin/main"));
        let advice = Diverged.examine(&s).unwrap();
        assert_eq!(advice.rule_id, "diverged");
        assert!(advice.title.contains("2 ahead"));
        assert!(advice.title.contains("3 behind"));
        assert!(advice
            .suggestions
            .iter()
            .any(|s| s.contains("git pull --rebase")));
    }

    #[test]
    fn diverged_does_not_fire_when_only_one_side_moved() {
        assert!(Diverged
            .examine(&clean_state(2, 0, Some("origin/main")))
            .is_none());
        assert!(Diverged
            .examine(&clean_state(0, 3, Some("origin/main")))
            .is_none());
    }

    // ── Behind ─────────────────────────────────────────────────────

    #[test]
    fn behind_fires_when_only_behind() {
        let advice = Behind
            .examine(&clean_state(0, 4, Some("origin/main")))
            .unwrap();
        assert_eq!(advice.rule_id, "behind-upstream");
        assert!(advice.title.contains("4 commits"));
        assert!(advice.suggestions[0].contains("git pull --rebase"));
    }

    #[test]
    fn behind_yields_singular_for_one() {
        let advice = Behind
            .examine(&clean_state(0, 1, Some("origin/main")))
            .unwrap();
        assert!(advice.title.contains("1 commit."), "got: {}", advice.title);
    }

    #[test]
    fn behind_does_not_fire_when_also_ahead() {
        assert!(Behind
            .examine(&clean_state(2, 3, Some("origin/main")))
            .is_none());
    }

    // ── Ahead ──────────────────────────────────────────────────────

    #[test]
    fn ahead_with_upstream_yields_simple_push() {
        let advice = Ahead
            .examine(&clean_state(2, 0, Some("origin/main")))
            .unwrap();
        assert_eq!(advice.rule_id, "unpushed-commits");
        assert!(advice.title.contains("Ahead of origin/main"));
        assert_eq!(
            advice.suggestions,
            vec!["Push them: `git push`.".to_string()]
        );
    }

    #[test]
    fn ahead_without_upstream_yields_set_upstream_form() {
        let advice = Ahead.examine(&clean_state(1, 0, None)).unwrap();
        assert_eq!(advice.rule_id, "unpushed-commits-no-upstream");
        assert!(advice.suggestions[0].contains("-u origin main"));
    }

    #[test]
    fn ahead_does_not_fire_when_also_behind() {
        assert!(Ahead
            .examine(&clean_state(2, 3, Some("origin/main")))
            .is_none());
    }

    // ── shared guards ──────────────────────────────────────────────

    #[test]
    fn no_sync_rule_fires_with_dirty_working_tree() {
        let s = RepoState {
            branch: BranchInfo {
                ahead: 2,
                ..Default::default()
            },
            working_tree: WorkingTreeInfo {
                unstaged: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(Diverged.examine(&s).is_none());
        assert!(Behind.examine(&s).is_none());
        assert!(Ahead.examine(&s).is_none());
    }
}
