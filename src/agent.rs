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
/// `examples/agents/` (orchestrator, planner, researcher, writer, reviewer — the
/// engineer prompt has no checked-in file), so a hired teammate starts from the
/// same prompt the examples show. The asset-creator id (`modeler`) has a mandate
/// of its own but no [`Role`]; its checked-in `examples/agents/modeler.md` mirrors
/// the `MODELER` text exactly, and a test keeps the two in step. Any other role
/// gets a generic identity prompt built from the employee's name and human role.
pub fn definition(employee: &Employee) -> String {
    format!(
        "---\ndescription: {}\nmode: all\n---\n\n{}\n",
        yaml_scalar(&description(employee)),
        prompt(employee)
    )
}

/// The frontmatter `description` for `employee`.
///
/// It has to describe the agent that will actually run, so it follows the same
/// effective id the prompt does ([`Employee::agent_id`]) rather than blindly
/// copying [`Employee::role`]. Otherwise an employee with id `qa` and
/// `agent = "reviewer"` would write `description: "QA lead"` above the reviewer
/// prompt — a frontmatter/body contradiction.
///
/// Resolution order: a canonical role named by the effective id, then the
/// asset-creator mandate's own description, then the employee's human role, and
/// finally the id (for an employee whose role was left blank).
fn description(employee: &Employee) -> String {
    let agent_id = employee.agent_id();

    let role = Role::from_label(agent_id);
    if role != Role::General {
        return capitalize(role.label());
    }
    if agent_id == MODELER_ID {
        return MODELER_DESCRIPTION.to_string();
    }

    let role = employee.role.trim();
    if role.is_empty() {
        agent_id.to_string()
    } else {
        role.to_string()
    }
}

/// Uppercase the first character, leaving the rest alone (`reviewer` → `Reviewer`),
/// so a role-derived description reads as prose in the frontmatter.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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
///
/// Both branches key on the same thing — [`Employee::agent_id`], the id of the
/// OpenCode agent that will actually run the prompt. The asset-creator mandate is
/// selected by id rather than a [`Role`]: roles order the plan → research →
/// implement → write → review pipeline, and an asset creator sits outside that
/// chain (its id resolves to [`Role::General`], like any other non-workflow id).
fn prompt(employee: &Employee) -> String {
    match employee.agent_id() {
        MODELER_ID => MODELER.into(),
        _ => role_prompt(employee),
    }
}

/// The prompt for a canonical workflow id, or the generic identity prompt when the
/// id maps to no canonical role.
///
/// The role is read off [`Employee::agent_id`] — the same key [`prompt`] uses — so
/// an `agent` override selects the role prompt for the agent it names, not the one
/// implied by the employee's own id.
///
/// **This deliberately differs from the pipeline.** [`Employee::workflow_role`]
/// (and everything built on it — `stage_label`, `App::workflow_order`, the
/// fan-out aggregator choice) resolves the role from the employee's *id*, because
/// those are display/ordering concerns that must stay stable for a given employee.
/// Prompt selection is about the agent that will actually run, so it follows the
/// `agent` override. The two can therefore disagree: an employee with id `qa` and
/// `agent = "reviewer"` gets the reviewer *prompt* while still rendering in the
/// `GENERAL` pipeline stage. That is intended — the pipeline orders the team, the
/// prompt tells the agent how to behave.
fn role_prompt(employee: &Employee) -> String {
    match Role::from_label(employee.agent_id()) {
        Role::Orchestrator => ORCHESTRATOR.into(),
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
         hand it off with a line-leading `@<id> <instruction>` directive. When your part \
         is finished, report it upward with `@orchestrator <result>` — the orchestrator \
         synthesizes and `@user` is its alone.",
        employee.name, employee.id, role
    )
}

const ORCHESTRATOR: &str =
    "You are the **orchestrator**, the root of the team. A goal reaches the team through
you, and the finished answer leaves through you.

Break the goal into the parts that need doing, hand each part to the teammate whose role
covers it, then synthesize what comes back into one answer. Every teammate reports to you;
`@user` is yours alone.

