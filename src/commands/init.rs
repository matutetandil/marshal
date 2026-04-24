// ws init: initialize a workspace in the current directory.
//
// Creates .workspace/ with a minimal manifest.toml and state.toml.
// Does NOT create the workspace repo as a git repo; that's the user's choice
// (git init separately, or point at an existing git repo).

use anyhow::{bail, Result};
use std::process::ExitCode;

use crate::context::{LOCAL_DIR, MANIFEST_FILE, STATE_FILE, WORKSPACE_MARKER};

pub fn run() -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let marker = cwd.join(WORKSPACE_MARKER);

    if marker.exists() {
        bail!(
            "workspace already initialized at {}\nremove {} to reinitialize",
            cwd.display(),
            marker.display()
        );
    }

    // Create directory structure
    std::fs::create_dir(&marker)?;
    std::fs::create_dir(marker.join(LOCAL_DIR))?;

    // Minimal manifest
    let manifest_content = format!(
        r#"[workspace]
name = "{}"
default_branch = "main"

# Add repos here:
# [[repos]]
# name = "service-a"
# url = "git@github.com:your-org/service-a.git"
# kind = "service"
"#,
        cwd.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "workspace".to_string())
    );
    std::fs::write(marker.join(MANIFEST_FILE), manifest_content)?;

    // Empty state declaration
    std::fs::write(
        marker.join(STATE_FILE),
        "# Declared state for current branch\n",
    )?;

    // Create .gitignore in .workspace/ to ignore local/
    std::fs::write(marker.join(".gitignore"), "local/\n")?;

    println!("Initialized workspace in {}", cwd.display());
    println!();
    println!("Next steps:");
    println!("  1. Edit .workspace/manifest.toml to declare your repos");
    println!("  2. Run 'git init' if this directory is not yet a git repo");
    println!("  3. Clone child repos into src/<repo-name>/");

    Ok(ExitCode::from(0))
}
