//! Model routing: pick the most cost-effective model for a task.
//!
//! The catalog is read live (never hardcoded); the router filters chat-capable
//! tool-using models and ranks them with a weighted score that balances a
//! *capability* signal (provider tier + reasoning + context + vision) against
//! cost. [`TaskComplexity`] drives the budget band (simple → cheap, hard →
//! top-tier) while [`Role`] sets trait preferences; [`Router::auto_route`]
//! infers both from a free-form task description. The router is
//! provider-agnostic: it scores any normalized [`ModelInfo`] regardless of
//! which gateway (`source`) the model came from.

use std::collections::HashSet;

use super::models::ModelInfo;

/// A task role an employee can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Planner,
    Researcher,
    Writer,
    Reviewer,
    General,
}

impl Role {
    pub fn from_employee_id(id: &str) -> Role {
        Self::from_label(id)
    }

    /// Parse a role from its label (`"planner"`, `"researcher"`, `"writer"`,
    /// `"reviewer"`). Unknown labels map to [`Role::General`].
    pub fn from_label(label: &str) -> Role {
        match label {
            "planner" => Role::Planner,
            "researcher" => Role::Researcher,
            "writer" => Role::Writer,
            "reviewer" => Role::Reviewer,
            _ => Role::General,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Planner => "planner",
            Role::Researcher => "researcher",
            Role::Writer => "writer",
            Role::Reviewer => "reviewer",
            Role::General => "general",
        }
    }

    /// Position in the canonical plan → research → write → review workflow,
    /// used to order the team as a pipeline in the UI.
    pub fn order(self) -> u8 {
        match self {
            Role::Planner => 0,
            Role::Researcher => 1,
            Role::Writer => 2,
            Role::Reviewer => 3,
            Role::General => 4,
        }
    }

    /// Infer the role best suited to a free-form task description (cue words).
    pub fn from_goal(goal: &str) -> Role {
        let text = goal.to_ascii_lowercase();

        const RESEARCH: &[&str] = &["research", "investigate", "survey", "compare", "benchmark"];
        const PLAN: &[&str] = &[
            "plan",
            "roadmap",
            "design",
            "strategy",
            "schedule",
            "architect",
            "break down",
        ];
        const REVIEW: &[&str] = &["review", "audit", "critique", "proofread", "check"];
        const WRITE: &[&str] = &[
            "write", "draft", "blog", "article", "document", "email", "content", "copy", "author",
        ];

        if RESEARCH.iter().any(|k| text.contains(*k)) {
            Role::Researcher
        } else if PLAN.iter().any(|k| text.contains(*k)) {
            Role::Planner
        } else if REVIEW.iter().any(|k| text.contains(*k)) {
            Role::Reviewer
        } else if WRITE.iter().any(|k| text.contains(*k)) {
            Role::Writer
        } else {
            Role::General
        }
    }
}

/// How demanding a task is; drives the budget band and thus the model tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    /// Trivial/quick — the cheapest models that can still do the job.
    Low,
    /// Balanced — the role's default budget.
    Medium,
    /// Hard — reasoning required, top-tier budget.
    High,
}

impl TaskComplexity {
    pub fn label(self) -> &'static str {
        match self {
            TaskComplexity::Low => "low",
            TaskComplexity::Medium => "medium",
            TaskComplexity::High => "high",
        }
    }

    /// Infer complexity from a description using cue words, falling back to
    /// length. A documented heuristic, tuned by the unit tests.
    pub fn infer(goal: &str) -> TaskComplexity {
        let text = goal.to_ascii_lowercase();
        let words = text.split_whitespace().count();

        const HIGH: &[&str] = &[
            "architect",
            "design",
            "refactor",
            "research",
            "investigate",
            "analy",
            "compare",
            "benchmark",
            "audit",
            "strategy",
            "roadmap",
            "plan",
            "spec",
            "deep",
            "complex",
            "thorough",
            "comprehensive",
            "migrate",
            "debug",
            "optimize",
            "evaluate",
            "synthesize",
            "review",
            "security",
        ];
        const LOW: &[&str] = &[
            "quick",
            "simple",
            "trivial",
            "one-line",
            "rename",
            "format",
            "lint",
            "typo",
            "bump",
            "tag",
            "echo",
            "list",
            "chore",
            "summarize",
            "spelling",
            "label",
        ];

        let high = HIGH.iter().filter(|k| text.contains(**k)).count();
        let low = LOW.iter().filter(|k| text.contains(**k)).count();

        if high > low {
            TaskComplexity::High
        } else if low > high {
            TaskComplexity::Low
        } else if words >= 40 {
            TaskComplexity::High
        } else if words <= 6 {
            TaskComplexity::Low
        } else {
            TaskComplexity::Medium
        }
    }
}

