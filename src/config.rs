use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub tick_rate: Duration,
    /// Working directory OpenCode sessions are rooted at. Defaults to the
    /// current directory when unset.
    pub workdir: Option<PathBuf>,
    /// Optional Jev (System One) classifier settings.
    pub jev: JevConfig,
    /// Cron schedules for autonomous dispatch.
    pub schedules: Vec<Schedule>,
    /// File-change watch hooks for autonomous dispatch.
    pub watches: Vec<Watch>,
}

/// A cron schedule: fire `goal` at `employee` on a cron cadence.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Schedule {
    /// Cron expression. A 5-field crontab (`minute hour day-of-month month
    /// day-of-week`, e.g. `0 9 * * 1-5`) is accepted; seconds default to `0`.
    /// The scheduler itself parses seconds-first, so 6-field (`second minute
    /// hour day-of-month month day-of-week`) and 7-field (plus a trailing
    /// `year`) expressions are accepted too and passed through unchanged.
    /// Anything else is skipped with a warning at startup.
    pub cron: String,
    /// The employee id to dispatch to.
    pub employee: String,
    /// The natural-language goal to hand that employee.
    pub goal: String,
}

/// A watch hook: fire `goal` at `employee` when `path` changes. The `goal` may
/// contain `{path}`, which is replaced with the path that changed. Changes are
/// debounced (a burst of editor events collapses into one dispatch).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watch {
    /// The file or directory to watch. `~` is expanded to the home directory.
    pub path: PathBuf,
    /// The employee id to dispatch to.
    pub employee: String,
    /// The natural-language goal to hand that employee (`{path}` is templated).
    pub goal: String,
    /// Whether to recurse into subdirectories when `path` is a directory.
    pub recursive: bool,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            employee: String::new(),
            goal: String::new(),
            recursive: true,
        }
    }
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
            schedules: Vec::new(),
            watches: Vec::new(),
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

    #[test]
    fn deserializes_schedules() {
        let text = r#"
            tick_rate = "100ms"
            [[schedules]]
            cron = "0 9 * * 1-5"
            employee = "planner"
            goal = "Summarize yesterday's commits"
        "#;
        let cfg: Config = toml::from_str(text).expect("parse config");
        assert_eq!(cfg.schedules.len(), 1);
        assert_eq!(cfg.schedules[0].cron, "0 9 * * 1-5");
        assert_eq!(cfg.schedules[0].employee, "planner");
        assert_eq!(cfg.schedules[0].goal, "Summarize yesterday's commits");
    }

    #[test]
    fn deserializes_watches() {
        let text = r#"
            tick_rate = "100ms"
            [[watches]]
            path = "~/Projects/mywaker/src"
            employee = "reviewer"
            goal = "Review the latest change to {path}"
            recursive = false
        "#;
        let cfg: Config = toml::from_str(text).expect("parse config");
        assert_eq!(cfg.watches.len(), 1);
        assert_eq!(cfg.watches[0].path, PathBuf::from("~/Projects/mywaker/src"));
        assert_eq!(cfg.watches[0].employee, "reviewer");
        assert_eq!(cfg.watches[0].goal, "Review the latest change to {path}");
        assert!(!cfg.watches[0].recursive);
    }

    #[test]
    fn watch_recurses_by_default() {
        let text = r#"
            [[watches]]
            path = "src"
            employee = "reviewer"
            goal = "review {path}"
        "#;
        let cfg: Config = toml::from_str(text).expect("parse config");
        assert!(cfg.watches[0].recursive);
    }

    #[test]
    fn rejects_unknown_watch_field() {
        let text = r#"
            [[watches]]
            path = "src"
            employee = "reviewer"
            goal = "x"
            recurse = true
        "#;
        assert!(toml::from_str::<Config>(text).is_err());
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let text = r#"
            tick_rate = "100ms"
            tickrate = "100ms"
        "#;
        assert!(toml::from_str::<Config>(text).is_err());
    }

    #[test]
    fn rejects_unknown_schedule_field() {
        let text = r#"
            [[schedules]]
            cron = "0 9 * * 1-5"
            crone = "typo"
            employee = "planner"
            goal = "x"
        "#;
        assert!(toml::from_str::<Config>(text).is_err());
    }

    #[test]
    fn rejects_malformed_tick_rate() {
        // `tick_rate` must be a duration string, not a bare number.
        let text = "tick_rate = 100\n";
        assert!(toml::from_str::<Config>(text).is_err());
    }
}
