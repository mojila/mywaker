//! Workflow role, used only to order the team as a grid in the UI.
//!
//! This is a *display/ordering* concern, not model routing: each employee's
//! role is derived from their (stable) id and only drives the employee grid's
//! card order and labels.
//!
//! Resolution lives with the data owner — [`crate::team::Employee::workflow_role`]
//! wraps [`Role::from_label`] — so the rest of the app asks an employee for its
//! role instead of re-deriving it from the id at each call site.

/// A team workflow role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Orchestrator,
    Planner,
    Researcher,
    Engineer,
    Writer,
    Reviewer,
    General,
}

impl Role {
    /// Parse a role from an employee/agent id (`"orchestrator"`/`"lead"`/`"root"`,
    /// `"planner"`, `"researcher"`, `"coder"`/`"engineer"`/`"developer"`/
    /// `"software-engineer"`, `"writer"`, `"reviewer"`). Unknown ids map to
    /// [`Role::General`].
    pub fn from_label(label: &str) -> Role {
        match label {
            "orchestrator" | "lead" | "root" => Role::Orchestrator,
            "planner" => Role::Planner,
            "researcher" => Role::Researcher,
            "engineer" | "coder" | "developer" | "dev" | "software-engineer" => Role::Engineer,
            "writer" => Role::Writer,
            "reviewer" => Role::Reviewer,
            _ => Role::General,
        }
    }

    /// The canonical stage label for this role.
    pub fn label(self) -> &'static str {
        match self {
            Role::Orchestrator => "orchestrator",
            Role::Planner => "planner",
            Role::Researcher => "researcher",
            Role::Engineer => "engineer",
            Role::Writer => "writer",
            Role::Reviewer => "reviewer",
            Role::General => "general",
        }
    }

    /// Position in the canonical orchestrator → plan → research → implement →
    /// write → review workflow, used to order the team as a grid in the UI. The
    /// orchestrator sits at the root, above the pipeline stages it directs.
    pub fn order(self) -> u8 {
        match self {
            Role::Orchestrator => 0,
            Role::Planner => 1,
            Role::Researcher => 2,
            Role::Engineer => 3,
            Role::Writer => 4,
            Role::Reviewer => 5,
            Role::General => 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids_into_roles() {
        assert_eq!(Role::from_label("coder"), Role::Engineer);
        assert_eq!(Role::from_label("engineer"), Role::Engineer);
        assert_eq!(Role::from_label("software-engineer"), Role::Engineer);
        assert_eq!(Role::from_label("dev"), Role::Engineer);
        assert_eq!(Role::from_label("planner"), Role::Planner);
        assert_eq!(Role::from_label("writer"), Role::Writer);
        assert_eq!(Role::from_label("bogus"), Role::General);
    }

    /// The root employee is reachable by its id and by the two words people use
    /// for it, so a hand-written roster can name it either way.
    #[test]
    fn recognizes_the_orchestrator_and_its_aliases() {
        assert_eq!(Role::from_label("orchestrator"), Role::Orchestrator);
        assert_eq!(Role::from_label("lead"), Role::Orchestrator);
        assert_eq!(Role::from_label("root"), Role::Orchestrator);
        assert_eq!(Role::Orchestrator.label(), "orchestrator");
    }

    #[test]
    fn labels_round_trip() {
        assert_eq!(Role::Engineer.label(), "engineer");
        assert_eq!(Role::from_label("engineer").label(), "engineer");
        assert_eq!(Role::from_label("writer").label(), "writer");
    }

    #[test]
    fn workflow_order_is_canonical() {
        assert!(Role::Orchestrator.order() < Role::Planner.order());
        assert!(Role::Planner.order() < Role::Researcher.order());
        assert!(Role::Researcher.order() < Role::Engineer.order());
        assert!(Role::Engineer.order() < Role::Writer.order());
        assert!(Role::Writer.order() < Role::Reviewer.order());
        assert!(Role::Reviewer.order() < Role::General.order());
    }
}
