//! Hints for first-time setup gaps that block commits or basic ops.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// Modern git refuses to record a commit without an author identity.
/// Two phrasings to look for:
///   * `fatal: empty ident name (for <…>) not allowed` — when only the
///     name is empty.
///   * `Author identity unknown` (with the inline `git config` snippet)
///     — when both name and email are unset.
pub struct EmptyIdent;

impl ErrorHintRule for EmptyIdent {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        let triggered = ctx.stderr.contains("empty ident")
            || ctx.stderr.contains("Author identity unknown")
            || ctx.stderr.contains("Please tell me who you are");
        if !triggered {
            return None;
        }
        Some(Hint {
            rule_id: "empty-ident",
            title: "git needs your name and email before it can record a commit.".to_string(),
            actions: vec![
                "Set both globally (recommended for personal machines): \
                 `git config --global user.name \"Your Name\"` and \
                 `git config --global user.email \"you@example.com\"`."
                    .to_string(),
                "Or per-repo if this machine has multiple identities: \
                 drop `--global` and run the same commands from inside the repository."
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
    fn matches_empty_ident_name() {
        let stderr = "fatal: empty ident name (for <test@example.com>) not allowed\n";
        let parsed = parse(&[]);
        let hint = EmptyIdent.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "empty-ident");
        assert!(hint.actions[0].contains("user.name"));
        assert!(hint.actions[0].contains("user.email"));
    }

    #[test]
    fn matches_author_identity_unknown() {
        let stderr = "Author identity unknown\n\n\
                      *** Please tell me who you are.\n\n\
                      Run\n\n  \
                      git config --global user.email \"you@example.com\"\n  \
                      git config --global user.name \"Your Name\"\n";
        let parsed = parse(&[]);
        assert!(EmptyIdent.examine(&ctx(stderr, &parsed)).is_some());
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(EmptyIdent
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
    }
}
