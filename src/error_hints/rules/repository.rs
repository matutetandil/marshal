//! Hints for failures rooted in the absence of a Git repository.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// Detects `fatal: not a git repository (or any of the parent directories)`.
/// Marshal points the user at `git init` (new project) or `cd`
/// (existing project) — the two operations git does not suggest itself.
pub struct NotAGitRepository;

impl ErrorHintRule for NotAGitRepository {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        // Git's exact phrasing has been stable for many releases. Match a
        // short substring rather than the full sentence so a future
        // wording tweak ("not in a git repository", localisation) still
        // fires the hint.
        if !ctx.stderr.contains("not a git repository") {
            return None;
        }
        Some(Hint {
            rule_id: "not-a-git-repository",
            title: "this directory is not inside a Git repository.".to_string(),
            actions: vec![
                "If this is a new project, run `git init` to start one here.".to_string(),
                "If you meant to work in an existing repo, `cd` into it first.".to_string(),
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
    fn matches_typical_fatal_line() {
        let stderr = "fatal: not a git repository (or any of the parent directories): .git\n";
        let parsed = parse(&[]);
        let hint = NotAGitRepository.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "not-a-git-repository");
        assert_eq!(hint.actions.len(), 2);
        assert!(hint.actions[0].contains("git init"));
        assert!(hint.actions[1].contains("cd"));
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(NotAGitRepository
            .examine(&ctx("fatal: pathspec 'foo' did not match any files\n", &parsed))
            .is_none());
        assert!(NotAGitRepository.examine(&ctx("", &parsed)).is_none());
    }
}
