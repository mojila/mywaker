//! Usage & resource telemetry: LLM token usage aggregation and process metrics.
//!
//! Token totals are accumulated from OpenCode assistant messages (deduped by
//! message id so transcript re-loads don't double-count). Process RAM/CPU are
//! read lazily from `sysinfo` and refreshed at most once a second from the tick
//! loop, with CPU averaged via a simple exponential moving average.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::opencode::SessionMessage;

/// Running token totals across the app's sessions, plus a cache-hit ratio.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Message ids already tallied, so transcript re-loads don't double-count.
    seen: HashSet<String>,
}

impl Usage {
    /// Tally one message's token usage (idempotent per message id).
    pub fn record(&mut self, message: &SessionMessage) {
        if !self.seen.insert(message.id.clone()) {
            return;
        }
        if let Some(tokens) = &message.tokens {
            self.input_tokens += tokens.input;
            self.output_tokens += tokens.output;
            self.reasoning_tokens += tokens.reasoning;
            self.cache_read_tokens += tokens.cache.read;
            self.cache_write_tokens += tokens.cache.write;
        }
    }

    /// Fraction of input tokens served from prompt cache, `0.0` when there is no
    /// input or no cache reads; clamped to `1.0`.
    pub fn cache_hit_rate(&self) -> f64 {
        if self.input_tokens == 0 {
            return 0.0;
        }
        (self.cache_read_tokens as f64 / self.input_tokens as f64).min(1.0)
    }
}

/// Format a byte count as a compact human string (B / KiB / MiB / GiB).
pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Format a token/event count compactly (e.g. `874`, `12.4k`, `3.2M`).
pub fn fmt_count(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

/// A simple exponential moving average over sample values.
#[derive(Debug, Clone)]
pub struct Ema {
    alpha: f32,
    value: Option<f32>,
}

impl Ema {
    pub fn new(alpha: f32) -> Self {
        Self { alpha, value: None }
    }

    /// Push a sample and return the updated average.
    pub fn push(&mut self, sample: f32) -> f32 {
        match self.value {
            None => self.value = Some(sample),
            Some(previous) => self.value = Some(previous + self.alpha * (sample - previous)),
        }
        self.value.unwrap_or(0.0)
    }

    pub fn value(&self) -> f32 {
        self.value.unwrap_or(0.0)
    }
}

/// Process resource usage (RSS + CPU%), refreshed via `sysinfo` at most every
/// [`ProcessMetrics::REFRESH_INTERVAL`].
pub struct ProcessMetrics {
    system: sysinfo::System,
    cpu: Ema,
    last_refresh: Instant,
    pid: sysinfo::Pid,
}

impl ProcessMetrics {
    /// Refresh process stats at most this often (the tick loop calls `refresh`
    /// every frame, but only one in `N` actually re-reads `/proc`/`mach`).
    pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

    pub fn new() -> Self {
        let pid = sysinfo::get_current_pid().unwrap_or_else(|_| sysinfo::Pid::from_u32(0));
        let mut system = sysinfo::System::new();
        // Establish a baseline so the first `cpu_usage()` has a prior snapshot.
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        Self {
            system,
            cpu: Ema::new(0.3),
            last_refresh: Instant::now(),
            pid,
        }
    }

    /// Re-read process stats if the refresh interval has elapsed.
    pub fn refresh(&mut self) {
        if self.last_refresh.elapsed() < Self::REFRESH_INTERVAL {
            return;
        }
        self.system
            .refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.pid]), true);
        self.last_refresh = Instant::now();
        if let Some(process) = self.system.process(self.pid) {
            self.cpu.push(process.cpu_usage());
        }
    }

    /// The process's resident memory in bytes, if available.
    pub fn memory_bytes(&self) -> Option<u64> {
        self.system
            .process(self.pid)
            .map(|process| process.memory())
    }

    /// The exponentially-averaged process CPU percentage.
    pub fn cpu_percent(&self) -> f32 {
        self.cpu.value()
    }
}

impl Default for ProcessMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assistant_with_tokens(id: &str, input: u64, read: u64) -> SessionMessage {
        serde_json::from_value(json!({
            "id": id,
            "time": {"created": 1},
            "type": "assistant",
            "tokens": {"input": input, "output": 50, "reasoning": 20,
                       "cache": {"read": read, "write": 10}},
            "content": [{"type": "text", "text": "hi"}],
            "finish": "done"
        }))
        .unwrap()
    }

    #[test]
    fn usage_records_tokens_and_dedupes() {
        let mut usage = Usage::default();
        let msg = assistant_with_tokens("m1", 100, 80);
        usage.record(&msg);
        usage.record(&msg); // duplicate id → ignored

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.reasoning_tokens, 20);
        assert_eq!(usage.cache_read_tokens, 80);
        assert_eq!(usage.cache_write_tokens, 10);
        assert!((usage.cache_hit_rate() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn cache_hit_rate_clamps_and_handles_zero_input() {
        let mut usage = Usage::default();
        assert_eq!(usage.cache_hit_rate(), 0.0);

        // read > input is clamped to 100%.
        usage.record(&assistant_with_tokens("a", 50, 80));
        assert!((usage.cache_hit_rate() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fmt_bytes_and_counts() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KiB");
        assert_eq!(fmt_bytes(42 * 1024 * 1024), "42.0 MiB");

        assert_eq!(fmt_count(874), "874");
        assert_eq!(fmt_count(12_400), "12.4k");
        assert_eq!(fmt_count(3_200_000), "3.2M");
    }

    #[test]
    fn ema_converges_toward_samples() {
        let mut ema = Ema::new(0.5);
        assert_eq!(ema.push(10.0), 10.0);
        assert_eq!(ema.push(20.0), 15.0);
        assert_eq!(ema.push(20.0), 17.5);
        assert_eq!(ema.value(), 17.5);
    }
}
