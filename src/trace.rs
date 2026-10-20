//! Orchestration tracing: one id per goal, carried by every hop it causes.
//!
//! A delegation chain is a distributed conversation — a goal goes down, reports
//! come back up, retries re-issue it, fan-outs fork it. Until now the only record
//! was the status log, where a line could not be tied back to the goal that
//! caused it, so a stuck chain produced a screen of unrelated-looking messages
//! and no way to name the run in a bug report.
//!
//! A [`TraceId`] fixes that. The goal mints one; every hop it spawns records a
//! numbered [`Span`] under it; every failure line is prefixed with the id. The
//! ids are deliberately short (`t7`, step `t7.3`) so they survive being pasted
//! out of a terminal and read back out of a log.
//!
//! Timing is recorded as an offset from the trace's start rather than a wall
//! clock: the question a trace answers is "what took 12 minutes", and an offset
//! makes that visible without arithmetic. The absolute start is kept too, so a
//! trace can be lined up against OpenCode's own session timestamps.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant, SystemTime};

/// The id of one orchestration run. Short on purpose — it is typed, pasted, and
/// grepped by hand.
pub type TraceId = u64;

/// How many traces are retained. A window rather than a list that grows forever:
/// the interesting trace is always one of the most recent.
pub const TRACE_LIMIT: usize = 32;

/// How many spans are retained per trace, guarding a runaway chain from growing
/// the record without bound.
pub const SPAN_LIMIT: usize = 64;

/// What one hop in a chain did. The label is what the trace prints, so it is
/// chosen to be readable in a pasted bug report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The user's goal entered the team.
    Goal,
    /// A hand-off down to a teammate.
    Delegation,
    /// A result bubbling back up.
    Report,
    /// A hand-off re-issued after it went silent or failed.
    Retry,
    /// A hand-off given up on.
    Abandon,
    /// The final answer reaching the user.
    Deliver,
    /// A parallel round opening or closing.
    Fanout,
    /// A hop that failed.
    Failed,
}

impl Kind {
    /// The label printed in a trace line.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Goal => "goal",
            Kind::Delegation => "delegate",
            Kind::Report => "report",
            Kind::Retry => "retry",
            Kind::Abandon => "abandon",
            Kind::Deliver => "deliver",
            Kind::Fanout => "fanout",
            Kind::Failed => "failed",
        }
    }
}

/// One hop in a traced chain.
#[derive(Debug, Clone)]
pub struct Span {
    /// 1-based position within the trace.
    pub step: u32,
    pub kind: Kind,
    /// The employee the hop left, when it has one.
    pub from: Option<String>,
    /// The employee (or `user`) the hop reached, when it has one.
    pub to: Option<String>,
    /// What moved — the instruction, the report, or the failure reason.
    pub detail: String,
    /// How long after the trace started this hop happened.
    pub after: Duration,
}

impl Span {
    /// The `from → to` route, or whichever half exists.
    pub fn route(&self) -> String {
        match (&self.from, &self.to) {
            (Some(from), Some(to)) => format!("{from} → {to}"),
            (Some(from), None) => from.clone(),
            (None, Some(to)) => format!("→ {to}"),
            (None, None) => String::new(),
        }
    }
}

/// One orchestration run: the goal that started it and every hop it caused.
#[derive(Debug, Clone)]
pub struct Trace {
    pub id: TraceId,
    /// The goal as the user stated it.
    pub goal: String,
    pub started: Instant,
    /// The wall clock at the start, for lining a trace up against OpenCode's own
    /// session timestamps.
    pub started_wall: SystemTime,
    pub spans: Vec<Span>,
}

impl Trace {
    /// How long this trace has been running.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The most recent span, when there is one.
    pub fn last_span(&self) -> Option<&Span> {
        self.spans.last()
    }
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t{}", self.id)
    }
}

/// The trace store: allocates ids and keeps the recent window.
#[derive(Debug, Default)]
pub struct Traces {
    next: TraceId,
    open: BTreeMap<TraceId, Trace>,
    latest: Option<TraceId>,
}

impl Traces {
    /// Start a trace for `goal` and return its id.
    ///
    /// Ids start at 1 so `t0` is never a real trace (an absent trace renders as
    /// no prefix rather than as `t0`).
    pub fn begin(&mut self, goal: impl Into<String>) -> TraceId {
        self.next += 1;
        let id = self.next;
        self.open.insert(
            id,
            Trace {
                id,
                goal: goal.into(),
                started: Instant::now(),
                started_wall: SystemTime::now(),
                spans: Vec::new(),
            },
        );
        self.latest = Some(id);
        self.prune();
        id
    }

