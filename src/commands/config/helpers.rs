//! Argv helpers shared between the `config get/set/unset/list`
//! commands. Pulled out so adding a fifth `config` operation does
//! not require touching the file that owns these helpers.

use anyhow::{anyhow, Result};
use std::ffi::OsString;

use crate::config::Level;

/// Strip a leading `--system` / `--global` / `--local` level flag
/// when present and return the target level together with the
/// remaining args. Default level when no flag is given: `Global`,
/// matching git's own `--global` default for `git config`.
pub fn extract_level_flag(args: &[OsString]) -> Result<(Level, &[OsString])> {
    match args.first().and_then(|a| a.to_str()) {
        Some("--system") => Ok((Level::System, &args[1..])),
        Some("--global") => Ok((Level::Global, &args[1..])),
        Some("--local") => Ok((Level::Local, &args[1..])),
        _ => Ok((Level::Global, args)),
    }
}

/// Extract `args[idx]` as a UTF-8 string, or fail with the given
/// usage hint. Config keys and values are required to be UTF-8 even
/// on platforms where argv can carry raw bytes.
pub fn arg_as_str<'a>(args: &'a [OsString], idx: usize, usage: &str) -> Result<&'a str> {
    let arg = args.get(idx).ok_or_else(|| anyhow!("usage: {usage}"))?;
    arg.to_str().ok_or_else(|| {
        anyhow!(
            "argument {idx} is not valid UTF-8; config keys and values must be UTF-8\n\
             usage: {usage}"
        )
    })
}
