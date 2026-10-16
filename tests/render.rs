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
