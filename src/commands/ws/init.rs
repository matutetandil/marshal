//! `ws init` — create a `.workspace/` directory in the current
//! directory, with a starter `manifest.toml` and an empty
//! `state.toml`. First workspace command that writes to disk.
//!
//! Refuses to run when the cwd is already inside a workspace
//! (Invariant 8: Conservative Defaults — never modify state without
//! the user being explicit). `--force` overrides; it overwrites
//! `manifest.toml` and `state.toml` but leaves any other files in
//! `.workspace/` (e.g. a `local/` directory) intact.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::process::Command as ProcessCommand;

use crate::cli::{Command, Renderable};
use crate::context::{self, MANIFEST_FILE, STATE_FILE, WORKSPACE_MARKER};
use crate::workspace::manifest::{Manifest, WorkspaceMeta};

/// Initial content of `state.toml` — a header comment so a user
/// who opens the file for the first time knows what shape entries
/// take. The TOML body is empty (every repo defaults to the
/// manifest's default branch).
const STATE_FILE_HEADER: &str = "\
# state.toml — declares the expected state of each child repo
# for the currently active branch of the workspace repo.
# Entries are added as:
#   [repos.\"<repo-name>\"]
#   branch = \"<branch-name>\"
#   commit = \"<optional-sha>\"
";

pub struct WsInit {
    pub explain: bool,
}

impl Command for WsInit {
    type Output = WsInitOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let parsed = parse_args(args)?;

        // Refuse if already inside a workspace, unless --force.
        // We still run this check under --explain so the user sees
        // the same error they'd get without it.
        if !parsed.force {
            if let Some(ctx) = context::detect()? {
                bail!(
                    "already inside a marshal workspace at {}.\n  \
                     Run `ws init --force` to overwrite the manifest and state \
                     here, or `cd` somewhere outside before re-initialising.",
                    ctx.root.display()
                );
            }
        }

        let cwd = std::env::current_dir().context("failed to read current directory")?;

        let workspace_name = parsed
            .name
            .clone()
            .or_else(|| {
                cwd.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            })
            // Last resort when cwd has no filename component
            // (e.g. invoked at filesystem root). Rare, but keep the
            // command from panicking.
            .unwrap_or_else(|| "workspace".to_string());

        let default_branch = parsed
            .default_branch
            .clone()
            .unwrap_or_else(detect_init_default_branch);

        // Build the manifest in memory. Used in both branches:
        // for --explain we surface its serialised size; for the
        // real run we write it to disk.
        let workspace_dir = cwd.join(WORKSPACE_MARKER);
        let manifest_path = workspace_dir.join(MANIFEST_FILE);
        let state_path = workspace_dir.join(STATE_FILE);

        let manifest = Manifest {
            workspace: WorkspaceMeta {
                name: workspace_name.clone(),
                default_branch: default_branch.clone(),
            },
            repos: Vec::new(),
            affinities: HashMap::new(),
        };
        let manifest_toml =
            toml::to_string_pretty(&manifest).context("failed to serialise manifest")?;

        // --explain: list what we *would* do, without doing it.
        if self.explain {
            let plan = vec![
                format!("create directory `{}`", workspace_dir.display()),
                format!(
                    "write `{}` ({} bytes — workspace name `{}`, default branch `{}`)",
                    manifest_path.display(),
                    manifest_toml.len(),
                    workspace_name,
                    default_branch
                ),
                format!(
                    "write `{}` ({} bytes — header comment + empty body)",
                    state_path.display(),
                    STATE_FILE_HEADER.len()
                ),
            ];
            return Ok(WsInitOutput {
                root: cwd.to_string_lossy().into_owned(),
                workspace_name,
                default_branch,
                created_files: vec![
                    format!("{WORKSPACE_MARKER}/{MANIFEST_FILE}"),
                    format!("{WORKSPACE_MARKER}/{STATE_FILE}"),
                ],
                forced: parsed.force,
                explain_plan: Some(plan),
            });
        }

        // Real run: create the directory and write the files.
        fs::create_dir_all(&workspace_dir).with_context(|| {
            format!(
                "failed to create workspace directory at {}",
                workspace_dir.display()
            )
        })?;
        fs::write(&manifest_path, manifest_toml)
            .with_context(|| format!("failed to write manifest at {}", manifest_path.display()))?;

        // Write `state.toml` with a header comment. An empty TOML
        // body is intentional: every repo defaults to the manifest's
        // default branch until the user pins one explicitly.
        fs::write(&state_path, STATE_FILE_HEADER).with_context(|| {
            format!(
                "failed to write state declaration at {}",
                state_path.display()
            )
        })?;

        Ok(WsInitOutput {
            root: cwd.to_string_lossy().into_owned(),
            workspace_name,
            default_branch,
            created_files: vec![
                format!("{WORKSPACE_MARKER}/{MANIFEST_FILE}"),
                format!("{WORKSPACE_MARKER}/{STATE_FILE}"),
            ],
            forced: parsed.force,
            explain_plan: None,
        })
    }
}

#[derive(Serialize)]
pub struct WsInitOutput {
    pub root: String,
    pub workspace_name: String,
    pub default_branch: String,
    /// Paths (relative to `root`) of files marshal wrote (or
    /// would have written, under `--explain`) during this init.
    pub created_files: Vec<String>,
    /// `true` when `--force` was used to overwrite an existing
    /// workspace's manifest/state. JSON consumers can branch on
    /// this; humans see a different headline.
    pub forced: bool,
    /// `Some(plan)` when `--explain` was set — the steps marshal
    /// would have performed, none of which actually ran. JSON
    /// drops the field when the command ran for real.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

impl Renderable for WsInitOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws init", plan);
        }

        let action = if self.forced {
            "Re-initialised"
        } else {
            "Initialised"
        };
        writeln!(
            w,
            "{action} workspace `{}` (default branch: {})",
            self.workspace_name, self.default_branch
        )?;
        writeln!(w, "  Root: {}", self.root)?;
        for path in &self.created_files {
            writeln!(w, "  Wrote: {path}")?;
        }
        writeln!(w)?;
        writeln!(w, "Next steps:")?;
        writeln!(
            w,
            "  Edit `.workspace/manifest.toml` to declare child repos."
        )?;
        writeln!(w, "  Run `git ws` to verify the workspace context.")
    }
}

