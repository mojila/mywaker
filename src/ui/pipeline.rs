//! The team pipeline: a left → right workflow diagram (plan → research →
//! write → review → …), one stage box per employee with a live status and
//! spinner, connected by arrows. Replaces the flat roster list.
//!
//! Stages are ordered by routing role ([`Role::order`]), so the running task
//! is always laid out along the canonical workflow regardless of the order in
//! which employees were added to `team.toml`.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::route::Role;
use crate::team::{Employee, EmployeeStatus};

/// Braille spinner frames, indexed by the app tick (in sync with the avatar).
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let order = app.workflow_order();
    let employees = app.team().employees();

    if order.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" team ".bold().cyan());
        frame.render_widget(
            Paragraph::new("no employees — add one with `mywaker employee add`")
                .block(block)
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    // Horizontal row: [stage, arrow, stage, arrow, …].
    let steps = order.len();
    let mut constraints = Vec::with_capacity(steps * 2 - 1);
    for step in 0..steps {
        constraints.push(Constraint::Fill(1));
        if step + 1 < steps {
            constraints.push(Constraint::Length(3));
        }
    }

    // Vertically centre the row within the main area.
    let [_, row_area, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(STAGE_HEIGHT),
        Constraint::Fill(1),
    ])
    .areas(area);

    let columns = Layout::horizontal(constraints).split(row_area);

    let mut column = 0;
    for (step, &employee_index) in order.iter().enumerate() {
        let employee = &employees[employee_index];
        let selected = employee_index == app.selected();
        render_stage(frame, app, employee, columns[column], selected);
        column += 1;

        if step + 1 < steps {
            render_arrow(frame, columns[column]);
            column += 1;
        }
    }
}

/// Number of content rows inside a stage box (plus two border rows).
const CONTENT_LINES: u16 = 4;
const STAGE_HEIGHT: u16 = CONTENT_LINES + 2;

/// The workflow stage label for an employee, from their routing role.
fn stage_label(employee: &Employee) -> String {
    Role::from_label(&employee.id).label().to_ascii_uppercase()
}

fn render_stage(frame: &mut Frame, app: &App, employee: &Employee, area: Rect, selected: bool) {
    let tick = app.frame();
    let color = status_color(employee.status);

    let title = Span::styled(
        format!(" {} ", stage_label(employee)),
        Style::default().fg(color).add_modifier(if selected {
            Modifier::BOLD
        } else {
            Modifier::DIM
        }),
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        })
        .title(title);

    let width = area.width.saturating_sub(2) as usize;

    let lines = vec![
        Line::from(vec![
            status_icon(employee.status, tick),
            Span::raw(" "),
            Span::styled(
                truncate(&employee.name, width.saturating_sub(2)),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            truncate(&employee.role, width),
            Color::DarkGray,
        )),
        status_label(employee.status),
        Line::from(Span::styled(
            truncate(employee.model.as_deref().unwrap_or("no model"), width),
            Color::DarkGray,
        )),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

fn render_arrow(frame: &mut Frame, area: Rect) {
    let [_, arrow_row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    let arrow = Paragraph::new(Line::from(Span::styled(
        "─▶",
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);

    frame.render_widget(arrow, arrow_row);
}

fn status_color(status: EmployeeStatus) -> Color {
    match status {
        EmployeeStatus::Idle => Color::DarkGray,
        EmployeeStatus::Thinking => Color::Yellow,
        EmployeeStatus::Working => Color::Cyan,
        EmployeeStatus::Done => Color::Green,
        EmployeeStatus::Error => Color::Red,
    }
}

fn status_icon(status: EmployeeStatus, tick: u64) -> Span<'static> {
    let symbol = match status {
        EmployeeStatus::Idle => "●",
        EmployeeStatus::Thinking => SPINNER[(tick as usize) % SPINNER.len()],
        EmployeeStatus::Working => "▶",
        EmployeeStatus::Done => "✓",
        EmployeeStatus::Error => "✕",
    };
    Span::styled(symbol, status_color(status))
}

fn status_label(status: EmployeeStatus) -> Line<'static> {
    let (text, color) = match status {
        EmployeeStatus::Idle => ("idle", Color::DarkGray),
        EmployeeStatus::Thinking => ("thinking", Color::Yellow),
        EmployeeStatus::Working => ("working", Color::Cyan),
        EmployeeStatus::Done => ("done", Color::Green),
        EmployeeStatus::Error => ("error", Color::Red),
    };
    Line::from(Span::styled(text, color))
}

/// Truncate to `width` display columns, appending an ellipsis when clipped.
fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