/// A classified task: the role that should do it and how demanding it is.
///
/// This is the router's provider-agnostic first layer — inferred locally from
/// cue words ([`Role::from_goal`] + [`TaskComplexity::infer`]) with no network
/// or provider dependency. An optional gateway classifier (e.g. Kenari's
/// [`JevClient`](super::kenari::JevClient)) can refine a [`TaskClass`] out of
/// band, but the router never requires one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskClass {
    pub role: Role,
    pub complexity: TaskComplexity,
}

impl TaskClass {
    /// Infer a task's role and complexity from its description.
    pub fn infer(goal: &str) -> Self {
        Self {
            role: Role::from_goal(goal),
            complexity: TaskComplexity::infer(goal),
        }
    }
}

/// Constraints and scoring weights for a role (tuned for a task complexity).
#[derive(Debug, Clone)]
pub struct RoleProfile {
    pub label: String,
    pub min_context: u64,
    pub require_reasoning: bool,
    /// Price caps in IDR per 1M tokens (`None` = uncapped).
    pub max_input_idr: Option<u64>,
    pub max_output_idr: Option<u64>,
    pub w_capability: f64,
    pub w_reasoning: f64,
    pub w_context: f64,
    pub w_vision: f64,
    pub w_cheap: f64,
}

impl RoleProfile {
    pub fn for_role(role: Role) -> RoleProfile {
        match role {
            Role::Planner => RoleProfile {
                label: "planner".to_string(),
                min_context: 128_000,
                require_reasoning: true,
                max_input_idr: None,
                max_output_idr: None,
                w_capability: 0.35,
                w_reasoning: 0.30,
                w_context: 0.25,
                w_vision: 0.05,
                w_cheap: 0.05,
            },
            Role::Researcher => RoleProfile {
                label: "researcher".to_string(),
                min_context: 128_000,
                require_reasoning: true,
                max_input_idr: None,
                max_output_idr: None,
                w_capability: 0.25,
                w_reasoning: 0.15,
                w_context: 0.25,
                w_vision: 0.25,
                w_cheap: 0.10,
            },
            Role::Writer => RoleProfile {
                label: "writer".to_string(),
                min_context: 32_000,
                require_reasoning: false,
                max_input_idr: Some(15_000),
                max_output_idr: Some(60_000),
                w_capability: 0.30,
                w_reasoning: 0.10,
                w_context: 0.15,
                w_vision: 0.10,
                w_cheap: 0.35,
            },
            Role::Reviewer => RoleProfile {
                label: "reviewer".to_string(),
                min_context: 32_000,
                require_reasoning: false,
                max_input_idr: Some(15_000),
                max_output_idr: Some(60_000),
                w_capability: 0.25,
                w_reasoning: 0.15,
                w_context: 0.15,
                w_vision: 0.05,
                w_cheap: 0.40,
            },
            Role::General => RoleProfile {
                label: "general".to_string(),
                min_context: 32_000,
                require_reasoning: false,
                max_input_idr: None,
                max_output_idr: None,
                w_capability: 0.25,
                w_reasoning: 0.15,
                w_context: 0.20,
                w_vision: 0.10,
                w_cheap: 0.30,
            },
        }
    }

