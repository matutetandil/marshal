//! `marshal config unset` — clear a key from a specific config layer.

use anyhow::Result;
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};

use crate::cli::{Command, Renderable};
use crate::config::{ConfigKey, ConfigResolver};

use super::helpers::{arg_as_str, extract_level_flag};

pub struct ConfigUnset;

impl Command for ConfigUnset {
    type Output = UnsetOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let (level, rest) = extract_level_flag(args)?;
        let usage = "marshal config unset [--system|--global|--local] <key>";
        let key_str = arg_as_str(rest, 0, usage)?;
        let key = ConfigKey::from_dotted(key_str)?;

        let resolver = ConfigResolver::current_user()?;
        resolver.mutate(level, |cfg| {
            cfg.unset(key);
            Ok(())
        })?;

        Ok(UnsetOutput {
            key: key.as_dotted().to_string(),
            level: level.as_str().to_string(),
        })
    }
}

#[derive(Serialize)]
pub struct UnsetOutput {
    pub key: String,
    pub level: String,
}

impl Renderable for UnsetOutput {
    /// Silent on success — same contract as `set`. JSON form
    /// emits the structured payload.
    fn render_human(&self, _w: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
}
