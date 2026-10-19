//! Live M7 smoke test: one goal fans out to the whole team in parallel, each
//! worker answers from its own role, and their outputs are aggregated into a
//! single deliverable handed back to the origin chat.
//!
//! This drives the *real* orchestration code — the same `App::update` loop the
//! TUI runs — against a running OpenCode service. It plays the part the SSE
//! stream and the render loop normally play: it drains the app's message
//! channel, polls each session's transcript for a completed turn, and feeds it
//! back as `Message::TranscriptLoaded` (what `session.execution.succeeded`
//! triggers in a live run).
//!
//! Requires a running `opencode service` and inference budget. Run explicitly:
//!
//!   cargo test --test live_fan_out -- --ignored --nocapture

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mywaker::app::{App, TranscriptRole};
use mywaker::config::Config;
use mywaker::message::Message;
use mywaker::opencode::{OpenCode, SessionMessage};
use mywaker::team::Team;
use mywaker::workflow::{is_final, parse_delegations};

/// The goal the source "employee" fanned out. Short, so every worker can answer
/// cheaply and the run stays well inside the deadline.
const GOAL: &str =
    "Write one short slogan (8 words max) for a terminal app that runs a team of AI employees.";

/// How long to wait for the whole fan-out (workers → aggregation → deliverable).
const DEADLINE: Duration = Duration::from_secs(300);

/// A guard against a runaway chain spawning unbounded turns.
const MAX_TURNS: usize = 40;

