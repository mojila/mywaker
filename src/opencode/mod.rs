//! Client for the OpenCode local background service (HTTP API + SSE).

pub mod events;
pub mod models;
pub mod service;

use color_eyre::eyre::{Result, WrapErr};
use reqwest::Url;
use serde::de::DeserializeOwned;

pub use models::{
    AgentInfo, Event, Location, MessageError, Part, ProviderModel, QuestionInfo, QuestionOption,
    QuestionRequest, ServerInfo, SessionInfo, SessionMessage,
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

    /// Create a session rooted at `directory`.
    pub async fn create_session(&self, directory: &str) -> Result<SessionInfo> {
        let body = serde_json::json!({ "location": { "directory": directory } });
        let envelope: models::DataEnvelope<SessionInfo> =
            self.post_json("/api/session", &body).await?;
        Ok(envelope.data)
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

    /// Answer a pending question request (`question.asked`).
    ///
    /// `answers` lists one entry per question in the request, in order; each
    /// entry is the array of selected option labels (a single element for a
    /// single-choice question). After this the session resumes its turn.
    pub async fn reply_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<()> {
        let body = serde_json::json!({ "answers": answers });
        let path = format!("/api/session/{session_id}/question/{request_id}/reply");
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
            .wrap_err_with(|| format!("GET {url}"))?
            .error_for_status()
            .wrap_err_with(|| format!("GET {url}"))?;

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
            .wrap_err_with(|| format!("POST {path}"))?
            .error_for_status()
            .wrap_err_with(|| format!("POST {path}"))?;

        response
            .json::<T>()
            .await
            .wrap_err_with(|| format!("decoding POST {path}"))
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base.join(path).wrap_err("invalid request path")
    }
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