    /// Role trait weights + budget tuned for a task's [`TaskComplexity`].
    pub fn for_task(role: Role, complexity: TaskComplexity) -> RoleProfile {
        let role_label = role.label();
        let mut profile = Self::for_role(role);
        match complexity {
            TaskComplexity::Low => {
                profile.max_input_idr = Some(5_000);
                profile.max_output_idr = Some(25_000);
                profile.require_reasoning = false;
                profile.min_context = 8_000;
                profile.w_cheap += 0.15;
                profile.w_capability = (profile.w_capability - 0.10).max(0.0);
            }
            TaskComplexity::Medium => {}
            TaskComplexity::High => {
                profile.max_input_idr = None;
                profile.max_output_idr = None;
                profile.require_reasoning = true;
                profile.min_context = profile.min_context.max(128_000);
                profile.w_capability += 0.15;
                profile.w_cheap = (profile.w_cheap - 0.15).max(0.0);
            }
        }
        profile.label = format!("{role_label}/{}", complexity.label());
        profile
    }
}

/// A routed result, best first.
#[derive(Debug, Clone)]
pub struct ModelChoice {
    pub id: String,
    /// Underlying model vendor (e.g. "anthropic", "openai").
    pub provider: String,
    /// Routing provenance: the gateway this model was sourced from
    /// (e.g. "kenari", "openrouter", "clario"). The budget fallback skips an
    /// exhausted `source` as a whole.
    pub source: String,
    pub context_length: u64,
    pub reasoning: bool,
    pub vision: bool,
    pub free: bool,
    pub input_idr: Option<u64>,
    pub output_idr: Option<u64>,
    pub score: f64,
    pub reason: String,
}

/// Normalization caps for scoring.
const CONTEXT_CAP: u64 = 1_000_000;
const PRICE_CAP: u64 = 200_000; // IDR/1M past which models are "top of market"

/// A ranked selection plus the budget-fallback chain.
///
/// [`Router::plan`] returns one. The caller runs [`RoutePlan::best`]; on a
/// budget/quota error it calls [`RoutePlan::exclude`] with the exhausted
/// `source` and picks the new best — which is guaranteed to come from a
/// *different* gateway (`source`), not just another model on the same one.
#[derive(Debug, Clone)]
pub struct RoutePlan {
    pub role: Role,
    pub complexity: TaskComplexity,
    /// All passing models, best first.
    pub choices: Vec<ModelChoice>,
    /// Sources (gateways) known to be out of budget; skipped by [`Self::best`].
    pub exhausted: HashSet<String>,
}

impl RoutePlan {
    /// The best selectable choice, skipping exhausted sources.
    pub fn best(&self) -> Option<&ModelChoice> {
        self.choices
            .iter()
            .find(|c| !self.exhausted.contains(&c.source))
    }

    /// The best choice that would be used if `source` is additionally excluded
    /// — i.e. the fallback for that source (also skips already-exhausted ones).
    pub fn fallback(&self, source: &str) -> Option<&ModelChoice> {
        self.choices
            .iter()
            .find(|c| c.source != source && !self.exhausted.contains(&c.source))
    }

    /// Mark a source as exhausted; returns true if it was newly added.
    pub fn exclude(&mut self, source: &str) -> bool {
        self.exhausted.insert(source.to_string())
    }

    /// Ordered list of currently selectable sources (for display).
    pub fn selectable_sources(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for choice in &self.choices {
            if !self.exhausted.contains(&choice.source) && !seen.contains(&choice.source) {
                seen.push(choice.source.clone());
            }
        }
        seen
    }
}

pub struct Router;

impl Router {
    /// Rank chat-capable models for `role` at medium complexity, best first.
    pub fn route(role: Role, models: &[ModelInfo]) -> Vec<ModelChoice> {
        Self::route_with(role, TaskComplexity::Medium, models)
    }

