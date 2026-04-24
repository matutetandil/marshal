//! Marshal configuration system.
//!
//! The design follows Git's 3-level model (`system < global < local`) so users
//! already understand precedence. Step 5a ships only the `global` layer;
//! `system` arrives in 5b and `local` in 5c. Each layer is one
//! [`ConfigSource`]; the [`ConfigResolver`] merges them by precedence.
//!
//! Config values live in [`Config`] as `Option<T>` fields. `None` in a given
//! layer means "not set here" — the resolver falls back to the next layer,
//! and the compiled-in defaults apply when every layer is silent. The
//! helpers [`Config::modernize_tips`] and [`Config::modernize_rewrite`]
//! return the final `bool` after all fallbacks.
//!
//! SOLID applied:
//! * **SRP** — `Config` is data; `ConfigSource` is I/O; `ConfigResolver`
//!   aggregates; [`ConfigKey`] handles key/name mapping.
//! * **OCP** — new layer (system, local, workspace) = new `ConfigSource`
//!   impl + one registration line.
//! * **DIP** — the resolver and callers depend on `ConfigSource`, not on
//!   concrete sources.

pub mod global;
pub mod local;
pub mod source;
pub mod system;

pub use source::{ConfigSource, Level};

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Mutex;

    /// Tests that mutate process env vars (`HOME`, `XDG_CONFIG_HOME`,
    /// `MARSHAL_CONFIG`, `MARSHAL_SYSTEM_CONFIG`, etc.) must hold this lock
    /// to prevent parallel tests from racing on shared state. Poisoning is
    /// ignored — a panicking test should not take down the rest.
    pub static ENV_MUTEX: Mutex<()> = Mutex::new(());
}

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

// ── Data types ──────────────────────────────────────────────────────────

/// In-memory representation of one config layer. Missing fields in the TOML
/// file decode to `None`; `None` means "unset at this layer".
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    #[serde(skip_serializing_if = "ModernizeConfig::is_empty")]
    pub modernize: ModernizeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModernizeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tips: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<bool>,
}

impl ModernizeConfig {
    fn is_empty(&self) -> bool {
        self.tips.is_none() && self.rewrite.is_none()
    }
}

impl Config {
    /// Effective value of `modernize.tips` after all layers and defaults.
    /// Default: `true` — tips are informational, low-noise, opt-in off.
    pub fn modernize_tips(&self) -> bool {
        self.modernize.tips.unwrap_or(true)
    }

    /// Effective value of `modernize.rewrite` after all layers and defaults.
    /// Default: `false` — Invariant 8 (Conservative Defaults) demands that
    /// Marshal never alters the user-typed invocation without explicit opt-in.
    pub fn modernize_rewrite(&self) -> bool {
        self.modernize.rewrite.unwrap_or(false)
    }

    /// Render the value for one key, falling back through defaults.
    pub fn get_effective_string(&self, key: ConfigKey) -> String {
        match key {
            ConfigKey::ModernizeTips => self.modernize_tips().to_string(),
            ConfigKey::ModernizeRewrite => self.modernize_rewrite().to_string(),
        }
    }

    /// Parse a string and set the corresponding field.
    pub fn set_from_str(&mut self, key: ConfigKey, value: &str) -> Result<()> {
        match key {
            ConfigKey::ModernizeTips => self.modernize.tips = Some(parse_bool(value)?),
            ConfigKey::ModernizeRewrite => self.modernize.rewrite = Some(parse_bool(value)?),
        }
        Ok(())
    }

    /// Clear this key from the layer — the resolver will fall through to
    /// the next layer below (or the compiled-in default) on next load.
    pub fn unset(&mut self, key: ConfigKey) {
        match key {
            ConfigKey::ModernizeTips => self.modernize.tips = None,
            ConfigKey::ModernizeRewrite => self.modernize.rewrite = None,
        }
    }

    /// The value *this specific layer* has set for `key`, as a renderable
    /// string. `None` means the layer is silent about this key — the
    /// resolver must fall through. Used by `origin_of` to identify which
    /// layer owns the effective value.
    pub fn layer_value(&self, key: ConfigKey) -> Option<String> {
        match key {
            ConfigKey::ModernizeTips => self.modernize.tips.map(|b| b.to_string()),
            ConfigKey::ModernizeRewrite => self.modernize.rewrite.map(|b| b.to_string()),
        }
    }
}

// ── Config keys ─────────────────────────────────────────────────────────

/// The set of recognised configuration keys.
///
/// Using an enum (rather than free-form strings) lets us validate the key
/// once at the CLI boundary, keep all call sites type-safe, and produce a
/// definitive list for `marshal config list` without a separate registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    ModernizeTips,
    ModernizeRewrite,
}

