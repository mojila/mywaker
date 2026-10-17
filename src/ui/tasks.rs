//! The "tasks" panel: a right-hand list of every employee and its latest
//! task, complementing the workflow pipeline. Read-only — the pipeline stays
//! the selection surface; this panel just surfaces what each teammate is up to.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::team::{Employee, EmployeeStatus};

/// Braille spinner frames, indexed by the app tick (in sync with the avatar).
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let employees = app.team().employees();
    let order = app.workflow_order();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(" tasks ".bold().cyan());

    if order.is_empty() {
        frame.render_widget(Paragraph::new("no employees").block(block), area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    for &index in &order {
        let employee = &employees[index];
        let selected = index == app.selected();
        lines.push(employee_header(employee, app.frame(), selected));
        lines.push(task_line(employee));
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Line 1: status icon + name (+ role, dim).
fn employee_header(employee: &Employee, tick: u64, selected: bool) -> Line<'static> {
    let name_style = if selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        status_icon(employee.status, tick),
        Span::raw(" "),
        Span::styled(truncate(&employee.name, 18), name_style),
        Span::styled(" · ", Color::DarkGray),
        Span::styled(employee.role.clone(), Color::DarkGray),
    ])
}

/// Line 2: the employee's latest task, or a muted placeholder when idle.
fn task_line(employee: &Employee) -> Line<'static> {
    match &employee.last_task {
        Some(task) => Line::from(vec![
            Span::raw("  "),
            Span::styled(
                truncate(task, 24),
                Style::default().fg(task_color(employee.status)),
            ),
        ]),
        None => Line::from(Span::styled("  —", Color::DarkGray)),
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

fn status_color(status: EmployeeStatus) -> Color {
    match status {
        EmployeeStatus::Idle => Color::DarkGray,
        EmployeeStatus::Thinking => Color::Yellow,
        EmployeeStatus::Working => Color::Cyan,
        EmployeeStatus::Done => Color::Green,
        EmployeeStatus::Error => Color::Red,
    }
}

fn task_color(status: EmployeeStatus) -> Color {
    match status {
        EmployeeStatus::Idle => Color::Gray,
        EmployeeStatus::Thinking => Color::Yellow,
        EmployeeStatus::Working => Color::Cyan,
        EmployeeStatus::Done => Color::Green,
        EmployeeStatus::Error => Color::Red,
    }
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
