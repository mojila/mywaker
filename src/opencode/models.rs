//! Serde models mirroring OpenCode HTTP API responses.
//!
//! These are a *tolerant* subset of the documented schemas: `serde` ignores
//! unknown keys and `Option` fields default to `None` when absent, so we only
//! model what the app actually consumes.

use serde::{Deserialize, Serialize};

/// `GET /api/info` response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub version: String,
    #[serde(default)]
    pub pid: Option<u64>,
    #[serde(default)]
    pub urls: Vec<String>,
}

/// Working-directory scope carried by requests and events.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub directory: String,
    #[serde(default, rename = "workspaceID")]
    pub workspace_id: Option<String>,
}

/// A single agent ("employee") from `GET /api/agent`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub permissions: Vec<PermissionRule>,
}

/// A permission rule (`{ action, resource, effect }`) attached to an agent.
#[derive(Debug, Clone, Deserialize)]
pub struct PermissionRule {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub resource: String,
    #[serde(default)]
    pub effect: String,
}

/// Envelope for list endpoints: `{ "location": {...}, "data": [...] }`.
///
/// `location` is optional because the session list (`GET /api/session`) omits it
/// while the agent/model/MCP lists include it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse<T> {
    #[allow(dead_code)]
    #[serde(default)]
    pub location: Option<Location>,
    pub data: Vec<T>,
}

/// Envelope for endpoints that wrap a single resource: `{ "data": {...} }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataEnvelope<T> {
    pub data: T,
}

/// Creation/update timestamps of a session (epoch millis).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub updated: Option<u64>,
}

/// One MCP server and its connection state, from `GET /api/mcp`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    #[serde(default)]
    pub status: Option<McpStatus>,
}

impl McpServer {
    /// True when the server reports itself connected (or reports nothing —
    /// an unknown state is not treated as down).
    pub fn is_connected(&self) -> bool {
        self.status
            .as_ref()
            .map(|s| s.status.as_deref() != Some("disconnected"))
            .unwrap_or(true)
    }
}

/// The nested connection state of an [`McpServer`] (`{ "status": "connected" }`).
#[derive(Debug, Clone, Deserialize)]
pub struct McpStatus {
    #[serde(default)]
    pub status: Option<String>,
}

/// A session returned by `POST /api/session` or `GET /api/session`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, rename = "projectID")]
    pub project_id: Option<String>,
    #[serde(default, rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub location: Option<Location>,
    /// The agent the session was created under (`"orchestrator"`, `"writer"`,
    /// …) — how previous sessions are matched back to an employee.
    #[serde(default)]
    pub agent: Option<String>,
    /// Creation/update timestamps (epoch millis), when the service reports them.
    #[serde(default)]
    pub time: Option<SessionTime>,
}

/// What kind of capability a tool call exercised.
///
/// OpenCode's own tool ids do not say this: `skill` and `subagent` are ordinary
/// built-ins that happen to take an argument naming *another* capability, and an
/// MCP or plugin tool arrives as a namespaced id (`db-audit.run_select_query`)
/// called from inside the code-mode `execute` tool. The class is therefore
/// derived in [`Event::tool_use`], not read off the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// A tool OpenCode ships itself (`shell`, `read`, `edit`, `execute`, …).
    Builtin,
    /// A skill loaded through the `skill` tool.
    Skill,
    /// A sub-agent started through the `subagent` tool.
    Subagent,
    /// A namespaced tool (`ns.function`) whose *source* — MCP server or plugin —
    /// this client cannot resolve from the event alone.
    ///
    /// Deliberately its own class rather than a guess: an MCP server and a plugin
    /// both register namespaced ids, and the event carries no marker that tells
    /// them apart. [`ToolUse::label`] renders it with **no prefix**, so the card
    /// never claims `mcp:` or `plugin:` for a namespace it has not resolved.
    Namespaced,
    /// A namespaced tool known to come from an MCP server.
    ///
    /// Reserved for the deferred source resolution (see [`ToolUse`]); nothing
    /// produces it from an event today.
    Mcp,
    /// A namespaced tool known to come from a plugin. Reserved, like [`Self::Mcp`].
    Plugin,
}

impl ToolClass {
    /// The prefix the card prints before the tool's name, or `None` when the name
    /// stands on its own.
    ///
    /// The prefixes are ASCII on purpose: the card's width budget is counted in
    /// `char`s (`ui::truncate`), so a double-width glyph would push the border
    /// out of shape.
    pub fn prefix(self) -> Option<&'static str> {
        match self {
            ToolClass::Builtin | ToolClass::Namespaced => None,
            ToolClass::Skill => Some("skill:"),
            ToolClass::Subagent => Some("subagent:"),
            ToolClass::Mcp => Some("mcp:"),
            ToolClass::Plugin => Some("plugin:"),
        }
    }
}

/// One tool call, classified: what was used, and the name it was used by.
///
/// This is what the employee grid's `tool` row shows — `shell`, `skill:opencode`,
/// `subagent:backend`, `db-audit.run_select_query` — so a glance says both *what
/// kind* of capability is in play and which one. The chat transcript keeps the
/// raw [`Event::tool_label`] string, which is the event's own wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolUse {
    pub class: ToolClass,
    /// The tool id, the skill id, the sub-agent id, or the namespaced tool id.
    pub name: String,
}

impl ToolUse {
    /// A classified tool call.
    pub fn new(class: ToolClass, name: impl Into<String>) -> Self {
        Self {
            class,
            name: name.into(),
        }
    }

    /// A built-in tool call, by id.
    pub fn builtin(name: impl Into<String>) -> Self {
        Self::new(ToolClass::Builtin, name)
    }

    /// What the card prints: the class prefix (when there is one) and the name.
    pub fn label(&self) -> String {
        match self.class.prefix() {
            Some(prefix) => format!("{prefix}{}", self.name),
            None => self.name.clone(),
        }
    }

    /// The namespace of a namespaced tool (`db-audit` in
    /// `db-audit.run_select_query`), or `None` when the name has no `.`.
    ///
    /// This is the key a future `ToolSources` lookup would use to decide
    /// [`ToolClass::Mcp`] from [`ToolClass::Plugin`]; nothing reads it yet.
    pub fn namespace(&self) -> Option<&str> {
        self.name.split_once('.').map(|(namespace, _)| namespace)
    }
}