    /// Record one hop under `id`, returning its step number.
    ///
    /// Returns `None` for an id that has already fallen out of the window, so a
    /// late arrival from an old chain is dropped rather than resurrecting it.
    pub fn record(
        &mut self,
        id: TraceId,
        kind: Kind,
        from: Option<String>,
        to: Option<String>,
        detail: impl Into<String>,
    ) -> Option<u32> {
        let trace = self.open.get_mut(&id)?;
        if trace.spans.len() >= SPAN_LIMIT {
            return None;
        }
        let step = trace.spans.len() as u32 + 1;
        let after = trace.started.elapsed();
        trace.spans.push(Span {
            step,
            kind,
            from,
            to,
            detail: detail.into(),
            after,
        });
        Some(step)
    }

    /// One trace, when it is still in the window.
    pub fn get(&self, id: TraceId) -> Option<&Trace> {
        self.open.get(&id)
    }

    /// The most recently started trace.
    pub fn latest(&self) -> Option<&Trace> {
        self.latest.and_then(|id| self.open.get(&id))
    }

    /// The most recently started trace's id.
    pub fn latest_id(&self) -> Option<TraceId> {
        self.latest
    }

    /// Every retained trace, oldest first.
    pub fn all(&self) -> impl Iterator<Item = &Trace> {
        self.open.values()
    }

    /// How many traces are retained.
    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// True when no trace has been started yet.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Render one trace as a copyable block, or `None` when it is not retained.
    ///
    /// The layout is columnar so a pasted trace stays readable in a bug report:
    /// the id, the offset from the start, the hop, the route, and the payload.
    pub fn describe(&self, id: TraceId) -> Option<String> {
        let trace = self.open.get(&id)?;
        Some(trace_block(trace))
    }

