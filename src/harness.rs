//! Reliability primitives for employee ↔ employee hand-offs.
//!
//! Hand-offs are fire-and-forget today: a delegated worker or fan-out branch that
//! goes silent is never retried and can hang the chain forever. This module adds
//! the small, pure building blocks that make them reliable — a bounded retry
//! budget, a silence deadline, and deterministic abandon notes. The app drives
//! these from its tick loop (`tick_reliability`) and delivery path.

use std::time::{Duration, Instant};

use crate::opencode::{Delivery, PromptOptions};

/// How many times a single hand-off is retried before it is abandoned.
pub const MAX_ATTEMPTS: u8 = 3;

/// Maximum delegation depth along a chain (the real cycle-stopper; the run-wide
/// [`DELEGATION_TOTAL_LIMIT`] sits on top as a blunt circuit breaker).
pub const MAX_CHAIN_DEPTH: usize = 8;

/// Run-wide cap on hand-offs, guarding against runaway cycles.
pub const DELEGATION_TOTAL_LIMIT: u64 = 64;

/// How long a hand-off may stay silent (no SSE event for its session) before it
/// is considered stuck. The app heartbeats a pending delivery's `since` on every
/// event for its session, so this only fires on genuinely silent workers.
pub const HANDOFF_TIMEOUT: Duration = Duration::from_secs(300);

/// A delegated hand-off in flight for one employee: what was sent, how many
/// attempts have been made, and when it last showed signs of life.
#[derive(Debug, Clone)]
pub struct PendingDelivery {
    /// The instruction handed to the employee (for the retry preamble).
    pub instruction: String,
    /// Number of deliveries already made (0 = the original send).
    pub attempts: u8,
    /// When the delivery's turn actually started, or the last liveness
    /// timestamp. Meaningless until [`Self::armed`] is set.
    pub since: Instant,
    /// Whether the silence clock is running.
    ///
    /// A delivery is **admitted** when it is sent but **armed** only once the
    /// session reports `session.execution.started` for it. Queue-delivered
    /// hand-offs sit behind the turn in flight, so counting silence from the send
    /// would time out a perfectly healthy prompt that simply had not been reached
    /// yet — and then "retry" it, admitting the same work twice. The clock
    /// therefore starts when the work does, not when it was asked for.
    pub armed: bool,
}

impl PendingDelivery {
    pub fn new(instruction: String) -> Self {
        Self {
            instruction,
            attempts: 0,
            since: Instant::now(),
            armed: false,
        }
    }

    /// True when the delivery has been silent past the timeout. A delivery that
    /// has not started executing is never silent — it is waiting.
    pub fn expired(&self, now: Instant) -> bool {
        self.armed && now.duration_since(self.since) >= HANDOFF_TIMEOUT
    }

    /// Start the silence clock: the session began executing this delivery.
    pub fn arm(&mut self) {
        self.armed = true;
        self.since = Instant::now();
    }

    /// Record another attempt and reset the clock to *unarmed*: the re-issued
    /// prompt is admitted again and must wait for its own execution to start.
    pub fn bump(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.armed = false;
        self.since = Instant::now();
    }

    /// Touch the liveness clock without an attempt (an event arrived). Does not
    /// arm a delivery that has not started.
    pub fn heartbeat(&mut self) {
        if self.armed {
            self.since = Instant::now();
        }
    }

    /// True once the retry budget is used up.
    pub fn exhausted(&self) -> bool {
        self.attempts >= MAX_ATTEMPTS
    }
}

/// Preamble prepended when re-issuing a hand-off that failed or timed out.
pub fn retry_preamble(attempt: u8) -> String {
    format!(
        "The previous attempt failed or timed out (attempt {attempt} of {MAX_ATTEMPTS}); \
         please retry the task from scratch."
    )
}

/// Preamble prepended when re-issuing a task the user paused: the earlier
/// attempt was interrupted, so the model should treat it as unfinished and
/// start over rather than assume the transcript is a complete result.
pub fn resume_preamble() -> String {
    "The previous attempt was paused and interrupted; please start this task over from the \
     beginning."
        .to_string()
}

