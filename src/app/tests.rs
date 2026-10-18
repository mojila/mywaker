use super::*;

fn make_app() -> (App, tokio::sync::mpsc::Receiver<Message>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<Message>(16);
    (App::new(Config::default(), tx), rx)
}

/// Deliver one plain key press through the app's own key path.
fn press(app: &mut App, code: KeyCode) {
    app.update(Message::Key(KeyEvent::new(code, KeyModifiers::NONE)));
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
    open_chat(&mut app, 0);
    // The writer (index 1) already has a live session to receive into.
    app.chats.entry(1).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"Here's the plan.\n@writer draft the doc"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(0, messages);

    assert_eq!(app.delegation_count, 1);
    assert!(
        app.chats[&1]
            .transcript
            .iter()
            .any(|line| line.text.contains("draft the doc"))
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
    open_chat(&mut app, 3); // planner
    app.origin_of.insert(3, 3);

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"plan it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"done\n@user the final summary"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(3, messages);

    assert_eq!(app.delegation_count, 1);
    assert!(
        app.chats[&3]
            .transcript
            .iter()
            .any(|line| line.role == TranscriptRole::Assistant && line.text == "the final summary")
    );
}

/// Re-loading the same transcript does not re-dispatch the same hand-off.
#[test]
fn dispatch_delegations_is_idempotent_per_message_id() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 0);
    app.chats.entry(1).or_default().session_id = Some("ses_w".into());

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
/// aggregator (reviewer), registering a fan-out group and dispatching each.
#[test]
fn parallel_directive_fans_out_to_workers() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 3); // planner is the source
    app.origin_of.insert(3, 3);
    app.chats.entry(0).or_default().session_id = Some("ses_r".into());
    app.chats.entry(1).or_default().session_id = Some("ses_w".into());

    let messages: Vec<SessionMessage> = serde_json::from_value(serde_json::json!([
        {"id":"m1","time":{"created":1},"text":"ship it","type":"user"},
        {"id":"m2","time":{"created":2,"completed":3},"type":"assistant",
         "content":[{"type":"text","text":"Let's parallelize.\n@parallel build the launch plan"}],"finish":"done"}
    ]))
    .unwrap();

    app.handle_transcript(3, messages);

    // One fan-out, workers = researcher + writer, aggregator = reviewer.
    assert_eq!(app.fanouts.len(), 1);
    let (_, fanout) = app.fanouts.iter().next().unwrap();
    assert_eq!(fanout.workers, vec![0, 1]);
    assert_eq!(fanout.aggregator, 2);
    assert_eq!(fanout.goal, "build the launch plan");

    // The goal reached the workers…
    assert!(
        app.chats[&0]
            .transcript
            .iter()
            .any(|l| l.text.contains("build the launch plan"))
    );
    assert!(
        app.chats[&1]
            .transcript
            .iter()
            .any(|l| l.text.contains("build the launch plan"))
    );
    // …but not the aggregator (reviewer), which only synthesises later.
    assert!(!app.chats.contains_key(&2));
}

/// Once every worker contributes, the fan-out aggregates their outputs and
/// hands the synthesis to the reviewer.
#[test]
fn fan_out_aggregates_once_all_workers_finish() {
    let (mut app, _rx) = make_app();
    open_chat(&mut app, 3);
    app.origin_of.insert(3, 3);
    app.fan_out(3, "ship it".to_string());

    let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
            {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text": text}],"finish":"done"}
        ]))
        .unwrap()
    };

    // Researcher finishes; the fan-out is not yet complete.
    app.handle_transcript(0, assistant("r", "Market research findings."));
    assert!(!app.fanouts.values().next().unwrap().aggregated);

    // Writer finishes; now aggregation fires and the reviewer receives the
    // synthesized prompt (queued since the reviewer has no live session yet).
    app.handle_transcript(1, assistant("w", "Draft copy for the launch."));
    let (_, fanout) = app.fanouts.iter().next().unwrap();
    assert!(fanout.aggregated);

    let outbox = &app.chats[&2].outbox;
    assert_eq!(outbox.len(), 1);
    assert!(outbox[0].contains("Market research findings"));
    assert!(outbox[0].contains("Draft copy for the launch"));
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
    app.chats.get_mut(&0).unwrap().reasoning_text = "think".into();

    app.interrupt_current();

    let chat = &app.chats[&0];
    assert!(chat.streaming_text.is_empty());
    assert!(chat.reasoning_text.is_empty());
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

    assert_eq!(app.reasoning_text(), "thinking");
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
    // Starter storage order: researcher(0), writer(1), reviewer(2),
    // planner(3). Workflow order should be planner → researcher → writer →
    // reviewer.
    let ids: Vec<&str> = app
        .workflow_order()
        .iter()
        .map(|&i| app.team.employees()[i].id.as_str())
        .collect();
    assert_eq!(ids, vec!["planner", "researcher", "writer", "reviewer"]);
}

