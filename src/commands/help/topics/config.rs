//! `marshal help config` — comprehensive treatment of the
//! configuration system.

use crate::commands::help::topic::{HelpOutput, HelpSection, HelpTopic};
use crate::config::ConfigKey;

pub struct Config;

impl HelpTopic for Config {
    fn name(&self) -> &'static str {
        "config"
    }

    fn produce(&self) -> HelpOutput {
        let title = "Marshal configuration — the three-tier model.".to_string();

        let summary = HelpSection {
            heading: "Summary:".to_string(),
            body: vec![
                "Marshal mirrors git's three-tier config model: system < global < local."
                    .to_string(),
                "Higher tiers override lower; unset keys fall through to compiled-in defaults."
                    .to_string(),
            ],
        };

        let commands = HelpSection {
            heading: "Commands:".to_string(),
            body: vec![
                "marshal config get [--show-origin] <key>".to_string(),
                "marshal config set   [--system|--global|--local] <key> <value>".to_string(),
                "marshal config unset [--system|--global|--local] <key>".to_string(),
                "marshal config list".to_string(),
            ],
        };

        let levels = HelpSection {
            heading: "Levels (precedence: system < global < local):".to_string(),
            body: vec![
                "--global (default)  per-user config.".to_string(),
                "                    Unix:    $XDG_CONFIG_HOME/marshal/config.toml".to_string(),
                "                    Windows: %APPDATA%\\marshal\\config.toml".to_string(),
                "--system            machine-wide config.".to_string(),
                "                    Unix:    /etc/marshal/config.toml".to_string(),
                "                    Windows: %ProgramData%\\marshal\\config.toml".to_string(),
                "--local             per-repo config (inside `<git-dir>/marshal/`).".to_string(),
            ],
        };

        let keys = HelpSection {
            heading: "Known keys:".to_string(),
            body: ConfigKey::all()
                .iter()
                .map(|k| format!("{:<25}  {}", k.as_dotted(), k.description()))
                .collect(),
        };

        let envs = HelpSection {
            heading: "Override paths via environment (mostly for tests/CI):".to_string(),
            body: vec![
                "MARSHAL_CONFIG          Override the global path.".to_string(),
                "MARSHAL_SYSTEM_CONFIG   Override the system path.".to_string(),
                "MARSHAL_LOCAL_CONFIG    Override the local path.".to_string(),
            ],
        };

        let robustness = HelpSection {
            heading: "Robustness:".to_string(),
            body: vec![
                "A malformed config file does not abort commands. Marshal warns once on stderr"
                    .to_string(),
                "and falls back to defaults so the user's git command still completes.".to_string(),
            ],
        };

        HelpOutput {
            topic: self.name().to_string(),
            title,
            sections: vec![summary, commands, levels, keys, envs, robustness],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_topic_lists_every_command_form() {
        let out = Config.produce();
        let commands = out
            .sections
            .iter()
            .find(|s| s.heading.starts_with("Commands:"))
            .unwrap();
        for token in ["get", "set", "unset", "list"] {
            assert!(
                commands.body.iter().any(|l| l.contains(token)),
                "expected `{token}` mention, got: {:?}",
                commands.body
            );
        }
    }

    #[test]
    fn config_topic_documents_every_known_key() {
        let out = Config.produce();
        let keys = out
            .sections
            .iter()
            .find(|s| s.heading.starts_with("Known keys:"))
            .unwrap();
        for key in ConfigKey::all() {
            assert!(
                keys.body.iter().any(|l| l.contains(key.as_dotted())),
                "missing key: {}",
                key.as_dotted()
            );
        }
    }

    #[test]
    fn config_topic_documents_env_overrides() {
        let out = Config.produce();
        let body: Vec<_> = out.sections.iter().flat_map(|s| s.body.iter()).collect();
        assert!(body.iter().any(|l| l.contains("MARSHAL_CONFIG")));
        assert!(body.iter().any(|l| l.contains("MARSHAL_SYSTEM_CONFIG")));
        assert!(body.iter().any(|l| l.contains("MARSHAL_LOCAL_CONFIG")));
    }
}
