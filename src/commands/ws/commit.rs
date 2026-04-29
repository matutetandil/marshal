//! `ws commit` — flush the staging area into a workspace commit.
//!
//! The workspace-level analogue of `git commit`. Reads
//! `.workspace/local/staged.toml`, upserts every entry into the
//! workspace's `.workspace/state.toml`, runs `git commit -- <path>`
//! against just `state.toml`, and clears the staging area on
//! success.
//!
//! Behaviour mirrors `git commit` deliberately:
//!
//! * **Only the staged contents are committed.** Other files in the
//!   workspace repo's git index — Dockerfile, README, untracked
//!   files — are not touched. We use the `git commit -- <path>`
//!   form (`--only` semantics): the index is updated from the
//!   working tree just for `state.toml`, the commit captures
//!   that snapshot, and any other staged paths stay staged for the
//!   user's next plain-git commit.
//!
//! * **Empty stage / no real change → refuse.** "Nothing staged"
//!   and "every staged entry already matches the declared state"
//!   are distinct errors with distinct hints. Mirrors how
//!   `git commit` exits non-zero when the index has no changes.
//!
//! * **Editor support.** Without `-m`, git's commit invocation
//!   inherits stdio so the user's `$EDITOR` (or `core.editor`)
//!   takes over the terminal exactly as plain `git commit` does.
//!   The commit message is read back from the new HEAD afterwards
//!   so the JSON form still has it.
//!
//! `--on <name>` and any positional argument are rejected — `ws
//! commit` is workspace-wide; per-repo intent is expressed via
//! `ws add <repo>` ahead of the commit.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};

use crate::cli::{Command, OutputFormat, Renderable};
use crate::context::{self, STATE_FILE, WORKSPACE_MARKER};
use crate::workspace::manifest::Manifest;
use crate::workspace::staged::StagedDeclaration;
use crate::workspace::state::{self, RepoState, StateChange, StateDeclaration};

pub struct WsCommit {
    pub on: Option<String>,
    pub explain: bool,
    /// `Json` is rejected when `-m` is not provided — we cannot
    /// open `$EDITOR` and emit JSON in the same invocation. The
    /// dispatcher always passes the active format; we keep it on
    /// the struct so the validation reads naturally.
    pub format: OutputFormat,
}

impl Command for WsCommit {
    type Output = WsCommitOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        if let Some(on_name) = &self.on {
            bail!(
                "ws commit: takes no target — flushes the entire staging area. \
                 Use `ws unstage {on_name}` if you want to drop a single staged entry, \
                 or stage what you want first and re-run `ws commit`."
            );
        }
        let parsed = parse_args(args)?;

