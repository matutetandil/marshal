//! `ws reset` — clear the per-developer staging area in one go.
//!
//! Counterpart to `ws add` / `ws unstage`: those add or drop a
//! single entry; `ws reset` empties the file. Useful for "I want
//! to start over before composing the next workspace commit"
//! workflows, the workspace-level analogue of `git reset` (the
//! mode that empties the index without touching the working tree).
//!
//! Read-only with respect to child working trees — only
//! `.workspace/local/staged.toml` mutates. No pre-flight needed,
//! no `--auto-stash` / `--discard-changes` machinery; the staging
//! file is per-developer and discarding entries does not destroy
//! any committed work in any repo.
//!
//! Empty-staging case is benign: reports `was_empty: true` and
//! still rewrites the file (header preserved, body empty) to keep
//! the on-disk shape consistent. No error.
//!
//! `ws reset <repo>` would overlap with `ws unstage <repo>`. We
//! explicitly refuse positional arguments to keep the two commands
//! disjoint: `ws reset` for "clear all", `ws unstage` for "drop
//! one". `--on <name>` is rejected for the same reason.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};

use crate::cli::{Command, Renderable};
use crate::context;
use crate::workspace::staged::StagedDeclaration;

pub struct WsReset {
    pub on: Option<String>,
    pub explain: bool,
}

impl Command for WsReset {
    type Output = WsResetOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        // Refuse `--on <name>` and any positional argument before
        // doing anything else: both would overlap with `ws unstage`'s
        // semantics. Keep the two commands disjoint so users have
        // exactly one canonical form for each intent.
        if let Some(on_name) = &self.on {
            bail!(
                "ws reset: clears the entire staging area; it does not take a target. \
                 Use `ws unstage {on_name}` to drop a single entry."
            );
        }
        reject_positional(args)?;

        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here with `ws init`."
            )
        })?;

        if self.explain {
            let plan = vec![
                format!(
                    "load `{}/.workspace/local/staged.toml` (treat missing as empty)",
                    ctx.root.display()
                ),
                "drop every staged entry".to_string(),
                format!(
                    "write `{}/.workspace/local/staged.toml` with the header preserved \
                     and an empty body",
                    ctx.root.display()
                ),
            ];
            return Ok(WsResetOutput {
                root: ctx.root.to_string_lossy().into_owned(),
                cleared: Vec::new(),
                was_empty: true,
                explain_plan: Some(plan),
            });
        }

        let staged = StagedDeclaration::load_or_default(&ctx.root)
            .context("failed to load existing staging declaration")?;
        let was_empty = staged.is_empty();

        // Snapshot what we are about to clear, in stable order, for
        // both the human render and the JSON form.
        let mut cleared: Vec<ClearedEntry> = staged
            .repos
            .iter()
            .map(|(name, snap)| ClearedEntry {
                name: name.clone(),
                branch: snap.branch.clone(),
                commit: snap.commit.clone(),
            })
            .collect();
        cleared.sort_by(|a, b| a.name.cmp(&b.name));

        // Persist the empty declaration. We always write — even when
        // nothing changed — so the on-disk file's header stays
        // present and a future user opening it sees the documentation
        // comment. Cheap (small file) and keeps the file's existence
        // a stable invariant after the first stage.
        StagedDeclaration::default()
            .save_to_workspace(&ctx.root)
            .context("failed to persist cleared staging declaration")?;

        Ok(WsResetOutput {
            root: ctx.root.to_string_lossy().into_owned(),
            cleared,
            was_empty,
            explain_plan: None,
        })
    }
}

#[derive(Serialize)]
pub struct WsResetOutput {
    pub root: String,
    /// One entry per staged repo that was just cleared, sorted by
    /// repo name. Empty when nothing was staged. JSON consumers
    /// branch on `was_empty` rather than on `cleared.is_empty()`
    /// to distinguish "explain" from "real run on empty staging".
    pub cleared: Vec<ClearedEntry>,
    /// `true` when the staging area was already empty before the
    /// reset (no-op). The on-disk file is still rewritten so the
    /// header comment stays present.
    pub was_empty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ClearedEntry {
    pub name: String,
    pub branch: String,
    pub commit: String,
}

impl Renderable for WsResetOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws reset", plan);
        }
        if self.was_empty {
            writeln!(w, "Staging area was already empty — nothing to do.")?;
            return Ok(());
        }
        let n = self.cleared.len();
        let plural = if n == 1 { "entry" } else { "entries" };
        writeln!(w, "Cleared {n} staged {plural}:")?;
        let pad = self.cleared.iter().map(|e| e.name.len()).max().unwrap_or(0);
        for entry in &self.cleared {
            let short = short_sha(&entry.commit);
            writeln!(
                w,
                "  - {:<pad$}  was `{}`@{}",
                entry.name,
                entry.branch,
                short,
                pad = pad
            )?;
        }
        Ok(())
    }
}

// ── Argument parsing ──────────────────────────────────────────────

/// `ws reset` accepts only `--explain` (handled by the dispatcher).
/// Reject anything else loudly: a positional would overlap with
/// `ws unstage`, and an unknown flag is almost always a typo.
///
/// The function fails on the *first* extra argument; subsequent
/// args do not matter for the diagnostic. The clippy hint
/// "loop never actually loops" applies — `args.first()` is
/// clearer than a `for` body that always bails.
fn reject_positional(args: &[OsString]) -> Result<()> {
    let Some(first) = args.first() else {
        return Ok(());
    };
    let s = first.to_str().ok_or_else(|| {
        anyhow!(
            "ws reset: argument is not valid UTF-8: {:?}",
            first.to_string_lossy()
        )
    })?;
    if s.starts_with("--") {
        bail!(
            "ws reset: unexpected flag '{s}'. \
             `ws reset` clears the staging area and accepts only `--explain`. \
             To drop a single entry, use `ws unstage <repo>`."
        );
    }
    bail!(
        "ws reset: unexpected argument '{s}'. \
         `ws reset` clears the entire staging area; it does not take a target. \
         To drop a single entry, use `ws unstage {s}`."
    );
}

fn short_sha(sha: &str) -> &str {
    let n = sha.len().min(7);
    &sha[..n]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn empty_args_is_accepted() {
        assert!(reject_positional(&[]).is_ok());
    }

    #[test]
    fn rejects_positional_with_unstage_hint() {
        let err = reject_positional(&os(&["alpha"])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'alpha'"));
        assert!(msg.contains("ws unstage alpha"));
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = reject_positional(&os(&["--bogus"])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'--bogus'"));
        assert!(msg.contains("--explain"));
    }

    #[test]
    fn short_sha_truncates_to_seven() {
        assert_eq!(short_sha("abcdef1234567890"), "abcdef1");
    }

    #[test]
    fn short_sha_handles_short_input() {
        assert_eq!(short_sha("abc"), "abc");
    }
}
