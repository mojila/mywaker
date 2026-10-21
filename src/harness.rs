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
    /// The kind this delivery was sent as, kept so a **resend** can repeat it.
    /// The kind is part of the admission id, so re-offering a delivery under a
    /// different kind would change its id and defeat deduplication.
    pub kind: PromptKind,
    /// The delivery's identity token, minted once when it was queued.
    ///
    /// A **resend** reuses this, so the admission id repeats and a service that
    /// already admitted the delivery deduplicates the second attempt instead of
    /// running the work twice. A silent-worker re-issue deliberately does *not*
    /// reuse it: that is new work, and it mints a fresh token.
    pub token: String,
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
    pub fn new(instruction: String, kind: PromptKind, token: String) -> Self {
        Self {
            instruction,
            kind,
            token,
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

/// The prefix every client-supplied admission id must carry.
///
/// OpenCode types a prompt's `id` as `Session.Message.ID`, which is
/// `^msg_`-anchored. A well-meaning `mywaker:…` id is rejected outright with
/// `400 InvalidRequestError: Expected a string starting with "msg_" at ["id"]`,
/// so the prefix is part of the contract, not a convention.
const ADMISSION_PREFIX: &str = "msg_mywaker";

/// A fresh token identifying one logical delivery.
///
/// The `(kind, employee, depth)` tuple alone does **not** identify a delivery: a
/// user's second prompt to the same employee, a fresh session after `/new`, and
/// two independent chains that reach the same employee at the same depth all
/// produce the same tuple. OpenCode's admission ids are unique across the whole
/// service (a repeat in *another* session is a `409 ConflictError`), and within
/// one session a repeat is silently deduplicated — the second message would be
/// swallowed. So each delivery carries its own token, minted once when the
/// delivery is created and reused only if that exact delivery is re-sent.
pub fn delivery_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The admission id for one delivery: `(kind, employee, chain depth, token)`.
///
/// The id starts with `msg_` because OpenCode requires it, and it is unique per
/// logical delivery because of the token. Re-sending the *same* delivery (same
/// tuple, same token) repeats the id, which is what lets the service deduplicate
/// it instead of starting a second turn; a genuinely different delivery (another
/// employee, one step deeper, a new token) gets a different id.
///
/// The retry **attempt** is deliberately not part of the id. It is the sender's
/// own budget — how many times this app has tried — not part of what is being
/// delivered, so folding it in made every resend look like a new message and
/// defeated the deduplication the token exists for. Two re-issues therefore
/// differ by *token*, not by attempt: a resend of a delivery that never landed
/// keeps its token (and so its id), while a silent worker being asked again is
/// new work and mints a fresh one.
pub fn admission_id(kind: PromptKind, employee: &str, depth: usize, token: &str) -> String {
    format!(
        "{ADMISSION_PREFIX}:{}:{employee}:{depth}:{token}",
        kind.label()
    )
}

/// The options for one delivery: the policy from [`delivery_for`] plus the
/// idempotent admission id.
pub fn prompt_options(
    kind: PromptKind,
    employee: &str,
    depth: usize,
    token: &str,
) -> PromptOptions {
    PromptOptions::new(
        delivery_for(kind),
        admission_id(kind, employee, depth, token),
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
        let token = "abc123";
        let first = admission_id(PromptKind::Delegation, "writer", 2, token);

        // Same employee, same depth, same token → the same id.
        assert_eq!(
            first,
            admission_id(PromptKind::Delegation, "writer", 2, token)
        );

        // The retry budget is not part of the identity: re-sending this exact
        // delivery is the *same* admission however many times it has been tried.
        // Folding the attempt in here would give every resend a new id and the
        // service would admit the work twice instead of deduplicating it.
        assert_eq!(first, "msg_mywaker:delegation:writer:2:abc123");

        // A different delivery gets a different id: another employee, another
        // kind, one step deeper, or a fresh token.
        assert_ne!(
            first,
            admission_id(PromptKind::Delegation, "writer", 3, token)
        );
        assert_ne!(
            first,
            admission_id(PromptKind::Delegation, "coder", 2, token)
        );
        assert_ne!(first, admission_id(PromptKind::Report, "writer", 2, token));
        assert_ne!(
            first,
            admission_id(PromptKind::Delegation, "writer", 2, "other")
        );

        // It names the kind and the employee, so a log line is readable.
        assert!(first.contains("delegation"), "{first}");
        assert!(first.contains("writer"), "{first}");
    }

    /// OpenCode types a prompt's `id` as `Session.Message.ID` (`^msg_`). An id
    /// without that prefix is rejected with a 400, which is what broke every
    /// prompt before this was pinned.
    #[test]
    fn admission_ids_start_with_the_prefix_the_service_requires() {
        for kind in [
            PromptKind::UserPrompt,
            PromptKind::Delegation,
            PromptKind::Report,
            PromptKind::FanoutBrief,
            PromptKind::Retry,
            PromptKind::AutonomousTask,
        ] {
            let id = admission_id(kind, "orchestrator", 0, &delivery_token());
            assert!(id.starts_with("msg_"), "{kind:?} produced {id}");
        }
    }

    /// Two deliveries that share `(kind, employee, depth)` are still distinct: a
    /// user's second prompt, or a fresh session after `/new`, must not reuse an
    /// admission id. Reusing one makes the service swallow the message (same
    /// session) or reject it with a 409 (another session).
    #[test]
    fn two_deliveries_with_the_same_shape_get_different_ids() {
        let first = admission_id(PromptKind::UserPrompt, "orchestrator", 0, &delivery_token());
        let second = admission_id(PromptKind::UserPrompt, "orchestrator", 0, &delivery_token());
        assert_ne!(first, second);
    }

    /// The options pair the policy with the id — one call site cannot take one
    /// without the other.
    #[test]
    fn prompt_options_carry_both_the_policy_and_the_id() {
        let token = delivery_token();
        let options = prompt_options(PromptKind::Report, "writer", 1, &token);
        assert_eq!(options.delivery, Delivery::Queue);
        assert_eq!(
            options.admission_id.as_deref(),
            Some(admission_id(PromptKind::Report, "writer", 1, &token).as_str())
        );

        let user = prompt_options(PromptKind::UserPrompt, "writer", 0, &token);
        assert_eq!(user.delivery, Delivery::Steer);
    }

    #[test]
    fn pending_delivery_retry_and_exhaustion() {
        let mut p =
            PendingDelivery::new("draft the doc".into(), PromptKind::Delegation, "tok".into());
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
        let mut p = PendingDelivery::new("x".into(), PromptKind::Delegation, "tok".into());
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
