//! Kenari integration: a *provider adapter* the router uses as one of several
//! model sources, plus an optional Jev (System One) classifier.
//!
//! The router itself ([`super::router::Router`]) is provider-agnostic and
//! classifies tasks with local heuristics ([`super::router::TaskClass::infer`]).
//! This module only (1) fetches Kenari's public model catalog and (2) offers
//! Jev — Kenari's typed decision model — as an *opt-in* refinement a caller may
//! apply out-of-band. Nothing here is on the router's default decision path.

use color_eyre::eyre::{Result, WrapErr};
use serde_json::Value;

use super::models::{ModelInfo, ModelList};
use super::router::{ModelChoice, Role, Router, TaskComplexity};
use crate::config::JevConfig;

/// Kenari model catalog endpoint (public; no API key).
const DEFAULT_MODELS_URL: &str = "https://kenari.id/v1/models";

/// Minimal client for Kenari's public model catalog.
#[derive(Debug, Clone)]
pub struct KenariClient {
    http: reqwest::Client,
    models_url: String,
}

impl KenariClient {
    pub fn new() -> Self {
        Self::with_url(DEFAULT_MODELS_URL)
    }

    pub fn with_url(models_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            models_url: models_url.into(),
        }
    }

    /// Fetch the live model catalog (public; no API key).
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let response = self
            .http
            .get(&self.models_url)
            .header("accept", "application/json")
            .send()
            .await
            .wrap_err("GET kenari models")?
            .error_for_status()
            .wrap_err("GET kenari models")?;
        let list: ModelList = response.json().await.wrap_err("decode kenari models")?;
        Ok(list.data)
    }

    /// Fetch models and pick the top candidate for `role`.
    pub async fn best_for(&self, role: Role) -> Result<Option<ModelChoice>> {
        let models = self.list_models().await?;
        Ok(Router::route(role, &models).into_iter().next())
    }
}

impl Default for KenariClient {
    fn default() -> Self {
        Self::new()
    }
}

// --- Jev (System One) — an optional classifier, NOT on the router's path ---

/// Default System One endpoint (Kenari's public one).
pub const DEFAULT_SYSTEMONE_URL: &str = "https://kenari.id/v1/systemone";
/// Default (free) decision model; swap for any id from `GET /v1/models?modality=systemone`.
pub const DEFAULT_JEV_MODEL: &str = "jev-1-13-free";
/// Minimum confidence to trust Jev for a given dimension.
const MIN_CONFIDENCE: f64 = 0.55;

