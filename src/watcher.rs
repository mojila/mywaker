//! Watch hooks: fire an autonomous task when files change.
//!
//! Each `[[watches]]` entry in the config arms a filesystem watcher on `path`.
//! Editor saves emit a burst of events, so changes are coalesced over a short
//! debounce window and then sent as a [`Message::WatchedTask`], which the app
//! turns into a persisted `Task` and dispatches to the named employee — the same
//! autonomous path the cron [`crate::scheduler`] uses.
//!
//! The goal is a template: `{path}` is replaced with the path that changed, so a
//! watch can hand the employee a concrete file to look at.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use notify::{Event, RecursiveMode, Watcher};
use tokio::sync::mpsc::{self, Sender, UnboundedReceiver};

use crate::config::Watch;
use crate::message::Message;

/// How long to wait after the last observed change before firing, so a burst of
/// editor events (write + rename + chmod) collapses into a single dispatch.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Coalesces a burst of filesystem events into a single dispatch: remembers the
/// most recent changed path and only reports it once [`DEBOUNCE`] has elapsed
/// with no further changes. Pure and clock-injected so it can be unit tested.
#[derive(Debug, Default)]
pub struct Coalescer {
    pending: Option<PathBuf>,
    deadline: Option<Instant>,
}

impl Coalescer {
    /// Record a change to `path`, (re)arming the debounce deadline from `now`.
    pub fn observe(&mut self, path: PathBuf, now: Instant) {
        self.pending = Some(path);
        self.deadline = Some(now + DEBOUNCE);
    }

    /// The path to dispatch once the debounce window has closed, if any. Clears
    /// the pending change so each burst fires exactly once.
    pub fn due(&mut self, now: Instant) -> Option<PathBuf> {
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.deadline = None;
                self.pending.take()
            }
            _ => None,
        }
    }

    /// When the next [`Self::due`] check is due, if a change is pending.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

/// Render a watch goal template, substituting `{path}` with the changed path.
pub fn render_goal(template: &str, path: &Path) -> String {
    template.replace("{path}", &path.display().to_string())
}

/// Expand a leading `~` to the user's home directory (best effort).
fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

/// Arm every watch hook and keep them alive for the app's lifetime. Returns
/// immediately when there is nothing to watch.
pub async fn run(tx: Sender<Message>, watches: Vec<Watch>) -> Result<()> {
    if watches.is_empty() {
        return Ok(());
    }

    // Watchers stop delivering once dropped, so hold them for the app's lifetime.
    let mut kept = Vec::new();
    let mut armed = 0usize;

    for watch in watches {
        if watch.path.as_os_str().is_empty() || watch.employee.is_empty() || watch.goal.is_empty() {
            tracing::warn!("skipping watch with empty path/employee/goal");
            continue;
        }

        let path = expand_tilde(&watch.path);
        let mode = if watch.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<notify::Result<Event>>();
        let mut watcher = match notify::recommended_watcher(move |event| {
            let _ = raw_tx.send(event);
        }) {
            Ok(watcher) => watcher,
            Err(err) => {
                tracing::warn!("cannot create watcher for {}: {err}", path.display());
                continue;
            }
        };

        if let Err(err) = watcher.watch(&path, mode) {
            tracing::warn!("cannot watch {}: {err}", path.display());
            continue;
        }

        let tx = tx.clone();
        let employee = watch.employee.clone();
        let goal = watch.goal.clone();
        tokio::spawn(async move {
            watch_one(raw_rx, tx, employee, goal).await;
        });

        kept.push(watcher);
        armed += 1;
        tracing::info!(
            "watching {} (recursive={})",
            path.display(),
            watch.recursive
        );
    }

    if armed == 0 {
        return Ok(());
    }

    tracing::info!("watch hooks armed ({armed} path(s))");

    // Keep the watchers (and their callback threads) alive for the app's lifetime.
    std::future::pending::<()>().await;
    Ok(())
}

