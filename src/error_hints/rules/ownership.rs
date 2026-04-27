//! Hint for `fatal: detected dubious ownership`.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// Modern git refuses to operate on a repository owned by a different user
/// without an explicit allow-list entry. Git already prints the literal
/// `git config --global --add safe.directory <path>` command — the hint
/// adds the *why* (it is a security check, not a bug) and the broader
/// option (`*` to trust all directories, with the trade-off called out).
pub struct DubiousOwnership;

impl ErrorHintRule for DubiousOwnership {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.stderr.contains("dubious ownership") {
            return None;
        }
        Some(Hint {
            rule_id: "dubious-ownership",
            title: "Git refused to use this repository because it is owned by a different user."
                .to_string(),
            actions: vec![
                "Trust this exact repo: run the `git config --global --add safe.directory …` \
                 command git showed above."
                    .to_string(),
                "Trust every repository (less secure): \
                 `git config --global --add safe.directory '*'`."
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
    fn matches_canonical_message() {
        let stderr = "fatal: detected dubious ownership in repository at '/srv/repo'\n\
                      To add an exception for this directory, call:\n\
                      \tgit config --global --add safe.directory /srv/repo\n";
        let parsed = parse(&[]);
        let hint = DubiousOwnership.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "dubious-ownership");
        assert!(hint.actions[0].contains("safe.directory"));
        assert!(hint.actions[1].contains("'*'"));
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(DubiousOwnership
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
    }
}
