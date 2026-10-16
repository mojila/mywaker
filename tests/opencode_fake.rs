//! Integration test: drive the OpenCode client against a scripted fake server.
//!
//! Verifies HTTP + SSE plumbing and serde parsing end-to-end without a real
//! service (and without triggering model inference).

use axum::body::Body;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use mywaker::opencode::OpenCode;
use serde_json::{Value, json};

fn app() -> Router {
    Router::new()
        .route("/api/info", get(info))
        .route("/api/session", post(create_session))
        .route("/api/session/ses_test/message", get(messages))
        .route("/api/event", get(events))
}

async fn info() -> Json<Value> {
    Json(json!({"version": "2.0.30", "pid": 1234, "urls": []}))
}

async fn create_session() -> Json<Value> {
    Json(json!({
        "data": {
            "id": "ses_test",
            "projectID": "global",
            "cost": 0,
            "time": {"created": 1, "updated": 2},
            "location": {"directory": "/x"}
        }
    }))
}

async fn messages() -> Json<Value> {
    Json(json!({
        "data": [
            {"id": "m1", "time": {"created": 1}, "text": "go", "type": "user"},
            {"id": "m2", "time": {"created": 2, "completed": 3}, "type": "assistant",
             "agent": "build", "content": [{"id": "p1", "type": "text", "text": "hello world"}],
             "finish": "done"}
        ]
    }))
}

async fn events() -> Response {
    // A single static SSE payload: the client's bytes_stream() reassembles
    // frames across chunk boundaries, so no true streaming is needed here.
    let frames = [
        json!({"id":"e1","type":"server.connected","data":{}}),
        json!({"id":"e2","type":"session.execution.started","data":{"sessionID":"ses_test"}}),
        json!({"id":"e3","type":"session.text.delta","data":{"sessionID":"ses_test","delta":"Hel"}}),
        json!({"id":"e4","type":"message.part.delta","data":{"sessionID":"ses_test","delta":"lo"}}),
        json!({"id":"e5","type":"session.execution.succeeded","data":{"sessionID":"ses_test"}}),
    ];
    let mut body = String::from(": heartbeat\n\n");
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }

    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(body))
        .expect("build SSE response")
}

#[tokio::test]
async fn client_streams_and_reads_messages_from_fake_server() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app()).await.unwrap();
    });

    let base = reqwest::Url::parse(&format!("http://{addr}")).unwrap();
    let client = OpenCode::with_base(base, "irrelevant");

    let info = client.server_info().await.unwrap();
    assert_eq!(info.version, "2.0.30");

    let session = client.create_session("/x").await.unwrap();
    assert_eq!(session.id, "ses_test");

    let events = client.events().unwrap();
    futures::pin_mut!(events);
    let mut types = Vec::new();
    let mut deltas = Vec::new();
    while let Some(event) = events.next().await {
        let event = event.unwrap();
        types.push(event.event_type.clone());
        if let Some(delta) = event.text_delta() {
            deltas.push(delta.to_string());
        }
    }

    assert_eq!(
        types,
        vec![
            "server.connected",
            "session.execution.started",
            "session.text.delta",
            "message.part.delta",
            "session.execution.succeeded",
        ]
    );
    assert_eq!(deltas, vec!["Hel", "lo"]);

    let messages = client.messages("ses_test").await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role(), "user");
    assert_eq!(messages[1].display_text(), "hello world");
}
