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

#[test]
fn renders_connecting_state_without_panicking() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = app();

    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
}

#[test]
fn renders_wide_terminal_without_panicking() {
    let backend = TestBackend::new(160, 50);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = app();

    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
}

#[test]
fn renders_narrow_terminal_without_panicking() {
    // A 40-column pane is narrower than the roster's fixed side panels
    // (24-col robot + 32-col tasks), so the pipeline column collapses to zero
    // width and the robot nameplate clips. Rendering must still degrade
    // gracefully — no overflow, no panic — with the tasks panel and roster
    // names surviving intact.
    let backend = TestBackend::new(40, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = app();

    terminal.draw(|frame| ui::render(frame, &app)).unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(
        text.contains("tasks"),
        "tasks panel missing at narrow width"
    );
    assert!(text.contains("Aria"), "roster name missing at narrow width");
}

#[test]
fn renders_nameplate_and_roster() {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = app();

    terminal.draw(|frame| ui::render(frame, &app)).unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

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

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(text.contains("tasks"), "tasks panel title missing");
    assert!(text.contains("draft the intro"), "last task not rendered");
}
