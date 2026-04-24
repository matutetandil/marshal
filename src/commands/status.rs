// ws status: show aggregated workspace status.
//
// Lists all child repos with their current branch, dirty state, and any
// divergence from the declared state.
//
// This is the fundamental visibility command. It must be fast and clear.

use anyhow::Result;
use owo_colors::OwoColorize;
use std::process::ExitCode;

use crate::context::{Context, MANIFEST_FILE, STATE_FILE, WORKSPACE_MARKER};
use crate::workspace::{Manifest, StateDeclaration};

pub fn run(ctx: Context, explain: bool, json: bool) -> Result<ExitCode> {
    let manifest_path = ctx.root.join(WORKSPACE_MARKER).join(MANIFEST_FILE);
    let state_path = ctx.root.join(WORKSPACE_MARKER).join(STATE_FILE);

    let manifest = Manifest::load(&manifest_path)?;
    let state = StateDeclaration::load(&state_path)?;

    if explain {
        return explain_mode(&ctx, &manifest);
    }

    if json {
        // TODO: implement JSON output for scripting
        eprintln!("--json not yet implemented");
        return Ok(ExitCode::from(2));
    }

    print_human_readable(&ctx, &manifest, &state)?;

    Ok(ExitCode::from(0))
}

fn print_human_readable(
    ctx: &Context,
    manifest: &Manifest,
    state: &StateDeclaration,
) -> Result<()> {
    println!("Workspace: {}", manifest.workspace.name.bold());
    println!("Root:      {}", ctx.root.display());

    // Workspace repo branch
    let workspace_branch =
        crate::git::current_branch(&ctx.root)?.unwrap_or_else(|| "(detached)".to_string());
    println!("Branch:    {}", workspace_branch.cyan());
    println!();

    if manifest.repos.is_empty() {
        println!("No repos declared in manifest.");
        println!("Edit {}/{} to add repos.", WORKSPACE_MARKER, MANIFEST_FILE);
        return Ok(());
    }

    println!("{}", "Repos:".bold());
    for repo_entry in &manifest.repos {
        let repo_path = ctx.root.join("src").join(&repo_entry.name);

        if !repo_path.exists() {
            println!(
                "  {} {}  {}",
                "?".yellow(),
                repo_entry.name,
                "(not cloned)".dimmed()
            );
            continue;
        }

        let actual_branch =
            crate::git::current_branch(&repo_path)?.unwrap_or_else(|| "(detached)".to_string());
        let dirty = crate::git::is_dirty(&repo_path)?;

        let declared = state.get(&repo_entry.name);
        let expected_branch = declared
            .map(|d| d.branch.as_str())
            .unwrap_or(&manifest.workspace.default_branch);

        let status_icon = if actual_branch != expected_branch {
            "⚡".yellow().to_string()
        } else if dirty {
            "●".cyan().to_string()
        } else {
            "✓".green().to_string()
        };

        let divergence = if actual_branch != expected_branch {
            format!("  (declared: {})", expected_branch)
                .dimmed()
                .to_string()
        } else {
            String::new()
        };

        let dirty_marker = if dirty {
            " [modified]".cyan().to_string()
        } else {
            String::new()
        };

        println!(
            "  {} {:<20} {}{}{}",
            status_icon, repo_entry.name, actual_branch, divergence, dirty_marker
        );
    }

    Ok(())
}

fn explain_mode(ctx: &Context, manifest: &Manifest) -> Result<ExitCode> {
    println!("Scope inference for 'status':");
    println!("  Policy: full workspace (no dimensions applied)");
    println!(
        "  Current directory: {}",
        std::env::current_dir()?.display()
    );
    println!("  Current repo context: {:?}", ctx.current_repo);
    println!("  Workspace root: {}", ctx.root.display());
    println!("  Repos in manifest: {}", manifest.repos.len());
    println!();
    println!("Plan:");
    println!("  For each repo, read:");
    println!("    - current branch (git symbolic-ref --short HEAD)");
    println!("    - dirty state (git status --porcelain)");
    println!("  Compare against state.toml declaration.");
    println!("  Report divergences.");
    println!();
    println!("No modifications made. Status is read-only.");
    Ok(ExitCode::from(0))
}
