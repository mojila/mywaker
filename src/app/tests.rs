use super::*;

fn make_app() -> (App, tokio::sync::mpsc::Receiver<Message>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<Message>(16);
    (App::new(Config::default(), tx), rx)
}

/// Deliver one plain key press through the app's own key path.
fn press(app: &mut App, code: KeyCode) {
    app.update(Message::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// The roster index of `id`.
///
/// The roster is rooted at the orchestrator, so a positional literal in a fixture
/// names whichever employee happens to sit at that index — the bug this helper
/// exists to prevent. Fixtures that mean "the writer" ask for the writer.
fn employee(app: &App, id: &str) -> usize {
    app.team_index_by_id(id)
        .unwrap_or_else(|| panic!("no employee {id:?} on the roster"))
}

#[test]
fn task_summary_takes_first_line_and_truncates() {
    assert_eq!(task_summary("draft the intro"), "draft the intro");
    // Blank leading line skipped, internal whitespace collapsed.
    assert_eq!(
        task_summary("\n   research   the best   runtime  \nmore\n"),
        "research the best runtime"
    );
    // Long input truncates to 48 chars with an ellipsis.
    let long = "x".repeat(100);
    let summary = task_summary(&long);
    assert_eq!(summary.chars().count(), 48);
    assert!(summary.ends_with('…'));
    // Empty input yields an empty string.
    assert_eq!(task_summary("   \n  "), "");
}

fn event(event_type: &str, data: serde_json::Value) -> Event {
    serde_json::from_value(serde_json::json!({
        "id": "e", "type": event_type, "data": data,
    }))
    .unwrap()
}

fn open_chat(app: &mut App, employee: usize) {
    app.mode = Mode::Chat;
    app.chat_employee = Some(employee);
    app.chats.entry(employee).or_default().session_id = Some("ses_1".into());
}

/// The queued instructions in an outbox, joined for a substring assertion.
fn outbox_text(outbox: &[Outgoing]) -> String {
    outbox
        .iter()
        .map(Outgoing::text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Draw the whole app at `width`×`height` on a throwaway terminal and return the
/// buffer's symbols — one place builds the screen the render tests read.
fn screen(app: &App, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// The first prompt to an employee carries the team roster + delegation
/// protocol; subsequent prompts are verbatim.
#[test]
fn first_prompt_injects_team_context_once() {
    let (mut app, _rx) = make_app();
    app.chats.entry(0).or_default();

    let first = app.bootstrap_prompt(0, "hello".to_string());
    assert!(first.contains("hello"));
    assert!(first.contains("@writer"));
    assert!(first.contains("@user"));

    let second = app.bootstrap_prompt(0, "again".to_string());
    assert_eq!(second, "again");
}

/// A completed assistant reply carrying `@writer …` relays the instruction
/// to the writer and advances the hand-off counter.
#[test]
fn dispatch_delegations_routes_to_teammate() {
    let (mut app, _rx) = make_app();
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, orchestrator);
    // The writer already has a live session to receive into.
    let writer = employee(&app, "writer");
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"Here's the plan.\n@writer draft the doc"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(orchestrator, messages);

    assert_eq!(app.delegation_count, 1);
    assert!(
        app.chats[&writer]
            .transcript
            .iter()
            .any(|line| line.text.contains("draft the doc"))
    );
}

// --- delegation traces (the wires) ---

/// A hand-off records one outbound trace from the root to the worker it went to,
/// so the wires have something to draw.
#[test]
fn a_hand_off_records_a_trace_from_the_root() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    open_chat(&mut app, root);
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@writer draft the doc"}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(root, messages);

    assert_eq!(app.traces().len(), 1);
    let trace = app.traces()[0];
    assert_eq!(trace.from, root);
    assert_eq!(trace.to, writer);
    assert_eq!(trace.kind, TraceKind::Out);
    assert_eq!(trace.started, app.frame());
}

/// A worker's report records a trace back up to the root, in the report colour's
/// direction — the pair of the hand-off that started it.
#[test]
fn a_report_records_a_trace_back_to_the_root() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    open_chat(&mut app, writer);
    app.origin_of.insert(writer, writer);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"draft it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@user the draft is ready"}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(writer, messages);

    let trace = app
        .traces()
        .iter()
        .find(|trace| trace.kind == TraceKind::Report)
        .expect("the report is traced");
    assert_eq!(trace.from, writer);
    assert_eq!(trace.to, root);
}

/// A fan-out traces one broadcast per worker, in the fan-out colour.
#[test]
fn a_fan_out_records_a_broadcast_per_worker() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    open_chat(&mut app, planner);
    app.fan_out(planner, "ship it".to_string());

    let broadcast: Vec<DelegationTrace> = app
        .traces()
        .iter()
        .copied()
        .filter(|trace| trace.kind == TraceKind::Fanout)
        .collect();
    assert!(!broadcast.is_empty(), "a fan-out draws its briefs");
    for trace in &broadcast {
        assert_eq!(trace.from, planner);
        assert_ne!(trace.to, planner, "the source does not wire to itself");
    }
}

/// A hand-off to yourself is not a wire: nothing moved, so nothing is drawn.
#[test]
fn a_self_hand_off_records_no_trace() {
    let (mut app, _rx) = make_app();
    let writer = employee(&app, "writer");
    app.trace(writer, writer, TraceKind::Out);
    assert!(app.traces().is_empty());
}

/// Traces are capped, oldest dropped first, so a long chain cannot grow the list
/// without bound.
#[test]
fn traces_are_capped_at_the_limit() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");

    for _ in 0..App::TRACE_LIMIT + 5 {
        app.trace(root, writer, TraceKind::Out);
    }
    assert_eq!(app.traces().len(), App::TRACE_LIMIT);
}

/// A trace's packet advances with the frame and is pruned once it has run its
/// course — so the wires animate without the app keeping any history.
#[test]
fn traces_advance_and_expire() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    app.trace(root, writer, TraceKind::Out);

    let start = app.traces()[0];
    assert_eq!(
        start.progress(app.frame()),
        0.0,
        "a new packet is at the port"
    );

    app.advance();
    let midway = app.traces()[0];
    assert!(midway.progress(app.frame()) > 0.0);
    assert!(midway.progress(app.frame()) < 1.0);

    // Walk to the TTL: the packet has arrived, and the next tick forgets it.
    for _ in 0..DelegationTrace::TTL {
        app.advance();
    }
    assert!(app.traces().is_empty(), "an expired trace is pruned");
}

/// `progress` is a pure function of the trace and the frame, clamped to `1.0` so
/// a stale trace cannot overshoot its wire.
#[test]
fn trace_progress_is_pure_and_clamped() {
    let trace = DelegationTrace {
        from: 0,
        to: 1,
        kind: TraceKind::Out,
        started: 100,
    };

    assert_eq!(trace.progress(100), 0.0);
    assert_eq!(
        trace.progress(100),
        trace.progress(100),
        "same frame, same value"
    );
    assert_eq!(trace.progress(106), 0.5);
    assert_eq!(trace.progress(112), 1.0);
    assert_eq!(trace.progress(999), 1.0, "clamped, never past the end");

    assert!(!trace.is_expired(111));
    assert!(trace.is_expired(112));
}

// --- bubbling under its guards ---

/// The depth cap must not deliver. A worker at `MAX_CHAIN_DEPTH` that answers
/// `@user` has produced a *partial* result; handing it to the user is exactly the
/// failure the one-mouth rule exists to prevent, so it is logged and dropped —
/// never delivered.
#[test]
fn a_report_at_the_depth_cap_is_never_delivered_to_the_user() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    open_chat(&mut app, writer);
    app.origin_of.insert(writer, writer);
    app.depth_of.insert(writer, harness::MAX_CHAIN_DEPTH);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"draft it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@user the partial draft"}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(writer, messages);

    // The partial answer did not reach the user's chat…
    assert!(
        !app.chats[&writer].transcript.iter().any(|line| {
            line.role == TranscriptRole::Assistant && line.text == "the partial draft"
        }),
        "a capped report was delivered as the answer"
    );
    // …the guard said so…
    assert!(
        app.log
            .iter()
            .any(|line| line.contains("depth limit reached reporting")),
        "the drop must be logged: {:?}",
        app.log
    );
    // …and it became an abandon note, not a delivery.
    assert!(
        app.chats[&writer]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::Error && line.text.contains("no deliverable")),
        "{:?}",
        app.chats[&writer].transcript
    );
    // The root was not asked to synthesize a partial result.
    assert!(
        !app.chats.contains_key(&root) || app.chats[&root].outbox.is_empty(),
        "the capped report still reached the orchestrator"
    );
}

/// The run-wide delegation budget bounds bubbling too: a report spends it, and at
/// the limit the report is dropped rather than delivered.
#[test]
fn a_report_spends_the_delegation_budget() {
    let (mut app, _rx) = make_app();
    let writer = employee(&app, "writer");
    let root = employee(&app, "orchestrator");
    open_chat(&mut app, writer);
    app.origin_of.insert(writer, writer);

    let report = || -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id":"m1","time":{"created":1},"text":"draft it","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text":"@user the draft"}],"finish":"done"}
        ]))
        .unwrap()
    };

    app.handle_transcript(writer, report());
    assert_eq!(app.delegation_count, 1, "a report counts as a hand-off");
    assert!(
        !app.chats[&root].outbox.is_empty(),
        "the report reached the orchestrator"
    );

    // At the limit, a further report is refused. The budget is checked in
    // `dispatch_delegations`, before the directive is routed, so this also proves
    // the report path sits behind that guard rather than beside it.
    let (mut app, _rx) = make_app();
    let writer = employee(&app, "writer");
    let root = employee(&app, "orchestrator");
    open_chat(&mut app, writer);
    app.origin_of.insert(writer, writer);
    app.delegation_count = harness::DELEGATION_TOTAL_LIMIT;
    app.handle_transcript(writer, report());

    assert!(
        !app.chats[&writer]
            .transcript
            .iter()
            .any(|line| { line.role == TranscriptRole::Assistant && line.text == "the draft" }),
        "a report past the budget was delivered"
    );
    assert!(
        app.log
            .iter()
            .any(|line| line.contains("delegation limit reached")),
        "{:?}",
        app.log
    );
    assert!(
        !app.chats.contains_key(&root) || app.chats[&root].outbox.is_empty(),
        "a report past the budget still reached the orchestrator"
    );
}

/// §8.2: the root's finished turn reaches the main screen. A worker's plain
/// answer does not — it owes a report upward.
#[test]
fn the_orchestrators_plain_answer_is_logged() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    open_chat(&mut app, root);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"ship it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"Shipped the release."}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(root, messages);

    assert!(
        app.log
            .iter()
            .any(|line| line.contains("Shipped the release.")),
        "the root's answer is not on the main screen: {:?}",
        app.log
    );
}

/// A worker's plain answer is not logged as the deliverable: it has not reported
/// yet, so surfacing it would put a partial result on the main screen.
#[test]
fn a_workers_plain_answer_is_not_logged_as_the_deliverable() {
    let (mut app, _rx) = make_app();
    let writer = employee(&app, "writer");
    open_chat(&mut app, writer);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"draft it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"A draft, mid-way."}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(writer, messages);

    assert!(
        !app.log
            .iter()
            .any(|line| line.contains("A draft, mid-way.")),
        "a worker's partial answer reached the main screen: {:?}",
        app.log
    );
}

/// The root's turn that *delegates* is not logged as a finished answer: it has
/// handed work on, so the goal is not done.
#[test]
fn a_delegating_orchestrator_turn_is_not_logged_as_finished() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    open_chat(&mut app, root);
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"ship it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@writer draft the release notes"}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(root, messages);

    assert!(
        !app.log
            .iter()
            .any(|line| line.contains("draft the release notes")),
        "a delegating turn was logged as a finished answer: {:?}",
        app.log
    );
}

/// `fan_out`'s aggregator is never the source. A roofless roster whose source is
/// the reviewer falls through to the planner rather than aggregating through the
/// employee that issued the round.
#[test]
fn fan_out_never_aggregates_through_its_source() {
    // A roster with no orchestrator, so the fallbacks are the only candidates.
    // `App::with_team` re-roots every roster, so the root is taken back out
    // directly (the accessor the roster commands refuse is not involved).
    let mut team = Team::default();
    for (id, name, role) in [
        ("reviewer", "Cleo", "Reviewer"),
        ("planner", "Dex", "Planner"),
        ("writer", "Bruno", "Writer"),
    ] {
        team.add_employee(crate::team::Employee::new(id, name, role))
            .expect("unique ids");
    }
    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(8);
    let mut app = App::with_team(Config::default(), tx, team);
    // `with_team` roots the roster as an invariant and the roster commands refuse
    // to remove the root, so the flat roster this test needs is rebuilt from the
    // remaining employees.
    let flat: Vec<crate::team::Employee> = app
        .team()
        .employees()
        .iter()
        .filter(|employee| employee.id != crate::team::ORCHESTRATOR_ID)
        .cloned()
        .collect();
    let mut rebuilt = Team::default();
    for employee in flat {
        rebuilt.add_employee(employee).expect("unique ids");
    }
    *app.team_mut_for_test() = rebuilt;
    assert!(app.orchestrator_index().is_none(), "the roster is flat");

    let reviewer = app.team_index_by_id("reviewer").expect("reviewer");
    open_chat(&mut app, reviewer);
    app.fan_out(reviewer, "review the release".to_string());

    let (_, fanout) = app.fanouts.iter().next().expect("a round");
    assert_ne!(
        fanout.aggregator, reviewer,
        "the round aggregated through its own source"
    );
    assert!(
        !fanout.workers.contains(&fanout.aggregator),
        "the aggregator is also a worker"
    );
    // The planner is the first fallback that is not the source.
    assert_eq!(
        app.team.name_at(fanout.aggregator).as_deref(),
        Some("Dex"),
        "the fallback order is reviewer → planner"
    );
}

/// A `@jev` directive routes the question to the decision model and posts the
/// verdict back into the asking employee's chat (deterministic "yes" when no
/// Jev client is attached).
#[test]
fn dispatch_delegations_routes_question_to_jev() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"review","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@jev is this plan sound?"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(0, messages);

    assert_eq!(app.delegation_count, 1);
    assert!(
        app.chats[&0]
            .transcript
            .iter()
            .any(|line| line.text.contains("is this plan sound?"))
    );
    assert!(
        app.chats[&0]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::Assistant && line.text == "yes")
    );
}

/// A `@user`/`@deliver` directive hands the final answer back to the origin
/// employee's chat and clears the tracked origin.
#[test]
fn dispatch_delegations_delivers_final_answer() {
    let (mut app, _rx) = make_app();
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, orchestrator);
    app.origin_of.insert(orchestrator, orchestrator);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"done\n@user the final summary"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(orchestrator, messages);

    assert_eq!(app.delegation_count, 1);
    assert!(
        app.chats[&orchestrator]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::Assistant && line.text == "the final summary")
    );
}

/// A worker's `@user` is not the user's answer: it bubbles up to the
/// orchestrator as a named report, and the origin chat stays untouched.
#[test]
fn a_workers_final_answer_bubbles_up_to_the_orchestrator() {
    let (mut app, _rx) = make_app();
    let orchestrator = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    open_chat(&mut app, writer); // the writer was prompted directly
    app.origin_of.insert(writer, writer);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"draft it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@user the draft is ready"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(writer, messages);

    // The user's chat did not receive it as a delivered answer…
    assert!(
        !app.chats[&writer]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::Assistant && line.text == "the draft is ready"),
        "a worker's @user was delivered to the user"
    );
    // …the orchestrator did, attributed to the writer.
    let queued = outbox_text(&app.chats[&orchestrator].outbox);
    assert!(queued.contains("Report from Bruno"), "{queued}");
    assert!(queued.contains("the draft is ready"), "{queued}");
    assert!(
        app.log
            .iter()
            .any(|line| line.contains("reported to the orchestrator")),
        "the relay is logged: {:?}",
        app.log
    );
}

/// Re-loading the same transcript does not re-dispatch the same hand-off.
#[test]
fn dispatch_delegations_is_idempotent_per_message_id() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    let writer = employee(&app, "writer");
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"@writer draft"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(0, messages.clone());
    app.handle_transcript(0, messages);

    assert_eq!(app.delegation_count, 1);
}

/// `@parallel` fans a goal out to every teammate but the source and the
/// aggregator (the orchestrator), registering a fan-out group and dispatching
/// each.
#[test]
fn parallel_directive_fans_out_to_workers() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let researcher = employee(&app, "researcher");
    let writer = employee(&app, "writer");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner); // the planner is the source
    app.origin_of.insert(planner, planner);
    app.chats.entry(researcher).or_default().session_id = Some("ses_r".into());
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"ship it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"Let's parallelize.\n@parallel build the launch plan"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(planner, messages);

    // One fan-out; the orchestrator aggregates, and everyone else but the source
    // is a worker.
    assert_eq!(app.fanouts.len(), 1);
    let (_, fanout) = app.fanouts.iter().next().unwrap();
    let mut expected: Vec<usize> = (0..app.team.len())
        .filter(|&i| i != planner && i != orchestrator)
        .collect();
    expected.sort_unstable();
    assert_eq!(fanout.workers, expected);
    assert_eq!(fanout.aggregator, orchestrator);
    assert_eq!(fanout.goal, "build the launch plan");

    // The goal reached the workers…
    for worker in &expected {
        let queued = outbox_text(&app.chats[worker].outbox);
        assert!(
            queued.contains("build the launch plan"),
            "worker {worker} never received the goal: {queued:?}"
        );
    }
    // …but not the aggregator (the orchestrator), which only synthesises later.
    assert!(!app.chats.contains_key(&orchestrator));
}

