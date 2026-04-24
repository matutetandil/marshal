// ws clone: clone a workspace and all its child repos.
//
// Takes a URL pointing at a workspace repo. Clones that repo, reads the
// manifest, and clones all declared child repos in parallel.
//
// This is a Phase 2 command. Currently a stub.

use anyhow::Result;
use std::process::ExitCode;

pub fn run(url: &str) -> Result<ExitCode> {
    // TODO: implement in Phase 2
    eprintln!("ws clone not yet implemented");
    eprintln!("  would clone workspace from: {}", url);
    eprintln!();
    eprintln!("Implementation plan:");
    eprintln!("  1. git clone <url> <target-dir>");
    eprintln!("  2. Read .workspace/manifest.toml from the cloned repo");
    eprintln!("  3. For each repo in manifest, clone it into src/<n>/ in parallel");
    eprintln!("  4. Apply state.toml to set each repo to its declared branch");
    Ok(ExitCode::from(2))
}