/// Deterministic note standing in for a deliverable that never arrived, so a
/// fan-out still aggregates instead of waiting forever for a worker that will
/// never respond. `reason` is omitted when the hand-off produced nothing at all
/// (an empty reply), so the note reads as a statement rather than a diagnosis.
pub fn abandon_note(name: &str, reason: Option<&str>) -> String {
    match reason {
        Some(reason) => format!("⚠ no deliverable from {name} ({reason})"),
        None => format!("⚠ no deliverable from {name}"),
    }
}

/// What one prompt to an employee *is* — the input to [`delivery_for`].
///
/// Every path that puts text into a session names itself here, so the
/// steer-vs-queue policy is one table rather than a decision re-made at each call
/// site (where it would drift).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// The user typed this in a chat. They are waiting on it and expect it to
    /// take over.
    UserPrompt,
    /// The root (or a peer) handed a piece of work down.
    Delegation,
    /// A worker reported a finished part back up.
    Report,
    /// A parallel round's brief to one worker.
    FanoutBrief,
    /// A hand-off being re-issued after it went silent.
    Retry,
    /// A cron or watch fire.
    AutonomousTask,
}

impl PromptKind {
    /// A short stable name, used to build the admission id.
    pub fn label(self) -> &'static str {
        match self {
            PromptKind::UserPrompt => "user",
            PromptKind::Delegation => "delegation",
            PromptKind::Report => "report",
            PromptKind::FanoutBrief => "fanout",
            PromptKind::Retry => "retry",
            PromptKind::AutonomousTask => "task",
        }
    }

    /// True when this kind is the user speaking. Only the user's own prompt may
    /// interrupt a turn.
    pub fn is_user(self) -> bool {
        matches!(self, PromptKind::UserPrompt)
    }
}

/// The one place the delivery policy is decided.
///
/// **Only a user prompt steers.** Everything the app does on its own — a
/// delegation, a report, a fan-out brief, a retry, an autonomous task — is
/// *queued*, so background traffic waits for the turn in flight instead of
/// cutting it off mid-thought. Steering on a delegation would interrupt the very
/// work it is trying to add to.
pub fn delivery_for(kind: PromptKind) -> Delivery {
    match kind {
        PromptKind::UserPrompt => Delivery::Steer,
        PromptKind::Delegation
        | PromptKind::Report
        | PromptKind::FanoutBrief
        | PromptKind::Retry
        | PromptKind::AutonomousTask => Delivery::Queue,
    }
}

/// The admission id for one delivery: `(employee, chain depth, attempt)`.
///
/// Idempotency is what makes a retry safe. The same logical delivery — the same
/// employee, at the same point in the same chain, on the same attempt — always
/// produces the same id, so a resend after a timeout is deduplicated by the
/// service instead of starting a second turn. A genuinely different delivery
/// (another employee, one step deeper, a later attempt) gets a different id.
pub fn admission_id(kind: PromptKind, employee: &str, depth: usize, attempt: u8) -> String {
    format!("mywaker:{}:{employee}:{depth}:{attempt}", kind.label())
}