/// Once every worker contributes, the fan-out aggregates their outputs and
/// hands the synthesis to the orchestrator.
#[test]
fn fan_out_aggregates_once_all_workers_finish() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let researcher = employee(&app, "researcher");
    let writer = employee(&app, "writer");
    let reviewer = employee(&app, "reviewer");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner);
    app.origin_of.insert(planner, planner);
    app.fan_out(planner, "ship it".to_string());

    let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
            {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text": text}],"finish":"done"}
        ]))
        .unwrap()
    };

    // Researcher finishes; the fan-out is not yet complete.
    app.handle_transcript(researcher, assistant("r", "Market research findings."));
    assert!(!app.fanouts.values().next().unwrap().aggregated);

    // Writer finishes; the reviewer is the last worker, so aggregation fires and
    // the orchestrator receives the synthesized prompt (queued, since it has no
    // live session yet).
    app.handle_transcript(writer, assistant("w", "Draft copy for the launch."));
    assert!(!app.fanouts.values().next().unwrap().aggregated);
    app.handle_transcript(reviewer, assistant("v", "Both parts check out."));
    let (_, fanout) = app.fanouts.iter().next().unwrap();
    assert!(fanout.aggregated);

    let outbox = &app.chats[&orchestrator].outbox;
    assert_eq!(outbox.len(), 1);
    assert!(outbox[0].text().contains("Market research findings"));
    assert!(outbox[0].text().contains("Draft copy for the launch"));
}

/// Workers are briefed as a parallel round, not handed the bare goal: the brief
/// asks for their own contribution and forbids handing off.
#[test]
fn fan_out_briefs_each_worker_not_to_hand_off() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner);
    app.fan_out(planner, "ship it".to_string());

    let workers: Vec<usize> = (0..app.team.len())
        .filter(|&i| i != planner && i != orchestrator)
        .collect();
    assert!(workers.len() >= 2, "the fixture needs more than one worker");
    for worker in workers {
        let queued = outbox_text(&app.chats[&worker].outbox);
        assert!(
            queued.contains("Parallel round"),
            "worker {worker}: {queued}"
        );
        assert!(queued.contains("GOAL: ship it"));
        assert!(queued.contains("no @user"));
    }
}

/// A worker's `@user` must not deliver a partial result to the user: the round
/// collects the text and the aggregator owns the final answer. The worker's
/// directives are logged, not dispatched.
#[test]
fn worker_final_directive_contributes_instead_of_delivering() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let writer = employee(&app, "writer");
    let researcher = employee(&app, "researcher");
    let reviewer = employee(&app, "reviewer");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner);
    app.origin_of.insert(planner, planner);
    app.fan_out(planner, "ship it".to_string());

    let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
            {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text": text}],"finish":"done"}
        ]))
        .unwrap()
    };

    // The writer answers with a final directive only — the failure mode seen
    // live. It must not reach the origin chat, and its payload must be the
    // contribution the aggregator receives.
    app.handle_transcript(writer, assistant("w", "@user **Wake your team.**"));
    assert!(
        !app.transcript()
            .iter()
            .any(|line| line.text.contains("Wake your team")),
        "a worker's @user leaked into the origin chat"
    );
    assert_eq!(
        app.delegation_count, 0,
        "a worker's directive was dispatched"
    );

    // The round is still waiting on the researcher and the reviewer.
    assert!(!app.fanouts.values().next().unwrap().aggregated);

    app.handle_transcript(researcher, assistant("r", "Market research findings."));
    app.handle_transcript(reviewer, assistant("v", "Both parts check out."));
    let outbox = &app.chats[&orchestrator].outbox;
    assert_eq!(outbox.len(), 1);
    assert!(outbox[0].text().contains("Market research findings"));
    assert!(
        outbox[0].text().contains("**Wake your team.**"),
        "the directive-only contribution was lost: {}",
        outbox[0].text()
    );
}

/// A worker's `@<teammate>` does not spawn a side-chain during a round; the
/// target never receives the instruction, and the round still aggregates.
#[test]
fn worker_delegation_does_not_spawn_a_side_chain() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let writer = employee(&app, "writer");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner);
    app.fan_out(planner, "ship it".to_string());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"ship it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"Copy is prose.\n@writer write it"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(writer, messages);

    assert_eq!(app.delegation_count, 0, "worker hand-off was dispatched");
    // Nothing was relayed to anyone: the only outbox entries are the fan-out
    // briefs every worker received, and no new one appeared.
    for worker in (0..app.team.len()).filter(|&i| i != planner && i != orchestrator) {
        assert_eq!(
            app.chats[&worker].outbox.len(),
            1,
            "worker {worker} received an extra hand-off: {:?}",
            app.chats[&worker].outbox
        );
    }
    // The round recorded its text as a contribution.
    let fanout = app.fanouts.values().next().unwrap();
    assert_eq!(fanout.outputs[&writer], "Copy is prose.");
}

/// Once a round is over, the same employee is an ordinary delegator again.
#[test]
fn a_finished_worker_may_delegate_again() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let writer = employee(&app, "writer");
    let researcher = employee(&app, "researcher");
    let reviewer = employee(&app, "reviewer");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner);
    app.fan_out(planner, "ship it".to_string());

    // Every worker contributes, so the round aggregates and stops awaiting them.
    let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
            {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text": text}],"finish":"done"}
        ]))
        .unwrap()
    };
    app.handle_transcript(researcher, assistant("r", "Findings."));
    app.handle_transcript(writer, assistant("w", "Copy."));
    app.handle_transcript(reviewer, assistant("v", "Checked."));
    assert!(app.fanouts.values().next().unwrap().aggregated);
    assert!(
        app.chats.contains_key(&orchestrator),
        "the round aggregated to the orchestrator"
    );

    // A later reply from that employee delegates normally again.
    app.handle_transcript(
        writer,
        assistant("w2", "Next task.\n@researcher gather more sources"),
    );
    assert_eq!(app.delegation_count, 1);
    assert!(!app.chats[&researcher].outbox.is_empty());
}

/// A file watch fires the same autonomous path a cron schedule does: a task is
/// recorded for the named employee, the roster reflects it, and the log names
/// the watch hook as the trigger.
#[test]
fn watched_task_records_a_task_for_the_employee() {
    let (mut app, _rx) = make_app();

    app.update(Message::WatchedTask {
        employee: "researcher".into(),
        goal: "Review the change to src/app.rs".into(),
    });

    let task = app.store.tasks.last().expect("a task was recorded");
    assert_eq!(task.assigned_to.as_deref(), Some("researcher"));
    assert_eq!(task.goal, "Review the change to src/app.rs");
    assert_eq!(task.status, TaskStatus::Running);

    let index = app.team_index_by_id("researcher").expect("known employee");
    assert_eq!(app.team.employees()[index].status, EmployeeStatus::Thinking);
    assert_eq!(
        app.team.employees()[index].last_task.as_deref(),
        Some("Review the change to src/app.rs")
    );
    assert!(app.log.iter().any(|line| line.contains("👁 watched task")));
}

#[test]
fn streams_deltas_in_order_and_ignores_other_sessions() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.handle_stream_event(&event(
        "session.text.delta",
        serde_json::json!({"sessionID":"ses_1","delta":"Hel"}),
    ));
    app.handle_stream_event(&event(
        "message.part.delta",
        serde_json::json!({"sessionID":"ses_1","delta":"lo"}),
    ));
    assert_eq!(app.streaming_text(), "Hello");

    // A delta for a different session is ignored.
    app.handle_stream_event(&event(
        "session.text.delta",
        serde_json::json!({"sessionID":"ses_9","delta":"x"}),
    ));
    assert_eq!(app.streaming_text(), "Hello");
}

#[test]
fn streams_to_owning_employee_even_when_not_viewed() {
    let (mut app, _rx) = make_app();
    // The user is viewing employee 0; employee 1 has its own session.
    open_chat(&mut app, 0);
    app.chats.entry(1).or_default().session_id = Some("ses_2".into());

    app.handle_stream_event(&event(
        "session.text.delta",
        serde_json::json!({"sessionID":"ses_2","delta":"hi from 1"}),
    ));

    // The viewed employee 0 is untouched; employee 1's chat got the delta.
    assert_eq!(app.streaming_text(), "");
    assert_eq!(app.chats[&1].streaming_text, "hi from 1");

    // Status transitions also route to the owning employee, not the viewer.
    app.handle_stream_event(&event(
        "session.execution.started",
        serde_json::json!({"sessionID":"ses_2"}),
    ));
    assert_eq!(app.team.employees()[1].status, EmployeeStatus::Working);
    assert_eq!(app.team.employees()[0].status, EmployeeStatus::Idle);
}

#[test]
fn slash_menu_filters_navigates_and_completes() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    // Typing '/' opens the menu with all seven commands.
    app.handle_chat_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let (options, selected) = app.command_options().expect("menu open");
    assert_eq!(options.len(), 7);
    assert_eq!(options[selected].command, "/help");

    // Typing 's' narrows to the four '/s*' commands.
    app.handle_chat_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let (options, selected) = app.command_options().expect("menu open");
    assert_eq!(options.len(), 4);
    assert_eq!(selected, 0);
    assert_eq!(options[0].command, "/stop");

    // Four downs wrap back to the first option.
    for _ in 0..4 {
        app.handle_chat_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let (_, selected) = app.command_options().unwrap();
    assert_eq!(selected, 0);

    // Enter inserts the highlighted command and closes the menu.
    app.handle_chat_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.input, "/stop");
    assert!(app.command_options().is_none());
    assert!(app.command_menu.is_none());
}

#[test]
fn slash_command_completion_appends_arg_space() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    for c in "/skill add".chars() {
        app.handle_chat_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    // input "/skill add" matches only itself.
    let (options, selected) = app.command_options().unwrap();
    assert_eq!(options.len(), 1);
    assert_eq!(options[selected].command, "/skill add");

    // Completing an arg-taking command appends a trailing space for the arg.
    app.handle_chat_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.input, "/skill add ");
    assert!(app.command_menu.is_none());
}

#[test]
fn cursor_editing_inserts_and_deletes_at_position() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    for c in ['a', 'b', 'c'] {
        app.input_insert(c);
    }
    assert_eq!(app.input, "abc");
    assert_eq!(app.input_cursor(), 3);

    app.cursor_home();
    app.input_insert('X');
    assert_eq!(app.input, "Xabc");
    assert_eq!(app.input_cursor(), 1);

    app.cursor_right(); // now between 'a' and 'b'
    app.input_delete_forward();
    assert_eq!(app.input, "Xac");

    app.input_delete_backward(); // removes 'a'
    assert_eq!(app.input, "Xc");
    assert_eq!(app.input_cursor(), 1);
}

#[test]
fn history_recall_cycles_and_restores_draft() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.input = "first".into();
    app.send_prompt();
    app.input = "second".into();
    app.send_prompt();
    assert_eq!(app.chats[&0].history, vec!["first", "second"]);

    app.history_up();
    assert_eq!(app.input, "second");
    app.history_up();
    assert_eq!(app.input, "first");
    app.history_up(); // clamped at oldest
    assert_eq!(app.input, "first");
    app.history_down();
    assert_eq!(app.input, "second");
    app.history_down(); // past newest → restores the (empty) draft
    assert_eq!(app.input, "");
}

#[test]
fn scroll_transcript_moves_from_and_back_to_tail() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    assert_eq!(app.chat_scroll(), 0);

    app.scroll_transcript(-1); // page up → scroll up from the tail
    assert!(app.chat_scroll() > 0);

    app.scroll_transcript(1); // page down → back to the live tail
    assert_eq!(app.chat_scroll(), 0);
}

#[test]
fn interrupt_clears_streaming_and_notes() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.chats.get_mut(&0).unwrap().streaming_text = "partial".into();
    app.chats.get_mut(&0).unwrap().thinking.push("think");

    app.interrupt_current();

    let chat = &app.chats[&0];
    assert!(chat.streaming_text.is_empty());
    assert!(chat.thinking.is_empty());
    assert!(
        chat.transcript
            .iter()
            .any(|l| l.text.contains("interrupted"))
    );
}

#[test]
fn new_command_resets_session_binding() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    assert_eq!(app.chats[&0].session_id.as_deref(), Some("ses_1"));

    app.input = "/new".into();
    app.send_prompt();

    assert!(app.chats[&0].session_id.is_none());
    assert!(app.chats[&0].creating);
}

#[test]
fn reasoning_deltas_accumulate() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.handle_stream_event(&event(
        "session.reasoning.delta",
        serde_json::json!({"sessionID":"ses_1","delta":"thin"}),
    ));
    app.handle_stream_event(&event(
        "session.reasoning.delta",
        serde_json::json!({"sessionID":"ses_1","delta":"king"}),
    ));

    assert_eq!(app.thinking_text(), "thinking");
    assert_eq!(app.thinking_tail(), "thinking");
}

// --- thinking (the deliberation buffer and its indicator) ---

/// Appending a fragment is O(1): the buffer stores fragments, not one growing
/// string. The visible tail is still the newest text, in order.
#[test]
fn thinking_accumulates_fragments_in_order() {
    let mut thinking = Thinking::default();
    for fragment in ["Let me ", "check the ", "roster."] {
        thinking.push(fragment);
    }

    assert_eq!(thinking.text(), "Let me check the roster.");
    assert_eq!(thinking.tail(), "Let me check the roster.");
    assert!(!thinking.is_empty());
}

/// A new assistant message starts a fresh buffer: reasoning from one message is
/// not glued onto the next one's.
#[test]
fn a_new_message_key_starts_a_fresh_buffer() {
    let mut thinking = Thinking::default();
    thinking.begin(Some("msg_1".to_string()));
    thinking.push("first message's reasoning");
    assert_eq!(thinking.text(), "first message's reasoning");

    thinking.begin(Some("msg_2".to_string()));
    assert!(thinking.is_empty(), "a new message starts clean");
    thinking.push("second");
    assert_eq!(thinking.text(), "second");

    // Re-beginning the *same* key keeps what was accumulated (a resumed stream).
    thinking.begin(Some("msg_2".to_string()));
    assert_eq!(thinking.text(), "second");
}

/// The tail is bounded: a long deliberation keeps only the newest
/// [`Thinking::TAIL_BUDGET`] columns, so the indicator cannot grow with the turn.
#[test]
fn the_thinking_tail_is_bounded() {
    let mut thinking = Thinking::default();
    for _ in 0..200 {
        thinking.push("a fairly long fragment of deliberation ");
    }
    let tail = thinking.tail();
    assert!(
        tail.chars().count() <= Thinking::TAIL_BUDGET,
        "tail is {} columns",
        tail.chars().count()
    );
    // It is the *newest* text: the last fragment is in there, the first is not.
    assert!(tail.ends_with("deliberation"));
    assert!(thinking.text().len() > tail.len(), "the buffer keeps more");
}

/// The buffer is capped too: a turn that reasons for a very long time drops the
/// oldest fragments rather than growing without bound.
#[test]
fn the_thinking_buffer_is_capped() {
    let mut thinking = Thinking::default();
    for i in 0..Thinking::CAPACITY + 50 {
        thinking.push(&format!("f{i} "));
    }
    assert_eq!(thinking.fragments.len(), Thinking::CAPACITY);
    assert!(
        thinking
            .text()
            .contains(&format!("f{}", Thinking::CAPACITY + 49))
    );
    assert!(
        !thinking.text().contains("f0 "),
        "the oldest fragment is gone"
    );
}

/// The tail is one line: the provider's own line breaks and runs of whitespace
/// collapse, because the indicator is a single row.
#[test]
fn the_thinking_tail_is_a_single_line() {
    let mut thinking = Thinking::default();
    thinking.push("first line\n\n  second   line\tthird");
    let tail = thinking.tail();
    assert!(!tail.contains('\n'));
    assert_eq!(tail, "first line second line third");
}

/// `display_text` skips reasoning: a deliberation is not the answer, so it must
/// not reach a fan-out aggregate or a `@user` deliverable. `reasoning_text`
/// exposes it separately.
#[test]
fn display_text_skips_reasoning_parts() {
    let message: crate::opencode::SessionMessage = serde_json::from_value(serde_json::json!({
        "id": "m1",
        "type": "assistant",
        "content": [
            {"type": "reasoning", "text": "the model's private scratchpad"},
            {"type": "text", "text": "the actual answer"},
            {"type": "reasoning", "text": "more deliberation"}
        ],
        "finish": "done"
    }))
    .unwrap();

    let display = message.display_text();
    assert_eq!(display, "the actual answer");
    assert!(!display.contains("scratchpad"), "{display}");

    // The reasoning is available, just not as the answer.
    assert_eq!(
        message.reasoning_text(),
        "the model's private scratchpad\nmore deliberation"
    );
}

/// The transcript never shows reasoning as an assistant line: it is not a
/// message, it is the pinned indicator.
#[test]
fn reasoning_does_not_enter_the_transcript() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"go","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"reasoning","text":"hmm, let me think"},
                    {"type":"text","text":"done"}],"finish":"done"}
    ]))
    .unwrap();
    app.handle_transcript(0, messages);

    let transcript = app.transcript();
    assert!(
        !transcript
            .iter()
            .any(|line| line.text.contains("let me think")),
        "reasoning leaked into the transcript: {transcript:?}"
    );
    assert!(transcript.iter().any(|line| line.text == "done"));
}

/// The indicator is pinned to the chat's last row and survives scrolling back
/// through history: it says the employee is busy *now*.
#[test]
fn the_thinking_indicator_is_pinned_below_the_scrollback() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.team.set_status(0, EmployeeStatus::Thinking);
    app.chats
        .get_mut(&0)
        .unwrap()
        .thinking
        .push("pondering the goal");

    let text = screen(&app, 120, 30);
    assert!(text.contains("thinking"), "{text}");
    assert!(text.contains("pondering the goal"), "{text}");

    // Paging back through history does not scroll the indicator away.
    app.scroll_transcript(-1);
    let scrolled = screen(&app, 120, 30);
    assert!(scrolled.contains("thinking"), "{scrolled}");
}

