//! Hints for `git push` rejections.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `git push` rejected with a non-fast-forward error: the remote has
/// commits the local branch does not. Gated on `parsed.subcommand ==
/// "push"` so the same substring appearing in unrelated stderr (e.g.
/// CI output captured by a wrapper) does not trigger the hint.
pub struct PushNonFastForward;

impl ErrorHintRule for PushNonFastForward {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.parsed.subcommand_is("push") {
            return None;
        }
        // Two phrases git uses for the same condition:
        //   "Updates were rejected because the remote contains work …"
        //   "Updates were rejected because the tip of your current branch is behind …"
        // Both share "Updates were rejected because", which is a stable
        // anchor across the variants.
        let rejected = ctx.stderr.contains("Updates were rejected because")
            || ctx.stderr.contains("(non-fast-forward)");
        if !rejected {
            return None;
        }
        Some(Hint {
            rule_id: "push-non-fast-forward",
            title: "the remote has commits your local branch does not — push refused.".to_string(),
            actions: vec![
                "Bring those commits in first, then push: \
                 `git pull --rebase` followed by `git push`."
                    .to_string(),
                "If you are certain the remote history can be replaced \
                 (e.g. you rewrote it intentionally), \
                 use `git push --force-with-lease` — never plain `--force` on shared branches."
                    .to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::parser::parse;
    use std::ffi::OsString;

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
    fn matches_canonical_push_rejection() {
        let stderr = "To github.com:user/repo.git\n\
                      ! [rejected]        main -> main (non-fast-forward)\n\
                      error: failed to push some refs to 'github.com:user/repo.git'\n\
                      hint: Updates were rejected because the remote contains work …\n";
        let parsed = parse(&[OsString::from("push")]);
        let hint = PushNonFastForward.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "push-non-fast-forward");
        assert!(hint.actions[0].contains("pull --rebase"));
        assert!(hint.actions[1].contains("force-with-lease"));
    }

    #[test]
    fn does_not_fire_for_non_push_subcommands() {
        // Same stderr substring under a fetch invocation is suspicious
        // (pull invokes fetch+merge), but this rule speaks for the user's
        // typed command. Guard explicitly so the hint matches user intent.
        let stderr = "Updates were rejected because the remote contains work …\n";
        let parsed = parse(&[OsString::from("fetch")]);
        assert!(PushNonFastForward.examine(&ctx(stderr, &parsed)).is_none());
    }

    #[test]
    fn does_not_fire_when_stderr_is_unrelated() {
        let parsed = parse(&[OsString::from("push")]);
        assert!(PushNonFastForward
            .examine(&ctx("Everything up-to-date\n", &parsed))
            .is_none());
    }
}
