//! The reopen prompt: a centred modal offering back the tasks mywaker paused
//! when it last closed.
//!
//! The body copy says the tasks are *re-run from the beginning*, because that is
//! what resume does — each one starts a fresh OpenCode session rather than
//! continuing the interrupted one.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

use super::{centered, key_hint};

/// Draw the reopen prompt over `area` — a no-op when there is no prompt to show.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let Some(prompt) = app.resume_prompt() else {
        return;
    };

    frame.render_widget(Clear, area);
    let popup = centered(area, 58, 10);

    let count = prompt.count();
    let noun = if count == 1 { "task was" } else { "tasks were" };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("{count} {noun} paused when mywaker closed."),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "They will be re-run from the beginning.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        key_hint("y · Enter", "resume all"),
        key_hint("n · Esc", "keep paused"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::Yellow)
        .bg(Color::Black)
        .title(" resume paused tasks ".bold().cyan());

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center),
        popup,
    );
}
