//! The team of AI employees, their lifecycle status, and roster persistence.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::opencode::{AgentInfo, DEFAULT_PROVIDER, ModelRef};
use crate::role::Role;

/// Lifecycle status of an employee; drives the robot avatar and roster UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmployeeStatus {
    #[default]
    Idle,
    Thinking,
    Working,
    Done,
    Error,
}

impl EmployeeStatus {
    /// A short human-readable status label.
    pub fn label(self) -> &'static str {
        match self {
            EmployeeStatus::Idle => "idle",
            EmployeeStatus::Thinking => "thinking",
            EmployeeStatus::Working => "working",
            EmployeeStatus::Done => "done",
            EmployeeStatus::Error => "error",
        }
    }

    /// True while the employee is mid-turn — the predicate behind
    /// [`Team::is_busy`] and [`Employee::is_busy`]. The terminal states (`Done`,
    /// `Error`) are deliberately *not* active: the turn is over and the employee
    /// can take more work.
    pub fn is_active(self) -> bool {
        matches!(self, EmployeeStatus::Thinking | EmployeeStatus::Working)
    }
}

/// A named AI employee — maps 1:1 to an OpenCode agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    /// Stable id (doubles as the OpenCode agent id, e.g. "researcher").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Human role.
    pub role: String,
    /// Model the backing agent runs on (`None` until agents are loaded).
    #[serde(default)]
    pub model: Option<String>,
    /// Provider hosting [`Self::model`], needed to build OpenCode's `Model.Ref`
    /// (`{ id, providerID }`). Persisted with the model assignment; `None` for
    /// legacy entries (falls back to the id's vendor segment at ref time).
    #[serde(default)]
    pub model_provider: Option<String>,
    /// Skills from the agent-skills ecosystem attached to this employee.
    #[serde(default)]
    pub skills: Vec<String>,
    /// OpenCode agent id backing this employee. Defaults to the employee's own
    /// `id` (which doubles as the agent id) when unset.
    #[serde(default)]
    pub agent: Option<String>,
    /// Onboarding date (ISO), for the roster's "fun" field.
    pub onboarded: String,
    /// The most recent task assigned to this employee (runtime-only).
    ///
    /// Set on prompt send (`App::send_prompt`) and peer delegation
    /// (`App::flush_outbox`), cleared only by `App::reset_chat` (`/new` · `/clear`).
    /// The live board keeps the *last* task — **not** cleared on `Done`/`Error` —
    /// tinted by its final status, until the employee is reset or handed the next
    /// task.
    #[serde(skip)]
    pub last_task: Option<String>,
    /// Current work status (runtime-only).
    #[serde(skip)]
    pub status: EmployeeStatus,
}

impl Employee {
    /// A named employee with sensible defaults (empty skills, idle, today's date).
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            role: role.into(),
            model: None,
            model_provider: None,
            skills: Vec::new(),
            agent: None,
            onboarded: today_iso(),
            last_task: None,
            status: EmployeeStatus::Idle,
        }
    }

    /// True while this employee is mid-turn (`Thinking` / `Working`).
    pub fn is_busy(&self) -> bool {
        self.status.is_active()
    }

    /// The employee's workflow role, resolved from the stable id (which doubles
    /// as the OpenCode agent id). This is the one place the id → [`Role`] mapping
    /// happens: the grid's card order and stage labels both read it, so the
    /// id-as-role convention lives with the data rather than at the call sites.
    pub fn workflow_role(&self) -> Role {
        Role::from_label(&self.id)
    }

    /// The card's stage label for this employee (`"ENGINEER"`, …).
    pub fn stage_label(&self) -> String {
        self.workflow_role().label().to_ascii_uppercase()
    }

    /// The OpenCode agent backing this employee: the explicit `agent` override,
    /// else the employee's own id (which doubles as the agent id).
    pub fn agent_id(&self) -> &str {
        self.agent.as_deref().unwrap_or(&self.id)
    }

    /// The model reference for OpenCode `POST /api/session`: the assigned model
    /// id paired with its provider. `None` while no model is assigned. The
    /// provider defaults to [`DEFAULT_PROVIDER`] (kenari) when a legacy
    /// assignment predates `model_provider`.
    pub fn model_ref(&self) -> Option<ModelRef> {
        let id = self.model.clone()?;
        let provider = self
            .model_provider
            .clone()
            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
        Some(ModelRef {
            id,
            provider_id: Some(provider),
        })
    }
}