#[test]
fn renders_pipeline_with_mixed_statuses() {
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
    open_chat(&mut app, 3);
    app.origin_of.insert(3, 3);
    app.fan_out(3, "ship it".to_string());

    // Worker 0 goes silent past its deadline and has exhausted its budget.
    app.chats.entry(0).or_default().pending = Some(stale_pending("ship it", harness::MAX_ATTEMPTS));

    app.tick_reliability();

    // Worker 0 is abandoned → its ⚠ contribution is recorded, footed by its id.
    let fanout = app.fanouts.values().next().unwrap();
    assert!(fanout.outputs.contains_key(&0));
    assert!(!fanout.aggregated);

    // Worker 1 delivers; the fan-out then aggregates through the reviewer (2).
    let assistant = |id: &str, text: &str| -> Vec<SessionMessage> {
        serde_json::from_value(serde_json::json!([
            {"id": format!("{id}_u"), "time":{"created":1}, "text":"ship it","type":"user"},
            {"id": id, "time":{"created":2,"completed":3},"type":"assistant",
             "content":[{"type":"text","text": text}],"finish":"done"}
        ]))
        .unwrap()
    };
    app.handle_transcript(1, assistant("w", "Draft copy."));

    let fanout = app.fanouts.values().next().unwrap();
    assert!(fanout.aggregated);
    let outbox = &app.chats[&2].outbox;
    assert_eq!(outbox.len(), 1);
    assert!(outbox[0].contains("no deliverable")); // worker 0's ⚠
    assert!(outbox[0].contains("Draft copy."));
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

    let last = app.chats[&0].transcript.last().expect("a help line");
    assert_eq!(last.text, chat_help_text());
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

/// AC4: resuming a paused task returns it to `running` and re-dispatches it.
#[test]
fn resuming_a_paused_task_returns_it_to_running() {
    let (mut app, _rx) = make_app();
    let mut task = Task::new("sweep the inbox", Some("researcher".into()));
    assert!(task.transition(TaskStatus::Running));
    app.store.upsert_task(task);

    app.selected = app.team_index_by_id("researcher").expect("known employee");
    press(&mut app, KeyCode::Char('s')); // pause
    assert_eq!(app.store.tasks[0].status, TaskStatus::Paused);

    press(&mut app, KeyCode::Char('s')); // resume
    assert_eq!(app.store.tasks[0].status, TaskStatus::Running);
    assert!(!app.store.tasks[0].paused);
    assert!(app.log.iter().any(|line| line.contains("▶ resumed task")));
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
    open_chat(&mut app, 3); // the planner; the reviewer (2) aggregates for it
    app.fan_out(3, "ship it".to_string());

    let id = *app.fanouts.keys().next().unwrap();
    assert_eq!(app.fanouts[&id].workers, vec![0, 1]);
    assert_eq!(app.fanouts_awaiting(0), vec![id]);

    app.force_worker_contribution(0, "⚠ gone".to_string());
    assert!(app.fanouts_awaiting(0).is_empty(), "already on record");
    assert_eq!(app.fanouts[&id].outputs[&0], "⚠ gone");

    // The source and the aggregator are not workers, so they owe nothing.
    assert!(app.fanouts_awaiting(3).is_empty(), "the source");
    assert!(app.fanouts_awaiting(2).is_empty(), "the aggregator");

    // The last worker's contribution completes the fan-out: aggregated, and owed
    // by nobody afterwards.
    app.force_worker_contribution(1, "Draft copy.".to_string());
    assert!(app.fanouts[&id].aggregated);
    assert!(app.fanouts_awaiting(0).is_empty());
    assert!(app.fanouts_awaiting(1).is_empty());
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
fn clicking_a_pipeline_node_selects_that_employee() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);

    // The node the renderer drew is the node the click resolves to. The anchor is
    // the node's own title (its stage label, uppercased), because a name alone
    // would also match the tasks panel, which prints every name.
    for (stage, index) in [
        ("RESEARCHER", 0),
        ("WRITER", 1),
        ("REVIEWER", 2),
        ("PLANNER", 3),
    ] {
        let (column, row) = find_on_screen(&app, 120, 40, &format!(" {stage} "));
        app.handle_mouse(mouse_click(column, row));
        assert_eq!(app.selected(), index, "clicking the {stage} node");
    }

    // The shared Jev node stands for no employee: it is not a target.
    app.selected = 0;
    let (column, row) = find_on_screen(&app, 120, 40, "answers questions");
    app.handle_mouse(mouse_click(column, row));
    assert_eq!(app.selected(), 0, "the Jev node has no employee to select");
}

#[test]
fn clicking_outside_a_target_keeps_the_selection() {
    let (mut app, _rx) = make_app();
    app.last_size = (120, 40);
    app.selected = 2;

    // Title bar, hint bar, robot avatar, tasks panel, empty space above the tree.
    for (column, row) in [(0, 0), (119, 39), (2, 20), (119, 8), (40, 2)] {
        app.handle_mouse(mouse_click(column, row));
    }
    assert_eq!(app.selected(), 2);
}

#[test]
fn wheel_steps_the_roster_selection() {
    let (mut app, _rx) = make_app();

    // The starter stage order is planner, researcher, writer, reviewer, starting
    // on the researcher — the same walk ←/→ makes, clamping at both ends.
    app.handle_mouse(mouse_wheel(60, 20, false));
    assert_eq!(app.selected(), 1);
    app.handle_mouse(mouse_wheel(60, 20, true));
    assert_eq!(app.selected(), 0);

    for _ in 0..5 {
        app.handle_mouse(mouse_wheel(60, 20, true));
    }
    assert_eq!(app.selected(), 3, "the wheel stops at the first stage");
    for _ in 0..5 {
        app.handle_mouse(mouse_wheel(60, 20, false));
    }
    assert_eq!(app.selected(), 2, "and at the last");
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
