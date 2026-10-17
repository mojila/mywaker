//! End-to-end watch hook: a real filesystem change flows through
//! [`mywaker::watcher::run`] as a [`Message::WatchedTask`].
//!
//! This exercises the real `notify` backend (no OpenCode service and no model
//! budget involved). Platform notifications can be slow to start, so the test
//! keeps touching the file until an event lands, within a generous deadline.

use std::time::Duration;

use mywaker::config::Watch;
use mywaker::message::Message;

#[tokio::test]
async fn a_file_change_dispatches_a_watched_task() {
    let dir = std::env::temp_dir().join(format!("mywaker-watch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create watch dir");

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let handle = tokio::spawn(mywaker::watcher::run(
        tx,
        vec![Watch {
            path: dir.clone(),
            employee: "reviewer".to_string(),
            goal: "review {path}".to_string(),
            recursive: true,
        }],
    ));

    // Touch the file until an event arrives, so a slow-to-arm backend can't make
    // the test flaky.
    let stopped = |deadline: tokio::time::Instant| tokio::time::Instant::now() >= deadline;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut message = None;
    while message.is_none() && !stopped(deadline) {
        std::fs::write(dir.join("change.rs"), "fn main() {}\n").expect("write file");
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(received)) => message = Some(received),
            Ok(None) => panic!("watcher channel closed before dispatching"),
            Err(_) => continue,
        }
    }

    handle.abort();
    let _ = std::fs::remove_dir_all(&dir);

    match message.expect("a watched task within 20s") {
        Message::WatchedTask { employee, goal } => {
            assert_eq!(employee, "reviewer");
            assert!(goal.starts_with("review "), "goal was {goal:?}");
            assert!(goal.contains("change.rs"), "goal was {goal:?}");
        }
        other => panic!("expected a watched task, got {other:?}"),
    }
}
