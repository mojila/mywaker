//! The "tasks" panel: a right-hand list of every employee and its latest
//! task, complementing the workflow pipeline. Read-only — the pipeline stays
//! the selection surface; this panel just surfaces what each teammate is up to.
//!
//! Above the per-employee board sits the persisted scheduled-task queue: the
//! autonomous (cron/file-watch) goals the harness has on record, active work
//! first. It is shown only when there is at least one task.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{self, App};
use crate::task::Task;
use crate::team::Employee;
use crate::ui::{status, truncate};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let employees = app.team().employees();
    let order = app.workflow_order();
    let tasks = app.tasks();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(" tasks ".bold().cyan());

    if order.is_empty() {
        frame.render_widget(Paragraph::new("no employees").block(block), area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // The persisted scheduled-task queue, active work first. The heading only
    // appears when there is something queued, so an idle store stays quiet.
    if !tasks.is_empty() {
        lines.push(Line::from(Span::styled(
            " queue",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for &index in &app::order_tasks(tasks) {
            lines.push(queued_task_line(&tasks[index]));
        }
        lines.push(Line::from(""));
    }

    for &index in &order {
        let employee = &employees[index];
        let selected = index == app.selected();
        lines.push(employee_header(employee, app.frame(), selected));
        lines.push(task_line(employee));
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// One line of the persisted-task queue: the status label (colored) plus a
/// truncated goal summary.
fn queued_task_line(task: &Task) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<9}", task.status.label()),
            Style::default().fg(status::task_color(task.status)),
        ),
        Span::styled(truncate(&app::task_summary(&task.goal), 18), Color::Gray),
    ])
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
        status::icon_span(employee.status, tick),
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
                Style::default().fg(status::text_color(employee.status)),
            ),
        ]),
        None => Line::from(Span::styled("  —", Color::DarkGray)),
    }
}