/// Debounce one watch's raw event stream and forward coalesced changes to the app.
async fn watch_one(
    mut raw_rx: UnboundedReceiver<notify::Result<Event>>,
    tx: Sender<Message>,
    employee: String,
    goal_template: String,
) {
    let mut coalescer = Coalescer::default();

    loop {
        let event = if let Some(deadline) = coalescer.deadline() {
            let next = tokio::select! {
                event = raw_rx.recv() => Some(event),
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => None,
            };

            match next {
                Some(event) => event,
                // The debounce window closed: fire the accumulated change.
                None => {
                    if let Some(path) = coalescer.due(Instant::now()) {
                        let goal = render_goal(&goal_template, &path);
                        let message = Message::WatchedTask {
                            employee: employee.clone(),
                            goal,
                        };
                        if tx.send(message).await.is_err() {
                            return; // the app is gone; stop watching.
                        }
                    }
                    continue;
                }
            }
        } else {
            raw_rx.recv().await
        };

        match event {
            Some(Ok(event)) => observe(&event, &mut coalescer),
            Some(Err(err)) => tracing::warn!("watch error: {err}"),
            None => return, // the watcher was dropped.
        }
    }
}

/// Feed one raw event into the coalescer, ignoring pure reads (which would
/// otherwise make a watch fire whenever something merely opens a file).
fn observe(event: &Event, coalescer: &mut Coalescer) {
    if event.kind.is_access() {
        return;
    }
    if let Some(path) = event.paths.first() {
        coalescer.observe(path.clone(), Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, EventKind, ModifyKind};

    #[test]
    fn observe_ignores_pure_access_events() {
        let mut coalescer = Coalescer::default();

        // A pure read (open/close/execute) must not arm a dispatch.
        let access = Event::new(EventKind::Access(AccessKind::Any)).add_path(PathBuf::from("x.rs"));
        observe(&access, &mut coalescer);
        assert!(coalescer.deadline().is_none());

        // A modification does.
        let modify = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from("x.rs"));
        observe(&modify, &mut coalescer);
        assert_eq!(
            coalescer.due(Instant::now() + DEBOUNCE),
            Some(PathBuf::from("x.rs"))
        );
    }

    #[tokio::test]
    async fn watch_one_debounces_a_burst_then_forwards_a_watched_task() {
        let (raw_tx, raw_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = mpsc::channel(4);

        let handle = tokio::spawn(watch_one(
            raw_rx,
            tx,
            "reviewer".to_string(),
            "review {path}".to_string(),
        ));

        // Two changes inside the debounce window collapse into a single dispatch.
        for _ in 0..2 {
            let event =
                Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from("a.rs"));
            raw_tx.send(Ok(event)).expect("send event");
            tokio::time::sleep(DEBOUNCE / 4).await;
        }

        let message = tokio::time::timeout(DEBOUNCE * 4, rx.recv())
            .await
            .expect("a dispatch within the timeout")
            .expect("channel open");
        match message {
            Message::WatchedTask { employee, goal } => {
                assert_eq!(employee, "reviewer");
                assert_eq!(goal, "review a.rs");
            }
            other => panic!("expected a watched task, got {other:?}"),
        }

        // The watch task ends once its raw stream is dropped.
        drop(raw_tx);
        handle.await.expect("watch task joins");
    }

    #[test]
    fn coalesces_a_burst_into_one_dispatch() {
        let mut coalescer = Coalescer::default();
        let start = Instant::now();

        assert!(coalescer.due(start).is_none());

        // A burst of events within the debounce window keeps pushing the deadline.
        coalescer.observe(PathBuf::from("a.rs"), start);
        assert!(coalescer.due(start + DEBOUNCE / 2).is_none());
        coalescer.observe(PathBuf::from("b.rs"), start + DEBOUNCE / 2);
        assert!(coalescer.due(start + DEBOUNCE).is_none());
        assert_eq!(
            coalescer.due(start + DEBOUNCE * 2),
            Some(PathBuf::from("b.rs"))
        );

        // Once fired, nothing is left to dispatch.
        assert!(coalescer.due(start + DEBOUNCE * 3).is_none());
        assert!(coalescer.deadline().is_none());
    }

    #[test]
    fn renders_the_changed_path_into_the_goal() {
        let goal = render_goal("Review the change to {path}", Path::new("src/app.rs"));
        assert_eq!(goal, "Review the change to src/app.rs");

        // A template without a placeholder is passed through untouched.
        assert_eq!(
            render_goal("run the suite", Path::new("x")),
            "run the suite"
        );
    }

    #[test]
    fn expands_a_leading_tilde() {
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(
            expand_tilde(Path::new("~/Projects/x")),
            home.join("Projects/x")
        );
        assert_eq!(expand_tilde(Path::new("~/")), home);

        // Absolute and relative paths are left alone.
        assert_eq!(
            expand_tilde(Path::new("/etc/hosts")),
            PathBuf::from("/etc/hosts")
        );
        assert_eq!(
            expand_tilde(Path::new("src/app.rs")),
            PathBuf::from("src/app.rs")
        );
    }
}