/// The full roster of employees.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Team {
    employees: Vec<Employee>,
}

impl Team {
    /// The built-in starter roster until project-defined agents are loaded.
    pub fn starter() -> Self {
        let roster = [
            ("researcher", "Aria", "Research lead"),
            ("writer", "Bruno", "Writer"),
            ("reviewer", "Cleo", "Reviewer"),
            ("planner", "Dex", "Planner"),
        ];
        Self {
            employees: roster
                .into_iter()
                .map(|(id, name, role)| Employee::new(id, name, role))
                .collect(),
        }
    }

    #[allow(dead_code)]
    pub fn from_agents(_agents: &[AgentInfo]) -> Self {
        // Placeholder: build a roster from project agent definitions.
        Self::starter()
    }

    pub fn employees(&self) -> &[Employee] {
        &self.employees
    }

    pub fn employees_mut(&mut self) -> &mut [Employee] {
        &mut self.employees
    }

    pub fn len(&self) -> usize {
        self.employees.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.employees.is_empty()
    }

    /// True if any employee is actively working (drives the avatar). Active work
    /// is the single predicate [`EmployeeStatus::is_active`], so the roster, the
    /// avatar and the prompt queue never disagree about "busy".
    pub fn is_busy(&self) -> bool {
        self.employees.iter().any(Employee::is_busy)
    }

    /// True when the employee at `index` is mid-turn (`false` if out of range).
    pub fn is_busy_at(&self, index: usize) -> bool {
        self.employees.get(index).is_some_and(Employee::is_busy)
    }

    /// The display name of the employee at `index`, or `None` when the roster has
    /// no such entry (a stale id from persisted state, say).
    pub fn name_at(&self, index: usize) -> Option<String> {
        self.employees.get(index).map(|e| e.name.clone())
    }

    /// Set one employee's status by index (no-op if out of range).
    pub fn set_status(&mut self, index: usize, status: EmployeeStatus) {
        if let Some(employee) = self.employees.get_mut(index) {
            employee.status = status;
        }
    }

    /// Set one employee's status by id (no-op if the id is unknown).
    pub fn set_status_by_id(&mut self, id: &str, status: EmployeeStatus) {
        if let Some(employee) = self.employee_mut(id) {
            employee.status = status;
        }
    }

    /// Set one employee's model and its provider by index (no-op if out of range).
    pub fn set_model(&mut self, index: usize, model: String, provider: Option<String>) {
        if let Some(employee) = self.employees.get_mut(index) {
            employee.model = Some(model);
            employee.model_provider = provider;
        }
    }

    /// Set one employee's last task by index (no-op if out of range).
    pub fn set_last_task(&mut self, index: usize, task: Option<String>) {
        if let Some(employee) = self.employees.get_mut(index) {
            employee.last_task = task;
        }
    }

    /// Set one employee's last task by id (no-op if the id is unknown).
    pub fn set_last_task_by_id(&mut self, id: &str, task: Option<String>) {
        if let Some(employee) = self.employee_mut(id) {
            employee.last_task = task;
        }
    }

    // --- roster management (CRUD) ---

    /// Add an employee, rejecting duplicate ids.
    pub fn add_employee(&mut self, employee: Employee) -> Result<(), String> {
        if self.employees.iter().any(|e| e.id == employee.id) {
            return Err(format!("employee '{}' already exists", employee.id));
        }
        self.employees.push(employee);
        Ok(())
    }

    /// Remove an employee by id; true if anything was removed.
    pub fn remove_employee(&mut self, id: &str) -> bool {
        let before = self.employees.len();
        self.employees.retain(|e| e.id != id);
        self.employees.len() != before
    }

    pub fn employee(&self, id: &str) -> Option<&Employee> {
        self.employees.iter().find(|e| e.id == id)
    }

    pub fn employee_mut(&mut self, id: &str) -> Option<&mut Employee> {
        self.employees.iter_mut().find(|e| e.id == id)
    }

    /// Attach a skill (deduplicated) to an employee.
    pub fn add_skill(&mut self, id: &str, skill: &str) -> Result<(), String> {
        let employee = self
            .employee_mut(id)
            .ok_or_else(|| format!("no employee '{id}'"))?;
        if !employee.skills.iter().any(|s| s == skill) {
            employee.skills.push(skill.to_string());
        }
        Ok(())
    }

    /// Detach a skill from an employee.
    pub fn remove_skill(&mut self, id: &str, skill: &str) -> Result<(), String> {
        let employee = self
            .employee_mut(id)
            .ok_or_else(|| format!("no employee '{id}'"))?;
        employee.skills.retain(|s| s != skill);
        Ok(())
    }

