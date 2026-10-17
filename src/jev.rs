//! Jev (System One) — Kenari's typed decision model — as an opt-in answerer.
//!
//! Jev is *not* a chat model: it answers *typed* questions (`choice`, `score`,
//! `noul`) against a `state`. mywaker uses it in two places:
//!
//! 1. **OpenCode `form.created` auto-answer** — a session paused on a form has
//!    each choice-shaped field resolved with [`JevClient::answer_field`],
//!    falling back to [`default_answer_field`] when Jev is unavailable or
//!    uncertain.
//! 2. **`@jev <question>` delegation** — a teammate's yes/no question is framed
//!    as a binary `choice` and answered with [`JevClient::answer_text`].

use color_eyre::eyre::{Result, WrapErr};
use serde_json::Value;

use crate::config::JevConfig;
use crate::opencode::FormField;

/// Default System One endpoint (Kenari's public one).
pub const DEFAULT_SYSTEMONE_URL: &str = "https://kenari.id/v1/systemone";
/// Default (free) decision model; swap for any id from `GET /v1/models?modality=systemone`.
pub const DEFAULT_JEV_MODEL: &str = "jev-1-13-free";
/// Minimum confidence to trust Jev for a given dimension.
const MIN_CONFIDENCE: f64 = 0.55;

/// Minimal client for a System One (Jev) endpoint.
#[derive(Debug, Clone)]
pub struct JevClient {
    http: reqwest::Client,
    api_key: String,
    endpoint: String,
    model: String,
}

impl JevClient {
    /// Build a client from explicit settings.
    pub fn new(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .wrap_err("failed to build reqwest client")?;
        Ok(Self {
            http,
            api_key: api_key.into(),
            endpoint: endpoint.into(),
            model: model.into(),
        })
    }

    /// Build a client from [`JevConfig`]. The API key is taken from the config,
    /// falling back to `KENARI_API_KEY`; `None` when no key is available (Jev
    /// disabled). Endpoint and model default to the public Kenari values.
    pub fn from_config(cfg: &JevConfig) -> Option<JevClient> {
        let (api_key, endpoint, model) = Self::settings(cfg)?;
        Self::new(api_key, endpoint, model).ok()
    }

    /// Resolve the (api key, endpoint, model) from config, with a
    /// `KENARI_API_KEY` env fallback for the key and Kenari's public defaults
    /// for endpoint/model. `None` when no key is available.
    fn settings(cfg: &JevConfig) -> Option<(String, String, String)> {
        let api_key = cfg
            .api_key
            .clone()
            .or_else(|| std::env::var("KENARI_API_KEY").ok())
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty())?;
        Some((
            api_key,
            cfg.endpoint
                .clone()
                .unwrap_or_else(|| DEFAULT_SYSTEMONE_URL.to_owned()),
            cfg.model
                .clone()
                .unwrap_or_else(|| DEFAULT_JEV_MODEL.to_owned()),
        ))
    }

    /// Answer one typed [`FormField`] with System One.
    ///
    /// Jev is a typed decider, so only *choice-shaped* fields are answerable:
    /// fields with enumerated `options`, plus booleans. The picked option's
    /// `value` (coerced to the field's JSON type) is returned. `Ok(None)` means
    /// Jev had no confident answer (caller falls back to
    /// [`default_answer_field`]); `Err` is a transport/HTTP error. Free-text or
    /// numeric fields (no options) yield `Ok(None)`.
    pub async fn answer_field(&self, field: &FormField) -> Result<Option<serde_json::Value>> {
        let criteria = field_criteria(field);
        let criteria = match criteria {
            Some(criteria) => criteria,
            None => return Ok(None),
        };

        let state = if field.title.is_empty() {
            field.description.clone()
        } else if field.description.is_empty() {
            field.title.clone()
        } else {
            format!("{}: {}", field.title, field.description)
        };

        let body = serde_json::json!({
            "model": self.model,
            "state": state,
            "questions": {
                "answer": {
                    "type": "choice",
                    "instructions": "Choose the single best answer to the question.",
                    "criteria": criteria,
                }
            }
        });

        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .wrap_err("POST systemone")?
            .error_for_status()
            .wrap_err("POST systemone")?;

        let json: Value = response.json().await.wrap_err("decode systemone answer")?;
        Ok(map_field_choice(&json, field))
    }

    /// Answer a free-form yes/no question as a binary `choice` ("yes" / "no").
    ///
    /// Jev is a *typed decider*, not a chat model, so a free question is framed
    /// as a confident yes/no verdict. `Ok(None)` means Jev was unavailable or
    /// uncertain — callers fall back to a deterministic affirmative.
    pub async fn answer_text(&self, question: &str) -> Result<Option<String>> {
        let body = serde_json::json!({
            "model": self.model,
            "state": question,
            "questions": {
                "answer": {
                    "type": "choice",
                    "instructions": "Answer the question with a confident yes or no verdict.",
                    "criteria": {
                        "yes": "affirmative — the answer is yes / true / proceed / correct",
                        "no": "negative — the answer is no / false / stop / incorrect"
                    }
                }
            }
        });

        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .wrap_err("POST systemone")?
            .error_for_status()
            .wrap_err("POST systemone")?;

        let json: Value = response.json().await.wrap_err("decode systemone answer")?;
        Ok(parse_yes_no(&json))
    }
}

