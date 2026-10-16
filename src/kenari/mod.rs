//! Cross-provider model routing: Kenari's public catalog plus the user's
//! *existing* providers via OpenCode's `GET /api/model`.
//!
//! Kenari's catalog is read live from `GET /v1/models` (public — no API key)
//! with per-token IDR pricing. OpenCode's model list covers every provider the
//! user already has installed (openrouter, clario, mlx, …). Both are normalized
//! into [`ModelInfo`] (see [`sources`]) and ranked together so [`auto_route`]
//! picks the best route across providers, not just Kenari.

pub mod models;
pub mod router;
pub mod sources;

use color_eyre::eyre::{Result, WrapErr};

pub use models::{ModelInfo, ModelList};
pub use router::{ModelChoice, Role, Router, TaskComplexity};

use crate::team::Employee;

/// Default public models endpoint.
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

/// The routed model assignment for one employee.
#[derive(Debug, Clone)]
pub struct EmployeeSelection {
    pub employee_id: String,
    pub name: String,
    pub role: Role,
    pub choice: Option<ModelChoice>,
}

/// Route every employee to the best model for their role, across Kenari's
/// catalog and the user's existing OpenCode providers.
pub async fn route_roster(employees: &[Employee]) -> Result<Vec<EmployeeSelection>> {
    let models = gather_candidates().await;
    Ok(employees
        .iter()
        .map(|employee| {
            let role = Role::from_employee_id(&employee.id);
            let choice = Router::route(role, &models).into_iter().next();
            EmployeeSelection {
                employee_id: employee.id.clone(),
                name: employee.name.clone(),
                role,
                choice,
            }
        })
        .collect())
}

/// Auto-find the best model across *existing* providers for a free-form task
/// description: gather Kenari's catalog plus OpenCode's model list, infer the
/// role and complexity, and return the top pick.
pub async fn auto_route(goal: &str) -> Result<Option<ModelChoice>> {
    let models = gather_candidates().await;
    let role = Role::from_goal(goal);
    Ok(Router::auto_route(role, goal, &models))
}

/// Gather every routing candidate: Kenari's live catalog merged with the
/// user's existing OpenCode providers. Best-effort — if either source is
/// unreachable its contribution is simply dropped.
async fn gather_candidates() -> Vec<ModelInfo> {
    let mut candidates = KenariClient::new().list_models().await.unwrap_or_default();

    if let Ok(client) = crate::opencode::OpenCode::connect().await
        && let Ok(models) = client.models().await
    {
        candidates.extend(models.iter().filter_map(sources::from_provider_model));
    }

    candidates
}
