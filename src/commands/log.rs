// ws log: show workspace history.
//
// At the workspace root: shows commits to the workspace repo (changes to
// manifest and state.toml over time).
//
// Inside a child repo: delegates to normal git log for that repo.
//
// This is a Phase 2 command. Currently passes through to git for basic cases.

use anyhow::Result;
use std::process::ExitCode;

use crate::context::Context;

pub fn run(ctx: Context) -> Result<ExitCode> {
    // For now, if we're inside a repo, just passthrough.
    if ctx.current_repo.is_some() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        return crate::commands::passthrough::run(&args);
    }

    // At workspace root: show workspace repo log.
    // TODO: enrich with state.toml diffs per commit.
    let status = crate::git::run_interactive(&ctx.root, &["log", "--oneline", "-20"])?;
    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(code as u8))
}
