// Passthrough command: forward everything to git verbatim.
//
// Used when:
//   - We're not inside a workspace (plain Git repo or nowhere).
//   - The user passed --raw.
//   - A subcommand we don't recognize needs to go to git.
//
// Passthrough preserves git's exact behavior. Output goes to the user's
// terminal unchanged, including colors and interactive prompts.

use anyhow::Result;
use std::process::ExitCode;

pub fn run(args: &[String]) -> Result<ExitCode> {
    tracing::debug!("passthrough: forwarding to git with args {:?}", args);

    // Strip --raw if present so git doesn't see it
    let args: Vec<&str> = args
        .iter()
        .filter(|a| a.as_str() != "--raw")
        .map(String::as_str)
        .collect();

    let cwd = std::env::current_dir()?;
    let status = crate::git::run_interactive(&cwd, &args)?;

    // Forward git's exit code
    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(code as u8))
}
