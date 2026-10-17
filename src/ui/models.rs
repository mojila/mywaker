//! The model picker — browse the models exposed by OpenCode's providers and
//! assign one to the selected employee (persisted to team.toml and applied to
//! that employee's next session). The list is filterable by typing a query.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::App;

use super::layout;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [header_area, search_area, list_area] = layout::model_areas(area);

    let employee = app.team().employees().get(app.selected());
    let employee_name = employee.map(|e| e.name.as_str()).unwrap_or("employee");
    let assigned = employee.and_then(|e| e.model.as_deref());

    let header = Paragraph::new(Line::from(vec![
        Span::styled("model ", Color::Cyan),
        Span::styled(format!(" for {employee_name}"), Color::Gray),
        Span::styled(
            match assigned {
                Some(model) => format!("  (assigned: {model})"),
                None => "  (Enter to assign)".to_string(),
            },
            Color::DarkGray,
        ),
    ]));
    frame.render_widget(header, header_area);

    let matches = app.filtered_models().len();
    let total = app.models().len();
    let search = Paragraph::new(Line::from(vec![
        Span::styled(" search ", Color::Cyan),
        Span::styled(app.model_filter().to_string(), Color::Yellow),
        Span::styled("▏", Color::DarkGray),
        Span::styled(format!("  ({matches}/{total})"), Color::DarkGray),
    ]));
    frame.render_widget(search, search_area);

    if total == 0 {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            "no models — make sure OpenCode has providers configured".to_string(),
            Color::DarkGray,
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" models ".bold().cyan()),
        );
        frame.render_widget(empty, list_area);
        return;
    }

    let items: Vec<ListItem> = app
        .filtered_models()
        .iter()
        .map(|&index| {
            let model = &app.models()[index];
            let active = assigned == Some(model.id.as_str());
            let name = model.name.clone().unwrap_or_default();
            let provider = model.provider_id.clone().unwrap_or_else(|| model.vendor());
            let cost = model
                .input_cost()
                .map(|c| format!("${c:.2}/1M in"))
                .unwrap_or_else(|| "free".to_string());
            ListItem::new(Line::from(vec![
                if active {
                    Span::styled("✓ ", Color::Green)
                } else {
                    Span::raw("  ")
                },
                Span::styled(
                    model.id.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  [{provider}]"), Color::DarkGray),
                Span::styled(format!(" {name}"), Color::DarkGray),
                Span::styled(format!("  {cost}"), Color::DarkGray),
            ]))
        })
        .collect();

    if items.is_empty() {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            format!("no models match \"{}\"", app.model_filter()),
            Color::DarkGray,
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" models ".bold().cyan()),
        );
        frame.render_widget(empty, list_area);
        return;
    }

    let mut state = ListState::default();
    state.select(Some(app.model_selected()));
    // Scroll to the window hit-testing expects (see `layout::list_window`), so a
    // click lands on the row it points at.
    *state.offset_mut() = layout::list_window(list_area, items.len(), app.model_selected());

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" models ".bold().cyan()),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, list_area, &mut state);
}
