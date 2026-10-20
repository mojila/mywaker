//! Integration test against a fake OpenCode HTTP server (axum).
//!
//! Exercises the `opencode` client end-to-end — service handshake, agents,
//! session creation, prompt/wait, and transcript read-back — against a
//! controlled local server, so CI needs no real OpenCode service.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mywaker::opencode::OpenCode;
use reqwest::Url;
use serde_json::{Value, json};

/// One prompt as the server received it: the text, the `delivery` knob, and the
/// admission `id` (if any).
#[derive(Clone, Debug, Default)]
struct ReceivedPrompt {
    text: String,
    delivery: Option<String>,
    admission_id: Option<String>,
}

/// Shared fake-server state: the prompts and requested session agents it has
/// received, in order.
#[derive(Clone, Default)]
struct FakeState {
    prompts: Arc<Mutex<Vec<String>>>,
    received: Arc<Mutex<Vec<ReceivedPrompt>>>,
    session_agents: Arc<Mutex<Vec<Option<String>>>>,
}

async fn info() -> Json<Value> {
    Json(json!({ "version": "2.0.30", "pid": 4242, "urls": [] }))
}

async fn agents() -> Json<Value> {
    Json(json!({
        "location": { "directory": "/work" },
        "data": [
            { "id": "researcher", "name": "Aria", "mode": "primary" },
            { "id": "writer", "name": "Bruno", "mode": "primary" }
        ]
    }))
}

async fn create_session(State(state): State<FakeState>, Json(body): Json<Value>) -> Response {
    let agent = body.get("agent").and_then(Value::as_str).map(str::to_owned);

    // Simulate a project that defined agents for everyone except "missing".
    if agent.as_deref() == Some("missing") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "Agent not found: missing" })),
        )
            .into_response();
    }

    state.session_agents.lock().unwrap().push(agent);
    (
        StatusCode::OK,
        Json(json!({ "data": { "id": "ses_test", "title": "fake session" } })),
    )
        .into_response()
}

async fn prompt(
    State(state): State<FakeState>,
    Path(_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let admission_id = body.get("id").and_then(Value::as_str).map(str::to_owned);

    // OpenCode types a prompt's `id` as `Session.Message.ID` (`^msg_`) and
    // rejects anything else with a 400 — the exact failure this fake must
    // reproduce, or a regression in the client goes unnoticed.
    if let Some(id) = &admission_id
        && !id.starts_with("msg_")
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "_tag": "InvalidRequestError",
                "message": "Expected a string starting with \"msg_\"\n  at [\"id\"]",
                "kind": "Payload",
            })),
        )
            .into_response();
    }

    let received = ReceivedPrompt {
        text: text.clone(),
        delivery: body
            .get("delivery")
            .and_then(Value::as_str)
            .map(str::to_owned),
        admission_id,
    };
    state.prompts.lock().unwrap().push(text);
    state.received.lock().unwrap().push(received);
    Json(json!({})).into_response()
}

async fn messages(State(state): State<FakeState>, Path(_id): Path<String>) -> Json<Value> {
    let last = state
        .prompts
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap_or_default();
    Json(json!({
        "data": [
            { "id": "m1", "time": { "created": 1 }, "text": last, "type": "user" },
            { "id": "m2", "time": { "created": 2, "completed": 3 }, "type": "assistant",
              "content": [{ "id": "p1", "type": "text", "text": "the deliverable" }], "finish": "done" }
        ],
        "cursor": { "previous": null, "next": null }
    }))
}

async fn wait(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({}))
}

/// `GET /api/session?directory=…` — the previous-sessions list, shaped like the
/// live service: sessions across agents, each carrying `agent` and `time`.
async fn list_sessions() -> Json<Value> {
    Json(json!({
        "location": { "directory": "/work" },
        "data": [
            { "id": "ses_old_writer", "title": "draft the release notes", "agent": "writer",
              "time": { "created": 100, "updated": 300 } },
            { "id": "ses_old_orchestrator", "title": "run the pipeline", "agent": "orchestrator",
              "time": { "created": 200, "updated": 400 } },
            { "id": "ses_older_writer", "title": "earlier writer work", "agent": "writer",
              "time": { "created": 10, "updated": 50 } }
        ]
    }))
}

