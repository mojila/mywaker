use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::config::Config;
use crate::event::EventHandler;
use crate::message::Message;
use crate::opencode::{AgentInfo, Event, OpenCode, ServerInfo, SessionMessage};
use crate::projects::{self, Project};
use crate::route::Role;
use crate::team::{EmployeeStatus, Team, move_selection};
use crate::tui::Tui;
use crate::ui;

/// Maximum number of retained status-log lines.
const LOG_LIMIT: usize = 64;

/// Which screen is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Roster,
    Chat,
    Projects,
}

/// A rendered conversation line.
#[derive(Debug, Clone)]
pub struct TranscriptLine {
    pub role: TranscriptRole,
    pub text: String,
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
    /// Providers (sources) that hit a budget/quota limit; skipped on re-route.
    exhausted_sources: HashSet<String>,
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

    mode: Mode,
    chat_employee: Option<usize>,
    input: String,
    /// Per-employee chat state, keyed by roster index.
    chats: HashMap<usize, Chat>,
    /// Selected index in the slash-command autocomplete menu, when it is open
    /// (i.e. the input starts with `/` and matches one or more commands).
    command_menu: Option<usize>,
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
            exhausted_sources: HashSet::new(),
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
            mode: Mode::Roster,
            chat_employee: None,
            input: String::new(),
            chats: HashMap::new(),
            command_menu: None,
        }
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
            Message::Resize(_, _) => {}
            Message::Key(key) => self.handle_key(key),
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
                self.route_team_models();
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
            Message::ModelsAssigned(assigned) => {
                let count = assigned.len();
                for (index, model) in assigned {
                    self.team.set_model(index, model);
                }
                self.push_log(format!("routed {count} employees to models"));
            }
            Message::Log(line) => self.push_log(line),
            Message::Quit => self.should_quit = true,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C quits from anywhere.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
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
            Mode::Chat => self.handle_chat_key(key),
        }
    }

    /// Handle a key press while in chat: slash-command menu navigation first,
    /// then ordinary input editing.
    fn handle_chat_key(&mut self, key: KeyEvent) {
        let menu_open = self.command_menu.is_some();
        match key.code {
            KeyCode::Esc => {
                if menu_open {
                    // Dismiss the menu and clear the partial command.
                    self.command_menu = None;
                    self.input.clear();
                } else {
                    self.close_chat();
                }
            }
            KeyCode::Up => {
                if menu_open {
                    self.move_command_menu(-1);
                }
            }
            KeyCode::Down => {
                if menu_open {
                    self.move_command_menu(1);
                }
            }
            KeyCode::Enter => {
                if menu_open {
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
            KeyCode::Backspace => {
                self.input.pop();
                self.sync_command_menu();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
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

    // --- chat lifecycle ---

    fn start_chat(&mut self, employee: usize) {
        if self.client.is_none() {
            self.push_log("not connected to OpenCode".to_string());
            return;
        }

        self.mode = Mode::Chat;
        self.chat_employee = Some(employee);
        self.input.clear();
        self.command_menu = None;

        let chat = self.chats.entry(employee).or_default();

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

        chat.creating = true;
        chat.transcript.clear();
        chat.streaming_text.clear();
        self.team.set_status(employee, EmployeeStatus::Thinking);

        let directory = self.workdir();
        let client = self.client.clone().expect("checked above");
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .create_session(&directory)
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
                    chat.transcript.push(TranscriptLine {
                        role: TranscriptRole::Info,
                        text: format!("session {id} — type a task and press Enter"),
                    });
                }
            }
            Err(err) => {
                self.team.set_status(employee, EmployeeStatus::Error);
                self.push_log(format!("session failed: {err}"));
                if let Some(chat) = self.chats.get_mut(&employee) {
                    chat.creating = false;
                    chat.transcript.push(TranscriptLine {
                        role: TranscriptRole::Error,
                        text: err,
                    });
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
            chat.transcript.push(TranscriptLine {
                role: TranscriptRole::User,
                text: text.clone(),
            });
            chat.streaming_text.clear();
        }
        self.team.set_status(employee, EmployeeStatus::Thinking);

        let client = match self.client.clone() {
            Some(client) => client,
            None => return,
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(err) = client.prompt(&session_id, &text).await {
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

        // `/help` — show the chat-local commands.
        if rest == "help" {
            self.push_transcript_line(
                TranscriptRole::Info,
                "chat commands:\n  /skill add <source>  install & attach a skill\n  /skill <name>        attach an installed skill\n  /skills              list installed skills"
                    .to_string(),
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
            chat.transcript.push(TranscriptLine { role, text });
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
            chat.transcript.push(TranscriptLine { role, text });
        }
    }

    fn close_chat(&mut self) {
        self.mode = Mode::Roster;
        self.chat_employee = None;
        self.input.clear();
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

        // Fall back across providers when a source reports budget exhaustion.
        let mut newly = false;
        for source in exhausted_sources_in(&messages) {
            if self.exhausted_sources.insert(source.clone()) {
                self.push_log(format!(
                    "budget exhausted on provider '{source}' — falling back"
                ));
                newly = true;
            }
        }
        if newly {
            self.route_team_models();
        }
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
            "session.reasoning.started" => self.team.set_status(employee, EmployeeStatus::Thinking),
            "session.execution.started" => self.team.set_status(employee, EmployeeStatus::Working),
            "session.text.delta" | "message.part.delta" => {
                if let Some(delta) = event.text_delta()
                    && let Some(chat) = self.chats.get_mut(&employee)
                {
                    chat.streaming_text.push_str(delta);
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

    /// Kick off a background cross-provider fetch that routes each employee to a
    /// model (skipping budget-exhausted sources), then assigns the results back
    /// through the event channel.
    fn route_team_models(&self) {
        let roster = self.team.employees().to_vec();
        let exhausted = self.exhausted_sources.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match crate::route::route_roster_excluding(&roster, &exhausted).await {
                Ok(selections) => {
                    let assigned: Vec<(usize, String)> = selections
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, selection)| {
                            selection.choice.map(|choice| (index, choice.id))
                        })
                        .collect();
                    let _ = tx.send(Message::ModelsAssigned(assigned)).await;
                }
                Err(err) => {
                    let _ = tx
                        .send(Message::Log(format!("model route failed: {err}")))
                        .await;
                }
            }
        });
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

    pub fn transcript(&self) -> &[TranscriptLine] {
        static EMPTY: &[TranscriptLine] = &[];
        self.current_chat()
            .map_or(EMPTY, |chat| chat.transcript.as_slice())
    }

    pub fn streaming_text(&self) -> &str {
        self.current_chat()
            .map_or("", |chat| chat.streaming_text.as_str())
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

    pub fn chat_employee_name(&self) -> Option<&str> {
        self.chat_employee
            .and_then(|index| self.team.employees().get(index))
            .map(|employee| employee.name.as_str())
    }

    pub fn is_connected(&self) -> bool {
        self.server_info.is_some()
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
            (role != TranscriptRole::Info || !text.is_empty())
                .then_some(TranscriptLine { role, text })
        })
        .collect()
}

/// Providers (by `model.providerID`) that reported a budget/quota exhaustion in
/// this transcript. These become fallback exclusions on the next re-route.
fn exhausted_sources_in(messages: &[SessionMessage]) -> HashSet<String> {
    let mut sources = HashSet::new();
    for message in messages {
        if message
            .error
            .as_ref()
            .is_some_and(|e| e.is_budget_exhausted())
            && let Some(source) = message
                .model
                .as_ref()
                .and_then(|m| m.provider_id.as_deref())
        {
            sources.insert(source.to_string());
        }
    }
    sources
}

/// Connect to OpenCode and forward live events + status to the app channel.
///
/// Runs for the lifetime of the app: after the initial connect handshake it
/// subscribes to the event stream and forwards every event. Holding the `tx`
/// clone in this loop also keeps the channel open so the receiver never ends.
pub async fn opencode_background(tx: Sender<Message>) {
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
            Ok(event) => Message::OpenCodeEvent(event),
            Err(err) => Message::Log(format!("event error: {err}")),
        };
        if tx.send(message).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> (App, tokio::sync::mpsc::Receiver<Message>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<Message>(16);
        (App::new(Config::default(), tx), rx)
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

        // Typing '/' opens the menu with all four commands.
        app.handle_chat_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        let (options, selected) = app.command_options().expect("menu open");
        assert_eq!(options.len(), 4);
        assert_eq!(options[selected].command, "/help");

        // Typing 's' narrows to the three '/s*' commands.
        app.handle_chat_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let (options, selected) = app.command_options().expect("menu open");
        assert_eq!(options.len(), 3);
        assert_eq!(selected, 0);
        assert_eq!(options[0].command, "/skills");

        // Three downs wrap back to the first option.
        for _ in 0..3 {
            app.handle_chat_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let (_, selected) = app.command_options().unwrap();
        assert_eq!(selected, 0);

        // Enter inserts the highlighted command and closes the menu.
        app.handle_chat_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input, "/skills");
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
    fn budget_error_excludes_provider_source() {
        use crate::opencode::models::{MessageError, ModelRef, SessionMessage};

        // A failed assistant message carries a budget error plus its provider.
        let messages = vec![SessionMessage {
            error: Some(MessageError {
                error_type: Some("provider.auth".into()),
                message: Some("Budget limit exceeded".into()),
                status: Some(403),
            }),
            model: Some(ModelRef {
                id: "anthropic/claude-opus-5.5".into(),
                provider_id: Some("openrouter".into()),
            }),
            ..serde_json::from_value(serde_json::json!({
                "id":"m2","time":{"created":1,"completed":2},"type":"assistant"
            }))
            .unwrap()
        }];

        let sources = exhausted_sources_in(&messages);
        assert_eq!(sources.len(), 1);
        assert!(sources.contains("openrouter"));

        // Non-budget errors do NOT yield an exclusion.
        let mut other = messages.clone();
        other[0].error = Some(MessageError {
            error_type: Some("provider.auth".into()),
            message: Some("invalid api key".into()),
            status: Some(401),
        });
        assert!(exhausted_sources_in(&other).is_empty());
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
        chat.transcript.push(TranscriptLine {
            role: TranscriptRole::User,
            text: "hi".into(),
        });
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