/// The indicator disappears the moment the employee stops thinking.
#[test]
fn the_indicator_disappears_when_the_turn_ends() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.team.set_status(0, EmployeeStatus::Thinking);
    app.chats.get_mut(&0).unwrap().thinking.push("brief");
    assert!(screen(&app, 120, 30).contains("thinking"));

    app.team.set_status(0, EmployeeStatus::Working);
    assert!(
        !screen(&app, 120, 30).contains("thinking"),
        "a working employee is not thinking"
    );
}

/// The delivery policy reaches the wire: a user prompt steers, a background
/// hand-off queues. This is the end-to-end claim the `harness::delivery_for`
/// table makes, verified against the request the client actually sends.
#[tokio::test]
async fn user_prompts_steer_and_hand_offs_queue() {
    let (client, server) = recorder::start().await;

    let (mut app, _rx) = make_app();
    app.client = Some(client);

    // A user prompt to the writer.
    let writer = employee(&app, "writer");
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());
    open_chat(&mut app, writer);
    app.input = "draft the intro".into();
    app.send_prompt();
    recorder::wait_for_prompts(&server, 1).await;

    // A delegated hand-off to the same employee. The source does not matter for
    // the policy — only the kind does — so the root is not looked up here.
    app.send_to_employee(
        writer,
        "polish the intro".to_string(),
        harness::PromptKind::Delegation,
    );
    recorder::wait_for_prompts(&server, 2).await;

    let deliveries = server.deliveries.lock().unwrap().clone();
    let ids = server.admission_ids.lock().unwrap().clone();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(
        deliveries[0].as_deref(),
        Some("steer"),
        "the user's own prompt must take over"
    );
    assert_eq!(
        deliveries[1].as_deref(),
        Some("queue"),
        "a hand-off must wait for the turn in flight"
    );

    // Both carry an admission id, and they differ: different deliveries.
    assert!(ids[0].is_some(), "{ids:?}");
    assert!(ids[1].is_some(), "{ids:?}");
    assert_ne!(ids[0], ids[1], "two deliveries shared one admission id");
    assert!(
        ids[1].as_deref().unwrap_or("").contains("delegation"),
        "{ids:?}"
    );
}

/// A queued delivery that has not started executing is never treated as silent:
/// the retry clock starts at `session.execution.started`, so a prompt waiting
/// behind the previous turn is not "retried" into a duplicate admission.
#[tokio::test]
async fn a_queued_delivery_is_not_silent_before_its_execution_starts() {
    let (mut app, _rx) = make_app();
    // A client is needed for the outbox to flush and the delivery to be tracked;
    // the test only reads the model, so the endpoint is never reached.
    app.client = Some(crate::opencode::OpenCode::with_base(
        "http://127.0.0.1:1".parse().expect("a literal URL parses"),
        "sekrit",
    ));
    let writer = employee(&app, "writer");
    app.chats.entry(writer).or_default().session_id = Some("ses_w".into());
    app.send_to_employee(
        writer,
        "queued work".to_string(),
        harness::PromptKind::Delegation,
    );

    // Admitted, and then a very long time passes with no execution.
    let chat = app
        .chats
        .get_mut(&writer)
        .expect("the hand-off was tracked");
    let pending = chat.pending.as_mut().expect("a pending delivery");
    assert!(!pending.armed, "a fresh delivery is admitted, not armed");
    pending.since = Instant::now() - harness::HANDOFF_TIMEOUT - std::time::Duration::from_secs(5);

    app.tick_reliability();
    let chat = &app.chats[&writer];
    assert_eq!(
        chat.pending.as_ref().map(|p| p.attempts),
        Some(0),
        "a waiting delivery was retried"
    );
    assert!(
        !chat.transcript.iter().any(|line| line.text.contains("🔁")),
        "a waiting delivery was called silent"
    );

    // Once its execution starts, the clock runs — and then silence is real.
    app.handle_stream_event(&event(
        "session.execution.started",
        serde_json::json!({"sessionID":"ses_w"}),
    ));
    let chat = app.chats.get_mut(&writer).expect("chat");
    let pending = chat.pending.as_mut().expect("still pending");
    assert!(pending.armed, "execution start arms the clock");
    pending.since = Instant::now() - harness::HANDOFF_TIMEOUT - std::time::Duration::from_secs(5);

    app.tick_reliability();
    assert_eq!(
        app.chats[&writer].pending.as_ref().map(|p| p.attempts),
        Some(1),
        "a genuinely silent started turn is retried"
    );
}

#[test]
fn new_clears_the_bootstrap_so_the_fresh_session_is_briefed() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.chats.get_mut(&0).unwrap().bootstrapped = true;

    app.input = "/new".into();
    app.send_prompt();

    assert!(
        !app.chats[&0].bootstrapped,
        "the fresh session would never receive team_context"
    );

    // And the next prompt carries the preamble again.
    let prompt = app.bootstrap_prompt(0, "next task".to_string());
    assert!(prompt.contains("You are"), "{prompt}");
    assert!(prompt.contains("@orchestrator"), "{prompt}");
    assert!(prompt.ends_with("next task"), "{prompt}");
}

/// A slash command typed in the main prompt bar is handled locally, exactly as it
/// is in a chat — it must never be sent to the orchestrator as a goal.
#[test]
fn slash_commands_in_the_main_prompt_are_not_sent_as_goals() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");

    press(&mut app, KeyCode::Char('i'));
    for c in "/help".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);

    // No goal was queued or dispatched…
    assert!(
        !app.chats.contains_key(&root) || app.chats[&root].queue_running.is_empty(),
        "a slash command was sent as a goal"
    );
    // …and the command ran: its table is on the main screen's log, where the user
    // typed it.
    assert!(
        app.log.iter().any(|line| line.contains("/parallel")),
        "the command's output is not visible from the roster: {:?}",
        app.log
    );
    // A command is not a goal, so it does not enter the goal history.
    assert!(app.prompt_history.is_empty(), "{:?}", app.prompt_history);
}

/// An unknown command-shaped input is still a goal: only the real commands are
/// intercepted, so a goal that happens to start with `/` is not silently eaten.
#[test]
fn an_unknown_slash_input_is_still_a_goal() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");

    press(&mut app, KeyCode::Char('i'));
    for c in "/summarize the repo".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);

    assert!(
        app.chats[&root]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::User && line.text == "/summarize the repo"),
        "an unrecognised `/` goal was dropped"
    );
}

#[test]
fn the_thinking_mode_controls_how_much_is_shown() {
    let mut chat_thinking = Thinking::default();
    for _ in 0..40 {
        chat_thinking.push("a long stretch of deliberation ");
    }

    // Indicator (the default): one bounded line.
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.team.set_status(0, EmployeeStatus::Thinking);
    app.chats.get_mut(&0).unwrap().thinking = chat_thinking.clone();
    assert!(screen(&app, 120, 30).contains("thinking"));

    // Hidden: no indicator row at all. The chat *title* still says `thinking` —
    // hidden mode hides the deliberation, not the fact that the employee is busy.
    let (mut hidden, _rx) = make_app();
    hidden.config.ui.thinking = crate::config::ThinkingMode::Hidden;
    open_chat(&mut hidden, 0);
    hidden.team.set_status(0, EmployeeStatus::Thinking);
    hidden.chats.get_mut(&0).unwrap().thinking = chat_thinking.clone();
    let hidden_frame = screen(&hidden, 120, 30);
    assert!(
        !hidden_frame.contains("a long stretch"),
        "hidden mode draws no deliberation: {hidden_frame}"
    );
    assert!(
        hidden.thinking_line(120).is_none(),
        "hidden mode yields no indicator row"
    );

    // Full: the deliberation is shown, not just its tail.
    let (mut full, _rx) = make_app();
    full.config.ui.thinking = crate::config::ThinkingMode::Full;
    open_chat(&mut full, 0);
    full.team.set_status(0, EmployeeStatus::Thinking);
    full.chats.get_mut(&0).unwrap().thinking = chat_thinking;
    let line = full.thinking_line(200).expect("full mode draws a row");
    let drawn: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        drawn.chars().count() > Thinking::TAIL_BUDGET,
        "full mode is longer than the tail budget"
    );
}

/// The prompt band grows a row for the root's indicator, and the panels keep
/// their resting geometry — the row comes from the log.
#[test]
fn the_prompt_band_grows_for_the_roots_thinking() {
    let (mut app, _rx) = make_app();
    app.update(Message::Resize(120, 40));
    let panels = ui::team_area((120, 40));
    let resting = ui::prompt_area((120, 40)).height;

    let root = employee(&app, "orchestrator");
    app.team.set_status(root, EmployeeStatus::Thinking);
    app.chats.entry(root).or_default().thinking.push("planning");

    let text = screen(&app, 120, 40);
    assert!(
        text.contains("thinking"),
        "the bar shows the root's thinking"
    );
    assert_eq!(
        ui::team_area((120, 40)),
        panels,
        "the panels never move for the indicator"
    );

    // The taller band is what the renderer used: the resting height plus the
    // indicator row.
    let taller = crate::ui::layout::Regions::of(
        ratatui::layout::Rect::new(0, 0, 120, 40),
        Mode::Roster,
        true,
    );
    assert_eq!(taller.prompt.height, resting + 1);
    assert_eq!(
        taller.main,
        crate::ui::layout::Regions::of(
            ratatui::layout::Rect::new(0, 0, 120, 40),
            Mode::Roster,
            false
        )
        .main,
        "the taller band still leaves main alone"
    );
}

#[test]
fn status_transitions_on_lifecycle_events() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 1);

    app.handle_stream_event(&event(
        "session.execution.started",
        serde_json::json!({"sessionID":"ses_1"}),
    ));
    assert_eq!(app.team.employees()[1].status, EmployeeStatus::Working);

    app.handle_stream_event(&event(
        "session.reasoning.started",
        serde_json::json!({"sessionID":"ses_1"}),
    ));
    assert_eq!(app.team.employees()[1].status, EmployeeStatus::Thinking);

    // Failure marks the employee errored (transcript load short-circuits:
    // no client is attached in this unit test).
    app.handle_stream_event(&event(
        "session.execution.failed",
        serde_json::json!({"sessionID":"ses_1","error":{"message":"boom"}}),
    ));
    assert_eq!(app.team.employees()[1].status, EmployeeStatus::Error);
    assert!(app.log.iter().any(|line| line.contains("boom")));
}

#[test]
fn transcript_loading_derives_final_status() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 2);

    let response: crate::opencode::models::MessagesResponse =
        serde_json::from_value(serde_json::json!({"data":[
            {"id":"m1","time":{"created":1},"text":"go","type":"user"},
            {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"id":"p1","type":"text","text":"hello world"}],"finish":"done"}
        ]}))
        .unwrap();

    app.handle_transcript(2, response.data);

    assert_eq!(app.team.employees()[2].status, EmployeeStatus::Done);
    assert_eq!(app.transcript().len(), 2);
    assert_eq!(app.transcript()[0].role, TranscriptRole::User);
    assert_eq!(app.transcript()[1].text, "hello world");
}

#[test]
fn build_transcript_orders_chronologically_and_drops_empty_idle() {
    // The message endpoint returns newest-first by default; the transcript
    // must re-sort by creation time and drop the contentless idle marker.
    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m3","time":{"created":300},"type":"idle","outcome":"succeeded"},
        {"id":"m2","time":{"created":200,"completed":250},"type":"assistant",
         "content":[{"type":"text","text":"hello"}],"finish":"stop"},
        {"id":"m1","time":{"created":100},"text":"go","type":"user"}
    ]))
    .unwrap();

    let transcript = build_transcript(&messages);

    assert_eq!(transcript.len(), 2, "idle marker with no text is dropped");
    assert_eq!(transcript[0].role, TranscriptRole::User);
    assert_eq!(transcript[0].text, "go");
    assert_eq!(transcript[1].role, TranscriptRole::Assistant);
    assert_eq!(transcript[1].text, "hello");
}

#[test]
fn workflow_order_sorts_by_stage_then_name() {
    let (app, _rx) = make_app();
    // Starter storage order: orchestrator(0), researcher(1), writer(2),
    // reviewer(3), planner(4). Workflow order is the tree: the orchestrator
    // roots it, then planner → researcher → writer → reviewer.
    let ids: Vec<&str> = app
        .workflow_order()
        .iter()
        .map(|&i| app.team.employees()[i].id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "orchestrator",
            "planner",
            "researcher",
            "writer",
            "reviewer"
        ]
    );
}

/// The grid's order is the workflow order minus the root: the orchestrator is the
/// strip above the cards, so it must not be drawn as one. The tasks panel keeps
/// reading `workflow_order`, which still lists everyone.
#[test]
fn card_order_is_the_workflow_order_without_the_orchestrator() {
    let (app, _rx) = make_app();
    let root = employee(&app, "orchestrator");

    let cards: Vec<&str> = app
        .card_order()
        .iter()
        .map(|&i| app.team.employees()[i].id.as_str())
        .collect();
    assert_eq!(cards, vec!["planner", "researcher", "writer", "reviewer"]);
    assert!(!app.card_order().contains(&root), "the root has no card");

    // Both lists are drawn from the same roster, so the cards are a strict
    // subset — one entry fewer, and in the same relative order.
    let workflow = app.workflow_order();
    assert_eq!(workflow.len(), app.card_order().len() + 1);
    let relative: Vec<usize> = workflow.into_iter().filter(|&i| i != root).collect();
    assert_eq!(relative, app.card_order());
}

/// A roster with no orchestrator (a fixture built by hand) has every employee as
/// a card: the filter is on the root, not on the roster size.
#[test]
fn card_order_of_a_roofless_roster_is_the_whole_roster() {
    let (mut app, _rx) = make_app();
    app.team = long_team(3);
    assert_eq!(app.card_order().len(), 3);
    assert_eq!(app.card_order(), app.workflow_order());
}

/// The cursor starts on the orchestrator, and `↑` from the first card row reaches
/// it — the one employee without a card.
#[test]
fn the_orchestrator_is_selected_first_and_reached_by_arrowing_up() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);
    let root = employee(&app, "orchestrator");
    let planner = employee(&app, "planner");
    let researcher = employee(&app, "researcher");

    assert_eq!(app.selected(), root, "the roster opens on the root");

    // Down from the root lands on the first card, in card order.
    press(&mut app, KeyCode::Down);
    assert_eq!(app.selected(), planner);

    press(&mut app, KeyCode::Right);
    assert_eq!(app.selected(), researcher);

    // Up from the first card row goes back to the root.
    press(&mut app, KeyCode::Up);
    assert_eq!(app.selected(), root);

    // Left/right never leave the grid, so from the root they stay put.
    press(&mut app, KeyCode::Left);
    assert_eq!(app.selected(), root);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.selected(), root);
}

/// The root is not revealed: selecting it leaves the viewport where the first
/// card row is, so the strip stays on screen above it.
#[test]
fn selecting_the_orchestrator_does_not_scroll_the_grid() {
    let (mut app, _rx) = make_app();
    app.team = long_team(30);
    app.team.ensure_orchestrator();
    app.selected = employee(&app, "orchestrator");
    app.last_size = (120, 24); // three columns, two card rows on screen
    assert!(app.roster_max_scroll() > 0, "the fixture must overflow");

    // Scroll to the bottom, then walk back up to the root.
    app.selected = *app.card_order().last().unwrap();
    app.reveal_selection();
    let bottom = app.roster_scroll();
    assert_eq!(bottom, app.roster_max_scroll());

    for _ in 0..40 {
        press(&mut app, KeyCode::Up);
    }
    assert_eq!(app.selected(), employee(&app, "orchestrator"));
    assert_eq!(app.roster_scroll(), 0, "the root sits above the first page");
}

// --- the roster prompt ---

/// A goal typed on the roster screen goes to the orchestrator, whatever card
/// happens to be selected — the root is the one that decomposes and delegates.
#[test]
fn submit_goal_routes_to_the_orchestrator_not_the_selection() {
    let (mut app, _rx) = make_app();
    let root = employee(&app, "orchestrator");
    let writer = employee(&app, "writer");
    app.selected = writer; // the cursor is on a card, not the root

    press(&mut app, KeyCode::Char('i'));
    assert!(app.prompt_focused());
    for c in "ship the release".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);

    // The goal is the orchestrator's: it is in that chat, queued for the root's
    // session, and the writer never saw it.
    assert!(
        app.chats[&root]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::User && line.text == "ship the release")
    );
    assert!(
        app.chats[&root]
            .queue_running
            .iter()
            .any(|queued| queued == "ship the release"),
        "the root's session is not live, so the goal is queued"
    );
    assert!(
        !app.chats.contains_key(&writer) || app.chats[&writer].outbox.is_empty(),
        "the selected card must not receive the goal"
    );

    // The chain is rooted at the orchestrator, so a later `@user` delivers there.
    assert_eq!(app.origin_of.get(&root), Some(&root));
    assert_eq!(app.depth_of.get(&root), Some(&0));

    // The buffer is cleared and the bar keeps the keyboard for the next goal.
    assert!(app.input().is_empty());
    assert!(app.prompt_focused());
    assert_eq!(app.prompt_history, vec!["ship the release".to_string()]);
}

/// `↑`/`↓` in the prompt recall previous goals, restoring the draft past the
/// newest one.
#[test]
fn the_prompt_recalls_previous_goals() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char('i'));

    for goal in ["first goal", "second goal"] {
        for c in goal.chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
    }

    press(&mut app, KeyCode::Up);
    assert_eq!(app.input(), "second goal");
    press(&mut app, KeyCode::Up);
    assert_eq!(app.input(), "first goal");
    press(&mut app, KeyCode::Up);
    assert_eq!(app.input(), "first goal", "clamped at the oldest goal");

    press(&mut app, KeyCode::Down);
    assert_eq!(app.input(), "second goal");
    press(&mut app, KeyCode::Down);
    assert_eq!(app.input(), "", "past the newest goal is the draft");
}