- Delegate with a line-leading `@<id> <instruction>` directive — one per part, each saying
  what to produce and what it is for. Do the work yourself only when no teammate's role
  covers it.
- Wait for the reports. When a teammate answers you, that is a report, not a deliverable:
  decide whether it is complete, ask for the missing piece with another directive, or fold
  it into the synthesis.
- Synthesize. Resolve overlaps, fill the gaps, and keep the result in one voice. Then
  deliver it to the user with a single line: `@user <final deliverable>`.
- For work that genuinely splits across the whole team, use `@parallel <goal>`; you still
  own the synthesis that follows.

Never hand the user a partial result, and never leave a goal with no owner.";

const PLANNER: &str =
    "You are the **planner**. Produce clear plans, roadmaps, and task breakdowns ONLY.

Never write implementation, code, or final prose yourself — delegate those to the
teammate whose role covers them (the software engineer / writer) using line-leading
`@<id> <instruction>` directives. When the plan is complete, hand off with `@writer`
(or the relevant teammate) instructions, and report the plan upward to the orchestrator
with `@orchestrator <summary>`. The orchestrator synthesizes and delivers; `@user` is not
yours to use.";

const RESEARCHER: &str =
    "You are the **researcher**. Investigate, gather, and summarize facts, data, and
references.

Drafting and implementation belong to the other teammates — hand those off with
line-leading `@<id> <instruction>` directives. Return your findings upward to the
orchestrator with `@orchestrator <summary>`; it owns the synthesis and `@user` is its
alone.";

const ENGINEER: &str =
    "You are the **engineer**. Implement the requested change in code, tests, and
configuration, and explain what you changed.

Research and planning belong to the other teammates — hand those off with
line-leading `@<id> <instruction>` directives. When the change is done and verified,
report it upward to the orchestrator with `@orchestrator <summary>`. The orchestrator
synthesizes and delivers; `@user` is not yours to use.";

const WRITER: &str =
    "You are the **writer**. Produce clear, accurate prose, documentation, and copy from
the source material and direction you are given.

Defer to teammates for research (`@researcher`) and planning (`@planner`) when you
lack the material you need, using line-leading `@<id> <instruction>` directives.
Report the finished piece upward to the orchestrator with `@orchestrator <text>`. The
orchestrator synthesizes and delivers; `@user` is not yours to use.";

const REVIEWER: &str =
    "You are the **reviewer**. Inspect deliverables for correctness, quality, and
consistency, and report problems precisely.

When something is wrong, hand it back with line-leading `@<id> <instruction>`
directives rather than silently rewriting it. When it passes, approve and report that
upward to the orchestrator with `@orchestrator <note>`. The orchestrator synthesizes and
delivers; `@user` is not yours to use.";

/// The canonical id of the asset-creator mandate — the one string both [`prompt`]
/// and [`description`] key on.
const MODELER_ID: &str = "modeler";

/// The frontmatter description that pairs with [`MODELER`], and the one written
/// into `examples/agents/modeler.md`.
const MODELER_DESCRIPTION: &str = "3D asset creator";

/// The 3D asset-creator mandate, mirrored in `examples/agents/modeler.md`.
///
/// **Keyed on the effective agent id**, not the employee id: [`prompt`] matches
/// [`Employee::agent_id`], so `agent = "modeler"` selects this mandate for any
/// employee, while an employee whose own id is `modeler` but whose `agent`
/// override names something else does **not** get it — the override wins, because
/// it is the agent that actually runs.
///
/// Two Blender bridges are named here on purpose, because conflating them sends the
/// employee at the wrong port: `blender-toolkit` speaks WebSocket on `localhost:9400+`,
/// while `product-polish`, `threejs-export`, and the `polyhaven-*` scripts speak the
/// socket-addon protocol on `localhost:9876`.
const MODELER: &str = "You are the **modeler**, the team's 3D asset creator.

