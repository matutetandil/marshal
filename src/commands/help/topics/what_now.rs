//! `marshal help what-now` — the what-now command in detail.

use crate::commands::help::topic::{HelpOutput, HelpSection, HelpTopic};

pub struct WhatNow;

impl HelpTopic for WhatNow {
    fn name(&self) -> &'static str {
        "what-now"
    }

    fn produce(&self) -> HelpOutput {
        let title =
            "marshal what-now — analyse repo state and suggest the next action.".to_string();

        let summary = HelpSection {
            heading: "Summary:".to_string(),
            body: vec![
                "Reads the cold state of the repository (branch, ahead/behind, working".to_string(),
                "tree counters, in-progress operations) and prints one concrete next".to_string(),
                "step on stdout. The proactive counterpart to actionable error hints:".to_string(),
                "hints fire reactively after a git failure; what-now is the user-invoked"
                    .to_string(),
                "\"what should I do here?\" call.".to_string(),
            ],
        };

        let invocation = HelpSection {
            heading: "Invocation:".to_string(),
            body: vec![
                "marshal what-now           Human form (default).".to_string(),
                "marshal what-now --json    Structured JSON: {rule_id, title, suggestions}."
                    .to_string(),
            ],
        };

        let priority = HelpSection {
            heading: "Rule priority (first match wins):".to_string(),
            body: vec![
                "1.  merge-conflict          Unresolved conflicts — abort cmd adapts to active op."
                    .to_string(),
                "2.  *-in-progress           rebase / cherry-pick / revert / bisect / paused-merge."
                    .to_string(),
                "3.  initial-state           Fresh repo, no commits.".to_string(),
                "4.  detached-head           HEAD not on a branch.".to_string(),
                "5.  uncommitted-changes     Title composes only the buckets that have files."
                    .to_string(),
                "6.  diverged                Both ahead and behind upstream.".to_string(),
                "7.  behind-upstream         Behind only.".to_string(),
                "8.  unpushed-commits[-no-upstream]  Ahead only.".to_string(),
                "9.  clean                   Catch-all so every state produces advice.".to_string(),
            ],
        };

        let data_sources = HelpSection {
            heading: "Data sources (no human-readable git output is parsed):".to_string(),
            body: vec![
                "git status --porcelain=v2 --branch  — branch + ahead/behind + working tree."
                    .to_string(),
                "<git-dir>/MERGE_HEAD                — paused merge.".to_string(),
                "<git-dir>/rebase-merge/             — rebase in progress.".to_string(),
                "<git-dir>/rebase-apply/             — rebase via apply backend.".to_string(),
                "<git-dir>/CHERRY_PICK_HEAD          — cherry-pick in progress.".to_string(),
                "<git-dir>/REVERT_HEAD               — revert in progress.".to_string(),
                "<git-dir>/BISECT_LOG                — bisect in progress.".to_string(),
            ],
        };

        HelpOutput {
            topic: self.name().to_string(),
            title,
            sections: vec![summary, invocation, priority, data_sources],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_now_topic_lists_every_advice_rule_in_priority_order() {
        let out = WhatNow.produce();
        let priority = out
            .sections
            .iter()
            .find(|s| s.heading.starts_with("Rule priority"))
            .unwrap();
        for rule_id in [
            "merge-conflict",
            "in-progress",
            "initial-state",
            "detached-head",
            "uncommitted-changes",
            "diverged",
            "behind-upstream",
            "unpushed-commits",
            "clean",
        ] {
            assert!(
                priority.body.iter().any(|l| l.contains(rule_id)),
                "missing rule_id: {rule_id}"
            );
        }
    }

    #[test]
    fn what_now_topic_documents_data_sources() {
        let out = WhatNow.produce();
        let body: Vec<_> = out.sections.iter().flat_map(|s| s.body.iter()).collect();
        assert!(body.iter().any(|l| l.contains("--porcelain=v2")));
        assert!(body.iter().any(|l| l.contains("MERGE_HEAD")));
        assert!(body.iter().any(|l| l.contains("CHERRY_PICK_HEAD")));
    }
}
