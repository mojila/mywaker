//! Client for the OpenCode local background service (HTTP API + SSE).

pub mod events;
pub mod models;
pub mod service;

use color_eyre::eyre::{Result, WrapErr};
use reqwest::Url;
use serde::de::DeserializeOwned;

pub use models::{
    AgentInfo, DEFAULT_PROVIDER, Event, FormField, FormOption, FormRequest, Location, MessageError,
    ModelRef, Part, ProviderModel, ServerInfo, SessionInfo, SessionMessage, pick_provider,
};

/// The fixed username OpenCode's local server expects in its `Basic` auth.
const AUTH_USER: &str = "opencode";

/// Build the base64 `Authorization: Basic` credential for the local service.
fn basic_auth(password: &str) -> String {
    use base64::Engine;
    let credentials = format!("{AUTH_USER}:{password}");
    base64::engine::general_purpose::STANDARD.encode(credentials)
}

/// A client for the OpenCode HTTP API.
#[derive(Clone, Debug)]
pub struct OpenCode {
    http: reqwest::Client,
    base: Url,
    auth: String,
}

impl OpenCode {
    /// Discover the local service and build an authenticated client.
    pub async fn connect() -> Result<Self> {
        let base = service::discover().await?.ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "no OpenCode service running; start it with `opencode service start`"
            )
        })?;
        let password = service::password()?;
        Ok(Self::with_base(base, &password))
    }

    /// Build a client for an explicit base URL + password (test/compose helper).
    pub fn with_base(base: Url, password: &str) -> Self {
        let auth = format!("Basic {}", basic_auth(password));
        Self {
            http: reqwest::Client::new(),
            base,
            auth,
        }
    }

    /// Server info / health check.
    pub async fn server_info(&self) -> Result<ServerInfo> {
        self.get_json("/api/info").await
    }

    /// List agents ("employees").
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        let response: models::ListResponse<AgentInfo> = self.get_json("/api/agent").await?;
        Ok(response.data)
    }

    /// List models across the user's existing providers (`GET /api/model`).
    pub async fn models(&self) -> Result<Vec<ProviderModel>> {
        let response: models::ListResponse<ProviderModel> = self.get_json("/api/model").await?;
        Ok(response.data)
    }

    /// Create a session rooted at `directory`, optionally bound to the named
    /// agent (`None` → OpenCode's default agent) and to a model override
    /// (`None` → the agent's own model). Binding each employee to its own agent
    /// is what gives it a role-specific system prompt; the model is what the
    /// user picked for that employee in the model selector.
    ///
    /// The model must be a full [`ModelRef`] (`{ id, providerID }`): OpenCode v2
    /// rejects a bare model id with a 400 (`Expected Model.Ref | null`).
    pub async fn create_session(
        &self,
        directory: &str,
        agent: Option<&str>,
        model: Option<&ModelRef>,
    ) -> Result<SessionInfo> {
        let body = session_body(directory, agent, model);
        let envelope: models::DataEnvelope<SessionInfo> =
            self.post_json("/api/session", &body).await?;
        Ok(envelope.data)
    }

    /// Create a session for `agent`, falling back to OpenCode's default agent
    /// when the named agent isn't defined in the project.
    ///
    /// Binding an employee to a role-specific agent is a nice-to-have (it gives
    /// the session a bespoke system prompt); a session must still start when
    /// that agent hasn't been defined yet. On a `400 Agent not found` this
    /// retries once without the agent. Returns `(session, used_agent)` — the
    /// second field is `false` on the fallback path, so callers can surface a
    /// note instead of a silent downgrade.
    pub async fn create_session_with_agent_fallback(
        &self,
        directory: &str,
        agent: Option<&str>,
        model: Option<&ModelRef>,
    ) -> Result<(SessionInfo, bool)> {
        match self.create_session(directory, agent, model).await {
            Ok(session) => Ok((session, true)),
            Err(err) if agent.is_some() && is_agent_not_found(&err) => {
                let session = self.create_session(directory, None, model).await?;
                Ok((session, false))
            }
            Err(err) => Err(err),
        }
    }

    /// Fetch the full transcript of a session.
    ///
    /// The endpoint returns messages newest-first by default and pages by an
    /// opaque cursor, so we request oldest-first on the first page and then walk
    /// `cursor.next` until the timeline is exhausted. The result is the complete
    /// transcript in chronological order.
    pub async fn messages(&self, session_id: &str) -> Result<Vec<SessionMessage>> {
        let mut all: Vec<SessionMessage> = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut url = self.url(&format!("/api/session/{session_id}/message"))?;
            {
                let mut query = url.query_pairs_mut();
                match &cursor {
                    Some(c) => query.append_pair("cursor", c),
                    None => query.append_pair("order", "asc"),
                };
            }

            let response: models::MessagesResponse = self.get_json_url(url).await?;
            let has_next = response.cursor.as_ref().and_then(|c| c.next.clone());
            all.extend(response.data);
            cursor = has_next;
            if cursor.is_none() {
                break;
            }
        }

        Ok(all)
    }

    /// Send a prompt to a session.
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<()> {
        let body = serde_json::json!({ "text": text });
        let path = format!("/api/session/{session_id}/prompt");
        let _: serde_json::Value = self.post_json(&path, &body).await?;
        Ok(())
    }

    /// Block until the session finishes its turn.
    pub async fn wait(&self, session_id: &str) -> Result<()> {
        let path = format!("/api/experimental/session/{session_id}/wait");
        let _: serde_json::Value = self.post_json(&path, &serde_json::json!({})).await?;
        Ok(())
    }

    /// Interrupt a running session.
    pub async fn interrupt(&self, session_id: &str) -> Result<()> {
        let path = format!("/api/session/{session_id}/interrupt");
        let _: serde_json::Value = self.post_json(&path, &serde_json::json!({})).await?;
        Ok(())
    }

    /// Answer a pending form (`form.created`).
    ///
    /// `answer` maps each field's `key` to its typed value (`Form.Answer`,
    /// i.e. `{ "<key>": <string|number|boolean|string[]> }`). After this the
    /// session resumes its turn.
    pub async fn reply_form(
        &self,
        session_id: &str,
        form_id: &str,
        answer: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let body = serde_json::json!({ "answer": answer });
        let path = format!("/api/session/{session_id}/form/{form_id}/reply");
        let _: serde_json::Value = self.post_json(&path, &body).await?;
        Ok(())
    }

    /// Subscribe to the live event stream.
    pub fn events(&self) -> Result<impl futures::Stream<Item = Result<Event>> + '_> {
        let url = self.base.join("/api/event").wrap_err("invalid event URL")?;
        Ok(events::subscribe(self.http.clone(), url, self.auth.clone()))
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.get_json_url(self.url(path)?).await
    }

    async fn get_json_url<T: DeserializeOwned>(&self, url: Url) -> Result<T> {
        let response = self
            .http
            .get(url.clone())
            .header("authorization", &self.auth)
            .send()
            .await
            .wrap_err_with(|| format!("GET {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(http_status_error("GET", url.as_str(), status, &body));
        }

        response
            .json::<T>()
            .await
            .wrap_err_with(|| format!("decoding GET {url}"))
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let response = self
            .http
            .post(self.url(path)?)
            .header("authorization", &self.auth)
            .json(body)
            .send()
            .await
            .wrap_err_with(|| format!("POST {path}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(http_status_error("POST", path, status, &body));
        }

        response
            .json::<T>()
            .await
            .wrap_err_with(|| format!("decoding POST {path}"))
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base.join(path).wrap_err("invalid request path")
    }
}

/// Build the `POST /api/session` request body. The `model` field must be a
/// [`ModelRef`] object (`{ id, providerID }`): OpenCode v2 rejects a bare model
/// id with a 400 (`Expected Model.Ref | null`).
fn session_body(
    directory: &str,
    agent: Option<&str>,
    model: Option<&ModelRef>,
) -> serde_json::Value {
    let mut body = serde_json::json!({ "location": { "directory": directory } });
    if let Some(agent) = agent {
        body["agent"] = serde_json::Value::String(agent.to_owned());
    }
    if let Some(model) = model {
        body["model"] = serde_json::json!({
            "id": model.id,
            "providerID": model.provider_id,
        });
    }
    body
}

/// Build a descriptive error from a non-2xx response, surfacing OpenCode's
/// `message` field when the body is its standard `{ "_tag", "message", … }`
/// error envelope. This turns the bare `POST /api/session` wrap context into
/// `POST /api/session: 400 Bad Request: Expected Model.Ref | null …`.
fn http_status_error(
    method: &str,
    path: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> color_eyre::eyre::Report {
    let detail = body_detail(body).unwrap_or_else(|| body.trim().to_owned());
    let detail = if detail.is_empty() {
        "(empty body)".to_owned()
    } else {
        detail
    };
    color_eyre::eyre::eyre!("{method} {path}: {status}: {detail}")
}

/// Extract `message` from an OpenCode error body (`{ "message": "…" }`).
fn body_detail(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("message")
        .and_then(|m| m.as_str())
        .map(str::to_owned)
}

/// Whether an error means "the named agent doesn't exist" — i.e. we should fall
/// back to OpenCode's default agent rather than failing the session.
fn is_agent_not_found(err: &color_eyre::eyre::Report) -> bool {
    // `{err:#}` prints the full source chain, so the check holds whether the
    // message is the top error or a wrapped cause.
    format!("{err:#}")
        .to_ascii_lowercase()
        .contains("agent not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_encodes_opencode_user() {
        use base64::Engine;
        let encoded = basic_auth("sekrit");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "opencode:sekrit");
    }

    /// Regression: `create_session` must serialize `model` as OpenCode's
    /// `Model.Ref` object (`{ id, providerID }`), not a bare string.
    #[test]
    fn session_body_serializes_model_as_ref() {
        let model = ModelRef {
            id: "deepseek-v4-pro".into(),
            provider_id: Some("kenari".into()),
        };

        let body = session_body("/x", Some("researcher"), Some(&model));
        assert_eq!(body["location"]["directory"], "/x");
        assert_eq!(body["agent"], "researcher");
        assert_eq!(body["model"]["id"], "deepseek-v4-pro");
        assert_eq!(body["model"]["providerID"], "kenari");
        assert!(!body["model"].is_string(), "model must be an object");

        // No model → no `model` key (the agent's own default applies).
        let minimal = session_body("/x", None, None);
        assert!(minimal.get("model").is_none());
        assert!(minimal.get("agent").is_none());
    }

    /// End-to-end check of discovery + auth + read-only endpoints. Does not
    /// trigger model inference, so it is safe against a live service.
    #[tokio::test]
    #[ignore = "requires a running local OpenCode service"]
    async fn live_smoke_connect_list_agents() {
        let client = OpenCode::connect().await.expect("connect to service");

        let info = client.server_info().await.expect("server info");
        assert!(info.version.starts_with('2'), "unexpected version {info:?}");

        let agents = client.agents().await.expect("list agents");
        assert!(!agents.is_empty(), "expected at least one agent");
        assert!(
            agents.iter().any(|a| a.id == "build"),
            "build agent missing"
        );
    }

    /// End-to-end check of the SSE path: subscribe and read the handshake event.
    #[tokio::test]
    #[ignore = "requires a running local OpenCode service"]
    async fn live_smoke_event_stream() {
        use futures::StreamExt;

        let client = OpenCode::connect().await.expect("connect to service");
        let events = client.events().expect("subscribe");
        futures::pin_mut!(events);

        let mut saw_connected = false;
        for _ in 0..10 {
            let event = match tokio::time::timeout(std::time::Duration::from_secs(5), events.next())
                .await
            {
                Ok(Some(Ok(event))) => event,
                Ok(Some(Err(_))) => continue,
                _ => break,
            };
            if event.event_type == "server.connected" {
                saw_connected = true;
                break;
            }
        }

        assert!(saw_connected, "never observed the server.connected event");
    }
}
