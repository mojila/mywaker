use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::{Interval, interval};

use crate::message::Message;

/// Merges crossterm input, the animation tick, and background messages into a
/// single [`Message`] stream for the app loop.
pub struct EventHandler {
    events: EventStream,
    tick: Interval,
    rx: mpsc::Receiver<Message>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration, rx: mpsc::Receiver<Message>) -> Self {
        // The interval is created once: a fresh interval's first tick completes
        // immediately, so rebuilding it per call would starve keyboard input.
        Self {
            events: EventStream::new(),
            tick: interval(tick_rate),
            rx,
        }
    }

    pub async fn next(&mut self) -> Result<Message> {
        select! {
            Some(Ok(event)) = self.events.next() => Ok(match event {
                Event::Key(key) => Message::Key(key),
                Event::Resize(width, height) => Message::Resize(width, height),
                _ => Message::Tick,
            }),
            _ = self.tick.tick() => Ok(Message::Tick),
            message = self.rx.recv() => Ok(match message {
                Some(message) => message,
                // Channel closed: fall back to tick. Kept alive by the
                // background task in practice, so this is a safety net.
                None => Message::Tick,
            }),
        }
    }
}
