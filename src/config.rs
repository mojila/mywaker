use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub tick_rate: Duration,
    /// Working directory OpenCode sessions are rooted at. Defaults to the
    /// current directory when unset.
    pub workdir: Option<PathBuf>,
    /// Optional Jev (System One) classifier settings.
    pub jev: JevConfig,
}

/// Jev (System One) classifier settings. Jev is an *optional* classifier — it is
/// disabled unless an API key is available (here or via `KENARI_API_KEY`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct JevConfig {
    /// System One endpoint override (defaults to Kenari's public endpoint).
    pub endpoint: Option<String>,
    /// Decision model id (defaults to `jev-1-13-free`).
    pub model: Option<String>,
    /// API key; falls back to the `KENARI_API_KEY` env var when unset.
    pub api_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(100),
            workdir: None,
            jev: JevConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut builder = config::Config::builder();

        // Load from default location if exists
        if let Some(config_dir) = dirs::config_dir() {
            let default_path = config_dir.join("mywaker").join("config.toml");
            if default_path.exists() {
                builder = builder.add_source(config::File::from(default_path));
            }
        }

        // Load from explicit path if provided
        if let Some(path) = path {
            builder = builder.add_source(config::File::from(path.to_path_buf()));
        }

        // Environment variables with prefix
        builder = builder.add_source(
            config::Environment::with_prefix("MYWAKER")
                .separator("_")
                .try_parsing(true),
        );

        let config = builder
            .build()
            .wrap_err("failed to build configuration")?
            .try_deserialize()
            .wrap_err("failed to deserialize configuration")?;

        Ok(config)
    }
}

mod humantime_serde {
    use serde::{Deserialize, Deserializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jev_defaults_to_none() {
        let cfg = Config::default();
        assert!(cfg.jev.endpoint.is_none());
        assert!(cfg.jev.model.is_none());
        assert!(cfg.jev.api_key.is_none());
    }

    #[test]
    fn deserializes_jev_overrides() {
        let text = r#"
            tick_rate = "100ms"
            [jev]
            endpoint = "https://example.test/v1/systemone"
            model = "custom-decision-model"
            api_key = "sk-test"
        "#;
        let cfg: Config = toml::from_str(text).expect("parse config");
        assert_eq!(
            cfg.jev.endpoint.as_deref(),
            Some("https://example.test/v1/systemone")
        );
        assert_eq!(cfg.jev.model.as_deref(), Some("custom-decision-model"));
        assert_eq!(cfg.jev.api_key.as_deref(), Some("sk-test"));
    }
}
