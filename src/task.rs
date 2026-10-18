//! The task model: a unit of autonomous work handed to the harness.
//!
//! A `Task` records one schedulable goal — its target employee, its lifecycle
//! state machine, and enough provenance (timestamps, session id, final result)
//! to be persisted and inspected. The `TaskStatus` transitions are validated so
//! a task can't leap from `Pending` straight to `Succeeded`, and retries are
//! bounded by [`MAX_ATTEMPTS`]. A task may also be *paused* — held out of
//! dispatch by [`Task::pause_for`]/[`Task::resume`] until the user continues it.
//! Who paused it is recorded ([`PauseReason`]) so a task mywaker held on its way
//! out can be offered back on the next launch, while a task the user paused on
//! purpose stays held.

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
    Paused,
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
            TaskStatus::Paused => "paused",
            TaskStatus::Retrying => "retrying",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
        }
    }
}

/// Who paused a task: the user, deliberately, or mywaker itself on the way out
/// so unfinished work survives the restart. The distinction is what keeps a
/// deliberate pause from being offered back for resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    /// Held by the user (`s`).
    User,
    /// Held by mywaker while shutting down.
    Shutdown,
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
    /// The task is held out of dispatch until resumed. Kept in lockstep with
    /// [`TaskStatus::Paused`] by [`Task::pause_for`] and [`Task::resume`], which
    /// are the only writers.
    #[serde(default)]
    pub paused: bool,
    /// Why the task is held, when it is. `Some` exactly when `paused` is set and
    /// the status is [`TaskStatus::Paused`] — the three move together, and
    /// `resume` clears this alongside the flag. Defaults to `None` so a store
    /// written before the field existed loads unchanged.
    #[serde(default)]
    pub pause_reason: Option<PauseReason>,
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
            paused: false,
            pause_reason: None,
            attempts: 0,
            session_id: None,
            result: None,
            created_at: now_unix(),
            finished_at: None,
        }
    }

    /// Advance the task to `next`, returning `false` (and leaving state
    /// untouched) when the transition is not legal from the current state.
    ///
    /// Pausing is its own pair of edges (`Pending`/`Running`/`Retrying` ⇄
    /// `Paused`) rather than a plain status write, so a terminal task can never
    /// be paused and a paused task can never be sent straight to
    /// `Succeeded`/`Failed` without going back through `Running`. `Retrying` is
    /// holdable because a retry in flight must be pausable at shutdown.
    pub fn transition(&mut self, next: TaskStatus) -> bool {
        let legal = matches!(
            (self.status, next),
            (TaskStatus::Pending, TaskStatus::Running)
                | (TaskStatus::Pending, TaskStatus::Paused)
                | (TaskStatus::Pending, TaskStatus::Failed)
                | (TaskStatus::Running, TaskStatus::Paused)
                | (TaskStatus::Running, TaskStatus::Succeeded)
                | (TaskStatus::Running, TaskStatus::Failed)
                | (TaskStatus::Running, TaskStatus::Retrying)
                | (TaskStatus::Retrying, TaskStatus::Paused)
                | (TaskStatus::Paused, TaskStatus::Running)
                | (TaskStatus::Retrying, TaskStatus::Running)
                | (TaskStatus::Retrying, TaskStatus::Succeeded)
                | (TaskStatus::Retrying, TaskStatus::Failed)
        );
        if legal {
            self.status = next;
        }
        legal
    }

    /// Pause the task for `reason`: hold it out of dispatch until
    /// [`Task::resume`]. Legal only while the task is queued, running, or
    /// retrying, so a finished task can't be paused. The flag, the reason, and
    /// the status move together.
    pub fn pause_for(&mut self, reason: PauseReason) -> bool {
        if !self.transition(TaskStatus::Paused) {
            return false;
        }
        self.paused = true;
        self.pause_reason = Some(reason);
        true
    }

    /// Pause the task at the user's request — [`Task::pause_for`] with
    /// [`PauseReason::User`].
    pub fn pause(&mut self) -> bool {
        self.pause_for(PauseReason::User)
    }

    /// Resume a paused task, returning it to `Running` so the caller can start a
    /// fresh attempt. `false` (and no change) when the task isn't paused. The
    /// pause reason is cleared with the flag.
    pub fn resume(&mut self) -> bool {
        if !self.paused || !self.transition(TaskStatus::Running) {
            return false;
        }
        self.paused = false;
        self.pause_reason = None;
        true
    }

    /// Whether mywaker itself paused this task on the way out — the tasks the
    /// reopen prompt offers back. A task the user paused deliberately answers
    /// `false`, so it is never offered.
    pub fn paused_at_shutdown(&self) -> bool {
        self.paused && self.pause_reason == Some(PauseReason::Shutdown)
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

    /// Pausing moves the status and the dispatch flag together; resuming does
    /// the reverse. Only a queued or running task can be paused.
    #[test]
    fn pause_and_resume_move_status_and_flag_together() {
        let mut task = Task::new("x", None);

        // A queued task can be held before it ever starts.
        assert!(task.pause());
        assert_eq!(task.status, TaskStatus::Paused);
        assert!(task.paused);

        // Resuming returns it to `Running` and clears the flag.
        assert!(task.resume());
        assert_eq!(task.status, TaskStatus::Running);
        assert!(!task.paused);

        // A running task can be paused.
        assert!(task.pause());
        assert_eq!(task.status, TaskStatus::Paused);

        // A finished task can never be paused.
        let mut done = Task::new("x", None);
        done.succeed("ok");
        assert!(!done.pause());
        assert_eq!(done.status, TaskStatus::Succeeded);
        assert!(!done.paused);

        // Resuming a task that isn't paused is a no-op.
        let mut fresh = Task::new("x", None);
        assert!(!fresh.resume());
        assert_eq!(fresh.status, TaskStatus::Pending);
    }

    /// `paused` survives a round-trip so a paused task reloads paused, and a
    /// store written before the field existed still loads (serde default).
    #[test]
    fn paused_flag_round_trips_and_defaults_for_legacy_json() {
        let mut task = Task::new("hold me", Some("writer".into()));
        assert!(task.pause());

        let text = serde_json::to_string(&task).unwrap();
        let loaded: Task = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.status, TaskStatus::Paused);
        assert!(loaded.paused);

        // A payload predating the field (no `paused` key) defaults to not-paused.
        let mut legacy = serde_json::to_value(&task).unwrap();
        legacy.as_object_mut().unwrap().remove("paused");
        let legacy: Task = serde_json::from_value(legacy).unwrap();
        assert!(!legacy.paused);
    }

    /// The pause *reason* round-trips (so a shutdown-paused task reloads as such
    /// and is offered back on reopen), and a payload predating the field loads
    /// with no reason — never mistaken for a shutdown pause.
    #[test]
    fn pause_reason_round_trips_and_defaults_for_legacy_json() {
        let mut by_user = Task::new("hold me", Some("writer".into()));
        assert!(by_user.pause_for(PauseReason::User));
        assert_eq!(by_user.pause_reason, Some(PauseReason::User));
        assert!(!by_user.paused_at_shutdown());

        let mut at_shutdown = Task::new("finish me", Some("writer".into()));
        assert!(at_shutdown.pause_for(PauseReason::Shutdown));
        assert!(at_shutdown.paused_at_shutdown());

        let text = serde_json::to_string(&at_shutdown).unwrap();
        let loaded: Task = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.pause_reason, Some(PauseReason::Shutdown));
        assert!(
            loaded.paused_at_shutdown(),
            "shutdown pause must survive reload"
        );

        // Resuming clears the reason alongside the flag.
        let mut resumed = loaded;
        assert!(resumed.resume());
        assert_eq!(resumed.pause_reason, None);
        assert!(!resumed.paused_at_shutdown());

        // A payload predating the field (no `pause_reason` key) loads as `None`.
        let mut legacy = serde_json::to_value(&at_shutdown).unwrap();
        legacy.as_object_mut().unwrap().remove("pause_reason");
        let legacy: Task = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.pause_reason, None);
        assert!(!legacy.paused_at_shutdown());
    }
}
