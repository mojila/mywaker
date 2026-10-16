//! The chat panel — live transcript plus the input prompt.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{App, TranscriptRole};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [transcript_area, input_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(area);

    render_transcript(frame, app, transcript_area);
    render_input(frame, app, input_area);
}

fn render_transcript(frame: &mut Frame, app: &App, area: Rect) {
    let name = app.chat_employee_name().unwrap_or("chat");

    let mut lines: Vec<Line> = Vec::new();
    for entry in app.transcript() {
        lines.push(role_line(entry.role, &entry.text));
    }
    if !app.streaming_text().is_empty() {
        lines.push(Line::from(vec![
            Span::styled("◔ ", Color::Yellow),
            Span::styled(app.streaming_text().to_string(), Color::Yellow),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(format!(" chat · {name} "))
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = Line::from(vec![
        Span::styled("> ", Color::Cyan),
        Span::raw(app.input().to_string()),
        Span::styled("█", Color::Cyan),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray);
    frame.render_widget(Paragraph::new(prompt).block(block), area);
}

fn role_line(role: TranscriptRole, text: &str) -> Line<'static> {
    match role {
        TranscriptRole::User => Line::from(vec![
            Span::styled("you ", Color::Cyan),
            Span::raw(text.to_string()),
        ]),
        TranscriptRole::Assistant => Line::from(vec![
            Span::styled("wkr ", Color::Green),
            Span::raw(text.to_string()),
        ]),
        TranscriptRole::Error => Line::from(vec![
            Span::styled("err ", Color::Red),
            Span::raw(text.to_string()),
        ]),
        TranscriptRole::Info => Line::from(Span::styled(text.to_string(), Color::DarkGray)),
    }
}
