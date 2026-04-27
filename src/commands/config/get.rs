//! `marshal config get` — read one config key's effective value.

use anyhow::Result;
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};

use crate::cli::{Command, Renderable};
use crate::config::{ConfigKey, ConfigResolver};

use super::helpers::arg_as_str;

pub struct ConfigGet;

impl Command for ConfigGet {
    type Output = GetOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        // Optional leading `--show-origin` flag — same position as
        // before the migration, so existing scripts keep working.
        let (show_origin, rest) = if args.first().and_then(|a| a.to_str()) == Some("--show-origin")
        {
            (true, &args[1..])
        } else {
            (false, args)
        };

        let usage = "marshal config get [--show-origin] <key>";
        let key_str = arg_as_str(rest, 0, usage)?;
        let key = ConfigKey::from_dotted(key_str)?;

        let resolver = ConfigResolver::current_user()?;

        let key_label = key.as_dotted().to_string();
        if show_origin {
            match resolver.origin_of(key)? {
                Some((level, value)) => Ok(GetOutput {
                    key: key_label,
                    value,
                    origin: Some(level.as_str().to_string()),
                }),
                None => {
                    let effective = resolver.effective()?;
                    Ok(GetOutput {
                        key: key_label,
                        value: effective.get_effective_string(key),
                        // "default" is also what the human form
                        // prints when no layer has the key set,
                        // so the JSON shape stays consistent.
                        origin: Some("default".to_string()),
                    })
                }
            }
        } else {
            let effective = resolver.effective()?;
            Ok(GetOutput {
                key: key_label,
                value: effective.get_effective_string(key),
                origin: None,
            })
        }
    }
}

#[derive(Serialize)]
pub struct GetOutput {
    pub key: String,
    pub value: String,

    /// `Some(level)` when `--show-origin` was passed (one of
    /// "system" / "global" / "local" / "default"). `None` is dropped
    /// from the JSON shape so plain `config get` produces just
    /// `{"key": "...", "value": "..."}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl Renderable for GetOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        match &self.origin {
            // Tab-separated `<origin>\t<value>` — same shape as before
            // the migration, so scripts that key off `cut -f 1/2`
            // keep working.
            Some(origin) => writeln!(w, "{origin}\t{}", self.value),
            None => writeln!(w, "{}", self.value),
        }
    }
}
