//! Status presentation: the single home for the glyph, colour, and spinner used
//! to draw an employee's or a task's lifecycle state.
//!
//! Three panels render the same lifecycle status — the employee grid, the
//! tasks panel, and the robot avatar — and each had grown its own copy of the
//! tables (the spinner alone existed four times). They live here instead.
//!
//! The *text* of an employee status is [`EmployeeStatus::label`] in the model
//! layer; this module only adds ratatui styling, which keeps `team.rs` free of a
//! UI dependency.

use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::task::TaskStatus;
use crate::team::EmployeeStatus;
use crate::ui::truncate;

use super::theme;

/// Braille spinner frames, indexed by the app tick (in sync with the avatar).
pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The spinner frame for `tick`, wrapping every [`SPINNER`] cycle.
pub fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}

/// The accent colour for an employee status — its border, title, and status
/// text.
pub fn color(status: EmployeeStatus) -> Color {
    // Lookups into the palette, not a second table (`docs/keymap.md` §2.2 rule 3).
    match status {
        EmployeeStatus::Idle => theme::MUTED,
        EmployeeStatus::Thinking => theme::WARN,
        EmployeeStatus::Working => theme::ACCENT,
        EmployeeStatus::Done => theme::OK,
        EmployeeStatus::Error => theme::ERR,
    }
}

/// The status colour for *body text* (a latched task summary): the same accent
/// as [`color`], brightened out of the dim idle grey so the text stays legible.
pub fn text_color(status: EmployeeStatus) -> Color {
    match status {
        EmployeeStatus::Idle => theme::TEXT,
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

/// The "thinking" indicator: one dim line of spinner plus the tail of the
/// employee's current reasoning.
///
/// A deliberation is not the answer, so it never joins the transcript. Instead
/// one pinned row says *that* the employee is thinking and *what about*, which is
/// enough to tell a working turn from a hung one without the scratchpad pushing
/// the real output off the screen.
///
/// `tail` is [`Thinking::tail`](crate::app::Thinking::tail) — already collapsed to
/// a single line and bounded — and is truncated here to `width` columns.
pub fn thinking_line(tick: u64, tail: &str, width: usize) -> Line<'static> {
    let mut spans = vec![
        Span::styled(
            format!("{} ", spinner(tick)),
            Style::default().fg(theme::WARN),
        ),
        Span::styled(
            "thinking",
            Style::default()
                .fg(theme::WARN)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    // The tail is the part that gives way: the spinner and the word must survive
    // a narrow panel, because they are what says the employee is alive.
    let used = 2 + "thinking".len();
    if !tail.is_empty() && width > used + 3 {
        let room = width - used - 1;
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            truncate(tail, room),
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::DIM),
        ));
    }
    Line::from(spans)
}

/// The "thinking" indicator for an employee's status: `None` when there is
/// nothing to show, so the caller can decide whether the row exists at all.
#[cfg(test)]
pub fn thinking_indicator(
    status: EmployeeStatus,
    tick: u64,
    tail: &str,
    width: usize,
) -> Option<Line<'static>> {
    (status == EmployeeStatus::Thinking).then(|| thinking_line(tick, tail, width))
}

/// The accent colour for a persisted task's status (the queue panel).
pub fn task_color(status: TaskStatus) -> Color {
    match status {
        TaskStatus::Pending => theme::MUTED,
        TaskStatus::Running => theme::ACCENT,
        TaskStatus::Paused => theme::WARN,
        TaskStatus::Retrying => theme::WARN,
        TaskStatus::Succeeded => theme::OK,
        TaskStatus::Failed => theme::ERR,
    }
}

#[cfg(test)]
mod tests {
    use super::theme;
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
        assert_eq!(text_color(EmployeeStatus::Idle), theme::TEXT);
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
        assert_eq!(task_color(TaskStatus::Pending), theme::MUTED);
        assert_eq!(task_color(TaskStatus::Running), theme::ACCENT);
        assert_eq!(task_color(TaskStatus::Paused), theme::WARN);
        assert_eq!(task_color(TaskStatus::Retrying), theme::WARN);
        assert_eq!(task_color(TaskStatus::Succeeded), theme::OK);
        assert_eq!(task_color(TaskStatus::Failed), theme::ERR);
        // Active work reads brighter than terminal work.
        assert_ne!(
            task_color(TaskStatus::Running),
            task_color(TaskStatus::Succeeded)
        );
    }

    /// The indicator is one row: spinner, the word, then the tail — and the
    /// spinner and word survive a narrow panel.
    #[test]
    fn thinking_line_is_spinner_word_then_tail() {
        let line = thinking_line(0, "weighing the options", 80);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            text,
            format!("{} thinking weighing the options", SPINNER[0])
        );

        // The spinner is the live frame, so the row animates.
        let later = thinking_line(1, "weighing the options", 80);
        let later_text: String = later.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_ne!(text, later_text);
    }

    /// With no tail yet the row still says "thinking": that is the part that
    /// tells a working turn from a hung one.
    #[test]
    fn an_empty_tail_still_shows_the_word() {
        let line = thinking_line(0, "", 80);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, format!("{} thinking", SPINNER[0]));
    }

    /// A narrow panel drops the tail and keeps the indicator, rather than
    /// truncating "thinking" itself.
    #[test]
    fn a_narrow_thinking_line_keeps_the_indicator() {
        let line = thinking_line(0, "a very long deliberation indeed", 12);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("thinking"), "{text}");
        assert!(text.chars().count() <= 12, "{text}");

        // At an absurd width there is not even room for the tail; the word
        // survives anyway.
        let tiny = thinking_line(0, "tail", 4);
        let tiny_text: String = tiny.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(tiny_text.contains("thinking"));
    }

    /// The indicator only exists while the employee is actually thinking.
    #[test]
    fn thinking_indicator_is_thinking_only() {
        assert!(thinking_indicator(EmployeeStatus::Thinking, 0, "x", 40).is_some());
        for status in [
            EmployeeStatus::Idle,
            EmployeeStatus::Working,
            EmployeeStatus::Done,
            EmployeeStatus::Error,
        ] {
            assert!(
                thinking_indicator(status, 0, "x", 40).is_none(),
                "{status:?}"
            );
        }
    }
}