/// A single event from the `GET /api/event` SSE stream.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub location: Option<Location>,
    #[serde(default)]
    pub data: serde_json::Value,
    /// Some event surfaces carry their payload under `properties` rather than
    /// `data`; kept raw so both are tolerated.
    #[serde(default)]
    pub properties: serde_json::Value,
}

impl Event {
    /// The session this event belongs to, if it carries one.
    pub fn session_id(&self) -> Option<&str> {
        if let Some(id) = self.data.get("sessionID").and_then(|v| v.as_str()) {
            return Some(id);
        }
        if let Some(id) = self
            .data
            .get("part")
            .and_then(|part| part.get("sessionID"))
            .and_then(|v| v.as_str())
        {
            return Some(id);
        }
        // `form.created` carries no top-level session id: it lives inside the
        // nested form (`data.form.sessionID`).
        self.form_session_id()
    }

    /// The session id of `form.created`'s nested form, from whichever payload
    /// container holds it.
    fn form_session_id(&self) -> Option<&str> {
        self.form_payload()
            .and_then(|form| form.get("sessionID"))
            .and_then(|v| v.as_str())
    }

    /// The raw form object carried by a `form.created` event, unwrapping the
    /// payload container and the `form` key. `None` when the event carries no
    /// form payload at all.
    fn form_payload(&self) -> Option<&serde_json::Value> {
        for payload in [&self.data, &self.properties] {
            if let Some(form) = payload.get("form") {
                return Some(form);
            }
        }
        None
    }

    /// Incoming text delta, for `session.text.delta` / `message.part.delta`.
    pub fn text_delta(&self) -> Option<&str> {
        self.data
            .get("delta")
            .and_then(|v| v.as_str())
            .or_else(|| self.data.get("text").and_then(|v| v.as_str()))
    }

    /// The assistant message a delta belongs to, when the stream names one
    /// (`assistantMessageID`, or a nested `part.messageID`).
    ///
    /// Reasoning and text deltas are keyed on this so a turn that reasons,
    /// answers, and reasons again starts a fresh buffer rather than merging
    /// unrelated thinking into one block.
    pub fn assistant_message_id(&self) -> Option<&str> {
        if let Some(id) = self.data.get("assistantMessageID").and_then(|v| v.as_str()) {
            return Some(id);
        }
        self.data
            .get("part")
            .and_then(|part| part.get("messageID"))
            .and_then(|v| v.as_str())
    }

    /// Full part snapshot, for `message.part.updated`.
    pub fn part(&self) -> Option<&serde_json::Value> {
        self.data.get("part")
    }

