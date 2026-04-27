//! Hints for network-layer failures reaching the remote.

use crate::error_hints::rule::{ErrorHintRule, Hint, HintContext};

/// `fatal: unable to access '…': Could not resolve host: <hostname>`.
/// Almost always one of: offline, VPN intercepting DNS, or a typo in
/// the remote URL that DNS legitimately can't resolve.
pub struct HostResolutionFailed;

impl ErrorHintRule for HostResolutionFailed {
    fn examine(&self, ctx: &HintContext<'_>) -> Option<Hint> {
        if !ctx.stderr.contains("Could not resolve host") {
            return None;
        }
        Some(Hint {
            rule_id: "host-resolution-failed",
            title: "git could not reach the remote — DNS failed to resolve the host name."
                .to_string(),
            actions: vec![
                "Check connectivity: are you online? \
                 Some VPNs (especially split-tunnel or work-VPNs) hijack DNS for internal hosts."
                    .to_string(),
                "Verify the remote URL: `git remote -v`. \
                 A typo in the host (`github.con` instead of `github.com`) reaches DNS but resolves to nothing."
                    .to_string(),
                "Test the host directly: `ping <host>` or \
                 `nslookup <host>` (replace `<host>` with the name in the error)."
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
    fn matches_canonical_dns_error() {
        let stderr = "fatal: unable to access 'https://github.com/user/repo.git/': \
                      Could not resolve host: github.com\n";
        let parsed = parse(&[]);
        let hint = HostResolutionFailed.examine(&ctx(stderr, &parsed)).unwrap();
        assert_eq!(hint.rule_id, "host-resolution-failed");
        assert!(hint.actions[0].contains("VPN"));
        assert!(hint.actions[1].contains("git remote -v"));
        assert!(hint.actions[2].contains("ping"));
    }

    #[test]
    fn does_not_match_unrelated_stderr() {
        let parsed = parse(&[]);
        assert!(HostResolutionFailed
            .examine(&ctx("fatal: not a git repository\n", &parsed))
            .is_none());
    }
}
