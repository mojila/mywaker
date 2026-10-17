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

/// Shared fake-server state: the prompts and requested session agents it has
/// received, in order.
#[derive(Clone, Default)]
struct FakeState {
    prompts: Arc<Mutex<Vec<String>>>,
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
) -> Json<Value> {
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    state.prompts.lock().unwrap().push(text);
    Json(json!({}))
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

fn router(state: FakeState) -> Router {
    Router::new()
        .route("/api/info", get(info))
        .route("/api/agent", get(agents))
        .route("/api/session", post(create_session))
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
