//! Jev (System One) — Kenari's typed decision model — as an opt-in answerer.
//!
//! Jev is *not* a chat model: it answers *typed* questions (`choice`, `score`,
//! `noul`) against a `state`. mywaker uses it in two places:
//!
//! 1. **OpenCode `form.created` auto-answer** — a session paused on a form has
//!    each choice-shaped field resolved with [`answer_field_or_default`], so the
//!    session always resumes.
//! 2. **`@jev <question>` delegation** — a teammate's yes/no question is framed
//!    as a binary `choice` and answered with [`answer_text_or_default`].
//!
//! Both surfaces go through the same three steps, defined once each: ask
//! (`JevClient::ask`), gate on confidence (`confident_choice`), and fall back
//! deterministically ([`default_answer_field`] / [`FALLBACK_VERDICT`]).

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
/// The verdict used when Jev is unavailable or uncertain on a free-form
/// question: proceed rather than stall the delegation chain.
pub const FALLBACK_VERDICT: &str = "yes";

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

    /// The decision model id this client asks (`jev-1-13-free` unless
    /// overridden) — shown on the pipeline's Jev node.
    pub fn model(&self) -> &str {
        &self.model
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
        let Some(criteria) = field_criteria(field) else {
            return Ok(None);
        };

        let json = self
            .ask(
                &field_state(field),
                "Choose the single best answer to the question.",
                criteria,
            )
            .await?;
        Ok(map_field_choice(&json, field))
    }

    /// Answer a free-form yes/no question as a binary `choice` ("yes" / "no").
    ///
    /// Jev is a *typed decider*, not a chat model, so a free question is framed
    /// as a confident yes/no verdict. `Ok(None)` means Jev was unavailable or
    /// uncertain — callers fall back to [`FALLBACK_VERDICT`] via
    /// [`answer_text_or_default`].
    pub async fn answer_text(&self, question: &str) -> Result<Option<String>> {
        let json = self
            .ask(
                question,
                "Answer the question with a confident yes or no verdict.",
                serde_json::json!({
                    "yes": "affirmative — the answer is yes / true / proceed / correct",
                    "no": "negative — the answer is no / false / stop / incorrect"
                }),
            )
            .await?;
        Ok(parse_yes_no(&json))
    }

    /// POST one `choice` question to System One and decode its answer envelope.
    ///
    /// The single transport path behind [`answer_field`](Self::answer_field) and
    /// [`answer_text`](Self::answer_text): both frame their question as a
    /// `choice` and differ only in the `state`, instructions, and criteria.
    async fn ask(&self, state: &str, instructions: &str, criteria: Value) -> Result<Value> {
        let body = serde_json::json!({
            "model": self.model,
            "state": state,
            "questions": {
                "answer": {
                    "type": "choice",
                    "instructions": instructions,
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

        response.json().await.wrap_err("decode systemone answer")
    }
}

/// A form field's answer, resolved end-to-end: Jev's confident value when a
/// client is attached, else the deterministic [`default_answer_field`]. Never
/// fails — this is the policy the `form.created` auto-answer relies on, since a
/// session paused on a form would otherwise hang forever.
pub async fn answer_field_or_default(
    jev: Option<&JevClient>,
    field: &FormField,
) -> serde_json::Value {
    if let Some(jev) = jev
        && let Ok(Some(value)) = jev.answer_field(field).await
    {
        return value;
    }
    default_answer_field(field)
}

/// A free-form question answered end-to-end: Jev's confident verdict when a
/// client is attached, else the deterministic [`FALLBACK_VERDICT`]. Never fails.
pub async fn answer_text_or_default(jev: Option<&JevClient>, question: &str) -> String {
    match jev {
        Some(jev) => match jev.answer_text(question).await {
            Ok(Some(answer)) => answer,
            _ => FALLBACK_VERDICT.to_owned(),
        },
        None => FALLBACK_VERDICT.to_owned(),
    }
}

/// The `state` sent to Jev for a field: its title, its description, or both.
fn field_state(field: &FormField) -> String {
    if field.title.is_empty() {
        field.description.clone()
    } else if field.description.is_empty() {
        field.title.clone()
    } else {
        format!("{}: {}", field.title, field.description)
    }
}

/// The `choice` string from a System One answer envelope, gated on
/// [`MIN_CONFIDENCE`]. `None` when the envelope is malformed or Jev was unsure —
/// the gate both answer paths share.
fn confident_choice(json: &Value) -> Option<&str> {
    let answer = json.get("answers")?.get("answer")?;
    let confidence = answer
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }
    answer.get("choice")?.as_str()
}

/// Parse a binary yes/no `choice` verdict from a System One answer envelope.
fn parse_yes_no(json: &Value) -> Option<String> {
    confident_choice(json).map(|choice| choice.to_ascii_lowercase())
}

/// The choice criteria used to ask Jev about a field: `None` when the field is
/// not a typed choice (free-text/number rather than booleans or options).
fn field_criteria(field: &FormField) -> Option<Value> {
    if field.field_type == "boolean" {
        return Some(serde_json::json!({
            "true": "affirmative — yes / true / proceed / confirm",
            "false": "negative — no / false / stop / decline"
        }));
    }
    if field.options.is_empty() {
        return None;
    }
    let mut criteria = serde_json::Map::new();
    for option in &field.options {
        let description = if option.description.is_empty() {
            option.label.clone()
        } else {
            option.description.clone()
        };
        criteria.insert(option.value.clone(), Value::String(description));
    }
    Some(Value::Object(criteria))
}

/// Map a System One `choice` answer envelope into the field's typed value,
/// confidence-gated. `None` when missing or below [`MIN_CONFIDENCE`].
fn map_field_choice(json: &Value, field: &FormField) -> Option<serde_json::Value> {
    Some(typed_value(field, confident_choice(json)?))
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

    /// The confidence gate is shared: a missing `confidence` field counts as
    /// certain, and a choice with no `answers` envelope yields nothing.
    #[test]
    fn confident_choice_defaults_confidence_and_rejects_malformed() {
        let missing_confidence = serde_json::json!({
            "answers": { "answer": { "choice": "a" } }
        });
        assert_eq!(confident_choice(&missing_confidence), Some("a"));

        assert_eq!(confident_choice(&serde_json::json!({"answers": {}})), None);
        assert_eq!(
            confident_choice(&serde_json::json!({"answers": {"answer": {"confidence": 0.9}}})),
            None
        );
    }

    #[test]
    fn field_state_joins_title_and_description() {
        let field = |title: &str, description: &str| FormField {
            key: "k".into(),
            field_type: "string".into(),
            title: title.into(),
            description: description.into(),
            options: Vec::new(),
            custom: false,
            default: None,
        };

        assert_eq!(
            field_state(&field("Deploy?", "Ship it to prod")),
            "Deploy?: Ship it to prod"
        );
        // Either half alone is used verbatim; both empty stays empty.
        assert_eq!(field_state(&field("", "Ship it")), "Ship it");
        assert_eq!(field_state(&field("Deploy?", "")), "Deploy?");
        assert_eq!(field_state(&field("", "")), "");
    }

    #[test]
    fn field_criteria_is_shaped_by_field_type() {
        let option = |value: &str, label: &str, description: &str| FormOption {
            value: value.into(),
            label: label.into(),
            description: description.into(),
        };
        let with_options = |field_type: &str, options: Vec<FormOption>| FormField {
            key: "k".into(),
            field_type: field_type.into(),
            title: String::new(),
            description: String::new(),
            options,
            custom: false,
            default: None,
        };

        // A boolean becomes a typed true/false choice.
        let criteria = field_criteria(&with_options("boolean", Vec::new())).unwrap();
        assert_eq!(criteria.as_object().unwrap().len(), 2);
        assert!(criteria.get("true").is_some());
        assert!(criteria.get("false").is_some());

        // Options are keyed by value and described by description, falling back
        // to the label when there is none.
        let criteria = field_criteria(&with_options(
            "string",
            vec![
                option("a", "Alpha", "the first one"),
                option("b", "Beta", ""),
            ],
        ))
        .unwrap();
        assert_eq!(criteria["a"], serde_json::json!("the first one"));
        assert_eq!(criteria["b"], serde_json::json!("Beta"));

        // Free text and numbers are not typed choices.
        assert!(field_criteria(&with_options("string", Vec::new())).is_none());
        assert!(field_criteria(&with_options("number", Vec::new())).is_none());
    }

    /// Both surfaces resolve end-to-end without a client: the deterministic
    /// fallbacks apply rather than the caller having to branch.
    #[tokio::test]
    async fn or_default_helpers_fall_back_without_a_client() {
        let field = FormField {
            key: "k".into(),
            field_type: "string".into(),
            title: "Which?".into(),
            description: String::new(),
            options: vec![FormOption {
                value: "Alpha".into(),
                label: "Alpha".into(),
                description: String::new(),
            }],
            custom: false,
            default: None,
        };

        assert_eq!(
            answer_field_or_default(None, &field).await,
            serde_json::json!("Alpha")
        );
        assert_eq!(
            answer_text_or_default(None, "is this plan sound?").await,
            FALLBACK_VERDICT
        );
    }
}