/// `GET /api/mcp` — one connected and one (deliberately) disconnected server,
/// so the summary has something to count on both sides.
async fn mcp() -> Json<Value> {
    Json(json!({
        "location": { "directory": "/work" },
        "data": [
            { "name": "filesystem", "status": { "status": "connected" } },
            { "name": "browser", "status": { "status": "disconnected" } },
            { "name": "gateway", "status": { "status": "connected" } }
        ]
    }))
}

fn router(state: FakeState) -> Router {
    Router::new()
        .route("/api/info", get(info))
        .route("/api/agent", get(agents))
        .route("/api/session", post(create_session).get(list_sessions))
        .route("/api/mcp", get(mcp))
        .route("/api/session/{id}/prompt", post(prompt))
        .route("/api/session/{id}/message", get(messages))
        .route("/api/experimental/session/{id}/wait", post(wait))
        .with_state(state)
}

/// Start the fake server on an ephemeral port and return a client + a handle to
/// the (shared) server state so tests can assert on what it recorded.
async fn start_server() -> (OpenCode, FakeState) {
    let state = FakeState::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router_state = state.clone();
    tokio::spawn(async move {
        axum::serve(listener, router(router_state)).await.unwrap();
    });

    let base = Url::parse(&format!("http://{addr}")).unwrap();
    let client = OpenCode::with_base(base, "sekrit");
    (client, state)
}

#[tokio::test]
async fn client_exercises_info_agents_and_session_flow() {
    let (client, _state) = start_server().await;

    // Health / version.
    let info = client.server_info().await.unwrap();
    assert_eq!(info.version, "2.0.30");

    // Agents.
    let agents = client.agents().await.unwrap();
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].id, "researcher");

    // Session + prompt + wait + transcript.
    let session = client
        .create_session("/work", Some("researcher"), None)
        .await
        .unwrap();
    assert_eq!(session.id, "ses_test");

    client.prompt("ses_test", "draft a plan").await.unwrap();
    client.wait("ses_test").await.unwrap();

    let messages = client.messages("ses_test").await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role(), "user");
    assert_eq!(messages[0].display_text(), "draft a plan");
    assert_eq!(messages[1].role(), "assistant");
    assert_eq!(messages[1].display_text(), "the deliverable");
}

#[tokio::test]
async fn prompt_is_recorded_on_the_server() {
    let (client, state) = start_server().await;

    client.prompt("ses_test", "hello fake").await.unwrap();

    let recorded = state.prompts.lock().unwrap();
    assert_eq!(recorded.as_slice(), ["hello fake".to_string()]);
}

#[tokio::test]
async fn wait_endpoint_is_accepted() {
    let (client, _state) = start_server().await;
    client.create_session("/work", None, None).await.unwrap();
    // The experimental wait endpoint can drift; here it is served and accepted.
    assert!(client.wait("ses_test").await.is_ok());
}

#[tokio::test]
async fn missing_agent_falls_back_to_default() {
    let (client, state) = start_server().await;

    // "missing" is not a defined agent: the server rejects it, and the client
    // must retry without the agent so the session still starts.
    let (session, used_agent) = client
        .create_session_with_agent_fallback("/work", Some("missing"), None)
        .await
        .unwrap();
    assert_eq!(session.id, "ses_test");
    assert!(!used_agent, "fallback path should report the downgrade");

    // The server saw the rejected agent first, then a session with no agent.
    let agents = state.session_agents.lock().unwrap();
    assert_eq!(agents.as_slice(), [None]);
}

#[tokio::test]
async fn defined_agent_binds_directly() {
    let (client, state) = start_server().await;

    let (session, used_agent) = client
        .create_session_with_agent_fallback("/work", Some("researcher"), None)
        .await
        .unwrap();
    assert_eq!(session.id, "ses_test");
    assert!(used_agent, "defined agent should bind on the first try");

    let agents = state.session_agents.lock().unwrap();
    assert_eq!(agents.as_slice(), [Some("researcher".to_string())]);
}

