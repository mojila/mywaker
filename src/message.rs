//! App-level messages (the `Message` of the TEA event loop).

use crossterm::event::{KeyEvent, MouseEvent};

use crate::opencode::{AgentInfo, Event, OpenCode, ServerInfo, SessionInfo, SessionMessage};

/// A unit of work the app processes once per loop iteration.
#[derive(Debug)]
pub enum Message {
    /// Animation tick from the scheduler.
    Tick,
    /// A terminal key press.
    Key(KeyEvent),
    /// A mouse event: a click or a wheel notch at a screen coordinate.
    Mouse(MouseEvent),
    /// A bracketed paste from the terminal.
    Paste(String),
    /// Terminal resize (width, height).
    Resize(u16, u16),
    /// Connected to the OpenCode service.
    Connected { client: OpenCode, info: ServerInfo },
    /// Connection failed with a human-readable reason.
    ConnectionFailed(String),
    /// The roster of available OpenCode agents is loaded.
    AgentsLoaded(Vec<AgentInfo>),
    /// A live event from the OpenCode stream.
    OpenCodeEvent(Event),
    /// A chat session finished creating (or failed).
    SessionCreated {
        employee: usize,
        result: Result<SessionInfo, String>,
    },
    /// The employee's previous sessions (for the resume/new picker), most
    /// recently updated first and already filtered to that employee's agent.
    PreviousSessionsLoaded {
        employee: usize,
        sessions: Vec<SessionInfo>,
    },
    /// A session transcript finished loading.
    TranscriptLoaded {
        employee: usize,
        messages: Vec<SessionMessage>,
    },
    /// A prompt delivery failed (user or delegated), carrying the target employee
    /// and the reason so the app can retry an autonomous hand-off or surface the
    /// error in the chat.
    DeliveryFailed { target: usize, reason: String },
    /// A chat-local `/skill add` install finished (names of installed skills, or the error).
    SkillInstallDone {
        employee: usize,
        result: Result<Vec<String>, String>,
    },
    /// The models available through OpenCode's providers, for the model picker.
    ModelsLoaded(Vec<crate::opencode::ProviderModel>),
    /// The MCP servers and their connection status, for the sidebar's runtime row.
    McpLoaded(Vec<crate::opencode::McpServer>),
    /// Jev answered a delegated `@jev` question (for `employee`, who asked).
    JevAnswer { employee: usize, answer: String },
    /// A cron schedule fired, handing `goal` to `employee` (autonomous dispatch).
    ScheduledTask { employee: String, goal: String },
    /// A watched path changed, handing `goal` to `employee` (autonomous dispatch).
    WatchedTask { employee: String, goal: String },
    /// One delivery attempt for a scheduled task finished (success or failure).
    TaskAttemptFinished {
        task_id: crate::task::TaskId,
        succeeded: bool,
        result: String,
    },
    /// One delivery attempt for a scheduled task started, reporting the session
    /// it runs in so a pause can interrupt it (and so a pause that landed before
    /// the session existed can still be honoured).
    TaskAttemptStarted {
        task_id: crate::task::TaskId,
        session_id: String,
    },
    /// A free-form line for the status log.
    Log(String),
    /// A free-form error line for the status log, rendered in the error colour.
    LogError(String),
    /// Quit the application.
    Quit,
}
