//! `marshal help modernize` — modernization tips reference.

use crate::commands::help::topic::{HelpOutput, HelpSection, HelpTopic};

pub struct Modernize;

impl HelpTopic for Modernize {
    fn name(&self) -> &'static str {
        "modernize"
    }

    fn produce(&self) -> HelpOutput {
        let title =
            "Modernization tips — surface the modern equivalent of legacy git forms.".to_string();

        let summary = HelpSection {
            heading: "Summary:".to_string(),
            body: vec![
                "Marshal recognises 12 legacy command shapes that git itself treats as".to_string(),
                "deprecated or succeeded (e.g. `checkout -b` → `switch -c`, `stash save` →"
                    .to_string(),
                "`stash push`, `remote rm` → `remote remove`). When you type one of them,"
                    .to_string(),
                "Marshal prints a one-line tip on stderr **before** running git unchanged."
                    .to_string(),
            ],
        };

        let format = HelpSection {
            heading: "Format:".to_string(),
            body: vec![
                "marshal: tip: try `<modern-form>` instead of `<legacy-form>`".to_string(),
                "             <optional one-line historical note>".to_string(),
            ],
        };

        let settings = HelpSection {
            heading: "Settings:".to_string(),
            body: vec![
                "modernize.tips        Default `true`. Set `false` to silence all tips."
                    .to_string(),
                "modernize.rewrite     Default `false`. Set `true` to *rewrite* the legacy"
                    .to_string(),
                "                      form to its modern equivalent before running git"
                    .to_string(),
                "                      (Invariant 8: rewriting is opt-in only).".to_string(),
            ],
        };

        let coverage = HelpSection {
            heading: "Families covered (12 patterns, 11 rule impls):".to_string(),
            body: vec![
                "checkout → switch / restore   8 patterns from the Git 2.23 split.".to_string(),
                "reset    → restore --staged   file-mode `reset [HEAD] <files>`.".to_string(),
                "stash save → stash push       deprecated since Git 2.16; preserves `-u` / `-m`."
                    .to_string(),
                "remote rm → remote remove     remote-management alias rename.".to_string(),
            ],
        };

        HelpOutput {
            topic: self.name().to_string(),
            title,
            sections: vec![summary, format, settings, coverage],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modernize_topic_documents_the_two_settings() {
        let out = Modernize.produce();
        let body: Vec<_> = out.sections.iter().flat_map(|s| s.body.iter()).collect();
        assert!(body.iter().any(|l| l.contains("modernize.tips")));
        assert!(body.iter().any(|l| l.contains("modernize.rewrite")));
    }

    #[test]
    fn modernize_topic_lists_all_four_families() {
        let out = Modernize.produce();
        let coverage = out
            .sections
            .iter()
            .find(|s| s.heading.starts_with("Families"))
            .unwrap();
        for family in ["checkout", "reset", "stash", "remote"] {
            assert!(
                coverage.body.iter().any(|l| l.contains(family)),
                "missing family: {family}"
            );
        }
    }
}
