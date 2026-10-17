use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
use crate::jev::JevClient;
use crate::message::Message;
use crate::opencode::{
    AgentInfo, Event, ModelRef, OpenCode, ProviderModel, ServerInfo, SessionMessage, pick_provider,
};
use crate::persistence::Store;
use crate::projects::{self, Project};
use crate::role::Role;
use crate::task::{Task, TaskId, TaskStatus};
use crate::team::{EmployeeStatus, Team, move_selection};
use crate::tui::Tui;
use crate::ui;
use crate::usage::{ProcessMetrics, Usage};
use crate::workflow::{
    Delegation, is_final, is_jev, is_parallel, parse_delegations, strip_directives, team_context,
};

/// Maximum number of retained status-log lines.
const LOG_LIMIT: usize = 64;

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
    /// Live reasoning text (surfaced dim while the employee is thinking).
    reasoning_text: String,
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
    outbox: Vec<String>,
    /// The delegated hand-off currently in flight for this employee, tracked so
    /// a silent worker can be retried or abandoned on a deadline.
    pending: Option<PendingDelivery>,
    /// User prompts submitted while the employee was mid-turn (or before their
    /// session was ready), drained one per completed turn by
    /// [`App::pump_employee_queue`].
    queue_running: Vec<String>,
}

/// A parallel fan-out: one goal broadcast to several workers, whose outputs are
/// collected and handed to an aggregator (the reviewer) once they all finish.
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

/// One entry in the chat slash-command autocomplete menu.
#[derive(Debug, Clone, Copy)]
pub struct CommandOption {
    pub command: &'static str,
    pub description: &'static str,
    /// Whether selecting this option auto-appends a trailing space (it takes an
    /// argument the user then types).
    pub takes_arg: bool,
}

/// The slash commands offered by the chat completion menu, in display order.
const CHAT_COMMANDS: &[CommandOption] = &[
    CommandOption {
        command: "/help",
        description: "show chat commands",
        takes_arg: false,
    },
    CommandOption {
        command: "/stop",
        description: "interrupt the current run",
        takes_arg: false,
    },
    CommandOption {
        command: "/new",
        description: "start a fresh session",
        takes_arg: false,
    },
    CommandOption {
        command: "/clear",
        description: "clear this conversation",
        takes_arg: false,
    },
    CommandOption {
        command: "/skills",
        description: "list installed skills",
        takes_arg: false,
    },
    CommandOption {
        command: "/skill add",
        description: "install & attach a skill",
        takes_arg: true,
    },
    CommandOption {
        command: "/skill",
        description: "attach an installed skill",
        takes_arg: true,
    },
];

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
    log: Vec<String>,
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
    /// Last known terminal size, for page-scroll estimation.
    last_size: (u16, u16),
    /// Per-employee chat state, keyed by roster index.
    chats: HashMap<usize, Chat>,
    /// Selected index in the slash-command autocomplete menu, when it is open
    /// (i.e. the input starts with `/` and matches one or more commands).
    command_menu: Option<usize>,
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
}

impl App {
    pub fn new(config: Config, tx: Sender<Message>) -> Self {
        Self::with_team(config, tx, Team::starter())
    }

    pub fn with_team(config: Config, tx: Sender<Message>, team: Team) -> Self {
        Self {
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
            log: vec!["connecting to OpenCode…".into()],
            frame: 0,
            shutting_down: false,
            shutdown_frame: 0,
            projects: Vec::new(),
            project_roots: Vec::new(),
            active_project: None,
            project_selected: 0,
            models: Vec::new(),
            model_selected: 0,
            model_filter: String::new(),
            mode: Mode::Roster,
            chat_employee: None,
            input: String::new(),
            input_cursor: 0,
            input_scroll: 0,
            last_size: (80, 24),
            chats: HashMap::new(),
            command_menu: None,
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
        }
    }

    /// Attach the Jev (System One) decision client, used to answer `@jev`
    /// questions and OpenCode `form.created` prompts.
    pub fn set_jev(&mut self, jev: Option<JevClient>) {
        self.jev = jev;
    }

