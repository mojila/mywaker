use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tokio::sync::mpsc::Sender;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::event::EventHandler;
use crate::harness::{self, PendingDelivery};
use crate::jev::{FALLBACK_VERDICT, JevClient, answer_field_or_default, answer_text_or_default};
use crate::message::Message;
use crate::opencode::{
    AgentInfo, Event, MessageError, ModelRef, OpenCode, ProviderModel, ServerInfo, SessionInfo,
    SessionMessage, pick_provider,
};
use crate::persistence::Store;
use crate::projects::{self, Project};
use crate::task::{PauseReason, Task, TaskId, TaskStatus};
use crate::team::{EmployeeStatus, Team, move_selection};
use crate::tui::Tui;
use crate::ui;
use crate::ui::pipeline;
use crate::usage::{ProcessMetrics, Usage};
use crate::workflow::{
    self, Delegation, is_final, is_jev, is_parallel, parse_delegations, strip_directives,
};

/// Maximum number of retained status-log lines. Deliberately generous: the log
/// panel reads back more than the oldest few lines it draws, and a long session
/// of autonomous work (cron/watches) otherwise pushes failures out of reach.
const LOG_LIMIT: usize = 256;

/// One retained status-log line, tagged so the panel can render failures in the
/// error colour instead of the same dim grey as ordinary progress.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub text: String,
    pub error: bool,
}

impl LogLine {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

/// The log lines read as plain strings (`.contains`, `.strip_prefix`, …), so the
/// renderer and the tests address the text the same way whether a line is an
/// error or ordinary progress.
impl std::ops::Deref for LogLine {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

/// The graceful-shutdown overlay lingers for a few animation frames so the
/// user sees the app shut down cleanly rather than the screen vanishing.
const SHUTDOWN_TICK: Duration = Duration::from_millis(80);
const SHUTDOWN_FRAMES: u64 = 8;

/// Which screen is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Roster,
    Chat,
    Projects,
    Models,
}

/// One prompt waiting for its employee's session to be free.
///
/// The kind travels with the text so the delivery policy (and the idempotent
/// admission id) is decided where the work is queued, not guessed at flush time.
/// The `token` is minted here, once, so a re-flush of this exact queued prompt
/// re-sends the same admission id (the service deduplicates it) while the next
/// prompt — even to the same employee — gets a fresh one.
///
/// That is what makes a **resend** idempotent: the queued `Outgoing` keeps its
/// token, so a delivery that never landed is offered again under the same
/// admission id and the service cannot admit the same work twice. The token is
/// deliberately *not* reused when a silent worker is asked again — that is new
/// work, and it goes through [`Outgoing::new`] like any other.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Outgoing {
    kind: harness::PromptKind,
    text: String,
    token: String,
}

impl Outgoing {
    /// A **new** delivery: mints its own token, so it can never collide with a
    /// delivery already admitted.
    fn new(kind: harness::PromptKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            token: harness::delivery_token(),
        }
    }

    /// The instruction text, for the transcript and the tests.
    #[cfg(test)]
    fn text(&self) -> &str {
        &self.text
    }
}

/// A rendered conversation line.
#[derive(Debug, Clone)]
pub struct TranscriptLine {
    pub role: TranscriptRole,
    pub text: String,
    /// Optional trailing attribution (model / timestamp), rendered dim.
    pub meta: Option<String>,
}

impl TranscriptLine {
    fn new(role: TranscriptRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            meta: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Assistant,
    Error,
    Info,
}

/// One employee's persistent chat: the OpenCode session backing it plus the
/// transcript and in-flight streaming text. Kept around after the user leaves
/// the chat view so re-entering resumes the same session instead of starting a
/// fresh one (and losing whatever the employee was mid-way through).
#[derive(Default)]
struct Chat {
    /// The OpenCode session backing this chat, once created.
    session_id: Option<String>,
    /// True while a `create_session` request for this employee is in flight.
    creating: bool,
    transcript: Vec<TranscriptLine>,
    streaming_text: String,
    /// Live "thinking" output for this chat's current turn, accumulated per
    /// assistant message. See [`Thinking`].
    thinking: Thinking,
    /// Submitted prompts, oldest first — for Up/Down recall.
    history: Vec<String>,
    /// Current position in the history navigation, when recalling.
    hist_index: Option<usize>,
    /// Input draft saved when history navigation started (to restore on Down).
    history_draft: String,
    /// Transcapped scroll offset: how many display lines we've scrolled *up*
    /// from the live tail. `0` means "stick to the bottom" (auto-follow).
    scroll: usize,
    /// Whether the team-context preamble has been injected into this employee's
    /// first prompt, so they know their teammates and how to delegate.
    bootstrapped: bool,
    /// Assistant message ids already scanned for delegation directives, so a
    /// re-loaded transcript never re-dispatches the same hand-off.
    handled_message_ids: HashSet<String>,
    /// Delegation instructions queued until this employee's session is ready.
    outbox: Vec<Outgoing>,
    /// The delegated hand-off currently in flight for this employee, tracked so
    /// a silent worker can be retried or abandoned on a deadline.
    pending: Option<PendingDelivery>,
    /// User prompts submitted while the employee was mid-turn (or before their
    /// session was ready), drained one per completed turn by
    /// [`App::pump_employee_queue`].
    queue_running: Vec<String>,
}

impl Chat {
    /// True when this chat's session exists and can take a prompt — false while
    /// one is still being created.
    fn ready(&self) -> bool {
        self.session_id.is_some() && !self.creating
    }

    /// True when a session exists or is already being created, which is exactly
    /// when [`App::begin_session`] has nothing left to do.
    fn started_or_starting(&self) -> bool {
        self.session_id.is_some() || self.creating
    }
}

/// The resume picker for one employee: the previous sessions found for their
/// agent, plus which row the cursor is on. Row `0` is always "start fresh"; the
/// rows after it are the previous sessions, newest first.
struct SessionPicker {
    employee: usize,
    sessions: Vec<SessionInfo>,
    selected: usize,
}

impl SessionPicker {
    /// The highlighted session, if the cursor is on one (not on "start fresh").
    fn selected_session(&self) -> Option<&SessionInfo> {
        self.sessions.get(self.selected.checked_sub(1)?)
    }

    /// Move the cursor by `delta`, clamped to the rows that exist. The first
    /// row is "start fresh", so the range is `0..=sessions.len()`.
    fn move_selection(&mut self, delta: isize) {
        let last = self.sessions.len();
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, last as isize) as usize;
    }
}

/// A parallel fan-out: one goal broadcast to several workers, whose outputs are
/// collected and handed to an aggregator once they all finish.
#[derive(Debug)]
struct FanOut {
    /// Worker employees fanned out to (excluding the source and the aggregator).
    workers: Vec<usize>,
    /// The employee that synthesizes the workers' outputs into one deliverable.
    aggregator: usize,
    /// The goal broadcast to each worker.
    goal: String,
    /// Per-worker deliverable text, filled in as each worker completes.
    outputs: HashMap<usize, String>,
    /// Set once aggregation has been dispatched, to avoid double-firing.
    aggregated: bool,
}

/// One chat slash command. Both surfaces that explain the commands read this
/// table — the completion menu and `/help` — so the two cannot drift apart.
#[derive(Debug, Clone, Copy)]
pub struct CommandOption {
    /// The command as typed, e.g. `/skill add`.
    pub command: &'static str,
    /// The argument it takes, e.g. `<source>` (empty when it takes none). The
    /// completion menu leaves a trailing space for it; `/help` writes it out.
    pub args: &'static str,
    pub description: &'static str,
    /// Whether the completion menu offers this command as you type. A command
    /// whose whole point is a free-text argument is listed by `/help` but left
    /// out of the menu, which would only get in the way of typing the goal.
    pub menu: bool,
}

impl CommandOption {
    /// The command with its argument hint, as `/help` writes it: `/skill add
    /// <source>`.
    fn usage(&self) -> String {
        if self.args.is_empty() {
            self.command.to_string()
        } else {
            format!("{} {}", self.command, self.args)
        }
    }
}

/// The chat's slash commands, in display order.
const CHAT_COMMANDS: &[CommandOption] = &[
    CommandOption {
        command: "/help",
        args: "",
        description: "show chat commands",
        menu: true,
    },
    CommandOption {
        command: "/stop",
        args: "",
        description: "interrupt the current run",
        menu: true,
    },
    CommandOption {
        command: "/new",
        args: "",
        description: "start a fresh session",
        menu: true,
    },
    CommandOption {
        command: "/clear",
        args: "",
        description: "clear this conversation",
        menu: true,
    },
    CommandOption {
        command: "/skills",
        args: "",
        description: "list installed skills",
        menu: true,
    },
    CommandOption {
        command: "/skill add",
        args: "<source>",
        description: "install & attach a skill",
        menu: true,
    },
    CommandOption {
        command: "/skill",
        args: "<name>",
        description: "attach an installed skill",
        menu: true,
    },
    CommandOption {
        command: "/parallel",
        args: "<goal>",
        description: "fan the goal out to the whole team",
        menu: false,
    },
    CommandOption {
        command: "/trace",
        args: "[all]",
        description: "show the orchestration trace",
        menu: true,
    },
    CommandOption {
        command: "/copy",
        args: "[trace|all]",
        description: "copy the error log (or a trace) to the clipboard",
        menu: true,
    },
];

/// The `/help` body: every command in the table, each with the argument it takes
/// and the description the completion menu shows, padded into two columns.
fn chat_help_text() -> String {
    let width = CHAT_COMMANDS
        .iter()
        .map(|command| command.usage().len())
        .max()
        .unwrap_or(0);
    let mut text = String::from("chat commands:");
    for command in CHAT_COMMANDS {
        let usage = command.usage();
        text.push_str(&format!("\n  {usage:width$}  {}", command.description));
    }
    text
}

/// The reopen prompt: the tasks mywaker paused on the way out, offered back as
/// one batch on the next launch, in the task panel's order (`order_tasks`).
///
/// Only tasks paused *at shutdown* are ever collected here — a task the user
/// paused deliberately is not, so a deliberate pause can't leak into
/// resume-all.
#[derive(Debug, Clone)]
pub struct ResumePrompt {
    tasks: Vec<TaskId>,
}

impl ResumePrompt {
    /// How many tasks the prompt covers.
    pub fn count(&self) -> usize {
        self.tasks.len()
    }
}

/// A rolling window of per-tick event counts, for the sidebar's activity
/// sparkline.
///
/// One sample is pushed per [`Message::Tick`], holding the number of OpenCode
/// events that arrived since the previous tick. The buffer is capped at
/// [`Pulse::CAPACITY`] samples and is deliberately cheap: it is a display-only
/// trend, never persisted and never read by dispatch.
#[derive(Debug, Default)]
pub struct Pulse {
    samples: Vec<u64>,
    /// The event count as of the previous tick, so a tick can record the delta.
    last_seen: u64,
}

impl Pulse {
    /// How many tick samples the window keeps.
    pub const CAPACITY: usize = 48;

    /// Record one tick: push the delta since the previous tick (zero when no
    /// events arrived) and drop the oldest sample once the window is full.
    pub fn tick(&mut self, event_count: u64) {
        let delta = event_count.saturating_sub(self.last_seen);
        self.last_seen = event_count;
        self.record(delta);
    }

    /// Append one sample directly, without a tick baseline — the seam the
    /// containment test uses to change the window and nothing else.
    pub fn record(&mut self, sample: u64) {
        self.samples.push(sample);
        if self.samples.len() > Self::CAPACITY {
            let excess = self.samples.len() - Self::CAPACITY;
            self.samples.drain(..excess);
        }
    }

    /// The samples, oldest first (at most [`Self::CAPACITY`]).
    pub fn samples(&self) -> &[u64] {
        &self.samples
    }

    /// Forget the window and the tick baseline.
    pub fn reset(&mut self) {
        self.samples.clear();
        self.last_seen = 0;
    }
}

/// A roll-up of the persisted task queue, for the sidebar's vitals lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskStats {
    /// Queued but not started.
    pub pending: usize,
    /// Currently running.
    pub running: usize,
    /// Running again after a failure.
    pub retrying: usize,
    /// Held (user or shutdown pause).
    pub paused: usize,
    /// Finished successfully.
    pub succeeded: usize,
    /// Finished in failure.
    pub failed: usize,
}

impl TaskStats {
    /// Work still in flight — pending, running, retrying, or paused.
    pub fn active(&self) -> usize {
        self.pending + self.running + self.retrying + self.paused
    }

    /// Work that has reached a terminal state.
    pub fn finished(&self) -> usize {
        self.succeeded + self.failed
    }
}

/// What a hand-off did, for the delegation wires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceKind {
    /// Work going out: one employee handing a piece of the goal to another.
    Out,
    /// Work coming back: a round's contributions arriving at its aggregator.
    Report,
    /// A fan-out brief: one goal broadcast to the whole team at once.
    Fanout,
}

impl TraceKind {
    /// The trace's colour: cyan out, green back, yellow for a broadcast — the
    /// same reading as the roster's status colours.
    pub fn color(self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            TraceKind::Out => Color::Cyan,
            TraceKind::Report => Color::Green,
            TraceKind::Fanout => Color::Yellow,
        }
    }
}

/// One hand-off, drawn as a packet travelling the wires between two employees.
///
/// A trace is a *display* record: it is created when a hand-off is dispatched and
/// forgotten a moment later (`App::tick_traces`), so the UI can animate work
/// moving between employees without keeping a history nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegationTrace {
    /// Roster index of the employee the work left.
    pub from: usize,
    /// Roster index of the employee it went to.
    pub to: usize,
    pub kind: TraceKind,
    /// The frame the trace was recorded on, so its age is a pure function of the
    /// app's frame counter — the UI never reads a clock.
    pub started: u64,
}

impl DelegationTrace {
    /// How many ticks a trace lives. At the default 100 ms tick this is ~1.2 s,
    /// long enough to read a packet cross the panel and short enough that a
    /// finished chain leaves no clutter.
    pub const TTL: u64 = 12;
    /// How far along its journey the packet is, as `0.0..=1.0`, at `frame`.
    ///
    /// Pure: same trace and frame, same answer, which is what lets the renderer
    /// be a function of the app state.
    pub fn progress(&self, frame: u64) -> f32 {
        let age = frame.saturating_sub(self.started);
        (age.min(Self::TTL) as f32) / (Self::TTL as f32)
    }

    /// True once the packet has run its course and can be forgotten.
    pub fn is_expired(&self, frame: u64) -> bool {
        frame.saturating_sub(self.started) >= Self::TTL
    }
}

/// Live "thinking" output for one turn, accumulated as the stream arrives.
///
/// Reasoning arrives as a *delta stream*, one token fragment at a time, and the
/// chat shows only the tail of it. Keeping the fragments in a `Vec` (rather than
/// appending to one `String`) is what makes that cheap: an append is `O(1)` and
/// the visible tail is a bounded walk backwards, so a long deliberation does not
/// turn every delta into a full re-copy of the text so far.
///
/// Fragments are keyed by the assistant message they belong to
/// (`assistantMessageID`), so a turn that reasons, answers, and then reasons again
/// (a tool round-trip) starts a fresh buffer instead of gluing unrelated
/// thinking together.
#[derive(Debug, Default, Clone)]
pub struct Thinking {
    /// The assistant message the fragments belong to, when the stream names one.
    key: Option<String>,
    /// The fragments, oldest first.
    fragments: Vec<String>,
}

impl Thinking {
    /// How many fragments the buffer keeps. A long turn can emit thousands; the
    /// chat only ever shows the tail, so older ones are dropped rather than
    /// growing the buffer without bound.
    pub const CAPACITY: usize = 512;

    /// How many display columns of thinking the tail renderer reads back through.
    pub const TAIL_BUDGET: usize = 240;

    /// Start (or continue) the buffer for assistant message `key`.
    ///
    /// A different key means a different message, so the buffer restarts: the new
    /// message's thinking is not a continuation of the old one's.
    pub fn begin(&mut self, key: Option<String>) {
        if self.key != key {
            self.key = key;
            self.fragments.clear();
        }
    }

    /// Append one fragment to the current message's buffer.
    pub fn push(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.fragments.push(delta.to_string());
        if self.fragments.len() > Self::CAPACITY {
            let excess = self.fragments.len() - Self::CAPACITY;
            self.fragments.drain(..excess);
        }
    }

    /// True when nothing has been thought yet this turn.
    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    /// The tail of the thinking, at most [`Self::TAIL_BUDGET`] columns, read back
    /// from the newest fragment.
    ///
    /// Walks the fragments in reverse and stops as soon as it has enough columns,
    /// so the cost is bounded by the budget rather than by the length of the
    /// deliberation. Whitespace is collapsed, because reasoning arrives with the
    /// provider's own line breaks and the chat shows one line.
    pub fn tail(&self) -> String {
        let mut out: Vec<&str> = Vec::new();
        let mut used = 0usize;
        for fragment in self.fragments.iter().rev() {
            if used >= Self::TAIL_BUDGET {
                break;
            }
            used += fragment.chars().count();
            out.push(fragment.as_str());
        }
        out.reverse();
        let joined = out.concat();
        let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
        // The last fragment read can overshoot the budget on its own, so the
        // assembled line is trimmed to it: the bound is the promise, not an
        // approximation of it.
        if collapsed.chars().count() <= Self::TAIL_BUDGET {
            collapsed
        } else {
            let skip = collapsed.chars().count() - Self::TAIL_BUDGET;
            collapsed.chars().skip(skip).collect()
        }
    }

    /// The whole buffer, joined — for the `full` thinking mode and for tests.
    pub fn text(&self) -> String {
        self.fragments.concat()
    }

    /// Forget the buffer and its key.
    pub fn clear(&mut self) {
        self.key = None;
        self.fragments.clear();
    }
}

pub struct App {
    config: Config,
    tx: Sender<Message>,
    team: Team,
    selected: usize,
    client: Option<OpenCode>,
    server_info: Option<ServerInfo>,
    agents: Vec<AgentInfo>,
    connection_error: Option<String>,
    event_count: u64,
    last_event: Option<String>,
    log: Vec<LogLine>,
    frame: u64,
    /// True once quit is requested: the app renders a brief "shutting down"
    /// overlay (with a spinner), flushes state, then exits.
    shutting_down: bool,
    /// Animation frame for the shutdown overlay.
    shutdown_frame: u64,

    /// Projects discovered under the projects root (refreshed on entry).
    projects: Vec<Project>,
    /// The directories scanned for projects (refreshed alongside `projects`).
    project_roots: Vec<PathBuf>,
    /// The project currently selected as the session working directory.
    active_project: Option<Project>,
    /// Selection index in the project-picker list.
    project_selected: usize,
    /// Models available through OpenCode's providers, for the model picker.
    models: Vec<ProviderModel>,
    /// MCP servers and their connection status, shown in the sidebar's runtime
    /// row. Loaded at connect (and on reconnect), never refreshed mid-run.
    mcp_servers: Vec<crate::opencode::McpServer>,
    /// Selection index in the model picker list (indexed into
    /// [`Self::filtered_models`]).
    model_selected: usize,
    /// Search query filtering the model picker list.
    model_filter: String,