impl ConfigKey {
    pub fn from_dotted(s: &str) -> Result<Self> {
        match s {
            "modernize.tips" => Ok(Self::ModernizeTips),
            "modernize.rewrite" => Ok(Self::ModernizeRewrite),
            other => bail!(
                "unknown config key '{other}'. Known keys:\n  modernize.tips\n  modernize.rewrite"
            ),
        }
    }

    pub fn as_dotted(&self) -> &'static str {
        match self {
            Self::ModernizeTips => "modernize.tips",
            Self::ModernizeRewrite => "modernize.rewrite",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::ModernizeTips => "Emit modernization tips on stderr.",
            Self::ModernizeRewrite => {
                "Rewrite legacy commands to their modern forms before running git."
            }
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::ModernizeTips, Self::ModernizeRewrite]
    }
}

// ── Resolver ────────────────────────────────────────────────────────────

/// Aggregates one or more layers by precedence. Sources are registered in
/// low→high precedence order; later-registered sources override earlier.
///
/// 5a ships a resolver with only the global layer. 5b prepends system, 5c
/// appends local.
pub struct ConfigResolver {
    sources: Vec<Box<dyn ConfigSource>>,
}

impl ConfigResolver {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub fn register(&mut self, source: Box<dyn ConfigSource>) {
        self.sources.push(source);
    }

    /// Default resolver for end-user operation.
    ///
    /// Layers are registered in low→high precedence order (system < global
    /// < local), so local overrides global which overrides system during
    /// the merge.
    ///
    /// System and local layers are best-effort: when the platform can't
    /// tell us where system config lives (e.g. missing `ProgramData` on
    /// Windows) or when we are not inside a Git repository, the respective
    /// layer is skipped with a `tracing::warn!` rather than aborting. The
    /// user may be trying to run `git status` — which is valid outside a
    /// repo, Git just errors — so Marshal must not refuse to launch Git
    /// because a config layer couldn't be materialised.
    pub fn current_user() -> Result<Self> {
        let mut r = Self::new();
        match system::SystemConfigSource::new() {
            Ok(s) => r.register(Box::new(s)),
            Err(err) => tracing::warn!(%err, "skipping system config layer"),
        }
        r.register(Box::new(global::GlobalConfigSource::new()?));
        match local::LocalConfigSource::new() {
            Ok(s) => r.register(Box::new(s)),
            Err(err) => tracing::debug!(%err, "skipping local config layer"),
        }
        Ok(r)
    }

    /// Compose the effective config by merging all layers in precedence
    /// order. Missing layer files are skipped silently; malformed layers
    /// propagate the parse error.
    pub fn effective(&self) -> Result<Config> {
        let mut eff = Config::default();
        for source in &self.sources {
            if let Some(layer) = source.load()? {
                merge(&mut eff, layer);
            }
        }
        Ok(eff)
    }

    /// Read one specific layer verbatim (for per-layer introspection).
    #[allow(dead_code)] // Reserved for future `config get --<level>` support.
    pub fn layer(&self, level: Level) -> Result<Option<Config>> {
        let source = self.source_for(level)?;
        source.load()
    }

    /// Find the highest-precedence layer that has a value for `key`, if any.
    /// Returns `(level, value)` for use with `config get --show-origin`.
    /// `Ok(None)` means no layer has the key set; the caller should report
    /// the compiled-in default.
    pub fn origin_of(&self, key: ConfigKey) -> Result<Option<(Level, String)>> {
        // High precedence first. `sources` is ordered low→high, so iterate
        // in reverse and return the first hit.
        for source in self.sources.iter().rev() {
            if let Some(layer) = source.load()? {
                if let Some(value) = layer.layer_value(key) {
                    return Ok(Some((source.level(), value)));
                }
            }
        }
        Ok(None)
    }

    /// Load → mutate → save one specific layer atomically (via the source's
    /// own atomic-write guarantees). If the layer's file does not yet exist,
    /// the mutation starts from [`Config::default`].
    pub fn mutate(&self, level: Level, f: impl FnOnce(&mut Config) -> Result<()>) -> Result<()> {
        let source = self.source_for(level)?;
        let mut layer = source.load()?.unwrap_or_default();
        f(&mut layer)?;
        source.save(&layer)?;
        Ok(())
    }

    fn source_for(&self, level: Level) -> Result<&dyn ConfigSource> {
        self.sources
            .iter()
            .find(|s| s.level() == level)
            .map(|b| b.as_ref())
            .ok_or_else(|| match level {
                Level::Local => anyhow!(
                    "local config is only available inside a git repository. \
                     Run this command from within a repo, or use --global/--system."
                ),
                Level::System | Level::Global => anyhow!(
                    "no config source registered for level '{}'. This is a bug.",
                    level.as_str()
                ),
            })
    }
}