/// A half-typed goal survives `Esc`: blurring the bar is not a cancel.
#[test]
fn escaping_the_prompt_keeps_the_typed_goal() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char('i'));
    for c in "unfinished".chars() {
        press(&mut app, KeyCode::Char(c));
    }

    press(&mut app, KeyCode::Esc);
    assert!(!app.prompt_focused());
    assert_eq!(app.input(), "unfinished");

    // And focusing again resumes editing the same text.
    press(&mut app, KeyCode::Tab);
    assert!(app.prompt_focused());
    assert_eq!(app.input(), "unfinished");
}

/// While the bar has the keyboard, the roster's single-letter shortcuts are text.
/// `q` types a `q` instead of quitting, `s`/`p`/`m` likewise.
#[test]
fn roster_shortcuts_are_text_while_the_prompt_is_focused() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char('i'));

    for c in "qspm".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    assert_eq!(app.input(), "qspm");
    assert!(!app.is_shutting_down(), "`q` must not quit while typing");
    assert_eq!(app.mode(), Mode::Roster, "`p`/`m` must not open a picker");

    // Blurred, the same keys are shortcuts again. `m` needs a connected client
    // (there is no model catalog to browse otherwise), so the check uses `p`,
    // which is purely local.
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('p'));
    assert_eq!(app.mode(), Mode::Projects);
}

/// The shortcuts are untouched while the bar is *not* focused — the plan's hard
/// requirement.
#[test]
fn roster_shortcuts_still_work_when_the_prompt_is_not_focused() {
    let (mut app, _rx) = make_app();
    assert!(
        !app.prompt_focused(),
        "the roster opens with the bar blurred"
    );

    press(&mut app, KeyCode::Char('p'));
    assert_eq!(app.mode(), Mode::Projects);
}

/// A bracketed paste goes into the focused prompt, and is ignored when neither
/// the chat nor the prompt has the keyboard.
#[test]
fn paste_lands_in_the_focused_prompt() {
    let (mut app, _rx) = make_app();
    app.update(Message::Paste("ignored".to_string()));
    assert_eq!(app.input(), "", "no field is focused");

    press(&mut app, KeyCode::Char('i'));
    app.update(Message::Paste("a pasted goal".to_string()));
    assert_eq!(app.input(), "a pasted goal");
    assert_eq!(app.input_cursor(), "a pasted goal".len());
}

/// `Enter` on an empty or whitespace-only prompt is a no-op: nothing is queued,
/// nothing is logged, and the history does not grow.
#[test]
fn an_empty_goal_is_not_submitted() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char('i'));

    press(&mut app, KeyCode::Enter);
    for _ in 0..3 {
        press(&mut app, KeyCode::Char(' '));
    }
    press(&mut app, KeyCode::Enter);

    assert!(app.prompt_history.is_empty());
    let root = employee(&app, "orchestrator");
    assert!(!app.chats.contains_key(&root) || app.chats[&root].queue_running.is_empty());
}

/// Opening a chat drops the keyboard from the roster prompt, so a half-typed goal
/// cannot follow the user into an employee's conversation.
#[tokio::test]
async fn opening_a_chat_clears_the_prompt_buffer() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char('i'));
    for c in "half a goal".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    assert!(app.prompt_focused());

    // Opening a chat is gated on a connection, so the fixture supplies one: the
    // point of the test is the buffer, not the transport.
    app.client = Some(crate::opencode::OpenCode::with_base(
        "http://127.0.0.1:1".parse().expect("a literal URL parses"),
        "sekrit",
    ));
    app.start_chat(employee(&app, "writer"));

    assert_eq!(app.mode(), Mode::Chat);
    assert!(!app.prompt_focused());
    assert!(app.input().is_empty(), "the chat opens on an empty buffer");
}

/// A click on the prompt band focuses the bar; a click on a card takes the
/// keyboard back and selects that employee.
#[test]
fn clicking_the_prompt_focuses_it_and_a_card_blurs_it() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);

    let band = ui::prompt_area((120, 40));
    assert!(band.height > 0, "the roster draws the band");
    app.handle_mouse(mouse_click(band.x + 2, band.y));
    assert!(app.prompt_focused(), "a click on the bar focuses it");
    assert_eq!(
        ui::hit_target_at(&app, (120, 40), band.x + 2, band.y),
        Some(ui::Target::Prompt)
    );

    let (column, row) = find_on_screen(&app, 120, 40, " WRITER ");
    app.handle_mouse(mouse_click(column, row));
    assert_eq!(app.selected(), employee(&app, "writer"));
    assert!(
        !app.prompt_focused(),
        "a card click takes the keyboard back"
    );
}

/// The roster's prompt band does not cost the panels a row: `main` is the same
/// rectangle it was before the bar existed.
#[test]
fn the_prompt_band_does_not_shrink_the_panels() {
    let (mut app, _rx) = make_app();
    app.update(Message::Resize(120, 40));

    let main = ui::team_area((120, 40));
    assert_eq!(main.y, 1);
    assert_eq!(main.height, 32, "40 - title - prompt - log - help");

    let band = ui::prompt_area((120, 40));
    assert_eq!(band.y, main.bottom(), "the bar sits directly under main");
    assert_eq!(band.height, 2);

    // Nothing in the grid changed shape: the geometry is the one the panels were
    // built against.
    assert!(!ui::grid_cells(&app, (120, 40)).is_empty());
}

#[test]
fn renders_grid_with_mixed_statuses() {
    let (mut app, _rx) = make_app();
    app.team.set_status(3, EmployeeStatus::Thinking); // planner
    app.team.set_status(0, EmployeeStatus::Working); // researcher
    app.selected = 3;

    screen(&app, 120, 30);
}

#[test]
fn renders_in_chat_mode() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    let chat = app.chats.get_mut(&0).unwrap();
    chat.transcript
        .push(TranscriptLine::new(TranscriptRole::User, "hi"));
    chat.streaming_text = "wor".into();
    app.input = "reply".into();

    screen(&app, 100, 30);
}

#[test]
fn renders_slash_command_menu_without_panicking() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.input = "/s".into();
    app.command_menu = Some(1);

    screen(&app, 100, 30);
}

#[test]
fn renders_projects_mode() {
    let (mut app, _rx) = make_app();
    app.mode = Mode::Projects;
    app.projects.push(Project {
        name: "alpha".into(),
        path: "/tmp/alpha".into(),
    });
    app.projects.push(Project {
        name: "beta".into(),
        path: "/tmp/beta".into(),
    });
    app.active_project = app.projects.first().cloned();
    app.project_selected = 0;

    screen(&app, 100, 30);
}

#[test]
fn quit_enters_shutdown_state() {
    let (mut app, _rx) = make_app();
    assert!(!app.is_shutting_down());

    // A quit message begins graceful shutdown exactly once (idempotent).
    app.update(Message::Quit);
    assert!(app.is_shutting_down());

    // A second quit shouldn't reset the shutdown animation frame.
    app.shutdown_frame = 7;
    app.update(Message::Quit);
    assert_eq!(app.shutdown_frame(), 7);
}

#[test]
fn renders_shutdown_overlay_without_panicking() {
    let (mut app, _rx) = make_app();
    app.update(Message::Quit);

    let text = screen(&app, 100, 30);
    assert!(
        text.contains("shutting down"),
        "shutdown overlay text missing"
    );
}

// --- reliability (deadline / retry / abandon) ---

fn stale_pending(instruction: &str, attempts: u8) -> PendingDelivery {
    PendingDelivery {
        instruction: instruction.into(),
        attempts,
        since: Instant::now() - harness::HANDOFF_TIMEOUT - std::time::Duration::from_secs(1),
        // Armed: the delivery started executing and then went silent, which is
        // what "stale" means here.
        armed: true,
    }
}

/// A silent hand-off that hasn't exhausted its retry budget is retried once
/// (bumping the attempt count) rather than abandoned.
#[test]
fn silent_handoff_retries_before_abandoning() {
    let (mut app, _rx) = make_app();
    app.chats.entry(1).or_default().session_id = Some("ses_w".into());
    app.chats.get_mut(&1).unwrap().pending = Some(stale_pending("draft the doc", 0));

    app.tick_reliability();

    let chat = &app.chats[&1];
    assert_eq!(chat.pending.as_ref().map(|p| p.attempts), Some(1));
    assert!(
        chat.transcript
            .iter()
            .any(|l| l.text.contains("🔁 retrying"))
    );
}

/// A silent hand-off past its retry budget is abandoned with a visible `⚠` note
/// and its deadline tracking cleared.
#[test]
fn silent_handoff_abandons_when_exhausted() {
    let (mut app, _rx) = make_app();
    app.chats.entry(1).or_default().session_id = Some("ses_w".into());
    app.origin_of.insert(1, 0);
    app.chats.get_mut(&1).unwrap().pending = Some(stale_pending("draft", harness::MAX_ATTEMPTS));

    app.tick_reliability();

    let chat = &app.chats[&1];
    assert!(chat.pending.is_none());
    assert!(
        chat.transcript
            .iter()
            .any(|l| l.text.contains("⚠ no deliverable"))
    );
    assert_eq!(app.team.employees()[1].status, EmployeeStatus::Error);
}

/// A fan-out whose worker goes silent and is abandoned still aggregates once the
/// remaining worker delivers — the abandoned worker contributes a `⚠` note.
#[test]
fn abandoned_worker_still_aggregates_fanout() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let researcher = employee(&app, "researcher");
    let writer = employee(&app, "writer");
    let reviewer = employee(&app, "reviewer");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner);
    app.origin_of.insert(planner, planner);
    app.fan_out(planner, "ship it".to_string());

    // The researcher goes silent past its deadline and has exhausted its budget.
    app.chats.entry(researcher).or_default().pending =
        Some(stale_pending("ship it", harness::MAX_ATTEMPTS));

    app.tick_reliability();

    // It is abandoned → its ⚠ contribution is recorded, footed by its id.
    let fanout = app.fanouts.values().next().unwrap();
    assert!(fanout.outputs.contains_key(&researcher));
    assert!(!fanout.aggregated);

    // The rest deliver; the fan-out then aggregates through the orchestrator.
    let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
            {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text": text}],"finish":"done"}
        ]))
        .unwrap()
    };
    app.handle_transcript(writer, assistant("w", "Draft copy."));
    app.handle_transcript(reviewer, assistant("v", "Checked."));

    let fanout = app.fanouts.values().next().unwrap();
    assert!(fanout.aggregated);
    let outbox = &app.chats[&orchestrator].outbox;
    assert_eq!(outbox.len(), 1);
    assert!(outbox[0].text().contains("no deliverable")); // the researcher's ⚠
    assert!(outbox[0].text().contains("Draft copy."));
}

// --- model picker search ---

/// Build a minimal [`ProviderModel`] for filtering tests.
fn model(id: &str, name: &str, provider: &str) -> ProviderModel {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "providerID": provider,
    }))
    .unwrap()
}

/// The search query filters case-insensitively across id, name, provider id,
/// and vendor (the id's first path segment); an empty query lists everything.
#[test]
fn model_picker_filter_matches_id_name_provider_vendor() {
    let (mut app, _rx) = make_app();
    app.models = vec![
        model("openrouter/gpt-5-wrap", "GPT-5", "openrouter"),
        model("meta/muse-spark-open", "Muse Spark Open", "openrouter"),
        model("kenari/deepseek-v4", "DeepSeek V4", "kenari"),
    ];

    // Empty query → every model, in order.
    assert_eq!(app.filtered_models(), vec![0, 1, 2]);

    // Case-insensitive match on the id.
    app.model_filter = "MUSE".into();
    assert_eq!(app.filtered_models(), vec![1]);

    // Provider id and vendor are searchable too.
    app.model_filter = "kenari".into();
    assert_eq!(app.filtered_models(), vec![2]);
    app.model_filter = "meta".into();
    assert_eq!(app.filtered_models(), vec![1]);

    // Display name.
    app.model_filter = "deepseek".into();
    assert_eq!(app.filtered_models(), vec![2]);

    // Whitespace is trimmed; no match yields an empty set.
    app.model_filter = "  zzz  ".into();
    assert!(app.filtered_models().is_empty());
}

/// Typing filters immediately and lands the highlight on the first match;
/// assignment resolves through the filtered index back to the catalog entry.
#[test]
fn model_picker_search_resets_selection_and_resolves_assignment() {
    let (mut app, _rx) = make_app();
    app.models = vec![
        model("kenari/deepseek-v4", "DeepSeek V4", "kenari"),
        model("meta/muse-spark-open", "Muse Spark Open", "openrouter"),
    ];
    app.mode = Mode::Models;

    for c in "muse".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.model_filter, "muse");
    assert_eq!(app.filtered_models(), vec![1]);
    assert_eq!(app.model_selected, 0, "highlight resets to first match");

    // The highlighted entry resolves to the filtered model, not index 0.
    assert_eq!(
        app.selected_model_id().as_deref(),
        Some("meta/muse-spark-open")
    );

    // Backspace shrinks the query; the match set stays the same here.
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.model_filter, "mu");
    assert_eq!(app.filtered_models(), vec![1]);

    // Ctrl-U clears the query, restoring the full list.
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(app.model_filter, "");
    assert_eq!(app.filtered_models(), vec![0, 1]);
}

/// Esc leaves the picker and clears the query.
#[test]
fn model_picker_esc_clears_filter_and_returns() {
    let (mut app, _rx) = make_app();
    app.mode = Mode::Models;
    app.model_filter = "claude".into();

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(app.model_filter, "");
    assert_eq!(app.mode, Mode::Roster);
}

/// A fresh model list clamps the selection to the (filtered) range.
#[test]
fn models_loaded_clamps_selection_to_filtered_range() {
    let (mut app, _rx) = make_app();
    app.model_selected = 10;
    app.model_filter = "muse".into();

    app.update(Message::ModelsLoaded(vec![model(
        "meta/muse-spark-open",
        "Muse Spark Open",
        "openrouter",
    )]));

    assert_eq!(app.model_selected, 0);
}

// --- help: overlay, command table, footer hints ---

#[test]
fn question_mark_opens_help_and_any_key_dismisses() {
    let (mut app, _rx) = make_app();

    // `?` opens the help overlay from the roster.
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(app.show_help);

    // Any subsequent key dismisses it.
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(!app.show_help);
}

#[test]
fn help_does_not_open_in_chat() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    // `?` is a normal character in chat, not a help toggle.
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(!app.show_help);
    assert_eq!(app.input, "?");
}

#[test]
fn renders_help_overlay_without_panicking() {
    let (mut app, _rx) = make_app();
    app.show_help = true;

    screen(&app, 100, 30);
}

/// `/help` is generated from the command table, so every command the table knows
/// is listed with the argument it takes and the description the completion menu
/// shows — the two surfaces that explain the commands cannot drift apart.
#[test]
fn help_lists_every_command_from_the_table() {
    let text = chat_help_text();
    for command in CHAT_COMMANDS {
        assert!(
            text.contains(&command.usage()),
            "{} missing from /help",
            command.command
        );
        assert!(
            text.contains(command.description),
            "{} has no description in /help",
            command.command
        );
    }
}

/// A command whose whole point is a free-text argument is listed by `/help` but
/// kept out of the completion menu, which would otherwise trigger while the user
/// is still typing the goal.
#[test]
fn help_lists_argument_commands_the_menu_leaves_out() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.input = "/parallel".into();

    assert!(chat_help_text().contains("/parallel <goal>"));
    assert!(app.matching_commands().is_empty(), "menu offered it");
}

/// The `/help` chat command prints that same listing into the transcript.
#[test]
fn slash_help_prints_the_command_table() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.input = "/help".into();

    app.send_prompt();

    // `/help` is written one transcript line per command, so a long table cannot
    // be clipped by a single wrapped block.
    let help = chat_help_text();
    let expected: Vec<&str> = help.lines().collect();
    let printed: Vec<&str> = app.chats[&0]
        .transcript
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(
        printed[printed.len() - expected.len()..],
        expected[..],
        "the whole table, line for line"
    );
}

// --- grapheme-safe editing ---

/// Backspace removes a whole grapheme cluster — a family emoji is many code
/// points joined by ZWJ but is a single cluster and deletes as one.
#[test]
fn backspace_removes_whole_grapheme_cluster() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.input = "a👨‍👩‍👧‍👦b".into();
    app.cursor_end();
    assert_eq!(app.input_cursor(), 3); // "a", family, "b"

    app.input_delete_backward();
    assert_eq!(app.input, "a👨‍👩‍👧‍👦");

    app.input_delete_backward();
    assert_eq!(app.input, "a");
    assert_eq!(app.input_cursor(), 1);
}

/// The cursor moves by cluster, not by code point (🇺🇸 is two code points).
#[test]
fn cursor_navigates_by_grapheme_cluster() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.input = "👍🇺🇸".into(); // 👍 (1 cluster) + 🇺🇸 (1 cluster)

    app.cursor_end();
    assert_eq!(app.input_cursor(), 2, "two clusters, not three code points");

    app.cursor_left();
    assert_eq!(app.input_cursor(), 1);
    app.cursor_left();
    assert_eq!(app.input_cursor(), 0);
}

#[test]
fn forward_delete_removes_whole_grapheme_cluster() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.input = "👍🇺🇸".into();
    app.cursor_home();
    app.input_delete_forward();
    assert_eq!(app.input, "🇺🇸");
}

/// Inserting a combining mark merges it with the preceding cluster; the cursor
/// follows the cluster rather than overrunning the grapheme count.
#[test]
fn combining_char_insert_tracks_merged_cluster() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);

    app.input_insert('a');
    app.input_insert('b');
    assert_eq!(app.input_cursor(), 2);

    app.cursor_left();
    app.input_insert('\u{301}'); // merges with 'a' → "á"
    assert_eq!(app.input, "a\u{301}b");
    assert_eq!(
        app.input_cursor(),
        1,
        "cursor sits at the merged cluster's end"
    );

    app.input_delete_backward();
    assert_eq!(app.input, "b");
    assert_eq!(app.input_cursor(), 0);
}