    /// Rank chat-capable models for `role` at the given complexity, best first.
    pub fn route_with(
        role: Role,
        complexity: TaskComplexity,
        models: &[ModelInfo],
    ) -> Vec<ModelChoice> {
        let profile = RoleProfile::for_task(role, complexity);
        let mut choices: Vec<ModelChoice> = models
            .iter()
            .filter(|model| Self::passes(model, &profile))
            .map(|model| Self::score(model, &profile))
            .collect();

        // Best score first; tie-break cheapest input → output.
        choices.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| cost_key(a).cmp(&cost_key(b)))
        });
        choices
    }

    /// Auto-pick the best model for a task: infer complexity from the
    /// description, keep `role` as the trait hint, and return the top pick.
    pub fn auto_route(role: Role, goal: &str, models: &[ModelInfo]) -> Option<ModelChoice> {
        let complexity = TaskComplexity::infer(goal);
        Self::route_with(role, complexity, models)
            .into_iter()
            .next()
    }

    /// Build a ranked [`RoutePlan`] for a task: infer complexity and keep the
    /// full, best-first list so callers can walk a fallback chain when a
    /// source hits its budget limit.
    pub fn plan(role: Role, goal: &str, models: &[ModelInfo]) -> RoutePlan {
        Self::plan_with(role, TaskComplexity::infer(goal), models)
    }

    /// Build a [`RoutePlan`] from an already-classified task (e.g. the first
    /// layer — Jev — decided role + complexity before we scored models).
    pub fn plan_with(role: Role, complexity: TaskComplexity, models: &[ModelInfo]) -> RoutePlan {
        let choices = Self::route_with(role, complexity, models);
        RoutePlan {
            role,
            complexity,
            choices,
            exhausted: HashSet::new(),
        }
    }

    fn passes(model: &ModelInfo, profile: &RoleProfile) -> bool {
        if !model.is_chat() || !model.tool_call || model.sunset_at.is_some() {
            return false;
        }
        if model.context_length.unwrap_or(u64::MAX) < profile.min_context {
            return false;
        }
        if profile.require_reasoning && !model.reasoning {
            return false;
        }
        if let Some(cap) = profile.max_input_idr
            && model.effective_input_idr().unwrap_or(u64::MAX) > cap
        {
            return false;
        }
        if let Some(cap) = profile.max_output_idr
            && model.effective_output_idr().unwrap_or(u64::MAX) > cap
        {
            return false;
        }
        true
    }

    fn score(model: &ModelInfo, profile: &RoleProfile) -> ModelChoice {
        let capability_n =
            provider_tier(model.owned_by.as_deref().unwrap_or("")) * model_grade(&model.id);
        let reasoning_n = if model.reasoning { 1.0 } else { 0.0 };
        let context_n = (model.context_length.unwrap_or(CONTEXT_CAP).min(CONTEXT_CAP) as f64)
            / (CONTEXT_CAP as f64);
        let vision_n = if model.is_vision() { 1.0 } else { 0.0 };
        let input_idr = model.effective_input_idr();
        let cheap_n = if model.is_free() {
            1.0
        } else {
            1.0 - (input_idr.unwrap_or(PRICE_CAP).min(PRICE_CAP) as f64) / (PRICE_CAP as f64)
        };

        let score = profile.w_capability * capability_n
            + profile.w_reasoning * reasoning_n
            + profile.w_context * context_n
            + profile.w_vision * vision_n
            + profile.w_cheap * cheap_n;

        ModelChoice {
            id: model.id.clone(),
            provider: model.owned_by.clone().unwrap_or_default(),
            source: model.source.clone(),
            context_length: model.context_length(),
            reasoning: model.reasoning,
            vision: model.is_vision(),
            free: model.is_free(),
            input_idr,
            output_idr: model.output_idr(),
            score,
            reason: reason(model, &profile.label),
        }
    }
}

fn cost_key(choice: &ModelChoice) -> (u64, u64) {
    (
        choice.input_idr.unwrap_or(u64::MAX),
        choice.output_idr.unwrap_or(u64::MAX),
    )
}

