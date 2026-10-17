//! Cron scheduler: fires autonomous tasks on a cadence.
//!
//! Each `[[schedules]]` entry in the config becomes a cron job. When a job
//! fires it sends a [`Message::ScheduledTask`] into the app's message channel,
//! which the app turns into a persisted `Task` and dispatches to the named
//! employee. Invalid cron expressions are skipped (with a log line) rather than
//! taking the whole app down.

use color_eyre::eyre::Result;
use tokio::sync::mpsc::Sender;
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::config::Schedule;
use crate::message::Message;

/// Start the scheduler and run until the task is dropped. Returns immediately
/// if there are no schedules to arm.
pub async fn run(tx: Sender<Message>, schedules: Vec<Schedule>) -> Result<()> {
    if schedules.is_empty() {
        return Ok(());
    }

    let scheduler = JobScheduler::new().await?;

    let mut armed = 0usize;
    for schedule in schedules {
        let (employee, goal, cron) = (schedule.employee, schedule.goal, schedule.cron);
        if employee.is_empty() || goal.is_empty() {
            tracing::warn!("skipping schedule with empty employee/goal: {cron:?}");
            continue;
        }

        let tx = tx.clone();
        let job = match Job::new_async(cron.clone(), move |_uuid, _scheduler| {
            let tx = tx.clone();
            let employee = employee.clone();
            let goal = goal.clone();
            Box::pin(async move {
                let _ = tx.send(Message::ScheduledTask { employee, goal }).await;
            })
        }) {
            Ok(job) => job,
            Err(err) => {
                tracing::warn!("skipping invalid cron {cron:?}: {err}");
                continue;
            }
        };

        scheduler.add(job).await?;
        armed += 1;
        tracing::info!("armed schedule {cron:?}");
    }

    if armed == 0 {
        return Ok(());
    }

    scheduler.start().await?;
    tracing::info!("scheduler started ({armed} job(s))");

    // Keep the scheduler (and its internal actors) alive for the app's lifetime.
    std::future::pending::<()>().await;
    Ok(())
}
