//! Adapters that turn models from the user's *existing* providers (OpenCode's
//! `GET /api/model`) into the router's normalized [`ModelInfo`] shape, so
//! [`super::auto_route`] can rank across providers — not just Kenari's catalog.

use super::models::{Modalities, ModelInfo, Pricing};
use crate::opencode::models::ProviderModel;

/// Assumed FX rate for normalizing OpenCode provider costs (USD per 1M tokens)
/// to Kenari's IDR pricing. Approximate; not fed by a live quote.
pub const IDR_PER_USD: f64 = 16_000.0;

/// Convert USD per 1M tokens to micro-IDR per 1M tokens.
fn usd_to_micro_idr(usd: f64) -> u64 {
    (usd * IDR_PER_USD * 1_000_000.0).round() as u64
}

/// Convert an OpenCode provider model into a router candidate; `None` if it
/// should be skipped (disabled, or already covered by the direct Kenari fetch).
pub fn from_provider_model(model: &ProviderModel) -> Option<ModelInfo> {
    // Kenari-native models are pulled directly from Kenari's full catalog
    // (richer metadata + IDR pricing); skip their OpenCode mirror so we don't
    // double-count them under a different cost unit.
    if model.provider_id.as_deref() == Some("kenari") {
        return None;
    }
    // Disabled models can't run; drop them.
    if model.enabled == Some(false) {
        return None;
    }

    let input = model.input_cost();
    let output = model.output_cost();

    // Free = explicit zero cost, or a provider that declares no cost (local
    // MLX/opencode-inference). Unknown cost is treated as free to keep local
    // providers attractive for trivial tasks.
    let free = match (input, output) {
        (Some(i), Some(o)) => i == 0.0 && o == 0.0,
        _ => true,
    };

    let pricing = Some(Pricing {
        currency: Some("IDR".to_string()),
        free,
        input: if free {
            None
        } else {
            input.map(usd_to_micro_idr)
        },
        output: if free {
            None
        } else {
            output.map(usd_to_micro_idr)
        },
        cache_read: None,
        cache_write: None,
        unit: Some("micro_idr_per_1m_tokens".to_string()),
    });

    let modalities = model.capabilities.as_ref().map(|c| Modalities {
        input: c.input.clone(),
        output: c.output.clone(),
    });

    Some(ModelInfo {
        id: model.id.clone(),
        object: Some("model".to_string()),
        owned_by: Some(model.vendor()),
        name: Some(model.name.clone().unwrap_or_else(|| model.id.clone())),
        source: model
            .provider_id
            .clone()
            .unwrap_or_else(|| "opencode".to_string()),
        context_length: model.context_length(),
        max_output_tokens: model.limit.as_ref().and_then(|l| l.output),
        endpoints: vec!["chat".to_string()],
        modalities,
        pricing,
        reasoning: model.is_reasoning(),
        reasoning_toggle: false,
        reasoning_options: Vec::new(),
        tool_call: model.is_tool_capable(),
        sunset_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_usd_cost_to_micro_idr() {
        let model = ProviderModel {
            id: "meta/muse-spark-open".to_string(),
            model_id: Some("meta/muse-spark-open".to_string()),
            provider_id: Some("openrouter".to_string()),
            name: Some("Muse Spark Open".to_string()),
            capabilities: Some(crate::opencode::models::ModelCapabilities {
                tools: true,
                input: vec!["text".to_string(), "image".to_string()],
                output: vec!["text".to_string()],
            }),
            cost: Some(crate::opencode::models::Costs::List(vec![
                crate::opencode::models::ModelCost {
                    input: 1.25,
                    output: 4.25,
                },
            ])),
            limit: Some(crate::opencode::models::ModelLimit {
                context: Some(260_000),
                output: Some(28_000),
            }),
            variants: vec![crate::opencode::models::ModelVariant {
                id: "high".to_string(),
            }],
            enabled: Some(true),
            status: Some("active".to_string()),
        };

        let info = from_provider_model(&model).expect("convert");
        assert_eq!(info.id, "meta/muse-spark-open");
        assert_eq!(info.owned_by.as_deref(), Some("meta"));
        assert!(!info.is_free());
        // $1.25 × 16_000 → Rp 20_000 per 1M tokens.
        assert_eq!(info.input_idr(), Some(20_000));
        assert_eq!(info.output_idr(), Some(68_000));
        assert!(info.tool_call);
        assert!(info.reasoning);
        assert!(info.is_vision());
    }

    #[test]
    fn skips_kenari_mirror_and_disabled() {
        let mut kenari = ProviderModel {
            id: "claude-sonnet-5".to_string(),
            model_id: Some("claude-sonnet-5".to_string()),
            provider_id: Some("kenari".to_string()),
            name: None,
            capabilities: None,
            cost: None,
            limit: None,
            variants: Vec::new(),
            enabled: Some(true),
            status: Some("active".to_string()),
        };
        assert!(from_provider_model(&kenari).is_none());

        kenari.provider_id = Some("openrouter".to_string());
        kenari.enabled = Some(false);
        assert!(from_provider_model(&kenari).is_none());
    }

    #[test]
    fn treats_missing_cost_as_free() {
        let model = ProviderModel {
            id: "local/qwen-coder".to_string(),
            model_id: Some("local/qwen-coder".to_string()),
            provider_id: Some("mlx".to_string()),
            name: None,
            capabilities: Some(crate::opencode::models::ModelCapabilities {
                tools: true,
                input: vec!["text".to_string()],
                output: vec!["text".to_string()],
            }),
            cost: None,
            limit: None,
            variants: Vec::new(),
            enabled: Some(true),
            status: Some("active".to_string()),
        };
        let info = from_provider_model(&model).expect("convert");
        assert_eq!(info.owned_by.as_deref(), Some("local"));
        assert!(info.is_free());
        assert_eq!(info.effective_input_idr(), Some(0));
    }
}