/// Coarse provider quality prior (a documented heuristic, not a benchmark).
fn provider_tier(owned_by: &str) -> f64 {
    match owned_by {
        "anthropic" => 1.0,
        "openai" => 0.95,
        "deepseek" => 0.90,
        "google" => 0.90,
        "x-ai" => 0.85,
        "moonshotai" => 0.80,
        "z-ai" => 0.75,
        "meta" => 0.70,
        "nvidia" => 0.70,
        "qwen" => 0.65,
        "stepfun" => 0.60,
        "minimax" => 0.60,
        "xiaomi" => 0.60,
        _ => 0.50,
    }
}

/// Intra-provider grade prior derived from the model *name* (e.g. Anthropic's
/// `haiku` < `sonnet` < `opus`). A documented heuristic — the catalog exposes no
/// benchmark score — used only to stop a cheap `haiku` outranking a `sonnet`/`opus`
/// whose observable metadata is otherwise identical.
fn model_grade(id: &str) -> f64 {
    let id = id.to_ascii_lowercase();
    // Ordered strongest first; the first matching token wins.
    const TIERS: &[(&str, f64)] = &[
        ("fable", 1.00),
        ("opus", 1.00),
        ("astra", 1.00),
        ("dominion", 1.00),
        ("sonnet", 0.92),
        ("solar", 0.60),
        ("sol", 0.88),
        ("max", 0.86),
        ("pro", 0.84),
        ("ultra", 0.84),
        ("plus", 0.82),
        ("turbo", 0.78),
        ("luna", 0.72),
        ("haiku", 0.68),
        ("retro", 0.66),
        ("mini", 0.62),
        ("nano", 0.55),
        ("flash", 0.60),
        ("lite", 0.50),
        ("micro", 0.48),
        ("small", 0.46),
    ];
    for (token, grade) in TIERS {
        if id.contains(token) {
            return *grade;
        }
    }
    if id.ends_with(":free") {
        return 0.45;
    }
    0.75
}

