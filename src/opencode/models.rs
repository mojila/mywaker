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
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse<T> {
    #[allow(dead_code)]
    pub location: Location,
    pub data: Vec<T>,
}

/// Envelope for endpoints that wrap a single resource: `{ "data": {...} }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataEnvelope<T> {
    pub data: T,
}

/// A session returned by `POST /api/session`.
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
    /// V2 events carry their payload under `properties` (e.g. `form.created`)
    /// rather than `data`; kept raw so both surfaces are tolerated.
    #[serde(default)]
    pub properties: serde_json::Value,
}

impl Event {
    /// The session this event belongs to, if it carries one.
    pub fn session_id(&self) -> Option<&str> {
        if let Some(id) = self.data.get("sessionID").and_then(|v| v.as_str()) {
            return Some(id);
        }
        self.data
            .get("part")
            .and_then(|part| part.get("sessionID"))
            .and_then(|v| v.as_str())
    }

    /// Incoming text delta, for `session.text.delta` / `message.part.delta`.
    pub fn text_delta(&self) -> Option<&str> {
        self.data
            .get("delta")
            .and_then(|v| v.as_str())
            .or_else(|| self.data.get("text").and_then(|v| v.as_str()))
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

    /// True if this event's error signals a provider budget/quota exhaustion.
    pub fn is_budget_exhausted(&self) -> bool {
        self.error_message().is_some_and(is_budget_message)
    }

    /// True when this is a `form.created` event (the session paused waiting on
    /// typed input for one or more form fields).
    pub fn is_form(&self) -> bool {
        self.event_type == "form.created"
    }

    /// The form request this event carries, if any. The payload lives under
    /// `properties` on the v2 surface and `data` otherwise; we accept whichever
    /// is populated.
    pub fn form_request(&self) -> Option<FormRequest> {
        let payload = if !self.properties.is_null() {
            &self.properties
        } else {
            &self.data
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
    pub fn display_text(&self) -> String {
        if let Some(text) = &self.text
            && !text.is_empty()
        {
            return text.clone();
        }
        let mut parts: Vec<String> = Vec::new();
        for part in &self.content {
            match part.part_type.as_str() {
                "text" => {
                    if let Some(text) = part.text.as_deref() {
                        parts.push(text.to_string());
                    }
                }
                "tool" => parts.push(part_tool_label(part)),
                "step" => parts.push("[step]".to_string()),
                "reasoning" => parts.push("[thinking]".to_string()),
                _ => {}
            }
        }
        parts.join("\n")
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

/// Best-effort label for a tool part, e.g. `[tool: read]`.
fn part_tool_label(part: &Part) -> String {
    if let Some(name) = part.extra.get("tool").and_then(|v| v.as_str())
        && !name.is_empty()
    {
        format!("[tool: {name}]")
    } else {
        "[tool]".to_string()
    }
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

    #[test]
    fn detects_form_event_and_reads_payload() {
        let event: Event = serde_json::from_str(
            r#"{"id":"e1","type":"form.created","properties":{"id":"frm_1","sessionID":"ses_9","fields":[]}}"#,
        )
        .unwrap();
        assert!(event.is_form());
        assert!(
            !Event {
                id: "e".into(),
                created: None,
                event_type: "session.text.delta".into(),
                location: None,
                data: serde_json::Value::Null,
                properties: serde_json::Value::Null,
            }
            .is_form()
        );
        let req = event.form_request().expect("form payload");
        assert_eq!(req.form_id.as_deref(), Some("frm_1"));
        assert_eq!(req.session_id.as_deref(), Some("ses_9"));
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
