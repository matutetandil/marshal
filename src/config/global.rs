//! [`GlobalConfigSource`] — per-user Marshal config at
//! `$XDG_CONFIG_HOME/marshal/config.toml` (Unix) or
//! `%APPDATA%\marshal\config.toml` (Windows).
//!
//! The path can be overridden by the `MARSHAL_CONFIG` environment variable,
//! which tests use to point at a temp file and which power users can use to
//! isolate Marshal from their real config.

use super::{Config, ConfigSource, Level};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub struct GlobalConfigSource {
    path: PathBuf,
}

impl GlobalConfigSource {
    /// Construct using the platform default location (respecting the
    /// `MARSHAL_CONFIG` environment variable when set).
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: default_path()?,
        })
    }

    /// Construct pointing at an explicit path. Intended for tests.
    #[allow(dead_code)] // used from integration tests via env override instead; kept for future.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ConfigSource for GlobalConfigSource {
    fn level(&self) -> Level {
        Level::Global
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
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }

        let content = toml::to_string_pretty(config).context("serializing config to TOML")?;

        // Atomic write: stage into a sibling temp file, then rename. Rename
        // is atomic on POSIX within the same filesystem; on Windows it is
        // atomic from the caller's perspective.
        let mut tmp = self.path.clone();
        let file_name = self
            .path
            .file_name()
            .ok_or_else(|| anyhow!("config path has no file name: {}", self.path.display()))?;
        let mut tmp_name = file_name.to_os_string();
        tmp_name.push(".tmp");
        tmp.set_file_name(tmp_name);

        std::fs::write(&tmp, &content)
            .with_context(|| format!("writing temp file {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("renaming {} → {}", tmp.display(), self.path.display()))?;
        Ok(())
    }
}

/// Resolve the default global config path.
///
/// Env var override (`MARSHAL_CONFIG`) takes precedence over platform
/// defaults. Useful for tests and power users who want an isolated config.
fn default_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("MARSHAL_CONFIG").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("APPDATA is not set; cannot locate global config"))?;
        Ok(PathBuf::from(appdata).join("marshal").join("config.toml"))
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
            return Ok(PathBuf::from(xdg).join("marshal").join("config.toml"));
        }
        let home = std::env::var_os("HOME")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("HOME is not set; cannot locate global config"))?;
        Ok(PathBuf::from(home)
            .join(".config")
            .join("marshal")
            .join("config.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigKey};
    use tempfile::TempDir;

    #[test]
    fn load_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let source = GlobalConfigSource::at(tmp.path().join("nonexistent.toml"));
        assert!(source.load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let source = GlobalConfigSource::at(tmp.path().join("config.toml"));

        let mut cfg = Config::default();
        cfg.set_from_str(ConfigKey::ModernizeTips, "false").unwrap();
        source.save(&cfg).unwrap();

        let reloaded = source.load().unwrap().unwrap();
        assert_eq!(reloaded, cfg);
    }

    #[test]
    fn save_creates_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b").join("c").join("config.toml");
        let source = GlobalConfigSource::at(nested.clone());

        let cfg = Config::default();
        source.save(&cfg).unwrap();
        assert!(nested.is_file(), "config written at nested path");
    }

    #[test]
    fn save_is_atomic_in_spirit_no_leftover_tmp_on_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let source = GlobalConfigSource::at(path.clone());
        source.save(&Config::default()).unwrap();

        // The sibling temp file must not linger after a successful save.
        let tmp_path = tmp.path().join("config.toml.tmp");
        assert!(!tmp_path.exists(), "temp file was cleaned up via rename");
    }

    #[test]
    fn malformed_file_produces_parse_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "this is not valid [[ toml").unwrap();
        let source = GlobalConfigSource::at(path);
        let err = source.load().unwrap_err();
        // The error chain includes "parsing" (our context) somewhere.
        let chain = format!("{err:#}");
        assert!(
            chain.contains("parsing"),
            "error should mention parsing: {chain}"
        );
    }

    // Path resolution tests — only run on non-Windows because the env vars
    // differ. The Windows path is exercised by CI on that runner.
    #[cfg(not(windows))]
    mod default_path {
        use super::*;
        use crate::config::test_support::ENV_MUTEX;

        /// Save + restore env vars across tests to avoid cross-test pollution.
        struct EnvGuard {
            saved: Vec<(String, Option<std::ffi::OsString>)>,
        }

        impl EnvGuard {
            fn new(keys: &[&str]) -> Self {
                let saved = keys
                    .iter()
                    .map(|k| (k.to_string(), std::env::var_os(k)))
                    .collect();
                Self { saved }
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
        fn env_override_wins_over_xdg_and_home() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = EnvGuard::new(&["MARSHAL_CONFIG", "XDG_CONFIG_HOME", "HOME"]);
            std::env::set_var("MARSHAL_CONFIG", "/tmp/override.toml");
            std::env::set_var("XDG_CONFIG_HOME", "/should/not/be/used");
            std::env::set_var("HOME", "/should/not/be/used/either");

            let path = default_path().unwrap();
            assert_eq!(path, PathBuf::from("/tmp/override.toml"));
        }

        #[test]
        fn xdg_config_home_is_used_when_set() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = EnvGuard::new(&["MARSHAL_CONFIG", "XDG_CONFIG_HOME", "HOME"]);
            std::env::remove_var("MARSHAL_CONFIG");
            std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg");
            std::env::set_var("HOME", "/home/user");

            let path = default_path().unwrap();
            assert_eq!(path, PathBuf::from("/tmp/xdg/marshal/config.toml"));
        }

        #[test]
        fn falls_back_to_home_dot_config() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = EnvGuard::new(&["MARSHAL_CONFIG", "XDG_CONFIG_HOME", "HOME"]);
            std::env::remove_var("MARSHAL_CONFIG");
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::set_var("HOME", "/home/user");

            let path = default_path().unwrap();
            assert_eq!(
                path,
                PathBuf::from("/home/user/.config/marshal/config.toml")
            );
        }
    }
}
