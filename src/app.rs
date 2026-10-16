use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;

use crate::config::Config;
use crate::event::EventHandler;
use crate::message::Message;
use crate::opencode::{AgentInfo, Event, OpenCode, ServerInfo, SessionMessage};
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

    mode: Mode,
    chat_employee: Option<usize>,
    chat_session_id: Option<String>,
    input: String,
    transcript: Vec<TranscriptLine>,
    streaming_text: String,
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
            mode: Mode::Roster,
            chat_employee: None,
            chat_session_id: None,
            input: String::new(),
            transcript: Vec::new(),
            streaming_text: String::new(),
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
            Message::TranscriptLoaded(messages) => {
                self.handle_transcript(messages);
            }
            Message::ModelsAssigned(assigned) => {
                let count = assigned.len();
                for (index, model) in assigned {
                    self.team.set_model(index, model);
                }
                self.push_log(format!("routed {count} employees to Kenari models"));
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
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = move_selection(self.selected, -1, self.team.len());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = move_selection(self.selected, 1, self.team.len());
                }
                KeyCode::Enter => self.start_chat(self.selected),
                _ => {}
            },
            Mode::Chat => match key.code {
                KeyCode::Esc => self.close_chat(),
                KeyCode::Enter => self.send_prompt(),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Char(c) => self.input.push(c),
                _ => {}
            },
        }
    }

    // --- chat lifecycle ---

    fn start_chat(&mut self, employee: usize) {
        if self.client.is_none() {
            self.push_log("not connected to OpenCode".to_string());
            return;
        }

        self.mode = Mode::Chat;
        self.chat_employee = Some(employee);
        self.chat_session_id = None;
        self.input.clear();
        self.transcript.clear();
        self.streaming_text.clear();
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
                self.chat_session_id = Some(session.id.clone());
                self.team.set_status(employee, EmployeeStatus::Idle);
                self.push_log(format!("session {} ready", session.id));
                self.transcript.push(TranscriptLine {
                    role: TranscriptRole::Info,
                    text: format!("session {} — type a task and press Enter", session.id),
                });
            }
            Err(err) => {
                self.team.set_status(employee, EmployeeStatus::Error);
                self.push_log(format!("session failed: {err}"));
                self.transcript.push(TranscriptLine {
                    role: TranscriptRole::Error,
                    text: err,
                });
            }
        }
    }

    fn send_prompt(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();

        let Some(session_id) = self.chat_session_id.clone() else {
            self.push_log("no active session".to_string());
            return;
        };
        let Some(employee) = self.chat_employee else {
            return;
        };

        self.transcript.push(TranscriptLine {
            role: TranscriptRole::User,
            text: text.clone(),
        });
        self.streaming_text.clear();
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

    fn close_chat(&mut self) {
        self.mode = Mode::Roster;
        self.chat_employee = None;
        self.chat_session_id = None;
        self.input.clear();
        self.transcript.clear();
        self.streaming_text.clear();
    }

    fn handle_transcript(&mut self, messages: Vec<SessionMessage>) {
        self.transcript = build_transcript(&messages);
        self.streaming_text.clear();

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

        if let Some(employee) = self.chat_employee {
            self.team.set_status(employee, final_status);
        }
    }

    /// Route a streamed OpenCode event into the active chat transcript/status.
    fn handle_stream_event(&mut self, event: &Event) {
        if self.chat_session_id.is_none() {
            return;
        }
        let active = self.chat_session_id.as_deref().expect("checked above");
        // Single active conversation: route events that name our session, or that
        // carry no session id at all (some lifecycle events don't).
        if let Some(sid) = event.session_id()
            && sid != active
        {
            return;
        }

        match event.event_type.as_str() {
            "session.reasoning.started" => self.set_chat_status(EmployeeStatus::Thinking),
            "session.execution.started" => self.set_chat_status(EmployeeStatus::Working),
            "session.text.delta" | "message.part.delta" => {
                if let Some(delta) = event.text_delta() {
                    self.streaming_text.push_str(delta);
                }
            }
            "session.execution.succeeded" => {
                self.set_chat_status(EmployeeStatus::Done);
                self.load_transcript();
            }
            "session.execution.failed" | "session.step.failed" => {
                self.set_chat_status(EmployeeStatus::Error);
                if let Some(reason) = event.error_message() {
                    self.push_log(reason.to_string());
                }
                self.load_transcript();
            }
            _ => {}
        }
    }

    fn set_chat_status(&mut self, status: EmployeeStatus) {
        if let Some(employee) = self.chat_employee {
            self.team.set_status(employee, status);
        }
    }

    fn load_transcript(&mut self) {
        let Some(session_id) = self.chat_session_id.clone() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.messages(&session_id).await {
                Ok(messages) => {
                    let _ = tx.send(Message::TranscriptLoaded(messages)).await;
                }
                Err(err) => {
                    let _ = tx
                        .send(Message::Log(format!("transcript load failed: {err}")))
                        .await;
                }
            }
        });
    }

    /// Kick off a background Kenari fetch that routes each employee to a model,
    /// then assigns the results back through the event channel.
    fn route_team_models(&self) {
        let roster = self.team.employees().to_vec();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match crate::kenari::route_roster(&roster).await {
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
                        .send(Message::Log(format!("kenari route failed: {err}")))
                        .await;
                }
            }
        });
    }

    fn workdir(&self) -> String {
        self.config
            .workdir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .to_string_lossy()
            .to_string()
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
        &self.transcript
    }

    pub fn streaming_text(&self) -> &str {
        &self.streaming_text
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
fn build_transcript(messages: &[SessionMessage]) -> Vec<TranscriptLine> {
    messages
        .iter()
        .map(|message| {
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
            TranscriptLine { role, text }
        })
        .collect()
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
        app.chat_session_id = Some("ses_1".into());
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
        assert_eq!(app.streaming_text, "Hello");

        // A delta for a different session is ignored.
        app.handle_stream_event(&event(
            "session.text.delta",
            serde_json::json!({"sessionID":"ses_9","delta":"x"}),
        ));
        assert_eq!(app.streaming_text, "Hello");
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

        app.handle_transcript(response.data);

        assert_eq!(app.team.employees()[2].status, EmployeeStatus::Done);
        assert_eq!(app.transcript.len(), 2);
        assert_eq!(app.transcript[0].role, TranscriptRole::User);
        assert_eq!(app.transcript[1].text, "hello world");
    }

    #[test]
    fn renders_in_chat_mode() {
        let (mut app, _rx) = make_app();
        open_chat(&mut app, 0);
        app.transcript.push(TranscriptLine {
            role: TranscriptRole::User,
            text: "hi".into(),
        });
        app.streaming_text = "wor".into();
        app.input = "reply".into();

        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    }
}
