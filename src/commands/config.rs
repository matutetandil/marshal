//! `marshal config` — manage Marshal's configuration.
//!
//! Dispatched from [`crate::cli::dispatch`] when the user types
//! `git marshal config <…>`. Step 5a exposes `get`, `set`, `unset`, and
//! `list`. All operations in step 5a target the global layer; explicit
//! `--system` / `--global` / `--local` flags arrive in step 5c.

use anyhow::{anyhow, Result};
use std::ffi::OsString;
use std::process::ExitCode;

use crate::config::{ConfigKey, ConfigResolver, Level};

pub fn dispatch(args: &[OsString]) -> Result<ExitCode> {
    match args.first().and_then(|s| s.to_str()) {
        None => {
            print_help();
            Ok(ExitCode::from(2))
        }
        Some("get") => handle_get(&args[1..]),
        Some("set") => handle_set(&args[1..]),
        Some("unset") => handle_unset(&args[1..]),
        Some("list") => handle_list(),
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(ExitCode::from(0))
        }
        Some(other) => {
            eprintln!(
                "marshal config: unknown subcommand '{other}'. \
                 Expected get, set, unset, or list."
            );
            Ok(ExitCode::from(2))
        }
    }
}

fn print_help() {
    println!("marshal config — manage Marshal's configuration.");
    println!();
    println!("Usage:");
    println!("  git marshal config get <key>");
    println!("  git marshal config set <key> <value>");
    println!("  git marshal config unset <key>");
    println!("  git marshal config list");
    println!();
    println!("Known keys:");
    for key in ConfigKey::all() {
        println!("  {:<20}  {}", key.as_dotted(), key.description());
    }
    println!();
    println!("In 0.2.0 all write operations target the per-user (global) layer.");
    println!("System and per-repo layers become writable in later releases.");
}

fn handle_get(args: &[OsString]) -> Result<ExitCode> {
    let key_str = arg_as_str(args, 0, "marshal config get <key>")?;
    let key = ConfigKey::from_dotted(key_str)?;

    let resolver = ConfigResolver::current_user()?;
    let effective = resolver.effective()?;
    println!("{}", effective.get_effective_string(key));
    Ok(ExitCode::from(0))
}

fn handle_set(args: &[OsString]) -> Result<ExitCode> {
    let key_str = arg_as_str(args, 0, "marshal config set <key> <value>")?;
    let value_str = arg_as_str(args, 1, "marshal config set <key> <value>")?;
    let key = ConfigKey::from_dotted(key_str)?;

    let resolver = ConfigResolver::current_user()?;
    resolver.mutate(Level::Global, |cfg| cfg.set_from_str(key, value_str))?;
    Ok(ExitCode::from(0))
}

fn handle_unset(args: &[OsString]) -> Result<ExitCode> {
    let key_str = arg_as_str(args, 0, "marshal config unset <key>")?;
    let key = ConfigKey::from_dotted(key_str)?;

    let resolver = ConfigResolver::current_user()?;
    resolver.mutate(Level::Global, |cfg| {
        cfg.unset(key);
        Ok(())
    })?;
    Ok(ExitCode::from(0))
}

fn handle_list() -> Result<ExitCode> {
    let resolver = ConfigResolver::current_user()?;
    let effective = resolver.effective()?;
    for key in ConfigKey::all() {
        println!(
            "{}={}",
            key.as_dotted(),
            effective.get_effective_string(*key)
        );
    }
    Ok(ExitCode::from(0))
}

/// Extract `args[idx]` as a UTF-8 string, or fail with the given usage hint.
fn arg_as_str<'a>(args: &'a [OsString], idx: usize, usage: &str) -> Result<&'a str> {
    let arg = args.get(idx).ok_or_else(|| anyhow!("usage: {usage}"))?;
    arg.to_str()
        .ok_or_else(|| {
            anyhow!("argument {idx} is not valid UTF-8; config keys and values must be UTF-8")
        })
        .map_err(|e| {
            // Make the actionable hint accompany the type error.
            anyhow!("{e}\nusage: {usage}")
        })
}