    /// Human-readable error, for failed events.
    pub fn error_message(&self) -> Option<&str> {
        self.data
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|v| v.as_str())
    }

    /// Best-effort human label for a tool-activity event (`session.tool.*`).
    /// Prefers an explicit tool/name/title field, then a file path.
    pub fn tool_label(&self) -> Option<String> {
        for key in ["tool", "name", "title"] {
            if let Some(value) = self.data.get(key).and_then(|v| v.as_str())
                && !value.is_empty()
            {
                return Some(value.to_string());
            }
        }
        self.data
            .get("file")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    /// Classify a tool-activity event (`session.tool.*`) for the employee grid's
    /// `tool` row.
    ///
    /// The rules, in order:
    ///
    /// | event | class | name |
    /// |---|---|---|
    /// | `name == "skill"` | [`ToolClass::Skill`] | `input.id` |
    /// | `name == "subagent"` | [`ToolClass::Subagent`] | `input.agent` |
    /// | `name == "execute"` + `metadata.toolCalls[0].tool` | [`ToolClass::Namespaced`] | that id |
    /// | `name == "execute"` + `tools["ns"]` in `input.code` | [`ToolClass::Namespaced`] | `ns` |
    /// | `name == "execute"`, nothing nested | [`ToolClass::Builtin`] | `"execute"` |
    /// | anything else | [`ToolClass::Builtin`] | the name (then `data.file`) |
    ///
    /// **MCP vs plugin is not decided here.** Both register namespaced ids, and
    /// the event carries no marker that separates them, so a namespaced call is
    /// [`ToolClass::Namespaced`] and its [`ToolUse::label`] is printed with **no
    /// prefix** — the row states what it knows and never guesses a source. (A
    /// `ToolSources` set built from config would resolve it; the config mywaker
    /// loads has no MCP/plugin section, so that is deferred rather than faked.)
    ///
    /// Returns `None` for a malformed event — a `skill` with no `input.id`, a
    /// `subagent` with no `input.agent`, or no name at all — so a partial payload
    /// leaves the row alone instead of showing a blank tool.
    ///
    /// The transcript line still uses [`Self::tool_label`], which is the event's
    /// own wording and deliberately unchanged.
    pub fn tool_use(&self) -> Option<ToolUse> {
        let name = self.tool_name()?;
        match name.as_str() {
            "skill" => self
                .tool_input()
                .and_then(|input| input.get("id"))
                .and_then(|v| v.as_str())
                .filter(|id| !id.is_empty())
                .map(|id| ToolUse::new(ToolClass::Skill, id)),
            "subagent" => self
                .tool_input()
                .and_then(|input| input.get("agent"))
                .and_then(|v| v.as_str())
                .filter(|agent| !agent.is_empty())
                .map(|agent| ToolUse::new(ToolClass::Subagent, agent)),
            "execute" => Some(self.execute_use()),
            _ => Some(ToolUse::builtin(name)),
        }
    }

    /// What the code-mode `execute` tool actually ran: the first nested tool call
    /// when the service reported one, else the namespace referenced by the code,
    /// else `execute` itself.
    ///
    /// The nested call is the strongest evidence, so it wins even when the code
    /// names a different namespace.
    fn execute_use(&self) -> ToolUse {
        if let Some(tool) = self
            .tool_metadata()
            .and_then(|metadata| metadata.get("toolCalls"))
            .and_then(|calls| calls.as_array())
            .and_then(|calls| calls.first())
            .and_then(|call| call.get("tool"))
            .and_then(|v| v.as_str())
            .filter(|tool| !tool.is_empty())
        {
            // A dotted id is a namespaced tool; a bare one is just a tool.
            return if tool.contains('.') {
                ToolUse::new(ToolClass::Namespaced, tool)
            } else {
                ToolUse::builtin(tool)
            };
        }
        if let Some(namespace) = self
            .tool_input()
            .and_then(|input| input.get("code"))
            .and_then(|v| v.as_str())
            .and_then(first_tool_namespace)
        {
            return ToolUse::new(ToolClass::Namespaced, namespace);
        }
        ToolUse::builtin("execute")
    }

    /// The tool id an event names, from whichever payload shape carries it.
    ///
    /// Live `session.tool.*` events put it at `data.tool`; message parts call it
    /// `name`. Both are accepted, plus `title` as the last resort — the same order
    /// [`Self::tool_label`] uses, so the card and the transcript agree on which
    /// tool an event is about.
    fn tool_name(&self) -> Option<String> {
        let payloads = [&self.data, &self.properties];
        for payload in payloads {
            for key in ["tool", "name", "title"] {
                if let Some(value) = payload.get(key).and_then(|v| v.as_str())
                    && !value.is_empty()
                {
                    return Some(value.to_string());
                }
            }
        }
        // A `message.part.updated` snapshot nests the part.
        for payload in payloads {
            let Some(part) = payload.get("part") else {
                continue;
            };
            for key in ["tool", "name"] {
                if let Some(value) = part.get(key).and_then(|v| v.as_str())
                    && !value.is_empty()
                {
                    return Some(value.to_string());
                }
            }
        }
        // Last resort, as in `tool_label`: the file the event names. A card that
        // says `tool src/app.rs` is still telling the truth about what ran.
        for payload in payloads {
            if let Some(value) = payload.get("file").and_then(|v| v.as_str())
                && !value.is_empty()
            {
                return Some(value.to_string());
            }
        }
        None
    }

    /// The tool's input object, from the flat event shape or a nested part.
    fn tool_input(&self) -> Option<&serde_json::Value> {
        self.tool_field("input")
    }

    /// The tool's `metadata` object, from the flat event shape or a nested part.
    fn tool_metadata(&self) -> Option<&serde_json::Value> {
        self.tool_field("metadata")
    }

    /// One object field of the tool call, from whichever container holds it.
    ///
    /// Live `session.tool.*` events put it at `data.<field>`; a
    /// `message.part.updated` snapshot nests the whole call under `data.part`
    /// with its arguments and metadata inside `part.state`. Both are searched,
    /// in that order, so one classifier serves both surfaces.
    fn tool_field(&self, field: &str) -> Option<&serde_json::Value> {
        for payload in [&self.data, &self.properties] {
            let candidates = [
                Some(payload),
                payload.get("part"),
                payload.get("part").and_then(|part| part.get("state")),
                payload.get("state"),
            ];
            for container in candidates.into_iter().flatten() {
                if let Some(value) = container.get(field).filter(|v| v.is_object()) {
                    return Some(value);
                }
            }
            // The older alias for a tool's arguments.
            if field == "input"
                && let Some(value) = payload.get("args").filter(|v| v.is_object())
            {
                return Some(value);
            }
        }
        None
    }

    /// True if this event's error signals a provider budget/quota exhaustion.
    pub fn is_budget_exhausted(&self) -> bool {
        self.error_message().is_some_and(is_budget_message)
    }

    /// True when this is a `form.created` event (the session paused waiting on
    /// typed input for one or more form fields).
    pub fn is_form(&self) -> bool {
        self.event_type == "form.created"
    }

    /// The form request this event carries, if any.
    ///
    /// OpenCode v2 nests the form under `form` in the event payload — observed
    /// live as `data.form` (`{"data":{"form":{id, sessionID, title, fields}}}`),
    /// with `properties.form` tolerated for other surfaces. Older/alternate
    /// emissions put the form fields at the payload's top level; both shapes are
    /// accepted so a pending form is never missed.
    pub fn form_request(&self) -> Option<FormRequest> {
        if !self.is_form() {
            return None;
        }
        if let Some(form) = self.form_payload() {
            return serde_json::from_value(form.clone()).ok();
        }
        // Legacy shape: the payload *is* the form.
        let payload = if !self.data.is_null() {
            &self.data
        } else {
            &self.properties
        };
        if payload.is_null() {
            return None;
        }
        serde_json::from_value(payload.clone()).ok()
    }
}

/// Creation/completion timestamps carried by messages and parts.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTime {
    pub created: u64,
    #[serde(default)]
    pub completed: Option<u64>,
}

/// A model reference — OpenCode's `Model.Ref` (`{ "id", "providerID" }`). Used
/// both to describe the model an assistant message ran on ([`SessionMessage`])
/// and, when serialized, as the `model` field of `POST /api/session`, where the
/// provider disambiguates ids that exist under several providers (e.g.
/// `deepseek-v4-pro` on both `kenari` and `clario`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    #[serde(default, rename = "providerID")]
    pub provider_id: Option<String>,
}

/// A message error (provider auth, retry, etc.).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageError {
    #[serde(rename = "type", default)]
    pub error_type: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub status: Option<u64>,
}

impl MessageError {
    /// True if this error means a provider is out of budget/quota. Callers can
    /// surface this (e.g. suggest picking a different model) rather than retry
    /// the same provider.
    pub fn is_budget_exhausted(&self) -> bool {
        let by_status = matches!(self.status, Some(402 | 429));
        let by_type = self.error_type.as_deref().is_some_and(budget_related_type);
        let by_message = self.message.as_deref().is_some_and(is_budget_message);
        by_status || by_type || by_message
    }
}

