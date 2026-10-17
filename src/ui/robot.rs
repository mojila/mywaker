//! State-driven animated ASCII robot avatar (no external art dependencies).

use ratatui::text::Line;

use crate::ui::status;

/// Aggregate team mood the robot reflects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotState {
    Idle,
    Thinking,
    Working,
    Error,
}

/// Render the robot as pre-padded lines for the given state + animation frame.
pub fn render(state: RobotState, frame: u64) -> Vec<Line<'static>> {
    let spinner = status::spinner(frame);

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a line back into its visible text (for assertions).
    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// The first non-whitespace character of a rendered line.
    fn first_char(line: &Line) -> char {
        text(line).trim().chars().next().unwrap_or(' ')
    }

    /// The antenna glyph (first char of the first rendered line).
    fn antenna(state: RobotState, frame: u64) -> char {
        first_char(&render(state, frame)[0])
    }

    #[test]
    fn every_state_renders_nine_lines() {
        for state in [
            RobotState::Idle,
            RobotState::Thinking,
            RobotState::Working,
            RobotState::Error,
        ] {
            for frame in [0u64, 1, 7, 10, 19, 20, 12345] {
                assert_eq!(
                    render(state, frame).len(),
                    9,
                    "state {state:?} frame {frame}"
                );
            }
        }
    }

    #[test]
    fn idle_antenna_blinks_on_a_20_tick_period() {
        // Frames 0..=9 light the antenna; frames 10..=19 dim it.
        assert_eq!(antenna(RobotState::Idle, 0), '●');
        assert_eq!(antenna(RobotState::Idle, 9), '●');
        assert_eq!(antenna(RobotState::Idle, 10), '·');
        assert_eq!(antenna(RobotState::Idle, 19), '·');
        assert_eq!(antenna(RobotState::Idle, 20), '●');
    }

    #[test]
    fn thinking_and_working_antenna_spins_with_frame() {
        // The Braille spinner advances every tick.
        assert_ne!(
            antenna(RobotState::Thinking, 0),
            antenna(RobotState::Thinking, 1)
        );
        assert_ne!(
            antenna(RobotState::Working, 0),
            antenna(RobotState::Working, 1)
        );
        // …and wraps back around every 10 frames.
        assert_eq!(
            antenna(RobotState::Thinking, 0),
            antenna(RobotState::Thinking, 10)
        );
    }

    #[test]
    fn error_state_is_stable() {
        // Error is not animated: antenna stays ✕ regardless of frame.
        assert_eq!(antenna(RobotState::Error, 0), '✕');
        assert_eq!(antenna(RobotState::Error, 50), '✕');
        // Error eyes are also fixed ✕.
        assert!(text(&render(RobotState::Error, 7)[3]).contains('✕'));
    }
}
