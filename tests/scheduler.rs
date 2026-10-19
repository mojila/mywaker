//! End-to-end cron fire: a real [`mywaker::scheduler::run`] job sends a
//! [`Message::ScheduledTask`] into the app's channel, and the app turns it into a
//! dispatched, persisted task that survives a restart.
//!
//! This exercises the real `tokio-cron-scheduler` backend (no OpenCode service and
//! no model budget involved), mirroring `tests/watch_hook.rs`. The fire tests use
//! the six-field, seconds-required form the crate actually parses
//! (`tokio-cron-scheduler 0.15.1` builds its parser with
//! `croner::parser::Seconds::Required`), so they spend a second rather than a
//! minute; [`scheduler::normalize`] is covered directly by
//! `a_five_field_crontab_normalizes_to_the_seconds_required_form`.
//!
//! Two claims, one test each:
//!
//! - **fire → dispatch**: the message arrives, and `App::update` records a
//!   `Running` task for the named employee, names the cron trigger in the log, and
//!   puts that employee to work.
//! - **fire → restart**: the same fired task, held by the quit path, is written to
//!   a store, reloaded, and still qualifies for the reopen prompt — it is
//!   shutdown-paused, assigned to the same employee, and carries the same goal.

use std::time::Duration;

use mywaker::app::App;
use mywaker::config::{Config, Schedule};
use mywaker::message::Message;
use mywaker::persistence::Store;
use mywaker::task::TaskStatus;
use mywaker::team::EmployeeStatus;

/// A cron that fires every second — the shortest cadence the six-field parser
/// accepts, so the test spends a second rather than a minute.
const EVERY_SECOND: &str = "*/1 * * * * *";

/// The same cadence written the way the docs write it: five fields, minutes
/// first. This is the form `normalize` exists for.
const EVERY_MINUTE_FIVE_FIELD: &str = "* * * * *";

/// Arm a real scheduler and return the first `ScheduledTask` it sends.
///
/// The scheduler task is aborted before returning, so no job outlives the test.
async fn first_fire(employee: &str, goal: &str) -> Message {
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let handle = tokio::spawn(mywaker::scheduler::run(
        tx,
        vec![Schedule {
            cron: EVERY_SECOND.to_string(),
            employee: employee.to_string(),
            goal: goal.to_string(),
        }],
    ));

    let received = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("a scheduled task within 10s")
        .expect("the scheduler channel stayed open");
    assert!(
        matches!(received, Message::ScheduledTask { .. }),
        "the scheduler sent an unexpected message: {received:?}"
    );

    handle.abort();
    received
}

/// P0, end to end: a five-field crontab expression is armed rather than skipped.
///
/// `run` returns `Ok(())` immediately when nothing armed, so "still running after
/// a second" is proof the job was accepted — the regression was that the
/// seconds-required parser rejected the five-field form and the loop fell
/// through to the empty-scheduler return.
#[tokio::test]
async fn a_five_field_crontab_is_armed_not_skipped() {
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let handle = tokio::spawn(mywaker::scheduler::run(
        tx,
        vec![Schedule {
            cron: EVERY_MINUTE_FIVE_FIELD.to_string(),
            employee: "planner".to_string(),
            goal: "morning briefing".to_string(),
        }],
    ));

    // Give the arming path time to run and then return if it were going to.
    tokio::time::sleep(Duration::from_millis(750)).await;
    assert!(
        !handle.is_finished(),
        "a five-field schedule must arm a job, not fall through to the empty-scheduler return"
    );
    handle.abort();
}

/// The counterpart: a field count `normalize` rejects still lets `run` return
/// cleanly (with a warning) instead of arming a broken job or panicking.
#[tokio::test]
async fn an_unparseable_field_count_returns_without_arming() {
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        mywaker::scheduler::run(
            tx,
            vec![Schedule {
                cron: "* * *".to_string(),
                employee: "planner".to_string(),
                goal: "never fires".to_string(),
            }],
        ),
    )
    .await;

    let result = result.expect("run must return when nothing armed");
    assert!(result.is_ok(), "a bad field count is a skip, not an error");
}