/// True if an error *type* names a budget/quota/billing condition.
fn budget_related_type(error_type: &str) -> bool {
    let lower = error_type.to_ascii_lowercase();
    [
        "budget",
        "quota",
        "billing",
        "payment",
        "insufficient",
        "rate_limit",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

/// Case-insensitive keyword match for budget/quota-exhaustion messages.
fn is_budget_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "budget",
        "quota",
        "insufficient",
        "rate limit",
        "billing",
        "payment",
        "credit",
        "fund",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

/// The first `tools["ns"]` / `tools['ns']` reference in a code-mode snippet, if
/// any.
///
/// Used only as a fallback: when `execute` reports no `metadata.toolCalls`, the
/// code it ran still names the namespace it reached for.
fn first_tool_namespace(code: &str) -> Option<&str> {
    for (index, _) in code.match_indices("tools[") {
        let rest = code.get(index + "tools[".len()..)?;
        let rest = rest.strip_prefix('"').or_else(|| rest.strip_prefix('\''))?;
        let end = rest.find(['"', '\''])?;
        let namespace = rest.get(..end)?;
        if !namespace.is_empty() {
            return Some(namespace);
        }
    }
    None
}

/// A single content part of an assistant message (text, tool, step, …).
///
/// Only `part_type`/`text` are modeled explicitly; unknown part kinds land in
/// `extra` so the client tolerates schema drift.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(rename = "type", default)]
    pub part_type: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub time: Option<MessageTime>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A transcript message from `GET /api/session/{id}/message`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    #[serde(rename = "type", default)]
    pub message_type: String,
    #[serde(default)]
    pub time: Option<MessageTime>,
    /// Body of a user message.
    #[serde(default)]
    pub text: Option<String>,
    /// Parts of an assistant message.
    #[serde(default)]
    pub content: Vec<Part>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub error: Option<MessageError>,
    /// Token usage for this message (input/output/reasoning + cache), when the
    /// provider reports it on the assistant reply.
    #[serde(default)]
    pub tokens: Option<TokenUsage>,
}

impl SessionMessage {
    /// Canonical role: `user`, `assistant`, or `idle`.
    pub fn role(&self) -> &str {
        if self.message_type.is_empty() {
            "unknown"
        } else {
            &self.message_type
        }
    }

    /// Human-readable text: user body, else joined assistant text parts.
    ///
    /// Only `text` parts are content. `reasoning` is a deliberation, not the
    /// answer (see [`Self::reasoning_text`]), and `tool`/`step` are activity
    /// markers that the transcript renders separately — emitting `[step]` /
    /// `[thinking]` lines here would put non-content into the deliverable, where
    /// it survives [`crate::workflow::strip_directives`] and becomes the answer.
    pub fn display_text(&self) -> String {
        if let Some(text) = &self.text
            && !text.is_empty()
        {
            return text.clone();
        }
        self.content
            .iter()
            .filter(|part| part.part_type == "text")
            .filter_map(|part| part.text.as_deref())
            .map(str::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The message's reasoning, joined — the `thinking` parts only.
    ///
    /// Kept separate from [`Self::display_text`] so a transcript can show the
    /// deliberation as thinking rather than as the answer.
    pub fn reasoning_text(&self) -> String {
        self.content
            .iter()
            .filter(|part| part.part_type == "reasoning")
            .filter_map(|part| part.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// True if this assistant message never finished (still streaming or error).
    pub fn is_incomplete(&self) -> bool {
        self.message_type == "assistant"
            && self
                .finish
                .as_deref()
                .is_none_or(|f| f == "error" || f == "unknown")
    }
}

/// Token usage for a message, as reported by the provider on the assistant
/// reply. `cache` reflects prompt-caching reads/writes; a "cache hit" is
/// `cache.read` serving part of the `input` tokens.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub reasoning: u64,
    #[serde(default)]
    pub cache: CacheUsage,
}

/// Prompt-cache token counts (`read` = served from cache, `write` = newly cached).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    #[serde(default)]
    pub read: u64,
    #[serde(default)]
    pub write: u64,
}

/// One option offered by a [`FormField`] (`{ value, label, description }`).
///
/// The `value` is what the reply endpoint expects back as the answer; `label`
/// is the display text and `description` the longer explanation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormOption {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
}

/// One typed field of a [`FormRequest`] (`form.created`).
///
/// OpenCode v2's "form" flow has typed fields: `string`, `number`, `integer`,
/// `boolean`, `multiselect`, and `external`. Only the subset the app consumes
/// is modeled; unknown keys (e.g. `minLength`, `when`) are ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    /// The field key the answer map is keyed by.
    #[serde(default)]
    pub key: String,
    /// Discriminator: `string` | `number` | `integer` | `boolean` | `multiselect` | `external`.
    #[serde(rename = "type", default)]
    pub field_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub options: Vec<FormOption>,
    /// True when a free-form (custom) entry is allowed alongside the options.
    #[serde(default)]
    pub custom: bool,
    /// The field's default value, if any.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

/// A pending form request: `{ id, sessionID, title, fields }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormRequest {
    /// The form id (`frm_…`); the reply endpoint is scoped by it.
    #[serde(default, rename = "id")]
    pub form_id: Option<String>,
    #[serde(default, rename = "sessionID")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub fields: Vec<FormField>,
}

/// Pagination cursor for message-list responses, carried as `{ previous, next }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesCursor {
    #[serde(default)]
    pub previous: Option<String>,
    #[serde(default)]
    pub next: Option<String>,
}

/// `GET /api/session/{id}/message` response envelope.
///
/// The endpoint defaults to newest-first and pages by an opaque `cursor`, so the
/// client follows `cursor.next` until it has the full (chronological) transcript.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesResponse {
    pub data: Vec<SessionMessage>,
    #[serde(default)]
    pub cursor: Option<MessagesCursor>,
}

/// A model offered through an installed provider, from `GET /api/model`.
///
/// OpenCode lists every model across the user's *existing* providers (e.g.
/// `openrouter`, `clario`, `meta`, `opencode`, `mlx`, `kenari`), so this is
/// the catalog the model selector offers for per-employee assignment.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    /// A provider-qualified id, e.g. `"meta/muse-spark-open"`.
    pub id: String,
    #[serde(default, rename = "modelID")]
    pub model_id: Option<String>,
    #[serde(default, rename = "providerID")]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub capabilities: Option<ModelCapabilities>,
    #[serde(default)]
    pub cost: Option<Costs>,
    #[serde(default)]
    pub limit: Option<ModelLimit>,
    #[serde(default)]
    pub variants: Vec<ModelVariant>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub status: Option<String>,
}

impl ProviderModel {
    /// Per-1M input cost in the provider's unit (USD for most providers).
    pub fn input_cost(&self) -> Option<f64> {
        self.cost.as_ref().and_then(Costs::first).map(|c| c.input)
    }

    /// Per-1M output cost in the provider's unit (USD for most providers).
    pub fn output_cost(&self) -> Option<f64> {
        self.cost.as_ref().and_then(Costs::first).map(|c| c.output)
    }

