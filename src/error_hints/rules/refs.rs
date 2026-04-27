//! Hints for failures resolving refs (branches, tags, commits).

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `fatal: ambiguous argument '…': unknown revision or path not in the
/// working tree.` Fires when git can't tell whether what the user typed
/// is a ref or a path, and neither resolves. The most common real cause
/// is a branch/tag that exists on the remote but hasn't been fetched,
/// so the hint front-loads `git fetch`.
pub struct AmbiguousArgument;

impl ErrorHintRule for AmbiguousArgument {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        // "ambiguous argument" alone is too generic; require both halves
        // of git's actual phrasing.
        let triggered =
            ctx.stderr.contains("ambiguous argument") && ctx.stderr.contains("unknown revision");
        if !triggered {
            return None;
        }
        Some(Hint {
            rule_id: "ambiguous-argument",
            title: "git could not resolve that name to a commit, branch, tag, or path.".to_string(),
            actions: vec![
                "If it lives on the remote but not locally, run `git fetch` and try again."
                    .to_string(),
                "List candidates: `git branch -a` shows every local and remote branch; \
                 `git tag` lists tags."
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
    fn matches_canonical_ambiguous_argument() {
        let stderr = "fatal: ambiguous argument 'feat/missing': \
                      unknown revision or path not in the working tree.\n\
                      Use '--' to separate paths from revisions, like this:\n\
                      'git <command> [<revision>...] -- [<file>...]'\n";
        let parsed = parse(&[]);
        let hint = AmbiguousArgument.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "ambiguous-argument");
        assert!(hint.actions[0].contains("git fetch"));
        assert!(hint.actions[1].contains("git branch -a"));
    }

    #[test]
    fn does_not_fire_on_partial_phrase() {
        let parsed = parse(&[]);
        // "ambiguous argument" without "unknown revision" — different
        // failure mode (e.g. multiple matches), do not coopt the hint.
        assert!(AmbiguousArgument
            .examine(&ctx(
                "error: ambiguous argument matched 3 candidates\n",
                &parsed
            ))
            .is_none());
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(AmbiguousArgument
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
    }
}
