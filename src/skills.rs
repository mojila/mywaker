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
///
/// OpenCode — mywaker's backend — reads skills from both the project
/// (`.agents/skills/`) and the global (`~/.agents/skills/`) scope, so both are
/// shown. `npx skills list` has no combined mode, hence the two invocations.
pub fn list() -> Result<()> {
    run_skills(&["list"])?;
    run_skills(&["list", "--global"])
}

/// Names of skills installed for the current project, sorted and deduplicated.
///
/// Reads the project's `.agents/skills/` directory (local + repo-sourced skills)
/// and the `skills-lock.json` manifest, then merges in globally-installed skills
/// from `~/.agents/skills/` (with its `.skill-lock.json` manifest). Both scopes
/// are equally attachable to an employee, so both are reported.
pub fn installed() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();

    // Project scope, resolved relative to the process working directory.
    collect_skills_from_dir(std::path::Path::new(".agents/skills"), &mut names);
    collect_skills_from_lock(std::path::Path::new("skills-lock.json"), &mut names);

    // Global scope (skills installed with `skills add -g`). These live in the
    // user's home `.agents/skills/` directory and are shared across projects.
    if let Some(home) = dirs::home_dir() {
        collect_skills_from_dir(&home.join(".agents/skills"), &mut names);
        collect_skills_from_lock(&home.join(".agents/.skill-lock.json"), &mut names);
    }

    names.sort();
    names
}

/// Push the name of every skill directory under `dir` (each skill lives in its
/// own subdirectory) into `names`, deduplicated.
fn collect_skills_from_dir(dir: &std::path::Path, names: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir()
            && let Some(name) = entry.file_name().to_str()
            && !names.iter().any(|existing| existing == name)
        {
            names.push(name.to_string());
        }
    }
}

/// Push the skill names declared in a skills-lock manifest (a JSON object with a
/// `skills` map keyed by skill name) into `names`, deduplicated.
fn collect_skills_from_lock(path: &std::path::Path, names: &mut Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(lock) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(skills) = lock.get("skills").and_then(|s| s.as_object()) else {
        return;
    };
    for name in skills.keys() {
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_skills_from_dir_deduplicated_and_sorted_by_caller() {
        let dir = std::env::temp_dir().join("mywaker-skills-dir-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("alpha")).unwrap();
        std::fs::create_dir_all(dir.join("beta")).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored: not a directory").unwrap();

        let mut names = Vec::new();
        collect_skills_from_dir(&dir, &mut names);
        names.sort();

        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collects_skills_from_lock_and_deduplicates() {
        let path = std::env::temp_dir().join("mywaker-skills-lock-test.json");
        std::fs::write(
            &path,
            r#"{"version":1,"skills":{"alpha":{"source":"a"},"gamma":{"source":"g"}}}"#,
        )
        .unwrap();

        let mut names = vec!["alpha".to_string()];
        collect_skills_from_lock(&path, &mut names);

        assert_eq!(names, vec!["alpha".to_string(), "gamma".to_string()]);
        let _ = std::fs::remove_file(&path);
    }
}
