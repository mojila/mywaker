//! The ratatui renderer: title bar, robot avatar, team dashboard, chat, log, help.

mod chat;
mod dashboard;
mod robot;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{App, Mode};

use robot::{RobotState, render as render_robot};

pub fn render(frame: &mut Frame, app: &App) {
    let [title_area, main_area, log_area, help_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_title(frame, app, title_area);
    render_main(frame, app, main_area);
    render_log(frame, app, log_area);
    render_help(frame, app, help_area);
}

fn render_title(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Min(24), Constraint::Length(32)]).areas(area);

    let title = Line::from(vec![
        Span::styled(
            " mywaker ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " a team of AI employees ",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), left);

    frame.render_widget(
        Paragraph::new(connection_line(app)).alignment(Alignment::Right),
        right,
    );
}

fn connection_line(app: &App) -> Line<'static> {
    if let Some(err) = app.connection_error() {
        return Line::from(vec![
            Span::styled("✕ ", Color::Red),
            Span::styled(err.to_string(), Color::Red),
        ]);
    }
    if let Some(version) = app.server_version() {
        return Line::from(vec![
            Span::styled("● ", Color::Green),
            Span::raw(format!("opencode v{version}")),
            Span::styled(format!(" · {} agents", app.agent_count()), Color::Gray),
        ]);
    }
    Line::from(vec![
        Span::styled("◌ ", Color::Yellow),
        Span::styled("connecting…".to_string(), Color::Gray),
    ])
}

fn render_main(frame: &mut Frame, app: &App, area: Rect) {
    match app.mode() {
        Mode::Chat => chat::render(frame, app, area),
        Mode::Roster => {
            let [robot_area, team_area] =
                Layout::horizontal([Constraint::Length(24), Constraint::Fill(1)]).areas(area);
            render_robot_widget(frame, app, robot_area);
            dashboard::render(frame, app, team_area);
        }
    }
}

fn render_robot_widget(frame: &mut Frame, app: &App, area: Rect) {
    let state = robot_state(app);
    let lines = render_robot(state, app.frame());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(" waker ".bold().cyan());

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn robot_state(app: &App) -> RobotState {
    if app.connection_error().is_some() {
        return RobotState::Error;
    }
    if !app.is_connected() {
        return RobotState::Thinking;
    }
    if app.team().is_busy() {
        return RobotState::Working;
    }
    RobotState::Idle
}

fn render_log(frame: &mut Frame, app: &App, area: Rect) {
    let mut text: Vec<Line> = vec![stream_stats(app)];

    for line in app.log().iter().rev().take(3) {
        text.push(Line::from(Span::styled(line.clone(), Color::DarkGray)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(" log ".bold().cyan());

    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn stream_stats(app: &App) -> Line<'static> {
    let mut spans: Vec<Span> = vec![
        Span::styled(" stream ", Color::Cyan),
        Span::styled(format!("{} agents", app.agent_count()), Color::Gray),
        Span::raw(" · "),
        Span::styled(format!("{} events", app.event_count()), Color::Gray),
    ];
    if let Some(event) = app.last_event() {
        spans.push(Span::raw(" · "));
        spans.push(Span::styled(format!("last: {event}"), Color::Gray));
    }
    Line::from(spans)
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.mode() {
        Mode::Roster => Line::from(vec![
            " ↑/k ".bold().cyan(),
            "move ".dim(),
            " ↓/j ".bold().cyan(),
            "move ".dim(),
            " Enter ".bold().cyan(),
            "chat ".dim(),
            " q ".bold().cyan(),
            "quit ".dim(),
        ]),
        Mode::Chat => Line::from(vec![
            " Enter ".bold().cyan(),
            "send ".dim(),
            " Esc ".bold().cyan(),
            "back ".dim(),
            " Ctrl-C ".bold().cyan(),
            "quit ".dim(),
        ]),
    };
    frame.render_widget(Paragraph::new(help), area);
}