    mode: Mode,
    chat_employee: Option<usize>,
    input: String,
    /// Cursor position within `input`, in grapheme clusters (0..=len).
    input_cursor: usize,
    /// Horizontal scroll offset for the input line, in display columns: how far
    /// the input body has been scrolled right to keep the cursor in view when
    /// the prompt overflows the terminal width.
    input_scroll: usize,
    /// Whether the roster's prompt bar has the keyboard. While it does, keys are
    /// text (or line editing) rather than the roster's single-letter shortcuts;
    /// see [`Self::handle_roster_key`].
    prompt_focused: bool,
    /// Goals submitted from the roster prompt, oldest first — for Up/Down recall.
    prompt_history: Vec<String>,
    /// Current position in the prompt history navigation, when recalling.
    prompt_hist_index: Option<usize>,
    /// Prompt draft saved when history navigation started (restored on Down).
    prompt_hist_draft: String,
    /// Last known terminal size, for page-scroll estimation.
    last_size: (u16, u16),
    /// First card row shown in the roster grid. The wheel scrolls it and a
    /// selection move reveals the selected card; [`Self::roster_scroll`] clamps
    /// it against the current terminal size and roster length, so the stored
    /// value is only ever a hint after a resize or a smaller team.
    roster_scroll: usize,
    /// Per-employee chat state, keyed by roster index.
    chats: HashMap<usize, Chat>,
    /// Selected index in the slash-command autocomplete menu, when it is open
    /// (i.e. the input starts with `/` and matches one or more commands).
    command_menu: Option<usize>,
    /// The resume picker: an employee's previous sessions awaiting the user's
    /// continue-or-fresh decision, plus the highlighted row. Set by
    /// `start_chat` when the employee has no live session yet.
    session_picker: Option<SessionPicker>,
    /// Total hand-offs dispatched this run, guarding against runaway cycles.
    delegation_count: u64,
    /// Per-employee delegation-chain origin: the chat `@user`/`@deliver` should
    /// deliver into. Replaces a single global so concurrent chains don't clobber.
    origin_of: HashMap<usize, usize>,
    /// Per-employee delegation depth, bounding `A → B → A` chains.
    depth_of: HashMap<usize, usize>,
    /// Jev (System One) decision client, used to answer `@jev` questions and
    /// OpenCode `form.created` prompts. `None` when no Kenari key is set.
    jev: Option<JevClient>,
    /// Active parallel fan-outs, keyed by a monotonic id.
    fanouts: HashMap<u64, FanOut>,
    /// Monotonic id source for parallel fan-outs.
    next_fanout_id: u64,
    /// Aggregated LLM token usage across sessions.
    usage: Usage,
    /// Process resource metrics (RSS + CPU) for the telemetry readout.
    metrics: ProcessMetrics,
    /// Persisted scheduled-task history + per-employee memory.
    store: Store,
    /// Where the store persists (`None` disables persistence, e.g. in tests).
    store_path: Option<PathBuf>,
    /// Whether the full-screen help overlay is showing.
    show_help: bool,
    /// The reopen prompt: set at startup when mywaker itself paused tasks on the
    /// way out. `None` — the normal case — means nothing is offered.
    resume_prompt: Option<ResumePrompt>,
    /// When this run started, for the sidebar's uptime readout.
    started_at: Instant,
    /// Per-tick event counts, for the sidebar's activity sparkline.
    pulse: Pulse,
    /// Recent hand-offs, for the delegation wires. Capped at
    /// [`App::TRACE_LIMIT`] and pruned by [`App::tick_traces`].
    traces: Vec<DelegationTrace>,
    /// Orchestration traces: one per goal, carrying every hop it caused. This is
    /// the *diagnostic* record (ids, offsets, routes) rather than the wires'
    /// animation state, so a stuck chain can be named and pasted into a report.
    orchestration: crate::trace::Traces,
    /// The trace the current user-initiated work belongs to, when there is one.
    /// A hop is recorded under this id so unrelated background traffic does not
    /// attach itself to the goal the user is watching.
    active_trace: Option<crate::trace::TraceId>,
    /// A clipboard escape sequence waiting to be written to the terminal. The
    /// renderer owns stdout, so a copy queues the bytes here and [`App::draw`]
    /// flushes them right after the frame — writing mid-frame would race
    /// ratatui's own cursor positioning.
    pending_clipboard: Option<String>,
}

impl App {
    pub fn new(config: Config, tx: Sender<Message>) -> Self {
        Self::with_team(config, tx, Team::starter())
    }

    /// How many hand-off traces are kept at once. A long chain can dispatch
    /// several per frame; the window only has to cover what the wires draw, so
    /// the oldest are dropped rather than the list growing without bound.
    pub const TRACE_LIMIT: usize = 12;
    pub fn with_team(config: Config, tx: Sender<Message>, team: Team) -> Self {
        let mut app = Self {
            config,
            tx,
            team,
            selected: 0,
            client: None,
            server_info: None,
            agents: Vec::new(),
            connection_error: None,
            event_count: 0,
            last_event: None,
            log: vec![LogLine::info("connecting to OpenCode…")],
            frame: 0,
            shutting_down: false,
            shutdown_frame: 0,
            projects: Vec::new(),
            project_roots: Vec::new(),
            active_project: None,
            project_selected: 0,
            models: Vec::new(),
            mcp_servers: Vec::new(),
            model_selected: 0,
            model_filter: String::new(),
            mode: Mode::Roster,
            chat_employee: None,
            input: String::new(),
            input_cursor: 0,
            input_scroll: 0,
            prompt_focused: false,
            prompt_history: Vec::new(),
            prompt_hist_index: None,
            prompt_hist_draft: String::new(),
            last_size: (80, 24),
            roster_scroll: 0,
            chats: HashMap::new(),
            command_menu: None,
            session_picker: None,
            delegation_count: 0,
            origin_of: HashMap::new(),
            depth_of: HashMap::new(),
            jev: None,
            fanouts: HashMap::new(),
            next_fanout_id: 0,
            usage: Usage::default(),
            metrics: ProcessMetrics::new(),
            store: Store::default(),
            store_path: None,
            show_help: false,
            resume_prompt: None,
            started_at: Instant::now(),
            pulse: Pulse::default(),
            traces: Vec::new(),
            orchestration: crate::trace::Traces::default(),
            active_trace: None,
            pending_clipboard: None,
        };
        // Start on the first card in workflow order, so the prompt bar names an
        // employee the grid actually draws.
        app.selected = app.card_order().first().copied().unwrap_or(0);
        app
    }

    /// Attach the Jev (System One) decision client, used to answer `@jev`
    /// questions and OpenCode `form.created` prompts.
    pub fn set_jev(&mut self, jev: Option<JevClient>) {
        self.jev = jev;
    }

    /// Enable task/memory persistence at the default store path, loading any
    /// previously saved tasks. Call once after [`Self::with_team`].
    ///
    /// Loading is also where the reopen prompt is prepared: if the last run
    /// paused tasks on its way out, they are offered back now.
    pub fn enable_persistence(&mut self) {
        self.store_path = Store::default_path();
        if let Some(path) = &self.store_path {
            self.store = Store::load(path).unwrap_or_default();
        }
        self.prepare_resume_prompt();
    }

    pub async fn run(&mut self, tui: &mut Tui, mut events: EventHandler) -> Result<()> {
        loop {
            if self.shutting_down {
                // Graceful shutdown: animate the overlay for a few frames so
                // the terminal stays put while state is flushed, then return.
                self.shutdown_frame = self.shutdown_frame.wrapping_add(1);
                self.draw(tui)?;
                if self.shutdown_frame >= SHUTDOWN_FRAMES {
                    break;
                }
                tokio::time::sleep(SHUTDOWN_TICK).await;
                continue;
            }

            self.draw(tui)?;

            let message = events.next().await?;
            self.update(message);
        }

        Ok(())
    }

    /// Draw one frame, recording the size it was drawn at — mouse hit-testing
    /// then resolves clicks against the frame that is actually on screen.
    fn draw(&mut self, tui: &mut Tui) -> Result<()> {
        let mut size = self.last_size;
        tui.draw(|frame| {
            size = (frame.area().width, frame.area().height);
            ui::render(frame, self);
        })?;
        self.last_size = size;
        // A copy requested during the last update hands its escape sequence to
        // the terminal now, after the frame: the terminal must see the sequence
        // outside ratatui's own cursor work, or the screen is left mispositioned.
        if let Some(sequence) = self.pending_clipboard.take() {
            use std::io::Write;
            let mut out = std::io::stdout();
            let _ = out.write_all(sequence.as_bytes());
            let _ = out.flush();
        }
        Ok(())
    }