/// P1, first half: a cron fire becomes a dispatched task for the named employee.
#[tokio::test]
async fn a_cron_fire_dispatches_a_scheduled_task() {
    let goal = "summarise the overnight build failures";
    let message = first_fire("reviewer", goal).await;

    match message {
        Message::ScheduledTask {
            employee,
            goal: fired,
        } => {
            assert_eq!(employee, "reviewer");
            assert_eq!(fired, goal);
        }
        other => panic!("expected a scheduled task, got {other:?}"),
    }

    // Feed the real message through the app's own update path. No client is
    // attached, so `dispatch_task` stops at its "not connected" guard — but the
    // task is recorded and marked `Running` *before* that, which is the state a
    // dispatch leaves behind.
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let mut app = App::new(Config::default(), tx);
    app.update(Message::ScheduledTask {
        employee: "reviewer".into(),
        goal: goal.into(),
    });

    let task = app.tasks().last().expect("a task was recorded");
    assert_eq!(task.assigned_to.as_deref(), Some("reviewer"));
    assert_eq!(task.goal, goal);
    assert_eq!(task.status, TaskStatus::Running);
    assert!(!task.paused, "a dispatched fire is not held");

    let index = app
        .team()
        .employees()
        .iter()
        .position(|employee| employee.id == "reviewer")
        .expect("the starter roster has a reviewer");
    assert_eq!(
        app.team().employees()[index].status,
        EmployeeStatus::Thinking,
        "the fired employee is put to work"
    );
    assert_eq!(
        app.team().employees()[index].last_task.as_deref(),
        Some(goal),
        "the card shows the fired goal"
    );
    assert!(
        app.log()
            .iter()
            .any(|line| line.contains("⌛ scheduled task")),
        "the log names the cron trigger: {:?}",
        app.log()
    );
}

/// P1, second half (the restart slice): a scheduler-fired task is still there
/// after a relaunch, and the quit path hands it back.
///
/// The store is written to a temporary file and reloaded through the same
/// `Store::save`/`Store::load` pair the app uses, so the round trip is the real
/// one. "Offered back" is decided by `Task::paused_at_shutdown()` — the exact
/// predicate `App::prepare_resume_prompt` filters on — rather than by re-running
/// the prompt, whose own behaviour is pinned by AC5/AC7 in `src/app/tests.rs`.
#[tokio::test]
async fn a_scheduled_task_survives_a_restart() {
    let goal = "triage the new issues";
    let message = first_fire("researcher", goal).await;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let mut app = App::new(Config::default(), tx);
    app.update(message);
    let fired = app.tasks().last().expect("a task was recorded").clone();
    assert_eq!(fired.status, TaskStatus::Running);

    // Quit: the close path holds every non-terminal task.
    app.update(Message::Quit);
    let held = app
        .tasks()
        .iter()
        .find(|task| task.id == fired.id)
        .expect("the fired task is still in the queue");
    assert_eq!(
        held.status,
        TaskStatus::Paused,
        "quit holds the fired task rather than dropping it"
    );
    assert!(
        held.paused_at_shutdown(),
        "the hold is mywaker's own, so the next launch may offer it back"
    );

    // Persist exactly what the app holds, then reload as the next launch would.
    let dir = std::env::temp_dir().join(format!("mywaker-sched-{}", std::process::id()));
    let path = dir.join("store.json");
    let _ = std::fs::remove_dir_all(&dir);

    let mut store = Store::default();
    for task in app.tasks() {
        store.upsert_task(task.clone());
    }
    store.save(&path).expect("save the store");

    let loaded = Store::load(&path).expect("reload the store");
    let _ = std::fs::remove_dir_all(&dir);

    let task = loaded
        .tasks
        .iter()
        .find(|task| task.id == fired.id)
        .expect("the fired task survived the restart");
    assert_eq!(task.goal, goal, "the goal survived verbatim");
    assert_eq!(
        task.assigned_to.as_deref(),
        Some("researcher"),
        "the task is still the fired employee's"
    );
    assert!(
        task.paused_at_shutdown(),
        "a task held at quit is offered back on the next launch"
    );
}
