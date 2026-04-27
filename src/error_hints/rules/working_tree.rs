//! Hints for failures caused by uncommitted changes in the working tree.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `git checkout`, `git switch`, `git pull`, `git merge`, and `git rebase`
/// all refuse to clobber uncommitted changes. The phrasing varies
/// slightly per command but always includes "would be overwritten by".
pub struct LocalChangesWouldBeOverwritten;

impl ErrorHintRule for LocalChangesWouldBeOverwritten {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.stderr.contains("would be overwritten") {
            return None;
        }
        Some(Hint {
            rule_id: "local-changes-would-be-overwritten",
            title: "the operation would overwrite uncommitted changes in your working tree."
                .to_string(),
            actions: vec![
                "Save them temporarily: `git stash push -m \"wip\"`, \
                 then re-apply with `git stash pop` after the operation."
                    .to_string(),
                "Commit them: `git commit -am \"<message>\"` if they belong on the current branch."
                    .to_string(),
                "Discard them (irreversible): `git restore <files>` \
                 (or `git restore .` to drop everything)."
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
            exit_code: 1,
        }
    }

    #[test]
    fn matches_checkout_refusal() {
        let stderr =
            "error: Your local changes to the following files would be overwritten by checkout:\n\
                      \tsrc/main.rs\n\
                      Please commit your changes or stash them before you switch branches.\n";
        let parsed = parse(&[]);
        let hint = LocalChangesWouldBeOverwritten
            .examine(&ctx(stderr, &parsed))
            .unwrap();
        assert_eq!(hint.rule_id, "local-changes-would-be-overwritten");
        assert!(hint.actions.iter().any(|a| a.contains("stash")));
        assert!(hint.actions.iter().any(|a| a.contains("commit")));
        assert!(hint.actions.iter().any(|a| a.contains("restore")));
    }

    #[test]
    fn matches_merge_refusal() {
        let stderr =
            "error: Your local changes to the following files would be overwritten by merge:\n";
        let parsed = parse(&[]);
        assert!(LocalChangesWouldBeOverwritten
            .examine(&ctx(stderr, &parsed))
            .is_some());
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(LocalChangesWouldBeOverwritten
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
    }
}
