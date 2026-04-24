//! [`ConfigSource`] — one on-disk location Marshal reads from and writes to.
//!
//! Each concrete source implements the same load/save contract. The
//! [`ConfigResolver`](super::ConfigResolver) aggregates sources by
//! precedence. Adding a new level (step 5b adds system, step 5c adds local)
//! is a new `impl ConfigSource` + one registration line — no other code
//! changes (Open/Closed Principle).

use super::Config;
use anyhow::Result;
use std::path::Path;

/// Which layer a source represents. Precedence is low→high:
/// `System < Global < Local`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Machine-wide layer.
    System,
    /// Per-user layer.
    Global,
    /// Per-repository layer (under `<git-dir>/marshal/config.toml`).
    Local,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Global => "global",
            Self::Local => "local",
        }
    }
}

/// Strategy: one Marshal config file location.
pub trait ConfigSource: Send + Sync {
    fn level(&self) -> Level;

    /// The path this source reads from and writes to. Consumed by
    /// `--show-origin` reporting in step 5c.
    #[allow(dead_code)]
    fn path(&self) -> &Path;

    /// Load the layer. `Ok(None)` when the file does not exist (which is
    /// normal and not an error). `Ok(Some(_))` when the file exists and
    /// parses. `Err(_)` when the file exists but is unreadable or malformed.
    fn load(&self) -> Result<Option<Config>>;

    /// Persist the layer atomically: write-to-temp + rename. The source
    /// creates any missing parent directories for the path.
    fn save(&self, config: &Config) -> Result<()>;
}
