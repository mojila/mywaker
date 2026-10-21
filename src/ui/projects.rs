//! The project picker — browse directories under `~/Projects` and adopt one as
//! the OpenCode session working directory.

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
    let [header_area, list_area] = layout::project_areas(area);

    let active_name = app.active_project().map(|p| p.name.as_str());
    let roots = app
        .project_roots()
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let header = Paragraph::new(Line::from(vec![
        Span::styled("projects ", theme::ACCENT),
        Span::styled(format!(" {}", roots), theme::TEXT),
        Span::styled(
            match active_name {
                Some(name) => format!("  (selected: {name})"),
                None => "  (Enter to select)".to_string(),
            },
            theme::MUTED,
        ),
    ]));
    frame.render_widget(header, header_area);

    let items: Vec<ListItem> = app
        .projects()
        .iter()
        .map(|project| {
            let active = active_name == Some(project.name.as_str());
            ListItem::new(Line::from(vec![
                if active {
                    Span::styled("✓ ", theme::OK)
                } else {
                    Span::raw("  ")
                },
                Span::styled(
                    project.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {}", project.path.display()), theme::MUTED),
            ]))
        })
        .collect();

    if items.is_empty() {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            "no projects — create a directory there and reopen with  p".to_string(),
            theme::MUTED,
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" projects ".bold().cyan()),
        );
        frame.render_widget(empty, list_area);
        return;
    }

    let mut state = ListState::default();
    state.select(Some(app.project_selected()));
    // Scroll to the window hit-testing expects (see `layout::list_window`), so a
    // click lands on the row it points at.
    *state.offset_mut() = layout::list_window(list_area, items.len(), app.project_selected());

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" projects ".bold().cyan()),
        )
        .highlight_style(
            // K8: `selected_bg` **and** `selected_fg` (9.40:1).
            Style::default()
                .bg(theme::SELECTED_BG)
                .fg(theme::SELECTED_FG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, list_area, &mut state);
}
