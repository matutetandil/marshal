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

/// Strip a leading `--system` / `--global` / `--local` level flag (if
/// present) and return the target level together with the remaining args.
/// Default level when no flag is given: `Global`.
fn extract_level_flag(args: &[OsString]) -> Result<(Level, &[OsString])> {
    match args.first().and_then(|a| a.to_str()) {
        Some("--system") => Ok((Level::System, &args[1..])),
        Some("--global") => Ok((Level::Global, &args[1..])),
        Some("--local") => Ok((Level::Local, &args[1..])),
        _ => Ok((Level::Global, args)),
    }
}

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
    println!("  git marshal config get [--show-origin] <key>");
    println!("  git marshal config set   [--system|--global|--local] <key> <value>");
    println!("  git marshal config unset [--system|--global|--local] <key>");
    println!("  git marshal config list");
    println!();
    println!("Known keys:");
    for key in ConfigKey::all() {
        println!("  {:<20}  {}", key.as_dotted(), key.description());
    }
    println!();
    println!("Levels (precedence: system < global < local):");
    println!("  --global (default)  per-user config ($XDG_CONFIG_HOME/marshal/config.toml)");
    println!("  --system            machine-wide config (/etc/marshal/config.toml on Unix)");
    println!("  --local             per-repo config (<git-dir>/marshal/config.toml)");
}

fn handle_get(args: &[OsString]) -> Result<ExitCode> {
    // Optional leading --show-origin flag. Only accepted before the key, for
    // simplicity (mirrors how --system/--global/--local are positioned).
    let (show_origin, rest) = if args.first().and_then(|a| a.to_str()) == Some("--show-origin") {
        (true, &args[1..])
    } else {
        (false, args)
    };

    let usage = "marshal config get [--show-origin] <key>";
    let key_str = arg_as_str(rest, 0, usage)?;
    let key = ConfigKey::from_dotted(key_str)?;

    let resolver = ConfigResolver::current_user()?;

    if show_origin {
        // Tab-separated `<origin>\t<value>`. Origin is `system`/`global`/
        // `local` when some layer has the key set, or `default` when we
        // fall back to the compiled-in default.
        match resolver.origin_of(key)? {
            Some((level, value)) => println!("{}\t{}", level.as_str(), value),
            None => {
                let effective = resolver.effective()?;
                println!("default\t{}", effective.get_effective_string(key));
            }
        }
    } else {
        let effective = resolver.effective()?;
        println!("{}", effective.get_effective_string(key));
    }
    Ok(ExitCode::from(0))
}

fn handle_set(args: &[OsString]) -> Result<ExitCode> {
    let (level, rest) = extract_level_flag(args)?;
    let usage = "marshal config set [--system|--global] <key> <value>";
    let key_str = arg_as_str(rest, 0, usage)?;
    let value_str = arg_as_str(rest, 1, usage)?;
    let key = ConfigKey::from_dotted(key_str)?;

    let resolver = ConfigResolver::current_user()?;
    resolver.mutate(level, |cfg| cfg.set_from_str(key, value_str))?;
    Ok(ExitCode::from(0))
}

fn handle_unset(args: &[OsString]) -> Result<ExitCode> {
    let (level, rest) = extract_level_flag(args)?;
    let usage = "marshal config unset [--system|--global] <key>";
    let key_str = arg_as_str(rest, 0, usage)?;
    let key = ConfigKey::from_dotted(key_str)?;

    let resolver = ConfigResolver::current_user()?;
    resolver.mutate(level, |cfg| {
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