// --- input horizontal scroll ---

#[test]
fn input_scroll_follows_cursor_horizontally() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.last_size = (8, 24); // 4 display columns for the input body

    for c in "abcdefgh".chars() {
        app.input_insert(c);
    }
    assert_eq!(app.input_cursor(), 8);
    // The cursor (column 8) is past the 4-column window → scrolled to keep it in.
    assert_eq!(app.input_scroll, 8 + 1 - 4);

    app.cursor_home();
    assert_eq!(app.input_scroll, 0);
}

#[test]
fn input_scroll_retracts_on_delete_shrink() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.last_size = (8, 24); // 4 display columns for the input body

    for c in "abcdefgh".chars() {
        app.input_insert(c);
    }
    assert_eq!(app.input_cursor(), 8);
    // Cursor (column 8) is past the 4-column window → scrolled to the end.
    assert_eq!(app.input_scroll, 8 + 1 - 4);

    // Backspace 4× back to "abcd". The view must pull back left so the text
    // stays visible (showing "bcd" + cursor), not collapse to a bare cursor.
    for _ in 0..4 {
        app.input_delete_backward();
    }
    assert_eq!(app.input, "abcd");
    assert_eq!(app.input_cursor(), 4);
    assert_eq!(app.input_scroll, 4 + 1 - 4);
}

#[test]
fn input_scroll_retracts_on_delete_word() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.last_size = (8, 24); // 4 display columns for the input body

    for c in "abcdefgh".chars() {
        app.input_insert(c);
    }
    assert_eq!(app.input_scroll, 8 + 1 - 4);

    // Ctrl-W back to the empty prefix: the scroll must retract to zero.
    app.input_delete_word();
    assert_eq!(app.input, "");
    assert_eq!(app.input_scroll, 0);
}

/// The tasks panel orders active work (pending, running, retrying) ahead of
/// terminal tasks, each group newest-first by creation time.
#[test]
fn order_tasks_puts_active_first_then_recent() {
    let mut pending = Task::new("pending goal", None);
    let mut running = Task::new("running goal", None);
    let mut retrying = Task::new("retrying goal", None);
    let mut done_old = Task::new("old success", None);
    let mut done_new = Task::new("new success", None);
    let mut failed = Task::new("failed goal", None);

    // Fixed timestamps keep the expectation independent of wall-clock order.
    pending.created_at = 10;
    running.created_at = 30;
    retrying.created_at = 20;
    done_old.created_at = 5;
    done_new.created_at = 40;
    failed.created_at = 6;
    running.status = TaskStatus::Running;
    retrying.status = TaskStatus::Retrying;
    done_old.status = TaskStatus::Succeeded;
    done_new.status = TaskStatus::Succeeded;
    failed.status = TaskStatus::Failed;

    let tasks = vec![pending, running, retrying, done_old, done_new, failed];
    let ordered: Vec<&str> = order_tasks(&tasks)
        .into_iter()
        .map(|index| tasks[index].goal.as_str())
        .collect();

    assert_eq!(
        ordered,
        vec![
            "running goal",
            "retrying goal",
            "pending goal", // active work, newest first
            "new success",
            "failed goal",
            "old success", // terminal, newest first
        ]
    );
}

#[test]
fn tasks_accessor_exposes_persisted_tasks() {
    let (mut app, _rx) = make_app();
    assert!(app.tasks().is_empty());

    app.store
        .upsert_task(Task::new("write the changelog", None));
    app.store
        .upsert_task(Task::new("sweep the inbox", Some("researcher".into())));

    let goals: Vec<&str> = app.tasks().iter().map(|task| task.goal.as_str()).collect();
    assert_eq!(goals, vec!["write the changelog", "sweep the inbox"]);
}

/// The persisted queue section renders above the per-employee board without
/// panicking, showing each task's status and a goal summary.
#[test]
fn renders_task_queue_without_panicking() {
    let (mut app, _rx) = make_app();
    let mut running = Task::new("sweep the inbox", Some("researcher".into()));
    running.status = TaskStatus::Running;
    app.store.upsert_task(running);
    app.store
        .upsert_task(Task::new("write the changelog", None));

    let text = screen(&app, 120, 40);
    assert!(text.contains("queue"), "task queue heading missing");
    assert!(text.contains("running"), "queued task status missing");
    assert!(text.contains("sweep the inbox"), "queued goal missing");
    assert!(text.contains("pending"), "pending task status missing");
}

/// AC1: while the engine is paused, an autonomous fire is recorded as queued
/// work — `pending`, no session — and the log says so.
#[test]
fn paused_engine_queues_a_fire_without_dispatching() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char(' ')); // pause the engine
    assert!(app.is_paused());

    app.update(Message::WatchedTask {
        employee: "researcher".into(),
        goal: "Review the change to src/app.rs".into(),
    });

    let task = app.store.tasks.last().expect("the fire was recorded");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.session_id, None);
    assert!(
        app.log
            .iter()
            .any(|line| line.contains("⏸ queued (paused)")),
        "queue log line missing: {:?}",
        app.log
    );
}

/// AC2: resuming the engine dispatches what queued up — every queued task runs,
/// and the dispatch set is the queue panel's own ordering.
#[test]
fn resuming_the_engine_dispatches_queued_tasks_in_queue_order() {
    let (mut app, _rx) = make_app();
    press(&mut app, KeyCode::Char(' ')); // pause

    app.update(Message::WatchedTask {
        employee: "researcher".into(),
        goal: "first".into(),
    });
    app.update(Message::WatchedTask {
        employee: "writer".into(),
        goal: "second".into(),
    });
    assert!(app.tasks().iter().all(|t| t.status == TaskStatus::Pending));
    assert_eq!(app.resumable_tasks().len(), 2);

    press(&mut app, KeyCode::Char(' ')); // resume
    assert!(!app.is_paused());
    assert!(app.tasks().iter().all(|t| t.status == TaskStatus::Running));
}

/// The dispatch set is `order_tasks` restricted to queued work the user hasn't
/// individually paused — so a resume can't go out of order or wake a held task.
#[test]
fn resumable_tasks_follow_order_tasks_and_skip_held_work() {
    let (mut app, _rx) = make_app();

    let mut older = Task::new("older", Some("researcher".into()));
    older.created_at = 10;
    let mut newer = Task::new("newer", Some("writer".into()));
    newer.created_at = 20;
    let mut held = Task::new("held", Some("planner".into()));
    held.created_at = 30;
    assert!(held.pause());
    let mut done = Task::new("done", Some("reviewer".into()));
    done.succeed("ok");

    for task in [older, newer, held, done] {
        app.store.upsert_task(task);
    }

    let goals: Vec<&str> = app
        .resumable_tasks()
        .iter()
        .map(|&index| app.store.tasks[index].goal.as_str())
        .collect();
    // Newest active first; the individually paused and the terminal task drop out.
    assert_eq!(goals, vec!["newer", "older"]);
}

/// AC3: pausing a running task holds it, and the interrupted session's trailing
/// attempt-finish does not flip it back.
#[test]
fn pausing_a_running_task_survives_a_late_attempt_finish() {
    let (mut app, _rx) = make_app();
    let mut task = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(task.transition(TaskStatus::Running));
    task.session_id = Some("ses_task".into());
    let task_id = task.id;
    app.store.upsert_task(task);

    app.selected = app.team_index_by_id("researcher").expect("known employee");
    press(&mut app, KeyCode::Char('s'));

    assert_eq!(app.store.tasks[0].status, TaskStatus::Paused);
    assert!(app.store.tasks[0].paused);

    // The interrupt's trailing completion must be ignored.
    app.update(Message::TaskAttemptFinished {
        task_id,
        succeeded: true,
        result: "done anyway".into(),
    });

    let held = &app.store.tasks[0];
    assert_eq!(
        held.status,
        TaskStatus::Paused,
        "a late finish must not flip a paused task"
    );
    assert!(held.result.is_none());
    assert!(
        app.log
            .iter()
            .any(|line| line.contains("ignored late attempt")),
        "the race guard is silent: {:?}",
        app.log
    );
}

/// A task paused before its attempt reports in: the arriving session is
/// interrupted, not adopted, and the task stays paused.
#[test]
fn a_paused_task_does_not_adopt_a_late_session() {
    let (mut app, _rx) = make_app();
    let mut task = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(task.transition(TaskStatus::Running));
    let task_id = task.id;
    app.store.upsert_task(task);

    app.pause_task(0);
    app.update(Message::TaskAttemptStarted {
        task_id,
        session_id: "ses_late".into(),
    });

    assert_eq!(app.store.tasks[0].status, TaskStatus::Paused);
    assert_eq!(app.store.tasks[0].session_id, None);
}

/// A recording OpenCode double for the resume tests: the app dispatches real
/// HTTP at it, and the test reads back the sessions that were started and the
/// prompt text that was sent — so a resume is witnessed by the attempt it
/// actually issued, not inferred from the task's state. Scoped to what one
/// dispatch touches; the full client surface is covered by `tests/opencode_fake.rs`.
mod recorder {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::{
        Json, Router,
        extract::{Path, State},
        routing::{get, post},
    };
    use reqwest::Url;
    use serde_json::{Value, json};

    use crate::opencode::OpenCode;

    #[derive(Clone, Default)]
    pub struct Recorder {
        /// How many sessions the app has started (`POST /api/session`).
        pub sessions: Arc<Mutex<usize>>,
        /// The prompt text of every attempt, in order.
        pub prompts: Arc<Mutex<Vec<String>>>,
        /// The `delivery` knob of every attempt, in order (`None` when the client
        /// sent no knob at all).
        pub deliveries: Arc<Mutex<Vec<Option<String>>>>,
        /// The admission id of every attempt, in order.
        pub admission_ids: Arc<Mutex<Vec<Option<String>>>>,
        /// Every `(session_id, prompt)` pair, so a test can tie an attempt back
        /// to the task that issued it and prove no task adopted another's id.
        pub attempts: Arc<Mutex<Vec<(String, String)>>>,
        /// The session ids the app interrupted (`POST .../interrupt`).
        pub interrupts: Arc<Mutex<Vec<String>>>,
    }

    async fn create_session(State(state): State<Recorder>) -> Json<Value> {
        let mut count = state.sessions.lock().unwrap();
        *count += 1;
        // A distinct id per session, so a test can tell two attempts apart.
        let id = format!("ses_{}", *count);
        Json(json!({ "data": { "id": id, "title": "fresh" } }))
    }

    async fn prompt(
        State(state): State<Recorder>,
        Path(id): Path<String>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let text = body
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        state.prompts.lock().unwrap().push(text.clone());
        state.deliveries.lock().unwrap().push(
            body.get("delivery")
                .and_then(Value::as_str)
                .map(str::to_owned),
        );
        state
            .admission_ids
            .lock()
            .unwrap()
            .push(body.get("id").and_then(Value::as_str).map(str::to_owned));
        state.attempts.lock().unwrap().push((id, text));
        Json(json!({}))
    }

    async fn interrupt(State(state): State<Recorder>, Path(id): Path<String>) -> Json<Value> {
        state.interrupts.lock().unwrap().push(id);
        Json(json!({}))
    }

    async fn messages() -> Json<Value> {
        // A completed assistant reply, so an attempt reads back a deliverable and
        // finishes instead of retrying (which would start another session).
        Json(json!({
            "data": [
                { "id": "m1", "time": { "created": 1 }, "text": "go", "type": "user" },
                { "id": "m2", "time": { "created": 2, "completed": 3 }, "type": "assistant",
                  "content": [{ "id": "p1", "type": "text", "text": "the deliverable" }],
                  "finish": "done" }
            ],
            "cursor": { "previous": null, "next": null }
        }))
    }

    async fn ok() -> Json<Value> {
        Json(json!({}))
    }

