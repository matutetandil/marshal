//! [`SystemConfigSource`] — machine-wide Marshal config.
//!
//! Location:
//! * Unix: `/etc/marshal/config.toml`.
//! * Windows: `%ProgramData%\marshal\config.toml`.
//!
//! Override via the `MARSHAL_SYSTEM_CONFIG` environment variable (tests and
//! administrators both rely on this to point at an isolated file).
//!
//! Precedence: `system < global < local`. System is the lowest layer; its
//! values are overridden by any explicit global/local value.
//!
//! Write access normally requires elevated privileges (root on Unix,
//! Administrator on Windows). Permission errors propagate from the
//! filesystem with a clear message; Marshal does not silently swallow them.

use super::{Config, ConfigSource, Level};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub struct SystemConfigSource {
    path: PathBuf,
}

impl SystemConfigSource {
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: default_path()?,
        })
    }

    #[allow(dead_code)] // used from unit tests via direct path injection.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ConfigSource for SystemConfigSource {
    fn level(&self) -> Level {
        Level::System
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Result<Option<Config>> {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => {
                let cfg: Config = toml::from_str(&content)
                    .with_context(|| format!("parsing {}", self.path.display()))?;
                Ok(Some(cfg))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    fn save(&self, config: &Config) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "creating directory {} (system config usually requires elevated privileges)",
                    parent.display()
                )
            })?;
        }

        let content = toml::to_string_pretty(config).context("serializing config to TOML")?;

        let mut tmp = self.path.clone();
        let file_name = self.path.file_name().ok_or_else(|| {
            anyhow!(
                "system config path has no file name: {}",
                self.path.display()
            )
        })?;
        let mut tmp_name = file_name.to_os_string();
        tmp_name.push(".tmp");
        tmp.set_file_name(tmp_name);

        std::fs::write(&tmp, &content).with_context(|| {
            format!(
                "writing temp file {} (system config usually requires elevated privileges)",
                tmp.display()
            )
        })?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("renaming {} → {}", tmp.display(), self.path.display()))?;
        Ok(())
    }
}

/// Resolve the default system config path, honouring `MARSHAL_SYSTEM_CONFIG`.
fn default_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("MARSHAL_SYSTEM_CONFIG").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    #[cfg(windows)]
    {
        let pd = std::env::var_os("ProgramData")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("ProgramData is not set; cannot locate system config"))?;
        Ok(PathBuf::from(pd).join("marshal").join("config.toml"))
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("/etc/marshal/config.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{test_support::ENV_MUTEX, Config, ConfigKey};
    use tempfile::TempDir;

    #[test]
    fn level_is_system() {
        let tmp = TempDir::new().unwrap();
        let s = SystemConfigSource::at(tmp.path().join("config.toml"));
        assert_eq!(s.level(), Level::System);
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let s = SystemConfigSource::at(tmp.path().join("missing.toml"));
        assert!(s.load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let s = SystemConfigSource::at(tmp.path().join("config.toml"));

        let mut cfg = Config::default();
        cfg.set_from_str(ConfigKey::ModernizeRewrite, "true")
            .unwrap();
        s.save(&cfg).unwrap();

        let reloaded = s.load().unwrap().unwrap();
        assert_eq!(reloaded, cfg);
    }

    #[cfg(not(windows))]
    mod default_path {
        use super::*;

        struct EnvGuard {
            saved: Vec<(String, Option<std::ffi::OsString>)>,
        }
        impl EnvGuard {
            fn new(keys: &[&str]) -> Self {
                Self {
                    saved: keys
                        .iter()
                        .map(|k| (k.to_string(), std::env::var_os(k)))
                        .collect(),
                }
            }
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (k, v) in &self.saved {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }

        #[test]
        fn env_override_wins() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = EnvGuard::new(&["MARSHAL_SYSTEM_CONFIG"]);
            std::env::set_var("MARSHAL_SYSTEM_CONFIG", "/tmp/sysoverride.toml");

            let path = default_path().unwrap();
            assert_eq!(path, PathBuf::from("/tmp/sysoverride.toml"));
        }

        #[test]
        fn falls_back_to_etc_marshal_on_unix() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = EnvGuard::new(&["MARSHAL_SYSTEM_CONFIG"]);
            std::env::remove_var("MARSHAL_SYSTEM_CONFIG");

            let path = default_path().unwrap();
            assert_eq!(path, PathBuf::from("/etc/marshal/config.toml"));
        }
    }
}
