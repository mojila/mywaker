//! Cross-provider model routing.
//!
//! The router is **provider-agnostic**: every model — wherever it comes from —
//! is normalized into [`ModelInfo`] and ranked by a weighted capability/cost
//! score ([`Router`]); each task is classified locally ([`TaskClass::infer`],
//! no network or provider dependency). Model *sources* are provider adapters:
//! [`kenari`] (Kenari's catalog) and OpenCode's installed providers
//! ([`sources::from_provider_model`]); [`gather_candidates`] merges them.

pub mod kenari;
pub mod models;
pub mod router;
pub mod sources;

use std::collections::HashSet;

use color_eyre::eyre::Result;

pub use kenari::{JevClient, JevTask, KenariClient};
pub use models::{ModelInfo, ModelList};
pub use router::{ModelChoice, Role, RoutePlan, Router, TaskClass, TaskComplexity};

use crate::team::Employee;

/// The routed model assignment for one employee.
#[derive(Debug, Clone)]
pub struct EmployeeSelection {
    pub employee_id: String,
    pub name: String,
    pub role: Role,
    pub choice: Option<ModelChoice>,
}

/// Route every employee to the best model for their role, across all providers.
pub async fn route_roster(employees: &[Employee]) -> Result<Vec<EmployeeSelection>> {
    route_roster_excluding(employees, &HashSet::new()).await
}

/// Like [`route_roster`], but skips models whose `source` (gateway) is in
/// `exhausted` — used to fall back across providers after a budget limit.
pub async fn route_roster_excluding(
    employees: &[Employee],
    exhausted: &HashSet<String>,
) -> Result<Vec<EmployeeSelection>> {
    let models = gather_candidates().await;
    Ok(employees
        .iter()
        .map(|employee| {
            let role = Role::from_employee_id(&employee.id);
            let choice = Router::route(role, &models)
                .into_iter()
                .find(|choice| !exhausted.contains(&choice.source));
            EmployeeSelection {
                employee_id: employee.id.clone(),
                name: employee.name.clone(),
                role,
                choice,
            }
        })
        .collect())
}

/// Auto-find the best model across providers for a free-form task description:
/// gather candidates, classify the task, and return the top pick.
pub async fn auto_route(goal: &str) -> Result<Option<ModelChoice>> {
    Ok(auto_route_plan(goal).await?.best().cloned())
}

/// Build a full [`RoutePlan`] for a task so callers can walk the cross-provider
/// fallback chain when a source hits its budget limit. The task is classified
/// locally ([`TaskClass::infer`]) — provider-agnostic, no network call.
pub async fn auto_route_plan(goal: &str) -> Result<RoutePlan> {
    auto_route_plan_with(TaskClass::infer(goal)).await
}

/// Build a [`RoutePlan`] from an already-classified task, so a caller can
/// classify once (and show *how* it was classified) then reuse the result.
pub async fn auto_route_plan_with(class: TaskClass) -> Result<RoutePlan> {
    let models = gather_candidates().await;
    Ok(Router::plan_with(class.role, class.complexity, &models))
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
