//! App-level messages (the `Message` of the TEA event loop).

use crossterm::event::KeyEvent;

use crate::opencode::{AgentInfo, Event, OpenCode, ServerInfo, SessionInfo, SessionMessage};

/// A unit of work the app processes once per loop iteration.
#[derive(Debug)]
pub enum Message {
    /// Animation tick from the scheduler.
    Tick,
    /// A terminal key press.
    Key(KeyEvent),
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
    /// A session transcript finished loading.
    TranscriptLoaded(Vec<SessionMessage>),
    /// A chat-local `/skill add` install finished (names of installed skills, or the error).
    SkillInstallDone {
        employee: usize,
        result: Result<Vec<String>, String>,
    },
    /// Kenari routed models for `(employee index, model id)` pairs.
    ModelsAssigned(Vec<(usize, String)>),
    /// A free-form line for the status log.
    Log(String),
    /// Quit the application.
    Quit,
}
