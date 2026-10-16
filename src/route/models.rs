//! Serde models mirroring Kenari's `GET /v1/models` response.

use serde::Deserialize;

/// `GET /v1/models` envelope (`{"object":"list","data":[...]}`).
#[derive(Debug, Clone, Deserialize)]
pub struct ModelList {
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub data: Vec<ModelInfo>,
}

/// A single model entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub owned_by: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Routing provenance: which gateway/provider this model was sourced from
    /// (e.g. `"kenari"`, `"openrouter"`, `"clario"`, `"mlx"`). `"kenari"` for
    /// the native catalog. Drives the budget fallback — an exhausted gateway is
    /// skipped as a whole.
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub endpoints: Vec<String>,
    #[serde(default)]
    pub modalities: Option<Modalities>,
    #[serde(default)]
    pub pricing: Option<Pricing>,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub reasoning_toggle: bool,
    #[serde(default)]
    pub reasoning_options: Vec<String>,
    #[serde(default)]
    pub tool_call: bool,
    /// Unix timestamp of deprecation/sunset, if scheduled.
    #[serde(default)]
    pub sunset_at: Option<u64>,
}

impl ModelInfo {
    /// True if the model serves the OpenAI-style chat endpoint.
    pub fn is_chat(&self) -> bool {
        self.endpoints.iter().any(|e| e == "chat")
    }

    pub fn is_free(&self) -> bool {
        self.pricing.as_ref().is_some_and(|p| p.free)
    }

    /// Input price in IDR per 1M tokens (`None` if unknown, `Some(0)` if free).
    pub fn input_idr(&self) -> Option<u64> {
        self.pricing
            .as_ref()
            .and_then(|p| p.input)
            .map(micro_to_idr)
    }

    /// Output price in IDR per 1M tokens (`None` if unknown, `Some(0)` if free).
    pub fn output_idr(&self) -> Option<u64> {
        self.pricing
            .as_ref()
            .and_then(|p| p.output)
            .map(micro_to_idr)
    }

    /// Effective price for budget checks: free models cost nothing.
    pub fn effective_input_idr(&self) -> Option<u64> {
        if self.is_free() {
            Some(0)
        } else {
            self.input_idr()
        }
    }

    /// Effective output price for budget checks: free models cost nothing.
    pub fn effective_output_idr(&self) -> Option<u64> {
        if self.is_free() {
            Some(0)
        } else {
            self.output_idr()
        }
    }

    /// True if the model accepts image/pdf/video input (vision-capable).
    pub fn is_vision(&self) -> bool {
        self.modalities.as_ref().is_some_and(|m| {
            m.input
                .iter()
                .any(|k| k == "image" || k == "pdf" || k == "video")
        })
    }

    pub fn context_length(&self) -> u64 {
        self.context_length.unwrap_or(0)
    }
}

fn micro_to_idr(micro: u64) -> u64 {
    micro / 1_000_000
}

/// Default routing source: the native Kenari catalog.
fn default_source() -> String {
    "kenari".to_string()
}

/// Accepted/rejected input & output modalities.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Per-token pricing in micro-IDR per 1M tokens.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Pricing {
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub free: bool,
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
    #[serde(default)]
    pub cache_read: Option<u64>,
    #[serde(default)]
    pub cache_write: Option<u64>,
    #[serde(default)]
    pub unit: Option<String>,
}
