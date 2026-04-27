//! The `marshal help` overview topic — the screen the user lands on
//! when they type `marshal help` with no argument.

use crate::commands::help::topic::{HelpOutput, HelpSection, HelpTopic};
use crate::config::ConfigKey;

pub struct Overview;

impl HelpTopic for Overview {
    fn name(&self) -> &'static str {
        "overview"
    }

    fn produce(&self) -> HelpOutput {
        let title = format!(
            "Marshal {} — a transparent wrapper for git.",
            env!("CARGO_PKG_VERSION")
        );

        let subcommands = HelpSection {
            heading: "Subcommands:".to_string(),
            body: vec![
                "config     Manage Marshal configuration (get/set/unset/list).".to_string(),
                "what-now   Analyse repo state and suggest the next action.".to_string(),
                "help       Print this overview, or `help <topic>` for details.".to_string(),
            ],
        };

        let config_keys = HelpSection {
            heading: "Configuration keys:".to_string(),
            body: ConfigKey::all()
                .iter()
                .map(|k| format!("{:<25}  {}", k.as_dotted(), k.description()))
                .collect(),
        };

        let topics = HelpSection {
            heading: "Topics (run `marshal help <topic>` for any of them):".to_string(),
            body: vec![
                "overview     This screen.".to_string(),
                // Listed here for discoverability; the actual
                // topic structs land in H2.
                "config       Marshal's configuration system.".to_string(),
                "hints        Actionable error hints below git failures.".to_string(),
                "modernize    Modernization tips for legacy git forms.".to_string(),
                "what-now     The what-now command in detail.".to_string(),
            ],
        };

        let global_flags = HelpSection {
            heading: "Global flags (anywhere in argv after `marshal`):".to_string(),
            body: vec!["--json   Emit JSON instead of the human form.".to_string()],
        };

        let pointers = HelpSection {
            heading: "More:".to_string(),
            body: vec![
                "Project: https://github.com/matutetandil/marshal".to_string(),
                "Design:  see `docs/PRINCIPLES.md` and `docs/ARCHITECTURE.md`.".to_string(),
            ],
        };

        HelpOutput {
            topic: self.name().to_string(),
            title,
            sections: vec![subcommands, config_keys, topics, global_flags, pointers],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_carries_a_subcommands_section() {
        let out = Overview.produce();
        assert_eq!(out.topic, "overview");
        assert!(out.title.contains("Marshal"));
        assert!(out
            .sections
            .iter()
            .any(|s| s.heading.starts_with("Subcommands:")));
    }

    #[test]
    fn overview_lists_every_known_config_key() {
        let out = Overview.produce();
        let keys_section = out
            .sections
            .iter()
            .find(|s| s.heading.starts_with("Configuration keys:"))
            .expect("config keys section present");
        for key in ConfigKey::all() {
            assert!(
                keys_section
                    .body
                    .iter()
                    .any(|l| l.contains(key.as_dotted())),
                "expected key '{}' in body, got: {:?}",
                key.as_dotted(),
                keys_section.body
            );
        }
    }

    #[test]
    fn overview_advertises_the_json_global_flag() {
        let out = Overview.produce();
        let body: Vec<_> = out.sections.iter().flat_map(|s| s.body.iter()).collect();
        assert!(
            body.iter().any(|l| l.contains("--json")),
            "expected --json mention in some section, got: {body:?}"
        );
    }
}
