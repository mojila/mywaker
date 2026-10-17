//! The team pipeline: a top-down tree diagram — the planner on top, **Jev**
//! (the question answerer) in the middle, and the remaining employees
//! (researcher, writer, reviewer, …) fanned out along the bottom. One box per
//! employee with a live status and spinner. Jev is a shared, non-employee node
//! every employee delegates questions to via `@jev`.
//!
//! Stages are ordered by routing role ([`Role::order`]): the first stage (the
//! planner) is the tree's root and the rest form the leaf row.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::role::Role;
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

    // Root = the planner (first stage); leaves = everyone else.
    let top = order[0];
    let bottom = &order[1..];
    render_tree(frame, app, area, top, bottom, employees);
}

/// Number of content rows inside a stage box (plus two border rows).
const CONTENT_LINES: u16 = 3;
const STAGE_HEIGHT: u16 = CONTENT_LINES + 2;

/// Maximum width of a single node box (the root and Jev node are centred to
/// this, while leaf nodes share the full row).
const MAX_NODE_WIDTH: u16 = 42;

fn render_tree(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    top: usize,
    bottom: &[usize],
    employees: &[Employee],
) {
    let has_bottom = !bottom.is_empty();
    let node_width = (area.width / (bottom.len().max(1) as u16)).clamp(14, MAX_NODE_WIDTH);

    // Rows (top → bottom): [fill, planner, arrow, jev, arrow, leaves, fill].
    let mut constraints = vec![
        Constraint::Fill(1),
        Constraint::Length(STAGE_HEIGHT), // planner
        Constraint::Length(1),            // arrow
        Constraint::Length(STAGE_HEIGHT), // jev
    ];
    if has_bottom {
        constraints.push(Constraint::Length(1)); // arrow
        constraints.push(Constraint::Length(STAGE_HEIGHT)); // leaves
    }
    constraints.push(Constraint::Fill(1));

    let rows = Layout::vertical(constraints).split(area);

    render_stage(
        frame,
        app,
        &employees[top],
        centered(rows[1], node_width),
        top == app.selected(),
    );
    render_arrow_down(frame, rows[2]);
    render_jev_stage(frame, app, centered(rows[3], node_width));

    if has_bottom {
        render_arrow_down(frame, rows[4]);
        let leaves = Layout::horizontal(vec![Constraint::Fill(1); bottom.len()]).split(rows[5]);
        for (i, &employee_index) in bottom.iter().enumerate() {
            render_stage(
                frame,
                app,
                &employees[employee_index],
                leaves[i],
                employee_index == app.selected(),
            );
        }
    }
}

/// Centre `area` to `width` columns (leaving it untouched when narrower).
fn centered(area: Rect, width: u16) -> Rect {
    if area.width <= width {
        return area;
    }
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(area);
    center
}

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
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

/// The Jev (System One) answerer node — a shared, non-employee stage every
/// employee delegates questions to via `@jev`. Styled distinctly from employees.
fn render_jev_stage(frame: &mut Frame, app: &App, area: Rect) {
    let enabled = app.jev_enabled();
    let accent = if enabled {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = Span::styled(
        " jev ",
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(title);

    let lines = vec![
        Line::from(vec![
            Span::styled("◆", accent),
            Span::raw(" "),
            Span::styled("jev", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::styled("answers questions", Color::DarkGray)),
        Span::styled(
            if enabled { "on — @jev" } else { "off" },
            if enabled {
                Color::Green
            } else {
                Color::DarkGray
            },
        )
        .into(),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

/// A downward connector (planner → jev → leaves).
fn render_arrow_down(frame: &mut Frame, area: Rect) {
    let arrow = Paragraph::new(Line::from(Span::styled(
        "▼",
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);

    frame.render_widget(arrow, area);
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
