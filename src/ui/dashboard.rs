//! The team dashboard — roster list with per-employee detail and status.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::app::App;
use crate::team::{Employee, EmployeeStatus};

/// Distinct marker colors, one per roster row (cycled by index).
const AVATAR_COLORS: [Color; 4] = [Color::Cyan, Color::Magenta, Color::Yellow, Color::Green];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .team()
        .employees()
        .iter()
        .enumerate()
        .map(|(index, employee)| ListItem::new(employee_line(index, employee)))
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected()));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" team ".bold().cyan()),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn employee_line(index: usize, employee: &Employee) -> Line<'static> {
    let color = AVATAR_COLORS[index % AVATAR_COLORS.len()];

    let mut spans: Vec<Span> = vec![
        Span::styled(format!("{} ", initial(&employee.name)), color),
        Span::styled(
            format!("{} ", employee.name),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(employee.role.clone(), Color::Gray),
        Span::styled(
            format!(" · {}", employee.model.as_deref().unwrap_or("default")),
            Color::Gray,
        ),
    ];
    if !employee.skills.is_empty() {
        spans.push(Span::styled(
            format!(" · #{}", employee.skills.join(",")),
            Color::DarkGray,
        ));
    }
    spans.push(Span::styled(
        format!(" · onboard {}", employee.onboarded),
        Color::DarkGray,
    ));
    if let Some(task) = &employee.current_task {
        spans.push(Span::styled(format!(" · {task}"), Color::DarkGray));
    }
    spans.push(Span::raw("  "));
    spans.push(status_span(employee.status));

    Line::from(spans)
}

fn initial(name: &str) -> char {
    name.chars().next().unwrap_or('?').to_ascii_uppercase()
}

fn status_span(status: EmployeeStatus) -> Span<'static> {
    match status {
        EmployeeStatus::Idle => "● idle".gray(),
        EmployeeStatus::Thinking => "◔ thinking".yellow(),
        EmployeeStatus::Working => "▶ working".cyan(),
        EmployeeStatus::Done => "✓ done".green(),
        EmployeeStatus::Error => "✕ error".red(),
    }
}
