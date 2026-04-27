//! The `marshal help` overview topic — the screen the user lands on
//! when they type `marshal help` with no argument.
//!
//! Context-aware: the first section adapts to whether the user is
//! inside a git repository or outside one, so the suggested next
//! moves line up with what the cwd actually allows.

use crate::commands::help::topic::{HelpContext, HelpOutput, HelpSection, HelpTopic};
use crate::config::ConfigKey;

pub struct Overview;

impl HelpTopic for Overview {
    fn name(&self) -> &'static str {
        "overview"
    }

    fn produce(&self) -> HelpOutput {
        produce_with_ctx(&HelpContext::detect())
    }
}

/// Test seam: produce the overview given an explicit context. The
/// real `produce` calls `HelpContext::detect()` and forwards here;
/// unit tests drive both branches by passing synthetic contexts.
pub(crate) fn produce_with_ctx(ctx: &HelpContext) -> HelpOutput {
    let title = format!(
        "Marshal {} — a transparent wrapper for git.",
        env!("CARGO_PKG_VERSION")
    );

    let context_intro = context_intro(ctx);
    let subcommands = subcommands_section();
    let workspace_ns = workspace_namespace_section();
    let config_keys = config_keys_section();
    let topics = topics_section();
    let global_flags = global_flags_section();
    let pointers = pointers_section();

    HelpOutput {
        topic: "overview".to_string(),
        title,
        sections: vec![
            context_intro,
            subcommands,
            workspace_ns,
            config_keys,
            topics,
            global_flags,
            pointers,
        ],
    }
}

fn context_intro(ctx: &HelpContext) -> HelpSection {
    if ctx.in_git_repo {
        HelpSection {
            heading: "You're inside a Git repository. Quick start:".to_string(),
            body: vec![
                "marshal what-now           See what you should do next.".to_string(),
                "marshal config list        Inspect Marshal's configuration.".to_string(),
                "git status                 Standard git (passes through unchanged).".to_string(),
            ],
        }
    } else {
        HelpSection {
            heading: "You're outside a Git repository. To begin:".to_string(),
            body: vec![
                "`git init` here to start a new repo, or `cd` into an existing one.".to_string(),
                "marshal config list        Inspect Marshal's configuration (works anywhere)."
                    .to_string(),
            ],
        }
    }
}

fn subcommands_section() -> HelpSection {
    HelpSection {
        heading: "Subcommands (under the `marshal` namespace):".to_string(),
        body: vec![
            "config     Manage Marshal configuration (get/set/unset/list).".to_string(),
            "what-now   Analyse repo state and suggest the next action.".to_string(),
            "help       Print this overview, or `help <topic>` for details.".to_string(),
        ],
    }
}

fn workspace_namespace_section() -> HelpSection {
    HelpSection {
        heading: "Workspace operations (sibling namespace, opt-in):".to_string(),
        body: vec![
            "git ws     Show the current workspace context.".to_string(),
            "           More commands (init/status/log/diff/clone) ship in Phase 2.".to_string(),
        ],
    }
}

fn config_keys_section() -> HelpSection {
    HelpSection {
        heading: "Configuration keys:".to_string(),
        body: ConfigKey::all()
            .iter()
            .map(|k| format!("{:<25}  {}", k.as_dotted(), k.description()))
            .collect(),
    }
}

fn topics_section() -> HelpSection {
    HelpSection {
        heading: "Topics (run `marshal help <topic>` for any of them):".to_string(),
        body: vec![
            "overview     This screen.".to_string(),
            "config       Marshal's configuration system.".to_string(),
            "hints        Actionable error hints below git failures.".to_string(),
            "modernize    Modernization tips for legacy git forms.".to_string(),
            "what-now     The what-now command in detail.".to_string(),
        ],
    }
}

fn global_flags_section() -> HelpSection {
    HelpSection {
        heading: "Global flags (anywhere in argv after `marshal`):".to_string(),
        body: vec!["--json   Emit JSON instead of the human form.".to_string()],
    }
}

fn pointers_section() -> HelpSection {
    HelpSection {
        heading: "More:".to_string(),
        body: vec![
            "Project: https://github.com/matutetandil/marshal".to_string(),
            "Design:  see `docs/PRINCIPLES.md` and `docs/ARCHITECTURE.md`.".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(in_repo: bool) -> HelpContext {
        HelpContext {
            in_git_repo: in_repo,
        }
    }

    #[test]
    fn overview_carries_a_subcommands_section() {
        let out = produce_with_ctx(&ctx(true));
        assert_eq!(out.topic, "overview");
        assert!(out.title.contains("Marshal"));
        assert!(out
            .sections
            .iter()
            .any(|s| s.heading.starts_with("Subcommands")));
    }

    #[test]
    fn overview_carries_a_workspace_namespace_section() {
        let out = produce_with_ctx(&ctx(true));
        let workspace = out
            .sections
            .iter()
            .find(|s| s.heading.contains("Workspace operations"))
            .expect("workspace namespace section present");
        assert!(workspace.body.iter().any(|l| l.contains("git ws")));
    }

    #[test]
    fn overview_lists_every_known_config_key() {
        let out = produce_with_ctx(&ctx(true));
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
        let out = produce_with_ctx(&ctx(true));
        let body: Vec<_> = out.sections.iter().flat_map(|s| s.body.iter()).collect();
        assert!(
            body.iter().any(|l| l.contains("--json")),
            "expected --json mention in some section, got: {body:?}"
        );
    }

    #[test]
    fn intro_in_repo_recommends_what_now_and_passthrough_status() {
        let intro = context_intro(&ctx(true));
        assert!(intro.heading.contains("inside a Git repository"));
        assert!(intro.body.iter().any(|l| l.contains("marshal what-now")));
        assert!(intro.body.iter().any(|l| l.contains("git status")));
    }

    #[test]
    fn intro_outside_repo_recommends_init_or_cd() {
        let intro = context_intro(&ctx(false));
        assert!(intro.heading.contains("outside a Git repository"));
        assert!(intro.body.iter().any(|l| l.contains("git init")));
        assert!(intro.body.iter().any(|l| l.contains("`cd`")));
        // Does NOT recommend what-now (which would fail outside a repo).
        assert!(
            !intro.body.iter().any(|l| l.contains("marshal what-now")),
            "outside-repo intro must not recommend what-now"
        );
    }

    #[test]
    fn overview_first_section_is_context_aware_intro() {
        let out_in = produce_with_ctx(&ctx(true));
        let out_out = produce_with_ctx(&ctx(false));
        assert!(out_in.sections[0].heading.contains("inside"));
        assert!(out_out.sections[0].heading.contains("outside"));
    }
}