#[ignore = "requires a running OpenCode service + inference budget"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_goal_fans_out_in_parallel_and_aggregates() {
    // --- connect + roster -------------------------------------------------
    let client = OpenCode::connect()
        .await
        .expect("connect to a running OpenCode service (`opencode serve`)");
    let info = client.server_info().await.expect("server info");
    println!("connected to OpenCode v{}", info.version);

    let team = Team::load_or_starter();
    let ids: Vec<String> = team.employees().iter().map(|e| e.id.clone()).collect();
    assert!(
        ids.len() >= 3,
        "need at least 3 employees for a fan-out, roster is {ids:?}"
    );

    // The chat we open is the fan-out's source (roster selection starts at 0).
    // `fan_out` picks the aggregator as the first of reviewer/planner that
    // differs from the source; everyone else is a worker.
    let source = 0usize;
    let aggregator = ["reviewer", "planner"]
        .iter()
        .find_map(|id| ids.iter().position(|e| e == id).filter(|&i| i != source))
        .unwrap_or(source);
    let workers: Vec<usize> = (0..ids.len())
        .filter(|&i| i != source && i != aggregator)
        .collect();
    assert!(
        !workers.is_empty(),
        "fan-out has no workers: roster {ids:?}"
    );

    println!(
        "fan-out: source={} aggregator={} workers={:?}",
        ids[source],
        ids[aggregator],
        workers.iter().map(|&i| ids[i].clone()).collect::<Vec<_>>()
    );

    // --- start the app (no TUI; this test is the event loop) --------------
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(256);
    let mut app = App::with_team(Config::default(), tx, team);
    app.update(Message::Connected {
        client: client.clone(),
        info,
    });

    // Open the source employee's chat (Roster → Enter). This creates the chat
    // entry the transcript path needs; it starts an idle session that is never
    // prompted, so it costs nothing.
    app.update(Message::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));

    // --- trigger the fan-out, exactly as an assistant reply would ---------
    // The app only fans out on an *assistant* reply carrying `@parallel`; feed
    // that transcript so the real `dispatch_delegations` → `fan_out` path runs.
    let trigger: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id": "live_user", "time": {"created": 1}, "text": "team, take this", "type": "user"},
        {"id": "live_assistant", "time": {"created": 2, "completed": 3}, "type": "assistant",
         "content": [{"type": "text", "text": format!("Let's parallelize.\n@parallel {GOAL}")}],
         "finish": "done"}
    ]))
    .expect("build the trigger transcript");
    app.update(Message::TranscriptLoaded {
        employee: source,
        messages: trigger,
    });

    // --- drive the orchestration to completion ----------------------------
    let start = Instant::now();
    let mut sessions: HashMap<usize, String> = HashMap::new();
    let mut session_at: HashMap<usize, Instant> = HashMap::new();
    let mut injected: HashSet<(usize, String)> = HashSet::new();
    let mut turns = 0usize;
    // The deliverable the aggregator is expected to hand back (`@user …`).
    let mut expected: Option<String> = None;

    let deliverable = loop {
        // Drain everything the app (and its spawned tasks) produced.
        while let Ok(message) = rx.try_recv() {
            if let Message::SessionCreated {
                employee,
                result: Ok(session),
            } = &message
            {
                if !sessions.contains_key(employee) {
                    session_at.insert(*employee, Instant::now());
                    println!(
                        "  session for {} ({}) at +{:.1}s",
                        ids[*employee],
                        session.id,
                        start.elapsed().as_secs_f32()
                    );
                }
                sessions.insert(*employee, session.id.clone());
            }
            app.update(message);
        }

        // Feed back any session that has produced a *completed* assistant turn
        // (the job `session.execution.succeeded` does in a live run). Idempotent
        // by message id, so a repeated poll never double-dispatches.
        for (employee, session_id) in sessions.clone() {
            let Ok(messages) = client.messages(&session_id).await else {
                continue;
            };
            let Some(last) = messages
                .iter()
                .rev()
                .find(|m| m.role() == "assistant" && !m.is_incomplete())
            else {
                continue;
            };
            if !injected.insert((employee, last.id.clone())) {
                continue;
            }
            turns += 1;

            // The aggregator's reply is the M7 deliverable: it must end with a
            // line-leading final directive (`@user …`). Capture it, then confirm
            // the app routed that exact text into the origin chat.
            if employee == aggregator {
                expected = parse_delegations(&last.display_text())
                    .into_iter()
                    .find(|d| is_final(&d.target))
                    .map(|d| d.message);
                println!(
                    "\n  aggregator {} replied at +{:.1}s — final directive: {:?}",
                    ids[aggregator],
                    start.elapsed().as_secs_f32(),
                    expected
                );
            }

            app.update(Message::TranscriptLoaded { employee, messages });
        }

        // Success = the aggregator's deliverable arriving in the origin chat.
        // A worker's own early `@user` also lands there, but it bypasses
        // aggregation, so only *this* text closes the M7 check.
        if let Some(want) = &expected
            && app
                .transcript()
                .iter()
                .any(|l| l.role == TranscriptRole::Assistant && l.text.trim() == want.trim())
        {
            break want.clone();
        }

        if turns >= MAX_TURNS {
            panic!("stopped after {MAX_TURNS} turns without a deliverable");
        }
        if start.elapsed() > DEADLINE {
            panic!(
                "timed out after {:?} — sessions={:?}, expected={:?}, origin tail={:?}",
                start.elapsed(),
                sessions.keys().map(|i| ids[*i].clone()).collect::<Vec<_>>(),
                expected,
                app.transcript()
                    .iter()
                    .rev()
                    .take(6)
                    .map(|l| l.text.clone())
                    .collect::<Vec<_>>()
            );
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    };

    // --- assertions -------------------------------------------------------
    assert!(
        !deliverable.trim().is_empty(),
        "the aggregated deliverable is empty"
    );

    // Every worker was fanned out to (the fan-out dispatches them all before
    // any one finishes, so their sessions are created concurrently).
    for &w in &workers {
        assert!(
            session_at.contains_key(&w),
            "worker {} ({}) never received the goal",
            w,
            ids[w]
        );
    }
    // The aggregator only runs once every worker has contributed — its session
    // existing is proof the fan-out aggregated rather than stalling.
    assert!(
        session_at.contains_key(&aggregator),
        "aggregator {} ({}) never ran — the fan-out did not aggregate",
        aggregator,
        ids[aggregator]
    );

    println!(
        "\nM7 live fan-out PASSED in {:.1}s",
        start.elapsed().as_secs_f32()
    );
    println!(
        "  workers: {:?}",
        workers.iter().map(|&i| ids[i].clone()).collect::<Vec<_>>()
    );
    println!("  aggregator: {}", ids[aggregator]);
    println!("  deliverable: {deliverable}");
}