    /// Begin graceful shutdown: flush state, stop in-flight work, and let the run
    /// loop animate the "shutting down" overlay before relinquishing the terminal.
    ///
    /// The task queue is held *before* the store is flushed, so the pause is on
    /// disk even though release builds abort on panic: a half-finished task is
    /// then offered back on the next launch instead of being lost.
    fn initiate_shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.shutdown_frame = 0;
        // Hold the task queue (interrupting the sessions it was running in), stop
        // the current chat generation (best effort), then force a final write.
        self.pause_active_tasks();
        self.interrupt_current();
        self.persist_store();
    }

    /// Process one message: the `Update` half of TEA.
    ///
    /// Public so integration tests can drive the real input paths
    /// (`Message::Key`, `Message::Mouse`, `Message::Resize`) instead of
    /// reimplementing what a key or a click does.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick => {
                self.frame = self.frame.wrapping_add(1);
                self.pulse.tick(self.event_count);
                self.tick_reliability();
                self.tick_traces();
                self.metrics.refresh();
            }
            Message::Resize(w, h) => {
                self.last_size = (w, h);
                self.follow_input_cursor();
                // A shorter (or taller) panel changes how many rows fit, so the
                // stored offset is normalised now rather than left stale for
                // every later reader to clamp.
                let clamped = self.roster_scroll();
                self.roster_scroll = clamped;
            }
            Message::Key(key) => self.handle_key(key),
            Message::Mouse(mouse) => self.handle_mouse(mouse),
            Message::Paste(text) => self.handle_paste(&text),
            Message::Connected { client, info } => {
                let version = info.version.clone();
                self.client = Some(client);
                self.server_info = Some(info);
                self.push_log(format!("connected to OpenCode v{version}"));
                if self.config.prewarm_sessions {
                    self.prewarm_sessions();
                }
            }
            Message::ConnectionFailed(reason) => {
                self.connection_error = Some(reason.clone());
                self.push_error_log(format!("OpenCode unavailable: {reason}"));
            }
            Message::AgentsLoaded(agents) => {
                self.push_log(format!("{} agents available", agents.len()));
                self.agents = agents;
            }
            Message::OpenCodeEvent(event) => {
                self.event_count += 1;
                self.last_event = Some(event.event_type.clone());
                self.handle_stream_event(&event);
            }
            Message::PreviousSessionsLoaded { employee, sessions } => {
                self.open_session_picker(employee, sessions);
            }
            Message::SessionCreated { employee, result } => {
                self.handle_session_created(employee, result);
            }
            Message::TranscriptLoaded { employee, messages } => {
                self.handle_transcript(employee, messages);
            }
            Message::DeliveryFailed { target, reason } => {
                self.handle_delivery_failed(target, reason);
            }
            Message::SkillInstallDone { employee, result } => {
                self.handle_skill_install_done(employee, result);
            }
            Message::McpLoaded(servers) => {
                let connected = servers.iter().filter(|s| s.is_connected()).count();
                self.push_log(format!(
                    "{connected}/{0} mcp servers connected",
                    servers.len()
                ));
                self.mcp_servers = servers;
            }
            Message::ModelsLoaded(models) => {
                self.push_log(format!("{} models available", models.len()));
                self.models = models;
                self.backfill_model_providers();
                self.model_selected = self
                    .model_selected
                    .min(self.filtered_models().len().saturating_sub(1));
            }
            Message::JevAnswer { employee, answer } => {
                self.push_to_chat(employee, TranscriptRole::Assistant, answer);
            }
            Message::ScheduledTask { employee, goal } => {
                self.handle_autonomous_task("⌛ scheduled task", employee, goal);
            }
            Message::WatchedTask { employee, goal } => {
                self.handle_autonomous_task("👁 watched task", employee, goal);
            }
            Message::TaskAttemptFinished {
                task_id,
                succeeded,
                result,
            } => {
                self.handle_task_attempt(task_id, succeeded, result);
            }
            Message::TaskAttemptStarted {
                task_id,
                session_id,
            } => {
                self.note_task_attempt(task_id, session_id);
            }
            Message::Log(line) => self.push_log(line),
            Message::LogError(line) => self.push_error_log(line),
            Message::Quit => self.initiate_shutdown(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // The resume picker is drawn above the chat, so it takes the key before
        // anything else when open.
        if self.session_picker.is_some() {
            self.handle_session_picker_key(key);
            return;
        }

        // The reopen prompt is drawn above everything, so it takes the key
        // before the help overlay's dismiss and the global `q`/`Ctrl-C` quit
        // bindings.
        if self.resume_prompt.is_some() {
            self.handle_resume_prompt_key(key);
            return;
        }

        // The help overlay swallows the next key (dismiss) before anything else.
        if self.show_help {
            self.show_help = false;
            return;
        }

        // Ctrl-C quits from anywhere except chat, where it interrupts the
        // running generation instead.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.mode == Mode::Chat {
                self.handle_chat_key(key);
                return;
            }
            self.initiate_shutdown();
            return;
        }

        // `?` opens the help overlay from any non-chat screen.
        if key.code == KeyCode::Char('?') && self.mode != Mode::Chat {
            self.show_help = true;
            return;
        }

        // `y` copies the error log from any screen: the log band is drawn on the
        // roster too, so the failure a user is looking at must be copyable
        // without first opening a chat. (`Ctrl-Y` is free here — `Ctrl-Y` is not
        // bound to anything else, and a bare `y` would collide with the roster's
        // single-letter shortcuts only if it were one, which it is not.)
        if key.code == KeyCode::Char('y') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let text = self.error_log_text();
            let note = self.copy_to_clipboard("error-log", &text);
            self.push_log(note);
            return;
        }

        match self.mode {
            Mode::Roster => {
                // A focused prompt bar owns the keyboard: `q`, `s`, `p` … are text
                // there, not shortcuts. `Esc` (and `Tab`/`i`) hand the roster back.
                if self.prompt_focused {
                    self.handle_prompt_key(key);
                } else {
                    self.handle_roster_key(key);
                }
            }
            Mode::Projects => match key.code {
                KeyCode::Esc => self.mode = Mode::Roster,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.project_selected =
                        move_selection(self.project_selected, -1, self.projects.len());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.project_selected =
                        move_selection(self.project_selected, 1, self.projects.len());
                }
                KeyCode::Enter => self.select_project(),
                _ => {}
            },
            Mode::Models => match key.code {
                KeyCode::Esc => {
                    self.model_filter.clear();
                    self.mode = Mode::Roster;
                }
                // `k`/`j` mirror `↑`/`↓`, matching the project picker
                // (`docs/keymap.md` §3.8 K1).
                KeyCode::Up | KeyCode::Char('k') => {
                    self.model_selected =
                        move_selection(self.model_selected, -1, self.filtered_models().len());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.model_selected =
                        move_selection(self.model_selected, 1, self.filtered_models().len());
                }
                KeyCode::Enter => self.assign_selected_model(),
                KeyCode::Backspace => self.edit_model_filter(None),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.model_filter.clear();
                    self.model_selected = 0;
                }
                // Some terminals send backspace as `^H`, which crossterm reports
                // as `Char('h')` + CONTROL; delete with it, never type an 'h'.
                KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.edit_model_filter(None)
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.edit_model_filter(Some(c))
                }
                _ => {}
            },
            Mode::Chat => self.handle_chat_key(key),
        }
    }

    /// Handle a key on the roster screen while the prompt bar is **not** focused:
    /// the single-letter shortcuts, which stay exactly as they were.
    fn handle_roster_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.initiate_shutdown(),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-self.roster_cols()),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(self.roster_cols()),
            KeyCode::Left | KeyCode::Char('h') => self.move_selection(-1),
            KeyCode::Right | KeyCode::Char('l') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection_grid(-self.roster_page()),
            KeyCode::PageDown => self.move_selection_grid(self.roster_page()),
            KeyCode::Enter => self.start_chat(self.selected),
            // `i` and `Tab` both drop into the prompt: one is the plan's key, the
            // other the conventional "next field".
            KeyCode::Char('i') | KeyCode::Tab => self.focus_prompt(),
            KeyCode::Char('p') | KeyCode::Char('P') => self.open_projects(),
            KeyCode::Char('m') | KeyCode::Char('M') => self.open_models(),
            KeyCode::Char(' ') => self.toggle_engine_pause(),
            KeyCode::Char('s') => self.toggle_task_pause(),
            _ => {}
        }
    }

    /// Handle a key while the prompt bar is focused: line editing, history, and
    /// `Enter` to submit. `Esc` blurs; every other key edits the buffer.
    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            KeyCode::Esc => self.blur_prompt(),
            KeyCode::Enter if shift => self.input_insert('\n'),
            KeyCode::Enter => self.submit_goal(),
            KeyCode::Up => self.prompt_history_up(),
            KeyCode::Down => self.prompt_history_down(),
            KeyCode::Left => self.cursor_left(),
            KeyCode::Right => self.cursor_right(),
            KeyCode::Home => self.cursor_home(),
            KeyCode::End => self.cursor_end(),
            KeyCode::Delete => self.input_delete_forward(),
            KeyCode::Char('w') if ctrl => self.input_delete_word(),
            KeyCode::Char('u') if ctrl => self.input_delete_to_start(),
            KeyCode::Char('a') if ctrl => self.cursor_home(),
            KeyCode::Char('e') if ctrl => self.cursor_end(),
            // As in chat: `^H` is backspace on terminals that don't emit DEL.
            KeyCode::Char('h') if ctrl => self.input_delete_backward(),
            KeyCode::Backspace => self.input_delete_backward(),
            KeyCode::Tab => self.blur_prompt(),
            KeyCode::Char(c) if !ctrl => self.input_insert(c),
            _ => {}
        }
    }

    /// Handle a key while the reopen prompt is showing: resume the held tasks,
    /// keep them held, or quit leaving them held.
    ///
    /// Every other key is swallowed, so a stray press can't act on the screen
    /// underneath a modal the user hasn't answered yet.
    fn handle_resume_prompt_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                self.resume_shutdown_paused();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.dismiss_resume_prompt();
            }
            // Quitting from the prompt leaves the tasks exactly as they are —
            // still shutdown-paused, so the prompt comes back next launch.
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.dismiss_resume_prompt();
                self.initiate_shutdown();
            }
            KeyCode::Char('c') if ctrl => {
                self.dismiss_resume_prompt();
                self.initiate_shutdown();
            }
            _ => {}
        }
    }

    /// Handle a mouse event: a click moves the selection to whatever it landed on
    /// (a roster card, a picker row), and a wheel notch scrolls the active
    /// screen the way its paging keys do. Activation stays on Enter — a stray
    /// click never adopts a project or reassigns a model.
    fn handle_mouse(&mut self, mouse: MouseEvent) {
        let kind = mouse.kind;
        // Only clicks and wheel notches are input; motion is ignored (and must
        // not dismiss an overlay the user is reading).
        if !matches!(
            kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
        ) {
            return;
        }
        // The reopen prompt swallows input until it is answered, so a click
        // can't move a selection underneath it.
        if self.resume_prompt.is_some() {
            return;
        }
        // The shutdown overlay swallows everything; the help overlay is modal, so
        // any input dismisses it — as any key does — without touching what it
        // covers.
        if self.shutting_down {
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }

        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match ui::hit::target_at(self, self.last_size, mouse.column, mouse.row) {
                    Some(ui::hit::Target::Employee(index)) => {
                        self.selected = index;
                        // A click on a card takes the keyboard back from the
                        // prompt: the user pointed at the grid.
                        self.blur_prompt();
                    }
                    Some(ui::hit::Target::Prompt) => self.focus_prompt(),
                    Some(ui::hit::Target::Project(index)) => self.project_selected = index,
                    Some(ui::hit::Target::Model(index)) => self.model_selected = index,
                    None => {}
                }
            }
            MouseEventKind::ScrollUp => self.step_screen(-1),
            MouseEventKind::ScrollDown => self.step_screen(1),
            _ => {}
        }
    }

    /// Step the active screen by one row: a wheel notch, mirroring the arrow keys.
    ///
    /// The roster is the exception: its grid is a viewport, so a notch scrolls
    /// the view rather than moving the selection (which the arrows own). A user
    /// who scrolls to look around never changes what `Enter` would open.
    fn step_screen(&mut self, delta: isize) {
        match self.mode {
            Mode::Roster => self.scroll_roster(delta),
            Mode::Chat => self.scroll_transcript(delta),
            Mode::Projects => {
                self.project_selected =
                    move_selection(self.project_selected, delta, self.projects.len());
            }
            Mode::Models => {
                self.model_selected =
                    move_selection(self.model_selected, delta, self.filtered_models().len());
            }
        }
    }

    /// Handle a key press while in chat: slash-command menu navigation first,
    /// then ordinary input editing (cursor movement, history, multi-line).
    fn handle_chat_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let menu_open = self.command_menu.is_some();

        match key.code {
            KeyCode::Esc => {
                if menu_open {
                    // Dismiss the menu but keep the typed text intact.
                    self.command_menu = None;
                } else {
                    self.close_chat();
                }
            }
            // Ctrl-C interrupts the running generation rather than quitting.
            KeyCode::Char('c') if ctrl => self.interrupt_current(),
            // Ctrl-W / Ctrl-U are line-editing (delete word / to start of line).
            KeyCode::Char('w') if ctrl => self.input_delete_word(),
            KeyCode::Char('u') if ctrl => self.input_delete_to_start(),
            KeyCode::Char('a') if ctrl => self.cursor_home(),
            KeyCode::Char('e') if ctrl => self.cursor_end(),
            // Some terminals send backspace as `^H` (`\x08`), which crossterm
            // reports as `Char('h')` + CONTROL rather than `Backspace`. Treat it
            // as backspace so it never leaks a stray 'h' into the input.
            KeyCode::Char('h') if ctrl => self.input_delete_backward(),
            KeyCode::Up => {
                if menu_open {
                    self.move_command_menu(-1);
                } else {
                    self.history_up();
                }
            }
            KeyCode::Down => {
                if menu_open {
                    self.move_command_menu(1);
                } else {
                    self.history_down();
                }
            }
            KeyCode::PageUp => self.scroll_transcript(-1),
            KeyCode::PageDown => self.scroll_transcript(1),
            KeyCode::Left => self.cursor_left(),
            KeyCode::Right => self.cursor_right(),
            KeyCode::Home => self.cursor_home(),
            KeyCode::End => self.cursor_end(),
            KeyCode::Delete => self.input_delete_forward(),
            KeyCode::Backspace => {
                self.input_delete_backward();
                self.sync_command_menu();
            }
            KeyCode::Enter => {
                if shift {
                    self.input_insert('\n');
                    self.sync_command_menu();
                } else if menu_open {
                    self.complete_command();
                } else {
                    self.send_prompt();
                }
            }
            KeyCode::Tab => {
                if menu_open {
                    self.complete_command();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                self.input_insert(c);
                self.sync_command_menu();
            }
            _ => {}
        }
    }

    // --- slash-command autocomplete ---

    /// Slash commands matching the current input (independent of menu state).
    fn matching_commands(&self) -> Vec<CommandOption> {
        let input = self.input.trim();
        CHAT_COMMANDS
            .iter()
            .filter(|opt| opt.menu && opt.command.starts_with(input))
            .copied()
            .collect()
    }

    /// The MCP servers' connection summary: `(connected, total)`, or `None`
    /// before the first successful load. The sidebar turns this into its
    /// green/yellow `mcp n/n` runtime row.
    pub fn mcp_summary(&self) -> Option<(usize, usize)> {
        if self.mcp_servers.is_empty() {
            return None;
        }
        let total = self.mcp_servers.len();
        let up = self.mcp_servers.iter().filter(|s| s.is_connected()).count();
        Some((up, total))
    }

    /// Test seam: replace the loaded MCP server list. Tests in other modules
    /// cannot reach the private field, and going through the `Message` loop
    /// would couple them to the background client.
    #[cfg(test)]
    pub(crate) fn set_mcp_servers_for_tests(&mut self, servers: Vec<crate::opencode::McpServer>) {
        self.mcp_servers = servers;
    }

    /// The open resume picker: the employee's previous sessions plus the
    /// highlighted row, or `None` when no picker is showing.
    pub fn session_picker(&self) -> Option<(&[SessionInfo], usize)> {
        let picker = self.session_picker.as_ref()?;
        Some((picker.sessions.as_slice(), picker.selected))
    }

    /// The title of the picker row at `index` — row 0 is "start fresh", the rest
    /// are the previous sessions (title or id).
    pub fn session_picker_row(&self, index: usize) -> String {
        if index == 0 {
            return "Start a fresh session".to_string();
        }
        self.session_picker
            .as_ref()
            .and_then(|p| p.sessions.get(index - 1))
            .map(|s| s.title.clone().unwrap_or_else(|| s.id.clone()))
            .unwrap_or_default()
    }

    /// The open command menu: matching options plus the selected index, or
    /// `None` when the menu is closed.
    pub fn command_options(&self) -> Option<(Vec<CommandOption>, usize)> {
        let selected = self.command_menu?;
        let options = self.matching_commands();
        if options.is_empty() {
            return None;
        }
        let selected = selected.min(options.len() - 1);
        Some((options, selected))
    }

    /// Reconcile `command_menu` with the current input: open on a match, close
    /// when nothing matches, and clamp the selection into range.
    fn sync_command_menu(&mut self) {
        let options = self.matching_commands();
        if options.is_empty() {
            self.command_menu = None;
        } else {
            let selected = self.command_menu.unwrap_or(0).min(options.len() - 1);
            self.command_menu = Some(selected);
        }
    }

    /// Move the open menu's selection by `delta` (wrapping).
    fn move_command_menu(&mut self, delta: isize) {
        let Some(selected) = self.command_menu else {
            return;
        };
        let count = self.matching_commands().len();
        if count == 0 {
            return;
        }
        let next = ((selected as isize + delta).rem_euclid(count as isize)) as usize;
        self.command_menu = Some(next);
    }

    /// Insert the highlighted command into the input and close the menu. The menu
    /// stays closed until the next edit (so a completed `/help` isn't re-offered).
    fn complete_command(&mut self) {
        let Some(selected) = self.command_menu else {
            return;
        };
        let options = self.matching_commands();
        let Some(option) = options.get(selected) else {
            return;
        };
        self.input = option.command.to_string();
        if !option.args.is_empty() {
            self.input.push(' ');
        }
        self.input_cursor = self.grapheme_len();
        self.input_scroll = 0;
        self.command_menu = None;
    }

    // --- input editing (cursor-based line editor) ---

    /// Byte offset of the cursor within `self.input`, at a grapheme-cluster
    /// (hence char) boundary.
    fn input_byte_offset(&self) -> usize {
        self.input
            .grapheme_indices(true)
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    /// Byte offsets of every grapheme-cluster boundary in `self.input`
    /// (len + 1 elements, both ends included).
    fn grapheme_boundaries(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.input.grapheme_indices(true).map(|(i, _)| i).collect();
        v.push(self.input.len());
        v
    }

    /// The number of grapheme clusters in `self.input`.
    fn grapheme_len(&self) -> usize {
        self.input.graphemes(true).count()
    }

    /// Clamp the cursor into `0..=grapheme_len` as a defensive bound.
    fn clamp_cursor(&mut self) {
        let len = self.grapheme_len();
        if self.input_cursor > len {
            self.input_cursor = len;
        }
    }

    /// Display columns available for the input body — the prompt (`> `) and the
    /// panel's two side borders aside.
    ///
    /// The chat input sits in the chat panel (which spans `main`, the full
    /// terminal width less the title/hint bars); the roster prompt spans the whole
    /// width. Both leave the same four columns of chrome, so only the width source
    /// differs.
    fn input_body_width(&self) -> usize {
        (self.last_size.0 as usize).saturating_sub(4)
    }

    /// Keep the input horizontally scrolled so the cursor stays within the body
    /// window. Called after the input or cursor changes, and on resize.
    fn follow_input_cursor(&mut self) {
        let width = self.input_body_width();
        if width == 0 {
            self.input_scroll = 0;
            return;
        }
        let cursor_col = self.input[..self.input_byte_offset()].width();
        if cursor_col >= self.input_scroll + width {
            self.input_scroll = cursor_col + 1 - width;
        } else if cursor_col < self.input_scroll {
            self.input_scroll = cursor_col;
        }
        // On shrink (backspace / delete-word / delete-to-start), collapse the
        // view back to the smallest scroll that still keeps the cursor in view.
        // Without this, deleting leaves the scroll stranded past the (now
        // shorter) text, trimming the whole input off the left of the window.
        let minimal = cursor_col.saturating_add(1).saturating_sub(width);
        self.input_scroll = self.input_scroll.min(minimal);
    }

    fn cursor_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
        self.follow_input_cursor();
    }

    fn cursor_right(&mut self) {
        self.clamp_cursor();
        if self.input_cursor < self.grapheme_len() {
            self.input_cursor += 1;
        }
        self.follow_input_cursor();
    }

    fn cursor_home(&mut self) {
        self.input_cursor = 0;
        self.follow_input_cursor();
    }

    fn cursor_end(&mut self) {
        self.input_cursor = self.grapheme_len();
        self.follow_input_cursor();
    }

    fn input_insert(&mut self, c: char) {
        let at = self.input_byte_offset();
        self.input.insert(at, c);
        // Place the cursor just past the inserted char: if it merged with the
        // preceding cluster (a combining mark), the cursor sits at that cluster's
        // end rather than overrunning the grapheme count.
        self.input_cursor = self
            .input
            .grapheme_indices(true)
            .take_while(|(i, _)| *i <= at)
            .count();
        self.reset_hist_navigation();
        self.follow_input_cursor();
    }

    fn input_delete_backward(&mut self) {
        self.clamp_cursor();
        if self.input_cursor == 0 {
            return;
        }
        let bounds = self.grapheme_boundaries();
        let start = bounds[self.input_cursor - 1];
        let end = bounds[self.input_cursor];
        self.input.replace_range(start..end, "");
        self.input_cursor -= 1;
        self.reset_hist_navigation();
        self.follow_input_cursor();
    }

    fn input_delete_forward(&mut self) {
        self.clamp_cursor();
        if self.input_cursor >= self.grapheme_len() {
            return;
        }
        let bounds = self.grapheme_boundaries();
        let start = bounds[self.input_cursor];
        let end = bounds[self.input_cursor + 1];
        self.input.replace_range(start..end, "");
        self.reset_hist_navigation();
        self.follow_input_cursor();
    }

    fn input_delete_word(&mut self) {
        let graphemes: Vec<&str> = self.input.graphemes(true).collect();
        let is_space = |g: &str| g.chars().all(char::is_whitespace);
        let mut c = self.input_cursor.min(graphemes.len());
        while c > 0 && is_space(graphemes[c - 1]) {
            c -= 1;
        }
        while c > 0 && !is_space(graphemes[c - 1]) {
            c -= 1;
        }
        let bounds = self.grapheme_boundaries();
        let start = bounds[c];
        let end = bounds[self.input_cursor.min(graphemes.len())];
        self.input.replace_range(start..end, "");
        self.input_cursor = c;
        self.reset_hist_navigation();
        self.sync_command_menu();
        self.follow_input_cursor();
    }

    fn input_delete_to_start(&mut self) {
        self.clamp_cursor();
        if self.input_cursor == 0 {
            return;
        }
        let end = self.input_byte_offset();
        self.input.replace_range(..end, "");
        self.input_cursor = 0;
        self.reset_hist_navigation();
        self.sync_command_menu();
        self.follow_input_cursor();
    }

    /// Insert a bracketed paste into the input at the cursor.
    fn handle_paste(&mut self, text: &str) {
        // The one input buffer serves the chat and the roster prompt; a paste goes
        // to whichever has the keyboard.
        if self.mode != Mode::Chat && !self.prompt_focused {
            return;
        }
        let at = self.input_byte_offset();
        self.input.insert_str(at, text);
        self.input_cursor += text.graphemes(true).count();
        self.reset_hist_navigation();
        self.sync_command_menu();
        self.follow_input_cursor();
    }

    // --- history recall ---

    fn history_up(&mut self) {
        let Some(employee) = self.chat_employee else {
            return;
        };
        let Some(chat) = self.chats.get_mut(&employee) else {
            return;
        };
        if chat.history.is_empty() {
            return;
        }
        match chat.hist_index {
            None => {
                chat.history_draft = self.input.clone();
                let last = chat.history.len() - 1;
                chat.hist_index = Some(last);
                self.input = chat.history[last].clone();
            }
            Some(0) => {}
            Some(i) => {
                let next = i - 1;
                chat.hist_index = Some(next);
                self.input = chat.history[next].clone();
            }
        }
        self.input_cursor = self.input.graphemes(true).count();
        self.command_menu = None;
        self.follow_input_cursor();
    }

    fn history_down(&mut self) {
        let Some(employee) = self.chat_employee else {
            return;
        };
        let Some(chat) = self.chats.get_mut(&employee) else {
            return;
        };
        let Some(i) = chat.hist_index else {
            return;
        };
        if i + 1 < chat.history.len() {
            let next = i + 1;
            chat.hist_index = Some(next);
            self.input = chat.history[next].clone();
        } else {
            chat.hist_index = None;
            self.input = std::mem::take(&mut chat.history_draft);
        }
        self.input_cursor = self.input.graphemes(true).count();
        self.command_menu = None;
        self.follow_input_cursor();
    }

    /// Clear history navigation on the first fresh edit after a recall.
    fn reset_hist_navigation(&mut self) {
        if let Some(employee) = self.chat_employee
            && let Some(chat) = self.chats.get_mut(&employee)
        {
            chat.hist_index = None;
        }
        self.prompt_hist_index = None;
    }

    // --- the roster prompt ---

    /// Give the roster's prompt bar the keyboard (`i`, `Tab`, or a click on it).
    ///
    /// The bar shares the one `input` buffer with the chat, so focusing it starts
    /// from whatever is there — which is empty, because the buffer is cleared on
    /// every mode switch.
    fn focus_prompt(&mut self) {
        self.prompt_focused = true;
        self.prompt_hist_index = None;
        self.command_menu = None;
        self.follow_input_cursor();
    }

    /// Hand the keyboard back to the roster shortcuts, keeping whatever is typed
    /// in the bar (an abandoned goal is not thrown away by `Esc`).
    fn blur_prompt(&mut self) {
        self.prompt_focused = false;
        self.prompt_hist_index = None;
    }

    /// Recall the previous goal into the prompt (`↑`).
    fn prompt_history_up(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }
        match self.prompt_hist_index {
            None => {
                self.prompt_hist_draft = self.input.clone();
                let last = self.prompt_history.len() - 1;
                self.prompt_hist_index = Some(last);
                self.input = self.prompt_history[last].clone();
            }
            Some(0) => {}
            Some(i) => {
                self.prompt_hist_index = Some(i - 1);
                self.input = self.prompt_history[i - 1].clone();
            }
        }
        self.input_cursor = self.grapheme_len();
        self.follow_input_cursor();
    }

    /// Move back toward the draft (`↓`), restoring it past the newest goal.
    fn prompt_history_down(&mut self) {
        let Some(i) = self.prompt_hist_index else {
            return;
        };
        if i + 1 < self.prompt_history.len() {
            self.prompt_hist_index = Some(i + 1);
            self.input = self.prompt_history[i + 1].clone();
        } else {
            self.prompt_hist_index = None;
            self.input = std::mem::take(&mut self.prompt_hist_draft);
        }
        self.input_cursor = self.grapheme_len();
        self.follow_input_cursor();
    }

    /// Submit the goal typed on the roster screen.
    ///
    /// The goal goes to the **selected** employee: there is no root above the
    /// team any more, so the card the cursor is on is who the prompt bar speaks
    /// to. `Enter` in that employee's chat is the same conversation — the bar is
    /// just a shortcut that does not leave the roster.
    fn submit_goal(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }

        self.input.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
        self.prompt_hist_index = None;

        // Slash commands are handled locally, exactly as they are in a chat: a
        // `/help` typed in the main prompt is the same request as a `/help` typed
        // in a conversation, and must never be sent to an employee as a goal.
        //
        // This runs before the roster check below: `/help` and `/skills` do not
        // need an employee to act on, so they must still work on an empty roster.
        if self.dispatch_chat_command(&text) {
            return;
        }

        if self.team.is_empty() {
            self.push_log("no employees on the roster — cannot send the goal".to_string());
            return;
        }

        let employee = self.selected;

        if self.prompt_history.last() != Some(&text) {
            self.prompt_history.push(text.clone());
        }

        // The goal starts a chain rooted at this employee's own chat, so its
        // eventual `@user` is delivered there. The chat may not exist yet — a
        // goal typed on the roster screen is the first thing this employee is
        // asked to do — so it is created here rather than dropped.
        self.chats.entry(employee).or_default();
        self.push_to_chat(employee, TranscriptRole::User, text.clone());
        self.team.set_last_task(employee, Some(task_summary(&text)));
        self.origin_of.insert(employee, employee);
        self.depth_of.insert(employee, 0);
        // A goal from the user starts a new trace: everything it causes is
        // recorded under this id so the run can be named in a bug report.
        self.begin_trace(&text, Some(employee));
        let name = self
            .team
            .name_at(employee)
            .unwrap_or_else(|| employee.to_string());
        self.push_log(format!("◆ goal → {name}: {}", task_summary(&text)));

        if !self.can_prompt(employee) {
            self.enqueue_prompt(employee, text);
        } else {
            self.dispatch_prompt(employee, text);
        }
    }

    // --- transcript scrolling ---

    fn scroll_transcript(&mut self, dir: isize) {
        let Some(employee) = self.chat_employee else {
            return;
        };
        let Some(chat) = self.chats.get_mut(&employee) else {
            return;
        };
        let page = (self.last_size.1.saturating_sub(6)).max(5) as usize;
        if dir < 0 {
            // Scroll further up from the live tail.
            chat.scroll = chat.scroll.saturating_add(page);
        } else {
            // Paging down moves back toward (and past) the live tail.
            chat.scroll = chat.scroll.saturating_sub(page);
        }
    }

    // --- session controls ---

    /// Interrupt the current employee's running session (if any).
    fn interrupt_current(&mut self) {
        let Some(employee) = self.chat_employee else {
            return;
        };
        let Some(session_id) = self.session_id_of(employee) else {
            return;
        };
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.streaming_text.clear();
            chat.thinking.clear();
            chat.transcript.push(TranscriptLine::new(
                TranscriptRole::Info,
                "⏹ interrupted".to_string(),
            ));
        }
        if let Some(client) = self.client.clone() {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                if let Err(err) = client.interrupt(&session_id).await {
                    let _ = tx
                        .send(Message::LogError(format!("interrupt failed: {err}")))
                        .await;
                }
            });
        }
    }

    /// Reset the current chat to a brand-new session.
    fn reset_chat(&mut self) {
        let Some(employee) = self.chat_employee else {
            return;
        };
        self.input.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
        self.command_menu = None;
        self.team.set_last_task(employee, None);
        self.team.set_last_tool(employee, None);
        // Drop the old session binding and start a fresh one (same worker).
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.session_id = None;
            chat.creating = false;
            chat.transcript.clear();
            chat.streaming_text.clear();
            chat.thinking.clear();
            chat.scroll = 0;
            // Prompts queued against the old conversation don't carry over.
            chat.queue_running.clear();
            // The new session has never been briefed: the preamble (roster +
            // the delegation protocol) must go out again, or the employee
            // loses the protocol the moment the user types `/new`.
            chat.bootstrapped = false;
        }
        self.begin_session(employee);
    }

    // --- chat lifecycle ---

    fn start_chat(&mut self, employee: usize) {
        if self.client.is_none() {
            self.push_log("not connected to OpenCode".to_string());
            return;
        }

        self.mode = Mode::Chat;
        self.chat_employee = Some(employee);
        // The input buffer is shared with the roster prompt: leaving the roster
        // drops the keyboard there, so a chat never opens holding a half-typed
        // goal.
        self.prompt_focused = false;
        self.input.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
        self.command_menu = None;

        let chat = self.chats.entry(employee).or_default();
        // Re-entering resumes mid-conversation, at the live tail.
        chat.scroll = 0;

        // Resume the existing session. Its transcript and status are already kept
        // current by the event stream (routed per session, not per view), so just
        // re-enter without resetting anything.
        if chat.session_id.is_some() {
            return;
        }

        // A session is already being created for this employee; just wait.
        if chat.creating {
            self.team.set_status(employee, EmployeeStatus::Thinking);
            return;
        }

        // Ask the service what this employee has done here before, and let the
        // user pick: continue where that left off, or start fresh. Employees
        // with no history skip the picker and begin a session directly.
        self.offer_previous_sessions(employee);
    }

    /// Fetch `employee`'s previous sessions in the current workdir and open the
    /// resume picker. With no history the picker is skipped and a fresh session
    /// begins, exactly as before.
    fn offer_previous_sessions(&mut self, employee: usize) {
        let Some(client) = self.client.clone() else {
            self.begin_session(employee);
            return;
        };
        let directory = self.workdir();
        let agent = self
            .team
            .employees()
            .get(employee)
            .map(|e| e.agent_id().to_owned());
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let sessions = match client.list_sessions(&directory).await {
                Ok(all) => all
                    .into_iter()
                    .filter(|session| {
                        session
                            .agent
                            .as_deref()
                            .is_some_and(|a| Some(a) == agent.as_deref())
                    })
                    .collect(),
                Err(err) => {
                    let _ = tx
                        .send(Message::LogError(format!(
                            "previous sessions unavailable: {err}"
                        )))
                        .await;
                    Vec::new()
                }
            };
            let _ = tx
                .send(Message::PreviousSessionsLoaded { employee, sessions })
                .await;
        });
    }

    /// Show the resume picker for `employee`, or begin a fresh session when
    /// they have no previous ones. Called from `PreviousSessionsLoaded`.
    fn open_session_picker(&mut self, employee: usize, sessions: Vec<SessionInfo>) {
        // The user may have left the chat (or switched employees) while the
        // fetch was in flight — only answer when the chat is still theirs.
        if self.chat_employee != Some(employee) || self.mode != Mode::Chat {
            return;
        }
        if sessions.is_empty() {
            self.begin_session(employee);
            return;
        }
        let count = sessions.len();
        self.session_picker = Some(SessionPicker {
            employee,
            sessions,
            selected: 0,
        });
        self.team.set_status(employee, EmployeeStatus::Idle);
        self.push_log(format!(
            "{} previous session{} found — pick one to continue, or start fresh",
            count,
            if count == 1 { "" } else { "s" }
        ));
    }

    /// Handle a key while the resume picker is open. `Up`/`Down` move, `Enter`
    /// commits (start fresh on row 0, resume the highlighted session
    /// otherwise), `n` forces fresh, and `Esc` closes the chat untouched.
    fn handle_session_picker_key(&mut self, key: KeyEvent) {
        let Some(picker) = self.session_picker.as_mut() else {
            return;
        };
        let employee = picker.employee;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => picker.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => picker.move_selection(1),
            KeyCode::Esc => {
                self.session_picker = None;
                self.close_chat();
            }
            KeyCode::Char('n') => {
                self.session_picker = None;
                self.begin_session(employee);
            }
            KeyCode::Enter => {
                let chosen = picker.selected_session().cloned();
                self.session_picker = None;
                match chosen {
                    Some(session) => self.resume_session(employee, session),
                    None => self.begin_session(employee),
                }
            }
            _ => {}
        }
    }

    /// Bind `employee`'s chat to a previous `session` and load its transcript.
    ///
    /// The bootstrapping preamble is deliberately skipped — the session already
    /// carries the team context from when it ran — and the transcript load goes
    /// through the same `TranscriptLoaded` path a live reload would, so
    /// delegation scanning (`handled_message_ids`) and usage tallying behave
    /// identically to a resumed live session.
    fn resume_session(&mut self, employee: usize, session: SessionInfo) {
        let id = session.id.clone();
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.session_id = Some(id.clone());
            chat.creating = false;
            chat.scroll = 0;
        }
        self.team.set_status(employee, EmployeeStatus::Idle);
        let title = session.title.clone().unwrap_or_else(|| id.clone());
        self.push_log(format!("resumed {title}"));
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.transcript.push(TranscriptLine::new(
                TranscriptRole::Info,
                format!("resumed session {title} — type a task and press Enter"),
            ));
        }
        self.load_transcript(employee, id);
    }

    /// Create a session for every employee up front, so the first delegation hop
    /// to any of them starts instantly instead of paying an on-demand
    /// `create_session` round-trip on the chain's critical path. Idempotent —
    /// [`Self::begin_session`] skips anyone who already has (or is making) a
    /// session — and gated by `config.prewarm_sessions`. Each session creation is
    /// spawned independently, so they warm concurrently rather than one at a time.
    fn prewarm_sessions(&mut self) {
        if self.client.is_none() {
            return;
        }
        let count = self.team.employees().len();
        if count == 0 {
            return;
        }
        self.push_log(format!("pre-warming {count} sessions…"));
        for employee in 0..count {
            // Ensure the chat entry exists so `begin_session` records `creating`
            // and `handle_session_created` can store the returned session id.
            self.chats.entry(employee).or_default();
            self.begin_session(employee);
        }
    }

    /// Create the OpenCode session backing `employee`'s chat. A no-op when one
    /// already exists or is already being created, so callers may ask
    /// unconditionally.
    fn begin_session(&mut self, employee: usize) {
        if self
            .chats
            .get(&employee)
            .is_some_and(Chat::started_or_starting)
        {
            return;
        }
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.creating = true;
            chat.streaming_text.clear();
            chat.thinking.clear();
        }
        self.team.set_status(employee, EmployeeStatus::Thinking);

        let Some(client) = self.client.clone() else {
            return;
        };
        let directory = self.workdir();
        let employee_conf = self.team.employees().get(employee);
        let agent = employee_conf.map(|e| e.agent_id().to_owned());
        let model = employee_conf.and_then(|e| e.model_ref());
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let created = client
                .create_session_with_agent_fallback(&directory, agent.as_deref(), model.as_ref())
                .await;

            let result = match created {
                Ok((session, true)) => Ok(session),
                Ok((session, false)) => {
                    if let Some(agent_name) = agent.as_deref() {
                        let _ = tx
                            .send(Message::Log(format!(
                                "agent '{agent_name}' not defined — using OpenCode's default agent"
                            )))
                            .await;
                    }
                    Ok(session)
                }
                Err(err) => Err(format!("{err:#}")),
            };

            let _ = tx.send(Message::SessionCreated { employee, result }).await;
        });
    }

    fn handle_session_created(
        &mut self,
        employee: usize,
        result: Result<crate::opencode::SessionInfo, String>,
    ) {
        match result {
            Ok(session) => {
                let id = session.id.clone();
                self.team.set_status(employee, EmployeeStatus::Idle);
                self.push_log(format!("session {id} ready"));
                if let Some(chat) = self.chats.get_mut(&employee) {
                    chat.creating = false;
                    chat.session_id = Some(id.clone());
                    chat.scroll = 0;
                    chat.transcript.push(TranscriptLine::new(
                        TranscriptRole::Info,
                        format!("session {id} — type a task and press Enter"),
                    ));
                }
                // Relay any instructions queued while the session was being made.
                self.flush_outbox(employee);
                // Then anything the user typed while the session was pending.
                self.pump_employee_queue(employee);
            }
            Err(err) => {
                self.team.set_status(employee, EmployeeStatus::Error);
                self.push_error_log(format!("session failed: {err}"));
                if let Some(chat) = self.chats.get_mut(&employee) {
                    chat.creating = false;
                    chat.pending = None;
                    chat.transcript.push(TranscriptLine::new(
                        TranscriptRole::Error,
                        format!("session failed: {err}"),
                    ));
                }
                // A worker whose session never materialized can't deliver; note it
                // so any fan-out it belongs to still aggregates.
                self.force_worker_contribution(employee, format!("session failed: {err}"));
            }
        }
    }

    fn send_prompt(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
        self.command_menu = None;

        // Chat-local slash commands are handled here and never sent to the
        // OpenCode session (e.g. `/skill add <source>`).
        if self.dispatch_chat_command(&text) {
            return;
        }

        let Some(employee) = self.chat_employee else {
            return;
        };

        // Record the prompt immediately — in the transcript and the recall
        // history — whether it goes out now or waits behind an in-flight turn.
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.transcript
                .push(TranscriptLine::new(TranscriptRole::User, text.clone()));
            chat.streaming_text.clear();
            chat.thinking.clear();
            chat.scroll = 0;
            // Record history (dedup consecutive repeats), reset navigation.
            if chat.history.last() != Some(&text) {
                chat.history.push(text.clone());
            }
            chat.hist_index = None;
        }
        self.team.set_last_task(employee, Some(task_summary(&text)));
        // A fresh user prompt to this employee starts a new delegation chain whose
        // origin is this very chat, so a later `@user` delivers back here.
        self.origin_of.insert(employee, employee);
        self.depth_of.insert(employee, 0);
        // A fresh user prompt starts a new run, so it gets its own trace: the
        // hops that follow are attributable to *this* prompt rather than to
        // whatever the employee was doing before.
        self.begin_trace(&text, Some(employee));

        // If the employee is mid-turn (or their session isn't ready yet), the
        // prompt is queued and sent once they're idle — see `pump_employee_queue`.
        if !self.can_prompt(employee) {
            self.enqueue_prompt(employee, text);
            return;
        }

        self.dispatch_prompt(employee, text);
    }

    /// A clone of `employee`'s live session id, when there is one.
    fn session_id_of(&self, employee: usize) -> Option<String> {
        self.chats
            .get(&employee)
            .and_then(|chat| chat.session_id.clone())
    }

    /// True when `employee`'s chat has a session ready to take a prompt. This is
    /// the one definition of "a session exists"; `creating` can never be set
    /// alongside a session id, since [`Self::begin_session`] only starts one when
    /// there is none and [`Self::handle_session_created`] clears both together.
    fn session_ready(&self, employee: usize) -> bool {
        self.chats.get(&employee).is_some_and(Chat::ready)
    }

    /// True when `employee` can take a prompt right now: a ready session and no
    /// turn in flight. Both queue points gate on this, so "queue it instead of
    /// sending it" has exactly one definition.
    fn can_prompt(&self, employee: usize) -> bool {
        self.session_ready(employee) && !self.employee_is_busy(employee)
    }

    /// True when `employee` is mid-turn and so cannot take a new prompt right
    /// now. Delegates to [`Team::is_busy_at`] so "busy" has one definition.
    fn employee_is_busy(&self, employee: usize) -> bool {
        self.team.is_busy_at(employee)
    }

    /// Queue `text` for `employee`, starting their session if none is already
    /// being created so the queue can drain once it is ready.
    fn enqueue_prompt(&mut self, employee: usize, text: String) {
        let name = self
            .team
            .name_at(employee)
            .unwrap_or_else(|| employee.to_string());
        self.push_log(format!("queued for {name}"));
        self.chats
            .entry(employee)
            .or_default()
            .queue_running
            .push(text);
        // A no-op when the session already exists or is on its way.
        self.begin_session(employee);
    }

    /// Drain the next queued prompt for `employee`, if they are idle with a
    /// ready session. At most one prompt is sent per call: dispatching marks the
    /// employee `Thinking`, so the next call (on the following completed turn)
    /// picks up the rest.
    fn pump_employee_queue(&mut self, employee: usize) {
        if self.client.is_none() {
            return;
        }
        let Some(text) = self.next_queued(employee) else {
            return;
        };
        self.dispatch_prompt(employee, text);
    }

    /// Pop `employee`'s oldest queued prompt, but only when they are idle with a
    /// live session — otherwise `None`, leaving the queue intact. Split out from
    /// [`Self::pump_employee_queue`] so the gating can be exercised without a
    /// live client.
    fn next_queued(&mut self, employee: usize) -> Option<String> {
        if !self.can_prompt(employee) {
            return None;
        }
        let chat = self.chats.get_mut(&employee)?;
        if chat.queue_running.is_empty() {
            None
        } else {
            Some(chat.queue_running.remove(0))
        }
    }

    /// Send `text` to `employee`'s live session right now, marking them busy. The
    /// caller has already recorded it in the transcript and history.
    fn dispatch_prompt(&mut self, employee: usize, text: String) {
        let Some(session_id) = self.session_id_of(employee) else {
            return;
        };
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.streaming_text.clear();
            chat.thinking.clear();
            chat.scroll = 0;
        }
        self.team.set_status(employee, EmployeeStatus::Thinking);

        // The first prompt to an employee carries the team roster + delegation
        // protocol; later prompts go through verbatim.
        let prompt = self.bootstrap_prompt(employee, text);
        self.spawn_prompt(
            employee,
            session_id,
            prompt,
            harness::PromptKind::UserPrompt,
            &harness::delivery_token(),
        );
    }

    /// Spawn one prompt delivery to a live session, under the delivery policy for
    /// `kind`.
    ///
    /// The options carry both the steer/queue decision (from
    /// [`harness::delivery_for`]) and the admission id, so a resend of the same
    /// logical delivery is deduplicated rather than admitted twice. `token` is
    /// the delivery's identity: the same token re-sends the same admission id,
    /// a new token makes a new delivery. The retry **attempt** is deliberately
    /// not an argument here — it is the sender's budget, not part of what is
    /// being delivered, and folding it into the id made every resend a new
    /// admission.
    ///
    /// A transport error comes back as [`Message::DeliveryFailed`], so the
    /// caller's retry and queue logic sees it as the turn ending badly rather
    /// than by silence.
    fn spawn_prompt(
        &self,
        employee: usize,
        session_id: String,
        prompt: String,
        kind: harness::PromptKind,
        token: &str,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let id = self
            .team
            .employees()
            .get(employee)
            .map(|e| e.agent_id().to_string())
            .unwrap_or_else(|| employee.to_string());
        let depth = self.chain_depth(employee);
        let options = harness::prompt_options(kind, &id, depth, token);

        let tx = self.tx.clone();
        let target = employee;
        tokio::spawn(async move {
            if let Err(err) = client.prompt_with(&session_id, &prompt, &options).await {
                let _ = tx
                    .send(Message::DeliveryFailed {
                        target,
                        reason: err.to_string(),
                    })
                    .await;
            }
        });
    }

    /// Recognize and run chat-local slash commands. Returns true when `text` was
    /// consumed as a command (it must not be forwarded to the session).
    fn dispatch_chat_command(&mut self, text: &str) -> bool {
        let Some(rest) = text.trim().strip_prefix('/') else {
            return false;
        };
        let rest = rest.trim();

        // `/stop` — interrupt the current generation.
        if rest == "stop" {
            self.interrupt_current();
            return true;
        }

        // `/new` and `/clear` — start a fresh session for this employee.
        if rest == "new" || rest == "clear" {
            self.push_command_line(format!("⚡ {text}"));
            self.reset_chat();
            return true;
        }

        // `/skills` — list installed skill names into the transcript.
        if rest == "skills" {
            self.push_command_line(format!("⚡ {text}"));
            let names = crate::skills::installed();
            if names.is_empty() {
                self.push_command_line("no skills installed for this project".to_string());
            } else {
                for name in names {
                    self.push_command_line(format!("  • {name}"));
                }
            }
            return true;
        }

        // `/skill add <source>[@skill]` — install then attach to this employee.
        if let Some(source) = rest.strip_prefix("skill add ").map(str::trim) {
            if source.is_empty() {
                self.push_command_error("usage: /skill add <source>[@skill]".to_string());
                return true;
            }
            let Some(employee) = self.command_employee() else {
                self.push_command_error("no employee selected".to_string());
                return true;
            };
            self.push_command_line(format!("⚡ /skill add {source}"));
            self.spawn_skill_install(employee, source.to_string());
            return true;
        }

        // `/skill <name>` — attach an already-installed skill to this employee.
        if let Some(name) = rest.strip_prefix("skill ").map(str::trim) {
            if name.is_empty() {
                self.push_command_error("usage: /skill <name>".to_string());
                return true;
            }
            self.attach_skill(name.to_string());
            return true;
        }

        // `/parallel <goal>` — fan the goal out to the whole team and aggregate.
        if let Some(goal) = rest.strip_prefix("parallel ").map(str::trim) {
            if goal.is_empty() {
                self.push_command_error("usage: /parallel <goal>".to_string());
                return true;
            }
            let Some(employee) = self.command_employee() else {
                self.push_command_error("no employee selected".to_string());
                return true;
            };
            self.push_command_line(format!("⚡ /parallel {goal}"));
            self.fan_out(employee, goal.to_string());
            return true;
        }

        // `/help` — show the chat-local commands, straight from the table the
        // completion menu reads.
        if rest == "help" {
            for line in chat_help_text().lines() {
                self.push_command_line(line.to_string());
            }
            return true;
        }

        // `/trace` — print the active trace, or `/trace all` for every retained
        // one, into the transcript so it can be read (and copied) in place.
        if rest == "trace" || rest == "trace all" {
            self.push_command_line(format!("⚡ {text}"));
            let text = if rest == "trace all" {
                self.orchestration.describe_all()
            } else {
                match self
                    .active_trace
                    .and_then(|id| self.orchestration.describe(id))
                {
                    Some(block) => block,
                    None => "no active trace — start a goal first".to_string(),
                }
            };
            for line in text.lines() {
                self.push_command_line(line.to_string());
            }
            return true;
        }

        // `/copy` — put the error log (or `/copy trace`) on the clipboard.
        if rest == "copy" || rest == "copy trace" || rest == "copy all" {
            let (label, text) = match rest {
                "copy trace" => (
                    "trace".to_string(),
                    match self
                        .active_trace
                        .and_then(|id| self.orchestration.describe(id))
                    {
                        Some(block) => block,
                        None => "no active trace — start a goal first".to_string(),
                    },
                ),
                "copy all" => ("error-log".to_string(), self.full_log_text()),
                _ => ("error-log".to_string(), self.error_log_text()),
            };
            let note = self.copy_to_clipboard(&label, &text);
            // The note goes to the log *and* the transcript: the log band is
            // drawn on every screen (so the roster shows it too), while the
            // transcript keeps it next to the command that caused it.
            self.push_log(note.clone());
            self.push_command_line(note);
            return true;
        }

        false
    }

    /// The error lines currently in the log, oldest first, as one block.
    ///
    /// This is what "copy the error log" means: the failures, with their trace
    /// ids, and nothing else. A user reporting a bug wants the red lines, not
    /// every "connected to OpenCode" notice above them.
    pub fn error_log_text(&self) -> String {
        let lines: Vec<&str> = self
            .log
            .iter()
            .filter(|line| line.error)
            .map(|line| line.text.as_str())
            .collect();
        if lines.is_empty() {
            return "no errors logged".to_string();
        }
        lines.join("\n")
    }

    /// The whole log, oldest first, as one block — the fuller context for a
    /// report that needs the successes leading up to the failure.
    pub fn full_log_text(&self) -> String {
        self.log
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Hand `text` to the clipboard and report what happened, in one line the
    /// caller can push into the transcript.
    ///
    /// The file is always written, OSC 52 is always queued for the terminal, and
    /// a native tool is used when one exists. The returned line names the file
    /// path, so even when the clipboards fail the text is reachable.
    fn copy_to_clipboard(&mut self, label: &str, text: &str) -> String {
        let file = crate::clipboard::write_file(label, text).ok();
        let native = crate::clipboard::to_native(text);
        // Queue the terminal route regardless: it is what makes the copy work
        // over SSH, and it is harmless where the native tool already succeeded.
        self.pending_clipboard = Some(crate::clipboard::osc52(text));

        let mut note = if native {
            format!("📋 copied {label} to the clipboard")
        } else {
            format!("📋 {label} sent to the terminal clipboard (OSC 52)")
        };
        if let Some(path) = &file {
            note.push_str(&format!(" — also saved to {}", path.display()));
        } else {
            note.push_str(" — could not write the fallback file");
        }
        note
    }

    /// The employee a slash command acts on: the chat being viewed, else the
    /// card the roster cursor is on — the main prompt bar speaks for that
    /// employee, so a command typed there (`/new`, `/skill add …`,
    /// `/parallel …`) means them, not nobody.
    fn command_employee(&self) -> Option<usize> {
        self.chat_employee.or({
            // A roster with nobody on it has no employee to act on.
            (!self.team.is_empty()).then_some(self.selected)
        })
    }

    /// Report a command's output.
    ///
    /// It goes into the acting employee's transcript, and — when that chat is not
    /// the one on screen — into the main log as well, because a command typed in
    /// the roster prompt must answer *where the user is looking*. Without this,
    /// `/help` from the main screen would write into a transcript the user cannot
    /// see.
    fn push_command_line(&mut self, text: String) {
        match self.chat_employee {
            Some(employee) => self.push_to_chat(employee, TranscriptRole::Info, text),
            None => {
                if let Some(employee) = self.command_employee() {
                    self.push_to_chat(employee, TranscriptRole::Info, text.clone());
                }
                self.push_log(text);
            }
        }
    }

    /// [`Self::push_command_line`] for a failure, in the error colour.
    fn push_command_error(&mut self, text: String) {
        match self.chat_employee {
            Some(employee) => self.push_to_chat(employee, TranscriptRole::Error, text),
            None => {
                if let Some(employee) = self.command_employee() {
                    self.push_to_chat(employee, TranscriptRole::Error, text.clone());
                }
                self.push_log(text);
            }
        }
    }

    /// Kick off a background install of `source`, reporting installed skill
    /// names back through the event channel.
    fn spawn_skill_install(&mut self, employee: usize, source: String) {
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = crate::skills::add_named(&source).map_err(|err| err.to_string());
            let _ = tx.blocking_send(Message::SkillInstallDone { employee, result });
        });
    }

    /// Attach the freshly installed skills to the employee and persist the roster.
    fn handle_skill_install_done(&mut self, employee: usize, result: Result<Vec<String>, String>) {
        let names = match result {
            Ok(names) => names,
            Err(err) => {
                self.push_to_chat(
                    employee,
                    TranscriptRole::Error,
                    format!("skill install failed: {err}"),
                );
                return;
            }
        };

        let employee_name = self
            .team
            .employees()
            .get(employee)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "employee".to_string());
        let employee_id = self.team.employees().get(employee).map(|e| e.id.clone());

        let Some(employee_id) = employee_id else {
            self.push_to_chat(
                employee,
                TranscriptRole::Error,
                "no such employee".to_string(),
            );
            return;
        };

        let mut attached: Vec<String> = Vec::new();
        for name in &names {
            match self.team.add_skill(&employee_id, name) {
                Ok(()) => attached.push(name.clone()),
                Err(err) => self.push_to_chat(employee, TranscriptRole::Error, err),
            }
        }

        if let Err(err) = self.persist_team() {
            self.push_to_chat(
                employee,
                TranscriptRole::Error,
                format!("saving team failed: {err}"),
            );
            return;
        }

        let summary = if attached.len() == 1 {
            format!(
                "✓ installed & attached '{}' to {employee_name}",
                attached[0]
            )
        } else {
            format!(
                "✓ installed & attached {} skills ({}) to {employee_name}",
                attached.len(),
                attached.join(", ")
            )
        };
        self.push_to_chat(employee, TranscriptRole::Info, summary);
    }

    /// Attach an already-installed skill (by name) to the current employee.
    fn attach_skill(&mut self, name: String) {
        self.push_transcript_line(TranscriptRole::Info, format!("⚡ /skill {name}"));

        let Some(employee) = self.chat_employee else {
            self.push_transcript_error("no employee selected".to_string());
            return;
        };
        let Some(employee_name) = self.team.employees().get(employee).map(|e| e.name.clone())
        else {
            self.push_transcript_error("no such employee".to_string());
            return;
        };
        let Some(employee_id) = self.team.employees().get(employee).map(|e| e.id.clone()) else {
            self.push_transcript_error("no such employee".to_string());
            return;
        };

        if let Err(err) = self.team.add_skill(&employee_id, &name) {
            self.push_transcript_error(err);
            return;
        }
        if let Err(err) = self.persist_team() {
            self.push_transcript_error(format!("saving team failed: {err}"));
            return;
        }

        self.push_transcript_line(
            TranscriptRole::Info,
            format!("✓ attached '{name}' to {employee_name}"),
        );
    }

    /// Persist the roster to its default location.
    fn persist_team(&self) -> Result<()> {
        let path = crate::team::Team::default_path()
            .ok_or_else(|| color_eyre::eyre::eyre!("could not determine config directory"))?;
        self.team.save(&path)
    }

    /// The currently-viewed employee's chat, if any.
    fn current_chat(&self) -> Option<&Chat> {
        self.chat_employee
            .and_then(|employee| self.chats.get(&employee))
    }

    fn current_chat_mut(&mut self) -> Option<&mut Chat> {
        self.chat_employee
            .and_then(|employee| self.chats.get_mut(&employee))
    }

    /// The employee whose chat is backed by `session_id`, if any.
    fn employee_for_session(&self, session_id: &str) -> Option<usize> {
        self.chats.iter().find_map(|(&employee, chat)| {
            (chat.session_id.as_deref() == Some(session_id)).then_some(employee)
        })
    }

    /// Push a line into the currently-viewed chat's transcript.
    fn push_transcript_line(&mut self, role: TranscriptRole, text: String) {
        if let Some(chat) = self.current_chat_mut() {
            chat.scroll = 0;
            chat.transcript.push(TranscriptLine::new(role, text));
        }
    }

    /// Push an error line into the currently-viewed chat's transcript.
    fn push_transcript_error(&mut self, message: String) {
        self.push_transcript_line(TranscriptRole::Error, message);
    }

    /// Push a line into a specific employee's chat transcript (regardless of the
    /// current view), so async results land in the right conversation.
    fn push_to_chat(&mut self, employee: usize, role: TranscriptRole, text: String) {
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.transcript.push(TranscriptLine::new(role, text));
        }
    }

    fn close_chat(&mut self) {
        self.mode = Mode::Roster;
        self.chat_employee = None;
        self.input.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
        self.command_menu = None;
        // Leave the chat state (session + transcript) intact so re-entering the
        // same employee resumes the conversation instead of starting fresh.
    }
    fn handle_transcript(&mut self, employee: usize, messages: Vec<SessionMessage>) {
        // Tally token usage (idempotent per message id against transcript reloads).
        for message in &messages {
            self.usage.record(message);
        }
        let Some(chat) = self.chats.get_mut(&employee) else {
            return;
        };
        chat.transcript = build_transcript(&messages);
        chat.streaming_text.clear();
        chat.thinking.clear();
        chat.scroll = 0;
        // The hand-off "returned" (in some form), so stop tracking its deadline.
        chat.pending = None;

        let final_status = completion_status(&messages);
        self.team.set_status(employee, final_status);

        // A failed turn is never silent: the roster prompt's goal has no chat
        // transcript on screen, so the log is the only place the user can read
        // *why* it failed. The reason is the provider's error (type · message ·
        // status) when the transcript carries one.
        if final_status == EmployeeStatus::Error
            && let Some(reason) = failed_turn_reason(&messages)
        {
            let name = self
                .team
                .name_at(employee)
                .unwrap_or_else(|| employee.to_string());
            self.trace_hop(
                crate::trace::Kind::Failed,
                Some(employee),
                None,
                format!("{name}: {reason}"),
            );
            self.push_error_log(format!("{name} failed: {reason}"));
        }

        // Hand off any directives in the freshly completed assistant reply.
        let before = self.delegation_count;
        self.dispatch_delegations(employee, &messages);
        let handed_off = self.delegation_count != before;

        // Record the employee's deliverable into any active parallel fan-out.
        let owed_before = self.fanouts_awaiting(employee);
        self.note_fanout_result(employee, &messages);
        let contributed = !owed_before.is_empty();

        // A finished turn reaches the main screen. Every employee may deliver
        // its own work now, and the roster's prompt bar has no transcript on
        // screen — the log is where the user reads the answer. A turn that handed
        // work on, or that contributed to a round, has not finished the goal yet
        // and stays out of the log.
        if !handed_off
            && !contributed
            && let Some(text) = last_assistant_output(&messages)
        {
            let summary = task_summary(&text);
            if !summary.is_empty() {
                self.push_log(format!("◆ {}", summary));
            }
        }

        // The turn is over — send the next prompt the user queued while we were
        // busy, if any.
        self.pump_employee_queue(employee);
    }

    /// Route a streamed OpenCode event into the owning employee's chat
    /// transcript and status, keyed by the event's session id so output keeps
    /// streaming even while the user is on the roster or in another chat.
    fn handle_stream_event(&mut self, event: &Event) {
        let Some(session_id) = event.session_id() else {
            return;
        };
        let Some(employee) = self.employee_for_session(session_id) else {
            return;
        };

        // Any event for this session is a sign of life — push the pending delivery's
        // deadline back so a healthy (but slow) worker isn't abandoned mid-thought.
        if let Some(chat) = self.chats.get_mut(&employee)
            && let Some(pending) = chat.pending.as_mut()
        {
            pending.heartbeat();
        }

        match event.event_type.as_str() {
            "session.reasoning.started" => {
                self.team.set_status(employee, EmployeeStatus::Thinking);
                let key = event.assistant_message_id().map(str::to_owned);
                if let Some(chat) = self.chats.get_mut(&employee) {
                    chat.thinking.begin(key);
                }
            }
            "session.reasoning.delta" => {
                if let Some(delta) = event.text_delta()
                    && let Some(chat) = self.chats.get_mut(&employee)
                {
                    chat.thinking.push(delta);
                }
            }
            "session.execution.started" => {
                self.team.set_status(employee, EmployeeStatus::Working);
                // A new turn starts with no tool behind it: whatever the last
                // turn called is stale, and the card's `tool` row must not claim
                // otherwise while this turn reasons.
                self.team.set_last_tool(employee, None);
                // The turn actually started, so the silence clock starts now: a
                // queued delivery that sat behind the previous turn is not
                // "silent", it was waiting.
                if let Some(chat) = self.chats.get_mut(&employee)
                    && let Some(pending) = chat.pending.as_mut()
                {
                    pending.arm();
                }
            }
            "session.text.delta" | "message.part.delta" => {
                if let Some(delta) = event.text_delta()
                    && let Some(chat) = self.chats.get_mut(&employee)
                {
                    chat.streaming_text.push_str(delta);
                }
            }
            "message.part.updated" => {
                // A full part snapshot; seed the stream only if we missed the
                // deltas (avoid duplicating text we already accumulated).
                if let Some(text) = self.part_text(event)
                    && let Some(chat) = self.chats.get_mut(&employee)
                    && chat.streaming_text.is_empty()
                {
                    chat.streaming_text = text;
                }
            }
            "session.tool.started" | "session.tool.success" | "session.tool.failed" => {
                // The card's `tool` row shows the *classified* call (skill,
                // sub-agent, namespaced, built-in); the transcript keeps the
                // event's own wording. A malformed payload leaves the row alone
                // rather than blanking it, and the two are read separately so a
                // shape only one of them understands still reaches its surface.
                if let Some(tool) = event.tool_use() {
                    self.team.set_last_tool(employee, Some(tool));
                }
                if let Some(label) = event.tool_label() {
                    self.push_to_chat(employee, TranscriptRole::Info, format!("🔧 {label}"));
                }
            }
            "session.execution.succeeded" => {
                self.team.set_status(employee, EmployeeStatus::Done);
                self.load_transcript(employee, session_id.to_string());
            }
            "session.execution.failed" | "session.step.failed" => {
                self.team.set_status(employee, EmployeeStatus::Error);
                // Always log *something*: a failed event without an embedded
                // `data.error.message` still names the event that failed, so the
                // user is never left staring at a silent red card.
                let reason = event
                    .error_message()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{} (no error detail)", event.event_type));
                self.push_error_log(reason);
                self.load_transcript(employee, session_id.to_string());
            }
            _ => {}
        }
    }

    /// The text of a `message.part.updated` snapshot, if it carries one.
    fn part_text(&self, event: &Event) -> Option<String> {
        let part = event.part()?;
        let text = part.get("text")?.as_str()?;
        Some(text.to_string())
    }

    fn load_transcript(&mut self, employee: usize, session_id: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.messages(&session_id).await {
                Ok(messages) => {
                    let _ = tx
                        .send(Message::TranscriptLoaded { employee, messages })
                        .await;
                }
                Err(err) => {
                    let _ = tx
                        .send(Message::LogError(format!("transcript load failed: {err}")))
                        .await;
                }
            }
        });
    }

    // --- delegation (employee ↔ employee hand-off) ---

    /// Resolve an employee id (e.g. `"writer"`) to its roster index.
    fn team_index_by_id(&self, id: &str) -> Option<usize> {
        self.team.employees().iter().position(|e| e.id == id)
    }

    /// Build the prompt to send to `employee`: the first prompt gets the team
    /// roster + delegation protocol prepended so they know how to hand off;
    /// later prompts go through verbatim.
    fn bootstrap_prompt(&mut self, employee: usize, text: String) -> String {
        if self
            .chats
            .get(&employee)
            .is_none_or(|chat| chat.bootstrapped)
        {
            return text;
        }
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.bootstrapped = true;
        }
        let name = self.team.name_at(employee).unwrap_or_default();
        let context = workflow::team_context(&self.team, &name);
        format!("{context}\n\n{text}")
    }

    /// Relay `instruction` to `employee` as `kind`. If their session must be
    /// created first, the instruction is queued and flushed when the session is
    /// ready.
    fn send_to_employee(
        &mut self,
        employee: usize,
        instruction: String,
        kind: harness::PromptKind,
    ) {
        self.push_to_chat(employee, TranscriptRole::Info, format!("📩 {instruction}"));
        self.chats
            .entry(employee)
            .or_default()
            .outbox
            .push(Outgoing::new(kind, instruction));
        if self.session_ready(employee) {
            self.flush_outbox(employee);
        } else {
            self.begin_session(employee);
        }
    }

    /// Re-offer a delivery under its original identity, in its original session.
    ///
    /// This is the resend path: `kind` and `token` are the ones the delivery was
    /// first queued with, so the admission id repeats and a service that already
    /// admitted the first attempt deduplicates this one instead of running the
    /// work twice. It targets the session the delivery was queued against for the
    /// same reason — the id is only deduplicated inside the session that holds
    /// it, so a resend that started a new session would be admitted afresh.
    fn resend_to_employee(
        &mut self,
        employee: usize,
        instruction: String,
        kind: harness::PromptKind,
        token: String,
    ) {
        self.push_to_chat(employee, TranscriptRole::Info, format!("📩 {instruction}"));
        self.chats
            .entry(employee)
            .or_default()
            .outbox
            .push(Outgoing {
                kind,
                text: instruction,
                token,
            });
        if self.session_ready(employee) {
            self.flush_outbox(employee);
        } else {
            // No session to resend into: creating one would change the admission
            // context, so the delivery waits for the original session instead.
            self.begin_session(employee);
        }
    }

    /// Record one hand-off for the wires, dropping the oldest once the window is
    /// full. The cap is what keeps a long chain from growing the trace list
    /// without bound; TTL expiry is [`Self::tick_traces`]' job.
    fn trace(&mut self, from: usize, to: usize, kind: TraceKind) {
        if from == to {
            // A hand-off to yourself is not a wire: nothing moved.
            return;
        }
        self.traces.push(DelegationTrace {
            from,
            to,
            kind,
            started: self.frame,
        });
        if self.traces.len() > Self::TRACE_LIMIT {
            let excess = self.traces.len() - Self::TRACE_LIMIT;
            self.traces.drain(..excess);
        }
    }

    /// Forget the traces whose packets have finished travelling. Driven from the
    /// tick and from [`Self::advance`], so the UI's traces are always a function
    /// of the current frame.
    fn tick_traces(&mut self) {
        let frame = self.frame;
        self.traces.retain(|trace| !trace.is_expired(frame));
    }

    /// Send every queued delegation prompt out to `employee`'s live session.
    fn flush_outbox(&mut self, employee: usize) {
        // Without a client there is nowhere to send them: leave the outbox — and
        // so the hand-off — intact for a later attempt.
        if self.client.is_none() {
            return;
        }

        let (outbox, session_id) = {
            let Some(chat) = self.chats.get_mut(&employee) else {
                return;
            };
            let outbox = std::mem::take(&mut chat.outbox);
            let Some(session_id) = chat.session_id.clone() else {
                chat.outbox = outbox;
                return;
            };
            (outbox, session_id)
        };

        for outgoing in outbox {
            let Outgoing {
                kind,
                text: instruction,
                token,
            } = outgoing;
            self.team
                .set_last_task(employee, Some(task_summary(&instruction)));
            // Preserve any retry budget accrued on a prior send, then prepend a
            // retry preamble when this is a retry (never on the original attempt).
            let attempts = self
                .chats
                .get(&employee)
                .and_then(|c| c.pending.as_ref())
                .map(|p| p.attempts)
                .unwrap_or(0);
            let prompt = self.bootstrap_prompt(employee, instruction.clone());
            let prompt = if attempts > 0 {
                format!("{}\n\n{}", harness::retry_preamble(attempts), prompt)
            } else {
                prompt
            };
            self.team.set_status(employee, EmployeeStatus::Thinking);
            // Track the hand-off so a silent worker can be retried/abandoned. The
            // stored instruction stays preamble-free (the original goal), and the
            // kind + token are kept so a resend can repeat the admission id.
            if let Some(chat) = self.chats.get_mut(&employee) {
                chat.pending = Some(PendingDelivery::new(instruction, kind, token.clone()));
                if let Some(pending) = chat.pending.as_mut() {
                    pending.attempts = attempts;
                }
            }
            let session_id = session_id.clone();
            self.spawn_prompt(employee, session_id, prompt, kind, &token);
        }
    }

    /// Scan a freshly loaded transcript for finished assistant replies carrying
    /// hand-off directives and dispatch each one (once, by message id).
    ///
    /// A worker in an *active* parallel round is the one exception: its
    /// directives are not dispatched. The round collects its contribution and
    /// the aggregator owns the final answer, so a worker's `@user` must not
    /// deliver a partial result to the user, and its `@<teammate>` must not
    /// spawn a side-chain that competes with the round. The directive's own text
    /// still reaches the aggregator through `note_fanout_result`.
    fn dispatch_delegations(&mut self, employee: usize, messages: &[SessionMessage]) {
        let is_worker = !self.fanouts_awaiting(employee).is_empty();

        let new_delegations = {
            let Some(chat) = self.chats.get_mut(&employee) else {
                return;
            };
            let mut found = Vec::new();
            for message in messages {
                if message.role() != "assistant" || message.is_incomplete() {
                    continue;
                }
                if !chat.handled_message_ids.insert(message.id.clone()) {
                    continue;
                }
                found.extend(parse_delegations(&message.display_text()));
            }
            found
        };

        if is_worker && !new_delegations.is_empty() {
            self.push_log(format!(
                "fan-out: {} contributed instead of handing off",
                self.team
                    .name_at(employee)
                    .unwrap_or_else(|| employee.to_string())
            ));
            return;
        }

        for delegation in new_delegations {
            if self.delegation_count >= harness::DELEGATION_TOTAL_LIMIT {
                self.push_log("delegation limit reached — stopping the chain".to_string());
                return;
            }
            self.delegation_count += 1;
            self.dispatch_delegation(employee, delegation);
        }
    }

    /// Route one parsed hand-off: `@jev` asks the decision model, a final target
    /// delivers the answer back to the user, and anything else is relayed to the
    /// named teammate.
    fn dispatch_delegation(&mut self, source: usize, delegation: Delegation) {
        if is_jev(&delegation.target) {
            self.route_to_jev(source, delegation.message);
            return;
        }

        if is_parallel(&delegation.target) {
            self.fan_out(source, delegation.message);
            return;
        }

        if is_final(&delegation.target) {
            if delegation.message.is_empty() {
                return;
            }
            self.deliver(source, delegation.message);
            return;
        }

        let Some(target) = self.team_index_by_id(&delegation.target) else {
            self.push_log(format!("unknown delegate target '{}'", delegation.target));
            return;
        };
        // Bound the chain depth so `A → B → A → …` can't spin forever.
        let depth = self.chain_depth(source);
        if depth >= harness::MAX_CHAIN_DEPTH {
            self.push_log(format!(
                "delegation chain depth limit reached at '{}'",
                delegation.target
            ));
            return;
        }
        self.inherit_chain(source, target);
        self.trace(source, target, TraceKind::Out);
        self.trace_hop(
            crate::trace::Kind::Delegation,
            Some(source),
            Some(target),
            task_summary(&delegation.message),
        );
        self.send_to_employee(target, delegation.message, harness::PromptKind::Delegation);
    }

    /// A `@user` directive: deliver the employee's finished work to the chat the
    /// chain started from.
    ///
    /// The team is flat, so there is no root to report upward to — whoever was
    /// asked owns the answer, and its `@user` is the deliverable.
    fn deliver(&mut self, source: usize, message: String) {
        self.deliver_to_origin(source, message);
    }

    /// Put `message` in the chat the chain started from, as the final answer.
    fn deliver_to_origin(&mut self, source: usize, message: String) {
        let origin = self
            .origin_of
            .get(&source)
            .copied()
            .or(self.chat_employee)
            .unwrap_or(source);
        self.trace_hop(
            crate::trace::Kind::Deliver,
            Some(source),
            None,
            task_summary(&message),
        );
        self.push_to_chat(origin, TranscriptRole::Assistant, message);
        self.team.set_status(origin, EmployeeStatus::Done);
    }

    fn route_to_jev(&mut self, source: usize, question: String) {
        self.push_to_chat(source, TranscriptRole::Info, format!("🤖 @jev: {question}"));
        if self.jev.is_none() {
            self.push_to_chat(
                source,
                TranscriptRole::Assistant,
                FALLBACK_VERDICT.to_string(),
            );
            return;
        }
        let jev = self.jev.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let answer = answer_text_or_default(jev.as_ref(), &question).await;
            let _ = tx
                .send(Message::JevAnswer {
                    employee: source,
                    answer,
                })
                .await;
        });
    }

    /// Fan `goal` out to every teammate in parallel (each interprets it from its
    /// own role), then aggregate their outputs through one of them once all
    /// finish.
    ///
    /// The source and the aggregator are excluded from the worker set. The
    /// aggregator is the reviewer, then the planner, then the source itself —
    /// there is no root above the team to own the synthesis. The fallbacks must
    /// differ from the source too: aggregating *through the source* would make
    /// the round's synthesizer one of its own workers (and, for a reviewer
    /// source, put the review in the hands of the employee being reviewed).
    fn fan_out(&mut self, source: usize, goal: String) {
        let team = self.team.employees().to_vec();

        let aggregator = ["reviewer", "planner"]
            .iter()
            .find_map(|id| self.team_index_by_id(id).filter(|&index| index != source))
            .unwrap_or(source);

        let mut workers: Vec<usize> = (0..team.len())
            .filter(|&i| i != source && i != aggregator)
            .collect();
        workers.sort_unstable();

        if workers.is_empty() {
            self.push_log("fan-out: no other employees to delegate to".to_string());
            return;
        }

        let id = self.next_fanout_id;
        self.next_fanout_id += 1;
        let names = workers
            .iter()
            .map(|&i| team[i].name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        self.push_log(format!(
            "fan-out #{id} → {names} (review: {})",
            team[aggregator].name
        ));

        // Each worker (and the aggregator) inherits this chain's origin so the
        // eventual `@user` deliverable routes back to the chat that started it.
        for &worker in &workers {
            self.inherit_chain(source, worker);
        }
        if aggregator != source {
            self.inherit_chain(source, aggregator);
        }

        self.fanouts.insert(
            id,
            FanOut {
                workers: workers.clone(),
                aggregator,
                goal: goal.clone(),
                outputs: HashMap::new(),
                aggregated: false,
            },
        );

        for worker in workers {
            // Each worker gets the parallel-round brief, not the bare goal, so it
            // contributes its own part instead of handing off or answering as if
            // it owned the final deliverable.
            self.trace(source, worker, TraceKind::Fanout);
            self.send_to_employee(
                worker,
                workflow::fanout_brief(&goal),
                harness::PromptKind::FanoutBrief,
            );
        }
    }

    /// After a transcript finishes loading, record the employee's deliverable into
    /// any fan-out they belong to; once every worker has contributed, hand the
    /// synthesis to the aggregator.
    fn note_fanout_result(&mut self, employee: usize, messages: &[SessionMessage]) {
        // Record this worker's output into every fan-out they're still owed to.
        let affected = self.fanouts_awaiting(employee);

        if !affected.is_empty() {
            // `contribution` keeps a worker's plain text and recovers the
            // payload when the reply was *only* a directive — a worker that
            // ignored the brief still has its work counted.
            let output = last_assistant_output(messages).unwrap_or_default();
            let output = if output.is_empty() {
                // No text at all still counts as a contribution, so a fan-out
                // waiting on this worker can complete.
                harness::abandon_note(&self.team.name_at(employee).unwrap_or_default(), None)
            } else {
                output
            };
            for id in affected {
                if let Some(fanout) = self.fanouts.get_mut(&id) {
                    fanout.outputs.insert(employee, output.clone());
                }
            }
        }

        // Aggregate any fan-out whose workers have all reported.
        self.aggregate_ready_fanouts();
    }

    /// The ids of every fan-out still owed a contribution from `employee` — it
    /// lists them as a worker, has no output from them yet, and has not been
    /// aggregated. Both contribution paths (a real deliverable and a synthetic
    /// `⚠` note) go through this, so a worker is never counted twice or missed.
    fn fanouts_awaiting(&self, employee: usize) -> Vec<u64> {
        self.fanouts
            .iter()
            .filter(|(_, fanout)| {
                fanout.workers.contains(&employee)
                    && !fanout.outputs.contains_key(&employee)
                    && !fanout.aggregated
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Synthesize a fan-out's collected outputs and send them to the aggregator,
    /// instructing it to return a single deliverable to the user.
    fn aggregate_fanout(&mut self, id: u64) {
        let (aggregator, prompt) = {
            let Some(fanout) = self.fanouts.get_mut(&id) else {
                return;
            };
            if fanout.aggregated {
                return;
            }
            fanout.aggregated = true;

            let mut prompt = String::from("The team has worked on this goal in parallel:\n");
            prompt.push_str(&format!("GOAL: {}\n", fanout.goal));
            for worker in &fanout.workers {
                let name = self.team.name_at(*worker).unwrap_or_default();
                let output = fanout.outputs.get(worker).cloned().unwrap_or_default();
                prompt.push_str(&format!("\n## {name}\n{output}\n"));
            }
            prompt.push_str(
                "\nSynthesize these into ONE cohesive final deliverable for the user, \
                 resolving overlaps and filling gaps. No further delegation — deliver \
                 the final result with a single line:\n@user <final deliverable>",
            );
            (fanout.aggregator, prompt)
        };

        self.push_log(format!(
            "fan-out #{id} complete — aggregating via {}",
            self.team.name_at(aggregator).unwrap_or_default()
        ));
        // The synthesis is the round coming back together: draw it as a report
        // from the round's workers into the aggregator.
        for worker in self
            .fanouts
            .get(&id)
            .map(|f| f.workers.clone())
            .unwrap_or_default()
        {
            self.trace(worker, aggregator, TraceKind::Report);
        }
        self.send_to_employee(aggregator, prompt, harness::PromptKind::Delegation);
    }

    // --- reliability (timeouts, retry, per-chain isolation) ---

    /// The chat a chain's final answer returns to. A chain with no origin
    /// recorded is its own origin.
    fn chain_origin(&self, employee: usize) -> usize {
        self.origin_of.get(&employee).copied().unwrap_or(employee)
    }

    /// How deep `employee` sits in its delegation chain (0 = a user-prompted
    /// origin). An employee the chain maps have never seen counts as an origin.
    fn chain_depth(&self, employee: usize) -> usize {
        self.depth_of.get(&employee).copied().unwrap_or(0)
    }

    /// Record that `target` now belongs to the same delegation chain as `source`,
    /// inheriting its origin (the chat the final answer returns to) and bumping
    /// the chain depth one step.
    fn inherit_chain(&mut self, source: usize, target: usize) {
        let origin = self.chain_origin(source);
        let depth = self.chain_depth(source).saturating_add(1);
        self.origin_of.insert(target, origin);
        self.depth_of.insert(target, depth);
    }

    /// Scan pending hand-offs for any that have gone silent past the timeout,
    /// retrying each (up to the budget) or abandoning it so a fan-out can't hang.
    fn tick_reliability(&mut self) {
        let now = Instant::now();
        let expired: Vec<usize> = self
            .chats
            .iter()
            .filter(|(_, chat)| chat.pending.as_ref().is_some_and(|p| p.expired(now)))
            .map(|(&employee, _)| employee)
            .collect();
        for employee in expired {
            self.retry_or_abandon(employee, "timed out", false);
        }
    }

    /// A hand-off failed or timed out: retry it (bounded) or abandon it.
    ///
    /// `resend` distinguishes the two causes, because they are not the same
    /// operation and the admission id is what tells them apart:
    ///
    /// - **`resend: true` — the delivery never landed.** A transport failure means
    ///   the service rejected or could not accept the prompt, so the delivery is
    ///   *re-offered under its original identity*: same kind, same token, same
    ///   session, therefore the same admission id. If the first attempt was in
    ///   fact admitted, the service deduplicates this one; if it was not, this is
    ///   the first admission. Either way the work runs once.
    /// - **`resend: false` — a silent worker.** The previous attempt was admitted
    ///   and then went quiet, so asking again is new work and must produce a new
    ///   admission. Reusing the identity here would be silently swallowed as a
    ///   duplicate and the worker would never be asked again; a fresh token is
    ///   minted instead.
    fn retry_or_abandon(&mut self, employee: usize, reason: &str, resend: bool) {
        let exhausted = self
            .chats
            .get(&employee)
            .and_then(|c| c.pending.as_ref())
            .is_some_and(|p| p.exhausted());

        if exhausted {
            let instruction = self
                .chats
                .get(&employee)
                .and_then(|c| c.pending.as_ref())
                .map(|p| p.instruction.clone())
                .unwrap_or_default();
            if let Some(chat) = self.chats.get_mut(&employee) {
                chat.pending = None;
            }
            self.abandon(employee, &instruction, reason);
            return;
        }

        // Bump the stored attempt count and re-send the original instruction. The
        // follow-on `flush_outbox` preserves `attempts` and prepends the retry
        // preamble, so a repeatedly-silent worker can't loop forever.
        let (instruction, attempts, kind, token) = match self
            .chats
            .get_mut(&employee)
            .and_then(|c| c.pending.as_mut())
        {
            Some(pending) => {
                pending.bump();
                (
                    pending.instruction.clone(),
                    pending.attempts,
                    pending.kind,
                    pending.token.clone(),
                )
            }
            None => return,
        };

        self.push_to_chat(
            employee,
            TranscriptRole::Info,
            format!("🔁 retrying ({reason})"),
        );
        self.trace_hop(
            crate::trace::Kind::Retry,
            None,
            Some(employee),
            format!("attempt {attempts} of {} — {reason}", harness::MAX_ATTEMPTS),
        );

        if resend {
            // Re-offer the *same* delivery: same kind, same token, same session.
            self.resend_to_employee(employee, instruction, kind, token);
        } else {
            self.send_to_employee(employee, instruction, harness::PromptKind::Retry);
        }
    }

    /// Give up on `employee`'s hand-off and record a deterministic `⚠` deliverable
    /// so any fan-out it belongs to still aggregates.
    fn abandon(&mut self, employee: usize, _instruction: &str, reason: &str) {
        let name = self
            .team
            .name_at(employee)
            .unwrap_or_else(|| employee.to_string());
        let note = harness::abandon_note(&name, Some(reason));
        self.push_to_chat(employee, TranscriptRole::Error, note.clone());
        self.team.set_status(employee, EmployeeStatus::Error);
        self.trace_hop(
            crate::trace::Kind::Abandon,
            None,
            Some(employee),
            format!("{name}: {reason}"),
        );

        // Surface the failure at the chain origin so the user sees it. A chain with
        // no origin recorded, or whose origin is this very employee, has no other
        // chat to report to.
        let origin = self.chain_origin(employee);
        if origin != employee {
            self.push_to_chat(
                origin,
                TranscriptRole::Error,
                format!("{name} could not complete the hand-off ({reason})."),
            );
        }
        self.force_worker_contribution(employee, note);
    }

    /// Record a synthetic deliverable for `employee` into any fan-out still
    /// waiting on it, then aggregate anything that became complete.
    fn force_worker_contribution(&mut self, employee: usize, note: String) {
        for id in self.fanouts_awaiting(employee) {
            if let Some(fanout) = self.fanouts.get_mut(&id) {
                fanout.outputs.insert(employee, note.clone());
            }
        }
        self.aggregate_ready_fanouts();
    }

    /// Aggregate every fan-out whose workers have all reported (real or `⚠`).
    fn aggregate_ready_fanouts(&mut self) {
        let ready: Vec<u64> = self
            .fanouts
            .iter()
            .filter(|(_, f)| !f.aggregated && f.workers.iter().all(|w| f.outputs.contains_key(w)))
            .map(|(id, _)| *id)
            .collect();
        for id in ready {
            self.aggregate_fanout(id);
        }
    }

    /// A delegated (or user-facing) prompt delivery failed.
    fn handle_delivery_failed(&mut self, target: usize, reason: String) {
        let name = self
            .team
            .name_at(target)
            .unwrap_or_else(|| target.to_string());
        self.push_to_chat(
            target,
            TranscriptRole::Error,
            format!("delivery failed: {reason}"),
        );
        self.trace_hop(
            crate::trace::Kind::Failed,
            None,
            Some(target),
            format!("delivery failed: {reason}"),
        );
        self.push_error_log(format!("delivery to {name} failed: {reason}"));

        // Retry only autonomous (delegated) hand-offs, which carry a pending
        // delivery; a direct user prompt just surfaces the error for a manual retry.
        let has_pending = self
            .chats
            .get(&target)
            .is_some_and(|chat| chat.pending.is_some());
        if has_pending {
            // A transport failure never landed, so this is a resend of the same
            // delivery: it keeps its identity and is deduplicated if the service
            // did in fact admit the first attempt.
            self.retry_or_abandon(target, &reason, true);
        } else {
            self.team.set_status(target, EmployeeStatus::Error);
        }

        // A failed delivery ends the turn; let the next queued prompt through.
        self.pump_employee_queue(target);
    }

    // --- autonomous tasks (cron schedules + file watches) ---

    /// An autonomous trigger (a cron schedule or a file watch) fired: record a
    /// task and dispatch its first attempt. `label` prefixes the log line so the
    /// trigger source stays visible.
    ///
    /// While the engine is paused (AC1) the fire is still recorded, but it lands
    /// as `pending` and starts no session — the scheduler and watcher are
    /// untouched and simply keep firing into a queue.
    fn handle_autonomous_task(&mut self, label: &str, employee_id: String, goal: String) {
        let Some(index) = self.team_index_by_id(&employee_id) else {
            self.push_log(format!(
                "{label} for unknown employee '{employee_id}' — skipped"
            ));
            return;
        };

        let summary = task_summary(&goal);
        let mut task = Task::new(goal.clone(), Some(employee_id.clone()));

        if self.store.paused {
            self.store.upsert_task(task);
            self.persist_store();
            self.push_log(format!(
                "{label} for {employee_id}: {summary} — ⏸ queued (paused)"
            ));
            return;
        }

        task.transition(TaskStatus::Running);
        self.store.upsert_task(task.clone());
        self.persist_store();

        self.team.set_status(index, EmployeeStatus::Thinking);
        self.team.set_last_task(index, Some(summary.clone()));
        self.push_log(format!("{label} for {employee_id}: {summary}"));

        self.dispatch_task(&task, None);
    }

    /// Spawn one delivery attempt for `task` in the background. `preamble`, when
    /// set, is prepended to the goal (a retry or a resume note).
    fn dispatch_task(&mut self, task: &Task, preamble: Option<String>) {
        let Some(client) = self.client.clone() else {
            self.push_log("scheduled task: not connected to OpenCode".to_string());
            return;
        };
        let Some(employee_id) = task.assigned_to.clone() else {
            return;
        };
        let Some(index) = self.team_index_by_id(&employee_id) else {
            return;
        };

        let directory = self.workdir();
        let employee = &self.team.employees()[index];
        let agent = employee.agent_id().to_owned();
        let model = employee.model_ref();
        // A scheduled job runs outside any chain, so it is briefed as an isolated
        // job — see `run_task_attempt`.
        let context = workflow::team_context(&self.team, &employee.name);
        let goal = task.goal.clone();
        let task_id = task.id;
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let outcome = run_task_attempt(
                &client,
                &directory,
                Some(agent.as_str()),
                model.as_ref(),
                &goal,
                preamble.as_deref(),
                task_id,
                tx.clone(),
                &context,
            )
            .await;
            let (succeeded, result) = match outcome {
                Ok(result) => (true, result),
                Err(reason) => (false, reason),
            };
            let _ = tx
                .send(Message::TaskAttemptFinished {
                    task_id,
                    succeeded,
                    result,
                })
                .await;
        });
    }

    /// Record the session an in-flight task attempt is running in, so a pause
    /// can interrupt it (AC3). A task already paused when its attempt reports in
    /// is interrupted instead of adopted — the pause fired before the session
    /// existed and must not be undone.
    fn note_task_attempt(&mut self, task_id: TaskId, session_id: String) {
        let Some(position) = self.store.tasks.iter().position(|t| t.id == task_id) else {
            return;
        };
        if self.store.tasks[position].paused {
            self.interrupt_task_session(&session_id);
            return;
        }
        self.store.tasks[position].session_id = Some(session_id);
        self.persist_store();
    }

    /// Handle the outcome of one scheduled-task attempt: succeed, retry (bounded),
    /// or fail — persisting the task and updating the roster status each time.
    ///
    /// A task the user paused mid-flight is left alone: the interrupted
    /// session's trailing completion must not flip it back to a terminal state
    /// (the AC3 race guard).
    fn handle_task_attempt(&mut self, task_id: TaskId, succeeded: bool, result: String) {
        let Some(position) = self.store.tasks.iter().position(|t| t.id == task_id) else {
            return;
        };

        if self.store.tasks[position].paused {
            self.push_log(format!(
                "⏸ ignored late attempt for paused task: {}",
                task_summary(&self.store.tasks[position].goal)
            ));
            return;
        }

        let assigned = self.store.tasks[position].assigned_to.clone();
        let summary = task_summary(&self.store.tasks[position].goal);

        let mut retry = false;
        let mut status = EmployeeStatus::Error;
        if succeeded {
            self.store.tasks[position].succeed(result);
            status = EmployeeStatus::Done;
            self.push_log(format!("✓ scheduled task done: {summary}"));
        } else if self.store.tasks[position].try_retry() {
            status = EmployeeStatus::Thinking;
            retry = true;
            self.push_log(format!("🔁 scheduled task retrying: {summary} ({result})"));
        } else {
            self.store.tasks[position].fail(result);
            self.push_log(format!("✕ scheduled task failed: {summary}"));
        }

        if let Some(employee_id) = &assigned {
            self.team.set_status_by_id(employee_id, status);
        }
        self.persist_store();

        if retry {
            let task = self.store.tasks[position].clone();
            let preamble = Some(harness::retry_preamble(task.attempts));
            self.dispatch_task(&task, preamble);
        }
    }

    // --- pause & resume (the engine gate and per-task control) ---

    /// Toggle the engine gate (`Space`): while paused, autonomous fires queue as
    /// `pending` instead of starting a session. Resuming dispatches whatever
    /// queued up (AC2).
    fn toggle_engine_pause(&mut self) {
        self.store.paused = !self.store.paused;
        if self.store.paused {
            self.push_log("⏸ engine paused — new fires will queue".to_string());
        } else {
            self.push_log("▶ engine resumed".to_string());
            self.resume_pending();
        }
        self.persist_store();
    }

    /// Dispatch the tasks the engine held while paused, in the queue panel's own
    /// order (`order_tasks` — active work first, newest-first; AC2). Individually
    /// paused tasks are left for the user to resume.
    fn resume_pending(&mut self) {
        for position in self.resumable_tasks() {
            let mut task = self.store.tasks[position].clone();
            if !task.transition(TaskStatus::Running) {
                continue;
            }
            self.store.upsert_task(task.clone());
            if let Some(employee_id) = &task.assigned_to {
                self.team
                    .set_status_by_id(employee_id, EmployeeStatus::Thinking);
                self.team
                    .set_last_task_by_id(employee_id, Some(task_summary(&task.goal)));
            }
            self.push_log(format!(
                "▶ dispatching queued task: {}",
                task_summary(&task.goal)
            ));
            self.dispatch_task(&task, None);
        }
        self.persist_store();
    }

    /// The indices [`Self::resume_pending`] should dispatch: queued tasks that
    /// the user hasn't individually paused, in `order_tasks` order.
    fn resumable_tasks(&self) -> Vec<usize> {
        order_tasks(&self.store.tasks)
            .into_iter()
            .filter(|&index| {
                let task = &self.store.tasks[index];
                task.status == TaskStatus::Pending && !task.paused
            })
            .collect()
    }

    /// Toggle pause on the selected employee's current task (`s`): pause a
    /// queued or running one, or resume a paused one (AC3/AC4).
    fn toggle_task_pause(&mut self) {
        let Some(employee) = self.team.employees().get(self.selected) else {
            return;
        };
        let employee_id = employee.id.clone();
        let Some(position) = self.current_task_for(&employee_id) else {
            self.push_log(format!("no task to pause for {employee_id}"));
            return;
        };
        if self.store.tasks[position].paused {
            self.resume_task(position);
        } else {
            self.pause_task(position);
        }
    }

    /// The newest active task assigned to `employee_id` — the one `s` toggles.
    fn current_task_for(&self, employee_id: &str) -> Option<usize> {
        self.store
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| {
                task.assigned_to.as_deref() == Some(employee_id)
                    && matches!(
                        task.status,
                        TaskStatus::Pending | TaskStatus::Running | TaskStatus::Paused
                    )
            })
            .max_by_key(|(_, task)| task.created_at)
            .map(|(index, _)| index)
    }

    /// Pause one task: interrupt its session (when one exists) and hold it. Its
    /// employee goes idle — the task is no longer working.
    fn pause_task(&mut self, position: usize) {
        let summary = task_summary(&self.store.tasks[position].goal);
        if !self.store.tasks[position].pause() {
            return;
        }
        if let Some(session_id) = self.store.tasks[position].session_id.clone() {
            self.interrupt_task_session(&session_id);
        }
        if let Some(employee_id) = self.store.tasks[position].assigned_to.clone() {
            self.team
                .set_status_by_id(&employee_id, EmployeeStatus::Idle);
        }
        self.push_log(format!("⏸ paused task: {summary}"));
        self.persist_store();
    }

    /// Resume one task: back to `running` and dispatched again with the resume
    /// preamble (AC4).
    fn resume_task(&mut self, position: usize) {
        let summary = task_summary(&self.store.tasks[position].goal);
        if !self.store.tasks[position].resume() {
            return;
        }
        if let Some(employee_id) = self.store.tasks[position].assigned_to.clone() {
            self.team
                .set_status_by_id(&employee_id, EmployeeStatus::Thinking);
            self.team
                .set_last_task_by_id(&employee_id, Some(summary.clone()));
        }
        self.persist_store();
        self.push_log(format!("▶ resumed task: {summary}"));
        let task = self.store.tasks[position].clone();
        // The "start over" preamble only makes sense when there was an earlier
        // attempt to start over *from*: a task that never reached a session is
        // re-run with its bare goal.
        let preamble = task.session_id.is_some().then(harness::resume_preamble);
        self.dispatch_task(&task, preamble);
    }

    /// Interrupt an autonomous task's session, best-effort.
    fn interrupt_task_session(&mut self, session_id: &str) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let session_id = session_id.to_string();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(err) = client.interrupt(&session_id).await {
                let _ = tx
                    .send(Message::Log(format!("interrupt failed: {err}")))
                    .await;
            }
        });
    }

    // --- the shutdown / reopen path ---

    /// Hold every task that is still active and interrupt the sessions they were
    /// running in, so nothing keeps working once the terminal is gone.
    ///
    /// The ordering is the point: each task is paused *before* its session is
    /// interrupted, so the interrupt's trailing `TaskAttemptFinished` meets the
    /// paused-task guard in [`Self::handle_task_attempt`] and is ignored rather
    /// than flipping the task back to a terminal state.
    ///
    /// Terminal tasks are skipped — [`Task::pause_for`] refuses them — and a task
    /// the user had already paused keeps its own reason, so a deliberate pause is
    /// never offered back on reopen.
    fn pause_active_tasks(&mut self) {
        let mut sessions = Vec::new();
        let mut held = 0usize;

        for position in order_tasks(&self.store.tasks) {
            let task = &mut self.store.tasks[position];
            if !task.pause_for(PauseReason::Shutdown) {
                continue;
            }
            held += 1;
            if let Some(session_id) = task.session_id.clone() {
                sessions.push(session_id);
            }
            if let Some(employee_id) = task.assigned_to.clone() {
                self.team
                    .set_status_by_id(&employee_id, EmployeeStatus::Idle);
            }
        }

        for session_id in sessions {
            self.interrupt_task_session(&session_id);
        }

        if held > 0 {
            self.push_log(format!("⏸ paused {held} task(s) at shutdown"));
        }
    }

    /// Collect the tasks mywaker itself paused on the way out into the reopen
    /// prompt, in the task panel's own order (`order_tasks`).
    ///
    /// Runs once, right after the store is loaded. A normal launch — nothing
    /// shutdown-paused — leaves the prompt unset, so nothing is shown; a task the
    /// user paused deliberately is not collected.
    fn prepare_resume_prompt(&mut self) {
        let tasks: Vec<TaskId> = order_tasks(&self.store.tasks)
            .into_iter()
            .map(|index| &self.store.tasks[index])
            .filter(|task| task.paused_at_shutdown())
            .map(|task| task.id)
            .collect();

        self.resume_prompt = (!tasks.is_empty()).then_some(ResumePrompt { tasks });
    }

    /// Resume every task the prompt covers, in the prompt's own order, through
    /// the ordinary per-task resume path (`resume_task` → `dispatch_task`).
    ///
    /// Each task therefore starts a **fresh** OpenCode session: resume re-runs the
    /// task, it does not continue the interrupted session.
    fn resume_shutdown_paused(&mut self) {
        let Some(prompt) = self.resume_prompt.take() else {
            return;
        };
        for task_id in prompt.tasks {
            let Some(position) = self.store.tasks.iter().position(|task| task.id == task_id) else {
                continue;
            };
            self.resume_task(position);
        }
    }

    /// Keep the shutdown-paused tasks held: drop the prompt and say how to
    /// continue one later. Declining does not touch the engine gate — new cron
    /// and watch fires still dispatch.
    fn dismiss_resume_prompt(&mut self) {
        let Some(prompt) = self.resume_prompt.take() else {
            return;
        };
        let count = prompt.tasks.len();
        let plural = if count == 1 { "" } else { "s" };
        self.push_log(format!(
            "⏸ kept {count} task{plural} paused (press s to resume)"
        ));
    }

    /// Persist the store to its configured path (a no-op when persistence is
    /// disabled, e.g. in unit tests).
    fn persist_store(&mut self) {
        let Some(path) = &self.store_path else {
            return;
        };
        if let Err(err) = self.store.save(path) {
            self.push_error_log(format!("store save failed: {err}"));
        }
    }

    /// Open the model picker and fetch the models available through OpenCode's
    /// providers so the user can choose one for the selected employee.
    fn open_models(&mut self) {
        if self.client.is_none() {
            self.push_log("not connected to OpenCode".to_string());
            return;
        }
        self.model_selected = 0;
        self.model_filter.clear();
        self.mode = Mode::Models;
        self.load_models();
    }

    /// Fetch the models available through OpenCode's providers (for the model
    /// picker). Results arrive back via [`Message::ModelsLoaded`].
    fn load_models(&self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.models().await {
                Ok(models) => {
                    let _ = tx.send(Message::ModelsLoaded(models)).await;
                }
                Err(err) => {
                    let _ = tx
                        .send(Message::LogError(format!("model list failed: {err}")))
                        .await;
                }
            }
        });
    }

    /// Edit the model-picker search query: `Some(c)` appends a character,
    /// `None` pops the last one. Resets the highlight to the first match.
    fn edit_model_filter(&mut self, ch: Option<char>) {
        match ch {
            Some(c) => self.model_filter.push(c),
            None => {
                self.model_filter.pop();
            }
        }
        self.model_selected = 0;
    }

    /// Indices into [`Self::models`] matching the picker's search query — a
    /// trimmed, case-insensitive substring across the id, display name, provider
    /// id, and vendor (the id's first path segment). An empty query returns
    /// every model in original order.
    pub fn filtered_models(&self) -> Vec<usize> {
        let query = self.model_filter.trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..self.models.len()).collect();
        }
        self.models
            .iter()
            .enumerate()
            .filter_map(|(index, model)| {
                let name = model.name.as_deref().unwrap_or_default();
                let provider = model.provider_id.as_deref().unwrap_or_default();
                let vendor = model.vendor();
                let haystack = format!("{} {} {} {}", model.id, name, provider, vendor);
                haystack
                    .to_ascii_lowercase()
                    .contains(&query)
                    .then_some(index)
            })
            .collect()
    }

    /// The model catalog entry currently highlighted, resolving the filtered
    /// picker index back to the underlying catalog entry.
    fn current_model(&self) -> Option<&ProviderModel> {
        let index = *self.filtered_models().get(self.model_selected)?;
        self.models.get(index)
    }

    /// The model id currently highlighted in the picker.
    #[cfg(test)]
    fn selected_model_id(&self) -> Option<String> {
        self.current_model().map(|m| m.id.clone())
    }

    /// Assign the highlighted model to the currently selected employee,
    /// persisting the roster (id + provider) and returning to the dashboard.
    fn assign_selected_model(&mut self) {
        let Some(model) = self.current_model() else {
            return;
        };
        let id = model.id.clone();
        let provider = model.provider_id.clone().unwrap_or_else(|| model.vendor());
        let employee = self.selected;
        self.team.set_model(employee, id.clone(), Some(provider));
        self.push_log(format!(
            "{} → model {}",
            self.team
                .employees()
                .get(employee)
                .map(|e| e.name.as_str())
                .unwrap_or("employee"),
            id,
        ));
        if let Some(path) = Team::default_path()
            && let Err(err) = self.team.save(&path)
        {
            self.push_error_log(format!("could not save team: {err}"));
        }
        self.mode = Mode::Roster;
    }

    /// Backfill each employee's `model_provider` from the loaded catalog when a
    /// legacy assignment (a bare model id) predates the provider field, so
    /// `Employee::model_ref` resolves the real provider instead of guessing. The
    /// kenari mirror is preferred when an id is ambiguous. Persists only when
    /// something changed.
    fn backfill_model_providers(&mut self) {
        let mut changed = false;
        for employee in self.team.employees_mut() {
            if employee.model_provider.is_none()
                && let Some(model_id) = employee.model.as_deref()
                && let Some(found) = pick_provider(&self.models, model_id)
            {
                employee.model_provider =
                    Some(found.provider_id.clone().unwrap_or_else(|| found.vendor()));
                changed = true;
            }
        }
        if changed
            && let Some(path) = Team::default_path()
            && let Err(err) = self.team.save(&path)
        {
            self.push_error_log(format!("could not save team: {err}"));
        }
    }

    fn workdir(&self) -> String {
        if let Some(project) = &self.active_project {
            return project.path.to_string_lossy().into_owned();
        }
        self.config
            .workdir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .to_string_lossy()
            .into_owned()
    }

    // --- workflow ordering & the roster grid ---

    /// Employee storage indices in canonical workflow order (plan → research →
    /// write → review → general), stable by name within a stage. This is the
    /// whole roster, and it is what the *tasks* panel lists.
    pub fn workflow_order(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.team.len()).collect();
        indices.sort_by_key(|&i| {
            let employee = &self.team.employees()[i];
            (employee.workflow_role().order(), employee.name.clone())
        });
        indices
    }

    /// Employee storage indices the roster **grid** draws, in card order: the
    /// workflow order. Every employee is a card — there is no root above the
    /// grid to draw separately.
    ///
    /// The *drawing* splits this into two sections (active employees first, then
    /// idle); this flat order is what the arrow keys walk and what the tasks panel
    /// lists, so the two views can't drift.
    pub fn card_order(&self) -> Vec<usize> {
        self.workflow_order()
    }

    /// The roster split into the grid's two sections: employees mid-turn on top,
    /// everyone else below.
    ///
    /// Both lists keep workflow order, so a card's position within its section is
    /// stable as statuses change — an employee moving between sections is the one
    /// thing that re-orders the panel, which is exactly what the split is for.
    pub(crate) fn card_sections(&self) -> pipeline::Sections {
        let employees = self.team.employees();
        pipeline::Sections::new(&self.card_order(), |index| {
            employees
                .get(index)
                .is_some_and(|employee| employee.is_busy())
        })
    }

    /// Move the roster cursor one cell in the grid, revealing the card it lands
    /// on.
    ///
    /// The grid is read left-to-right, top-to-bottom *within each section* (the
    /// active employees above the idle ones), so `delta` walks that order; the
    /// arrow keys call this with `±1` (←/→) and `±cols` (↑/↓), which keeps the
    /// column fixed.
    fn move_selection_grid(&mut self, delta: isize) {
        let order = self.card_sections().flat();
        if order.is_empty() {
            return;
        }
        let position = order.iter().position(|&i| i == self.selected).unwrap_or(0);
        let next = (position as isize + delta).clamp(0, order.len() as isize - 1) as usize;
        self.selected = order[next];
        self.reveal_selection();
    }

    /// Move the roster cursor by `delta` cells.
    fn move_selection(&mut self, delta: isize) {
        self.move_selection_grid(delta);
    }

    /// Scroll the roster grid by `delta` rows, clamped to
    /// `[0, roster_max_scroll]`. This moves the viewport only: the selection
    /// stays where the user put it, so scrolling never changes what `Enter`
    /// opens.
    fn scroll_roster(&mut self, delta: isize) {
        let max = self.roster_max_scroll();
        // Start from the clamped offset, not the raw field: after a resize the
        // stored value can be past the last page, and one notch up should land on
        // the last page, not one short of it.
        let next = (self.roster_scroll() as isize + delta).clamp(0, max as isize);
        self.roster_scroll = next as usize;
    }

    /// Bring the selected card inside the viewport, scrolling the minimum
    /// distance. Called after every selection move, so the keyboard can never
    /// move the cursor somewhere the user cannot see.
    fn reveal_selection(&mut self) {
        let sections = self.card_sections();
        let Some(position) = sections.flat().iter().position(|&i| i == self.selected) else {
            return;
        };
        self.roster_scroll =
            ui::reveal_roster(self.last_size, &sections, position, self.roster_scroll);
    }

    /// Columns in the roster grid at the current terminal size — the `↑`/`↓`
    /// step, so a vertical move keeps the column.
    fn roster_cols(&self) -> isize {
        ui::roster_cols(self.last_size) as isize
    }

    /// The `PgUp`/`PgDn` step, in cells: one viewport of card rows.
    fn roster_page(&self) -> isize {
        ui::roster_page(self.last_size, &self.card_sections()) as isize
    }

    // --- project discovery ---

    /// Refresh the project list and open the picker, starting on the active
    /// project (if it is still present).
    fn open_projects(&mut self) {
        self.project_roots = projects::scan_roots();
        self.projects = projects::discover_all().unwrap_or_default();
        self.project_selected = self
            .active_project
            .as_ref()
            .and_then(|active| self.projects.iter().position(|p| p.path == active.path))
            .unwrap_or(0);
        if self.projects.is_empty() {
            let roots = self
                .project_roots
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            self.push_log(format!("no projects under {roots}"));
        }
        self.mode = Mode::Projects;
    }

    /// Adopt the highlighted project as the session working directory.
    fn select_project(&mut self) {
        if let Some(project) = self.projects.get(self.project_selected).cloned() {
            self.push_log(format!("project → {}", project.name));
            self.active_project = Some(project);
        }
        self.mode = Mode::Roster;
    }

    fn push_log(&mut self, line: String) {
        let line = self.traced(line);
        self.log.push(LogLine::info(line));
        if self.log.len() > LOG_LIMIT {
            let excess = self.log.len() - LOG_LIMIT;
            self.log.drain(..excess);
        }
    }

    /// [`Self::push_log`] for a failure: the line is tagged so the panel renders
    /// it in the error colour, and the log panel leads with the latest error
    /// rather than burying it behind the telemetry lines.
    fn push_error_log(&mut self, line: String) {
        let line = self.traced(line);
        self.log.push(LogLine::error(line));
        if self.log.len() > LOG_LIMIT {
            let excess = self.log.len() - LOG_LIMIT;
            self.log.drain(..excess);
        }
    }

    /// Prefix `line` with the active trace id, when there is one.
    ///
    /// Every log line the orchestration produces is stamped, so a line pasted
    /// out of the panel names the run it came from. Lines emitted before any
    /// goal exists (connection chatter) stay unprefixed — there is no trace to
    /// attribute them to, and inventing one would make the id meaningless.
    fn traced(&self, line: String) -> String {
        match self.active_trace {
            Some(id) if self.orchestration.get(id).is_some() => format!("t{id} · {line}"),
            _ => line,
        }
    }

    // --- orchestration tracing ---

    /// Start a trace for a new goal and make it the active one.
    ///
    /// The goal hop is recorded here rather than by the caller so it can never
    /// be recorded twice: the trace's first span is the goal entering the team,
    /// with the employee it was routed to.
    fn begin_trace(&mut self, goal: &str, to: Option<usize>) -> crate::trace::TraceId {
        let id = self.orchestration.begin(task_summary(goal));
        self.active_trace = Some(id);
        let label = to.and_then(|index| self.employee_label(index));
        self.orchestration.record(
            id,
            crate::trace::Kind::Goal,
            None,
            label,
            task_summary(goal),
        );
        id
    }

    /// Record one hop under the active trace, if there is one.
    ///
    /// An untraced hop (background traffic before any goal, or a chain whose
    /// trace has aged out of the window) is silently skipped rather than
    /// resurrecting a pruned record.
    fn trace_hop(
        &mut self,
        kind: crate::trace::Kind,
        from: Option<usize>,
        to: Option<usize>,
        detail: impl Into<String>,
    ) {
        let Some(id) = self.active_trace else {
            return;
        };
        let from = from.and_then(|index| self.employee_label(index));
        let to = to.and_then(|index| self.employee_label(index));
        self.orchestration.record(id, kind, from, to, detail);
    }

    /// The name an employee is known by in a trace: its id (the thing a
    /// delegation directive names), falling back to its index when the roster
    /// has no such entry.
    fn employee_label(&self, index: usize) -> Option<String> {
        self.team
            .employees()
            .get(index)
            .map(|employee| employee.id.clone())
            .or_else(|| Some(index.to_string()))
    }

    /// The orchestration traces, for the UI and for tests.
    pub fn orchestration(&self) -> &crate::trace::Traces {
        &self.orchestration
    }

    /// The active trace id, when a goal is being traced.
    pub fn active_trace(&self) -> Option<crate::trace::TraceId> {
        self.active_trace
    }

    // --- accessors for the UI ---

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn team(&self) -> &Team {
        &self.team
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The roster grid's scroll offset, in whole card rows, clamped to what the
    /// current terminal size and roster allow.
    ///
    /// The stored value is a hint: a resize or a shorter team can leave it past
    /// the last page, so every reader goes through this clamp rather than the
    /// raw field. That keeps the drawing, hit-testing, and the accessor itself
    /// in agreement after the window changes size.
    pub fn roster_scroll(&self) -> usize {
        self.roster_scroll.min(self.roster_max_scroll())
    }

    /// The largest scroll offset the roster grid accepts right now — the last
    /// page that still shows whole bands. `0` when everything fits.
    pub fn roster_max_scroll(&self) -> usize {
        ui::roster_max_scroll(self.last_size, &self.card_sections())
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Whether the app is mid graceful-shutdown (rendering the shutdown overlay).
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    /// Shutdown overlay animation frame (driven locally, not by the tick).
    pub fn shutdown_frame(&self) -> u64 {
        self.shutdown_frame
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether the full-screen help overlay is showing.
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    /// Whether the roster's prompt bar has the keyboard.
    pub fn prompt_focused(&self) -> bool {
        self.prompt_focused
    }

    /// The employee the roster prompt bar speaks to, for its border: the name of
    /// the selected card.
    pub fn prompt_employee_name(&self) -> Option<&str> {
        self.team
            .employees()
            .get(self.selected)
            .map(|employee| employee.name.as_str())
    }

    /// The prompt employee's live status, for the prompt bar's border.
    pub fn prompt_employee_status(&self) -> Option<EmployeeStatus> {
        self.team
            .employees()
            .get(self.selected)
            .map(|employee| employee.status)
    }

    /// The prompt employee's assigned model, when it has one.
    pub fn prompt_employee_model(&self) -> Option<&str> {
        self.team
            .employees()
            .get(self.selected)
            .and_then(|employee| employee.model.as_deref())
    }

    /// How many employees are mid-turn — the work currently running, whoever it
    /// belongs to.
    pub fn delegating_count(&self) -> usize {
        self.team
            .employees()
            .iter()
            .filter(|employee| employee.is_busy())
            .count()
    }

    /// True while the prompt employee is thinking — the prompt band grows a row
    /// for the indicator then.
    pub fn prompt_employee_is_thinking(&self) -> bool {
        self.prompt_employee_status() == Some(EmployeeStatus::Thinking)
    }

    /// The prompt employee's pinned thinking row for the prompt band, or `None`
    /// when it is not thinking (or the `[ui] thinking` mode hides it).
    pub fn prompt_employee_thinking_line(
        &self,
        width: usize,
    ) -> Option<ratatui::text::Line<'static>> {
        let employee = self.selected;
        if self.team.employees().get(employee)?.status != EmployeeStatus::Thinking {
            return None;
        }
        // The employee's thinking lives in its own chat, whether or not that chat
        // is the one on screen — the prompt bar always speaks for the card.
        let thinking = self.chats.get(&employee).map(|chat| &chat.thinking);
        self.thinking_line_for(thinking, width)
    }

    /// The hand-offs still being drawn, oldest first.
    pub fn traces(&self) -> &[DelegationTrace] {
        &self.traces
    }

    /// Mutable access to the roster, for tests that need a roster the app's own
    /// commands refuse to build (an empty one, say).
    #[cfg(test)]
    pub fn team_mut_for_test(&mut self) -> &mut Team {
        &mut self.team
    }

    /// Record a hand-off trace directly.
    ///
    /// The render tests need a packet on the wires without standing up a whole
    /// delegation chain (which needs a client, sessions and transcripts), and the
    /// trace list is otherwise written only by the dispatch paths.
    pub fn record_trace_for_test(&mut self, from: usize, to: usize) {
        self.trace(from, to, TraceKind::Out);
    }

    /// Cursor position within the input, in grapheme clusters (0..=len).
    pub fn input_cursor(&self) -> usize {
        self.input_cursor
    }

    /// Horizontal scroll offset for the input line, in display columns (how far
    /// the input body has scrolled right to keep the cursor in view).
    pub fn input_scroll(&self) -> usize {
        self.input_scroll
    }

    pub fn transcript(&self) -> &[TranscriptLine] {
        static EMPTY: &[TranscriptLine] = &[];
        self.current_chat()
            .map_or(EMPTY, |chat| chat.transcript.as_slice())
    }

    pub fn streaming_text(&self) -> &str {
        self.current_chat()
            .map_or("", |chat| chat.streaming_text.as_str())
    }

    /// The current chat's live thinking buffer, when it has one.
    pub fn thinking(&self) -> Option<&Thinking> {
        self.current_chat().map(|chat| &chat.thinking)
    }

    /// The tail of the current chat's thinking — what the indicator line shows.
    /// Empty when the employee is not thinking.
    pub fn thinking_tail(&self) -> String {
        self.thinking().map(Thinking::tail).unwrap_or_default()
    }

    /// The whole thinking buffer for the current chat (the `full` display mode).
    pub fn thinking_text(&self) -> String {
        self.thinking().map(Thinking::text).unwrap_or_default()
    }

    /// Whether the current chat has any thinking to show at all.
    pub fn has_thinking(&self) -> bool {
        self.thinking().is_some_and(|thinking| !thinking.is_empty())
    }

    /// The pinned "thinking" row for the current chat, or `None` when there is
    /// nothing to draw.
    ///
    /// This is where the `[ui] thinking` setting is applied, so both surfaces that
    /// show the indicator (the chat transcript and the roster prompt bar) obey it
    /// from one place:
    ///
    /// - `indicator` (default) — one dim line: spinner, `thinking`, and the tail.
    /// - `hidden` — nothing; the status glyph alone says the employee is busy.
    /// - `full` — the whole deliberation, wrapped as a dim block.
    pub fn thinking_line(&self, width: usize) -> Option<ratatui::text::Line<'static>> {
        if self.chat_employee_status() != Some(EmployeeStatus::Thinking) {
            return None;
        }
        self.thinking_line_for(self.thinking(), width)
    }

    /// The thinking row for a specific buffer, applying the `[ui] thinking` mode.
    /// The one implementation both surfaces share.
    fn thinking_line_for(
        &self,
        thinking: Option<&Thinking>,
        width: usize,
    ) -> Option<ratatui::text::Line<'static>> {
        match self.config.ui.thinking {
            crate::config::ThinkingMode::Hidden => None,
            crate::config::ThinkingMode::Indicator => {
                let tail = thinking.map(Thinking::tail).unwrap_or_default();
                Some(crate::ui::thinking_line(self.frame, &tail, width))
            }
            crate::config::ThinkingMode::Full => {
                let text = thinking.map(Thinking::text).unwrap_or_default();
                let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if collapsed.is_empty() {
                    return None;
                }
                // The full mode keeps the whole deliberation, so it wraps rather
                // than truncating — it is what the user asked for.
                Some(ratatui::text::Line::from(ratatui::text::Span::styled(
                    collapsed,
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::DarkGray)
                        .add_modifier(ratatui::style::Modifier::DIM),
                )))
            }
        }
    }

    /// Transcript scroll offset: display lines scrolled up from the live tail
    /// (`0` = auto-follow the newest output).
    pub fn chat_scroll(&self) -> usize {
        self.current_chat().map_or(0, |chat| chat.scroll)
    }

    /// The current chat employee's work status, for the chat title bar.
    pub fn chat_employee_status(&self) -> Option<EmployeeStatus> {
        self.chat_employee
            .and_then(|index| self.team.employees().get(index))
            .map(|employee| employee.status)
    }

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    pub fn project_roots(&self) -> &[PathBuf] {
        &self.project_roots
    }

    pub fn active_project(&self) -> Option<&Project> {
        self.active_project.as_ref()
    }

    pub fn project_selected(&self) -> usize {
        self.project_selected
    }

    pub fn models(&self) -> &[ProviderModel] {
        &self.models
    }

    pub fn model_selected(&self) -> usize {
        self.model_selected
    }

    pub fn model_filter(&self) -> &str {
        &self.model_filter
    }

    pub fn chat_employee_name(&self) -> Option<&str> {
        self.chat_employee
            .and_then(|index| self.team.employees().get(index))
            .map(|employee| employee.name.as_str())
    }

    pub fn is_connected(&self) -> bool {
        self.server_info.is_some()
    }

    /// True when a Jev (System One) client is attached — the grid's Jev header
    /// strip shows it "on" then.
    pub fn jev_enabled(&self) -> bool {
        self.jev.is_some()
    }

    /// The decision model the attached Jev client asks (`None` while Jev is
    /// off) — the grid's header strip renders it next to the on/off state.
    pub fn jev_model(&self) -> Option<&str> {
        self.jev.as_ref().map(JevClient::model)
    }

    pub fn server_version(&self) -> Option<&str> {
        self.server_info.as_ref().map(|info| info.version.as_str())
    }

    pub fn connection_error(&self) -> Option<&str> {
        self.connection_error.as_deref()
    }

    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    pub fn last_event(&self) -> Option<&str> {
        self.last_event.as_deref()
    }

    pub fn log(&self) -> &[LogLine] {
        &self.log
    }

    pub fn usage(&self) -> &Usage {
        &self.usage
    }

    pub fn metrics(&self) -> &ProcessMetrics {
        &self.metrics
    }

    /// The persisted scheduled tasks, in insertion order (newest last). The
    /// tasks panel re-sorts them for display via `order_tasks`.
    pub fn tasks(&self) -> &[Task] {
        &self.store.tasks
    }

    /// Whether the engine gate is set — autonomous fires are queuing as
    /// `pending` instead of dispatching.
    pub fn is_paused(&self) -> bool {
        self.store.paused
    }

    /// The reopen prompt, when the last run paused tasks on its way out. The
    /// renderer draws the modal from this and the prompter is the only writer.
    pub fn resume_prompt(&self) -> Option<&ResumePrompt> {
        self.resume_prompt.as_ref()
    }

    // --- sidebar vitals ---

    /// Advance one animation tick without going through the message loop.
    ///
    /// The render tests need to compare frame *n* against *n+1* (A9), and the
    /// event loop's tick is a `Message` the tests can't send. This is the same
    /// advance the loop performs — frame counter, pulse sample, reliability
    /// sweep — minus the process-metrics refresh, which needs wall time and is
    /// irrelevant to what the sidebar draws.
    pub fn advance(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.pulse.tick(self.event_count);
        self.tick_reliability();
        self.tick_traces();
    }

    /// Append one sample to the activity pulse window without advancing the
    /// animation frame.
    ///
    /// The sidebar's sparkline is a pure function of this window, and nothing
    /// else in the frame reads it — which makes this the A2 containment seam:
    /// two renders that differ only here must be identical outside the sidebar.
    /// [`Self::advance`] cannot serve that purpose, because it also bumps the
    /// frame counter the title bar's spinner reads.
    pub fn record_pulse(&mut self, events: u64) {
        self.pulse.record(events);
    }

    /// How long this run has been up, for the sidebar's runtime line.
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// The per-tick event counts behind the activity sparkline, oldest first.
    pub fn pulse(&self) -> &[u64] {
        self.pulse.samples()
    }

    /// How many employees are mid-turn (`Thinking` or `Working`).
    pub fn busy_count(&self) -> usize {
        self.team
            .employees()
            .iter()
            .filter(|employee| employee.is_busy())
            .count()
    }

    /// How many employees are specifically `Thinking` (a subset of `busy_count`).
    pub fn thinking_count(&self) -> usize {
        self.team
            .employees()
            .iter()
            .filter(|employee| employee.status == EmployeeStatus::Thinking)
            .count()
    }

    /// How many chats hold a live OpenCode session.
    pub fn live_sessions(&self) -> usize {
        self.chats
            .values()
            .filter(|chat| chat.session_id.is_some())
            .count()
    }

    /// How many prompts are queued behind an employee's current turn.
    pub fn queued_prompts(&self) -> usize {
        self.chats
            .values()
            .map(|chat| chat.queue_running.len())
            .sum()
    }

    /// How many prompts are queued behind **one** employee's current turn.
    ///
    /// The per-employee counterpart of [`Self::queued_prompts`], so a card or a
    /// tasks-panel row can read its own depth instead of reaching into
    /// `App::chats`. `0` for an employee with no chat yet.
    pub fn queued_prompts_for(&self, employee: usize) -> usize {
        self.chats
            .get(&employee)
            .map_or(0, |chat| chat.queue_running.len())
    }

    /// Every persisted task assigned to `employee`, in `order_tasks` order —
    /// active work first, then terminal, newest-first within each.
    ///
    /// The per-employee slice of the queue block: an employee's own work in the
    /// panel's own order, so the block and the board cannot disagree about what
    /// is "first". Unassigned tasks (`assigned_to == None`) belong to no
    /// employee and are therefore never returned here.
    pub fn tasks_for(&self, employee: usize) -> Vec<&Task> {
        let Some(id) = self.team.employees().get(employee).map(|e| e.id.as_str()) else {
            return Vec::new();
        };
        let mut mine: Vec<&Task> = self
            .store
            .tasks
            .iter()
            .filter(|task| task.assigned_to.as_deref() == Some(id))
            .collect();
        mine.sort_by_key(|task| task_order_key(task));
        mine
    }

    /// How many persisted tasks are held — `Paused`, and nothing else.
    ///
    /// Deliberately excludes `Pending` work sitting behind a paused engine gate
    /// (queued, not held) and `Retrying` work (the app is acting on it). This is
    /// the number the queue block's trailing row reports, so it must mean "needs
    /// a human", not "is not currently moving".
    pub fn held_tasks(&self) -> usize {
        self.store
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Paused)
            .count()
    }

    /// The unassigned tasks — `assigned_to == None` — in `order_tasks` order.
    ///
    /// They have no employee to group under, so the queue block draws them as a
    /// final group under the `—` placeholder.
    pub fn unassigned_tasks(&self) -> Vec<&Task> {
        let mut mine: Vec<&Task> = self
            .store
            .tasks
            .iter()
            .filter(|task| task.assigned_to.is_none())
            .collect();
        mine.sort_by_key(|task| task_order_key(task));
        mine
    }

    /// A roll-up of the persisted task queue by status.
    pub fn task_stats(&self) -> TaskStats {
        let mut stats = TaskStats::default();
        for task in &self.store.tasks {
            match task.status {
                TaskStatus::Pending => stats.pending += 1,
                TaskStatus::Running => stats.running += 1,
                TaskStatus::Retrying => stats.retrying += 1,
                TaskStatus::Paused => stats.paused += 1,
                TaskStatus::Succeeded => stats.succeeded += 1,
                TaskStatus::Failed => stats.failed += 1,
            }
        }
        stats
    }

    /// The most recent failed task's summary, for the sidebar's alert line.
    pub fn last_failure(&self) -> Option<&str> {
        self.store
            .tasks
            .iter()
            .rev()
            .find(|task| task.status == TaskStatus::Failed)
            .map(|task| task.goal.as_str())
    }
}