        // Editor mode + JSON would mean we'd open `$EDITOR` and
        // then emit JSON — the editor takes over the terminal,
        // which is incompatible with structured output for
        // automation. Fail fast with a clear hint.
        if parsed.message.is_none() && self.format == OutputFormat::Json {
            bail!(
                "ws commit: `--json` requires `-m <msg>`. \
                 Without `-m`, `ws commit` opens $EDITOR for the message — \
                 incompatible with JSON output. Pass `-m <msg>` or drop `--json`."
            );
        }

        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here with `ws init`."
            )
        })?;

        let manifest = Manifest::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace manifest")?
            .ok_or_else(|| {
                anyhow!(
                    "workspace at {} has no manifest yet.\n  \
                     Run `ws init` here to create one, or edit \
                     `.workspace/manifest.toml` directly.",
                    ctx.root.display()
                )
            })?;

        // The state.toml path relative to the workspace root, used
        // verbatim with `git commit -- <pathspec>`. POSIX form (forward
        // slashes) — git accepts it on every platform.
        let state_relpath = format!("{WORKSPACE_MARKER}/{STATE_FILE}");

        // --explain is independent of the current staging: it
        // describes the steps `ws commit` would perform if invoked
        // for real. Other ws commands behave the same way (the plan
        // is informational; concrete validation runs only on the
        // real path).
        if self.explain {
            let plan = build_explain_plan(&ctx.root, &state_relpath, parsed.message.as_deref());
            return Ok(WsCommitOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                workspace_name: manifest.workspace.name.clone(),
                changes: Vec::new(),
                cleared: Vec::new(),
                commit_sha: None,
                message: parsed.message,
                explain_plan: Some(plan),
            });
        }

        let state_before = StateDeclaration::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace state declaration")?
            .unwrap_or_default();
        let staged = StagedDeclaration::load_or_default(&ctx.root)
            .context("failed to load staging declaration")?;

        if staged.is_empty() {
            bail!(
                "ws commit: nothing staged. \
                 Run `ws add <repo>` first, then `ws commit`."
            );
        }

        // Compute the new state.toml: existing entries kept, every
        // staged entry upserted on top.
        let mut state_after = state_before.clone();
        for (name, snap) in &staged.repos {
            state_after.repos.insert(
                name.clone(),
                RepoState {
                    branch: snap.branch.clone(),
                    commit: Some(snap.commit.clone()),
                },
            );
        }

        let changes = state::diff_states(&state_before, &state_after);
        if changes.is_empty() {
            bail!(
                "ws commit: nothing to commit — every staged entry already matches \
                 the declared state in `.workspace/state.toml`. Drop the staging \
                 with `ws reset` (or `ws unstage <repo>` per entry)."
            );
        }

        let state_path = ctx.root.join(WORKSPACE_MARKER).join(STATE_FILE);

        let body = state_after
            .to_toml()
            .context("ws commit: failed to serialise the new state declaration")?;
        std::fs::write(&state_path, body).with_context(|| {
            format!(
                "ws commit: failed to write {} — the commit was not made; \
                 staging is unchanged",
                state_path.display()
            )
        })?;

        // `git commit -- <path>` requires the path to be known to
        // git (either in HEAD or in the index). On the very first
        // workspace commit the file is brand new — we need to stage
        // it first so the pathspec form works. `git add` is also
        // idempotent on already-tracked files, so this is safe to
        // run unconditionally.
        let add_out = ProcessCommand::new("git")
            .current_dir(&ctx.root)
            .args(["add", "--", &state_relpath])
            .output()
            .with_context(|| format!("failed to invoke `git add` in {}", ctx.root.display()))?;
        if !add_out.status.success() {
            bail!(
                "ws commit: `git add {state_relpath}` exited with status {}: {}",
                add_out.status,
                String::from_utf8_lossy(&add_out.stderr).trim()
            );
        }

        // git commit -- <path> runs with --only semantics: index is
        // refreshed from the working tree for the named path only,
        // commits that snapshot, leaves any other staged paths in
        // the index for the user's next commit. With -m we pass it
        // through; without, git opens $EDITOR which we let take over
        // the terminal via Stdio::inherit.
        let commit_status = run_git_commit(&ctx.root, parsed.message.as_deref(), &state_relpath)?;
        if !commit_status.success() {
            // The state.toml write succeeded but git commit failed
            // (editor cancelled, pre-commit hook, etc). The user can
            // recover: `git -C <root> restore --staged
            // .workspace/state.toml && git -C <root> restore .workspace/state.toml`
            // to revert, or fix the issue and `git commit -- ...` by hand.
            let code = commit_status
                .code()
                .map(|c| format!("{c}"))
                .unwrap_or_else(|| "?".to_string());
            bail!(
                "ws commit: `git commit` exited with status {code}. \
                 `state.toml` is left written and the staging area was \
                 not cleared. Inspect `git -C {} status` to see what \
                 happened; once resolved, re-run `ws commit` or commit \
                 by hand.",
                ctx.root.display()
            );
        }

        // Read back the new HEAD's sha + message so the output
        // reflects what got recorded (especially important for
        // editor mode, where the user wrote the message and we
        // never saw it directly).
        let commit_sha = read_head_sha(&ctx.root)
            .context("ws commit: commit succeeded but failed to read HEAD sha")?;
        let message = match parsed.message {
            Some(m) => Some(m),
            None => Some(
                read_head_message(&ctx.root)
                    .context("ws commit: commit succeeded but failed to read HEAD message")?,
            ),
        };

        // Clear the staging area. The file's existence is preserved
        // (header comment intact); only the entries are dropped —
        // same shape as `ws reset`.
        let cleared = snapshot_to_cleared(&staged);
        StagedDeclaration::default()
            .save_to_workspace(&ctx.root)
            .context(
                "ws commit: workspace was committed successfully but \
                 the staging file could not be cleared. Run `ws reset` \
                 manually to drop the leftover entries.",
            )?;

        Ok(WsCommitOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            workspace_name: manifest.workspace.name.clone(),
            changes,
            cleared,
            commit_sha: Some(commit_sha),
            message,
            explain_plan: None,
        })
    }
}

// ── git invocation helpers ────────────────────────────────────────