/// Overlay non-`None` fields from `overlay` onto `base`. The resolver calls
/// this once per layer, in low→high precedence order.
fn merge(base: &mut Config, overlay: Config) {
    if overlay.modernize.tips.is_some() {
        base.modernize.tips = overlay.modernize.tips;
    }
    if overlay.modernize.rewrite.is_some() {
        base.modernize.rewrite = overlay.modernize.rewrite;
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn parse_bool(s: &str) -> Result<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(anyhow!(
            "'{other}' is not a boolean (expected true/false/1/0/yes/no/on/off)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_documented_policy() {
        let cfg = Config::default();
        assert!(cfg.modernize_tips(), "tips default to enabled");
        assert!(
            !cfg.modernize_rewrite(),
            "rewrite defaults to disabled (conservative)"
        );
    }

    #[test]
    fn set_and_get_round_trip() {
        let mut cfg = Config::default();
        cfg.set_from_str(ConfigKey::ModernizeTips, "false").unwrap();
        cfg.set_from_str(ConfigKey::ModernizeRewrite, "true")
            .unwrap();
        assert_eq!(cfg.get_effective_string(ConfigKey::ModernizeTips), "false");
        assert_eq!(
            cfg.get_effective_string(ConfigKey::ModernizeRewrite),
            "true"
        );
    }

    #[test]
    fn set_accepts_alternative_boolean_spellings() {
        let mut cfg = Config::default();
        for (spelling, expected) in [
            ("yes", true),
            ("no", false),
            ("1", true),
            ("0", false),
            ("on", true),
            ("off", false),
            ("TRUE", true),
            ("False", false),
        ] {
            cfg.set_from_str(ConfigKey::ModernizeTips, spelling)
                .unwrap();
            assert_eq!(cfg.modernize_tips(), expected, "'{spelling}' → {expected}");
        }
    }

    #[test]
    fn set_rejects_non_boolean() {
        let mut cfg = Config::default();
        let err = cfg
            .set_from_str(ConfigKey::ModernizeTips, "maybe")
            .unwrap_err();
        assert!(err.to_string().contains("not a boolean"));
    }

    #[test]
    fn unset_returns_field_to_default() {
        let mut cfg = Config::default();
        cfg.set_from_str(ConfigKey::ModernizeTips, "false").unwrap();
        assert!(!cfg.modernize_tips());
        cfg.unset(ConfigKey::ModernizeTips);
        assert!(cfg.modernize_tips(), "fall back to default after unset");
    }

    #[test]
    fn merge_overlay_overrides_base_when_set() {
        let mut base = Config::default();
        base.set_from_str(ConfigKey::ModernizeTips, "true").unwrap();

        let mut overlay = Config::default();
        overlay
            .set_from_str(ConfigKey::ModernizeTips, "false")
            .unwrap();

        merge(&mut base, overlay);
        assert!(!base.modernize_tips(), "overlay wins when explicitly set");
    }

    #[test]
    fn merge_overlay_none_preserves_base_value() {
        let mut base = Config::default();
        base.set_from_str(ConfigKey::ModernizeTips, "false")
            .unwrap();

        let overlay = Config::default(); // all None

        merge(&mut base, overlay);
        assert!(
            !base.modernize_tips(),
            "overlay's None must not clobber base's explicit value"
        );
    }

    #[test]
    fn key_parsing_is_exact_match() {
        assert!(matches!(
            ConfigKey::from_dotted("modernize.tips"),
            Ok(ConfigKey::ModernizeTips)
        ));
        assert!(matches!(
            ConfigKey::from_dotted("modernize.rewrite"),
            Ok(ConfigKey::ModernizeRewrite)
        ));
        assert!(ConfigKey::from_dotted("modernize.TIPS").is_err());
        assert!(ConfigKey::from_dotted("garbage").is_err());
    }

    #[test]
    fn serialized_empty_config_has_no_keys() {
        // An empty/defaulted Config should serialize to an empty TOML —
        // no `[modernize]` section, no keys. Prevents the disk file from
        // leaking "tips = null" or similar noise.
        let cfg = Config::default();
        let serialized = toml::to_string_pretty(&cfg).unwrap();
        assert_eq!(
            serialized.trim(),
            "",
            "default Config serializes to empty TOML, got: {serialized:?}"
        );
    }

    #[test]
    fn serialized_partial_config_only_contains_set_keys() {
        let mut cfg = Config::default();
        cfg.set_from_str(ConfigKey::ModernizeTips, "false").unwrap();
        let serialized = toml::to_string_pretty(&cfg).unwrap();
        assert!(serialized.contains("[modernize]"));
        assert!(serialized.contains("tips = false"));
        assert!(
            !serialized.contains("rewrite"),
            "unset field must not appear in the TOML"
        );
    }
}
