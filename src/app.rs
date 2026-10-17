use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::config::Config;
use crate::event::EventHandler;
use crate::jev::JevClient;
use crate::message::Message;
use crate::opencode::{AgentInfo, Event, OpenCode, ProviderModel, ServerInfo, SessionMessage};
use crate::projects::{self, Project};
use crate::role::Role;
use crate::team::{EmployeeStatus, Team, move_selection};
use crate::tui::Tui;
use crate::ui;
use crate::workflow::{
    Delegation, is_final, is_jev, is_parallel, parse_delegations, strip_directives, team_context,
};

/// Maximum number of retained status-log lines.
const LOG_LIMIT: usize = 64;

/// Cap on the total hand-offs dispatched in one run, to stop a delegation
/// cycle (A → B → A → …) from spinning forever.
const MAX_DELEGATIONS: u64 = 64;

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
    should_quit: bool,

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
    /// Selection index in the model picker list.
    model_selected: usize,

    mode: Mode,
    chat_employee: Option<usize>,
    input: String,
    /// Cursor position within `input`, in characters (0..=len).
    input_cursor: usize,
    /// Last known terminal size, for page-scroll estimation.
    last_size: (u16, u16),
    /// Per-employee chat state, keyed by roster index.
    chats: HashMap<usize, Chat>,
    /// Selected index in the slash-command autocomplete menu, when it is open
    /// (i.e. the input starts with `/` and matches one or more commands).
    command_menu: Option<usize>,
    /// Total hand-offs dispatched this run, guarding against runaway cycles.
    delegation_count: u64,
    /// The employee whose chat the user last addressed directly; the final
    /// answer in a delegation chain is delivered back here.
    delegation_origin: Option<usize>,
    /// Jev (System One) decision client, used to answer `@jev` questions and
    /// OpenCode `form.created` prompts. `None` when no Kenari key is set.
    jev: Option<JevClient>,
    /// Active parallel fan-outs, keyed by a monotonic id.
    fanouts: HashMap<u64, FanOut>,
    /// Monotonic id source for parallel fan-outs.
    next_fanout_id: u64,
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
            should_quit: false,
            projects: Vec::new(),
            project_roots: Vec::new(),
            active_project: None,
            project_selected: 0,
            models: Vec::new(),
            model_selected: 0,
            mode: Mode::Roster,
            chat_employee: None,
            input: String::new(),
            input_cursor: 0,
            last_size: (80, 24),
            chats: HashMap::new(),
            command_menu: None,
            delegation_count: 0,
            delegation_origin: None,
            jev: None,
            fanouts: HashMap::new(),
            next_fanout_id: 0,
        }
    }

    /// Attach the Jev (System One) decision client, used to answer `@jev`
    /// questions and OpenCode `form.created` prompts.
    pub fn set_jev(&mut self, jev: Option<JevClient>) {
        self.jev = jev;
    }

    pub async fn run(&mut self, tui: &mut Tui, mut events: EventHandler) -> Result<()> {
        loop {
            tui.draw(|frame| ui::render(frame, self))?;

            let message = events.next().await?;
            self.update(message);

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => self.frame = self.frame.wrapping_add(1),
            Message::Resize(w, h) => self.last_size = (w, h),
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
            Message::SkillInstallDone { employee, result } => {
                self.handle_skill_install_done(employee, result);
            }
            Message::ModelsLoaded(models) => {
                self.push_log(format!("{} models available", models.len()));
                self.models = models;
                self.model_selected = self.model_selected.min(self.models.len().saturating_sub(1));
            }
            Message::JevAnswer { employee, answer } => {
                self.push_to_chat(employee, TranscriptRole::Assistant, answer);
            }
            Message::Log(line) => self.push_log(line),
            Message::Quit => self.should_quit = true,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C quits from anywhere except chat, where it interrupts the
        // running generation instead.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.mode == Mode::Chat {
                self.handle_chat_key(key);
                return;
            }
            self.should_quit = true;
            return;
        }

        match self.mode {
            Mode::Roster => match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.should_quit = true,
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
                KeyCode::Esc => self.mode = Mode::Roster,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.model_selected =
                        move_selection(self.model_selected, -1, self.models.len());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.model_selected = move_selection(self.model_selected, 1, self.models.len());
                }
                KeyCode::Enter => self.assign_selected_model(),
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
        self.command_menu = None;
    }

    // --- input editing (cursor-based line editor) ---

    /// Byte offset of the cursor within `self.input`, at a char boundary.
    fn input_byte_offset(&self) -> usize {
        self.input
            .char_indices()
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    /// Char boundaries of `self.input` (len + 1 elements, both ends included).
    fn char_boundaries(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.input.char_indices().map(|(i, _)| i).collect();
        v.push(self.input.len());
        v
    }

    fn cursor_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    fn cursor_right(&mut self) {
        if self.input_cursor < self.input.chars().count() {
            self.input_cursor += 1;
        }
    }

    fn cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    fn cursor_end(&mut self) {
        self.input_cursor = self.input.chars().count();
    }

    fn input_insert(&mut self, c: char) {
        let at = self.input_byte_offset();
        self.input.insert(at, c);
        self.input_cursor += 1;
        self.reset_hist_navigation();
    }

    fn input_delete_backward(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let bounds = self.char_boundaries();
        let start = bounds[self.input_cursor - 1];
        let end = bounds[self.input_cursor];
        self.input.replace_range(start..end, "");
        self.input_cursor -= 1;
        self.reset_hist_navigation();
    }

    fn input_delete_forward(&mut self) {
        if self.input_cursor >= self.input.chars().count() {
            return;
        }
        let bounds = self.char_boundaries();
        let start = bounds[self.input_cursor];
        let end = bounds[self.input_cursor + 1];
        self.input.replace_range(start..end, "");
        self.reset_hist_navigation();
    }

    fn input_delete_word(&mut self) {
        let mut c = self.input_cursor;
        while c > 0
            && self
                .input
                .chars()
                .nth(c - 1)
                .is_some_and(|ch| ch.is_whitespace())
        {
            c -= 1;
        }
        while c > 0
            && self
                .input
                .chars()
                .nth(c - 1)
                .is_some_and(|ch| !ch.is_whitespace())
        {
            c -= 1;
        }
        let bounds = self.char_boundaries();
        let start = bounds[c];
        let end = bounds[self.input_cursor];
        self.input.replace_range(start..end, "");
        self.input_cursor = c;
        self.reset_hist_navigation();
        self.sync_command_menu();
    }

    fn input_delete_to_start(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let end = self.input_byte_offset();
        self.input.replace_range(..end, "");
        self.input_cursor = 0;
        self.reset_hist_navigation();
        self.sync_command_menu();
    }

    /// Insert a bracketed paste into the input at the cursor.
    fn handle_paste(&mut self, text: &str) {
        if self.mode != Mode::Chat {
            return;
        }
        let at = self.input_byte_offset();
        self.input.insert_str(at, text);
        self.input_cursor += text.chars().count();
        self.reset_hist_navigation();
        self.sync_command_menu();
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
        self.input_cursor = self.input.chars().count();
        self.command_menu = None;
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
        self.input_cursor = self.input.chars().count();
        self.command_menu = None;
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
            chat.scroll = 0;
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
        let model = employee_conf.and_then(|e| e.model.clone());
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .create_session(&directory, agent.as_deref(), model.as_deref())
                .await
                .map_err(|err| err.to_string());
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
            }
            Err(err) => {
                self.team.set_status(employee, EmployeeStatus::Error);
                self.push_log(format!("session failed: {err}"));
                if let Some(chat) = self.chats.get_mut(&employee) {
                    chat.creating = false;
                    chat.transcript
                        .push(TranscriptLine::new(TranscriptRole::Error, err));
                }
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
        self.command_menu = None;

        // Chat-local slash commands are handled here and never sent to the
        // OpenCode session (e.g. `/skill add <source>`).
        if self.dispatch_chat_command(&text) {
            return;
        }

        let Some(employee) = self.chat_employee else {
            return;
        };
        let Some(session_id) = self
            .chats
            .get(&employee)
            .and_then(|chat| chat.session_id.clone())
        else {
            self.push_log("no active session".to_string());
            return;
        };

        if let Some(chat) = self.chats.get_mut(&employee) {
            chat.transcript
                .push(TranscriptLine::new(TranscriptRole::User, text.clone()));
            chat.streaming_text.clear();
            chat.reasoning_text.clear();
            chat.scroll = 0;
            chat.scroll = 0;
            // Record history (dedup consecutive repeats), reset navigation.
            if chat.history.last() != Some(&text) {
                chat.history.push(text.clone());
            }
            chat.hist_index = None;
        }
        self.team.set_status(employee, EmployeeStatus::Thinking);
        self.team.set_last_task(employee, Some(task_summary(&text)));
        // Remember where the chain started so the eventual final answer routes
        // back to the chat the user is actually looking at.
        self.delegation_origin = Some(employee);

        // The first prompt to an employee carries the team roster + delegation
        // protocol; later prompts go through verbatim.
        let prompt = self.bootstrap_prompt(employee, text.clone());

        let client = match self.client.clone() {
            Some(client) => client,
            None => return,
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(err) = client.prompt(&session_id, &prompt).await {
                let _ = tx.send(Message::Log(format!("prompt failed: {err}"))).await;
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
        self.command_menu = None;
        // Leave the chat state (session + transcript) intact so re-entering the
        // same employee resumes the conversation instead of starting fresh.
    }

    fn handle_transcript(&mut self, employee: usize, messages: Vec<SessionMessage>) {
        let Some(chat) = self.chats.get_mut(&employee) else {
            return;
        };
        chat.transcript = build_transcript(&messages);
        chat.streaming_text.clear();
        chat.reasoning_text.clear();
        chat.scroll = 0;
        chat.scroll = 0;

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
            let prompt = self.bootstrap_prompt(employee, instruction);
            self.team.set_status(employee, EmployeeStatus::Thinking);
            let session_id = session_id.clone();
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(err) = client.prompt(&session_id, &prompt).await {
                    let _ = tx
                        .send(Message::Log(format!("delegated prompt failed: {err}")))
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
            if self.delegation_count >= MAX_DELEGATIONS {
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
            let origin = self.delegation_origin.take().or(self.chat_employee);
            if let Some(employee) = origin {
                self.push_to_chat(employee, TranscriptRole::Assistant, delegation.message);
                self.team.set_status(employee, EmployeeStatus::Done);
            }
            return;
        }

        let Some(target) = self.team_index_by_id(&delegation.target) else {
            self.push_log(format!("unknown delegate target '{}'", delegation.target));
            return;
        };
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

        // The final deliverable routes back to wherever the user last addressed.
        self.delegation_origin = self.delegation_origin.or(Some(source));

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

    /// Open the model picker and fetch the models available through OpenCode's
    /// providers so the user can choose one for the selected employee.
    fn open_models(&mut self) {
        if self.client.is_none() {
            self.push_log("not connected to OpenCode".to_string());
            return;
        }
        self.model_selected = 0;
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

    /// Assign the highlighted model to the currently selected employee,
    /// persisting the roster and returning to the dashboard.
    fn assign_selected_model(&mut self) {
        let Some(model) = self.models.get(self.model_selected).map(|m| m.id.clone()) else {
            return;
        };
        let employee = self.selected;
        self.team.set_model(employee, model.clone());
        self.push_log(format!(
            "{} → model {}",
            self.team
                .employees()
                .get(employee)
                .map(|e| e.name.as_str())
                .unwrap_or("employee"),
            model,
        ));
        if let Some(path) = Team::default_path()
            && let Err(err) = self.team.save(&path)
        {
            self.push_log(format!("could not save team: {err}"));
        }
        self.mode = Mode::Roster;
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

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    /// Cursor position within the input, in characters (0..=len).
    pub fn input_cursor(&self) -> usize {
        self.input_cursor
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
}

/// A short, single-line summary of a task — the goal an employee is handed —
/// for the tasks panel: first non-empty line, whitespace collapsed, truncated.
fn task_summary(text: &str) -> String {
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

    let client = match OpenCode::connect().await {
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
mod tests {
    use super::*;

    fn make_app() -> (App, tokio::sync::mpsc::Receiver<Message>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<Message>(16);
        (App::new(Config::default(), tx), rx)
    }

    #[test]
    fn task_summary_takes_first_line_and_truncates() {
        assert_eq!(task_summary("draft the intro"), "draft the intro");
        // Blank leading line skipped, internal whitespace collapsed.
        assert_eq!(
            task_summary("\n   research   the best   runtime  \nmore\n"),
            "research the best runtime"
        );
        // Long input truncates to 48 chars with an ellipsis.
        let long = "x".repeat(100);
        let summary = task_summary(&long);
        assert_eq!(summary.chars().count(), 48);
        assert!(summary.ends_with('…'));
        // Empty input yields an empty string.
        assert_eq!(task_summary("   \n  "), "");
    }

    fn event(event_type: &str, data: serde_json::Value) -> Event {
        serde_json::from_value(serde_json::json!({
            "id": "e", "type": event_type, "data": data,
        }))
        .unwrap()
    }

    fn open_chat(app: &mut App, employee: usize) {
        app.mode = Mode::Chat;
        app.chat_employee = Some(employee);
        app.chats.entry(employee).or_default().session_id = Some("ses_1".into());
    }

    /// The first prompt to an employee carries the team roster + delegation
    /// protocol; subsequent prompts are verbatim.
    #[test]
    fn first_prompt_injects_team_context_once() {
        let (mut app, _rx) = make_app();
        app.chats.entry(0).or_default();

        let first = app.bootstrap_prompt(0, "hello".to_string());
        assert!(first.contains("hello"));
        assert!(first.contains("@writer"));
        assert!(first.contains("@user"));

        let second = app.bootstrap_prompt(0, "again".to_string());
        assert_eq!(second, "again");
    }

    /// A completed assistant reply carrying `@writer …` relays the instruction
    /// to the writer and advances the hand-off counter.
    #[test]
    fn dispatch_delegations_routes_to_teammate() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        // The writer (index 1) already has a live session to receive into.
        app.chats.entry(1).or_default().session_id = Some("ses_w".into());

        let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
            {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text":"Here's the plan.\n@writer draft the doc"}],"finish":"done"}
        ]))
        .unwrap();

        app.handle_transcript(0, messages);

        assert_eq!(app.delegation_count, 1);
        assert!(
            app.chats[&1]
                .transcript
                .iter()
                .any(|line| line.text.contains("draft the doc"))
        );
    }

    /// A `@jev` directive routes the question to the decision model and posts the
    /// verdict back into the asking employee's chat (deterministic "yes" when no
    /// Jev client is attached).
    #[test]
    fn dispatch_delegations_routes_question_to_jev() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
            {"id":"m1","time":{"created":1},"text":"review","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text":"@jev is this plan sound?"}],"finish":"done"}
        ]))
        .unwrap();

        app.handle_transcript(0, messages);

        assert_eq!(app.delegation_count, 1);
        assert!(
            app.chats[&0]
                .transcript
                .iter()
                .any(|line| line.text.contains("is this plan sound?"))
        );
        assert!(
            app.chats[&0]
                .transcript
                .iter()
                .any(|line| line.role == TranscriptRole::Assistant && line.text == "yes")
        );
    }

    /// A `@user`/`@deliver` directive hands the final answer back to the origin
    /// employee's chat and clears the tracked origin.
    #[test]
    fn dispatch_delegations_delivers_final_answer() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 3); // planner
        app.delegation_origin = Some(3);

        let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
            {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text":"done\n@user the final summary"}],"finish":"done"}
        ]))
        .unwrap();

        app.handle_transcript(3, messages);

        assert_eq!(app.delegation_count, 1);
        assert!(app.delegation_origin.is_none());
        assert!(
            app.chats[&3]
                .transcript
                .iter()
                .any(|line| line.role == TranscriptRole::Assistant
                    && line.text == "the final summary")
        );
    }

    /// Re-loading the same transcript does not re-dispatch the same hand-off.
    #[test]
    fn dispatch_delegations_is_idempotent_per_message_id() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        app.chats.entry(1).or_default().session_id = Some("ses_w".into());

        let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
            {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text":"@writer draft"}],"finish":"done"}
        ]))
        .unwrap();

        app.handle_transcript(0, messages.clone());
        app.handle_transcript(0, messages);

        assert_eq!(app.delegation_count, 1);
    }

    /// `@parallel` fans a goal out to every teammate but the source and the
    /// aggregator (reviewer), registering a fan-out group and dispatching each.
    #[test]
    fn parallel_directive_fans_out_to_workers() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 3); // planner is the source
        app.delegation_origin = Some(3);
        app.chats.entry(0).or_default().session_id = Some("ses_r".into());
        app.chats.entry(1).or_default().session_id = Some("ses_w".into());

        let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
            {"id":"m1","time":{"created":1},"text":"ship it","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text":"Let's parallelize.\n@parallel build the launch plan"}],"finish":"done"}
        ]))
        .unwrap();

        app.handle_transcript(3, messages);

        // One fan-out, workers = researcher + writer, aggregator = reviewer.
        assert_eq!(app.fanouts.len(), 1);
        let (_, fanout) = app.fanouts.iter().next().unwrap();
        assert_eq!(fanout.workers, vec![0, 1]);
        assert_eq!(fanout.aggregator, 2);
        assert_eq!(fanout.goal, "build the launch plan");

        // The goal reached the workers…
        assert!(
            app.chats[&0]
                .transcript
                .iter()
                .any(|l| l.text.contains("build the launch plan"))
        );
        assert!(
            app.chats[&1]
                .transcript
                .iter()
                .any(|l| l.text.contains("build the launch plan"))
        );
        // …but not the aggregator (reviewer), which only synthesises later.
        assert!(!app.chats.contains_key(&2));
    }

    /// Once every worker contributes, the fan-out aggregates their outputs and
    /// hands the synthesis to the reviewer.
    #[test]
    fn fan_out_aggregates_once_all_workers_finish() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 3);
        app.delegation_origin = Some(3);
        app.fan_out(3, "ship it".to_string());

        let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
            serde_json::from_value(serde_json::json!([
                {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
                {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
                 "content":[{"type":"text","text": text}],"finish":"done"}
            ]))
            .unwrap()
        };

        // Researcher finishes; the fan-out is not yet complete.
        app.handle_transcript(0, assistant("r", "Market research findings."));
        assert!(!app.fanouts.values().next().unwrap().aggregated);

        // Writer finishes; now aggregation fires and the reviewer receives the
        // synthesized prompt (queued since the reviewer has no live session yet).
        app.handle_transcript(1, assistant("w", "Draft copy for the launch."));
        let (_, fanout) = app.fanouts.iter().next().unwrap();
        assert!(fanout.aggregated);

        let outbox = &app.chats[&2].outbox;
        assert_eq!(outbox.len(), 1);
        assert!(outbox[0].contains("Market research findings"));
        assert!(outbox[0].contains("Draft copy for the launch"));
    }

    #[test]
    fn streams_deltas_in_order_and_ignores_other_sessions() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        app.handle_stream_event(&event(
            "session.text.delta",
            serde_json::json!({"sessionID":"ses_1","delta":"Hel"}),
        ));
        app.handle_stream_event(&event(
            "message.part.delta",
            serde_json::json!({"sessionID":"ses_1","delta":"lo"}),
        ));
        assert_eq!(app.streaming_text(), "Hello");

        // A delta for a different session is ignored.
        app.handle_stream_event(&event(
            "session.text.delta",
            serde_json::json!({"sessionID":"ses_9","delta":"x"}),
        ));
        assert_eq!(app.streaming_text(), "Hello");
    }

    #[test]
    fn streams_to_owning_employee_even_when_not_viewed() {
        let (mut app, _rx) = make_app();
        // The user is viewing employee 0; employee 1 has its own session.
        open_chat(&mut app, 0);
        app.chats.entry(1).or_default().session_id = Some("ses_2".into());

        app.handle_stream_event(&event(
            "session.text.delta",
            serde_json::json!({"sessionID":"ses_2","delta":"hi from 1"}),
        ));

        // The viewed employee 0 is untouched; employee 1's chat got the delta.
        assert_eq!(app.streaming_text(), "");
        assert_eq!(app.chats[&1].streaming_text, "hi from 1");

        // Status transitions also route to the owning employee, not the viewer.
        app.handle_stream_event(&event(
            "session.execution.started",
            serde_json::json!({"sessionID":"ses_2"}),
        ));
        assert_eq!(app.team.employees()[1].status, EmployeeStatus::Working);
        assert_eq!(app.team.employees()[0].status, EmployeeStatus::Idle);
    }

    #[test]
    fn slash_menu_filters_navigates_and_completes() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        // Typing '/' opens the menu with all seven commands.
        app.handle_chat_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        let (options, selected) = app.command_options().expect("menu open");
        assert_eq!(options.len(), 7);
        assert_eq!(options[selected].command, "/help");

        // Typing 's' narrows to the four '/s*' commands.
        app.handle_chat_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let (options, selected) = app.command_options().expect("menu open");
        assert_eq!(options.len(), 4);
        assert_eq!(selected, 0);
        assert_eq!(options[0].command, "/stop");

        // Four downs wrap back to the first option.
        for _ in 0..4 {
            app.handle_chat_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let (_, selected) = app.command_options().unwrap();
        assert_eq!(selected, 0);

        // Enter inserts the highlighted command and closes the menu.
        app.handle_chat_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input, "/stop");
        assert!(app.command_options().is_none());
        assert!(app.command_menu.is_none());
    }

    #[test]
    fn slash_command_completion_appends_arg_space() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        for c in "/skill add".chars() {
            app.handle_chat_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        // input "/skill add" matches only itself.
        let (options, selected) = app.command_options().unwrap();
        assert_eq!(options.len(), 1);
        assert_eq!(options[selected].command, "/skill add");

        // Completing an arg-taking command appends a trailing space for the arg.
        app.handle_chat_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input, "/skill add ");
        assert!(app.command_menu.is_none());
    }

    #[test]
    fn cursor_editing_inserts_and_deletes_at_position() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        for c in ['a', 'b', 'c'] {
            app.input_insert(c);
        }
        assert_eq!(app.input, "abc");
        assert_eq!(app.input_cursor(), 3);

        app.cursor_home();
        app.input_insert('X');
        assert_eq!(app.input, "Xabc");
        assert_eq!(app.input_cursor(), 1);

        app.cursor_right(); // now between 'a' and 'b'
        app.input_delete_forward();
        assert_eq!(app.input, "Xac");

        app.input_delete_backward(); // removes 'a'
        assert_eq!(app.input, "Xc");
        assert_eq!(app.input_cursor(), 1);
    }

    #[test]
    fn history_recall_cycles_and_restores_draft() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        app.input = "first".into();
        app.send_prompt();
        app.input = "second".into();
        app.send_prompt();
        assert_eq!(app.chats[&0].history, vec!["first", "second"]);

        app.history_up();
        assert_eq!(app.input, "second");
        app.history_up();
        assert_eq!(app.input, "first");
        app.history_up(); // clamped at oldest
        assert_eq!(app.input, "first");
        app.history_down();
        assert_eq!(app.input, "second");
        app.history_down(); // past newest → restores the (empty) draft
        assert_eq!(app.input, "");
    }

    #[test]
    fn scroll_transcript_moves_from_and_back_to_tail() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        assert_eq!(app.chat_scroll(), 0);

        app.scroll_transcript(-1); // page up → scroll up from the tail
        assert!(app.chat_scroll() > 0);

        app.scroll_transcript(1); // page down → back to the live tail
        assert_eq!(app.chat_scroll(), 0);
    }

    #[test]
    fn interrupt_clears_streaming_and_notes() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        app.chats.get_mut(&0).unwrap().streaming_text = "partial".into();
        app.chats.get_mut(&0).unwrap().reasoning_text = "think".into();

        app.interrupt_current();

        let chat = &app.chats[&0];
        assert!(chat.streaming_text.is_empty());
        assert!(chat.reasoning_text.is_empty());
        assert!(
            chat.transcript
                .iter()
                .any(|l| l.text.contains("interrupted"))
        );
    }

    #[test]
    fn new_command_resets_session_binding() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        assert_eq!(app.chats[&0].session_id.as_deref(), Some("ses_1"));

        app.input = "/new".into();
        app.send_prompt();

        assert!(app.chats[&0].session_id.is_none());
        assert!(app.chats[&0].creating);
    }

    #[test]
    fn reasoning_deltas_accumulate() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);

        app.handle_stream_event(&event(
            "session.reasoning.delta",
            serde_json::json!({"sessionID":"ses_1","delta":"thin"}),
        ));
        app.handle_stream_event(&event(
            "session.reasoning.delta",
            serde_json::json!({"sessionID":"ses_1","delta":"king"}),
        ));

        assert_eq!(app.reasoning_text(), "thinking");
    }

    #[test]
    fn status_transitions_on_lifecycle_events() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 1);

        app.handle_stream_event(&event(
            "session.execution.started",
            serde_json::json!({"sessionID":"ses_1"}),
        ));
        assert_eq!(app.team.employees()[1].status, EmployeeStatus::Working);

        app.handle_stream_event(&event(
            "session.reasoning.started",
            serde_json::json!({"sessionID":"ses_1"}),
        ));
        assert_eq!(app.team.employees()[1].status, EmployeeStatus::Thinking);

        // Failure marks the employee errored (transcript load short-circuits:
        // no client is attached in this unit test).
        app.handle_stream_event(&event(
            "session.execution.failed",
            serde_json::json!({"sessionID":"ses_1","error":{"message":"boom"}}),
        ));
        assert_eq!(app.team.employees()[1].status, EmployeeStatus::Error);
        assert!(app.log.iter().any(|line| line.contains("boom")));
    }

    #[test]
    fn transcript_loading_derives_final_status() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 2);

        let response: crate::opencode::models::MessagesResponse =
            serde_json::from_value(serde_json::json!({"data":[
                {"id":"m1","time":{"created":1},"text":"go","type":"user"},
                {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
                 "content":[{"id":"p1","type":"text","text":"hello world"}],"finish":"done"}
            ]}))
            .unwrap();

        app.handle_transcript(2, response.data);

        assert_eq!(app.team.employees()[2].status, EmployeeStatus::Done);
        assert_eq!(app.transcript().len(), 2);
        assert_eq!(app.transcript()[0].role, TranscriptRole::User);
        assert_eq!(app.transcript()[1].text, "hello world");
    }

    #[test]
    fn build_transcript_orders_chronologically_and_drops_empty_idle() {
        // The message endpoint returns newest-first by default; the transcript
        // must re-sort by creation time and drop the contentless idle marker.
        let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
            {"id":"m3","time":{"created":300},"type":"idle","outcome":"succeeded"},
            {"id":"m2","time":{"created":200,"completed":250},"type":"assistant",
             "content":[{"type":"text","text":"hello"}],"finish":"stop"},
            {"id":"m1","time":{"created":100},"text":"go","type":"user"}
        ]))
        .unwrap();

        let transcript = build_transcript(&messages);

        assert_eq!(transcript.len(), 2, "idle marker with no text is dropped");
        assert_eq!(transcript[0].role, TranscriptRole::User);
        assert_eq!(transcript[0].text, "go");
        assert_eq!(transcript[1].role, TranscriptRole::Assistant);
        assert_eq!(transcript[1].text, "hello");
    }

    #[test]
    fn workflow_order_sorts_by_stage_then_name() {
        let (app, _rx) = make_app();
        // Starter storage order: researcher(0), writer(1), reviewer(2),
        // planner(3). Workflow order should be planner → researcher → writer →
        // reviewer.
        let ids: Vec<&str> = app
            .workflow_order()
            .iter()
            .map(|&i| app.team.employees()[i].id.as_str())
            .collect();
        assert_eq!(ids, vec!["planner", "researcher", "writer", "reviewer"]);
    }

    #[test]
    fn renders_pipeline_with_mixed_statuses() {
        let (mut app, _rx) = make_app();
        app.team.set_status(3, EmployeeStatus::Thinking); // planner
        app.team.set_status(0, EmployeeStatus::Working); // researcher
        app.selected = 3;

        let backend = ratatui::backend::TestBackend::new(120, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    }

    #[test]
    fn renders_in_chat_mode() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        let chat = app.chats.get_mut(&0).unwrap();
        chat.transcript
            .push(TranscriptLine::new(TranscriptRole::User, "hi"));
        chat.streaming_text = "wor".into();
        app.input = "reply".into();

        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    }

    #[test]
    fn renders_slash_command_menu_without_panicking() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        app.input = "/s".into();
        app.command_menu = Some(1);

        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    }

    #[test]
    fn renders_projects_mode() {
        let (mut app, _rx) = make_app();
        app.mode = Mode::Projects;
        app.projects.push(Project {
            name: "alpha".into(),
            path: "/tmp/alpha".into(),
        });
        app.projects.push(Project {
            name: "beta".into(),
            path: "/tmp/beta".into(),
        });
        app.active_project = app.projects.first().cloned();
        app.project_selected = 0;

        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    }
}