Work within your own role and leave work that belongs to a teammate to them: hand it off
with a line-leading `@<id> <instruction>` directive. Deliver your finished result back to
the user with `@user <summary>`.

## 3D mandate

You own the 3D remit end to end: every 3D asset the team needs — a model, a textured
scene, a rendered turntable — is yours to build, and nothing outside it is.

**The pipeline.** Work in stages, and say which stage you are in:

1. **Source** — `image-to-3d` (one photo) or `multi-image-to-3d` (1–4 angles of the same
   object) to bring a reference into 3D. Both read `MESHY_API_KEY` from the environment
   and download the GLB into the working directory.
2. **Build** — `blender-toolkit` for Blender-side geometry, materials, modifiers, and rig
   work. It is a TypeScript CLI driving Blender over its own WebSocket server
   (`localhost:9400+`) — not an MCP bridge.
3. **Surface** — `polyhaven-texture-apply` or `polyhaven-material-swap` for real PBR
   materials; `product-polish` for the glossy product-shot finish.
4. **Scene & light** — `polyhaven-studio-setup` for a studio, `polyhaven-scene-builder`
   for a pedestal/props composition, `polyhaven-hdri-showcase` to compare lighting.
5. **Deliver** — `threejs-export` for a web viewer (GLB + HTML), or a render for stills.
   For motion, the camera skills are the deliverable: `turntable` for a full 360° orbit,
   `perfect-loop` when it must loop seamlessly, and `crane-shot` / `slow-zoom` /
   `dolly-rotate` / `dynamic-full-loop` for a reveal.

Stages 3–5 drive Blender through the socket addon on `localhost:9876` (its `--port`
flag) — a different mechanism from stage 2's WebSocket server, and not interchangeable.

**Blender discipline.** Look at the result and judge your own work. Look shader nodes up
by type, never by name. Read enum identifiers instead of hardcoding them. Imported and
generated models arrive at arbitrary scale: size them from the reported world bounding
box and seat them on the ground before assessing a composition.

