//! Bridge to the agent-skills ecosystem via the `npx skills` CLI.
//!
//! [`find`] searches and [`add`] installs skills; the employee roster (see
//! [`crate::team`]) then attaches installed skills to employees by name.

use std::process::Command;

use color_eyre::eyre::{Result, WrapErr, bail};
use serde::Deserialize;

/// Run `npx -y skills <args>`, inheriting the terminal so the interactive
/// finder and install prompts work.
fn run_skills(args: &[&str]) -> Result<()> {
    let status = Command::new("npx")
        .arg("-y")
        .arg("skills")
        .args(args)
        .status()
        .wrap_err_with(|| format!("failed to run `npx -y skills {}`", args.join(" ")))?;
    if !status.success() {
        bail!("`npx -y skills {}` exited with {status}", args.join(" "));
    }
    Ok(())
}

/// Search the ecosystem for skills matching `query` (interactive).
pub fn find(query: &[String]) -> Result<()> {
    let mut args: Vec<&str> = vec!["find"];
    args.extend(query.iter().map(String::as_str));
    run_skills(&args)
}

/// Install a skill package (optionally globally). `-y` skips prompts.
pub fn add(package: &str, global: bool) -> Result<()> {
    let mut args: Vec<&str> = vec!["add", package];
    if global {
        args.push("-g");
    }
    args.push("-y");
    run_skills(&args)
}

/// One element of the machine-readable `skills add --json --yes` result array.
#[derive(Debug, Deserialize)]
struct AddResult {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Install a skill source non-interactively and return the names of the skills
/// that were actually installed.
///
/// Unlike [`add`], this captures the subprocess output (so it is safe to call
/// from inside the TUI) and parses the `--json` result to learn the installed
/// skill names — the value that [`crate::team::Team::add_skill`] expects.
pub fn add_named(source: &str) -> Result<Vec<String>> {
    let output = Command::new("npx")
        .arg("-y")
        .arg("skills")
        .arg("add")
        .arg(source)
        .arg("--json")
        .arg("--yes")
        .output()
        .wrap_err_with(|| format!("failed to run `npx -y skills add {source}`"))?;

    if !output.status.success() {
        bail!(
            "`npx -y skills add {source}` exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<AddResult> = serde_json::from_str(stdout.trim())
        .wrap_err_with(|| format!("parsing `skills add` output: {stdout}"))?;

    let mut installed = Vec::new();
    for result in &results {
        match result.status.as_deref() {
            Some("installed") => {
                if let Some(name) = result.name.as_deref() {
                    installed.push(name.to_string());
                }
            }
            Some("failed") => {
                bail!(
                    "skill '{}' failed to install: {}",
                    result.name.as_deref().unwrap_or(source),
                    result.error.as_deref().unwrap_or("unknown error")
                );
            }
            _ => {}
        }
    }

    if installed.is_empty() {
        bail!("no skills were installed from '{source}'");
    }

    Ok(installed)
}

/// List installed skills.
pub fn list() -> Result<()> {
    run_skills(&["list"])
}

/// Names of skills installed for the current project, sorted and deduplicated.
///
/// Reads the project's `.agents/skills/` directory (local + repo-sourced skills)
/// and the `skills-lock.json` manifest, falling back to an empty list when the
/// project has no skills directory yet.
pub fn installed() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    };

    if let Ok(entries) = std::fs::read_dir(".agents/skills") {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                push(name);
            }
        }
    }

    if let Ok(text) = std::fs::read_to_string("skills-lock.json")
        && let Ok(lock) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(skills) = lock.get("skills").and_then(|s| s.as_object())
    {
        for name in skills.keys() {
            push(name);
        }
    }

    names.sort();
    names
}
