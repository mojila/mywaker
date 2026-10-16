//! The chat panel — live transcript, a slash-command autocomplete menu, and the
//! input prompt.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::app::{App, TranscriptRole};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [transcript_area, input_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(area);

    render_transcript(frame, app, transcript_area);
    render_input(frame, app, input_area);
    render_command_menu(frame, app, area);
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

/// Render the slash-command autocomplete menu as a floating list anchored just
/// above the input bar (visible only while `app.command_options()` is `Some`).
fn render_command_menu(frame: &mut Frame, app: &App, area: Rect) {
    let Some((options, selected)) = app.command_options() else {
        return;
    };

    let content_width = options
        .iter()
        .map(|opt| opt.command.len() + opt.description.len() + 4)
        .max()
        .unwrap_or(0);
    let width = (content_width + 2).clamp(12, area.width as usize) as u16;
    let height = (options.len() + 2) as u16;

    // Anchor just above the input bar (the bottom 3 rows of the chat area).
    let input_top = area.y + area.height.saturating_sub(3);
    let y = input_top.saturating_sub(height);
    let menu_rect = Rect {
        x: area.x + 1,
        y,
        width: width.min(area.width.saturating_sub(2)),
        height,
    };

    let items: Vec<ListItem> = options
        .iter()
        .map(|opt| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<12}", opt.command),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(opt.description, Style::default().fg(Color::Gray)),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Color::DarkGray)
                .title(" commands ")
                .title_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("» ");

    frame.render_widget(Clear, menu_rect);
    frame.render_stateful_widget(list, menu_rect, &mut state);
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
