//! The task model: a unit of autonomous work handed to the harness.
//!
//! A `Task` records one schedulable goal — its target employee, its lifecycle
//! state machine, and enough provenance (timestamps, session id, final result)
//! to be persisted and inspected. The `TaskStatus` transitions are validated so
//! a task can't leap from `Pending` straight to `Succeeded`, and retries are
//! bounded by [`MAX_ATTEMPTS`].

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Automatic retry cap for a task that fails.
pub const MAX_ATTEMPTS: u8 = 3;

/// A typed task id (`type-newtype-ids`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(Uuid);

impl TaskId {
    /// A fresh random id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// The underlying [`Uuid`].
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// The lifecycle state of a task (`type-enum-states`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Retrying,
    Succeeded,
    Failed,
}

impl TaskStatus {
    /// A short human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Retrying => "retrying",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
        }
    }
}

/// One unit of autonomous work, persisted across restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    /// The natural-language goal.
    pub goal: String,
    /// The employee id this task is assigned to (`None` = orchestrator-level).
    pub assigned_to: Option<String>,
    pub status: TaskStatus,
    pub attempts: u8,
    /// The OpenCode session id running this task, once created.
    pub session_id: Option<String>,
    /// The final deliverable (on success) or the failure reason.
    pub result: Option<String>,
    /// Unix seconds at creation.
    pub created_at: i64,
    /// Unix seconds at terminal state.
    pub finished_at: Option<i64>,
}

impl Task {
    /// A freshly-created pending task.
    pub fn new(goal: impl Into<String>, assigned_to: Option<String>) -> Self {
        Self {
            id: TaskId::new(),
            goal: goal.into(),
            assigned_to,
            status: TaskStatus::Pending,
            attempts: 0,
            session_id: None,
            result: None,
            created_at: now_unix(),
            finished_at: None,
        }
    }

    /// Advance the task to `next`, returning `false` (and leaving state
    /// untouched) when the transition is not legal from the current state.
    pub fn transition(&mut self, next: TaskStatus) -> bool {
        let legal = matches!(
            (self.status, next),
            (TaskStatus::Pending, TaskStatus::Running)
                | (TaskStatus::Pending, TaskStatus::Failed)
                | (TaskStatus::Running, TaskStatus::Succeeded)
                | (TaskStatus::Running, TaskStatus::Failed)
                | (TaskStatus::Running, TaskStatus::Retrying)
                | (TaskStatus::Retrying, TaskStatus::Running)
                | (TaskStatus::Retrying, TaskStatus::Succeeded)
                | (TaskStatus::Retrying, TaskStatus::Failed)
        );
        if legal {
            self.status = next;
        }
        legal
    }

    /// Try to retry the task: bump the attempt counter and mark `Retrying`,
    /// returning `false` once the retry budget is exhausted.
    pub fn try_retry(&mut self) -> bool {
        if self.attempts >= MAX_ATTEMPTS {
            return false;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.status = TaskStatus::Retrying;
        true
    }

    /// Record a successful outcome and move to `Succeeded`.
    pub fn succeed(&mut self, result: impl Into<String>) {
        self.result = Some(result.into());
        self.finished_at = Some(now_unix());
        self.status = TaskStatus::Succeeded;
    }

    /// Record a failed outcome and move to `Failed`.
    pub fn fail(&mut self, reason: impl Into<String>) {
        // Keep the first recorded reason; a later retry's failure shouldn't
        // overwrite an earlier (more useful) one.
        if self.result.is_none() {
            self.result = Some(reason.into());
        }
        self.finished_at = Some(now_unix());
        self.status = TaskStatus::Failed;
    }
}

/// Current time as Unix seconds (0 on a catastrophic clock failure).
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_is_pending_with_no_result() {
        let task = Task::new("summarize commits", Some("planner".into()));
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_to.as_deref(), Some("planner"));
        assert_eq!(task.attempts, 0);
        assert!(task.session_id.is_none());
        assert!(task.result.is_none());
        assert!(task.finished_at.is_none());
        assert!(task.created_at > 0);
    }

    #[test]
    fn only_legal_transitions_apply() {
        let mut task = Task::new("x", None);

        // Pending → Succeeded is illegal.
        assert!(!task.transition(TaskStatus::Succeeded));
        assert_eq!(task.status, TaskStatus::Pending);

        // Pending → Running is legal.
        assert!(task.transition(TaskStatus::Running));
        assert_eq!(task.status, TaskStatus::Running);

        // Running → Pending (backwards) is illegal.
        assert!(!task.transition(TaskStatus::Pending));
        assert_eq!(task.status, TaskStatus::Running);

        // Running → Retrying is legal.
        assert!(task.transition(TaskStatus::Retrying));

        // Retrying → Running is legal.
        assert!(task.transition(TaskStatus::Running));

        // Running → Succeeded is legal and terminal.
        assert!(task.transition(TaskStatus::Succeeded));
        // Succeeded has no outgoing transitions.
        assert!(!task.transition(TaskStatus::Running));
        assert!(!task.transition(TaskStatus::Failed));
    }

    #[test]
    fn retry_is_capped() {
        let mut task = Task::new("x", None);
        for _ in 0..MAX_ATTEMPTS {
            assert!(task.try_retry());
            assert_eq!(task.status, TaskStatus::Retrying);
        }
        assert_eq!(task.attempts, MAX_ATTEMPTS);

        // Budget exhausted — no further retry.
        assert!(!task.try_retry());
        assert_eq!(task.attempts, MAX_ATTEMPTS);
    }

    #[test]
    fn succeed_and_fail_set_terminal_state() {
        let mut task = Task::new("x", None);
        task.succeed("the deliverable");
        assert_eq!(task.status, TaskStatus::Succeeded);
        assert_eq!(task.result.as_deref(), Some("the deliverable"));
        assert!(task.finished_at.is_some());

        let mut task = Task::new("x", None);
        task.fail("boom");
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.result.as_deref(), Some("boom"));

        // A later failure does not overwrite the first reason.
        task.fail("also broke");
        assert_eq!(task.result.as_deref(), Some("boom"));
    }

    #[test]
    fn round_trips_through_json() {
        let mut task = Task::new("ship the launch plan", Some("planner".into()));
        task.attempts = 2;
        task.session_id = Some("ses_123".into());
        task.succeed("done");

        let text = serde_json::to_string(&task).unwrap();
        let loaded: Task = serde_json::from_str(&text).unwrap();

        assert_eq!(loaded.id, task.id);
        assert_eq!(loaded.goal, "ship the launch plan");
        assert_eq!(loaded.assigned_to.as_deref(), Some("planner"));
        assert_eq!(loaded.status, TaskStatus::Succeeded);
        assert_eq!(loaded.attempts, 2);
        assert_eq!(loaded.result.as_deref(), Some("done"));
    }
}