/// The options for one delivery: the policy from [`delivery_for`] plus the
/// idempotent admission id.
pub fn prompt_options(
    kind: PromptKind,
    employee: &str,
    depth: usize,
    attempt: u8,
) -> PromptOptions {
    PromptOptions::new(
        delivery_for(kind),
        admission_id(kind, employee, depth, attempt),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the user's own prompt steers. Everything the app does on its own is
    /// queued behind the turn in flight — steering a delegation would interrupt
    /// the very work it is adding to.
    #[test]
    fn only_a_user_prompt_steers() {
        assert_eq!(delivery_for(PromptKind::UserPrompt), Delivery::Steer);

        for kind in [
            PromptKind::Delegation,
            PromptKind::Report,
            PromptKind::FanoutBrief,
            PromptKind::Retry,
            PromptKind::AutonomousTask,
        ] {
            assert_eq!(
                delivery_for(kind),
                Delivery::Queue,
                "{kind:?} must not interrupt the turn in flight"
            );
        }
    }

    /// The admission id is derived from the delivery's identity, so a resend of
    /// the same logical delivery is deduplicated and a different one is not.
    #[test]
    fn admission_ids_are_stable_per_delivery_and_distinct_across_deliveries() {
        let first = admission_id(PromptKind::Delegation, "writer", 2, 0);

        // Same employee, same depth, same attempt → the same id.
        assert_eq!(first, admission_id(PromptKind::Delegation, "writer", 2, 0));

        // A retry of the same hand-off is a *different* delivery.
        assert_ne!(first, admission_id(PromptKind::Delegation, "writer", 2, 1));
        // As is one step deeper, another employee, or another kind.
        assert_ne!(first, admission_id(PromptKind::Delegation, "writer", 3, 0));
        assert_ne!(first, admission_id(PromptKind::Delegation, "coder", 2, 0));
        assert_ne!(first, admission_id(PromptKind::Report, "writer", 2, 0));

        // It names the kind and the employee, so a log line is readable.
        assert!(first.contains("delegation"), "{first}");
        assert!(first.contains("writer"), "{first}");
    }

    /// The options pair the policy with the id — one call site cannot take one
    /// without the other.
    #[test]
    fn prompt_options_carry_both_the_policy_and_the_id() {
        let options = prompt_options(PromptKind::Report, "writer", 1, 0);
        assert_eq!(options.delivery, Delivery::Queue);
        assert_eq!(
            options.admission_id.as_deref(),
            Some(admission_id(PromptKind::Report, "writer", 1, 0).as_str())
        );

        let user = prompt_options(PromptKind::UserPrompt, "writer", 0, 0);
        assert_eq!(user.delivery, Delivery::Steer);
    }

    #[test]
    fn pending_delivery_retry_and_exhaustion() {
        let mut p = PendingDelivery::new("draft the doc".into());
        assert!(!p.exhausted());
        assert_eq!(p.attempts, 0);

        p.bump();
        assert_eq!(p.attempts, 1);
        assert!(!p.exhausted());

        p.bump();
        assert_eq!(p.attempts, 2);
        assert!(!p.exhausted());

        p.bump();
        assert_eq!(p.attempts, MAX_ATTEMPTS);
        assert!(p.exhausted());
    }

    #[test]
    fn pending_delivery_expiry_and_heartbeat() {
        let mut p = PendingDelivery::new("x".into());
        assert!(!p.expired(Instant::now()));

        // An admitted-but-not-started delivery is never "silent": it is waiting
        // behind the turn in flight, so no amount of wall time expires it.
        p.since = Instant::now() - HANDOFF_TIMEOUT - Duration::from_secs(1);
        assert!(
            !p.expired(Instant::now()),
            "a queued delivery is not silent"
        );

        // Armed (its execution started) and then silent past the timeout, it is
        // expired.
        p.arm();
        p.since = Instant::now() - HANDOFF_TIMEOUT - Duration::from_secs(1);
        assert!(p.expired(Instant::now()));

        // A heartbeat resets the clock while armed…
        p.heartbeat();
        assert!(!p.expired(Instant::now()));

        // …and re-issuing the hand-off un-arms it: the new prompt is admitted
        // again and must wait for its own execution to start.
        p.bump();
        assert!(!p.armed);
        p.since = Instant::now() - HANDOFF_TIMEOUT - Duration::from_secs(1);
        assert!(!p.expired(Instant::now()));
    }

    #[test]
    fn retry_preamble_and_abandon_note() {
        let preamble = retry_preamble(2);
        assert!(preamble.contains("attempt 2"));
        assert!(preamble.contains(&MAX_ATTEMPTS.to_string()));

        // The resume preamble reads as a pause, not a retry.
        let resume = resume_preamble();
        assert!(resume.contains("paused"));
        assert!(!resume.contains("failed"));

        // A reason is spelled out in parentheses; no reason leaves the note as a
        // bare statement — both name the worker whose deliverable is missing.
        let with_reason = abandon_note("Bruno", Some("timed out"));
        assert!(with_reason.contains("Bruno"));
        assert!(with_reason.contains("timed out"));

        let bare = abandon_note("Bruno", None);
        assert_eq!(bare, "⚠ no deliverable from Bruno");
    }
}
