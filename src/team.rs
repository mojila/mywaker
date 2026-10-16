//! The team of AI employees, their lifecycle status, and roster persistence.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::opencode::AgentInfo;

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
    /// Skills from the agent-skills ecosystem attached to this employee.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Onboarding date (ISO), for the roster's "fun" field.
    pub onboarded: String,
    /// The task currently assigned to this employee, if any (runtime-only).
    #[serde(skip)]
    pub current_task: Option<String>,
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
            skills: Vec::new(),
            onboarded: today_iso(),
            current_task: None,
            status: EmployeeStatus::Idle,
        }
    }

    /// A short human-readable status label.
    pub fn status_label(&self) -> &'static str {
        self.status.label()
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

    pub fn len(&self) -> usize {
        self.employees.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.employees.is_empty()
    }

    /// True if any employee is actively working (drives the avatar).
    pub fn is_busy(&self) -> bool {
        self.employees.iter().any(|employee| {
            matches!(
                employee.status,
                EmployeeStatus::Thinking | EmployeeStatus::Working
            )
        })
    }

    /// Set one employee's status by index (no-op if out of range).
    pub fn set_status(&mut self, index: usize, status: EmployeeStatus) {
        if let Some(employee) = self.employees.get_mut(index) {
            employee.status = status;
        }
    }

    /// Set one employee's model by index (no-op if out of range).
    pub fn set_model(&mut self, index: usize, model: String) {
        if let Some(employee) = self.employees.get_mut(index) {
            employee.model = Some(model);
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
    fn round_trips_through_toml() {
        let mut team = Team::starter();
        team.add_skill("writer", "copywriting").unwrap();
        team.employee_mut("writer").unwrap().model = Some("openai/gpt-6-luna-pro".into());

        let text = toml::to_string_pretty(&team).unwrap();
        // Runtime fields are not persisted.
        assert!(!text.contains("status"));
        assert!(!text.contains("current_task"));

        let loaded: Team = toml::from_str(&text).unwrap();
        assert_eq!(loaded.len(), 4);
        let writer = loaded.employee("writer").unwrap();
        assert_eq!(writer.skills, vec!["copywriting".to_string()]);
        assert_eq!(writer.model.as_deref(), Some("openai/gpt-6-luna-pro"));
        assert_eq!(writer.status, EmployeeStatus::Idle);
    }
}