    // --- persistence ---

    /// Default roster path: `<config_dir>/mywaker/team.toml`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("mywaker").join("team.toml"))
    }

    /// Load the team from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).wrap_err_with(|| format!("parsing {}", path.display()))
    }

    /// Persist the team to a TOML file (creating parent directories).
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).wrap_err("serializing team")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(path, text).wrap_err_with(|| format!("writing {}", path.display()))
    }

    /// Load the persisted team, falling back to the starter roster.
    pub fn load_or_starter() -> Self {
        Self::default_path()
            .and_then(|path| Self::load(&path).ok())
            .unwrap_or_else(Self::starter)
    }
}

/// Clamp a roster cursor to `[0, len)` after moving by `delta`.
pub fn move_selection(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = len - 1;
    if delta < 0 {
        current.saturating_sub((-delta) as usize).min(max)
    } else {
        (current + delta as usize).min(max)
    }
}

/// Today's date as `YYYY-MM-DD` (UTC), via the civil-date algorithm.
fn today_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → (year, month, day) in the proleptic Gregorian calendar
/// (Howard Hinnant's `civil_from_days`). Assumes `z >= 0`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    debug_assert!(z >= 0);
    let z = z + 719_468;
    let era = z / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_labels() {
        assert_eq!(EmployeeStatus::Idle.label(), "idle");
        assert_eq!(EmployeeStatus::Thinking.label(), "thinking");
        assert_eq!(EmployeeStatus::Working.label(), "working");
        assert_eq!(EmployeeStatus::Done.label(), "done");
        assert_eq!(EmployeeStatus::Error.label(), "error");
    }

    #[test]
    fn only_thinking_and_working_are_active() {
        assert!(EmployeeStatus::Thinking.is_active());
        assert!(EmployeeStatus::Working.is_active());
        // Terminal states end the turn — they are not "busy".
        assert!(!EmployeeStatus::Idle.is_active());
        assert!(!EmployeeStatus::Done.is_active());
        assert!(!EmployeeStatus::Error.is_active());
    }

    #[test]
    fn busy_follows_active_statuses() {
        let mut team = Team::starter();
        assert!(!team.is_busy());

        team.set_status(0, EmployeeStatus::Thinking);
        assert!(team.is_busy());
        assert!(team.is_busy_at(0));
        assert!(!team.is_busy_at(1));
        assert!(!team.is_busy_at(99)); // out of range, not a panic

        team.set_status(0, EmployeeStatus::Done); // terminal → idle again
        assert!(!team.is_busy());
    }

    #[test]
    fn set_status_by_id_targets_the_named_employee() {
        let mut team = Team::starter();
        team.set_status_by_id("writer", EmployeeStatus::Working);
        assert_eq!(
            team.employee("writer").unwrap().status,
            EmployeeStatus::Working
        );
        assert_eq!(
            team.employee("planner").unwrap().status,
            EmployeeStatus::Idle
        );

        team.set_status_by_id("nobody", EmployeeStatus::Error); // no-op
        assert_eq!(
            team.employee("planner").unwrap().status,
            EmployeeStatus::Idle
        );
    }

    #[test]
    fn workflow_role_and_stage_label_derive_from_the_id() {
        let team = Team::starter();
        let planner = team.employee("planner").unwrap();
        assert_eq!(planner.workflow_role(), crate::role::Role::Planner);
        assert_eq!(planner.stage_label(), "PLANNER");
        assert_eq!(
            team.employee("researcher").unwrap().workflow_role(),
            crate::role::Role::Researcher
        );

        // An id with no known role is `General`, not an error.
        let stranger = Employee::new("qa", "Quinn", "Tester");
        assert_eq!(stranger.workflow_role(), crate::role::Role::General);
        assert_eq!(stranger.stage_label(), "GENERAL");
    }

    #[test]
    fn selection_clamps() {
        assert_eq!(move_selection(0, -1, 4), 0);
        assert_eq!(move_selection(0, 1, 4), 1);
        assert_eq!(move_selection(3, 1, 4), 3);
        assert_eq!(move_selection(2, 5, 4), 3);
        assert_eq!(move_selection(0, 0, 0), 0);
        assert_eq!(move_selection(4, -10, 4), 0);
    }

    #[test]
    fn civil_from_days_epoch_and_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-10-16 = 20_742 days since epoch.
        assert_eq!(civil_from_days(20_742), (2026, 10, 16));
    }

    #[test]
    fn add_remove_employee_rejects_duplicates() {
        let mut team = Team::starter();
        assert_eq!(team.len(), 4);

        team.add_employee(Employee::new("qa", "Quinn", "Tester"))
            .unwrap();
        assert_eq!(team.len(), 5);
        assert!(team.employee("qa").is_some());

        // Duplicate id is rejected.
        assert!(
            team.add_employee(Employee::new("qa", "Other", "Tester"))
                .is_err()
        );

        assert!(team.remove_employee("qa"));
        assert!(!team.remove_employee("qa"));
        assert_eq!(team.len(), 4);
    }

    #[test]
    fn add_remove_skill_deduplicates() {
        let mut team = Team::starter();
        team.add_skill("researcher", "web-search").unwrap();
        team.add_skill("researcher", "web-search").unwrap(); // dedup
        team.add_skill("researcher", "data-analysis").unwrap();
        assert_eq!(team.employee("researcher").unwrap().skills.len(), 2);

        team.remove_skill("researcher", "web-search").unwrap();
        assert_eq!(
            team.employee("researcher").unwrap().skills,
            vec!["data-analysis".to_string()]
        );

        // Unknown employee is an error.
        assert!(team.add_skill("nobody", "x").is_err());
    }

    #[test]
    fn set_last_task_updates_and_clears() {
        let mut team = Team::starter();
        team.set_last_task(1, Some("draft the intro".to_string()));
        assert_eq!(
            team.employee("writer").and_then(|e| e.last_task.as_deref()),
            Some("draft the intro")
        );

        team.set_last_task(1, None);
        assert!(
            team.employee("writer")
                .and_then(|e| e.last_task.as_deref())
                .is_none()
        );

        // Out-of-range index is a no-op, not a panic.
        team.set_last_task(99, Some("x".to_string()));
    }

    #[test]
    fn round_trips_through_toml() {
        let mut team = Team::starter();
        team.add_skill("writer", "copywriting").unwrap();
        team.employee_mut("writer").unwrap().model = Some("openai/gpt-6-luna-pro".into());
        team.employee_mut("writer").unwrap().model_provider = Some("openrouter".into());
        team.employee_mut("writer").unwrap().agent = Some("code-gen".into());

        let text = toml::to_string_pretty(&team).unwrap();
        // Runtime fields are not persisted.
        assert!(!text.contains("status"));
        assert!(!text.contains("last_task"));

        let loaded: Team = toml::from_str(&text).unwrap();
        assert_eq!(loaded.len(), 4);
        let writer = loaded.employee("writer").unwrap();
        assert_eq!(writer.skills, vec!["copywriting".to_string()]);
        assert_eq!(writer.model.as_deref(), Some("openai/gpt-6-luna-pro"));
        assert_eq!(writer.model_provider.as_deref(), Some("openrouter"));
        assert_eq!(writer.agent.as_deref(), Some("code-gen"));
        assert_eq!(writer.status, EmployeeStatus::Idle);

        // An employee without an explicit agent defaults to its own id.
        assert_eq!(loaded.employee("planner").unwrap().agent_id(), "planner");
    }

    #[test]
    fn model_ref_pairs_id_with_provider_and_defaults_to_kenari() {
        let mut team = Team::starter();
        let writer = team.employee_mut("writer").unwrap();
        writer.model = Some("deepseek-v4-pro".into());
        writer.model_provider = Some("kenari".into());
        assert_eq!(
            writer.model_ref(),
            Some(crate::opencode::ModelRef {
                id: "deepseek-v4-pro".into(),
                provider_id: Some("kenari".into()),
            })
        );

        // Legacy assignment without an explicit provider defaults to kenari.
        writer.model = Some("openai/gpt-6-luna-pro".into());
        writer.model_provider = None;
        assert_eq!(
            writer.model_ref(),
            Some(crate::opencode::ModelRef {
                id: "openai/gpt-6-luna-pro".into(),
                provider_id: Some("kenari".into()),
            })
        );

        writer.model = None;
        assert_eq!(writer.model_ref(), None);
    }

    #[test]
    fn name_at_indexes_the_roster() {
        let team = Team::starter();
        assert_eq!(team.name_at(0).as_deref(), Some("Aria"));
        assert_eq!(team.name_at(1).as_deref(), Some("Bruno"));
        assert_eq!(team.name_at(99), None); // out of range, not a panic
    }
}
