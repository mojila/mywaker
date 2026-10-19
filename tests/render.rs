//! Render smoke tests against a terminal buffer (no TTY required).

use mywaker::app::App;
use mywaker::config::Config;
use mywaker::message::Message;
use mywaker::ui;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn app() -> App {
    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    App::new(Config::default(), tx)
}

/// Draw the whole app at `width`×`height` on a throwaway terminal and return the
/// buffer's symbols — one place builds the screen the assertions read.
fn screen(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn renders_connecting_state_without_panicking() {
    screen(&app(), 80, 24);
}

#[test]
fn renders_wide_terminal_without_panicking() {
    screen(&app(), 160, 50);
}

#[test]
fn renders_narrow_terminal_without_panicking() {
    // A 40-column pane is narrower than the roster's fixed side panels
    // (24-col sidebar + 32-col tasks), so the team column collapses to zero
    // width. Rendering must still degrade gracefully — no overflow, no panic —
    // with the tasks panel and sidebar names surviving intact, and the grid
    // drawing no cards into a panel that does not exist.
    let text = screen(&app(), 40, 24);

    assert!(
        text.contains("tasks"),
        "tasks panel missing at narrow width"
    );
    assert!(text.contains("Aria"), "roster name missing at narrow width");
    assert!(
        ui::grid_cells(&app(), (40, 24)).is_empty(),
        "a zero-width team panel has no cards to draw"
    );
}

/// The grid falls back to one full-width column once a 3-column card would be
/// narrower than 12 cells — and still renders without panicking.
#[test]
fn renders_one_column_grid_in_a_narrow_team_panel() {
    let text = screen(&app(), 80, 24);
    let cells = ui::grid_cells(&app(), (80, 24));
    let panel = ui::team_area((80, 24));

    assert!(!cells.is_empty(), "the starter roster has cards at 80×24");
    for (_, rect) in &cells {
        assert_eq!(rect.width, panel.width, "one column spans the panel");
    }
    assert!(text.contains("jev"), "the Jev header is still drawn");
}

#[test]
fn renders_nameplate_and_roster() {
    let text = screen(&app(), 120, 40);

    assert!(text.contains("MYWAKER"), "robot nameplate missing");
    assert!(text.contains("Aria"), "starter roster missing");
    assert!(text.contains("waker"), "robot title missing");
}

#[test]
fn renders_last_tasks_in_right_panel() {
    let mut team = mywaker::team::Team::starter();
    team.set_last_task(1, Some("draft the intro".to_string()));

    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    let app = App::with_team(Config::default(), tx, team);
    let text = screen(&app, 120, 40);

    assert!(text.contains("tasks"), "tasks panel title missing");
    assert!(text.contains("draft the intro"), "last task not rendered");
}
