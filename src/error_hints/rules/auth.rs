//! Hints for HTTPS authentication failures against Git remotes.
//!
//! Sibling to `ssh.rs`: same goal (help the user past an auth wall),
//! different mechanism. SSH issues are about keys + agents; HTTPS
//! issues are about credentials + helpers.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `fatal: Authentication failed for 'https://…'`. Common causes after
/// 2021: GitHub no longer accepts passwords (PAT required), credential
/// helper missing or pointed at a stale token, or the user is fighting
/// HTTPS when SSH would be smoother.
pub struct HttpsAuthFailed;

impl ErrorHintRule for HttpsAuthFailed {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        // Anchored on the `https` scheme inside the quoted URL — the
        // SSH equivalent (`Permission denied (publickey)`) is handled
        // by `ssh-publickey-denied`, and we don't want both rules
        // firing on a mixed-protocol stderr.
        let triggered = ctx.stderr.contains("Authentication failed for 'https");
        if !triggered {
            return None;
        }
        Some(Hint {
            rule_id: "https-auth-failed",
            title: "the HTTPS server rejected your credentials.".to_string(),
            actions: vec![
                "GitHub no longer accepts passwords for HTTPS. \
                 Generate a Personal Access Token \
                 (Settings → Developer settings → Personal access tokens) \
                 and use it as the password."
                    .to_string(),
                "Often smoother: switch the remote to SSH. \
                 `git remote set-url origin git@github.com:<user>/<repo>.git`."
                    .to_string(),
                "Stop pasting credentials each push: \
                 `git config --global credential.helper osxkeychain` (macOS), \
                 `manager` (Windows), or `store` (Linux — note: plain text on disk)."
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
    fn matches_canonical_https_failure() {
        let stderr =
            "remote: Support for password authentication was removed on August 13, 2021.\n\
                      remote: Please see https://docs.github.com/get-started/...\n\
                      fatal: Authentication failed for 'https://github.com/user/repo.git/'\n";
        let parsed = parse(&[]);
        let hint = HttpsAuthFailed.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "https-auth-failed");
        assert!(hint.actions[0].contains("Personal Access Token"));
        assert!(hint.actions[1].contains("git remote set-url"));
        assert!(hint.actions[2].contains("credential.helper"));
    }

    #[test]
    fn does_not_match_ssh_auth_refusal() {
        // SSH rejection is handled by ssh-publickey-denied; this rule
        // must stay off the SSH path.
        let stderr = "git@github.com: Permission denied (publickey).\n\
                      fatal: Could not read from remote repository.\n";
        let parsed = parse(&[]);
        assert!(HttpsAuthFailed.examine(&ctx(stderr, &parsed)).is_none());
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(HttpsAuthFailed
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
    }
}