**Boundaries.** You do not write application code, documentation, or plans; hand those to
`@writer`, `@planner`, or `@researcher` with a line-leading directive. Ask `@reviewer` to
check a deliverable and `@tester` to verify a pipeline. Deliver the finished asset and
where it lives with `@user <summary>`.";

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
            ("orchestrator", "You are the **orchestrator**"),
            ("researcher", "You are the **researcher**"),
            ("planner", "You are the **planner**"),
            ("writer", "You are the **writer**"),
            ("reviewer", "You are the **reviewer**"),
            ("engineer", "You are the **engineer**"),
            ("modeler", "You are the **modeler**"),
        ] {
            let employee = Employee::new(id, "N", "Role");
            assert!(
                definition(&employee).contains(needle),
                "{id} should get its canonical prompt"
            );
        }
    }

    /// Every checked-in example matches the const it documents: a generated
    /// definition is byte-identical to its file under `examples/agents/`.
    ///
    /// This is what keeps the examples from drifting. The orchestrator is the one
    /// employee every roster has, and the workers' prompts are the ones a role
    /// change is most likely to leave behind — the reporting rule in particular, so
    /// a stale example would tell a worker to `@user` when the code no longer
    /// relays it that way.
    #[test]
    fn every_definition_matches_its_checked_in_example() {
        for (id, name, role, example) in [
            ("orchestrator", "Otto", "Orchestrator", ORCHESTRATOR_EXAMPLE),
            ("planner", "Dex", "Planner", PLANNER_EXAMPLE),
            ("researcher", "Aria", "Research lead", RESEARCHER_EXAMPLE),
            ("writer", "Bruno", "Writer", WRITER_EXAMPLE),
            ("reviewer", "Cleo", "Reviewer", REVIEWER_EXAMPLE),
        ] {
            assert_eq!(
                definition(&Employee::new(id, name, role)),
                example,
                "regenerate examples/agents/{id}.md from its const"
            );
        }
    }

    /// The checked-in example bodies, one per canonical role.
    const ORCHESTRATOR_EXAMPLE: &str = include_str!("../examples/agents/orchestrator.md");
    const PLANNER_EXAMPLE: &str = include_str!("../examples/agents/planner.md");
    const RESEARCHER_EXAMPLE: &str = include_str!("../examples/agents/researcher.md");
    const WRITER_EXAMPLE: &str = include_str!("../examples/agents/writer.md");
    const REVIEWER_EXAMPLE: &str = include_str!("../examples/agents/reviewer.md");

    /// No worker prompt tells the employee to deliver to the user: that is the
    /// orchestrator's alone, and a worker that thinks otherwise answers `@user`
    /// into a relay it did not expect.
    #[test]
    fn no_worker_prompt_claims_the_user() {
        for id in ["planner", "researcher", "writer", "reviewer"] {
            let text = definition(&Employee::new(id, "N", "Role"));
            assert!(
                !text.contains("@user <"),
                "{id} still delivers to the user: {text}"
            );
            assert!(
                text.contains("@orchestrator"),
                "{id} does not know where to report: {text}"
            );
        }
    }

    /// The root's contract, stated where the model reads it: it delegates, it
    /// synthesizes, and it is the only one that delivers.
    #[test]
    fn orchestrator_prompt_owns_delegation_synthesis_and_delivery() {
        let text = definition(&Employee::new("orchestrator", "Otto", "Orchestrator"));
        assert!(text.contains("@<id> <instruction>"), "must delegate");
        assert!(text.contains("Synthesize"), "must synthesize");
        assert!(text.contains("@user"), "must deliver");
        assert!(
            text.contains("`@user` is yours alone"),
            "must state that delivery is the orchestrator's"
        );
    }

    /// The mandate text the UI and the docs both point at: every skill it names must
    /// be a real roster entry, and the two Blender bridges must stay distinct.
    #[test]
    fn modeler_gets_the_3d_mandate() {
        let employee = Employee::new("modeler", "Echo", "3D asset creator");
        let text = definition(&employee);

        assert!(text.contains("## 3D mandate"), "should carry the mandate");
        for skill in [
            "image-to-3d",
            "multi-image-to-3d",
            "blender-toolkit",
            "polyhaven-texture-apply",
            "polyhaven-material-swap",
            "product-polish",
            "polyhaven-studio-setup",
            "polyhaven-scene-builder",
            "polyhaven-hdri-showcase",
            "threejs-export",
            // Stage 5's motion deliverables — the "rendered turntable" promise.
            "turntable",
            "perfect-loop",
            "crane-shot",
        ] {
            assert!(text.contains(skill), "the mandate should name `{skill}`");
        }

        // blender-toolkit drives Blender over its own WebSocket, not MCP.
        assert!(
            text.contains("localhost:9400"),
            "should name the WebSocket port"
        );
        assert!(
            text.contains("localhost:9876"),
            "should name the socket-addon port"
        );
        assert!(
            !text.contains("through MCP") && !text.contains("via MCP"),
            "must not describe blender-toolkit as an MCP bridge: {text}"
        );
    }

    /// The mandate is selected by the canonical agent id, so an employee that names
    /// `modeler` in its `agent` override gets it without renaming itself.
    #[test]
    fn modeler_mandate_follows_the_agent_override() {
        let mut employee = Employee::new("atlas", "Atlas", "3D asset creator");
        assert!(
            definition(&employee).contains("**Atlas** (`atlas`)"),
            "a plain id still gets the generic identity prompt"
        );

        employee.agent = Some("modeler".into());
        assert!(
            definition(&employee).contains("## 3D mandate"),
            "the `agent` override should select the mandate"
        );
    }

    /// The role prompt is keyed on the agent id, exactly like the modeler mandate:
    /// an employee whose `agent` override names a canonical role gets that role's
    /// prompt, not the generic identity prompt its own id would imply.
    #[test]
    fn role_prompt_follows_the_agent_override() {
        let mut employee = Employee::new("qa", "Quinn", "QA lead");
        assert!(
            definition(&employee).contains("**Quinn** (`qa`)"),
            "an id with no canonical role gets the generic identity prompt"
        );

        employee.agent = Some("reviewer".into());
        assert!(
            definition(&employee).contains("You are the **reviewer**"),
            "the `agent` override should select the role prompt it names"
        );

        // …and the two branches agree: a `modeler` override still wins over a
        // canonical role implied by the employee id.
        let mut employee = Employee::new("writer", "Wren", "Writer");
        employee.agent = Some("modeler".into());
        assert!(definition(&employee).contains("## 3D mandate"));
    }

    /// The checked-in copy and the generated definition cannot drift: a generated
    /// `modeler` file is byte-identical to `examples/agents/modeler.md`. Both the
    /// `MODELER` doc comment and the README claim they match, so the claim is a test.
    #[test]
    fn modeler_definition_matches_the_checked_in_example() {
        let employee = Employee::new("modeler", "Echo", "3D asset creator");
        assert_eq!(
            definition(&employee),
            include_str!("../examples/agents/modeler.md"),
            "regenerate examples/agents/modeler.md from the MODELER const"
        );
    }

    /// The frontmatter describes the agent that will actually run, so it follows
    /// the same effective id the prompt does. Without this, `qa` with
    /// `agent = "reviewer"` would write `description: "QA lead"` above the reviewer
    /// prompt — frontmatter contradicting body.
    #[test]
    fn description_follows_the_resolved_agent() {
        // A canonical role named by the override wins over the human role.
        let mut employee = Employee::new("qa", "Quinn", "QA lead");
        employee.agent = Some("reviewer".into());
        let text = definition(&employee);
        assert!(
            text.contains("description: \"Reviewer\""),
            "the description should follow the reviewer override: {text}"
        );
        assert!(
            !text.contains("QA lead"),
            "the stale human role must not win"
        );
        assert!(text.contains("You are the **reviewer**"));

        // The modeler override gets the mandate's own description.
        let mut employee = Employee::new("atlas", "Atlas", "3D asset creator");
        employee.agent = Some(MODELER_ID.into());
        assert!(definition(&employee).contains("description: \"3D asset creator\""));

        // No canonical override: the human role still describes the agent.
        let employee = Employee::new("qa", "Quinn", "QA lead");
        assert!(definition(&employee).contains("description: \"QA lead\""));
    }

    /// Pinning the *loss* case: an employee whose id names a canonical role but
    /// whose `agent` override points elsewhere gets the generic identity prompt, not
    /// that role's prompt. This is a deliberate decision — the override names the
    /// agent that actually runs — not an accident of keying on the wrong field.
    #[test]
    fn a_non_canonical_agent_override_demotes_a_canonical_id() {
        let mut employee = Employee::new("planner", "Dex", "Planner");
        employee.agent = Some("atlas".into());

        let text = definition(&employee);
        assert!(
            text.contains("**Dex** (`planner`)"),
            "an unknown agent id falls back to the generic identity prompt: {text}"
        );
        assert!(
            !text.contains("You are the **planner**"),
            "the planner prompt must not be selected when the override names another agent"
        );
        // The pipeline still files this employee under PLANNER — the divergence is
        // the point, and it is documented on `role_prompt`.
        assert_eq!(employee.workflow_role(), Role::Planner);
        assert_eq!(employee.stage_label(), "PLANNER");
    }

    /// The two keyings differ on purpose: the prompt follows `agent_id`, the
    /// pipeline follows the employee id. Same employee, two answers.
    #[test]
    fn prompt_and_pipeline_resolve_the_role_from_different_fields() {
        let mut employee = Employee::new("qa", "Quinn", "QA lead");
        employee.agent = Some("reviewer".into());

        // Prompt: the reviewer.
        assert!(definition(&employee).contains("You are the **reviewer**"));
        // Pipeline: still GENERAL, because ordering keys on the id.
        assert_eq!(employee.workflow_role(), Role::General);
        assert_eq!(employee.stage_label(), "GENERAL");
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
