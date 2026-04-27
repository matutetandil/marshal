//! `marshal config set` — write a key/value pair to a config layer.

use anyhow::Result;
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};

use crate::cli::{Command, Renderable};
use crate::config::{ConfigKey, ConfigResolver};

use super::helpers::{arg_as_str, extract_level_flag};

pub struct ConfigSet;

impl Command for ConfigSet {
    type Output = SetOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let (level, rest) = extract_level_flag(args)?;
        let usage = "marshal config set [--system|--global|--local] <key> <value>";
        let key_str = arg_as_str(rest, 0, usage)?;
        let value_str = arg_as_str(rest, 1, usage)?;
        let key = ConfigKey::from_dotted(key_str)?;

        let resolver = ConfigResolver::current_user()?;
        resolver.mutate(level, |cfg| cfg.set_from_str(key, value_str))?;

        Ok(SetOutput {
            key: key.as_dotted().to_string(),
            value: value_str.to_string(),
            level: level.as_str().to_string(),
        })
    }
}

#[derive(Serialize)]
pub struct SetOutput {
    pub key: String,
    pub value: String,
    pub level: String,
}

impl Renderable for SetOutput {
    /// Silent on success — preserves the pre-migration human
    /// behaviour where `config set` returned exit 0 with no
    /// stdout. The JSON form still emits the structured payload
    /// so machine consumers can confirm what changed.
    fn render_human(&self, _w: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
}
