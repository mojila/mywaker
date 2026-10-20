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
        assert_eq!(
            rect.width,
            panel.width.saturating_sub(1),
            "one column spans the panel less its riser"
        );
    }
    // The Jev strip survives at 24 columns: it is a single short line.
    assert!(text.contains("jev"), "the Jev strip is drawn");
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

/// `screen`'s buffer split into rows of `width` cells. Splits on characters, not
/// bytes: a cell can hold a multi-byte glyph (`┄`, `◆`), and a byte chunk would
/// cut one in half.
fn rows(text: &str, width: u16) -> Vec<String> {
    let cells: Vec<char> = text.chars().collect();
    cells
        .chunks(width as usize)
        .map(|row| row.iter().collect())
        .collect()
}

/// The delegation diagram is drawn: the Jev strip, a dashed bus across the panel,
/// and one riser per card column — so the team's wiring is legible before
/// anything moves.
#[test]
fn renders_the_strip_bus_and_risers() {
    let app = app();
    let text = screen(&app, 120, 40);

    assert!(text.contains("jev"), "the Jev strip is missing");
    assert!(text.contains("┄"), "the dashed bus is missing");
    assert!(text.contains("┆"), "the risers are missing");

    // The bus spans the team panel, and a riser sits in each column's gutter.
    let cells = ui::grid_cells(&app, (120, 40));
    assert!(!cells.is_empty(), "the starter roster draws cards");
    let bus_row = rows(&text, 120)
        .into_iter()
        .find(|row| row.contains('┄'))
        .expect("a bus row exists");
    assert!(
        bus_row.chars().filter(|c| *c == '┄').count() > 1,
        "the bus is a line, not a dot: {bus_row}"
    );
}

/// A live hand-off draws a packet on the wires, in the trace's colour — and the
/// packet moves as the app advances.
#[test]
fn renders_a_delegation_packet() {
    let mut app = app();
    let planner = app
        .team()
        .employees()
        .iter()
        .position(|e| e.id == "planner")
        .expect("planner");
    let writer = app
        .team()
        .employees()
        .iter()
        .position(|e| e.id == "writer")
        .expect("writer");

    // Record a hand-off through the public model, then draw it.
    app.record_trace_for_test(planner, writer);

    // The packet leaves the source card, so it is on the wire once it has started
    // moving.
    app.advance();
    app.advance();
    let first = screen(&app, 120, 40);
    assert!(first.contains("┄"), "the lit wire is drawn");
    let panel = ui::team_area((120, 40));
    let bus_y = panel.y + 2; // title, strip, then the bus
    let packet_on_wire = |frame: &str| {
        rows(frame, 120)
            .iter()
            .skip(bus_y as usize)
            .take(panel.height as usize - 2)
            .any(|row| row.contains('◆'))
    };
    assert!(packet_on_wire(&first), "the packet is on the wire");

    // The packet advances: a few more ticks draw a different frame.
    for _ in 0..3 {
        app.advance();
    }
    let later = screen(&app, 120, 40);
    assert_ne!(first, later, "the packet did not move");

    // And once the trace expires the packet is gone from the wire.
    for _ in 0..20 {
        app.advance();
    }
    let expired = screen(&app, 120, 40);
    assert!(
        !packet_on_wire(&expired),
        "an expired packet is still drawn: {expired}"
    );
}

/// The card's tool row is drawn from the employee's `last_tool`: the name after
/// a dim `tool` label at 120×40, and the placeholder before anything ran.
#[test]
fn renders_the_tool_row_on_a_card() {
    let mut team = mywaker::team::Team::starter();
    team.set_last_tool(1, Some(mywaker::opencode::ToolUse::builtin("bash")));

    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    let app = App::with_team(Config::default(), tx, team);

    let text = screen(&app, 120, 40);
    assert!(
        text.contains("tool bash"),
        "the tool row is missing: {text}"
    );
    // The other cards have not called one, so they carry the placeholder.
    assert!(text.contains("tool -"), "the placeholder is missing");
}

/// A classified tool reads with its class prefix on the card. At 120×40 the
/// three-column card is 16 cells wide, so the prefix survives and the name is
/// ellipsised; at 160×50 the whole label fits (A5/A10).
#[test]
fn renders_a_classified_tool_label() {
    let mut team = mywaker::team::Team::starter();
    team.set_last_tool(
        1,
        Some(mywaker::opencode::ToolUse::new(
            mywaker::opencode::ToolClass::Skill,
            "opencode",
        )),
    );

    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    let app = App::with_team(Config::default(), tx, team);

    // Narrow card: the class is the part that must survive, because it is the
    // information the row exists to add.
    let narrow = screen(&app, 120, 40);
    assert!(
        narrow.contains("tool skill:"),
        "the class prefix is missing: {narrow}"
    );
    assert!(
        !narrow.contains("skill:opencode"),
        "a 16-cell card cannot hold the whole label: {narrow}"
    );

    // Wide card: the whole label fits, unellipsised.
    let wide = screen(&app, 160, 50);
    assert!(
        wide.contains("tool skill:opencode"),
        "the full classified label is missing: {wide}"
    );
}

/// A namespaced call is printed with no source prefix: the row says what ran and
/// never claims `mcp:` or `plugin:` for a namespace it cannot resolve (A9).
#[test]
fn renders_a_namespaced_tool_unprefixed() {
    let mut team = mywaker::team::Team::starter();
    team.set_last_tool(
        1,
        Some(mywaker::opencode::ToolUse::new(
            mywaker::opencode::ToolClass::Namespaced,
            "db-audit.run_select_query",
        )),
    );

    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    let app = App::with_team(Config::default(), tx, team);

    let text = screen(&app, 120, 40);
    assert!(text.contains("tool db-audit"), "{text}");
    assert!(!text.contains("mcp:"), "no invented source prefix: {text}");
    assert!(!text.contains("plugin:"), "no invented source prefix");
}

/// A whole card row still fits at 80×24 with the five-row card — the height the
/// extra row costs comes out of the card *count*, never out of a card.
#[test]
fn the_fifth_row_still_leaves_a_whole_card_at_80x24() {
    let cells = ui::grid_cells(&app(), (80, 24));
    assert!(
        !cells.is_empty(),
        "80×24 must still draw at least one whole card"
    );
    let panel = ui::team_area((80, 24));
    for (_, rect) in &cells {
        assert!(
            rect.bottom() <= panel.bottom(),
            "a card is clipped by the panel: {rect:?} in {panel:?}"
        );
        assert!(
            rect.height >= 7,
            "a card must keep all five content rows and both borders: {rect:?}"
        );
    }
    // And the screen itself renders.
    screen(&app(), 80, 24);
}
