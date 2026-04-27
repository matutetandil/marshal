//! Hints for SSH authentication failures during fetch/clone/push/pull.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// Detects the canonical OpenSSH refusal: `Permission denied (publickey)`.
/// The cause is almost always one of: the agent does not hold the key,
/// the key is not registered with the host, or the wrong key matches
/// first in `~/.ssh/config`. We surface the three actions in that order.
pub struct PublicKeyDenied;

impl ErrorHintRule for PublicKeyDenied {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.stderr.contains("Permission denied (publickey)") {
            return None;
        }
        Some(Hint {
            rule_id: "ssh-publickey-denied",
            title: "the SSH server rejected your key.".to_string(),
            actions: vec![
                "List active identities: `ssh-add -l`. If empty, load one with `ssh-add ~/.ssh/id_ed25519` \
                 (or your key path)."
                    .to_string(),
                "Make sure the key is registered with the host \
                 (GitHub: Settings → SSH and GPG keys; GitLab: Preferences → SSH keys)."
                    .to_string(),
                "Test connectivity: `ssh -T git@github.com` (or your host) should greet you by username."
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
    fn matches_openssh_refusal() {
        let stderr = "git@github.com: Permission denied (publickey).\n\
                      fatal: Could not read from remote repository.\n";
        let parsed = parse(&[]);
        let hint = PublicKeyDenied.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "ssh-publickey-denied");
        assert_eq!(hint.actions.len(), 3);
        assert!(hint.actions[0].contains("ssh-add"));
        assert!(hint.actions[2].contains("ssh -T"));
    }

    #[test]
    fn does_not_match_password_denial() {
        // A different SSH refusal (password) should not fire this rule.
        // Future rules may target the password case specifically.
        let stderr = "Permission denied (password).\n";
        let parsed = parse(&[]);
        assert!(PublicKeyDenied.examine(&ctx(stderr, &parsed)).is_none());
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(PublicKeyDenied.examine(&ctx("", &parsed)).is_none());
    }
}