    /// Serve the double on an ephemeral port; return a client pointed at it plus
    /// the shared state to assert against.
    pub async fn start() -> (OpenCode, Recorder) {
        let state = Recorder::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let router = Router::new()
            .route("/api/session", post(create_session))
            .route("/api/session/{id}/prompt", post(prompt))
            .route("/api/session/{id}/message", get(messages))
            .route("/api/session/{id}/interrupt", post(interrupt))
            .route("/api/experimental/session/{id}/wait", post(ok))
            .with_state(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let base = Url::parse(&format!("http://{addr}")).unwrap();
        (OpenCode::with_base(base, "sekrit"), state)
    }

    /// Wait for the spawned attempt to send its prompt, then return it.
    pub async fn first_prompt(state: &Recorder) -> String {
        for _ in 0..400 {
            if let Some(text) = state.prompts.lock().unwrap().first().cloned() {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the dispatched attempt never sent a prompt");
    }

    /// Wait until at least `count` prompts have arrived, so a test can read the
    /// delivery knobs alongside the text.
    pub async fn wait_for_prompts(state: &Recorder, count: usize) {
        for _ in 0..400 {
            if state.prompts.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!(
            "only {} prompts arrived, wanted {count}",
            state.prompts.lock().unwrap().len()
        );
    }
}

/// AC4: resuming a paused task returns it to `running` and issues a **fresh**
/// attempt — a new session, prompted with the resume preamble ahead of the goal.
/// The recording double lets the test read the attempted prompt itself, so it
/// witnesses AC4's pass condition rather than inferring it from task state.
#[tokio::test]
async fn resuming_a_paused_task_returns_it_to_running() {
    let (client, server) = recorder::start().await;

    let (mut app, _rx) = make_app();
    app.client = Some(client);

    let mut task = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(task.transition(TaskStatus::Running));
    task.session_id = Some("ses_old".into());
    let task_id = task.id;
    app.store.upsert_task(task);

    app.selected = app.team_index_by_id("researcher").expect("known employee");
    press(&mut app, KeyCode::Char('s')); // pause
    assert_eq!(app.store.tasks[0].status, TaskStatus::Paused);

    press(&mut app, KeyCode::Char('s')); // resume
    assert_eq!(app.store.tasks[0].status, TaskStatus::Running);
    assert!(!app.store.tasks[0].paused);
    assert!(
        app.log.iter().any(|line| line.contains("▶ resumed task")),
        "resume must be logged: {:?}",
        app.log
    );

    // The fresh attempt reports its new session in; it *replaces* the
    // interrupted one — the id changes, it doesn't merely still exist.
    assert_eq!(app.store.tasks[0].session_id.as_deref(), Some("ses_old"));
    app.update(Message::TaskAttemptStarted {
        task_id,
        session_id: "ses_fresh".into(),
    });
    let resumed_session = app.store.tasks[0].session_id.clone();
    assert_eq!(resumed_session.as_deref(), Some("ses_fresh"));
    assert_ne!(resumed_session.as_deref(), Some("ses_old"));

    // Witness the attempt itself: exactly one fresh session, prompted with the
    // resume preamble ahead of the goal.
    let prompt = recorder::first_prompt(&server).await;
    assert_eq!(
        *server.sessions.lock().unwrap(),
        1,
        "resume must start exactly one fresh session"
    );
    assert!(
        prompt.contains(&harness::resume_preamble()),
        "the resumed attempt must carry the resume preamble: {prompt}"
    );
    assert!(
        prompt.ends_with("sweep the inbox"),
        "the goal must follow the preamble: {prompt}"
    );
}

/// The resume preamble describes *interrupted* work ("the previous attempt was
/// paused and interrupted"), so a task paused before it ever ran — no attempts,
/// no session — must resume with the bare goal rather than claim an attempt
/// that never happened.
#[tokio::test]
async fn resuming_a_task_that_never_ran_omits_the_preamble() {
    let (client, server) = recorder::start().await;

    let (mut app, _rx) = make_app();
    app.client = Some(client);

    // Queued but never dispatched: zero attempts, no session.
    let task = Task::new("sweep the inbox", Some("researcher".into()));
    assert_eq!(task.attempts, 0);
    assert!(task.session_id.is_none());
    app.store.upsert_task(task);

    app.selected = app.team_index_by_id("researcher").expect("known employee");
    press(&mut app, KeyCode::Char('s')); // pause the queued task
    assert_eq!(app.store.tasks[0].status, TaskStatus::Paused);
    press(&mut app, KeyCode::Char('s')); // resume

    let prompt = recorder::first_prompt(&server).await;
    // A scheduled job is briefed as an isolated job: team context, the "finish it
    // yourself" instruction, then the goal — with **no** retry/resume preamble,
    // because the task never ran.
    assert!(
        prompt.ends_with("sweep the inbox"),
        "the goal must be last: {prompt}"
    );
    assert!(
        !prompt.contains("previous attempt"),
        "a task with no prior attempt must not claim an interrupted attempt: {prompt}"
    );
}

/// The reopen prompt's body carries the restart disclaimer: the tasks were
/// paused when mywaker closed and will be **re-run from the beginning**, so the
/// user isn't misled into thinking an interrupted session is continued.
#[test]
fn reopen_prompt_body_carries_the_restart_disclaimer() {
    let (mut app, _rx) = make_app();
    let mut task = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(task.transition(TaskStatus::Running));
    assert!(task.pause_for(PauseReason::Shutdown));
    app.store.upsert_task(task);
    app.prepare_resume_prompt();
    assert_eq!(app.resume_prompt().expect("prompt offered").count(), 1);

    let text = screen(&app, 120, 40);
    assert!(text.contains("resume paused tasks"), "modal title missing");
    assert!(
        text.contains("paused when mywaker closed"),
        "modal count line missing"
    );
    assert!(
        text.contains("re-run from the beginning"),
        "restart disclaimer missing: {text}"
    );
}

/// The reopen prompt covers only the tasks *mywaker* paused on the way out:
/// answering it resumes those, while a task the user paused deliberately stays
/// held. The prompt is modal, so `y` is what continues them.
#[test]
fn reopen_prompt_resumes_shutdown_pauses_but_not_deliberate_ones() {
    let (mut app, _rx) = make_app();

    let mut held_at_close = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(held_at_close.transition(TaskStatus::Running));
    assert!(held_at_close.pause_for(PauseReason::Shutdown));
    let mut held_by_user = Task::new("write the changelog", Some("writer".into()));
    assert!(held_by_user.pause()); // PauseReason::User
    app.store.upsert_task(held_at_close);
    app.store.upsert_task(held_by_user);

    app.prepare_resume_prompt();
    assert_eq!(app.resume_prompt().expect("prompt offered").count(), 1);

    press(&mut app, KeyCode::Char('y')); // resume all the prompt covers

    assert!(app.resume_prompt().is_none(), "answering clears the prompt");
    let resumed = &app.store.tasks[0];
    assert_eq!(resumed.goal, "sweep the inbox");
    assert_eq!(resumed.status, TaskStatus::Running);
    let still_held = &app.store.tasks[1];
    assert_eq!(still_held.goal, "write the changelog");
    assert_eq!(still_held.status, TaskStatus::Paused);
    assert!(still_held.paused);
}

/// Answering the reopen prompt with `n` keeps the tasks held and drops the
/// prompt, so a later launch won't offer them again by accident.
#[test]
fn reopen_prompt_declining_keeps_the_tasks_held() {
    let (mut app, _rx) = make_app();
    let mut task = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(task.transition(TaskStatus::Running));
    assert!(task.pause_for(PauseReason::Shutdown));
    app.store.upsert_task(task);
    app.prepare_resume_prompt();

    press(&mut app, KeyCode::Char('n'));

    assert!(app.resume_prompt().is_none(), "declining clears the prompt");
    assert_eq!(app.store.tasks[0].status, TaskStatus::Paused);
    assert!(app.store.tasks[0].paused);
}

/// AC7: accepting the reopen prompt re-runs **every** shutdown-paused task in
/// `order_tasks` order, each in its own fresh session.
///
/// Each task is given a *distinct stale* session id first, so "the session
/// changed" is asserted against that task's own pre-pause id instead of merely
/// "an id exists", and the double's `(session, prompt)` record proves no task
/// adopted another's id. The resume loop runs synchronously, so the
/// `▶ resumed task` log lines witness the dispatch order; the attempts' sessions
/// are minted concurrently, so the ids are matched to tasks, not to positions.
#[tokio::test]
async fn reopen_prompt_resumes_each_task_in_order_with_a_fresh_session() {
    let (client, server) = recorder::start().await;

    let (mut app, mut rx) = make_app();
    app.client = Some(client);

    // Two shutdown-paused tasks; distinct `created_at` fixes `order_tasks`
    // (newest active first) to `[newer, older]`.
    let mut older = Task::new("older goal", Some("researcher".into()));
    assert!(older.transition(TaskStatus::Running));
    older.created_at = 10;
    older.session_id = Some("ses_stale_older".into());
    assert!(older.pause_for(PauseReason::Shutdown));
    let older_id = older.id;

    let mut newer = Task::new("newer goal", Some("researcher".into()));
    assert!(newer.transition(TaskStatus::Running));
    newer.created_at = 20;
    newer.session_id = Some("ses_stale_newer".into());
    assert!(newer.pause_for(PauseReason::Shutdown));
    let newer_id = newer.id;

    app.store.upsert_task(older);
    app.store.upsert_task(newer);

    // The batch order is `order_tasks` order, asserted rather than assumed.
    let expected_order: Vec<TaskId> = order_tasks(&app.store.tasks)
        .into_iter()
        .map(|index| app.store.tasks[index].id)
        .collect();
    assert_eq!(expected_order, vec![newer_id, older_id]);

    app.prepare_resume_prompt();
    assert_eq!(
        app.resume_prompt().expect("prompt offered").tasks,
        expected_order,
        "the prompt must collect the batch in order_tasks order"
    );

    app.resume_shutdown_paused();
    assert!(
        app.resume_prompt().is_none(),
        "answering consumes the prompt"
    );

    // (d) The dispatch sequence is the prompt's order, and the prompt's order is
    // `order_tasks`. The synchronous resume loop logs each task as it goes.
    let resumed: Vec<String> = app
        .log
        .iter()
        .filter_map(|line| line.strip_prefix("▶ resumed task: "))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        resumed,
        vec!["newer goal", "older goal"],
        "resume dispatch must follow order_tasks"
    );

    // (b) Drive the attempts' `TaskAttemptStarted` messages back in until each
    // task has replaced *its own* stale id — `is_some()` alone would not prove
    // that. Only the start message is applied: the fake session completes
    // instantly, so applying its trailing finish would move the tasks to
    // `Succeeded` and hide the resume state this test is about.
    let stale = [(older_id, "ses_stale_older"), (newer_id, "ses_stale_newer")];
    for _ in 0..400 {
        while let Ok(message) = rx.try_recv() {
            if matches!(message, Message::TaskAttemptStarted { .. }) {
                app.update(message);
            }
        }
        let replaced = stale.iter().all(|(id, old)| {
            app.store
                .tasks
                .iter()
                .find(|task| task.id == *id)
                .is_some_and(|task| task.session_id.as_deref() != Some(*old))
        });
        if replaced {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // (a) Every task in the batch is running again, unpaused, reason cleared.
    for id in [older_id, newer_id] {
        let task = app
            .store
            .tasks
            .iter()
            .find(|task| task.id == id)
            .expect("known task");
        assert_eq!(
            task.status,
            TaskStatus::Running,
            "{} must run again",
            task.goal
        );
        assert!(!task.paused, "{} must be unpaused", task.goal);
        assert_eq!(
            task.pause_reason, None,
            "{} must clear its pause reason",
            task.goal
        );
    }

    let session_of = |id: TaskId| -> String {
        app.store
            .tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.session_id.clone())
            .unwrap_or_else(|| panic!("task {id} never recorded a session"))
    };
    let older_session = session_of(older_id);
    let newer_session = session_of(newer_id);

    // (b) Each id changed from *that task's* stale id and never became the other
    // task's stale id; (c) the two fresh ids are pairwise distinct.
    assert_ne!(
        older_session, "ses_stale_older",
        "resume must not continue the interrupted session"
    );
    assert_ne!(
        newer_session, "ses_stale_newer",
        "resume must not continue the interrupted session"
    );
    assert_ne!(
        older_session, "ses_stale_newer",
        "no task may adopt another task's old id"
    );
    assert_ne!(
        newer_session, "ses_stale_older",
        "no task may adopt another task's old id"
    );
    assert_ne!(
        older_session, newer_session,
        "each resumed task must get its own fresh session, not share one"
    );

    // The double agrees: one session per task, and the session the app stored
    // for a goal is exactly the one that goal's prompt was sent to.
    assert_eq!(
        *server.sessions.lock().unwrap(),
        2,
        "one fresh session per resumed task"
    );
    // `TaskAttemptStarted` is reported before the prompt is sent, so give both
    // prompts time to reach the double before reading its attempt log.
    for _ in 0..400 {
        if server.attempts.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let attempts = server.attempts.lock().unwrap().clone();
    assert_eq!(attempts.len(), 2, "both resumed attempts were prompted");
    for (goal, session) in [("newer goal", newer_session), ("older goal", older_session)] {
        let (recorded, prompt) = attempts
            .iter()
            .find(|(_, prompt)| prompt.ends_with(goal))
            .unwrap_or_else(|| panic!("the double saw no prompt for {goal:?}"));
        assert_eq!(
            recorded, &session,
            "{goal:?} stored a session it did not start itself"
        );
        assert!(
            prompt.contains(&harness::resume_preamble()),
            "{goal:?} must be re-run with the resume preamble: {prompt}"
        );
    }
}

/// The close path holds the whole non-terminal queue with
/// `PauseReason::Shutdown` so it is offered back on reopen — while a task the
/// user paused deliberately keeps its own reason, and a finished task is left
/// alone.
///
/// Drive it over a **mixed** queue in one go: `Running` (with a session),
/// queued `Pending`, `Retrying`, terminal, and a deliberate user pause. The
/// session-bearing task's `/interrupt` is recorded by the double, so
/// pause-before-interrupt is witnessed rather than inferred from the code.
#[tokio::test]
async fn shutdown_holds_the_queue_but_respects_deliberate_pauses() {
    let (client, server) = recorder::start().await;
    let (mut app, _rx) = make_app();
    app.client = Some(client);

    let mut running = Task::new("running goal", Some("researcher".into()));
    assert!(running.transition(TaskStatus::Running));
    running.created_at = 10;
    running.session_id = Some("ses_run".into());
    let running_id = running.id;

    let mut queued = Task::new("queued goal", Some("researcher".into()));
    queued.created_at = 20;
    let queued_id = queued.id;

    let mut retrying = Task::new("retrying goal", Some("researcher".into()));
    assert!(retrying.transition(TaskStatus::Running));
    assert!(retrying.try_retry());
    retrying.created_at = 30;
    let retrying_id = retrying.id;

    let mut terminal = Task::new("finished goal", Some("writer".into()));
    terminal.created_at = 5;
    terminal.succeed("done");

    let mut by_user = Task::new("held goal", Some("planner".into()));
    by_user.created_at = 40;
    by_user.session_id = Some("ses_user".into());
    assert!(by_user.pause_for(PauseReason::User));
    let user_id = by_user.id;

    for task in [running, queued, retrying, terminal, by_user] {
        app.store.upsert_task(task);
    }

    app.initiate_shutdown();
    assert!(app.is_shutting_down());

    // The whole non-terminal queue is held, shutdown-paused.
    for (id, goal) in [
        (running_id, "running goal"),
        (queued_id, "queued goal"),
        (retrying_id, "retrying goal"),
    ] {
        let task = app
            .store
            .tasks
            .iter()
            .find(|task| task.id == id)
            .expect("known task");
        assert_eq!(task.goal, goal);
        assert_eq!(task.status, TaskStatus::Paused, "{goal} must be held");
        assert!(task.paused, "{goal} must be paused");
        assert_eq!(
            task.pause_reason,
            Some(PauseReason::Shutdown),
            "{goal} must be held by shutdown, not by the user"
        );
        assert!(task.paused_at_shutdown());
    }

    // Terminal work is untouched.
    let done = app
        .store
        .tasks
        .iter()
        .find(|task| task.goal == "finished goal")
        .expect("terminal task");
    assert_eq!(done.status, TaskStatus::Succeeded);
    assert!(!done.paused);
    assert_eq!(done.pause_reason, None);
    assert_eq!(done.result.as_deref(), Some("done"));

    // A deliberate pause keeps its own reason and is never offered back.
    let user_held = app
        .store
        .tasks
        .iter()
        .find(|task| task.id == user_id)
        .expect("user-held task");
    assert_eq!(user_held.status, TaskStatus::Paused);
    assert!(user_held.paused);
    assert_eq!(user_held.pause_reason, Some(PauseReason::User));
    assert!(!user_held.paused_at_shutdown());

    // Pause-before-interrupt, witnessed by the double: the one session a
    // shutdown pause took over (`ses_run`) is interrupted, and only that one.
    for _ in 0..400 {
        if !server.interrupts.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        *server.interrupts.lock().unwrap(),
        vec!["ses_run".to_string()],
        "quit must interrupt the session it paused, and not one the user already held"
    );
    assert!(
        app.log
            .iter()
            .any(|line| line.contains("paused 3 task(s) at shutdown")),
        "the close path must report the held count: {:?}",
        app.log
    );

    // The interrupt's trailing completion arrives *after* the pause: it must not
    // flip the held task — the observable consequence of pause-before-interrupt.
    app.update(Message::TaskAttemptFinished {
        task_id: running_id,
        succeeded: true,
        result: "finished anyway".into(),
    });
    let still_held = app
        .store
        .tasks
        .iter()
        .find(|task| task.id == running_id)
        .expect("known task");
    assert_eq!(still_held.status, TaskStatus::Paused);
    assert!(still_held.paused);
    assert_eq!(still_held.pause_reason, Some(PauseReason::Shutdown));

    // Only the shutdown-paused tasks are offered back, in `order_tasks` order.
    let expected: Vec<TaskId> = order_tasks(&app.store.tasks)
        .into_iter()
        .map(|index| &app.store.tasks[index])
        .filter(|task| task.paused_at_shutdown())
        .map(|task| task.id)
        .collect();
    assert_eq!(expected, vec![retrying_id, queued_id, running_id]);

    app.prepare_resume_prompt();
    let prompt = app.resume_prompt().expect("prompt offered");
    assert_eq!(
        prompt.tasks,
        vec![retrying_id, queued_id, running_id],
        "only shutdown pauses are collected, in order_tasks order"
    );
    let goals: Vec<&str> = prompt
        .tasks
        .iter()
        .map(|id| {
            app.store
                .tasks
                .iter()
                .find(|task| task.id == *id)
                .expect("known task")
                .goal
                .as_str()
        })
        .collect();
    assert_eq!(goals, vec!["retrying goal", "queued goal", "running goal"]);

    // Second, small case: a launch holding only deliberate pauses offers nothing.
    let (mut fresh, _rx) = make_app();
    let mut only_user = Task::new("still held", Some("planner".into()));
    assert!(only_user.pause_for(PauseReason::User));
    fresh.store.upsert_task(only_user);
    fresh.prepare_resume_prompt();
    assert!(
        fresh.resume_prompt().is_none(),
        "a deliberate pause must never open the reopen prompt"
    );
}

/// The pause keys are safe with nothing to act on: `s` invents no task and
/// `Space` still flips the gate.
#[test]
fn pause_keys_are_inert_without_a_task() {
    let (mut app, _rx) = make_app();

    press(&mut app, KeyCode::Char('s'));
    assert!(app.tasks().is_empty());
    assert!(app.log.iter().any(|line| line.contains("no task to pause")));

    press(&mut app, KeyCode::Char(' '));
    assert!(app.is_paused());
    press(&mut app, KeyCode::Char(' '));
    assert!(!app.is_paused());
}

/// The queue panel shows a paused engine, and a paused task reads `paused`.
#[test]
fn renders_paused_engine_and_task() {
    let (mut app, _rx) = make_app();
    app.store.paused = true;

    let text = screen(&app, 120, 40);
    assert!(text.contains("⏸ paused"), "engine pause indicator missing");

    app.store.paused = false;
    let mut held = Task::new("hold me", Some("researcher".into()));
    assert!(held.pause());
    app.store.upsert_task(held);

    let text = screen(&app, 120, 40);
    assert!(text.contains("paused"), "paused task status missing");
}

/// A prompt typed while the employee is mid-turn is queued (not dropped) and
/// still lands in the transcript and recall history right away.
#[test]
fn send_prompt_queues_when_employee_is_busy() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0); // a live session…
    app.team.set_status(0, EmployeeStatus::Thinking); // …but a turn in flight

    app.input = "second thought".into();
    app.send_prompt();

    assert_eq!(app.chats[&0].queue_running, vec!["second thought"]);
    assert_eq!(app.chats[&0].history, vec!["second thought"]);
    assert!(
        app.chats[&0]
            .transcript
            .iter()
            .any(|line| line.text == "second thought"),
        "queued prompt should appear in the transcript immediately"
    );
}

/// `next_queued` only hands out a prompt when the employee is idle *and* has a
/// live session, one prompt at a time.
#[test]
fn next_queued_requires_idle_and_ready_session() {
    let (mut app, _rx) = make_app();
    app.chats
        .entry(0)
        .or_default()
        .queue_running
        .push("queued".into());

    // No session yet → nothing drains.
    assert_eq!(app.next_queued(0), None);
    assert_eq!(app.chats[&0].queue_running, vec!["queued"]);

    // Live session, but a busy employee → still nothing.
    app.chats.get_mut(&0).unwrap().session_id = Some("ses_1".into());
    app.team.set_status(0, EmployeeStatus::Working);
    assert_eq!(app.next_queued(0), None);
    assert_eq!(app.chats[&0].queue_running, vec!["queued"]);

    // Idle with a session → the oldest prompt drains, then the queue is empty.
    app.team.set_status(0, EmployeeStatus::Idle);
    assert_eq!(app.next_queued(0), Some("queued".to_string()));
    assert!(app.chats[&0].queue_running.is_empty());
    assert_eq!(app.next_queued(0), None);
}

/// Without a client, pumping is a silent no-op and the queue is left intact.
#[test]
fn pump_is_a_noop_without_a_client() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.chats
        .get_mut(&0)
        .unwrap()
        .queue_running
        .push("held".into());

    app.pump_employee_queue(0);

    assert_eq!(app.chats[&0].queue_running, vec!["held"]);
}

/// The roster status after a turn comes from the last assistant (or idle)
/// message: a clean finish is `Done`, an errored one is `Error`, and a
/// transcript with no such message leaves the employee `Idle`.
#[test]
fn completion_status_reads_the_last_assistant_message() {
    let messages = |assistant: serde_json::Value| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": "u", "time": {"created": 1}, "text": "go", "type": "user"},
            assistant,
        ]))
        .unwrap()
    };

    assert_eq!(
        completion_status(&messages(serde_json::json!({
            "id": "m", "time": {"created": 2, "completed": 3}, "type": "assistant",
            "content": [{"type": "text", "text": "done"}], "finish": "done",
        }))),
        EmployeeStatus::Done
    );

    // `finish: "error"` and an explicit error body both grade as `Error`.
    assert_eq!(
        completion_status(&messages(serde_json::json!({
            "id": "m", "time": {"created": 2, "completed": 3}, "type": "assistant",
            "content": [{"type": "text", "text": "half"}], "finish": "error",
        }))),
        EmployeeStatus::Error
    );
    assert_eq!(
        completion_status(&messages(serde_json::json!({
            "id": "m", "time": {"created": 2, "completed": 3}, "type": "assistant",
            "content": [{"type": "text", "text": "half"}],
            "error": {"message": "budget exceeded"},
        }))),
        EmployeeStatus::Error
    );

    // The last turn wins over an earlier one.
    let two_turns: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id": "u1", "time": {"created": 1}, "text": "go", "type": "user"},
        {"id": "m1", "time": {"created": 2, "completed": 3}, "type": "assistant",
         "content": [{"type": "text", "text": "half"}], "finish": "error"},
        {"id": "u2", "time": {"created": 4}, "text": "retry", "type": "user"},
        {"id": "m2", "time": {"created": 5, "completed": 6}, "type": "assistant",
         "content": [{"type": "text", "text": "done"}], "finish": "done"},
    ]))
    .unwrap();
    assert_eq!(completion_status(&two_turns), EmployeeStatus::Done);

    // A user-only transcript has no completed turn to grade.
    let user_only: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id": "u", "time": {"created": 1}, "text": "go", "type": "user"},
    ]))
    .unwrap();
    assert_eq!(completion_status(&user_only), EmployeeStatus::Idle);
}

