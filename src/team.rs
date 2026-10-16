//! The team of AI employees and their work-lifecycle status.

use crate::opencode::AgentInfo;

/// Lifecycle status of an employee; drives the robot avatar and roster UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmployeeStatus {
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
#[derive(Debug, Clone)]
pub struct Employee {
    /// Stable id (doubles as the OpenCode agent id, e.g. "researcher").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Human role.
    pub role: String,
    /// Model the backing agent runs on (`None` until agents are loaded).
    pub model: Option<String>,
    /// Onboarding date (ISO), for the roster's "fun" field.
    pub onboarded: String,
    /// The task currently assigned to this employee, if any.
    pub current_task: Option<String>,
    /// Current work status.
    pub status: EmployeeStatus,
}

impl Employee {
    /// A short human-readable status label.
    pub fn status_label(&self) -> &'static str {
        self.status.label()
    }
}

/// The full roster of employees.
#[derive(Debug, Default)]
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
                .map(|(id, name, role)| Employee {
                    id: id.to_string(),
                    name: name.to_string(),
                    role: role.to_string(),
                    model: None,
                    onboarded: "2026-10-16".to_string(),
                    current_task: None,
                    status: EmployeeStatus::Idle,
                })
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
}
