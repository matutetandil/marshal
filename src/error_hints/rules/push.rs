//! Hints for `git push` rejections.
//!
//! Every rule in this file is gated on `parsed.subcommand == "push"` —
//! the same stderr substrings can appear in stderr captured from
//! wrappers or scripts, and the hints only make sense in the context
//! of a user-typed push.

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

/// First push of a brand-new branch: the local branch has no upstream
/// configured, so git asks the user to declare the remote+branch
/// pairing. The hint surfaces the modern `-u` shortcut and the helper
/// for getting the current branch name.
pub struct UpstreamNotConfigured;

impl ErrorHintRule for UpstreamNotConfigured {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.parsed.subcommand_is("push") {
            return None;
        }
        if !ctx.stderr.contains("no upstream branch") {
            return None;
        }
        Some(Hint {
            rule_id: "upstream-not-configured",
            title: "this branch has no upstream — git needs to know where to push it.".to_string(),
            actions: vec![
                "Push and set tracking in one go: \
                 `git push -u origin <branch>` (the `-u` is short for `--set-upstream`). \
                 Future pushes from this branch then need just `git push`."
                    .to_string(),
                "Branch name handy? `git branch --show-current` prints it.".to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod upstream_tests {
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
            exit_code: 128,
        }
    }

    #[test]
    fn matches_first_push_message() {
        let stderr = "fatal: The current branch feat/x has no upstream branch.\n\
                      To push the current branch and set the remote as upstream, use\n\n\
                      \tgit push --set-upstream origin feat/x\n";
        let parsed = parse(&[OsString::from("push")]);
        let hint = UpstreamNotConfigured
            .examine(&ctx(stderr, &parsed))
            .unwrap();
        assert_eq!(hint.rule_id, "upstream-not-configured");
        assert!(hint.actions[0].contains("git push -u origin"));
        assert!(hint.actions[1].contains("git branch --show-current"));
    }

    #[test]
    fn does_not_fire_outside_push() {
        let stderr = "fatal: no upstream branch\n";
        let parsed = parse(&[OsString::from("status")]);
        assert!(UpstreamNotConfigured
            .examine(&ctx(stderr, &parsed))
            .is_none());
    }

    #[test]
    fn does_not_fire_on_unrelated_push_failure() {
        let parsed = parse(&[OsString::from("push")]);
        assert!(UpstreamNotConfigured
            .examine(&ctx("Everything up-to-date\n", &parsed))
            .is_none());
    }
}

/// `error: src refspec <X> does not match any` happens when push has
/// nothing to send: there are no commits on the branch yet, the branch
/// name was typoed, or HEAD is detached. The hint walks all three.
pub struct SrcRefspecNoMatch;

impl ErrorHintRule for SrcRefspecNoMatch {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.parsed.subcommand_is("push") {
            return None;
        }
        if !(ctx.stderr.contains("src refspec") && ctx.stderr.contains("does not match any")) {
            return None;
        }
        Some(Hint {
            rule_id: "src-refspec-no-match",
            title: "push has nothing to send — no commits yet, or the branch name doesn't match locally."
                .to_string(),
            actions: vec![
                "Did you commit yet? `git log --oneline -1` should print at least one commit."
                    .to_string(),
                "Confirm you are on the branch you mean to push: `git branch --show-current`."
                    .to_string(),
                "If HEAD is detached (no current branch), check out a branch first: \
                 `git switch -c <new-name>` to create one from the current commit."
                    .to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod src_refspec_tests {
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
    fn matches_canonical_refspec_error() {
        let stderr = "error: src refspec main does not match any\n\
                      error: failed to push some refs to 'origin'\n";
        let parsed = parse(&[OsString::from("push")]);
        let hint = SrcRefspecNoMatch.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "src-refspec-no-match");
        assert!(hint.actions[0].contains("git log"));
        assert!(hint.actions[1].contains("branch --show-current"));
        assert!(hint.actions[2].contains("switch -c"));
    }

    #[test]
    fn does_not_fire_outside_push() {
        let stderr = "error: src refspec main does not match any\n";
        let parsed = parse(&[OsString::from("fetch")]);
        assert!(SrcRefspecNoMatch.examine(&ctx(stderr, &parsed)).is_none());
    }

    #[test]
    fn does_not_fire_on_partial_match() {
        // "src refspec" alone, without "does not match any", is a generic
        // diagnostic line — do not coopt the hint.
        let stderr = "Pushing src refspec main to origin\n";
        let parsed = parse(&[OsString::from("push")]);
        assert!(SrcRefspecNoMatch.examine(&ctx(stderr, &parsed)).is_none());
    }
}