/// A Jev classification, per-dimension `Option` (a dimension is `None` when the
/// answer was missing or too uncertain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JevTask {
    pub role: Option<Role>,
    pub complexity: Option<TaskComplexity>,
}

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
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            api_key: api_key.into(),
            endpoint: endpoint.into(),
            model: model.into(),
        }
    }

    /// Build a client from the `KENARI_API_KEY` env var, using the default
    /// endpoint and model; `None` when the key isn't set, so callers fall back
    /// to the provider-agnostic heuristics.
    pub fn from_env() -> Option<JevClient> {
        let api_key = std::env::var("KENARI_API_KEY").ok()?.trim().to_owned();
        if api_key.is_empty() {
            return None;
        }
        Some(Self::new(api_key, DEFAULT_SYSTEMONE_URL, DEFAULT_JEV_MODEL))
    }

    /// Build a client from [`JevConfig`]. The API key is taken from the config,
    /// falling back to `KENARI_API_KEY`; `None` when no key is available (Jev
    /// disabled). Endpoint and model default to the public Kenari values.
    pub fn from_config(cfg: &JevConfig) -> Option<JevClient> {
        let (api_key, endpoint, model) = Self::settings(cfg)?;
        Some(Self::new(api_key, endpoint, model))
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

    /// Classify a task's role and complexity in one System One round-trip.
    ///
    /// Returns `Ok(None)` when Jev has no confident answer for either
    /// dimension; `Err` on transport/HTTP errors. Callers overlay the result on
    /// their own heuristic fallback.
    pub async fn classify_task(&self, goal: &str) -> Result<Option<JevTask>> {
        let body = serde_json::json!({
            "model": self.model,
            "state": goal,
            "questions": {
                "role": {
                    "type": "choice",
                    "instructions": "Which job role is best suited to this task?",
                    "criteria": {
                        "planner": "breaks a goal into concrete steps, roadmap, or design",
                        "researcher": "investigates and gathers facts, data, or references",
                        "writer": "produces prose, copy, docs, or a report",
                        "reviewer": "checks, critiques, audits, or verifies existing work",
                        "general": "does not fit a specialized role"
                    }
                },
                "complexity": {
                    "type": "score",
                    "instructions": "How demanding is this task?",
                    "criteria": ["trivial", "simple", "moderate", "hard", "very hard"]
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
        Ok(Self::parse_task(&json))
    }

    /// Parse and confidence-gate the System One answer.
    fn parse_task(json: &Value) -> Option<JevTask> {
        let answers = json.get("answers")?;

        let role = answers.get("role").and_then(|answer| {
            let key = answer.get("choice")?.as_str()?;
            let confidence = answer
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            (confidence >= MIN_CONFIDENCE).then(|| Role::from_label(key))
        });

        let complexity = answers.get("complexity").and_then(|answer| {
            let score = answer.get("score")?.as_f64()?;
            let confidence = answer
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            (confidence >= MIN_CONFIDENCE).then(|| complexity_from_score(score))
        });

        if role.is_none() && complexity.is_none() {
            return None;
        }
        Some(JevTask { role, complexity })
    }
}

/// Map Jev's 0–4 score to the router's [`TaskComplexity`] band. The five
/// criteria (`trivial`…`very hard`, indices 0–4) collapse onto three bands;
/// midpoints 1.5 and 2.5 are the boundaries between them.
fn complexity_from_score(score: f64) -> TaskComplexity {
    if score < 1.5 {
        TaskComplexity::Low
    } else if score < 2.5 {
        TaskComplexity::Medium
    } else {
        TaskComplexity::High
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(role_key: &str, role_conf: f64, score: f64, complexity_conf: f64) -> Option<JevTask> {
        let json = serde_json::json!({
            "answers": {
                "role": { "type": "choice", "choice": role_key, "confidence": role_conf },
                "complexity": { "type": "score", "score": score, "confidence": complexity_conf }
            }
        });
        JevClient::parse_task(&json)
    }

    #[test]
    fn trusts_confident_dimensions() {
        assert_eq!(
            parse("researcher", 0.84, 2.0, 0.61),
            Some(JevTask {
                role: Some(Role::Researcher),
                complexity: Some(TaskComplexity::Medium),
            })
        );
    }

    #[test]
    fn drops_uncertain_dimensions() {
        assert_eq!(parse("researcher", 0.30, 2.0, 0.40), None);
    }

    #[test]
    fn keeps_partial_answer_when_one_dimension_is_confident() {
        let task = parse("planner", 0.91, 3.2, 0.12).expect("role is confident");
        assert_eq!(task.role, Some(Role::Planner));
        assert_eq!(task.complexity, None);
    }

    #[test]
    fn drops_everything_and_returns_none_when_unconfident() {
        let json = serde_json::json!({
            "answers": {
                "role": { "type": "choice", "choice": "writer", "confidence": 0.2 },
                "complexity": { "type": "score", "score": 3.0, "confidence": 0.1 }
            }
        });
        assert!(JevClient::parse_task(&json).is_none());
    }

    #[test]
    fn maps_scores_to_complexity_bands() {
        assert_eq!(complexity_from_score(0.4), TaskComplexity::Low);
        assert_eq!(complexity_from_score(1.0), TaskComplexity::Low);
        assert_eq!(complexity_from_score(1.5), TaskComplexity::Medium);
        assert_eq!(complexity_from_score(2.0), TaskComplexity::Medium);
        assert_eq!(complexity_from_score(2.5), TaskComplexity::High);
        assert_eq!(complexity_from_score(3.2), TaskComplexity::High);
        assert_eq!(complexity_from_score(4.0), TaskComplexity::High);
    }

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
}
