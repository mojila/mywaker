//! Workflow role, used only to order the team as a pipeline in the UI.
//!
//! This is a *display/ordering* concern, not model routing: each employee's
//! role is derived from their (stable) id and only drives the pipeline
//! diagram's stage order and labels.

/// A team workflow role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Planner,
    Researcher,
    Engineer,
    Writer,
    Reviewer,
    General,
}

impl Role {
    /// Parse a role from an employee/agent id (`"planner"`, `"researcher"`,
    /// `"coder"`/`"engineer"`/`"developer"`/`"software-engineer"`, `"writer"`,
    /// `"reviewer"`). Unknown ids map to [`Role::General`].
    pub fn from_label(label: &str) -> Role {
        match label {
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
            Role::Planner => "planner",
            Role::Researcher => "researcher",
            Role::Engineer => "engineer",
            Role::Writer => "writer",
            Role::Reviewer => "reviewer",
            Role::General => "general",
        }
    }

    /// Position in the canonical plan → research → implement → write → review
    /// workflow, used to order the team as a pipeline in the UI.
    pub fn order(self) -> u8 {
        match self {
            Role::Planner => 0,
            Role::Researcher => 1,
            Role::Engineer => 2,
            Role::Writer => 3,
            Role::Reviewer => 4,
            Role::General => 5,
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

    #[test]
    fn labels_round_trip() {
        assert_eq!(Role::Engineer.label(), "engineer");
        assert_eq!(Role::from_label("engineer").label(), "engineer");
        assert_eq!(Role::from_label("writer").label(), "writer");
    }

    #[test]
    fn workflow_order_is_canonical() {
        assert!(Role::Planner.order() < Role::Researcher.order());
        assert!(Role::Researcher.order() < Role::Engineer.order());
        assert!(Role::Engineer.order() < Role::Writer.order());
        assert!(Role::Writer.order() < Role::Reviewer.order());
        assert!(Role::Reviewer.order() < Role::General.order());
    }
}
