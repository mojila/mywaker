//! The model picker — browse the models exposed by OpenCode's providers and
//! assign one to the selected employee (persisted to team.toml and applied to
//! that employee's next session). The list is filterable by typing a query.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::App;

use super::{layout, theme};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [header_area, search_area, list_area] = layout::model_areas(area);

    let employee = app.team().employees().get(app.selected());
    let employee_name = employee.map(|e| e.name.as_str()).unwrap_or("employee");
    let assigned = employee.and_then(|e| e.model.as_deref());

    let header = Paragraph::new(Line::from(vec![
        Span::styled("model ", theme::ACCENT),
        Span::styled(format!(" for {employee_name}"), theme::TEXT),
        Span::styled(
            match assigned {
                Some(model) => format!("  (assigned: {model})"),
                None => "  (Enter to assign)".to_string(),
            },
            theme::MUTED,
        ),
    ]));
    frame.render_widget(header, header_area);

    let matches = app.filtered_models().len();
    let total = app.models().len();
    let search = Paragraph::new(Line::from(vec![
        Span::styled(" search ", theme::ACCENT),
        Span::styled(app.model_filter().to_string(), theme::WARN),
        Span::styled("▏", theme::MUTED),
        Span::styled(format!("  ({matches}/{total})"), theme::MUTED),
    ]));
    frame.render_widget(search, search_area);

    if total == 0 {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            "no models — make sure OpenCode has providers configured".to_string(),
            theme::MUTED,
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
                    Span::styled("✓ ", theme::OK)
                } else {
                    Span::raw("  ")
                },
                Span::styled(
                    model.id.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  [{provider}]"), theme::MUTED),
                Span::styled(format!(" {name}"), theme::MUTED),
                Span::styled(format!("  {cost}"), theme::MUTED),
            ]))
        })
        .collect();

    if items.is_empty() {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            format!("no models match \"{}\"", app.model_filter()),
            theme::MUTED,
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
            // K8: `selected_bg` **and** `selected_fg` (9.40:1). The old
            // `bg(DarkGray)` kept the item's own foreground, which fell to 3.18:1
            // where that foreground was `Gray`.
            Style::default()
                .bg(theme::SELECTED_BG)
                .fg(theme::SELECTED_FG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, list_area, &mut state);
}