/// A short, single-line summary of a task — the goal an employee is handed —
/// for the tasks panel: first non-empty line, whitespace collapsed, truncated.
pub(crate) fn task_summary(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let joined = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= 48 {
        return joined;
    }
    let mut out: String = joined.chars().take(47).collect();
    out.push('…');
    out
}

/// A display ordering for the tasks panel: active work first (Pending, Running,
/// Paused, Retrying), then terminal tasks (Succeeded, Failed), each group sorted
/// newest-first by `created_at`. Returns indices into `tasks` so callers keep
/// borrowing the original slice.
pub(crate) fn order_tasks(tasks: &[Task]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..tasks.len()).collect();
    indices.sort_by_key(|&index| task_order_key(&tasks[index]));
    indices
}

/// The one definition of "active first, then newest-first" — shared by
/// [`order_tasks`] and the per-employee accessors so a filtered list can never
/// order differently from the whole-queue view.
pub(crate) fn task_order_key(task: &Task) -> (bool, Reverse<i64>) {
    let terminal = matches!(task.status, TaskStatus::Succeeded | TaskStatus::Failed);
    (terminal, Reverse(task.created_at))
}

/// The roster status implied by a completed turn's message list: the last
/// assistant (or idle) message decides `Done` vs `Error`, and a transcript with
/// no such message leaves the employee `Idle`.
fn completion_status(messages: &[SessionMessage]) -> EmployeeStatus {
    messages
        .iter()
        .rev()
        .find(|m| m.role() == "assistant" || m.role() == "idle")
        .map(|m| {
            if m.error.is_some() || m.finish.as_deref() == Some("error") {
                EmployeeStatus::Error
            } else {
                EmployeeStatus::Done
            }
        })
        .unwrap_or(EmployeeStatus::Idle)
}