/// "Busy" has one definition: the app's per-employee check and the roster's
/// aggregate predicate agree, and a terminal status frees the employee again.
#[test]
fn busy_predicate_is_shared_between_app_and_team() {
    let (mut app, _rx) = make_app();
    assert!(!app.employee_is_busy(0));
    assert!(!app.team.is_busy());

    app.team.set_status(0, EmployeeStatus::Working);
    assert!(app.employee_is_busy(0));
    assert!(app.team.is_busy());

    app.team.set_status(0, EmployeeStatus::Done); // terminal → free again
    assert!(!app.employee_is_busy(0));
    assert!(!app.team.is_busy());

    // An out-of-range index is simply "not busy".
    assert!(!app.employee_is_busy(99));
}

// --- chat guards (one definition of "ready", one of "queue it") ---

/// `Chat`'s own readiness predicates: a session id without a creation in flight
/// is ready, and either one alone means "a session exists or is on its way".
#[test]
fn chat_readiness_predicates() {
    let mut chat = Chat::default();
    assert!(!chat.ready());
    assert!(!chat.started_or_starting());

    chat.creating = true;
    assert!(!chat.ready());
    assert!(chat.started_or_starting());

    chat.session_id = Some("ses_1".into());
    chat.creating = false;
    assert!(chat.ready());
    assert!(chat.started_or_starting());
}

/// `can_prompt` is the single gate both queue points use: it needs a ready
/// session *and* an idle employee, so a session-less or mid-turn employee is
/// queued instead of sent to.
#[test]
fn can_prompt_needs_a_ready_session_and_an_idle_employee() {
    let (mut app, _rx) = make_app();
    assert!(!app.can_prompt(0), "no chat at all");

    // A session still being created is not ready to take a prompt.
    app.chats.entry(0).or_default().creating = true;
    assert!(!app.session_ready(0));
    assert!(!app.can_prompt(0));

    // Live session and idle: the one state that sends immediately.
    let chat = app.chats.get_mut(&0).unwrap();
    chat.creating = false;
    chat.session_id = Some("ses_1".into());
    app.team.set_status(0, EmployeeStatus::Idle);
    assert!(app.can_prompt(0));

    // Mid-turn: queued again, however ready the session is.
    app.team.set_status(0, EmployeeStatus::Working);
    assert!(app.session_ready(0));
    assert!(!app.can_prompt(0));
}

/// Queueing asks for a session unconditionally — `begin_session` is its own
/// guard, so a second prompt while one is being created neither starts another
/// nor disturbs the queue.
#[test]
fn enqueue_prompt_starts_a_session_once_and_keeps_the_queue() {
    let (mut app, _rx) = make_app();
    app.enqueue_prompt(0, "first".into());
    assert!(app.chats[&0].creating, "a session is on its way");
    assert!(app.session_id_of(0).is_none());

    app.enqueue_prompt(0, "second".into());
    assert_eq!(app.chats[&0].queue_running, vec!["first", "second"]);
    assert!(app.chats[&0].creating);

    // With a live session, asking to begin is a no-op in the other direction too.
    let chat = app.chats.get_mut(&0).unwrap();
    chat.creating = false;
    chat.session_id = Some("ses_1".into());
    app.begin_session(0);
    assert_eq!(app.session_id_of(0).as_deref(), Some("ses_1"));
}

// --- delegation-chain guards ---

/// An employee the chain maps have never seen is its own origin at depth 0, and
/// `inherit_chain` carries the origin down while bumping the depth — so `abandon`
/// can tell "the origin is me" from "a distinct origin to report to".
#[test]
fn chain_origin_and_depth_default_then_inherit() {
    let (mut app, _rx) = make_app();
    assert_eq!(app.chain_origin(2), 2);
    assert_eq!(app.chain_depth(2), 0);

    app.inherit_chain(2, 1);
    assert_eq!(app.chain_origin(1), 2);
    assert_eq!(app.chain_depth(1), 1);

    // The origin is inherited, not re-rooted, however deep the chain goes.
    app.inherit_chain(1, 3);
    assert_eq!(app.chain_origin(3), 2);
    assert_eq!(app.chain_depth(3), 2);
}

// --- fan-out contribution guard ---

/// A worker owes a fan-out exactly one contribution: the guard shared by the
/// real and the synthetic path stops offering the fan-out once the worker is on
/// record, and a fan-out that has already been aggregated is never offered again.
#[test]
fn a_worker_owes_a_fanout_once() {
    let (mut app, _rx) = make_app();
    let planner = employee(&app, "planner");
    let orchestrator = employee(&app, "orchestrator");
    open_chat(&mut app, planner); // the planner; the orchestrator aggregates for it
    app.fan_out(planner, "ship it".to_string());

    let id = *app.fanouts.keys().next().unwrap();
    let workers = app.fanouts[&id].workers.clone();
    assert_eq!(workers.len(), 3, "everyone but the source and aggregator");
    assert!(
        !workers.contains(&orchestrator),
        "the root aggregates, so it is not a worker"
    );
    let first = workers[0];
    assert_eq!(app.fanouts_awaiting(first), vec![id]);

    app.force_worker_contribution(first, "⚠ gone".to_string());
    assert!(app.fanouts_awaiting(first).is_empty(), "already on record");
    assert_eq!(app.fanouts[&id].outputs[&first], "⚠ gone");

    // The source and the aggregator are not workers, so they owe nothing.
    assert!(app.fanouts_awaiting(planner).is_empty(), "the source");
    assert!(
        app.fanouts_awaiting(orchestrator).is_empty(),
        "the aggregator"
    );

    // The remaining workers' contributions complete the fan-out: aggregated, and
    // owed by nobody afterwards.
    for worker in &workers[1..] {
        app.force_worker_contribution(*worker, "Draft copy.".to_string());
    }
    assert!(app.fanouts[&id].aggregated);
    for worker in &workers {
        assert!(app.fanouts_awaiting(*worker).is_empty());
    }
}

// --- mouse ---

/// A model catalog of `count` entries, as OpenCode lists them.
fn model_list(count: usize) -> Vec<ProviderModel> {
    (0..count)
        .map(|i| {
            serde_json::from_value(serde_json::json!({
                "id": format!("m{i:02}"),
                "name": format!("Model {i:02}"),
                "providerID": "kenari",
            }))
            .unwrap()
        })
        .collect()
}

/// A project list of `count` entries, as the directory scan would find them.
fn project_list(count: usize) -> Vec<Project> {
    (0..count)
        .map(|i| Project {
            name: format!("proj{i:02}"),
            path: PathBuf::from(format!("/tmp/Projects/proj{i:02}")),
        })
        .collect()
}

/// A left-button press at a screen cell.
fn mouse_click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// A wheel notch at a screen cell.
fn mouse_wheel(column: u16, row: u16, up: bool) -> MouseEvent {
    MouseEvent {
        kind: if up {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        },
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// A pointer move, which is not input (motion is reported while capture is on).
fn mouse_move(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Moved,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// Where `needle` was drawn in a `width`×`height` frame, so a test can click what
/// it read on screen. Each cell contributes one character.
fn find_on_screen(app: &App, width: u16, height: u16, needle: &str) -> (u16, u16) {
    let text: Vec<char> = screen(app, width, height).chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    let index = text
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
        .unwrap_or_else(|| panic!("{needle:?} is not on screen"));
    (
        (index % width as usize) as u16,
        (index / width as usize) as u16,
    )
}

#[test]
fn clicking_a_grid_card_selects_that_employee() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);

    // The card the renderer drew is the card the click resolves to. The anchor is
    // the card's own title (its stage label, uppercased), because a name alone
    // would also match the tasks panel, which prints every name. The expected
    // employee is looked up by id, so the assertion survives the roster root
    // shifting every index by one.
    //
    // The orchestrator is deliberately absent: it is the strip above the grid, so
    // it has no card to click (see `the_orchestrator_has_no_grid_card`).
    for (stage, id) in [
        ("RESEARCHER", "researcher"),
        ("WRITER", "writer"),
        ("REVIEWER", "reviewer"),
        ("PLANNER", "planner"),
    ] {
        let expected = employee(&app, id);
        let (column, row) = find_on_screen(&app, 120, 40, &format!(" {stage} "));
        app.handle_mouse(mouse_click(column, row));
        assert_eq!(app.selected(), expected, "clicking the {stage} card");
    }

    // The strips above the grid are headers, not cards: neither resolves to an
    // employee.
    let root = employee(&app, "orchestrator");
    app.selected = root;
    let panel = ui::team_area((120, 40));
    app.handle_mouse(mouse_click(panel.x + 2, panel.y + 1));
    assert_eq!(
        app.selected(),
        root,
        "the Jev header has no employee to select"
    );
}

/// The orchestrator has no grid card: it is the strip above the grid. Its stage
/// label therefore appears exactly once on screen — in the strip — and never as a
/// card title, and no card in the grid belongs to it.
#[test]
fn the_orchestrator_has_no_grid_card() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);
    let root = employee(&app, "orchestrator");

    // No drawn card is the root, at any offset.
    for offset in 0..=app.roster_max_scroll() {
        app.roster_scroll = offset;
        let cards = ui::grid_cells(&app, (120, 40));
        assert!(
            cards.iter().all(|(employee, _)| *employee != root),
            "the orchestrator was drawn as a card at offset {offset}"
        );
    }
    app.roster_scroll = 0;

    // Its stage label is not a card title anywhere on screen, and the root is not
    // clickable as an employee.
    let frame = screen(&app, 120, 40);
    assert!(
        !frame.contains(" ORCHESTRATOR "),
        "the root is drawn as a card: {frame}"
    );
    for (employee, rect) in ui::grid_cells(&app, (120, 40)) {
        assert_eq!(
            ui::hit_target_at(&app, (120, 40), rect.x + 1, rect.y + 1),
            Some(ui::Target::Employee(employee)),
            "card {rect:?} resolves to its own employee"
        );
    }

    // The root *is* on screen — as the strip, by name.
    assert!(
        frame.contains("Otto"),
        "the root's strip is missing: {frame}"
    );
}

/// G6: every drawn card resolves to its own employee at any scroll offset, and
/// the title row, the strip/bus, and the gaps between cards resolve to nothing.
#[test]
fn grid_hit_testing_matches_the_drawn_cards_at_any_offset() {
    let (mut app, _rx) = make_app();
    app.team = long_team(12);
    // 120×24: a 64-column team panel (three 20-cell cards per row) only two card
    // rows tall, so the twelve employees overflow into four rows.
    let size = (120, 24);
    app.last_size = size;
    let max = app.roster_max_scroll();
    assert!(max > 0, "the fixture must overflow the viewport");

    for offset in [0, 1, max] {
        app.roster_scroll = offset;

        let cells = ui::grid_cells(&app, size);
        assert_eq!(cells.len() % 3, 0, "only whole rows are drawn");
        for (employee, rect) in &cells {
            let (x, y) = (rect.x + 2, rect.y + 2); // inside the border
            assert_eq!(
                ui::hit_target_at(&app, size, x, y),
                Some(ui::Target::Employee(*employee)),
                "cell {rect:?} at offset {offset}"
            );
        }

        let area = ui::team_area(size);
        // The title row is never a card.
        assert_eq!(ui::hit_target_at(&app, size, area.x + 1, area.y), None);
        // The strip and bus sit between the title and the first card row, and
        // neither is a card.
        let first = cells[0].1;
        assert_eq!(
            first.y,
            area.y + 3,
            "the strip and bus are drawn above the grid"
        );
        assert_eq!(ui::hit_target_at(&app, size, area.x + 1, area.y + 1), None);
        assert_eq!(ui::hit_target_at(&app, size, area.x + 1, area.y + 2), None);

        // The one-column gap between the first two cards is not a card.
        let second = cells[1].1;
        assert_eq!(second.y, first.y, "three cards share the first row");
        assert_eq!(
            ui::hit_target_at(&app, size, first.right(), first.y + 2),
            None,
            "the column gap is not a card"
        );
    }
}

/// G7/G6: at 80×24 the team panel is 24 columns wide, so the grid falls back to
/// one column — and hit-testing follows that fallback rather than the 3-column
/// assumption.
#[test]
fn narrow_panel_hit_tests_one_column_cards() {
    let (mut app, _rx) = make_app();
    let size = (80, 24);
    app.last_size = size;

    let cells = ui::grid_cells(&app, size);
    assert!(!cells.is_empty(), "at least one card row fits");
    for pair in cells.windows(2) {
        assert_eq!(pair[0].1.x, pair[1].1.x, "one column");
        assert!(pair[1].1.y > pair[0].1.y, "cards stack vertically");
    }
    // The single column keeps its one-column riser to its left, so the card is
    // the panel minus that gutter.
    let panel = ui::team_area(size);
    assert!(
        cells
            .iter()
            .all(|(_, rect)| rect.width == panel.width.saturating_sub(1)),
        "the single column spans the panel less its riser"
    );
    assert!(
        cells.iter().all(|(_, rect)| rect.x == panel.x + 1),
        "the riser is the panel's first column"
    );
    for (employee, rect) in &cells {
        assert_eq!(
            ui::hit_target_at(&app, size, rect.x + 2, rect.y + 2),
            Some(ui::Target::Employee(*employee))
        );
    }
}

#[test]
fn clicking_outside_a_target_keeps_the_selection() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);
    app.selected = 2;

    // Title bar, hint bar, sidebar, tasks panel, empty space below the grid.
    for (column, row) in [(0, 0), (119, 39), (2, 20), (119, 8), (40, 2)] {
        app.handle_mouse(mouse_click(column, row));
    }
    assert_eq!(app.selected(), 2);
}

/// A roster of `count` generalist employees — long enough to overflow the panel
/// at the sizes these tests use, and in workflow order already (all `General`).
fn long_team(count: usize) -> Team {
    let mut team = Team::default();
    for i in 0..count {
        team.add_employee(crate::team::Employee::new(
            format!("e{i:02}"),
            format!("Emp {i:02}"),
            "Generalist",
        ))
        .expect("unique ids");
    }
    team
}

/// G4: the wheel scrolls the grid's viewport and clamps at both ends — it never
/// moves the selection, so a stray notch can't change what `Enter` opens.
#[test]
fn wheel_scrolls_the_employee_grid() {
    let (mut app, _rx) = make_app();
    app.team = long_team(12);
    app.last_size = (80, 24);
    let max = app.roster_max_scroll();
    assert!(max > 0, "the fixture must overflow the viewport");
    let selected = app.selected();

    app.handle_mouse(mouse_wheel(60, 20, false));
    assert_eq!(app.roster_scroll(), 1, "a notch scrolls one row down");
    assert_eq!(app.selected(), selected, "the wheel never moves the cursor");

    for _ in 0..max + 5 {
        app.handle_mouse(mouse_wheel(60, 20, false));
    }
    assert_eq!(app.roster_scroll(), max, "clamped to the last full page");

    for _ in 0..max + 5 {
        app.handle_mouse(mouse_wheel(60, 20, true));
    }
    assert_eq!(app.roster_scroll(), 0, "and back at the top");
    assert_eq!(app.selected(), selected);
}

/// G3: a resize that shortens the viewport (or a shorter team) cannot leave the
/// offset past the last page.
#[test]
fn roster_scroll_reclamps_after_a_resize() {
    let (mut app, _rx) = make_app();
    app.team = long_team(12);
    app.last_size = (80, 24); // one column, two rows visible: plenty to scroll
    let max = app.roster_max_scroll();
    assert!(max > 0);
    app.roster_scroll = max;

    // A terminal wide and tall enough for all four card rows leaves nothing to
    // scroll.
    app.update(Message::Resize(160, 60));
    assert_eq!(app.roster_max_scroll(), 0);
    assert_eq!(app.roster_scroll(), 0, "the accessor clamps a stale offset");

    // A resize that makes the viewport shorter than before re-clamps to the new
    // last page rather than leaving the old offset stranded past the end.
    app.update(Message::Resize(120, 20));
    let shrunk = app.roster_max_scroll();
    app.roster_scroll = usize::MAX;
    assert_eq!(app.roster_scroll(), shrunk);
}

/// G5: arrows move by one cell across the row, and by a whole row up/down —
/// wrapping at the edges, as the plan specifies.
#[test]
fn arrows_move_around_the_grid_and_reveal_the_selection() {
    let (mut app, _rx) = make_app();
    // 120×24: a 64-column team panel (three 20-cell cards per row) that is only
    // two card rows tall, so a 30-strong roster has to scroll.
    app.team = long_team(30);
    app.last_size = (120, 24);
    assert!(app.roster_max_scroll() > 0, "the fixture must overflow");
    app.selected = 0;

    press(&mut app, KeyCode::Right);
    assert_eq!(app.selected(), 1);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.selected(), 0);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.selected(), 0, "clamped at the first cell");

    // Down is one row: three cells on.
    press(&mut app, KeyCode::Down);
    assert_eq!(app.selected(), 3);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.selected(), 0);

    // The selection is always revealed: walking to the bottom scrolls the
    // viewport just far enough, and never past the last page.
    for _ in 0..40 {
        press(&mut app, KeyCode::Down);
    }
    assert_eq!(app.selected(), 29, "clamped at the last cell");
    assert_eq!(app.roster_scroll(), app.roster_max_scroll());

    for _ in 0..40 {
        press(&mut app, KeyCode::Up);
    }
    assert_eq!(app.selected(), 0);
    assert_eq!(app.roster_scroll(), 0, "and back to the top of the grid");
}