    pub fn context_length(&self) -> Option<u64> {
        self.limit.as_ref().and_then(|l| l.context)
    }

    pub fn is_tool_capable(&self) -> bool {
        self.capabilities.as_ref().is_some_and(|c| c.tools)
    }

    /// Reasoners expose non-`"none"` variants (low/medium/high/xhigh/thinking).
    pub fn is_reasoning(&self) -> bool {
        self.variants
            .iter()
            .any(|v| !v.id.eq_ignore_ascii_case("none"))
    }

    pub fn input_modalities(&self) -> Vec<String> {
        self.capabilities
            .as_ref()
            .map(|c| c.input.clone())
            .unwrap_or_default()
    }

    pub fn output_modalities(&self) -> Vec<String> {
        self.capabilities
            .as_ref()
            .map(|c| c.output.clone())
            .unwrap_or_default()
    }

    /// The underlying model *vendor*, from the id's first path segment.
    pub fn vendor(&self) -> String {
        self.id.split('/').next().unwrap_or(&self.id).to_string()
    }
}

/// The provider assumed when a model id can't be resolved to its provider via
/// the catalog — Kenari is the app's gateway, so its mirrored entries are the
/// natural default.
pub const DEFAULT_PROVIDER: &str = "kenari";

/// Pick the catalog entry whose `id` equals `model`, preferring
/// [`DEFAULT_PROVIDER`] when several entries share the id (e.g. `deepseek-v4-pro`
/// is mirrored by both `kenari` and `clario`).
pub fn pick_provider<'a>(models: &'a [ProviderModel], model: &str) -> Option<&'a ProviderModel> {
    let viable: Vec<&'a ProviderModel> = models.iter().filter(|m| m.id == model).collect();
    viable
        .iter()
        .copied()
        .find(|m| m.provider_id.as_deref() == Some(DEFAULT_PROVIDER))
        .or_else(|| viable.first().copied())
}

/// `cost` is either a list of `{input,output}` entries (OpenRouter-style) or a
/// single object (some provider configs). `untagged` tolerates both.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Costs {
    List(Vec<ModelCost>),
    Single(ModelCost),
}

impl Costs {
    fn first(&self) -> Option<&ModelCost> {
        match self {
            Costs::List(list) => list.first(),
            Costs::Single(cost) => Some(cost),
        }
    }
}

/// One `cost` entry: per-1M-token price (USD for most providers).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
}

/// Input/output modality + tool support.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Context/output token limits.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