// ── Argument parsing ──────────────────────────────────────────────

#[derive(Debug)]
struct ParsedArgs {
    name: Option<String>,
    default_branch: Option<String>,
    force: bool,
}

fn parse_args(args: &[OsString]) -> Result<ParsedArgs> {
    let mut name: Option<String> = None;
    let mut default_branch: Option<String> = None;
    let mut force = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws init: argument {} is not valid UTF-8: {:?}",
                i,
                arg.to_string_lossy()
            )
        })?;

        if s == "--force" {
            force = true;
            i += 1;
        } else if let Some(value) = s.strip_prefix("--name=") {
            assign_once(&mut name, value, "--name")?;
            i += 1;
        } else if s == "--name" {
            let value = consume_value(args, i, "--name")?;
            assign_once(&mut name, &value, "--name")?;
            i += 2;
        } else if let Some(value) = s.strip_prefix("--default-branch=") {
            assign_once(&mut default_branch, value, "--default-branch")?;
            i += 1;
        } else if s == "--default-branch" {
            let value = consume_value(args, i, "--default-branch")?;
            assign_once(&mut default_branch, &value, "--default-branch")?;
            i += 2;
        } else {
            bail!(
                "ws init: unexpected argument '{s}'. \
                 Expected --name <name>, --default-branch <branch>, or --force."
            );
        }
    }

    Ok(ParsedArgs {
        name,
        default_branch,
        force,
    })
}

fn consume_value(args: &[OsString], idx: usize, flag: &str) -> Result<String> {
    args.get(idx + 1)
        .ok_or_else(|| anyhow!("ws init: '{flag}' expects a value"))?
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("ws init: value for '{flag}' is not valid UTF-8"))
}

fn assign_once(slot: &mut Option<String>, value: &str, flag: &str) -> Result<()> {
    if slot.is_some() {
        bail!("ws init: '{flag}' specified more than once");
    }
    *slot = Some(value.to_string());
    Ok(())
}

// ── Defaults ──────────────────────────────────────────────────────

/// Read `init.defaultBranch` from the user's git config; fall back
/// to `main` if unset or unreadable. Matches what `git init` itself
/// would do — the workspace and the user's plain-git workflow stay
/// aligned by default.
fn detect_init_default_branch() -> String {
    let output = ProcessCommand::new("git")
        .args(["config", "--get", "init.defaultBranch"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "main".to_string()
            } else {
                s
            }
        }
        _ => "main".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn parse_args_empty() {
        let p = parse_args(&[]).unwrap();
        assert!(p.name.is_none());
        assert!(p.default_branch.is_none());
        assert!(!p.force);
    }

    #[test]
    fn parse_args_supports_separated_and_equals_forms() {
        let p = parse_args(&os(&["--name", "foo", "--default-branch=develop"])).unwrap();
        assert_eq!(p.name.as_deref(), Some("foo"));
        assert_eq!(p.default_branch.as_deref(), Some("develop"));
        assert!(!p.force);
    }

    #[test]
    fn parse_args_force_is_boolean() {
        let p = parse_args(&os(&["--force"])).unwrap();
        assert!(p.force);
    }

    #[test]
    fn parse_args_rejects_duplicate_flags() {
        let err = parse_args(&os(&["--name", "a", "--name", "b"])).unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }

    #[test]
    fn parse_args_rejects_unknown_flag() {
        let err = parse_args(&os(&["--bogus"])).unwrap_err();
        assert!(err.to_string().contains("unexpected argument"));
    }

    #[test]
    fn parse_args_rejects_dangling_value() {
        let err = parse_args(&os(&["--name"])).unwrap_err();
        assert!(err.to_string().contains("expects a value"));
    }

    #[test]
    fn detect_default_branch_falls_back_to_main_or_returns_a_value() {
        let value = detect_init_default_branch();
        // Either `main` (fallback) or whatever the host's git
        // config has set. Either way: non-empty.
        assert!(!value.is_empty());
    }

    /// `WORKSPACE_MARKER`, `MANIFEST_FILE`, `STATE_FILE` are kept
    /// in sync between `context.rs` and the path-building here.
    /// Sanity check, not a behavioural test — catches accidental
    /// rename of the constants. Asserts the values directly so
    /// the test stays platform-agnostic (`PathBuf::join` uses `\`
    /// on Windows and `/` elsewhere; comparing string-formed paths
    /// would diverge between platforms).
    #[test]
    fn workspace_path_constants_have_expected_names() {
        assert_eq!(WORKSPACE_MARKER, ".workspace");
        assert_eq!(MANIFEST_FILE, "manifest.toml");
        assert_eq!(STATE_FILE, "state.toml");

        // `PathBuf::join` composes correctly on every platform; we
        // don't compare the rendered string but we do check that
        // the leaf is what we expect.
        let manifest = PathBuf::from(WORKSPACE_MARKER).join(MANIFEST_FILE);
        assert_eq!(manifest.file_name().unwrap(), MANIFEST_FILE);
        let state = PathBuf::from(WORKSPACE_MARKER).join(STATE_FILE);
        assert_eq!(state.file_name().unwrap(), STATE_FILE);
    }
}