/// Parse a binary yes/no `choice` verdict from a System One answer envelope.
fn parse_yes_no(json: &Value) -> Option<String> {
    let answer = json.get("answers")?.get("answer")?;
    let confidence = answer
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }
    answer
        .get("choice")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase())
}

/// The choice criteria used to ask Jev about a field: `None` when the field is
/// not a typed choice (free-text/number rather than booleans or options).
fn field_criteria(field: &FormField) -> Option<serde_json::Map<String, Value>> {
    if field.field_type == "boolean" {
        let mut m = serde_json::Map::new();
        m.insert(
            "true".to_string(),
            Value::String("affirmative — yes / true / proceed / confirm".to_string()),
        );
        m.insert(
            "false".to_string(),
            Value::String("negative — no / false / stop / decline".to_string()),
        );
        return Some(m);
    }
    if field.options.is_empty() {
        return None;
    }
    let mut m = serde_json::Map::new();
    for option in &field.options {
        let description = if option.description.is_empty() {
            option.label.clone()
        } else {
            option.description.clone()
        };
        m.insert(option.value.clone(), Value::String(description));
    }
    Some(m)
}

/// Map a System One `choice` answer envelope into the field's typed value,
/// confidence-gated. `None` when missing or below [`MIN_CONFIDENCE`].
fn map_field_choice(json: &Value, field: &FormField) -> Option<serde_json::Value> {
    let answer = json.get("answers")?.get("answer")?;
    let confidence = answer
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }
    let choice = answer.get("choice")?.as_str()?;
    Some(typed_value(field, choice))
}

/// Shape Jev's `choice` string into the field's expected JSON value type.
fn typed_value(field: &FormField, choice: &str) -> serde_json::Value {
    match field.field_type.as_str() {
        "boolean" => serde_json::Value::Bool(choice.eq_ignore_ascii_case("true")),
        "multiselect" => {
            serde_json::Value::Array(vec![serde_json::Value::String(choice.to_owned())])
        }
        _ => serde_json::Value::String(choice.to_owned()),
    }
}

