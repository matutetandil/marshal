//! Catch-all advice when nothing else matches.
//!
//! Registered last so every state produces some output; if any
//! earlier rule fires, this one is skipped.

use crate::commands::what_now::rule::{Advice, AdviceRule};
use crate::git::porcelain::RepoState;

pub struct Clean;

impl AdviceRule for Clean {
    fn examine(&self, state: &RepoState) -> Option<Advice> {
        let branch = state
            .branch
            .name
            .as_deref()
            .map(|b| format!("on `{b}`"))
            .unwrap_or_else(|| "(no branch)".to_string());

        let upstream_clause = match &state.branch.upstream {
            Some(u) => format!(" up to date with `{u}`"),
            None => String::new(),
        };

        Some(Advice {
            rule_id: "clean",
            title: format!("Working tree clean, {branch}{upstream_clause}."),
            suggestions: vec![
                "Start something new: `git switch -c feat/<name>`.".to_string(),
                "Or pull the latest: `git pull --rebase`.".to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::porcelain::BranchInfo;

    #[test]
    fn always_fires_with_appropriate_title() {
        let s = RepoState {
            branch: BranchInfo {
                name: Some("main".to_string()),
                upstream: Some("origin/main".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let advice = Clean.examine(&s).unwrap();
        assert_eq!(advice.rule_id, "clean");
        assert!(advice.title.contains("on `main`"));
        assert!(advice.title.contains("up to date with `origin/main`"));
    }

    #[test]
    fn handles_branch_without_upstream() {
        let s = RepoState {
            branch: BranchInfo {
                name: Some("feat/x".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let advice = Clean.examine(&s).unwrap();
        assert!(advice.title.contains("on `feat/x`"));
        assert!(!advice.title.contains("up to date"));
    }

    #[test]
    fn handles_no_branch_at_all() {
        // Hypothetical fallback path — in practice detached/initial
        // are caught by their own rules first, but the catch-all
        // still has to produce something.
        let advice = Clean.examine(&RepoState::default()).unwrap();
        assert!(advice.title.contains("(no branch)"));
    }
}
