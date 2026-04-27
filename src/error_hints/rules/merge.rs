//! Hints for failures during merge operations.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `git merge` (and `git pull`, which calls merge) refuses by default
/// when the two histories share no common ancestor. The flag to bypass
/// the check exists but is rarely the right answer — the more common
/// cause is the user picking the wrong branch or remote. The hint walks
/// through both possibilities.
pub struct UnrelatedHistories;

impl ErrorHintRule for UnrelatedHistories {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.stderr.contains("refusing to merge unrelated histories") {
            return None;
        }
        Some(Hint {
            rule_id: "unrelated-histories",
            title: "the two branches share no common ancestor — git refused to merge them."
                .to_string(),
            actions: vec![
                "Double-check the source: \
                 are you merging from the right branch / remote? \
                 An accidental `git pull` against an unrelated repository looks like this."
                    .to_string(),
                "If you really want to combine the histories, \
                 re-run with `--allow-unrelated-histories`. \
                 Use it deliberately — the resulting commit will tie two unrelated trees together."
                    .to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::parser::parse;

    fn ctx<'a>(
        stderr: &'a str,
        parsed: &'a crate::git::parser::ParsedGitInvocation,
    ) -> HintContext<'a> {
        HintContext {
            stderr,
            parsed,
            exit_code: 128,
        }
    }

    #[test]
    fn matches_merge_refusal() {
        let stderr = "fatal: refusing to merge unrelated histories\n";
        let parsed = parse(&[]);
        let hint = UnrelatedHistories.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "unrelated-histories");
        assert!(hint.actions[1].contains("--allow-unrelated-histories"));
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(UnrelatedHistories
            .examine(&ctx("Auto-merging foo.txt\n", &parsed))
            .is_none());
        assert!(UnrelatedHistories.examine(&ctx("", &parsed)).is_none());
    }
}
