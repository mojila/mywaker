//! The roster's prompt bar: the goal input on the main screen, addressed to the
//! selected employee.
//!
//! Two rows: a top border carrying who you are talking to (their name, status,
//! their model, and how many employees are mid-turn), and one content row
//! holding `> ` and the line editor. The editor itself is
//! [`super::input`], shared with the chat bar, so the two cursors behave the same.
//!
//! The bar is always drawn — an unfocused one has no caret — because it is how the
//! main screen tells the user a goal can be typed there at all.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;

use super::input;

/// The prefix on the content row, matching the chat input.
const PREFIX: &str = "> ";

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let focused = app.prompt_focused();
    let accent = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(accent));

    // The content row: the line editor, windowed to the bar's inner width (the
    // `> ` prefix is part of the body budget, as in chat).
    let body_width = area.width.saturating_sub(PREFIX.len() as u16 + 1) as usize;
    let scroll = input::scroll(
        app.input(),
        app.input_cursor(),
        body_width,
        app.input_scroll(),
    );
    let line = input::line(app.input(), app.input_cursor(), scroll, PREFIX, focused);

    frame.render_widget(
        Paragraph::new(line).block(block.clone().title(Line::from(left_title(app)))),
        area,
    );

    // The right end of the border: how much delegation is in flight. Drawn over
    // the border row, so it reads as part of the frame rather than as text in the
    // input.
    if let Some(badge) = right_badge(app) {
        let top = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(badge)).alignment(Alignment::Right),
            top,
        );
    }

    // While the selected employee is thinking, the band's last row says what
    // about. It uses the same indicator as the chat transcript (and the same
    // `[ui] thinking` setting), so the two screens cannot disagree about how much
    // is shown.
    if area.height > super::layout::PROMPT_HEIGHT
        && let Some(thinking) = app.prompt_employee_thinking_line(area.width as usize)
    {
        let row = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        frame.render_widget(Paragraph::new(thinking), row);
    }
}

/// The left half of the border: `to · Otto · idle · model`, graded so the name
/// survives a narrow bar.
fn left_title(app: &App) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(
            " to ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", app.prompt_employee_name().unwrap_or("—")),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(status) = app.prompt_employee_status() {
        spans.push(Span::styled("· ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            format!("{} ", status.label()),
            Style::default().fg(super::status::color(status)),
        ));
    }

    if let Some(model) = app.prompt_employee_model() {
        spans.push(Span::styled("· ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            format!("{model} "),
            Style::default().fg(Color::Cyan),
        ));
    }

    spans
}

/// The right half of the border: `◆ N active`, only while something is running, so
/// an idle team keeps the bar quiet.
fn right_badge(app: &App) -> Option<Vec<Span<'static>>> {
    let count = app.delegating_count();
    if count == 0 {
        return None;
    }
    Some(vec![
        Span::styled("◆ ", Style::default().fg(Color::Yellow)),
        Span::styled(
            format!("{count} active "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::DIM),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::message::Message;
    use crate::team::{EmployeeStatus, Team};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    fn app() -> App {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(4);
        App::new(Config::default(), tx)
    }

    /// Deliver a plain key through the app's own key path, so the tests exercise
    /// the focus routing rather than the editor directly.
    fn press(app: &mut App, code: KeyCode) {
        app.update(Message::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    /// Draw the bar on a throwaway terminal and return the buffer's symbols.
    fn bar(app: &App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 2)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, app, area);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    /// The border names the selected employee and shows its live status, so the
    /// main screen says who a typed goal is going to.
    #[test]
    fn the_bar_names_the_selected_employee_and_its_status() {
        let app = app();
        let text = bar(&app, 100);
        assert!(text.contains("to"), "{text}");
        // The cursor starts on the first card: the planner sorts first.
        assert!(text.contains("Dex"), "{text}");
        assert!(text.contains("idle"), "{text}");

        let mut team = Team::starter();
        team.set_status_by_id("planner", EmployeeStatus::Working);
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(4);
        let working = App::with_team(Config::default(), tx, team);
        assert!(bar(&working, 100).contains("working"));
    }

    /// The bar follows the roster cursor: moving the selection changes who the
    /// next goal is addressed to.
    #[test]
    fn the_bar_follows_the_selection() {
        let mut app = app();
        press(&mut app, KeyCode::Right);
        let text = bar(&app, 100);
        assert!(
            !text.contains("Dex"),
            "the bar still names the first card: {text}"
        );
        assert!(
            text.contains(app.prompt_employee_name().expect("a selection")),
            "{text}"
        );
    }

    /// The assigned model is shown when there is one, and omitted when there is
    /// not — the bar never prints a placeholder model.
    #[test]
    fn the_bar_shows_the_selected_model_when_assigned() {
        let app = app();
        assert!(!bar(&app, 100).contains("deepseek"), "no model yet");

        let mut team = Team::starter();
        team.employee_mut("planner").expect("planner").model = Some("deepseek-v4-pro".to_string());
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(4);
        let assigned = App::with_team(Config::default(), tx, team);
        assert!(bar(&assigned, 100).contains("deepseek-v4-pro"));
    }

    /// The input row is the shared line editor: prefix, text, caret.
    #[test]
    fn the_content_row_is_the_line_editor() {
        let mut app = app();
        press(&mut app, KeyCode::Char('i'));
        assert!(app.prompt_focused(), "`i` focuses the bar");
        type_text(&mut app, "ship the release");

        let text = bar(&app, 60);
        assert!(text.contains("> ship the release"), "{text}");
        assert!(text.contains(input::CURSOR), "a focused bar shows a caret");
    }

    /// An unfocused bar has no caret, so the main screen shows where the keyboard
    /// is not.
    #[test]
    fn an_unfocused_bar_has_no_caret() {
        let app = app();
        assert!(!bar(&app, 60).contains(input::CURSOR));
    }

    /// The badge counts the employees mid-turn, and disappears when the team is
    /// idle.
    #[test]
    fn the_badge_reports_work_in_flight() {
        let app = app();
        assert!(!bar(&app, 100).contains("active"), "idle team");

        // Two employees mid-turn: the badge says how much is running.
        let mut team = Team::starter();
        team.set_status_by_id("researcher", EmployeeStatus::Working);
        team.set_status_by_id("writer", EmployeeStatus::Thinking);
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(4);
        let delegating = App::with_team(Config::default(), tx, team);
        let text = bar(&delegating, 100);
        assert!(text.contains("◆ 2 active"), "{text}");
    }

    /// The prompt employee's own turn counts too: there is no root whose work is
    /// "delegation" and everyone else's "execution".
    #[test]
    fn the_prompt_employees_own_turn_is_counted() {
        let mut team = Team::starter();
        team.set_status_by_id("planner", EmployeeStatus::Working);
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(4);
        let app = App::with_team(Config::default(), tx, team);
        assert!(bar(&app, 100).contains("◆ 1 active"));
    }

    /// The bar survives a narrow terminal without panicking, and still shows the
    /// input row.
    #[test]
    fn the_bar_degrades_on_a_narrow_terminal() {
        let mut app = app();
        press(&mut app, KeyCode::Char('i'));
        type_text(&mut app, "abcdef");
        let text = bar(&app, 12);
        assert!(text.contains('>'), "{text}");
    }

    /// A zero-height band draws nothing (the roster's log keeps its rows on a
    /// short frame).
    #[test]
    fn a_zero_height_band_draws_nothing() {
        let app = app();
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, Rect::new(0, 0, 40, 0)))
            .unwrap();
    }
}