/// The failure reason of a completed turn's transcript, for the log. `None` when
/// the last assistant/idle message did not fail. The reason is the provider's
/// error rendered with the detail a debugger needs — type, message, and HTTP
/// status — or the `finish` marker when there is no structured error.
fn failed_turn_reason(messages: &[SessionMessage]) -> Option<String> {
    let failed = messages.iter().rev().find(|m| {
        (m.role() == "assistant" || m.role() == "idle")
            && (m.error.is_some() || m.finish.as_deref() == Some("error"))
    })?;
    Some(match &failed.error {
        Some(error) => format_message_error(error),
        None => format!("finish: {}", failed.finish.as_deref().unwrap_or("error")),
    })
}

/// A [`MessageError`] formatted with every field the provider reported, so a
/// failure in the log names the type, the message, and the status rather than
/// collapsing them into one vague line.
fn format_message_error(error: &MessageError) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(error_type) = error.error_type.as_deref().filter(|t| !t.is_empty()) {
        parts.push(error_type.to_string());
    }
    if let Some(message) = error.message.as_deref().filter(|m| !m.is_empty()) {
        parts.push(message.to_string());
    }
    if let Some(status) = error.status {
        parts.push(format!("status {status}"));
    }
    if parts.is_empty() {
        "execution failed".to_string()
    } else {
        parts.join(" · ")
    }
}

