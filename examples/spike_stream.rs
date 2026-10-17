//! SPIKE: discover OpenCode's message-streaming event names and data shapes.
//!
//! Creates a session, subscribes to the SSE stream, sends a one-word prompt, and
//! prints every distinct event type plus the JSON body of message/part events.
//! Run with `cargo run --example spike_stream`.

use futures::StreamExt;
use mywaker::opencode::OpenCode;

fn truncate(json: String, limit: usize) -> String {
    if json.len() > limit {
        let mut cut = json.chars().take(limit).collect::<String>();
        cut.push_str("…(truncated)");
        cut
    } else {
        json
    }
}

#[tokio::main]
async fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;

    let client = OpenCode::connect().await?;
    let info = client.server_info().await?;
    println!("connected to OpenCode v{}", info.version);

    let dir = std::env::current_dir()?.to_string_lossy().to_string();
    let session = client.create_session(&dir, None, None).await?;
    println!("created session {}", session.id);

    let events = client.events()?;
    futures::pin_mut!(events);

    // Drain the handshake event so we start clean.
    if let Some(Ok(first)) = events.next().await {
        println!("first event: {}", first.event_type);
    }

    // Send a minimal prompt (queued; the session runs async).
    client
        .prompt(&session.id, "Reply with exactly: ready")
        .await?;
    println!("prompt sent; watching events…\n");

    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut interesting: Vec<(String, String)> = Vec::new();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_secs(20), events.next()).await {
            Ok(Some(Ok(event))) => {
                *counts.entry(event.event_type.clone()).or_insert(0) += 1;
                let is_text = event.event_type.contains("message")
                    || event.event_type.contains("part")
                    || event.event_type.contains("reply");
                if is_text {
                    let body = serde_json::to_string_pretty(&event.data).unwrap_or_default();
                    interesting.push((event.event_type.clone(), body));
                }
            }
            Ok(Some(Err(err))) => {
                println!("stream error: {err}");
                break;
            }
            Ok(None) => break,
            Err(_elapsed) => {
                // No events for a while; assume the turn finished.
                println!("(idle — stream quiet)");
                break;
            }
        }
    }

    println!("\n=== event type counts ===");
    for (kind, count) in &counts {
        println!("{count:>3}  {kind}");
    }

    println!("\n=== message/part/reply event bodies ===");
    for (kind, body) in &interesting {
        println!("\n--- {kind} ---\n{}\n", truncate(body.clone(), 800));
    }

    Ok(())
}