fn run_git_commit(
    workspace_root: &Path,
    message: Option<&str>,
    pathspec: &str,
) -> Result<std::process::ExitStatus> {
    let mut cmd = ProcessCommand::new("git");
    cmd.current_dir(workspace_root).arg("commit");
    match message {
        Some(msg) => {
            cmd.args(["-m", msg]);
            // Non-editor mode: capture git's stdout/stderr so its
            // "[main abc] msg" line does not pollute our own
            // (especially important under `--json`). On commit
            // failure the captured stderr ends up surfaced via the
            // outer error.
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }
        None => {
            // Editor mode — let `git commit` take over the terminal.
            cmd.stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
    }
    cmd.arg("--").arg(pathspec);
    cmd.status().with_context(|| {
        format!(
            "failed to spawn `git commit` in {}",
            workspace_root.display()
        )
    })
}

fn read_head_sha(workspace_root: &Path) -> Result<String> {
    let out = ProcessCommand::new("git")
        .current_dir(workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .with_context(|| {
            format!(
                "failed to invoke `git rev-parse HEAD` in {}",
                workspace_root.display()
            )
        })?;
    if !out.status.success() {
        bail!(
            "git rev-parse HEAD exited with status {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn read_head_message(workspace_root: &Path) -> Result<String> {
    let out = ProcessCommand::new("git")
        .current_dir(workspace_root)
        .args(["log", "-1", "--pretty=format:%B"])
        .output()
        .with_context(|| {
            format!(
                "failed to invoke `git log -1 --pretty=format:%B` in {}",
                workspace_root.display()
            )
        })?;
    if !out.status.success() {
        bail!(
            "git log exited with status {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

// ── --explain plan ────────────────────────────────────────────────

fn build_explain_plan(
    workspace_root: &Path,
    state_relpath: &str,
    message: Option<&str>,
) -> Vec<String> {
    let mut plan = vec![
        "load `.workspace/local/staged.toml` (must be non-empty)".to_string(),
        format!("read `{state_relpath}` (treat missing as empty)"),
        "upsert every staged entry into the in-memory state declaration".to_string(),
        "abort if the resulting declaration matches what is already on disk".to_string(),
        format!("write the new declaration to `{state_relpath}`"),
        format!(
            "git -C {root} add -- {state_relpath}",
            root = workspace_root.display()
        ),
    ];
    let commit_step = match message {
        Some(msg) => format!(
            "git -C {root} commit -m {msg:?} -- {state_relpath}",
            root = workspace_root.display(),
            msg = msg,
            state_relpath = state_relpath,
        ),
        None => format!(
            "git -C {root} commit -- {state_relpath}  (opens $EDITOR for the commit message)",
            root = workspace_root.display(),
            state_relpath = state_relpath,
        ),
    };
    plan.push(commit_step);
    plan.push("clear `.workspace/local/staged.toml` (header preserved, body empty)".to_string());
    plan
}

// ── Output helpers ────────────────────────────────────────────────

fn snapshot_to_cleared(staged: &StagedDeclaration) -> Vec<ClearedEntry> {
    let mut out: Vec<ClearedEntry> = staged
        .repos
        .iter()
        .map(|(name, snap)| ClearedEntry {
            name: name.clone(),
            branch: snap.branch.clone(),
            commit: snap.commit.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Serialize)]
pub struct WsCommitOutput {
    pub root: String,
    pub workspace_name: String,
    /// Per-repo state changes the commit recorded. Empty under
    /// `--explain` (we computed them but the renderer prefers the
    /// plan in that case).
    pub changes: Vec<StateChange>,
    /// Staged entries that were promoted into `state.toml`. Sorted
    /// by name. Same shape as `ws reset`'s `ClearedEntry`.
    pub cleared: Vec<ClearedEntry>,
    /// SHA of the new workspace commit. `None` under `--explain`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Final commit message — `-m`'s value if provided, otherwise
    /// what the user wrote in the editor. `None` under `--explain`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ClearedEntry {
    pub name: String,
    pub branch: String,
    pub commit: String,
}

impl Renderable for WsCommitOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws commit", plan);
        }
        let sha_short = self
            .commit_sha
            .as_deref()
            .map(short_sha)
            .unwrap_or("??????");
        writeln!(
            w,
            "Committed `{}` workspace at {sha_short}.",
            self.workspace_name
        )?;
        if let Some(msg) = self.message.as_deref().map(first_line) {
            if !msg.is_empty() {
                writeln!(w, "  Message: {msg}")?;
            }
        }
        writeln!(w)?;
        writeln!(w, "State changes:")?;
        let pad = self
            .changes
            .iter()
            .map(|c| c.name().len())
            .max()
            .unwrap_or(0);
        for change in &self.changes {
            match change {
                StateChange::Added { name, branch } => {
                    writeln!(w, "  + {:<pad$}  declared on `{branch}`", name, pad = pad)?;
                }
                StateChange::Removed { name, branch } => {
                    writeln!(
                        w,
                        "  - {:<pad$}  declaration removed (was `{branch}`)",
                        name,
                        pad = pad
                    )?;
                }
                StateChange::Changed { name, from, to } => {
                    writeln!(w, "  ~ {:<pad$}  `{from}` → `{to}`", name, pad = pad)?;
                }
            }
        }
        writeln!(w)?;
        let n = self.cleared.len();
        let plural = if n == 1 { "entry" } else { "entries" };
        writeln!(w, "Cleared {n} staged {plural}.")?;
        Ok(())
    }
}

fn short_sha(sha: &str) -> &str {
    let n = sha.len().min(7);
    &sha[..n]
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

// ── Argument parsing ──────────────────────────────────────────────

#[derive(Debug)]
struct ParsedArgs {
    message: Option<String>,
}

/// `ws commit` takes only `-m <msg>` / `--message <msg>` /
/// `--message=<msg>`. Anything else is rejected, including
/// positional arguments.
fn parse_args(args: &[OsString]) -> Result<ParsedArgs> {
    let mut message: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws commit: argument {} is not valid UTF-8: {:?}",
                i,
                arg.to_string_lossy()
            )
        })?;

        if s == "-m" || s == "--message" {
            let value = args.get(i + 1).ok_or_else(|| {
                anyhow!("ws commit: '{s}' expects a value (e.g. `{s} \"my message\"`)")
            })?;
            let value = value
                .to_str()
                .ok_or_else(|| anyhow!("ws commit: value for '{s}' is not valid UTF-8"))?;
            assign_message(&mut message, value)?;
            i += 2;
            continue;
        }
        if let Some(value) = s.strip_prefix("--message=") {
            assign_message(&mut message, value)?;
            i += 1;
            continue;
        }
        if s.starts_with("--") || s.starts_with('-') {
            bail!(
                "ws commit: unexpected flag '{s}'. \
                 Expected: `ws commit [-m <msg> | --message <msg> | --message=<msg>]`."
            );
        }
        bail!(
            "ws commit: unexpected positional argument '{s}'. \
             `ws commit` flushes the staging area; it does not take per-repo targets. \
             Stage the repos with `ws add <repo>` first, then re-run `ws commit`."
        );
    }
    Ok(ParsedArgs { message })
}

fn assign_message(slot: &mut Option<String>, value: &str) -> Result<()> {
    if slot.is_some() {
        bail!("ws commit: commit message specified more than once");
    }
    if value.is_empty() {
        bail!("ws commit: commit message cannot be empty");
    }
    *slot = Some(value.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn parse_args_no_message_is_editor_mode() {
        let p = parse_args(&[]).unwrap();
        assert!(p.message.is_none());
    }

    #[test]
    fn parse_args_short_message_flag() {
        let p = parse_args(&os(&["-m", "release v2.4.8"])).unwrap();
        assert_eq!(p.message.as_deref(), Some("release v2.4.8"));
    }

    #[test]
    fn parse_args_long_message_flag_separated() {
        let p = parse_args(&os(&["--message", "hello"])).unwrap();
        assert_eq!(p.message.as_deref(), Some("hello"));
    }

    #[test]
    fn parse_args_long_message_flag_equals_form() {
        let p = parse_args(&os(&["--message=hello"])).unwrap();
        assert_eq!(p.message.as_deref(), Some("hello"));
    }

    #[test]
    fn parse_args_rejects_duplicate_message() {
        let err = parse_args(&os(&["-m", "a", "-m", "b"])).unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }

    #[test]
    fn parse_args_rejects_empty_message() {
        let err = parse_args(&os(&["-m", ""])).unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn parse_args_rejects_dangling_m() {
        let err = parse_args(&os(&["-m"])).unwrap_err();
        assert!(err.to_string().contains("expects a value"));
    }

    #[test]
    fn parse_args_rejects_unknown_flag() {
        let err = parse_args(&os(&["--bogus"])).unwrap_err();
        assert!(err.to_string().contains("unexpected flag"));
    }

    #[test]
    fn parse_args_rejects_positional_with_add_hint() {
        let err = parse_args(&os(&["alpha"])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'alpha'"));
        assert!(msg.contains("ws add"));
    }

    #[test]
    fn short_sha_truncates_to_seven() {
        assert_eq!(short_sha("abcdef1234567890"), "abcdef1");
    }

    #[test]
    fn first_line_grabs_one_line_only() {
        assert_eq!(first_line("hello\n\nbody text"), "hello");
        assert_eq!(first_line("  trim me  "), "trim me");
        assert_eq!(first_line(""), "");
    }
}