/// A reasoning-effort variant (e.g. `{ "id": "high", ... }`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVariant {
    #[serde(default)]
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `pick_provider` prefers the kenari mirror when an id is shared across
    /// providers; otherwise it returns the first match in catalog order.
    #[test]
    fn pick_provider_prefers_kenari_mirror() {
        fn model(id: &str, provider: &str) -> ProviderModel {
            serde_json::from_value(serde_json::json!({
                "id": id,
                "providerID": provider,
            }))
            .unwrap()
        }

        let models = vec![
            model("deepseek-v4-pro", "clario"),
            model("deepseek-v4-pro", "kenari"),
            model("anthropic/claude-haiku-5.5", "openrouter"),
        ];

        // Shared id → kenari wins regardless of catalog order.
        assert_eq!(
            pick_provider(&models, "deepseek-v4-pro")
                .unwrap()
                .provider_id
                .as_deref(),
            Some("kenari")
        );

        // Unique id → returned directly.
        assert_eq!(
            pick_provider(&models, "anthropic/claude-haiku-5.5")
                .unwrap()
                .provider_id
                .as_deref(),
            Some("openrouter")
        );

        assert!(pick_provider(&models, "unknown/model").is_none());
    }

    #[test]
    fn deserializes_server_connected_event() {
        let raw = r#"{"id":"evt_1","type":"server.connected","data":{}}"#;
        let event: Event = serde_json::from_str(raw).unwrap();
        assert_eq!(event.event_type, "server.connected");
        assert!(event.created.is_none());
        assert!(event.location.is_none());
    }

    #[test]
    fn deserializes_camel_case_session_event() {
        let raw = r#"{"id":"evt_2","created":1792125522235,"type":"shell.exited",
            "location":{"directory":"/x","workspaceID":"w1"},
            "data":{"id":"sh_1","exit":0,"status":"exited"}}"#;
        let event: Event = serde_json::from_str(raw).unwrap();
        assert_eq!(event.created, Some(1792125522235));
        let loc = event.location.unwrap();
        assert_eq!(loc.directory, "/x");
        assert_eq!(loc.workspace_id.as_deref(), Some("w1"));
        assert_eq!(event.data["exit"], 0);
    }

    #[test]
    fn deserializes_agent_list() {
        let raw = r#"{"location":{"directory":"/x"},"data":[{"id":"build","name":"Build","mode":"primary","hidden":false}]}"#;
        let resp: ListResponse<AgentInfo> = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].id, "build");
        assert_eq!(resp.data[0].name.as_deref(), Some("Build"));
        assert_eq!(resp.location.as_ref().unwrap().directory, "/x");
    }

    #[test]
    fn deserializes_session_list_without_location() {
        // The live `GET /api/session` envelope omits the top-level `location`
        // (it is present on /api/agent, /api/model, /api/mcp but not here).
        let raw = r#"{"data":[
            {"id":"ses_1","agent":"writer","model":{"id":"m","providerID":"p","variant":"default"},
             "cost":0,"tokens":{"input":1,"output":0,"reasoning":0,"cache":{"read":0,"write":0}},
             "time":{"created":1792511435072,"updated":1792511435072},
             "location":{"directory":"/Users/ajilaksono/Projects/NUHS"}},
            {"id":"ses_2","projectID":"p1","parentID":"ses_1","agent":"researcher",
             "outcome":"interrupted","time":{"created":1792511356772,"updated":1792511429333,
             "idle":1792511429335,"viewed":1792511429335},
             "title":"Fact sheet","location":{"directory":"/Users/ajilaksono/Projects/NUHS"}}
        ]}"#;
        let resp: ListResponse<SessionInfo> = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].id, "ses_1");
        assert_eq!(resp.data[0].agent.as_deref(), Some("writer"));
        assert_eq!(resp.data[1].parent_id.as_deref(), Some("ses_1"));
        assert_eq!(resp.data[1].title.as_deref(), Some("Fact sheet"));
    }

    #[test]
    fn deserializes_messages_response() {
        // Mirrors a real `GET /api/session/{id}/message` payload (types observed
        // against the live v2.0.30 service): a failed idle marker, an assistant
        // message with a text part, and a user message.
        let raw = r#"{"data":[
            {"id":"msg_a","time":{"created":1792128454136},"type":"idle","outcome":"failed",
             "error":{"type":"provider.auth","message":"Budget limit exceeded","status":403}},
            {"id":"msg_b","time":{"created":1792128452501,"completed":1792128454132},"type":"assistant",
             "agent":"build","model":{"id":"meta/muse-spark-open","providerID":"openrouter"},
             "content":[{"id":"part_1","type":"text","text":"hello world"}],"finish":"error"},
            {"id":"msg_c","time":{"created":1792128452491},"text":"Reply exactly","type":"user"}
        ]}"#;
        let resp: MessagesResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.data.len(), 3);

        let idle = &resp.data[0];
        assert_eq!(idle.role(), "idle");
        assert_eq!(
            idle.error.as_ref().unwrap().message.as_deref(),
            Some("Budget limit exceeded")
        );

        let assistant = &resp.data[1];
        assert_eq!(assistant.role(), "assistant");
        assert_eq!(assistant.display_text(), "hello world");
        assert_eq!(
            assistant.model.as_ref().unwrap().provider_id.as_deref(),
            Some("openrouter")
        );

        let user = &resp.data[2];
        assert_eq!(user.role(), "user");
        assert_eq!(user.display_text(), "Reply exactly");
    }

    #[test]
    fn deserializes_message_cursor_and_asc_page() {
        let raw = r#"{"data":[
            {"id":"m1","time":{"created":1},"text":"go","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"id":"p1","type":"text","text":"hi"}],"finish":"stop"}
        ],
        "cursor":{"previous":null,"next":"eyJzb21lIjoiY3Vyc29yIn0"}}"#;
        let resp: MessagesResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.data.len(), 2);
        let cursor = resp.cursor.expect("cursor present");
        assert!(cursor.previous.is_none());
        assert_eq!(cursor.next.as_deref(), Some("eyJzb21lIjoiY3Vyc29yIn0"));

        // A response with no cursor (final page) still deserializes.
        let last: MessagesResponse =
            serde_json::from_str(r#"{"data":[],"cursor":{"previous":"x","next":null}}"#).unwrap();
        assert!(last.cursor.as_ref().unwrap().next.is_none());
    }

    #[test]
    fn deserializes_form_request_and_fields() {
        // Mirrors a real `form.created` payload: typed fields with options,
        // custom entry, and a default value.
        let raw = r#"{
            "id": "frm_123",
            "sessionID": "ses_1",
            "title": "How should I proceed?",
            "fields": [
                {"key": "action", "type": "string", "title": "Action",
                 "options": [
                     {"value": "implement", "label": "Implement it", "description": "Write the code"},
                     {"value": "plan", "label": "Just plan"}
                 ]},
                {"key": "confirm", "type": "boolean", "title": "Confirm?", "default": true},
                {"key": "tags", "type": "multiselect", "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}]},
                {"key": "note", "type": "string", "custom": true, "placeholder": "anything"}
            ]
        }"#;
        let req: FormRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.form_id.as_deref(), Some("frm_123"));
        assert_eq!(req.session_id.as_deref(), Some("ses_1"));
        assert_eq!(req.fields.len(), 4);

        let action = &req.fields[0];
        assert_eq!(action.key, "action");
        assert_eq!(action.field_type, "string");
        assert_eq!(action.options.len(), 2);
        assert_eq!(action.options[0].value, "implement");
        assert_eq!(action.options[0].label, "Implement it");

        let confirm = &req.fields[1];
        assert_eq!(confirm.field_type, "boolean");
        assert_eq!(confirm.default, Some(serde_json::json!(true)));

        assert_eq!(req.fields[2].field_type, "multiselect");
        assert!(req.fields[3].custom);
    }

    /// Mirrors the live v2.0.32 `form.created` frame exactly: the form is nested
    /// under `data.form`, and the event carries no top-level `sessionID`.
    #[test]
    fn detects_form_event_and_reads_nested_payload() {
        let raw = r#"{"id":"evt_1","created":1792293021060,"type":"form.created",
            "location":{"directory":"/x"},
            "data":{"form":{"id":"frm_123","sessionID":"ses_1",
                "title":"How should I proceed?",
                "fields":[
                    {"key":"confirm","title":"Proceed?","type":"boolean"},
                    {"key":"choice","title":"Pick one","type":"string",
                     "options":[{"value":"a","label":"Alpha"},{"value":"b","label":"Beta"}]}
                ]}}}"#;
        let event: Event = serde_json::from_str(raw).unwrap();
        assert!(event.is_form());
        // The session id must be recovered from the nested form, not the (absent)
        // top-level `data.sessionID`.
        assert_eq!(event.session_id(), Some("ses_1"));

        let req = event.form_request().expect("form payload");
        assert_eq!(req.form_id.as_deref(), Some("frm_123"));
        assert_eq!(req.session_id.as_deref(), Some("ses_1"));
        assert_eq!(req.title, "How should I proceed?");
        assert_eq!(req.fields.len(), 2);
        assert_eq!(req.fields[0].field_type, "boolean");
        assert_eq!(req.fields[1].options[1].value, "b");
    }

    /// A `form.created` whose fields sit at the payload's top level (the older
    /// shape, under `properties` or `data`) still parses.
    #[test]
    fn form_request_tolerates_top_level_payload() {
        let props: Event = serde_json::from_str(
            r#"{"id":"e1","type":"form.created","properties":{"id":"frm_1","sessionID":"ses_9","fields":[]}}"#,
        )
        .unwrap();
        let req = props.form_request().expect("form payload");
        assert_eq!(req.form_id.as_deref(), Some("frm_1"));
        assert_eq!(req.session_id.as_deref(), Some("ses_9"));

        let data: Event = serde_json::from_str(
            r#"{"id":"e2","type":"form.created","data":{"id":"frm_2","sessionID":"ses_8","fields":[]}}"#,
        )
        .unwrap();
        assert_eq!(
            data.form_request().unwrap().form_id.as_deref(),
            Some("frm_2")
        );
    }

    #[test]
    fn non_form_events_carry_no_form_request() {
        let event = Event {
            id: "e".into(),
            created: None,
            event_type: "session.text.delta".into(),
            location: None,
            data: serde_json::json!({"sessionID": "ses_1", "delta": "hi"}),
            properties: serde_json::Value::Null,
        };
        assert!(!event.is_form());
        assert!(event.form_request().is_none());
    }

    #[test]
    fn deserializes_provider_model() {
        // Mirrors a real `GET /api/model` entry (openrouter → Muse Spark Open).
        let raw = r#"{"id":"meta/muse-spark-open","modelID":"meta/muse-spark-open",
            "providerID":"openrouter","name":"Muse Spark Open",
            "capabilities":{"tools":true,"input":["text","image","video","pdf","audio"],"output":["text"]},
            "cost":[{"input":1.25,"output":4.25,"cache":{"read":0.15,"write":0}}],
            "limit":{"context":260000,"output":28000},
            "variants":[{"id":"low"},{"id":"medium"},{"id":"high"},{"id":"xhigh"}],
            "enabled":true,"status":"active"}"#;
        let model: ProviderModel = serde_json::from_str(raw).unwrap();
        assert_eq!(model.id, "meta/muse-spark-open");
        assert_eq!(model.provider_id.as_deref(), Some("openrouter"));
        assert_eq!(model.vendor(), "meta");
        assert_eq!(model.input_cost(), Some(1.25));
        assert_eq!(model.output_cost(), Some(4.25));
        assert_eq!(model.context_length(), Some(260_000));
        assert!(model.is_tool_capable());
        assert!(model.is_reasoning());
        assert!(model.input_modalities().contains(&"image".to_string()));

        // A single-object `cost` (config-style) deserializes too.
        let single = r#"{"id":"x/provider","cost":{"input":3,"output":12}}"#;
        let model: ProviderModel = serde_json::from_str(single).unwrap();
        assert_eq!(model.input_cost(), Some(3.0));
        assert_eq!(model.output_cost(), Some(12.0));
    }

    /// One `session.tool.*` event, built from a raw `data` payload.
    fn tool_event(data: serde_json::Value) -> Event {
        serde_json::from_value(serde_json::json!({
            "id": "e", "type": "session.tool.started", "data": data,
        }))
        .unwrap()
    }

    /// The classification table: every row of it, from payloads shaped like the
    /// ones the service actually emits (verified against real tool parts —
    /// `{type:"tool", name, state:{input, metadata}}`).
    #[test]
    fn tool_use_classifies_the_observed_shapes() {
        // A built-in tool: its own id, no prefix.
        let builtin = tool_event(serde_json::json!({"sessionID":"s","tool":"shell"}))
            .tool_use()
            .expect("a plain tool is classifiable");
        assert_eq!(builtin, ToolUse::builtin("shell"));
        assert_eq!(builtin.class, ToolClass::Builtin);
        assert_eq!(builtin.label(), "shell");
        assert_eq!(builtin.class.prefix(), None);

        // `skill` carries the skill id in `input.id`.
        let skill = tool_event(serde_json::json!({
            "sessionID":"s","tool":"skill","input":{"id":"opencode"}
        }))
        .tool_use()
        .expect("a skill is classifiable");
        assert_eq!(skill, ToolUse::new(ToolClass::Skill, "opencode"));
        assert_eq!(skill.label(), "skill:opencode");

        // `subagent` carries the agent id in `input.agent`.
        let subagent = tool_event(serde_json::json!({
            "sessionID":"s","tool":"subagent","input":{"agent":"backend","prompt":"…"}
        }))
        .tool_use()
        .expect("a sub-agent is classifiable");
        assert_eq!(subagent, ToolUse::new(ToolClass::Subagent, "backend"));
        assert_eq!(subagent.label(), "subagent:backend");

        // `execute` with a nested tool call: that call is what actually ran, and
        // it is namespaced. The row prints it **without** a source prefix — the
        // event cannot say MCP from plugin (A9).
        let nested = tool_event(serde_json::json!({
            "sessionID":"s",
            "tool":"execute",
            "input":{"code":"return await tools[\"db-audit\"].run_select_query({})"},
            "metadata":{"toolCalls":[{"tool":"db-audit.run_select_query","status":"completed"}]}
        }))
        .tool_use()
        .expect("a code-mode call is classifiable");
        assert_eq!(
            nested,
            ToolUse::new(ToolClass::Namespaced, "db-audit.run_select_query")
        );
        assert_eq!(nested.label(), "db-audit.run_select_query");
        assert_eq!(nested.class.prefix(), None, "no invented mcp:/plugin:");
        assert_eq!(nested.namespace(), Some("db-audit"));

        // `execute` with only a `tools["ns"]` reference in the code.
        let from_code = tool_event(serde_json::json!({
            "sessionID":"s",
            "tool":"execute",
            "input":{"code":"await tools['mqtt-test'].broker_status({})"}
        }))
        .tool_use()
        .expect("a namespace in the code is classifiable");
        assert_eq!(from_code, ToolUse::new(ToolClass::Namespaced, "mqtt-test"));

        // `execute` with nothing nested: the code-mode tool itself, a built-in.
        let bare = tool_event(serde_json::json!({
            "sessionID":"s","tool":"execute","input":{"code":"return 1 + 1"}
        }))
        .tool_use()
        .expect("a bare execute is classifiable");
        assert_eq!(bare, ToolUse::builtin("execute"));
        assert_eq!(bare.label(), "execute");

        // A nested call that is not namespaced stays a built-in by its own name.
        let bare_nested = tool_event(serde_json::json!({
            "sessionID":"s",
            "tool":"execute",
            "metadata":{"toolCalls":[{"tool":"read"}]}
        }))
        .tool_use()
        .expect("a nested built-in is classifiable");
        assert_eq!(bare_nested, ToolUse::builtin("read"));
    }

    /// A nested tool call wins over the namespace the code mentions: the report
    /// of what ran beats a string in the source.
    #[test]
    fn a_reported_tool_call_beats_the_code_namespace() {
        let event = tool_event(serde_json::json!({
            "sessionID":"s",
            "tool":"execute",
            "input":{"code":"tools[\"db-audit\"].run_select_query({})"},
            "metadata":{"toolCalls":[{"tool":"mqtt-test.broker_status"}]}
        }));
        assert_eq!(
            event.tool_use(),
            Some(ToolUse::new(
                ToolClass::Namespaced,
                "mqtt-test.broker_status"
            ))
        );
    }

    /// Malformed payloads classify to `None` — never a panic, and never a blank
    /// tool on the card.
    #[test]
    fn tool_use_is_none_for_malformed_events() {
        // No name at all.
        assert!(
            tool_event(serde_json::json!({"sessionID":"s"}))
                .tool_use()
                .is_none()
        );
        // An empty name is not a name.
        assert!(
            tool_event(serde_json::json!({"tool":""}))
                .tool_use()
                .is_none()
        );
        // `skill` without `input.id`.
        assert!(
            tool_event(serde_json::json!({"tool":"skill"}))
                .tool_use()
                .is_none()
        );
        assert!(
            tool_event(serde_json::json!({"tool":"skill","input":{"id":""}}))
                .tool_use()
                .is_none()
        );
        assert!(
            tool_event(serde_json::json!({"tool":"skill","input":"not-an-object"}))
                .tool_use()
                .is_none()
        );
        // `subagent` without `input.agent`.
        assert!(
            tool_event(serde_json::json!({"tool":"subagent","input":{"prompt":"x"}}))
                .tool_use()
                .is_none()
        );
        // `execute` with junk in `toolCalls` still classifies as itself.
        let junk = tool_event(serde_json::json!({
            "tool":"execute",
            "metadata":{"toolCalls":"nope"},
            "input":{"code":42}
        }))
        .tool_use()
        .expect("execute always has a name");
        assert_eq!(junk, ToolUse::builtin("execute"));
    }

    /// The same classification works from a `message.part.updated` snapshot,
    /// where the tool part nests its input under `part.state`.
    #[test]
    fn tool_use_reads_a_nested_part_snapshot() {
        let part: Event = serde_json::from_value(serde_json::json!({
            "id": "e",
            "type": "message.part.updated",
            "data": {"sessionID": "s", "part": {
                "type": "tool",
                "name": "skill",
                "state": {"status": "completed", "input": {"id": "opencode"}}
            }}
        }))
        .unwrap();
        assert_eq!(
            part.tool_use(),
            Some(ToolUse::new(ToolClass::Skill, "opencode"))
        );

        let nested_execute: Event = serde_json::from_value(serde_json::json!({
            "id": "e",
            "type": "message.part.updated",
            "data": {"sessionID": "s", "part": {
                "type": "tool",
                "name": "execute",
                "state": {
                    "input": {"code": "return await tools[\"db-audit\"].run_select_query({})"},
                    "metadata": {"toolCalls": [{"tool": "db-audit.run_select_query"}]}
                }
            }}
        }))
        .unwrap();
        assert_eq!(
            nested_execute.tool_use(),
            Some(ToolUse::new(
                ToolClass::Namespaced,
                "db-audit.run_select_query"
            ))
        );
    }

    /// The transcript's label is untouched by the classification: it stays the
    /// event's own wording, so the chat line reads `🔧 shell`, not `🔧 skill:…`.
    #[test]
    fn tool_label_keeps_its_own_wording() {
        let event = tool_event(serde_json::json!({
            "sessionID":"s","tool":"skill","input":{"id":"opencode"}
        }));
        assert_eq!(event.tool_label().as_deref(), Some("skill"));
        assert_eq!(
            event.tool_use().map(|tool| tool.label()).as_deref(),
            Some("skill:opencode")
        );

        // The file fallback still works for both, as before.
        let by_file = tool_event(serde_json::json!({"sessionID":"s","file":"src/app.rs"}));
        assert_eq!(by_file.tool_label().as_deref(), Some("src/app.rs"));
        assert_eq!(by_file.tool_use(), Some(ToolUse::builtin("src/app.rs")));
    }

    #[test]
    fn event_delta_and_part_helpers() {
        let delta: Event = serde_json::from_str(
            r#"{"id":"e1","type":"session.text.delta","data":{"delta":"hi","sessionID":"ses_1"}}"#,
        )
        .unwrap();
        assert_eq!(delta.text_delta(), Some("hi"));
        assert_eq!(delta.session_id(), Some("ses_1"));

        let part: Event =
            serde_json::from_str(r#"{"id":"e2","type":"message.part.updated","data":{"part":{"id":"p1","type":"text","text":"full","sessionID":"ses_1"}}}"#)
                .unwrap();
        assert_eq!(part.session_id(), Some("ses_1"));
        assert_eq!(part.part().unwrap()["text"], "full");
    }

    #[test]
    fn detects_budget_exhaustion() {
        let budget: MessageError = serde_json::from_str(
            r#"{"type":"provider.auth","message":"Budget limit exceeded","status":403}"#,
        )
        .unwrap();
        assert!(budget.is_budget_exhausted());

        let quota: MessageError =
            serde_json::from_str(r#"{"type":"rate_limit","message":"429 too many requests"}"#)
                .unwrap();
        assert!(quota.is_budget_exhausted());

        let quota_msg: MessageError =
            serde_json::from_str(r#"{"type":"insufficient_quota","message":"quota exceeded"}"#)
                .unwrap();
        assert!(quota_msg.is_budget_exhausted());

        // A normal auth failure (bad key) is NOT treated as budget exhaustion.
        let auth: MessageError = serde_json::from_str(
            r#"{"type":"provider.auth","message":"invalid api key","status":401}"#,
        )
        .unwrap();
        assert!(!auth.is_budget_exhausted());

        // A generic failure is NOT budget exhaustion.
        let generic: MessageError =
            serde_json::from_str(r#"{"type":"model.not_found","message":"no such model"}"#)
                .unwrap();
        assert!(!generic.is_budget_exhausted());

        // An event carrying a budget message is detected too.
        let event: Event = serde_json::from_str(
            r#"{"id":"e","type":"session.execution.failed","data":{"error":{"message":"Budget limit exceeded"}}}"#,
        )
        .unwrap();
        assert!(event.is_budget_exhausted());
    }
}
