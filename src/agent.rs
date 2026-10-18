//! OpenCode agent definitions — the role-specific system prompt behind each
//! employee.
//!
//! Every employee binds 1:1 to an OpenCode agent whose id is
//! [`crate::team::Employee::agent_id`]. OpenCode reads those agents from markdown
//! files (YAML frontmatter + prompt) under a config `opencode/agents/` directory.
//! If the file is missing, OpenCode answers `POST /api/session` with a
//! `400 Agent not found`: the app's runtime fallback keeps working, but the
//! employee loses its own role prompt (and the live smoke test in
//! `tests/live_employees.rs` fails).
//!
//! So hiring an employee must also write their definition. This module owns that
//! write: [`ensure`] (write only when absent, so a hand-tuned definition is never
//! clobbered) and [`regenerate`] (the explicit overwrite behind
//! `mywaker employee agent <id> --force`).
//!
//! Definitions go to OpenCode's **global** scope — `$XDG_CONFIG_HOME/opencode/agents`
//! or `~/.config/opencode/agents` — which matches mywaker's global roster
//! (`team.toml`): one employee is hired once and works in every project.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};

use crate::role::Role;
use crate::team::Employee;

/// OpenCode's global agents directory. Honors `XDG_CONFIG_HOME` when it is set
/// to an absolute path, else falls back to `~/.config` — the same resolution
/// OpenCode itself uses. `None` only when neither is available.
pub fn agents_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;
    Some(base.join("opencode").join("agents"))
}

/// The definition file OpenCode looks up for `employee` (`<agent_id>.md`),
/// resolved against the global agents dir. `None` when no config dir exists.
pub fn path_for(employee: &Employee) -> Option<PathBuf> {
    agents_dir().map(|dir| dir.join(format!("{}.md", employee.agent_id())))
}

/// The markdown definition for `employee`: YAML frontmatter plus a role prompt.
///
/// Canonical workflow roles reuse the role prompts documented under
/// `examples/agents/` (planner, researcher, writer, reviewer — the engineer
/// prompt has no checked-in file), so a hired teammate starts from the same
/// prompt the examples show; any other role gets a generic identity prompt built
/// from the employee's name and human role.
pub fn definition(employee: &Employee) -> String {
    let description = {
        let role = employee.role.trim();
        if role.is_empty() {
            employee.id.as_str()
        } else {
            role
        }
    };

    format!(
        "---\ndescription: {}\nmode: all\n---\n\n{}\n",
        yaml_scalar(description),
        prompt(employee)
    )
}

/// Ensure `employee`'s definition exists, writing it only when absent. Returns
/// the path and whether a new file was written — a hand-tuned, pre-existing
/// definition is left untouched.
pub fn ensure(employee: &Employee) -> Result<(PathBuf, bool)> {
    let dir = agents_dir().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "no config dir (set HOME or XDG_CONFIG_HOME) to write the agent definition"
        )
    })?;
    ensure_in(&dir, employee)
}

/// Write `employee`'s definition, overwriting any existing file. Returns the path.
pub fn regenerate(employee: &Employee) -> Result<PathBuf> {
    let dir = agents_dir().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "no config dir (set HOME or XDG_CONFIG_HOME) to write the agent definition"
        )
    })?;
    write_in(&dir, employee)
}

/// [`ensure`] against an explicit directory (the seam unit tests drive).
fn ensure_in(dir: &Path, employee: &Employee) -> Result<(PathBuf, bool)> {
    let path = dir.join(format!("{}.md", employee.agent_id()));
    if path.exists() {
        return Ok((path, false));
    }
    std::fs::create_dir_all(dir).wrap_err_with(|| format!("creating {}", dir.display()))?;
    std::fs::write(&path, definition(employee))
        .wrap_err_with(|| format!("writing {}", path.display()))?;
    Ok((path, true))
}