/// A deterministic value for a field when Jev is unavailable or uncertain: the
/// field's own default, else an affirmative (boolean) / first-option (option
/// field) answer, else an empty string.
pub fn default_answer_field(field: &FormField) -> serde_json::Value {
    if let Some(default) = &field.default {
        return default.clone();
    }
    match field.field_type.as_str() {
        "boolean" => serde_json::Value::Bool(true),
        "multiselect" => serde_json::Value::Array(
            field
                .options
                .first()
                .map(|o| serde_json::Value::String(o.value.clone()))
                .into_iter()
                .collect(),
        ),
        _ => field
            .options
            .first()
            .map(|o| serde_json::Value::String(o.value.clone()))
            .unwrap_or_else(|| serde_json::Value::String(String::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencode::FormOption;

    #[test]
    fn settings_default_endpoint_and_model() {
        let cfg = JevConfig {
            endpoint: None,
            model: None,
            api_key: Some("kn-test".to_string()),
        };
        let (key, endpoint, model) = JevClient::settings(&cfg).expect("key present");
        assert_eq!(key, "kn-test");
        assert_eq!(endpoint, DEFAULT_SYSTEMONE_URL);
        assert_eq!(model, DEFAULT_JEV_MODEL);
    }

    #[test]
    fn settings_override_endpoint_and_model() {
        let cfg = JevConfig {
            endpoint: Some("https://example.test/v1/systemone".to_string()),
            model: Some("my-decision-model".to_string()),
            api_key: Some("sk-test".to_string()),
        };
        let (_, endpoint, model) = JevClient::settings(&cfg).expect("key present");
        assert_eq!(endpoint, "https://example.test/v1/systemone");
        assert_eq!(model, "my-decision-model");
    }

    #[test]
    fn settings_blank_key_disables_jev() {
        let cfg = JevConfig {
            endpoint: None,
            model: None,
            api_key: Some("   ".to_string()),
        };
        assert!(JevClient::settings(&cfg).is_none());
    }

    #[test]
    fn default_answer_field_respects_type_and_options() {
        let string = |options: &[&str]| FormField {
            key: "k".into(),
            field_type: "string".into(),
            title: "t".into(),
            description: String::new(),
            options: options
                .iter()
                .map(|o| FormOption {
                    value: (*o).into(),
                    label: (*o).into(),
                    description: String::new(),
                })
                .collect(),
            custom: false,
            default: None,
        };

        // First option wins for a plain string field.
        assert_eq!(
            default_answer_field(&string(&["Cancel", "Proceed"])),
            serde_json::json!("Cancel")
        );
        assert_eq!(
            default_answer_field(&string(&["Alpha", "Beta"])),
            serde_json::json!("Alpha")
        );
        // Free text with no options → empty string.
        assert_eq!(default_answer_field(&string(&[])), serde_json::json!(""));

        // Boolean defaults to the affirmative.
        let mut boolean = string(&[]);
        boolean.field_type = "boolean".into();
        assert_eq!(default_answer_field(&boolean), serde_json::json!(true));

        // Multiselect wraps the first value in an array.
        let mut multi = string(&["a", "b"]);
        multi.field_type = "multiselect".into();
        assert_eq!(default_answer_field(&multi), serde_json::json!(["a"]));

        // An explicit default is preferred over everything else.
        let mut with_default = string(&["x"]);
        with_default.default = Some(serde_json::json!("chosen"));
        assert_eq!(
            default_answer_field(&with_default),
            serde_json::json!("chosen")
        );
    }

    #[test]
    fn map_field_choice_coerces_to_field_type() {
        let field = |t: &str| FormField {
            key: "k".into(),
            field_type: t.into(),
            title: String::new(),
            description: String::new(),
            options: Vec::new(),
            custom: false,
            default: None,
        };

        let confident_bool = serde_json::json!({
            "answers": { "answer": { "type": "choice", "choice": "true", "confidence": 0.9 } }
        });
        assert_eq!(
            map_field_choice(&confident_bool, &field("boolean")),
            Some(serde_json::json!(true))
        );

        let choice = serde_json::json!({
            "answers": { "answer": { "choice": "a", "confidence": 0.9 } }
        });
        assert_eq!(
            map_field_choice(&choice, &field("string")),
            Some(serde_json::json!("a"))
        );
        assert_eq!(
            map_field_choice(&choice, &field("multiselect")),
            Some(serde_json::json!(["a"]))
        );

        // Below the confidence floor the field is left unanswered.
        let unsure = serde_json::json!({
            "answers": { "answer": { "choice": "a", "confidence": 0.1 } }
        });
        assert_eq!(map_field_choice(&unsure, &field("string")), None);
    }

    #[test]
    fn parse_yes_no_reads_binary_choice_and_gates_confidence() {
        let confident = serde_json::json!({
            "answers": { "answer": { "type": "choice", "choice": "yes", "confidence": 0.9 } }
        });
        assert_eq!(parse_yes_no(&confident).as_deref(), Some("yes"));

        // Confidence below the threshold yields no answer (caller falls back).
        let unsure = serde_json::json!({
            "answers": { "answer": { "type": "choice", "choice": "no", "confidence": 0.1 } }
        });
        assert_eq!(parse_yes_no(&unsure), None);

        // Missing answer envelope is None.
        assert_eq!(parse_yes_no(&serde_json::json!({})), None);
    }
}
