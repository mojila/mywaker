//! Bridge to the agent-skills ecosystem via the `npx skills` CLI.
//!
//! [`find`] searches and [`add`] installs skills; the employee roster (see
//! [`crate::team`]) then attaches installed skills to employees by name.

use std::process::Command;

use color_eyre::eyre::{Result, WrapErr, bail};

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

/// List installed skills.
pub fn list() -> Result<()> {
    run_skills(&["list"])
}