    /// Enable task/memory persistence at the default store path, loading any
    /// previously saved tasks. Call once after [`Self::with_team`].
    pub fn enable_persistence(&mut self) {
        self.store_path = Store::default_path();
        if let Some(path) = &self.store_path {
            self.store = Store::load(path).unwrap_or_default();
        }
    }

    pub async fn run(&mut self, tui: &mut Tui, mut events: EventHandler) -> Result<()> {
        loop {
            if self.shutting_down {
                // Graceful shutdown: animate the overlay for a few frames so
                // the terminal stays put while state is flushed, then return.
                self.shutdown_frame = self.shutdown_frame.wrapping_add(1);
                tui.draw(|frame| ui::render(frame, self))?;
                if self.shutdown_frame >= SHUTDOWN_FRAMES {
                    break;
                }
                tokio::time::sleep(SHUTDOWN_TICK).await;
                continue;
            }

            tui.draw(|frame| ui::render(frame, self))?;

            let message = events.next().await?;
            self.update(message);
        }

        Ok(())
    }

    /// Begin graceful shutdown: flush state, stop in-flight work, and let the run
    /// loop animate the "shutting down" overlay before relinquishing the terminal.
    fn initiate_shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.shutdown_frame = 0;
        // Stop the current generation (best effort) and force a final store write.
        self.interrupt_current();
        self.persist_store();
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => {
                self.frame = self.frame.wrapping_add(1);
                self.tick_reliability();
                self.metrics.refresh();
            }
            Message::Resize(w, h) => {
                self.last_size = (w, h);
                self.follow_input_cursor();
            }
            Message::Key(key) => self.handle_key(key),
            Message::Paste(text) => self.handle_paste(&text),
            Message::Connected { client, info } => {
                let version = info.version.clone();
                self.client = Some(client);
                self.server_info = Some(info);
                self.push_log(format!("connected to OpenCode v{version}"));
            }
            Message::ConnectionFailed(reason) => {
                self.connection_error = Some(reason.clone());
                self.push_log(format!("OpenCode unavailable: {reason}"));
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
            Message::Log(line) => self.push_log(line),
            Message::Quit => self.initiate_shutdown(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
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

        match self.mode {
            Mode::Roster => match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.initiate_shutdown(),
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
                    self.move_selection_stage(-1);
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
                    self.move_selection_stage(1);
                }
                KeyCode::Enter => self.start_chat(self.selected),
                KeyCode::Char('p') | KeyCode::Char('P') => self.open_projects(),
                KeyCode::Char('m') | KeyCode::Char('M') => self.open_models(),
                _ => {}
            },
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
                KeyCode::Up => {
                    self.model_selected =
                        move_selection(self.model_selected, -1, self.filtered_models().len());
                }
                KeyCode::Down => {
                    self.model_selected =
                        move_selection(self.model_selected, 1, self.filtered_models().len());
                }
                KeyCode::Enter => self.assign_selected_model(),
                KeyCode::Backspace => self.edit_model_filter(None),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.model_filter.clear();
                    self.model_selected = 0;
                }
                KeyCode::Char(c) => self.edit_model_filter(Some(c)),
                _ => {}
            },
            Mode::Chat => self.handle_chat_key(key),
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
            KeyCode::Char(c) => {
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
            .filter(|opt| opt.command.starts_with(input))
            .copied()
            .collect()
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
        if option.takes_arg {
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
    /// panel's two side borders aside — derived from the terminal width.
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
        if self.mode != Mode::Chat {
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
        let Some(session_id) = self
            .chats
            .get(&employee)
            .and_then(|chat| chat.session_id.clone())
        else {
            return;
        };
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.streaming_text.clear();
            chat.reasoning_text.clear();
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
                        .send(Message::Log(format!("interrupt failed: {err}")))
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
        // Drop the old session binding and start a fresh one (same worker).
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.session_id = None;
            chat.creating = false;
            chat.transcript.clear();
            chat.streaming_text.clear();
            chat.reasoning_text.clear();
            chat.scroll = 0;
            // Prompts queued against the old conversation don't carry over.
            chat.queue_running.clear();
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

        self.begin_session(employee);
    }

    /// Create the OpenCode session backing `employee`'s chat (skips if one is
    /// already present or being created).
    fn begin_session(&mut self, employee: usize) {
        if self
            .chats
            .get(&employee)
            .is_some_and(|c| c.session_id.is_some() || c.creating)
        {
            return;
        }
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.creating = true;
            chat.streaming_text.clear();
            chat.reasoning_text.clear();
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
                self.push_log(format!("session failed: {err}"));
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
            chat.reasoning_text.clear();
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

        // If the employee is mid-turn (or their session isn't ready yet), the
        // prompt is queued and sent once they're idle — see `pump_employee_queue`.
        if self.employee_is_busy(employee) || !self.has_session(employee) {
            self.enqueue_prompt(employee, text);
            return;
        }

        self.dispatch_prompt(employee, text);
    }

    /// True when `employee` has a live OpenCode session.
    fn has_session(&self, employee: usize) -> bool {
        self.chats
            .get(&employee)
            .is_some_and(|chat| chat.session_id.is_some())
    }

    /// True when `employee` is mid-turn (thinking or working) and so cannot take
    /// a new prompt right now.
    fn employee_is_busy(&self, employee: usize) -> bool {
        self.team
            .employees()
            .get(employee)
            .is_some_and(|e| matches!(e.status, EmployeeStatus::Thinking | EmployeeStatus::Working))
    }

    /// Queue `text` for `employee`, starting their session if none is already
    /// being created so the queue can drain once it is ready.
    fn enqueue_prompt(&mut self, employee: usize, text: String) {
        let name = self
            .team
            .employees()
            .get(employee)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| employee.to_string());
        self.push_log(format!("queued for {name}"));
        self.chats
            .entry(employee)
            .or_default()
            .queue_running
            .push(text);
        if !self.has_session(employee) {
            self.begin_session(employee);
        }
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
        if !self.has_session(employee) || self.employee_is_busy(employee) {
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
        let Some(session_id) = self
            .chats
            .get(&employee)
            .and_then(|chat| chat.session_id.clone())
        else {
            return;
        };
        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.streaming_text.clear();
            chat.reasoning_text.clear();
            chat.scroll = 0;
        }
        self.team.set_status(employee, EmployeeStatus::Thinking);

        // The first prompt to an employee carries the team roster + delegation
        // protocol; later prompts go through verbatim.
        let prompt = self.bootstrap_prompt(employee, text);

        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();
        let target = employee;
        tokio::spawn(async move {
            if let Err(err) = client.prompt(&session_id, &prompt).await {
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
            self.push_transcript_line(TranscriptRole::Info, format!("⚡ {text}"));
            self.reset_chat();
            return true;
        }

        // `/skills` — list installed skill names into the transcript.
        if rest == "skills" {
            self.push_transcript_line(TranscriptRole::Info, format!("⚡ {text}"));
            let names = crate::skills::installed();
            if names.is_empty() {
                self.push_transcript_line(
                    TranscriptRole::Info,
                    "no skills installed for this project".to_string(),
                );
            } else {
                for name in names {
                    self.push_transcript_line(TranscriptRole::Info, format!("  • {name}"));
                }
            }
            return true;
        }

        // `/skill add <source>[@skill]` — install then attach to this employee.
        if let Some(source) = rest.strip_prefix("skill add ").map(str::trim) {
            if source.is_empty() {
                self.push_transcript_error("usage: /skill add <source>[@skill]".to_string());
                return true;
            }
            let Some(employee) = self.chat_employee else {
                self.push_transcript_error("no employee selected".to_string());
                return true;
            };
            self.push_transcript_line(TranscriptRole::Info, format!("⚡ /skill add {source}"));
            self.spawn_skill_install(employee, source.to_string());
            return true;
        }

        // `/skill <name>` — attach an already-installed skill to this employee.
        if let Some(name) = rest.strip_prefix("skill ").map(str::trim) {
            if name.is_empty() {
                self.push_transcript_error("usage: /skill <name>".to_string());
                return true;
            }
            self.attach_skill(name.to_string());
            return true;
        }

        // `/parallel <goal>` — fan the goal out to the whole team and aggregate.
        if let Some(goal) = rest.strip_prefix("parallel ").map(str::trim) {
            if goal.is_empty() {
                self.push_transcript_error("usage: /parallel <goal>".to_string());
                return true;
            }
            let Some(employee) = self.chat_employee else {
                self.push_transcript_error("no employee selected".to_string());
                return true;
            };
            self.push_transcript_line(TranscriptRole::Info, format!("⚡ /parallel {goal}"));
            self.fan_out(employee, goal.to_string());
            return true;
        }

        // `/help` — show the chat-local commands.
        if rest == "help" {
            self.push_transcript_line(
                TranscriptRole::Info,
                "chat commands:\n  /skill add <source>  install & attach a skill\n  /skill <name>        attach an installed skill\n  /skills              list installed skills\n  /parallel <goal>     fan the goal out to the whole team\n  /stop                interrupt the current run\n  /new · /clear        start a fresh session".to_string(),
            );
            return true;
        }

        false
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
        chat.reasoning_text.clear();
        chat.scroll = 0;
        // The hand-off "returned" (in some form), so stop tracking its deadline.
        chat.pending = None;

        let final_status = messages
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
            .unwrap_or(EmployeeStatus::Idle);

        self.team.set_status(employee, final_status);

        // Hand off any directives in the freshly completed assistant reply.
        self.dispatch_delegations(employee, &messages);

        // Record the employee's deliverable into any active parallel fan-out.
        self.note_fanout_result(employee, &messages);

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
                if let Some(chat) = self.chats.get_mut(&employee) {
                    chat.reasoning_text.clear();
                }
            }
            "session.reasoning.delta" => {
                if let Some(delta) = event.text_delta()
                    && let Some(chat) = self.chats.get_mut(&employee)
                {
                    chat.reasoning_text.push_str(delta);
                }
            }
            "session.execution.started" => self.team.set_status(employee, EmployeeStatus::Working),
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
                if let Some(reason) = event.error_message() {
                    self.push_log(reason.to_string());
                }
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
                        .send(Message::Log(format!("transcript load failed: {err}")))
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
        let name = self
            .team
            .employees()
            .get(employee)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let context = team_context(&self.team, &name);
        format!("{context}\n\n{text}")
    }

    /// Relay `instruction` to `employee`. If their session must be created
    /// first, the instruction is queued and flushed when the session is ready.
    fn send_to_employee(&mut self, employee: usize, instruction: String) {
        self.push_to_chat(employee, TranscriptRole::Info, format!("📩 {instruction}"));
        self.chats
            .entry(employee)
            .or_default()
            .outbox
            .push(instruction);
        let ready = self
            .chats
            .get(&employee)
            .is_some_and(|chat| chat.session_id.is_some() && !chat.creating);
        if ready {
            self.flush_outbox(employee);
        } else {
            self.begin_session(employee);
        }
    }

    /// Send every queued delegation prompt out to `employee`'s live session.
    fn flush_outbox(&mut self, employee: usize) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();

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

        for instruction in outbox {
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
            // stored instruction stays preamble-free (the original goal).
            if let Some(chat) = self.chats.get_mut(&employee) {
                chat.pending = Some(PendingDelivery {
                    instruction,
                    attempts,
                    since: Instant::now(),
                });
            }
            let session_id = session_id.clone();
            let client = client.clone();
            let tx = tx.clone();
            let target = employee;
            tokio::spawn(async move {
                if let Err(err) = client.prompt(&session_id, &prompt).await {
                    let _ = tx
                        .send(Message::DeliveryFailed {
                            target,
                            reason: err.to_string(),
                        })
                        .await;
                }
            });
        }
    }

    /// Scan a freshly loaded transcript for finished assistant replies carrying
    /// hand-off directives and dispatch each one (once, by message id).
    fn dispatch_delegations(&mut self, employee: usize, messages: &[SessionMessage]) {
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
            let origin = self
                .origin_of
                .get(&source)
                .copied()
                .or(self.chat_employee)
                .unwrap_or(source);
            self.push_to_chat(origin, TranscriptRole::Assistant, delegation.message);
            self.team.set_status(origin, EmployeeStatus::Done);
            return;
        }

        let Some(target) = self.team_index_by_id(&delegation.target) else {
            self.push_log(format!("unknown delegate target '{}'", delegation.target));
            return;
        };
        // Bound the chain depth so `A → B → A → …` can't spin forever.
        let depth = self.depth_of.get(&source).copied().unwrap_or(0);
        if depth >= harness::MAX_CHAIN_DEPTH {
            self.push_log(format!(
                "delegation chain depth limit reached at '{}'",
                delegation.target
            ));
            return;
        }
        self.inherit_chain(source, target);
        self.send_to_employee(target, delegation.message);
    }

    /// Ask Jev to answer `question` and post the verdict back into `source`'s
    /// chat. Without a configured Jev client, falls back to a deterministic "yes".
    fn route_to_jev(&mut self, source: usize, question: String) {
        self.push_to_chat(source, TranscriptRole::Info, format!("🤖 @jev: {question}"));
        let Some(jev) = self.jev.clone() else {
            self.push_to_chat(source, TranscriptRole::Assistant, "yes".to_string());
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let answer = match jev.answer_text(&question).await {
                Ok(Some(answer)) => answer,
                _ => "yes".to_string(),
            };
            let _ = tx
                .send(Message::JevAnswer {
                    employee: source,
                    answer,
                })
                .await;
        });
    }

    /// Fan `goal` out to every teammate in parallel (each interprets it from its
    /// own role), then aggregate their outputs through a reviewer once all finish.
    ///
    /// The source and the aggregator are excluded from the worker set. The
    /// aggregator is the reviewer when one exists and differs from the source,
    /// else the planner, else the source itself.
    fn fan_out(&mut self, source: usize, goal: String) {
        let team = self.team.employees().to_vec();

        // The aggregator is the reviewer when one differs from the source, else
        // the planner, else the source itself.
        let aggregator = ["reviewer", "planner"]
            .iter()
            .find_map(|id| self.team_index_by_id(id).filter(|&i| i != source))
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
            self.send_to_employee(worker, goal.clone());
        }
    }

    /// After a transcript finishes loading, record the employee's deliverable into
    /// any fan-out they belong to; once every worker has contributed, hand the
    /// synthesis to the aggregator.
    fn note_fanout_result(&mut self, employee: usize, messages: &[SessionMessage]) {
        // Record this worker's output into every fan-out they're still owed to.
        let affected: Vec<u64> = self
            .fanouts
            .iter()
            .filter(|(_, f)| f.workers.contains(&employee) && !f.outputs.contains_key(&employee))
            .map(|(id, _)| *id)
            .collect();

        if !affected.is_empty() {
            let output = last_assistant_output(messages).unwrap_or_default();
            let output = if output.is_empty() {
                format!(
                    "⚠ no deliverable from {}",
                    self.team
                        .employees()
                        .get(employee)
                        .map(|e| e.name.clone())
                        .unwrap_or_default()
                )
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

    /// Synthesize a fan-out's collected outputs and send them to the aggregator,
    /// instructing it to return a single deliverable to the user.
    fn aggregate_fanout(&mut self, id: u64) {
        let team = self.team.employees().to_vec();
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
                let name = team
                    .get(*worker)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
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
            team.get(aggregator)
                .map(|e| e.name.clone())
                .unwrap_or_default()
        ));
        self.send_to_employee(aggregator, prompt);
    }

    // --- reliability (timeouts, retry, per-chain isolation) ---

    /// Record that `target` now belongs to the same delegation chain as `source`,
    /// inheriting its origin (the chat the final answer returns to) and bumping
    /// the chain depth one step.
    fn inherit_chain(&mut self, source: usize, target: usize) {
        let origin = self.origin_of.get(&source).copied().unwrap_or(source);
        let depth = self
            .depth_of
            .get(&source)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
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
            self.retry_or_abandon(employee, "timed out");
        }
    }

    /// A hand-off failed or timed out: retry it (bounded) or abandon it.
    fn retry_or_abandon(&mut self, employee: usize, reason: &str) {
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
        let instruction = match self
            .chats
            .get_mut(&employee)
            .and_then(|c| c.pending.as_mut())
        {
            Some(pending) => {
                pending.bump();
                pending.instruction.clone()
            }
            None => return,
        };

        self.push_to_chat(
            employee,
            TranscriptRole::Info,
            format!("🔁 retrying ({reason})"),
        );
        self.send_to_employee(employee, instruction);
    }

    /// Give up on `employee`'s hand-off and record a deterministic `⚠` deliverable
    /// so any fan-out it belongs to still aggregates.
    fn abandon(&mut self, employee: usize, _instruction: &str, reason: &str) {
        let name = self
            .team
            .employees()
            .get(employee)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| employee.to_string());
        let note = harness::abandon_note(&name, reason);
        self.push_to_chat(employee, TranscriptRole::Error, note.clone());
        self.team.set_status(employee, EmployeeStatus::Error);

        // Surface the failure at the chain origin so the user sees it.
        if let Some(origin) = self
            .origin_of
            .get(&employee)
            .copied()
            .filter(|&o| o != employee)
        {
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
        let affected: Vec<u64> = self
            .fanouts
            .iter()
            .filter(|(_, f)| {
                f.workers.contains(&employee) && !f.outputs.contains_key(&employee) && !f.aggregated
            })
            .map(|(id, _)| *id)
            .collect();
        for id in affected {
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
            .employees()
            .get(target)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| target.to_string());
        self.push_to_chat(
            target,
            TranscriptRole::Error,
            format!("delivery failed: {reason}"),
        );
        self.push_log(format!("delivery to {name} failed: {reason}"));

        // Retry only autonomous (delegated) hand-offs, which carry a pending
        // delivery; a direct user prompt just surfaces the error for a manual retry.
        let has_pending = self
            .chats
            .get(&target)
            .is_some_and(|chat| chat.pending.is_some());
        if has_pending {
            self.retry_or_abandon(target, &reason);
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
    fn handle_autonomous_task(&mut self, label: &str, employee_id: String, goal: String) {
        let Some(index) = self.team_index_by_id(&employee_id) else {
            self.push_log(format!(
                "{label} for unknown employee '{employee_id}' — skipped"
            ));
            return;
        };

        let mut task = Task::new(goal.clone(), Some(employee_id.clone()));
        task.transition(TaskStatus::Running);
        self.store.upsert_task(task.clone());
        self.persist_store();

        self.team.set_status(index, EmployeeStatus::Thinking);
        self.team.set_last_task(index, Some(task_summary(&goal)));
        self.push_log(format!(
            "{label} for {employee_id}: {}",
            task_summary(&goal)
        ));

        self.dispatch_task(&task);
    }

    /// Spawn one delivery attempt for `task` in the background.
    fn dispatch_task(&mut self, task: &Task) {
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
        let goal = task.goal.clone();
        let attempts = task.attempts;
        let task_id = task.id;
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let outcome = run_task_attempt(
                &client,
                &directory,
                Some(agent.as_str()),
                model.as_ref(),
                &goal,
                attempts,
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

    /// Handle the outcome of one scheduled-task attempt: succeed, retry (bounded),
    /// or fail — persisting the task and updating the roster status each time.
    fn handle_task_attempt(&mut self, task_id: TaskId, succeeded: bool, result: String) {
        let Some(position) = self.store.tasks.iter().position(|t| t.id == task_id) else {
            return;
        };

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

        if let Some(employee_id) = &assigned
            && let Some(employee) = self.team.employee_mut(employee_id)
        {
            employee.status = status;
        }
        self.persist_store();

        if retry {
            let task = self.store.tasks[position].clone();
            self.dispatch_task(&task);
        }
    }

    /// Persist the store to its configured path (a no-op when persistence is
    /// disabled, e.g. in unit tests).
    fn persist_store(&mut self) {
        let Some(path) = &self.store_path else {
            return;
        };
        if let Err(err) = self.store.save(path) {
            self.push_log(format!("store save failed: {err}"));
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
                        .send(Message::Log(format!("model list failed: {err}")))
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
            self.push_log(format!("could not save team: {err}"));
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
            self.push_log(format!("could not save team: {err}"));
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

    // --- workflow pipeline (roster ordering) ---

    /// Employee storage indices in canonical workflow order (plan → research →
    /// write → review → general), stable by name within a stage. The roster
    /// renders (and the cursor moves) in this order rather than storage order.
    pub fn workflow_order(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.team.len()).collect();
        indices.sort_by_key(|&i| {
            let employee = &self.team.employees()[i];
            (
                Role::from_label(&employee.id).order(),
                employee.name.clone(),
            )
        });
        indices
    }

    /// Move the roster cursor one workflow stage (per [`Self::workflow_order`]).
    fn move_selection_stage(&mut self, delta: isize) {
        let order = self.workflow_order();
        if order.is_empty() {
            return;
        }
        let position = order.iter().position(|&i| i == self.selected).unwrap_or(0);
        let next = (position as isize + delta).clamp(0, order.len() as isize - 1) as usize;
        self.selected = order[next];
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
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            let excess = self.log.len() - LOG_LIMIT;
            self.log.drain(..excess);
        }
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

    /// Live reasoning text for the current chat (surfaced dim while thinking).
    pub fn reasoning_text(&self) -> &str {
        self.current_chat()
            .map_or("", |chat| chat.reasoning_text.as_str())
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

    /// True when a Jev (System One) client is attached — the pipeline diagram
    /// shows the Jev node "on" then.
    pub fn jev_enabled(&self) -> bool {
        self.jev.is_some()
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

    pub fn log(&self) -> &[String] {
        &self.log
    }

    pub fn usage(&self) -> &Usage {
        &self.usage
    }

    pub fn metrics(&self) -> &ProcessMetrics {
        &self.metrics
    }

    /// The persisted scheduled tasks, in insertion order (newest last). The
    /// tasks panel re-sorts them for display via [`order_tasks`].
    pub fn tasks(&self) -> &[Task] {
        &self.store.tasks
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
/// Retrying), then terminal tasks (Succeeded, Failed), each group sorted
/// newest-first by `created_at`. Returns indices into `tasks` so callers keep
/// borrowing the original slice.
pub(crate) fn order_tasks(tasks: &[Task]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..tasks.len()).collect();
    indices.sort_by_key(|&index| {
        let task = &tasks[index];
        let terminal = matches!(task.status, TaskStatus::Succeeded | TaskStatus::Failed);
        (terminal, Reverse(task.created_at))
    });
    indices
}

/// The deliverable text of the last completed assistant reply — delegation
/// directives stripped — used as a worker's contribution during aggregation.
fn last_assistant_output(messages: &[SessionMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role() == "assistant" && !message.is_incomplete())
        .map(|message| strip_directives(&message.display_text()))
        .filter(|text| !text.is_empty())
}

/// Run one scheduled-task attempt: create a session, prompt (with a retry
/// preamble on later attempts), wait for completion, and read the outcome back
/// from the transcript. `Ok` carries the deliverable; `Err` the failure reason.
async fn run_task_attempt(
    client: &OpenCode,
    directory: &str,
    agent: Option<&str>,
    model: Option<&ModelRef>,
    goal: &str,
    attempts: u8,
) -> Result<String, String> {
    let (session, _used_agent) = client
        .create_session_with_agent_fallback(directory, agent, model)
        .await
        .map_err(|err| format!("session failed: {err:#}"))?;
    let session_id = session.id;

    let prompt = if attempts > 0 {
        format!("{}\n\n{}", harness::retry_preamble(attempts), goal)
    } else {
        goal.to_string()
    };

    client
        .prompt(&session_id, &prompt)
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

        let mut answer = serde_json::Map::with_capacity(request.fields.len());
        let mut chosen: Vec<String> = Vec::with_capacity(request.fields.len());
        for field in &request.fields {
            let value = match &jev {
                Some(jev) => match jev.answer_field(field).await {
                    Ok(Some(value)) => Some(value),
                    _ => None,
                },
                None => None,
            };
            let value = value.unwrap_or_else(|| crate::jev::default_answer_field(field));
            chosen.push(format!("{}={}", field.key, value_label(&value)));
            answer.insert(field.key.clone(), value);
        }

        match client.reply_form(&session_id, &form_id, &answer).await {
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