/// A plain `prompt` is the explicit-steer path: it sends `delivery: "steer"` and
/// no admission id, which is what a user typing in a chat means.
#[tokio::test]
async fn prompt_steers_by_default() {
    let (client, state) = start_server().await;
    client.create_session("/work", None, None).await.unwrap();

    client.prompt("ses_test", "do it now").await.unwrap();

    let received = state.received.lock().unwrap().clone();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].text, "do it now");
    assert_eq!(received[0].delivery.as_deref(), Some("steer"));
    assert_eq!(received[0].admission_id, None, "no id unless one is given");
}

/// `prompt_with` sends the delivery knob and the admission id it is given, so a
/// queued hand-off waits its turn and a resend is deduplicated by the service.
#[tokio::test]
async fn prompt_with_sends_delivery_and_admission_id() {
    let (client, state) = start_server().await;
    client.create_session("/work", None, None).await.unwrap();

    let options = mywaker::opencode::PromptOptions::new(
        mywaker::opencode::Delivery::Queue,
        "msg_mywaker:delegation:writer:1:0:abc123",
    );
    client
        .prompt_with("ses_test", "queued work", &options)
        .await
        .unwrap();

    let received = state.received.lock().unwrap().clone();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].text, "queued work");
    assert_eq!(received[0].delivery.as_deref(), Some("queue"));
    assert_eq!(
        received[0].admission_id.as_deref(),
        Some("msg_mywaker:delegation:writer:1:0:abc123")
    );

    // The same id again is the same admission: the client sends it unchanged, which
    // is what lets the service deduplicate a retry.
    client
        .prompt_with("ses_test", "queued work", &options)
        .await
        .unwrap();
    let received = state.received.lock().unwrap().clone();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].admission_id, received[1].admission_id);
}

/// A harness-built admission id must survive the service's own validation. This
/// drives the real `harness::prompt_options` into the fake, whose `prompt` handler
/// enforces OpenCode's `Session.Message.ID` (`^msg_`) contract — so reverting the
/// prefix (the bug that made every prompt a 400) fails here.
#[tokio::test]
async fn a_harness_admission_id_is_accepted_by_the_service_contract() {
    let (client, state) = start_server().await;
    client.create_session("/work", None, None).await.unwrap();

    for kind in [
        mywaker::harness::PromptKind::UserPrompt,
        mywaker::harness::PromptKind::Delegation,
        mywaker::harness::PromptKind::Report,
        mywaker::harness::PromptKind::FanoutBrief,
        mywaker::harness::PromptKind::Retry,
        mywaker::harness::PromptKind::AutonomousTask,
    ] {
        let options = mywaker::harness::prompt_options(kind, "writer", 1, 0, "abc123");
        client
            .prompt_with("ses_test", "hand-off", &options)
            .await
            .unwrap_or_else(|err| panic!("{kind:?} was rejected by the service contract: {err:#}"));
    }

    assert_eq!(state.received.lock().unwrap().len(), 6);
}

#[tokio::test]
async fn previous_sessions_list_sorted_newest_first() {
    let (client, _state) = start_server().await;

    let sessions = client.list_sessions("/work").await.unwrap();
    // Every session the fake serves, regardless of agent.
    assert_eq!(sessions.len(), 3);
    // Most recently updated first — the order the resume picker shows.
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        ["ses_old_orchestrator", "ses_old_writer", "ses_older_writer"]
    );

    // The fields the app filters and displays on.
    let writer = sessions.iter().find(|s| s.id == "ses_old_writer").unwrap();
    assert_eq!(writer.agent.as_deref(), Some("writer"));
    assert_eq!(writer.title.as_deref(), Some("draft the release notes"));
    assert_eq!(writer.time.as_ref().and_then(|t| t.updated), Some(300));
}

#[tokio::test]
async fn mcp_status_reports_connection_counts() {
    let (client, _state) = start_server().await;

    let servers = client.mcp_status().await.unwrap();
    assert_eq!(servers.len(), 3);

    // 2 of 3 connected: the summary the sidebar's `mcp n/n` row renders.
    let up = servers.iter().filter(|s| s.is_connected()).count();
    assert_eq!(up, 2);

    // The disconnected one is identifiable by name, for the log line.
    let down = servers.iter().find(|s| !s.is_connected()).unwrap();
    assert_eq!(down.name, "browser");
}