/// The plan's row-edge rule: moving left off the first cell of a row lands on
/// the last cell of the row above (the grid is read as one linear sequence), and
/// right off the last cell of a row continues onto the next row.
#[test]
fn arrows_wrap_at_row_edges() {
    let (mut app, _rx) = make_app();
    app.team = long_team(12);
    app.last_size = (120, 40); // three columns, all rows visible

    app.selected = 3; // first cell of the second row
    press(&mut app, KeyCode::Left);
    assert_eq!(app.selected(), 2, "left of the first cell is the row above");
    press(&mut app, KeyCode::Right);
    assert_eq!(
        app.selected(),
        3,
        "and right returns to the row's first cell"
    );

    app.selected = 2; // last cell of the first row
    press(&mut app, KeyCode::Right);
    assert_eq!(app.selected(), 3, "right of the last cell is the row below");

    // Vertically the column is kept: up from the second row is one row up.
    app.selected = 4;
    press(&mut app, KeyCode::Up);
    assert_eq!(app.selected(), 1);
}

/// G5: `PgUp`/`PgDn` move by one viewport of rows (`±visible_rows` cells, as the
/// plan pins it), clamped and revealed.
#[test]
fn page_keys_move_by_a_viewport() {
    let (mut app, _rx) = make_app();
    app.team = long_team(30);
    app.last_size = (120, 24); // three cards per row, two rows on screen
    let page = app.roster_page() as usize;
    assert!(page > 0);

    app.selected = 0;
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.selected(), page, "a page is one viewport of rows");
    // The reveal rule keeps it on screen: the viewport followed, and the
    // selected card is among the ones drawn.
    let drawn: Vec<usize> = ui::grid_cells(&app, app.last_size)
        .into_iter()
        .map(|(employee, _)| employee)
        .collect();
    assert!(
        drawn.contains(&app.selected()),
        "the selection must be on screen after a page move"
    );

    press(&mut app, KeyCode::PageUp);
    assert_eq!(app.selected(), 0);
    assert_eq!(app.roster_scroll(), 0);
}

#[test]
fn wheel_scrolls_the_chat_transcript() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    assert_eq!(app.chat_scroll(), 0);

    app.handle_mouse(mouse_wheel(40, 10, true));
    assert!(app.chat_scroll() > 0, "wheel up pages back through history");

    app.handle_mouse(mouse_wheel(40, 10, false));
    assert_eq!(app.chat_scroll(), 0, "wheel down returns to the live tail");
}

#[test]
fn wheel_steps_the_pickers() {
    let (mut app, _rx) = make_app();
    app.projects = project_list(4);
    app.models = model_list(4);

    app.mode = Mode::Projects;
    app.handle_mouse(mouse_wheel(40, 10, false));
    assert_eq!(app.project_selected(), 1);
    app.handle_mouse(mouse_wheel(40, 10, true));
    assert_eq!(app.project_selected(), 0);
    assert!(
        app.active_project().is_none(),
        "scrolling never adopts a project"
    );

    app.mode = Mode::Models;
    app.handle_mouse(mouse_wheel(40, 10, false));
    assert_eq!(app.model_selected(), 1);
    app.handle_mouse(mouse_wheel(40, 10, true));
    assert_eq!(app.model_selected(), 0);
}

#[test]
fn clicking_a_project_row_selects_it_without_adopting_it() {
    let (mut app, _rx) = make_app();
    app.mode = Mode::Projects;
    app.projects = project_list(12);
    app.last_size = (100, 30);

    let (column, row) = find_on_screen(&app, 100, 30, "proj07");
    app.handle_mouse(mouse_click(column, row));

    assert_eq!(app.project_selected(), 7);
    assert!(
        app.active_project().is_none(),
        "Enter still adopts the project"
    );
}

/// The picker scrolls to the window hit-testing assumes: with the selection deep
/// in a list too long for the panel, the row under the highlight is the selected
/// item (and the row above it is the item before).
#[test]
fn clicking_a_scrolled_row_selects_the_item_drawn_there() {
    let (mut app, _rx) = make_app();
    app.mode = Mode::Projects;
    app.projects = project_list(12);
    app.project_selected = 11;
    app.last_size = (60, 16); // the panel shows four of the twelve rows

    let (column, row) = find_on_screen(&app, 60, 16, "▶");
    app.handle_mouse(mouse_click(column, row));
    assert_eq!(app.project_selected(), 11, "the highlight selects itself");

    let (column, row) = find_on_screen(&app, 60, 16, "proj09");
    app.handle_mouse(mouse_click(column, row));
    assert_eq!(app.project_selected(), 9);
}

#[test]
fn clicking_a_model_row_selects_it_without_assigning_it() {
    let (mut app, _rx) = make_app();
    app.mode = Mode::Models;
    app.models = model_list(12);
    app.last_size = (100, 30);
    let assigned = app.team.employees()[0].model.clone();

    let (column, row) = find_on_screen(&app, 100, 30, "m07");
    app.handle_mouse(mouse_click(column, row));

    assert_eq!(app.model_selected(), 7);
    assert_eq!(
        app.team.employees()[0].model,
        assigned,
        "Enter still assigns the model"
    );
}

#[test]
fn a_click_dismisses_the_help_overlay_without_reaching_the_panel() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);
    app.selected = 0;

    // Find a point inside an employee's node, then cover it with the overlay.
    let (column, row) = find_on_screen(&app, 120, 40, " WRITER ");
    app.show_help = true;

    app.handle_mouse(mouse_click(column, row));
    assert!(!app.show_help());
    assert_eq!(app.selected(), 0, "the overlay swallowed the click");
}

#[test]
fn mouse_motion_is_not_input() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);

    let (column, row) = find_on_screen(&app, 120, 40, " WRITER ");
    app.show_help = true;
    app.handle_mouse(mouse_move(column, row));
    assert!(app.show_help(), "motion must not dismiss the overlay");

    app.show_help = false;
    app.handle_mouse(mouse_move(column, row));
    assert_eq!(app.selected(), 0, "motion must not move the selection");
}

/// A live-shaped `form.created` frame is answered end-to-end: the nested
/// `data.form` is parsed, each field is resolved, and a reply is POSTed back to
/// the session's form endpoint. Regression — the form used to be missed (the
/// payload was read from the wrong level), so a session paused on a question
/// hung forever.
#[tokio::test]
async fn form_created_is_answered_from_the_nested_payload() {
    use std::sync::{Arc, Mutex};

    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};

    /// Records each form reply the fake OpenCode service receives.
    #[derive(Clone, Default)]
    struct Recorder {
        replies: Arc<Mutex<Vec<RecordedReply>>>,
    }

    /// `(session, form, body, x-opencode-directory)` per reply.
    type RecordedReply = (String, String, serde_json::Value, Option<String>);

    async fn reply(
        State(recorder): State<Recorder>,
        Path((session, form)): Path<(String, String)>,
        headers: axum::http::HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> axum::http::StatusCode {
        let directory = headers
            .get("x-opencode-directory")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        recorder
            .replies
            .lock()
            .unwrap()
            .push((session, form, body, directory));
        // The live endpoint settles the form with `204 No Content`.
        axum::http::StatusCode::NO_CONTENT
    }

    let recorder = Recorder::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/api/session/{session}/form/{form}/reply", post(reply))
        .with_state(recorder.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = OpenCode::with_base(
        reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
        "pw",
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(16);

    // The exact frame OpenCode v2.0.32 emits: the form is nested under `data.form`
    // and there is no top-level `sessionID`.
    let event: Event = serde_json::from_str(
        r#"{"id":"evt_1","created":1792293021060,"type":"form.created",
            "location":{"directory":"/x"},
            "data":{"form":{"id":"frm_123","sessionID":"ses_1",
                "title":"How should I proceed?",
                "fields":[
                    {"key":"confirm","title":"Proceed?","type":"boolean"},
                    {"key":"choice","title":"Pick one","type":"string",
                     "options":[{"value":"a","label":"Alpha"},{"value":"b","label":"Beta"}]}
                ]}}}"#,
    )
    .unwrap();

    spawn_form_answer(client, None, event, tx);

    // Wait for the "answered form" log — proof the reply round-tripped.
    let mut log = String::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
            Ok(Some(Message::Log(line))) if line.starts_with("answered form") => {
                log = line;
                break;
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        }
    }
    assert!(
        log.contains("answered form"),
        "no answer was logged: {log:?}"
    );

    let replies = recorder.replies.lock().unwrap();
    assert_eq!(replies.len(), 1, "exactly one reply");
    assert_eq!(replies[0].0, "ses_1");
    assert_eq!(replies[0].1, "frm_123");
    // A session-scoped form needs no location header.
    assert_eq!(replies[0].3, None);
    let answer = &replies[0].2["answer"];
    // No Jev client → the deterministic defaults apply (boolean → true, the
    // first option for an option field).
    assert_eq!(answer["confirm"], serde_json::json!(true));
    assert_eq!(answer["choice"], serde_json::json!("a"));
}

/// A *global* `form.created` (`sessionID: "global"`) is answered with the
/// `x-opencode-directory` header naming the event's location. OpenCode's global
/// form store is location-scoped; without the header the server falls back to
/// its own cwd and 404s (`FormNotFoundError`) — the live failure this guards.
#[tokio::test]
async fn global_form_is_answered_with_its_location_header() {
    use std::sync::{Arc, Mutex};

    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};

    #[derive(Clone, Default)]
    struct Recorder {
        replies: Arc<Mutex<Vec<RecordedReply>>>,
    }

    /// `(session, form, x-opencode-directory)` per reply.
    type RecordedReply = (String, String, Option<String>);

    async fn reply(
        State(recorder): State<Recorder>,
        Path((session, form)): Path<(String, String)>,
        headers: axum::http::HeaderMap,
        Json(_body): Json<serde_json::Value>,
    ) -> axum::http::StatusCode {
        let directory = headers
            .get("x-opencode-directory")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        recorder
            .replies
            .lock()
            .unwrap()
            .push((session, form, directory));
        axum::http::StatusCode::NO_CONTENT
    }

    let recorder = Recorder::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/api/session/{session}/form/{form}/reply", post(reply))
        .with_state(recorder.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = OpenCode::with_base(
        reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
        "pw",
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(16);

    // A live-shaped global form: `sessionID` is the literal "global" and the
    // event carries the owning location.
    let event: Event = serde_json::from_str(
        r#"{"id":"evt_1","created":1792293021060,"type":"form.created",
            "location":{"directory":"/Users/me/Projects/app"},
            "data":{"form":{"id":"frm_9","sessionID":"global",
                "title":"Proceed?",
                "fields":[{"key":"confirm","title":"Proceed?","type":"boolean"}]}}}"#,
    )
    .unwrap();

    spawn_form_answer(client, None, event, tx);

    let mut log = String::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
            Ok(Some(Message::Log(line))) if line.starts_with("answered form") => {
                log = line;
                break;
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        }
    }
    assert!(
        log.contains("answered form"),
        "no answer was logged: {log:?}"
    );

    let replies = recorder.replies.lock().unwrap();
    assert_eq!(replies.len(), 1, "exactly one reply");
    assert_eq!(replies[0].0, "global");
    assert_eq!(replies[0].1, "frm_9");
    // `/` percent-encodes to `%2F`, matching OpenCode's own `encodeURIComponent`.
    assert_eq!(
        replies[0].2.as_deref(),
        Some("%2FUsers%2Fme%2FProjects%2Fapp")
    );
}

// --- sidebar vitals (SB2, SB4) -------------------------------------------------

/// SB2: `Pulse` keeps the last 48 tick samples, pushes 0 on an idle tick, and
/// resets cleanly.
#[test]
fn pulse_keeps_the_last_48_ticks() {
    let (mut app, _rx) = make_app();
    assert_eq!(app.pulse(), &[] as &[u64], "a fresh app has no samples");

    // An idle tick records a zero rather than skipping the slot: the sparkline's
    // x-axis is ticks, so a quiet tick has to occupy a column.
    app.advance();
    assert_eq!(app.pulse(), &[0], "an idle tick pushes 0");

    // The window is capped at CAPACITY, oldest first. The literal is part of the
    // criterion (§5 SB2: "the last 48 tick samples"), so it is asserted directly
    // — a test that only used `Pulse::CAPACITY` would pass whatever the constant
    // became.
    assert_eq!(Pulse::CAPACITY, 48, "SB2 pins the window at 48 samples");
    for tick in 0..(Pulse::CAPACITY + 12) {
        app.record_pulse(tick as u64);
    }
    assert_eq!(app.pulse().len(), 48);
    let expected: Vec<u64> = (12..(Pulse::CAPACITY + 12) as u64).collect();
    assert_eq!(
        app.pulse(),
        expected.as_slice(),
        "the oldest samples are dropped, not the newest"
    );

    // `tick` records the delta since the previous tick, not the running total.
    let mut pulse = Pulse::default();
    pulse.tick(5);
    pulse.tick(5);
    pulse.tick(9);
    assert_eq!(
        pulse.samples(),
        &[5, 0, 4],
        "one sample per tick, as a delta"
    );

    // Reset clears both the window and the baseline.
    pulse.reset();
    assert!(pulse.samples().is_empty());
    pulse.tick(3);
    assert_eq!(pulse.samples(), &[3], "the baseline was cleared too");
}

/// SB4: the aggregate accessors agree with a synthetic team and task set.
#[test]
fn aggregate_accessors_count_states() {
    // A connected app, so the aggregate mood is `Idle` rather than the
    // `connecting` fallback — every other render test in the suite runs
    // disconnected, which left that branch unexercised.
    let (mut app, _rx) = make_app();
    app.update(Message::Connected {
        client: OpenCode::with_base(
            reqwest::Url::parse("http://127.0.0.1:1").expect("a valid base url"),
            "sekrit",
        ),
        info: ServerInfo {
            version: "2.0.32".into(),
            pid: None,
            urls: Vec::new(),
        },
    });
    assert!(app.is_connected());

    // A fresh roster: five idle employees (the orchestrator plus the four
    // pipeline roles), no chats, no tasks.
    assert_eq!(app.team().len(), 5);
    assert_eq!(app.busy_count(), 0);
    assert_eq!(app.thinking_count(), 0);
    assert_eq!(app.live_sessions(), 0);
    assert_eq!(app.queued_prompts(), 0);
    assert_eq!(app.task_stats(), TaskStats::default());

    // Two busy employees, one of them specifically thinking. `thinking_count` is
    // a subset of `busy_count`, and `Working` counts as busy but not thinking.
    app.team.set_status(0, EmployeeStatus::Thinking);
    app.team.set_status(1, EmployeeStatus::Working);
    assert_eq!(app.busy_count(), 2);
    assert_eq!(app.thinking_count(), 1);

    // Terminal states are not busy: the turn is over.
    app.team.set_status(2, EmployeeStatus::Done);
    app.team.set_status(3, EmployeeStatus::Error);
    assert_eq!(app.busy_count(), 2, "done and error are not busy");
    assert_eq!(app.thinking_count(), 1);

    // Sessions and queued prompts are counted per chat.
    open_chat(&mut app, 0);
    open_chat(&mut app, 1);
    app.chats.entry(0).or_default().queue_running = vec!["one".into(), "two".into()];
    app.chats.entry(1).or_default().queue_running = vec!["three".into()];
    assert_eq!(app.live_sessions(), 2);
    assert_eq!(app.queued_prompts(), 3);

    // The task roll-up counts each status into its own bucket, and the derived
    // totals follow.
    let pending = Task::new("sweep the inbox", Some("researcher".into()));
    let mut running = Task::new("write the changelog", Some("writer".into()));
    assert!(running.transition(TaskStatus::Running));
    let mut retrying = Task::new("retry the build", Some("writer".into()));
    assert!(retrying.transition(TaskStatus::Running));
    assert!(retrying.transition(TaskStatus::Retrying));
    let mut paused = Task::new("hold the release", None);
    assert!(paused.pause());
    let mut succeeded = Task::new("ship it", None);
    assert!(succeeded.transition(TaskStatus::Running));
    succeeded.succeed("done");
    let mut failed = Task::new("roll back", None);
    assert!(failed.transition(TaskStatus::Running));
    failed.fail("boom");

    for task in [pending, running, retrying, paused, succeeded, failed] {
        app.store.upsert_task(task);
    }
    let stats = app.task_stats();
    assert_eq!(stats.pending, 1);
    assert_eq!(stats.running, 1);
    assert_eq!(stats.retrying, 1);
    assert_eq!(stats.paused, 1);
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(
        stats.active(),
        4,
        "in-flight work is pending+running+retrying+paused"
    );
    assert_eq!(stats.finished(), 2, "terminal work is succeeded+failed");

    // The alert line reads the most recent failure's goal.
    assert_eq!(app.last_failure(), Some("roll back"));
}

/// SB3 / A10: uptime advances, and the sidebar's formatter turns it into the
/// coarse label the runtime row draws.
#[test]
fn uptime_advances() {
    let (app, _rx) = make_app();
    let first = app.uptime();
    std::thread::sleep(std::time::Duration::from_millis(12));
    assert!(
        app.uptime() > first,
        "uptime is monotonic (first {first:?}, now {:?})",
        app.uptime()
    );
    // A live app is under a minute old, which is exactly the `Ns` branch the
    // runtime row prints in a real session.
    assert!(app.uptime() < std::time::Duration::from_secs(60));
}
