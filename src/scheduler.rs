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

/// Normalize a user-written cron expression to the form the scheduler parses.
///
/// `tokio-cron-scheduler` builds its parser with `Seconds::Required`, so the
/// first field is always **seconds**. People write the familiar five-field
/// crontab form instead, where the first field is minutes and seconds are
/// implicitly `0`. That mismatch is silent: a five-field expression fails to
/// parse, the job is skipped, and nothing ever fires.
///
/// - **5 fields** (`minute hour day-of-month month day-of-week`) — the standard
///   crontab form. Seconds default to `0`, so `"0 "` is prepended.
/// - **6 fields** (`second minute hour day-of-month month day-of-week`) — the
///   seconds-required form; passed through unchanged.
/// - **7 fields** (six fields plus a trailing `year`) — passed through
///   unchanged.
/// - **Anything else** — `None`; the caller skips the schedule with a log line.
///
/// Whitespace is trimmed and runs of whitespace are collapsed, so `"0  9 * * 1-5"`
/// normalizes the same as `"0 9 * * 1-5"`. A 6/7-field expression that is already
/// well-formed is returned unchanged.
pub fn normalize(expression: &str) -> Option<String> {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    let normalized = match fields.len() {
        5 => format!("0 {}", fields.join(" ")),
        6 | 7 => fields.join(" "),
        _ => return None,
    };
    Some(normalized)
}

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
        // The scheduler parses seconds-first; a five-field crontab expression is
        // normalized to six fields here (see [`normalize`]).
        let Some(cron) = normalize(&cron) else {
            tracing::warn!(
                "skipping cron {cron:?}: expected 5, 6, or 7 fields, found {}",
                cron.split_whitespace().count()
            );
            continue;
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this function exists for: the crate's parser requires a seconds
    /// field, so a five-field crontab expression is silently rejected. The
    /// standard form gains a leading `0` second.
    #[test]
    fn five_fields_gain_a_leading_zero_second() {
        assert_eq!(normalize("0 9 * * 1-5").as_deref(), Some("0 0 9 * * 1-5"));
        assert_eq!(normalize("*/15 * * * *").as_deref(), Some("0 */15 * * * *"));
        assert_eq!(normalize("0 0 1 1 *").as_deref(), Some("0 0 0 1 1 *"));
    }

    /// Six fields already carry seconds, so they pass through untouched.
    #[test]
    fn six_fields_pass_through_unchanged() {
        assert_eq!(normalize("*/1 * * * * *").as_deref(), Some("*/1 * * * * *"));
        assert_eq!(
            normalize("30 0 9 * * 1-5").as_deref(),
            Some("30 0 9 * * 1-5")
        );
    }

    /// Seven fields add a trailing year; still passed through.
    #[test]
    fn seven_fields_pass_through_unchanged() {
        assert_eq!(
            normalize("0 0 9 1 1 * 2027").as_deref(),
            Some("0 0 9 1 1 * 2027")
        );
    }

    /// Any other field count is not a cron expression the scheduler can arm.
    #[test]
    fn other_field_counts_are_rejected() {
        for expression in ["", "   ", "*", "* *", "* * *", "* * * *", "* * * * * * * *"] {
            assert_eq!(
                normalize(expression),
                None,
                "{expression:?} should be rejected"
            );
        }
    }

    /// Surrounding whitespace and runs of whitespace are normalized, so a
    /// hand-edited TOML value behaves like a tidy one.
    #[test]
    fn whitespace_is_trimmed_and_collapsed() {
        assert_eq!(
            normalize("  0   9  *  *  1-5 ").as_deref(),
            Some("0 0 9 * * 1-5")
        );
        assert_eq!(
            normalize("\t*/5\t* * * *\n").as_deref(),
            Some("0 */5 * * * *")
        );
    }

    /// A five-field expression that survives `normalize` must actually parse in
    /// the scheduler — the end-to-end claim, not just the string transform.
    #[tokio::test]
    async fn normalized_five_field_expression_arms_a_job() {
        let cron = normalize("0 9 * * 1-5").expect("five fields normalize");
        assert!(
            Job::new_async(cron.clone(), |_, _| Box::pin(async {})).is_ok(),
            "{cron:?} should be a valid scheduler expression"
        );
        // The raw five-field form does not: this is the regression being fixed.
        assert!(
            Job::new_async("0 9 * * 1-5", |_, _| Box::pin(async {})).is_err(),
            "the scheduler rejects the un-normalized form — normalize is load-bearing"
        );
    }
}
