//! [`LocalConfigSource`] — per-repository Marshal config.
//!
//! Location: `<git-dir>/marshal/config.toml`, where `<git-dir>` is resolved
//! by walking up from the current directory to the first `.git` directory
//! (handling worktrees whose `.git` is a file with a `gitdir:` pointer).
//!
//! Why inside `.git/`: follows Git's own per-repo config convention. The
//! file is not part of the repository's working tree, so it is not
//! committed and not shared across clones. Each developer's local layer is
//! personal.
//!
//! Override via `MARSHAL_LOCAL_CONFIG` for tests and power users. When the
//! override is set, no repository check is performed — the env var is the
//! authoritative path.
//!
//! Precedence: `system < global < local`. Local wins.

use super::{Config, ConfigSource, Level};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub struct LocalConfigSource {
    path: PathBuf,
}

impl LocalConfigSource {
    /// Construct by resolving the current repository's `.git` directory.
    /// Fails when outside a Git repository (and no `MARSHAL_LOCAL_CONFIG`
    /// override is set).
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: default_path()?,
        })
    }

    #[allow(dead_code)] // used from unit tests via explicit path injection.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ConfigSource for LocalConfigSource {
    fn level(&self) -> Level {
        Level::Local
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

        let mut tmp = self.path.clone();
        let file_name = self.path.file_name().ok_or_else(|| {
            anyhow!(
                "local config path has no file name: {}",
                self.path.display()
            )
        })?;
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

/// Resolve the default local config path. Env override first, then walk up
/// from the current directory looking for `.git`. Fails when no repo is
/// found.
fn default_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("MARSHAL_LOCAL_CONFIG").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let cwd = std::env::current_dir().context("reading current directory")?;
    let git_dir = find_git_dir(&cwd).ok_or_else(|| {
        anyhow!(
            "not inside a git repository; `--local` requires a repository (or set \
             MARSHAL_LOCAL_CONFIG to an explicit path)"
        )
    })?;
    Ok(git_dir.join("marshal").join("config.toml"))
}

/// Walk up from `start` looking for `.git`. Returns the actual `.git`
/// directory — for worktrees, that means following the `gitdir:` pointer in
/// the `.git` file. Returns `None` when no repo is found up to the
/// filesystem root.
fn find_git_dir(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            // Worktree: `.git` is a file whose first line is `gitdir: <path>`.
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                for line in content.lines() {
                    if let Some(stripped) = line.strip_prefix("gitdir: ") {
                        let resolved = if Path::new(stripped.trim()).is_absolute() {
                            PathBuf::from(stripped.trim())
                        } else {
                            dir.join(stripped.trim())
                        };
                        if resolved.exists() {
                            return Some(resolved);
                        }
                    }
                }
            }
        }
        cur = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{test_support::ENV_MUTEX, Config, ConfigKey};
    use tempfile::TempDir;

    #[test]
    fn level_is_local() {
        let tmp = TempDir::new().unwrap();
        let s = LocalConfigSource::at(tmp.path().join("config.toml"));
        assert_eq!(s.level(), Level::Local);
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let s = LocalConfigSource::at(tmp.path().join("missing.toml"));
        assert!(s.load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let s = LocalConfigSource::at(tmp.path().join("config.toml"));

        let mut cfg = Config::default();
        cfg.set_from_str(ConfigKey::ModernizeTips, "false").unwrap();
        s.save(&cfg).unwrap();

        let reloaded = s.load().unwrap().unwrap();
        assert_eq!(reloaded, cfg);
    }

    #[test]
    fn find_git_dir_returns_none_outside_any_repo() {
        let tmp = TempDir::new().unwrap();
        assert!(find_git_dir(tmp.path()).is_none());
    }

    #[test]
    fn find_git_dir_discovers_plain_git_directory() {
        let tmp = TempDir::new().unwrap();
        let git = tmp.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        let deep = tmp.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();

        let found = find_git_dir(&deep).unwrap();
        // Canonicalise both to tolerate symlinks (macOS /tmp → /private/tmp).
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(&git).unwrap()
        );
    }

    #[test]
    fn find_git_dir_follows_worktree_pointer() {
        let tmp = TempDir::new().unwrap();
        // Real gitdir lives here:
        let real_gitdir = tmp.path().join("real-gitdir");
        std::fs::create_dir(&real_gitdir).unwrap();

        // Worktree checkout: `.git` is a file pointing at real_gitdir.
        let worktree = tmp.path().join("wt");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", real_gitdir.display()),
        )
        .unwrap();

        let found = find_git_dir(&worktree).unwrap();
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(&real_gitdir).unwrap()
        );
    }

    #[test]
    fn default_path_env_override_wins() {
        let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved = std::env::var_os("MARSHAL_LOCAL_CONFIG");
        std::env::set_var("MARSHAL_LOCAL_CONFIG", "/tmp/localoverride.toml");
        let path = default_path().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/localoverride.toml"));
        match saved {
            Some(v) => std::env::set_var("MARSHAL_LOCAL_CONFIG", v),
            None => std::env::remove_var("MARSHAL_LOCAL_CONFIG"),
        }
    }
}
