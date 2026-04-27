//! `marshal help hints` — actionable error hints reference.

use crate::commands::help::topic::{HelpOutput, HelpSection, HelpTopic};

pub struct Hints;

impl HelpTopic for Hints {
    fn name(&self) -> &'static str {
        "hints"
    }

    fn produce(&self) -> HelpOutput {
        let title = "Actionable error hints — concrete next steps after a git failure.".to_string();

        let summary = HelpSection {
            heading: "Summary:".to_string(),
            body: vec![
                "When git exits non-zero with a recognised failure, Marshal appends a short hint"
                    .to_string(),
                "to stderr below git's own message. The hint never modifies git's output —"
                    .to_string(),
                "it only adds a `marshal: hint:` block underneath.".to_string(),
            ],
        };

        let format = HelpSection {
            heading: "Format:".to_string(),
            body: vec![
                "marshal: hint: <one-line title>".to_string(),
                "  • <action 1>".to_string(),
                "  • <action 2>".to_string(),
            ],
        };

        let toggle = HelpSection {
            heading: "Toggle:".to_string(),
            body: vec![
                "On by default (errors.actionable_hints = true).".to_string(),
                "Disable: `marshal config set errors.actionable_hints false`.".to_string(),
                "Disabling restores byte-exact stderr passthrough — Marshal stops".to_string(),
                "capturing stderr at all when the feature is off.".to_string(),
            ],
        };

        let rules = HelpSection {
            heading: "Rules currently shipped:".to_string(),
            body: vec![
                "not-a-git-repository                 fatal: not a git repository".to_string(),
                "dubious-ownership                    detected dubious ownership in repository at"
                    .to_string(),
                "empty-ident                          empty author identity / Author identity unknown"
                    .to_string(),
                "ssh-publickey-denied                 Permission denied (publickey)"
                    .to_string(),
                "https-auth-failed                    Authentication failed for 'https://…'"
                    .to_string(),
                "host-resolution-failed               Could not resolve host (DNS/network/VPN)"
                    .to_string(),
                "push-non-fast-forward                git push rejected because remote moved ahead"
                    .to_string(),
                "upstream-not-configured              first push of a new branch — no upstream"
                    .to_string(),
                "src-refspec-no-match                 push has nothing to send"
                    .to_string(),
                "pathspec-no-match                    pathspec '…' did not match any file"
                    .to_string(),
                "ambiguous-argument                   ambiguous argument: unknown revision or path"
                    .to_string(),
                "local-changes-would-be-overwritten   uncommitted changes block the operation"
                    .to_string(),
                "unrelated-histories                  refusing to merge unrelated histories"
                    .to_string(),
            ],
        };

        HelpOutput {
            topic: self.name().to_string(),
            title,
            sections: vec![summary, format, toggle, rules],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_topic_lists_every_canonical_rule_id() {
        // Sanity — keep the topic in sync with the rules registered
        // in `error_hints/rules/mod.rs`. If a rule is added/removed
        // the topic body must follow.
        let out = Hints.produce();
        let rules = out
            .sections
            .iter()
            .find(|s| s.heading.starts_with("Rules"))
            .unwrap();
        for rule_id in [
            "not-a-git-repository",
            "dubious-ownership",
            "empty-ident",
            "ssh-publickey-denied",
            "https-auth-failed",
            "host-resolution-failed",
            "push-non-fast-forward",
            "upstream-not-configured",
            "src-refspec-no-match",
            "pathspec-no-match",
            "ambiguous-argument",
            "local-changes-would-be-overwritten",
            "unrelated-histories",
        ] {
            assert!(
                rules.body.iter().any(|l| l.contains(rule_id)),
                "missing rule_id: {rule_id}"
            );
        }
    }

    #[test]
    fn hints_topic_documents_the_toggle() {
        let out = Hints.produce();
        let body: Vec<_> = out.sections.iter().flat_map(|s| s.body.iter()).collect();
        assert!(body.iter().any(|l| l.contains("errors.actionable_hints")));
    }
}
