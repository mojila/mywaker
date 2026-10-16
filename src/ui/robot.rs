//! State-driven animated ASCII robot avatar (no external art dependencies).

use ratatui::text::Line;

/// Aggregate team mood the robot reflects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotState {
    Idle,
    Thinking,
    Working,
    Error,
}

/// Braille spinner frames, indexed by tick.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Render the robot as pre-padded lines for the given state + animation frame.
pub fn render(state: RobotState, frame: u64) -> Vec<Line<'static>> {
    let spinner = SPINNER[(frame as usize) % SPINNER.len()];

    let antenna = match state {
        RobotState::Error => "✕",
        RobotState::Idle => {
            if frame % 20 < 10 {
                "●"
            } else {
                "·"
            }
        }
        RobotState::Thinking | RobotState::Working => spinner,
    };

    let eyes = match state {
        RobotState::Idle => "●   ●",
        RobotState::Thinking => {
            if frame % 8 < 4 {
                "◉   ◉"
            } else {
                "◍   ◍"
            }
        }
        RobotState::Working => "▶   ◀",
        RobotState::Error => "✕   ✕",
    };

    let mouth = match state {
        RobotState::Thinking => spinner,
        RobotState::Error => "⋁",
        _ => "▂",
    };

    let lines = [
        format!("        {antenna}         "),
        "   ▄▄▄▄▄▄▄▄▄▄▄▄   ".to_string(),
        "   █            █   ".to_string(),
        format!("   █    {eyes}    █   "),
        format!("   █      {mouth}      █   "),
        "   █▄▄▄▄▄▄▄▄▄▄▄▄▄▄█   ".to_string(),
        "   ┃   MYWAKER   ┃   ".to_string(),
        "  ┌┴──────────────┴┐  ".to_string(),
        "  └────────────────┘  ".to_string(),
    ];

    lines.into_iter().map(Line::raw).collect()
}
