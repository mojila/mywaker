//! Discover the local OpenCode background service and read its credentials.

use color_eyre::eyre::{Result, WrapErr};
use reqwest::Url;
use serde::Deserialize;

/// Return the running service's HTTP endpoint, if any, by shelling out to
/// `opencode service status` (OpenCode's own discovery path).
pub async fn discover() -> Result<Option<Url>> {
    let output = tokio::process::Command::new("opencode")
        .args(["service", "status"])
        .output()
        .await
        .wrap_err("failed to run `opencode service status`")?;

    if !output.status.success() {
        return Ok(None);
    }

    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes).trim().to_owned();
        if let Ok(url) = Url::parse(&text) {
            return Ok(Some(url));
        }
    }

    Ok(None)
}

/// The local service password. Combined with the fixed `opencode` username it
/// forms the `Authorization: Basic` credential.
pub fn password() -> Result<String> {
    let path = service_config_path()?;
    let raw = std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("failed to read {}", path.display()))?;

    #[derive(Deserialize)]
    struct ServiceConfig {
        password: String,
    }

    let config: ServiceConfig = serde_json::from_str(&raw)
        .wrap_err_with(|| format!("failed to parse {}", path.display()))?;

    Ok(config.password)
}

/// Resolve OpenCode's service config path (`~/.config/opencode/service.json`,
/// or `$XDG_CONFIG_HOME/opencode/service.json`). OpenCode follows the XDG
/// layout even on macOS, where `dirs::config_dir()` points at
/// `~/Library/Application Support`, so we resolve it explicitly.
fn service_config_path() -> Result<std::path::PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => {
            let home = dirs::home_dir()
                .ok_or_else(|| color_eyre::eyre::eyre!("no home directory found"))?;
            home.join(".config")
        }
    };
    Ok(base.join("opencode").join("service.json"))
}
