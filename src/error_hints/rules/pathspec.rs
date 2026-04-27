//! Hints for failures involving file/path resolution by git.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `error: pathspec '…' did not match any file(s) known to git`. Fires
/// across `git checkout`, `git switch`, `git restore`, `git add` and a
/// few others — the same resolution path emits the same message
/// whether the user typed a branch name, a file path, or a typo. The
/// hint covers the three most common causes without being too specific
/// about which command was running.
pub struct PathspecNoMatch;

impl ErrorHintRule for PathspecNoMatch {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        // Two near-identical phrasings exist across git versions:
        //   "did not match any files"
        //   "did not match any file(s)"
        // Match the prefix common to both.
        let triggered =
            ctx.stderr.contains("pathspec") && ctx.stderr.contains("did not match any file");
        if !triggered {
            return None;
        }
        Some(Hint {
            rule_id: "pathspec-no-match",
            title: "git did not find a file or ref matching what you typed.".to_string(),
            actions: vec![
                "Re-check spelling. `git status` lists tracked changes; \
                 `git branch -a` lists every local and remote branch."
                    .to_string(),
                "If you meant to add a brand-new file, run `git add <path>` first — \
                 commands like `git restore` only see files git is already tracking."
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
    fn matches_modern_pathspec_error() {
        let stderr = "error: pathspec 'foo.txt' did not match any file(s) known to git\n";
        let parsed = parse(&[]);
        let hint = PathspecNoMatch.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "pathspec-no-match");
        assert!(hint.actions[0].contains("git status"));
        assert!(hint.actions[1].contains("git add"));
    }

    #[test]
    fn matches_legacy_pathspec_error() {
        let stderr = "error: pathspec 'feat/x' did not match any files\n";
        let parsed = parse(&[]);
        assert!(PathspecNoMatch.examine(&ctx(stderr, &parsed)).is_some());
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(PathspecNoMatch
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
        // "did not match" without "pathspec" is too generic to fire on.
        assert!(PathspecNoMatch
            .examine(&ctx(
                "error: something did not match expected output\n",
                &parsed
            ))
            .is_none());
    }
}
