// ws log: show workspace history.
//
// At the workspace root: shows commits to the workspace repo (changes to
// manifest and state.toml over time).
//
// Inside a child repo: delegates to normal git log for that repo.
//
// Scaffolded for Phase 2; not wired from main in 0.1.0. Forwards to git via
// passthrough so that the scaffold remains compilable and honest until the
// real workspace-aware implementation lands.

use anyhow::Result;
use std::ffi::OsString;
use std::process::ExitCode;

use crate::context::Context;

pub fn run(_ctx: Context) -> Result<ExitCode> {
    let forward: Vec<OsString> = std::env::args_os().skip(1).collect();
    Ok(crate::commands::passthrough::run(&forward))
}
