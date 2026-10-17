//! Status presentation: the single home for the glyph, colour, and spinner used
//! to draw an employee's or a task's lifecycle state.
//!
//! Three panels render the same lifecycle status — the workflow pipeline, the
//! tasks panel, and the robot avatar — and each had grown its own copy of the
//! tables (the spinner alone existed four times). They live here instead.
//!
//! The *text* of an employee status is [`EmployeeStatus::label`] in the model
//! layer; this module only adds ratatui styling, which keeps `team.rs` free of a
//! UI dependency.

use ratatui::style::Color;
use ratatui::text::Span;

use crate::task::TaskStatus;
use crate::team::EmployeeStatus;

/// Braille spinner frames, indexed by the app tick (in sync with the avatar).
pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The spinner frame for `tick`, wrapping every [`SPINNER`] cycle.
pub fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}

/// The accent colour for an employee status — its border, title, and status
/// text.
pub fn color(status: EmployeeStatus) -> Color {
    match status {
        EmployeeStatus::Idle => Color::DarkGray,
        EmployeeStatus::Thinking => Color::Yellow,
        EmployeeStatus::Working => Color::Cyan,
        EmployeeStatus::Done => Color::Green,
        EmployeeStatus::Error => Color::Red,
    }
}

/// The status colour for *body text* (a latched task summary): the same accent
/// as [`color`], brightened out of the dim idle grey so the text stays legible.
pub fn text_color(status: EmployeeStatus) -> Color {
    match status {
        EmployeeStatus::Idle => Color::Gray,
        other => color(other),
    }
}

/// The glyph for an employee status; `Thinking` animates through the spinner.
pub fn icon(status: EmployeeStatus, tick: u64) -> &'static str {
    match status {
        EmployeeStatus::Idle => "●",
        EmployeeStatus::Thinking => spinner(tick),
        EmployeeStatus::Working => "▶",
        EmployeeStatus::Done => "✓",
        EmployeeStatus::Error => "✕",
    }
}

/// [`icon`], styled in the status's [`color`].
pub fn icon_span(status: EmployeeStatus, tick: u64) -> Span<'static> {
    Span::styled(icon(status, tick), color(status))
}

/// The accent colour for a persisted task's status (the queue panel).
pub fn task_color(status: TaskStatus) -> Color {
    match status {
        TaskStatus::Pending => Color::DarkGray,
        TaskStatus::Running => Color::Cyan,
        TaskStatus::Retrying => Color::Yellow,
        TaskStatus::Succeeded => Color::Green,
        TaskStatus::Failed => Color::Red,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_wraps_and_thinking_animates() {
        assert_eq!(spinner(0), SPINNER[0]);
        assert_eq!(spinner(SPINNER.len() as u64), SPINNER[0]);
        assert_ne!(spinner(0), spinner(1));
        // Thinking borrows the spinner; every other status is a fixed glyph.
        assert_eq!(icon(EmployeeStatus::Thinking, 3), spinner(3));
    }

    #[test]
    fn icons_are_stable_per_status() {
        assert_eq!(icon(EmployeeStatus::Idle, 5), "●");
        assert_eq!(icon(EmployeeStatus::Working, 5), "▶");
        assert_eq!(icon(EmployeeStatus::Done, 5), "✓");
        assert_eq!(icon(EmployeeStatus::Error, 5), "✕");
    }

    /// Body text is the accent colour, except idle — which brightens so a
    /// latched task summary stays readable.
    #[test]
    fn body_text_color_only_diverges_from_the_accent_when_idle() {
        assert_eq!(text_color(EmployeeStatus::Idle), Color::Gray);
        assert_ne!(
            text_color(EmployeeStatus::Idle),
            color(EmployeeStatus::Idle),
            "idle body text must not be the dim border grey"
        );

        for status in [
            EmployeeStatus::Thinking,
            EmployeeStatus::Working,
            EmployeeStatus::Done,
            EmployeeStatus::Error,
        ] {
            assert_eq!(text_color(status), color(status), "{status:?}");
        }
    }

    #[test]
    fn task_colors_track_task_status() {
        assert_eq!(task_color(TaskStatus::Pending), Color::DarkGray);
        assert_eq!(task_color(TaskStatus::Running), Color::Cyan);
        assert_eq!(task_color(TaskStatus::Retrying), Color::Yellow);
        assert_eq!(task_color(TaskStatus::Succeeded), Color::Green);
        assert_eq!(task_color(TaskStatus::Failed), Color::Red);
        // Active work reads brighter than terminal work.
        assert_ne!(
            task_color(TaskStatus::Running),
            task_color(TaskStatus::Succeeded)
        );
    }
}