/// [`regenerate`] against an explicit directory (the seam unit tests drive).
fn write_in(dir: &Path, employee: &Employee) -> Result<PathBuf> {
    let path = dir.join(format!("{}.md", employee.agent_id()));
    std::fs::create_dir_all(dir).wrap_err_with(|| format!("creating {}", dir.display()))?;
    std::fs::write(&path, definition(employee))
        .wrap_err_with(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// A `description:` value safe for YAML — always double-quoted, with backslashes
/// and quotes escaped, so roles containing `:`, `#`, or `"` stay valid.
fn yaml_scalar(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The system-prompt body for `employee`.
fn prompt(employee: &Employee) -> String {
    match employee.workflow_role() {
        Role::Planner => PLANNER.into(),
        Role::Researcher => RESEARCHER.into(),
        Role::Engineer => ENGINEER.into(),
        Role::Writer => WRITER.into(),
        Role::Reviewer => REVIEWER.into(),
        Role::General => generic_prompt(employee),
    }
}

/// The fallback prompt for an employee whose id maps to no canonical role. It
/// carries the same hand-off protocol the shared team-context preamble teaches
/// (see [`crate::workflow::team_context`]), so a custom role still delegates and
/// delivers correctly.
fn generic_prompt(employee: &Employee) -> String {
    let role = {
        let role = employee.role.trim();
        if role.is_empty() { "teammate" } else { role }
    };
    format!(
        "You are **{}** (`{}`), the team's {}.\n\n\
         Work within your own role and leave work that belongs to a teammate to them: \
         hand it off with a line-leading `@<id> <instruction>` directive. Deliver your \
         finished result back to the user with `@user <summary>`.",
        employee.name, employee.id, role
    )
}

const PLANNER: &str =
    "You are the **planner**. Produce clear plans, roadmaps, and task breakdowns ONLY.

Never write implementation, code, or final prose yourself — delegate those to the
teammate whose role covers them (the software engineer / writer) using line-leading
`@<id> <instruction>` directives. When the plan is complete, hand off with `@writer`
(or the relevant teammate) instructions, and deliver a final answer to the user with
`@user <summary>`.";

const RESEARCHER: &str =
    "You are the **researcher**. Investigate, gather, and summarize facts, data, and
references.

Drafting and implementation belong to the other teammates — hand those off with
line-leading `@<id> <instruction>` directives. Return findings to the user with
`@user <summary>`.";

const ENGINEER: &str =
    "You are the **engineer**. Implement the requested change in code, tests, and
configuration, and explain what you changed.

Research and planning belong to the other teammates — hand those off with
line-leading `@<id> <instruction>` directives. When the change is done and verified,
deliver it to the user with `@user <summary>`.";

const WRITER: &str =
    "You are the **writer**. Produce clear, accurate prose, documentation, and copy from
the source material and direction you are given.

Defer to teammates for research (`@researcher`) and planning (`@planner`) when you
lack the material you need, using line-leading `@<id> <instruction>` directives.
Deliver the finished piece to the user with `@user <text>`.";

const REVIEWER: &str =
    "You are the **reviewer**. Inspect deliverables for correctness, quality, and
consistency, and report problems precisely.

When something is wrong, hand it back with line-leading `@<id> <instruction>`
directives rather than silently rewriting it. When it passes, approve and return it
to the user with `@user <note>`.";

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mywaker-agent-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn definition_has_frontmatter_and_role_prompt() {
        let employee = Employee::new("planner", "Dex", "Planner");
        let text = definition(&employee);
        assert!(text.starts_with("---\n"), "frontmatter must open first");
        assert!(text.contains("description: \"Planner\"\n"));
        assert!(text.contains("mode: all\n"));
        assert!(text.contains("You are the **planner**"));
    }

    #[test]
    fn canonical_ids_reuse_the_documented_role_prompts() {
        for (id, needle) in [
            ("researcher", "You are the **researcher**"),
            ("planner", "You are the **planner**"),
            ("writer", "You are the **writer**"),
            ("reviewer", "You are the **reviewer**"),
            ("engineer", "You are the **engineer**"),
        ] {
            let employee = Employee::new(id, "N", "Role");
            assert!(
                definition(&employee).contains(needle),
                "{id} should get its canonical prompt"
            );
        }
    }

    #[test]
    fn unknown_role_gets_a_generic_prompt_with_name_and_id() {
        let employee = Employee::new("tester", "Tess", "Test lead");
        let text = definition(&employee);
        assert!(text.contains("**Tess** (`tester`)"));
        assert!(text.contains("Test lead"));
        assert!(text.contains("@user"));
    }

    #[test]
    fn description_quotes_yaml_sensitive_roles() {
        let employee = Employee::new("qa", "Quinn", "QA: lead \"#1\"");
        let text = definition(&employee);
        assert!(
            text.contains(r##"description: "QA: lead \"#1\"""##),
            "colons/quotes must be escaped: {text}"
        );
    }

    #[test]
    fn empty_role_falls_back_to_the_id_for_the_description() {
        let mut employee = Employee::new("tester", "Tess", "");
        assert!(definition(&employee).contains("description: \"tester\""));
        // A blank role also degrades to "teammate" in the prompt.
        employee.role = "   ".into();
        assert!(definition(&employee).contains("the team's teammate"));
    }

    #[test]
    fn ensure_writes_once_and_never_clobbers() {
        let dir = scratch("ensure");

        let employee = Employee::new("tester", "Tess", "Test lead");
        let (path, written) = ensure_in(&dir, &employee).unwrap();
        assert!(written, "first ensure should write");
        assert!(path.ends_with("tester.md"));
        assert!(path.exists());

        // A hand-tuned definition must survive a second ensure untouched.
        std::fs::write(&path, "hand tuned").unwrap();
        let (_, written) = ensure_in(&dir, &employee).unwrap();
        assert!(!written, "second ensure must skip an existing file");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hand tuned");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regenerate_overwrites() {
        let dir = scratch("regenerate");
        let employee = Employee::new("tester", "Tess", "Test lead");

        let path = write_in(&dir, &employee).unwrap();
        std::fs::write(&path, "hand tuned").unwrap();
        let path = write_in(&dir, &employee).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("**Tess**"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn definition_path_uses_the_agent_override() {
        let mut employee = Employee::new("tester", "Tess", "Test lead");
        employee.agent = Some("code-gen".into());
        // `path_for` may be `None` without a home dir; when it resolves, the
        // filename must follow the agent override, not the employee id.
        if let Some(path) = path_for(&employee) {
            assert_eq!(path.file_name().unwrap().to_str().unwrap(), "code-gen.md");
        }
    }
}
