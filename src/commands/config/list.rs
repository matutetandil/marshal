//! `marshal config list` — dump every known key with its effective value.

use anyhow::Result;
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};

use crate::cli::{Command, Renderable};
use crate::config::{ConfigKey, ConfigResolver};

pub struct ConfigList;

impl Command for ConfigList {
    type Output = ListOutput;

    fn run(&self, _args: &[OsString]) -> Result<Self::Output> {
        let resolver = ConfigResolver::current_user()?;
        let effective = resolver.effective()?;
        let entries = ConfigKey::all()
            .iter()
            .map(|key| ListEntry {
                key: key.as_dotted().to_string(),
                value: effective.get_effective_string(*key),
            })
            .collect();
        Ok(ListOutput { entries })
    }
}

#[derive(Serialize)]
pub struct ListOutput {
    pub entries: Vec<ListEntry>,
}

#[derive(Serialize)]
pub struct ListEntry {
    pub key: String,
    pub value: String,
}

impl Renderable for ListOutput {
    /// `key=value` lines, one per known key — same shape as before
    /// the migration so existing scripts (e.g. `grep …` filters)
    /// keep working.
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        for entry in &self.entries {
            writeln!(w, "{}={}", entry.key, entry.value)?;
        }
        Ok(())
    }
}