/// The deliverable text of the last completed assistant reply — delegation
/// directives stripped — used as a worker's contribution during aggregation.
///
/// [`workflow::contribution`] handles the reply whose only text *was* a final
/// directive, so a worker that answered `@user <answer>` still contributes the
/// answer instead of an empty string.
fn last_assistant_output(messages: &[SessionMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role() == "assistant" && !message.is_incomplete())
        .and_then(|message| workflow::contribution(&message.display_text()))
}

/// Run one scheduled-task attempt: create a session, prompt (with a retry or
/// resume preamble when given), wait for completion, and read the outcome back
/// from the transcript. `Ok` carries the deliverable; `Err` the failure reason.
///
/// The session id is reported back as a [`Message::TaskAttemptStarted`] before
/// the prompt is sent, so the app can interrupt the attempt if the task is
/// paused while it runs.
#[allow(clippy::too_many_arguments)]
/// Run one scheduled/watch task attempt: a fresh session, the goal, and the
/// transcript read back as an outcome.
///
/// **A scheduled task is an isolated job, not a delegation chain.** It runs in its
/// own session with no `origin_of`/`depth_of` entry, so there is no chain for a
/// report to travel: a `@user` in its reply has nowhere to bubble to. It is
/// therefore briefed with the roster and the reporting rule (so a worker does not
/// try to hand off into a chain that does not exist) *and* told to finish the job
/// itself. The deliverable is the task's result, which the harness records against
/// the task — not a chat delivery.
async fn run_task_attempt(
    client: &OpenCode,
    directory: &str,
    agent: Option<&str>,
    model: Option<&ModelRef>,
    goal: &str,
    preamble: Option<&str>,
    task_id: TaskId,
    tx: Sender<Message>,
    context: &str,
) -> Result<String, String> {
    let (session, _used_agent) = client
        .create_session_with_agent_fallback(directory, agent, model)
        .await
        .map_err(|err| format!("session failed: {err:#}"))?;
    let session_id = session.id;

    let _ = tx
        .send(Message::TaskAttemptStarted {
            task_id,
            session_id: session_id.clone(),
        })
        .await;

    let brief = task_brief(context, goal, preamble);
    let options = harness::prompt_options(
        harness::PromptKind::AutonomousTask,
        agent.unwrap_or_default(),
        0,
        &harness::delivery_token(),
    );

    client
        .prompt_with(&session_id, &brief, &options)
        .await
        .map_err(|err| format!("prompt failed: {err}"))?;

    // Block until the session is idle — best-effort: the experimental endpoint
    // may drift, so completion is verified against the transcript below.
    let _ = client.wait(&session_id).await;

    let messages = client
        .messages(&session_id)
        .await
        .map_err(|err| format!("transcript failed: {err}"))?;

    task_outcome(&messages)
}

