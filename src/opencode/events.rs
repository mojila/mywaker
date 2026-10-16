//! Live event subscription over OpenCode's `GET /api/event` SSE stream.

use async_stream::stream;
use color_eyre::eyre::Result;
use futures::{Stream, StreamExt};
use reqwest::Url;

use crate::opencode::models::Event;

/// Yield typed [`Event`]s parsed from the SSE endpoint.
///
/// OpenCode v2.0.30 emits `data: {json}\n\n` frames with `: heartbeat`
/// keep-alive comments. Malformed or unrecognized frames are dropped rather
/// than aborting the stream.
pub fn subscribe(
    client: reqwest::Client,
    url: Url,
    auth: String,
) -> impl Stream<Item = Result<Event>> {
    stream! {
        let response = match client
            .get(url)
            .header("authorization", &auth)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                yield Err(err.into());
                return;
            }
        };

        let mut body = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = body.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(err) => {
                    yield Err(err.into());
                    return;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline) = buffer.find('\n') {
                let line: String = buffer.drain(..=newline).collect();
                let line = line.trim_end_matches(['\r', '\n']);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<Event>(data) {
                    yield Ok(event);
                }
            }
        }
    }
}