    /// Render every retained trace, oldest first.
    pub fn describe_all(&self) -> String {
        if self.open.is_empty() {
            return "no traces recorded".to_string();
        }
        self.open
            .values()
            .map(trace_block)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Drop the oldest traces once the window is full.
    fn prune(&mut self) {
        while self.open.len() > TRACE_LIMIT {
            let Some(&oldest) = self.open.keys().next() else {
                break;
            };
            self.open.remove(&oldest);
        }
    }
}

/// One trace as text, with the columns padded to the widest cell.
fn trace_block(trace: &Trace) -> String {
    let route_width = trace
        .spans
        .iter()
        .map(|span| span.route().chars().count())
        .max()
        .unwrap_or(0);
    let offset_width = trace
        .spans
        .iter()
        .map(|span| format_offset(span.after).chars().count())
        .max()
        .unwrap_or(0);
    let kind_width = trace
        .spans
        .iter()
        .map(|span| span.kind.label().len())
        .max()
        .unwrap_or(0);

    let mut out = format!(
        "trace t{}  started {}  elapsed {}\n  goal: {}\n",
        trace.id,
        humantime::format_rfc3339_seconds(trace.started_wall),
        format_offset(trace.elapsed()),
        trace.goal,
    );

    if trace.spans.is_empty() {
        out.push_str("  (no hops recorded)\n");
        return out;
    }

    for span in &trace.spans {
        let route = span.route();
        out.push_str(&format!(
            "  t{}.{:<3} {:<offset$}  {:<kind$}  {:<route$}  {}\n",
            trace.id,
            span.step,
            format_offset(span.after),
            span.kind.label(),
            route,
            span.detail,
            offset = offset_width,
            kind = kind_width,
            route = route_width,
        ));
    }
    out
}

/// A duration as a compact offset from the trace start: `0ms`, `2s 300ms`.
fn format_offset(duration: Duration) -> String {
    humantime::format_duration(duration).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_start_at_one_and_increase() {
        let mut traces = Traces::default();
        assert!(traces.is_empty());
        assert_eq!(traces.begin("first"), 1);
        assert_eq!(traces.begin("second"), 2);
        assert_eq!(traces.len(), 2);
        // The latest is the one just started.
        assert_eq!(traces.latest_id(), Some(2));
        assert_eq!(traces.latest().map(|t| t.goal.as_str()), Some("second"));
    }

    #[test]
    fn steps_number_from_one_within_each_trace() {
        let mut traces = Traces::default();
        let id = traces.begin("goal");

        let first = traces.record(id, Kind::Goal, None, Some("orchestrator".into()), "ship it");
        let second = traces.record(
            id,
            Kind::Delegation,
            Some("orchestrator".into()),
            Some("writer".into()),
            "write it",
        );

        assert_eq!(first, Some(1));
        assert_eq!(second, Some(2));
        let trace = traces.get(id).unwrap();
        assert_eq!(trace.spans.len(), 2);
        assert_eq!(trace.last_span().map(|s| s.step), Some(2));
    }

    /// A hop arriving for a trace that has fallen out of the window is dropped,
    /// not resurrected: a stale chain must not recreate a pruned record.
    #[test]
    fn a_hop_for_an_evicted_trace_is_dropped() {
        let mut traces = Traces::default();
        let old = traces.begin("old");
        for i in 0..TRACE_LIMIT {
            traces.begin(format!("filler {i}"));
        }
        assert!(traces.get(old).is_none());
        assert_eq!(
            traces.record(old, Kind::Report, None, None, "late"),
            None,
            "an evicted trace must not be recreated"
        );
    }

    #[test]
    fn the_window_keeps_the_newest_traces() {
        let mut traces = Traces::default();
        let first = traces.begin("first");
        for i in 0..TRACE_LIMIT {
            traces.begin(format!("filler {i}"));
        }
        assert_eq!(traces.len(), TRACE_LIMIT);
        assert!(traces.get(first).is_none(), "the oldest was pruned");
        assert!(traces.latest_id().is_some());
    }

    #[test]
    fn spans_are_capped_per_trace() {
        let mut traces = Traces::default();
        let id = traces.begin("goal");
        for i in 0..SPAN_LIMIT + 10 {
            traces.record(id, Kind::Delegation, None, None, format!("hop {i}"));
        }
        assert_eq!(traces.get(id).unwrap().spans.len(), SPAN_LIMIT);
        // The cap reports itself rather than silently truncating.
        assert_eq!(
            traces.record(id, Kind::Delegation, None, None, "over"),
            None
        );
    }

    /// The rendered block is the thing a user pastes into a bug report, so it
    /// must name the trace, the goal, every hop, and the route between
    /// employees.
    #[test]
    fn describe_renders_the_id_goal_and_every_hop() {
        let mut traces = Traces::default();
        let id = traces.begin("fix the renderer");
        traces.record(
            id,
            Kind::Goal,
            None,
            Some("orchestrator".into()),
            "fix the renderer",
        );
        traces.record(
            id,
            Kind::Delegation,
            Some("orchestrator".into()),
            Some("writer".into()),
            "edit chat.rs",
        );

        let text = traces.describe(id).unwrap();
        assert!(text.contains("trace t1"), "{text}");
        assert!(text.contains("goal: fix the renderer"), "{text}");
        assert!(text.contains("t1.1"), "{text}");
        assert!(text.contains("t1.2"), "{text}");
        assert!(text.contains("orchestrator → writer"), "{text}");
        assert!(text.contains("edit chat.rs"), "{text}");
    }

    /// An untraced hop (a failure raised outside any chain) still renders — it
    /// just has no route to show.
    #[test]
    fn a_span_without_a_route_still_renders() {
        let mut traces = Traces::default();
        let id = traces.begin("goal");
        traces.record(id, Kind::Failed, None, None, "provider timed out");
        let text = traces.describe(id).unwrap();
        assert!(text.contains("provider timed out"), "{text}");
        assert!(text.contains("failed"), "{text}");
    }

    #[test]
    fn an_empty_trace_says_so_and_describe_all_handles_no_traces() {
        let mut traces = Traces::default();
        assert_eq!(traces.describe_all(), "no traces recorded");

        let id = traces.begin("goal");
        let text = traces.describe(id).unwrap();
        assert!(text.contains("no hops recorded"), "{text}");
    }

    #[test]
    fn describe_all_covers_every_retained_trace() {
        let mut traces = Traces::default();
        traces.begin("one");
        traces.begin("two");
        let all = traces.describe_all();
        assert!(all.contains("trace t1"), "{all}");
        assert!(all.contains("trace t2"), "{all}");
        assert!(all.contains("goal: one"), "{all}");
        assert!(all.contains("goal: two"), "{all}");
    }

    /// The offset column is what makes a stall visible at a glance.
    #[test]
    fn offsets_render_as_compact_durations() {
        assert_eq!(format_offset(Duration::from_millis(0)), "0s");
        assert_eq!(format_offset(Duration::from_millis(300)), "300ms");
        assert_eq!(format_offset(Duration::from_secs(2)), "2s");
        let long = format_offset(Duration::from_secs(12 * 60));
        assert!(long.contains("12m"), "{long}");
    }
}