/// The prompt for one autonomous attempt: the team context (so the employee knows
/// the roster and the reporting rule), the retry/resume preamble when there is
/// one, and the goal — plus the instruction that makes the isolation explicit.
fn task_brief(context: &str, goal: &str, preamble: Option<&str>) -> String {
    let mut brief = String::new();
    if !context.is_empty() {
        brief.push_str(context);
        brief.push_str("\n\n");
    }
    brief.push_str(
        "This is a scheduled job running in its own session, not part of a delegation \
         chain: no one is waiting on a report, so finish the work yourself and reply with \
         the result. Do not hand this job off and do not use `@user` — your reply *is* the \
         deliverable.",
    );
    brief.push_str("\n\n");
    if let Some(preamble) = preamble {
        brief.push_str(preamble);
        brief.push_str("\n\n");
    }
    brief.push_str(goal);
    brief
}

/// Interpret a completed transcript as ‹success → deliverable› or ‹failure → reason›.
fn task_outcome(messages: &[SessionMessage]) -> Result<String, String> {
    let Some(message) = messages
        .iter()
        .rev()
        .find(|m| m.role() == "assistant" && !m.is_incomplete())
    else {
        return Err("no assistant reply".to_string());
    };

    // A typed error (or `finish: "error"`) means the attempt failed.
    if message.error.is_some() || message.finish.as_deref() == Some("error") {
        let reason = message
            .error
            .as_ref()
            .and_then(|e| e.message.clone())
            .unwrap_or_else(|| "execution failed".to_string());
        return Err(reason);
    }

    let text = strip_directives(&message.display_text());
    if text.is_empty() {
        Err("no deliverable (empty reply)".to_string())
    } else {
        Ok(text)
    }
}

