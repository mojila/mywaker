//! Reliability primitives for employee ↔ employee hand-offs.
//!
//! Hand-offs are fire-and-forget today: a delegated worker or fan-out branch that
//! goes silent is never retried and can hang the chain forever. This module adds
//! the small, pure building blocks that make them reliable — a bounded retry
//! budget, a silence deadline, and deterministic abandon notes. The app drives
//! these from its tick loop (`tick_reliability`) and delivery path.

use std::time::{Duration, Instant};

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
    /// Last known liveness timestamp (send time, or last seen event).
    pub since: Instant,
}

impl PendingDelivery {
    pub fn new(instruction: String) -> Self {
        Self {
            instruction,
            attempts: 0,
            since: Instant::now(),
        }
    }

    /// True when the delivery has been silent past the timeout.
    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.since) >= HANDOFF_TIMEOUT
    }

    /// Record another attempt and reset the liveness clock.
    pub fn bump(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.since = Instant::now();
    }

    /// Touch the liveness clock without an attempt (an event arrived).
    pub fn heartbeat(&mut self) {
        self.since = Instant::now();
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

/// Deterministic note injected when a hand-off is abandoned, so a fan-out still
/// aggregates instead of waiting forever for a worker that will never respond.
pub fn abandon_note(name: &str, reason: &str) -> String {
    format!("⚠ no deliverable from {name} ({reason})")
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // A delivery far in the past is expired.
        p.since = Instant::now() - HANDOFF_TIMEOUT - Duration::from_secs(1);
        assert!(p.expired(Instant::now()));

        // A heartbeat resets the clock.
        p.heartbeat();
        assert!(!p.expired(Instant::now()));
    }

    #[test]
    fn retry_preamble_and_abandon_note() {
        let preamble = retry_preamble(2);
        assert!(preamble.contains("attempt 2"));
        assert!(preamble.contains(&MAX_ATTEMPTS.to_string()));

        let note = abandon_note("Bruno", "timed out");
        assert!(note.contains("Bruno"));
        assert!(note.contains("timed out"));
    }
}