fn reason(model: &ModelInfo, role: &str) -> String {
    let mut tags: Vec<&str> = Vec::new();
    if model.reasoning {
        tags.push("reasoning");
    }
    if model.is_vision() {
        tags.push("vision");
    }
    let price = if model.is_free() {
        "free".to_string()
    } else {
        match model.input_idr() {
            Some(p) => format!("Rp{p}/1M in"),
            None => "price?".to_string(),
        }
    };
    if tags.is_empty() {
        format!("{role}: {price}")
    } else {
        format!("{role}: {} · {price}", tags.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::models::{Modalities, Pricing};

    #[allow(clippy::too_many_arguments)]
    fn m(
        id: &str,
        provider: &str,
        context: u64,
        reasoning: bool,
        vision: bool,
        free: bool,
        input_idr: u64,
        output_idr: u64,
    ) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            object: Some("model".into()),
            owned_by: Some(provider.into()),
            name: Some(id.into()),
            source: "kenari".to_string(),
            context_length: Some(context),
            max_output_tokens: None,
            endpoints: vec!["chat".into()],
            modalities: Some(Modalities {
                input: if vision {
                    vec!["text".into(), "image".into()]
                } else {
                    vec!["text".into()]
                },
                output: vec!["text".into()],
            }),
            pricing: Some(Pricing {
                currency: Some("IDR".into()),
                free,
                input: if free {
                    None
                } else {
                    Some(input_idr * 1_000_000)
                },
                output: if free {
                    None
                } else {
                    Some(output_idr * 1_000_000)
                },
                cache_read: None,
                cache_write: None,
                unit: Some("micro_idr_per_1m_tokens".into()),
            }),
            reasoning,
            reasoning_toggle: false,
            reasoning_options: Vec::new(),
            tool_call: true,
            sunset_at: None,
        }
    }

    fn first(role: Role, models: &[ModelInfo]) -> Option<ModelChoice> {
        Router::route(role, models).into_iter().next()
    }

    /// Re-tag a model's routing source (gateway) for fallback tests.
    fn src(mut model: ModelInfo, source: &str) -> ModelInfo {
        model.source = source.to_string();
        model
    }

    #[test]
    fn planner_requires_reasoning() {
        let models = vec![
            m(
                "cheap-no-reason",
                "deepseek",
                1_000_000,
                false,
                false,
                false,
                2000,
                8000,
            ),
            m(
                "brain",
                "anthropic",
                1_000_000,
                true,
                false,
                false,
                25000,
                125000,
            ),
        ];
        let pick = first(Role::Planner, &models).unwrap();
        assert_eq!(pick.id, "brain");
    }

    #[test]
    fn filters_non_chat_no_tool_and_deprecated() {
        let mut image = m("img", "openai", 1_000_000, true, true, true, 0, 0);
        image.endpoints = vec!["image".into()];
        let mut no_tool = m(
            "no-tool",
            "anthropic",
            1_000_000,
            true,
            false,
            false,
            25000,
            125000,
        );
        no_tool.tool_call = false;
        let mut deprecated = m("old", "deepseek", 1_000_000, true, false, false, 1000, 4000);
        deprecated.sunset_at = Some(1_792_638_000);
        let models = vec![
            image,
            no_tool,
            deprecated,
            m("ok", "openai", 1_000_000, true, true, false, 40000, 200000),
        ];
        let pick = first(Role::Planner, &models).unwrap();
        assert_eq!(pick.id, "ok");
    }

    #[test]
    fn reviewer_respects_price_cap() {
        // An expensive premium reasoning model is excluded by the reviewer's cap.
        let models = vec![
            m(
                "premium",
                "anthropic",
                1_000_000,
                true,
                true,
                false,
                80_000,
                400_000,
            ),
            m(
                "value", "deepseek", 1_000_000, true, false, false, 10_000, 20_000,
            ),
        ];
        let pick = first(Role::Reviewer, &models).unwrap();
        assert_eq!(pick.id, "value");
    }

    #[test]
    fn role_profiles_differ() {
        // A free reasoning+vision model wins cost-focused roles but not the
        // capability-focused planner (which rejects free-tier providers).
        let models = vec![
            m("free-omni", "qwen", 1_000_000, true, true, true, 0, 0),
            m(
                "flagship",
                "anthropic",
                1_000_000,
                true,
                true,
                false,
                25_000,
                125_000,
            ),
        ];
        assert_eq!(first(Role::Planner, &models).unwrap().id, "flagship");
        assert_eq!(first(Role::Reviewer, &models).unwrap().id, "free-omni");
    }

    #[test]
    fn context_floor_excludes_small_models() {
        let models = vec![
            m("tiny", "deepseek", 8_000, true, false, false, 1000, 4000),
            m(
                "big", "deepseek", 1_000_000, true, false, false, 10_000, 20_000,
            ),
        ];
        assert_eq!(first(Role::Planner, &models).unwrap().id, "big");
    }

    #[test]
    fn infer_complexity_uses_cue_words() {
        assert_eq!(
            TaskComplexity::infer("rename the variable"),
            TaskComplexity::Low
        );
        assert_eq!(
            TaskComplexity::infer("quick summary of this file"),
            TaskComplexity::Low
        );
        assert_eq!(
            TaskComplexity::infer("design the architecture and write a spec"),
            TaskComplexity::High
        );
        assert_eq!(
            TaskComplexity::infer("send the weekly status update to the team"),
            TaskComplexity::Medium
        );
    }

    #[test]
    fn role_inferred_from_goal() {
        assert_eq!(Role::from_goal("research the market"), Role::Researcher);
        assert_eq!(Role::from_goal("write a blog article"), Role::Writer);
        assert_eq!(Role::from_goal("review this pull request"), Role::Reviewer);
        assert_eq!(Role::from_goal("design a roadmap"), Role::Planner);
        assert_eq!(Role::from_goal("do something"), Role::General);
    }

    #[test]
    fn role_workflow_order_is_canonical() {
        assert!(Role::Planner.order() < Role::Researcher.order());
        assert!(Role::Researcher.order() < Role::Writer.order());
        assert!(Role::Writer.order() < Role::Reviewer.order());
        assert!(Role::Reviewer.order() < Role::General.order());
    }

    #[test]
    fn task_class_inferred_locally() {
        let class = TaskClass::infer("design a roadmap for the migration");
        assert_eq!(class.role, Role::Planner);
        assert_eq!(class.complexity, TaskComplexity::High);

        let quick = TaskClass::infer("rename the variable");
        assert_eq!(quick.complexity, TaskComplexity::Low);
    }

    #[test]
    fn complexity_selects_price_tier() {
        let models = vec![
            m(
                "flash-cheap",
                "openai",
                32_000,
                false,
                false,
                false,
                500,
                2500,
            ),
            m(
                "haiku",
                "anthropic",
                1_000_000,
                true,
                true,
                false,
                2000,
                10000,
            ),
            m(
                "sonnet",
                "anthropic",
                1_000_000,
                true,
                true,
                false,
                25_000,
                125_000,
            ),
            m(
                "opus",
                "anthropic",
                1_000_000,
                true,
                true,
                false,
                80_000,
                400_000,
            ),
        ];

        let low = Router::route_with(Role::Planner, TaskComplexity::Low, &models);
        assert_eq!(low[0].id, "haiku");

        let med = Router::route_with(Role::Planner, TaskComplexity::Medium, &models);
        assert_eq!(med[0].id, "opus");

        let high = Router::route_with(Role::Planner, TaskComplexity::High, &models);
        assert_eq!(high[0].id, "opus");
    }

    #[test]
    fn fallback_skips_exhausted_source_for_another_provider() {
        // Three sources: kenari (best), openrouter, clario. The best two models
        // are on kenari; the budget fallback must jump to a *different* gateway.
        let models = vec![
            src(
                m(
                    "kenari-a",
                    "anthropic",
                    1_000_000,
                    true,
                    true,
                    false,
                    20_000,
                    100_000,
                ),
                "kenari",
            ),
            src(
                m(
                    "kenari-b", "openai", 1_000_000, true, true, false, 30_000, 150_000,
                ),
                "kenari",
            ),
            src(
                m(
                    "openrouter-a",
                    "openai",
                    1_000_000,
                    true,
                    true,
                    false,
                    34_000,
                    170_000,
                ),
                "openrouter",
            ),
            src(
                m(
                    "clario-a", "deepseek", 1_000_000, true, false, false, 10_000, 20_000,
                ),
                "clario",
            ),
        ];

        let mut plan = Router::plan(Role::Planner, "design the architecture", &models);
        assert_eq!(plan.best().map(|c| c.source.as_str()), Some("kenari"));

        // Marking kenari exhausted moves to the next distinct source.
        assert!(plan.exclude("kenari"));
        assert_eq!(plan.best().map(|c| c.source.as_str()), Some("openrouter"));

        // The fallback for the top source is a different source, and never the
        // same source again.
        let fb = plan.fallback("kenari");
        assert!(fb.is_some());
        assert_ne!(fb.unwrap().source, "kenari");
    }

    #[test]
    fn plan_reports_selectable_sources_and_exhausts_idempotently() {
        let models = vec![
            src(
                m(
                    "a",
                    "anthropic",
                    1_000_000,
                    true,
                    true,
                    false,
                    20_000,
                    100_000,
                ),
                "kenari",
            ),
            src(
                m("b", "openai", 1_000_000, true, true, false, 30_000, 150_000),
                "openrouter",
            ),
        ];
        let mut plan = Router::plan(Role::Planner, "roadmap", &models);
        assert_eq!(plan.selectable_sources(), vec!["kenari", "openrouter"]);

        assert!(plan.exclude("kenari"));
        assert!(!plan.exclude("kenari")); // already exhausted
        assert_eq!(plan.selectable_sources(), vec!["openrouter"]);
        assert_eq!(plan.best().map(|c| c.source.as_str()), Some("openrouter"));

        // Exhausting everything leaves nothing selectable.
        assert!(plan.exclude("openrouter"));
        assert!(plan.best().is_none());
    }
}