/// Build display lines from a canonical transcript.
///
/// Messages are sorted by their creation timestamp so the rendered transcript
/// reads chronologically regardless of the transport's paging/order, and
/// contentless markers (e.g. a successful `idle` turn boundary) are dropped.
fn build_transcript(messages: &[SessionMessage]) -> Vec<TranscriptLine> {
    let mut sorted: Vec<&SessionMessage> = messages.iter().collect();
    sorted.sort_by_key(|message| message.time.as_ref().map(|time| time.created).unwrap_or(0));

    sorted
        .into_iter()
        .filter_map(|message| {
            let role = match message.role() {
                "user" => TranscriptRole::User,
                "assistant" => TranscriptRole::Assistant,
                _ if message.error.is_some() => TranscriptRole::Error,
                _ => TranscriptRole::Info,
            };
            let mut text = message.display_text();
            if text.is_empty()
                && let Some(err) = message.error.as_ref().and_then(|e| e.message.clone())
            {
                text = format!("error: {err}");
            }
            // Drop markers that carry no visible content (empty idle boundaries).
            if role == TranscriptRole::Info && text.is_empty() {
                return None;
            }
            // Attribution: show which model produced the assistant reply.
            let meta = (role == TranscriptRole::Assistant)
                .then(|| message.model.as_ref().map(|m| m.id.as_str()))
                .flatten()
                .map(str::to_owned);
            Some(TranscriptLine { role, text, meta })
        })
        .collect()
}

/// Connect to OpenCode and forward live events + status to the app channel.
///
/// Runs for the lifetime of the app: after the initial connect handshake it
/// subscribes to the event stream and forwards every event. Holding the `tx`
/// clone in this loop also keeps the channel open so the receiver never ends.
pub async fn opencode_background(tx: Sender<Message>, jev: Option<JevClient>) {
    use futures::StreamExt;

    // When no service is running, surface a "starting…" status before connect
    // (connect_or_start starts the service and blocks until it's ready).
    let running = matches!(crate::opencode::service::discover().await, Ok(Some(_)));
    if !running {
        let _ = tx
            .send(Message::Log(
                "OpenCode service not running — starting…".to_string(),
            ))
            .await;
    }

    let client = match OpenCode::connect_or_start().await {
        Ok(client) => client,
        Err(err) => {
            let _ = tx.send(Message::ConnectionFailed(err.to_string())).await;
            return;
        }
    };

    match client.server_info().await {
        Ok(info) => {
            let _ = tx
                .send(Message::Connected {
                    client: client.clone(),
                    info,
                })
                .await;
        }
        Err(err) => {
            let _ = tx.send(Message::ConnectionFailed(err.to_string())).await;
            return;
        }
    }

    if let Ok(agents) = client.agents().await {
        let _ = tx.send(Message::AgentsLoaded(agents)).await;
    }

    // Pre-load the model catalog so session creation can resolve a model's
    // provider without the user opening the picker first.
    if let Ok(models) = client.models().await {
        let _ = tx.send(Message::ModelsLoaded(models)).await;
    }

    // Pre-load the MCP server status so the sidebar can show it from the first
    // frame; refreshed on reconnect.
    if let Ok(servers) = client.mcp_status().await {
        let _ = tx.send(Message::McpLoaded(servers)).await;
    }

    let Some(events) = client.events().ok() else {
        let _ = tx
            .send(Message::Log("event stream unavailable".to_string()))
            .await;
        return;
    };
    // The async-stream generator is `!Unpin`, so pin it before driving `.next()`.
    futures::pin_mut!(events);

    while let Some(event) = events.next().await {
        let message = match event {
            Ok(event) => {
                // A session paused on a form must be answered (or it hangs
                // forever); resolve it out-of-band via Jev with a deterministic
                // fallback, then keep the event flowing to the UI.
                if event.is_form() {
                    spawn_form_answer(client.clone(), jev.clone(), event.clone(), tx.clone());
                }
                Message::OpenCodeEvent(event)
            }
            Err(err) => Message::Log(format!("event error: {err}")),
        };
        if tx.send(message).await.is_err() {
            break;
        }
    }
}

/// Answer an OpenCode `form.created` request so the session resumes without
/// waiting on human input. Each field is resolved with Jev (when available),
/// falling back to a deterministic default.
fn spawn_form_answer(client: OpenCode, jev: Option<JevClient>, event: Event, tx: Sender<Message>) {
    tokio::spawn(async move {
        let Some(request) = event.form_request() else {
            let _ = tx
                .send(Message::Log("form.created without a payload".to_string()))
                .await;
            return;
        };
        let session_id = match request
            .session_id
            .clone()
            .or_else(|| event.session_id().map(str::to_owned))
        {
            Some(id) => id,
            None => {
                let _ = tx
                    .send(Message::Log("form without a session id".to_string()))
                    .await;
                return;
            }
        };
        let Some(form_id) = request.form_id.clone() else {
            let _ = tx
                .send(Message::Log("form without an id".to_string()))
                .await;
            return;
        };
        // A global form (`sessionID: "global"`) is location-scoped; the reply
        // must name its location so the server consults the right store.
        let directory = event.location.as_ref().map(|loc| loc.directory.clone());

        let mut answer = serde_json::Map::with_capacity(request.fields.len());
        let mut chosen: Vec<String> = Vec::with_capacity(request.fields.len());
        for field in &request.fields {
            let value = answer_field_or_default(jev.as_ref(), field).await;
            chosen.push(format!("{}={}", field.key, value_label(&value)));
            answer.insert(field.key.clone(), value);
        }

        match client
            .reply_form(&session_id, &form_id, &answer, directory.as_deref())
            .await
        {
            Ok(()) => {
                let _ = tx
                    .send(Message::Log(format!(
                        "answered form → {}",
                        chosen.join(", ")
                    )))
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(Message::Log(format!("form reply failed: {err}")))
                    .await;
            }
        }
    });
}

/// A compact log label for a form-field value.
fn value_label(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(items) => {
            items.iter().map(value_label).collect::<Vec<_>>().join(", ")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
