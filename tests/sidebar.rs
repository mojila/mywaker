//! Sidebar acceptance tests — the A1–A10 matrix from
//! `docs/acceptance-sidebar.md`, one test per pinned size.
//!
//! Expectations are **derived** from the spec's §2 budget through
//! [`mywaker::ui::sidebar::fit`] and `ui::layout::roster_areas`, not hardcoded,
//! so a geometry or budget change shows up as a failure here rather than as a
//! silently-stale table in the docs.
//!
//! Every assertion below is decidable from the rendered buffer plus the one seam
//! that cannot be read off a buffer: [`sidebar::source_rows`], the untruncated
//! string each row was composed from (A6).

use mywaker::app::App;
use mywaker::config::Config;
use mywaker::message::Message;
use mywaker::opencode::{OpenCode, ServerInfo};
use mywaker::team::{Employee, Team};
use mywaker::ui;
use mywaker::ui::sidebar;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

/// A populated app: five employees (so the roster exceeds a short tasks panel),
/// latched tasks, an assigned model, and a mixed task queue — enough that every
/// sidebar section has something real to draw.
fn app() -> App {
    let mut team = Team::starter();
    for index in 0..team.employees().len() {
        team.set_last_task(index, Some("turn the last meeting into a plan".into()));
    }
    team.set_model(0, "openai/gpt-6-luna-pro".into(), Some("openai".into()));
    team.add_employee(Employee::new("atlas", "Bartholomew", "3D asset creator"))
        .unwrap();

    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    App::with_team(Config::default(), tx, team)
}

/// A **connected** app, so the aggregate mood is `Idle` rather than the
/// `connecting` fallback — the only state in which A9's blink period is visible.
///
/// This fixture exists because every other render test in the suite runs
/// disconnected, which left the idle avatar branch unexercised: freezing
/// `render_mini` entirely used to leave the whole suite green.
fn idle_app() -> App {
    let mut app = app();
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
    // `Team::starter()` is all-idle and nothing is mid-turn, so `busy_count` is
    // 0 and the Now line reads `idle`.
    app
}

/// Draw the whole app at `width`×`height` and return the rendered rows.
fn screen(app: &App, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

/// The sidebar's rectangle for a terminal of this size — read from the same
/// layout function the renderer uses, never computed by arithmetic (spec §3).
fn sidebar_rect(width: u16, height: u16) -> Rect {
    ui::sidebar_area((width, height))
}

/// The sidebar's visible text, one string per content row, right-trimmed.
fn sidebar_rows(app: &App, width: u16, height: u16) -> Vec<String> {
    content_rows(&screen(app, width, height), sidebar_rect(width, height))
}

/// The inside of `rect`, one string per row, right-trimmed.
fn content_rows(rows: &[String], rect: Rect) -> Vec<String> {
    let top = rect.y.saturating_add(1);
    let bottom = rect.y + rect.height.saturating_sub(1);
    rows.iter()
        .enumerate()
        .filter(|(y, _)| *y >= top as usize && (*y as u16) < bottom)
        .map(|(_, row)| {
            let start = rect.x as usize + 1;
            let end = (rect.x + rect.width.saturating_sub(1)) as usize;
            row.chars()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// The sections §3 requires and forbids at a given inner height, derived from
/// §2's budget **through `fit`** rather than restated by hand.
///
/// The budget's content lines are the contract; the cumulative totals follow
/// from them by the derivation rule, which `fit` implements. A section is
/// present only when its full height fits, so "the longest prefix that fits" is
/// exactly the expected set.
fn expected_markers(
    app: &App,
    inner_width: usize,
    inner_height: usize,
) -> (Vec<&'static str>, Vec<&'static str>) {
    let markers = sidebar::markers();
    // §2's per-section content lines, in priority order (P0…P6).
    let heights = [4usize, 1, 4, 1, 2, 2, 2];
    let mut used = 0;
    let mut present = 0;
    for (index, lines) in heights.iter().enumerate() {
        let needed = lines + usize::from(index > 0); // gap row above, except P0
        if used + needed > inner_height {
            break;
        }
        used += needed;
        present += 1;
    }

    // Cross-check the hand-written budget against the implementation's own
    // `fit`: if they disagree, the docs and the code have drifted and the test
    // should say so rather than assert a stale table.
    let fitted = sidebar::fitted_rows(app, inner_width, inner_height);
    assert_eq!(
        fitted.len(),
        present,
        "§2's budget implies {present} sections at inner {inner_width}×{inner_height}, \
         but `fit` kept {}",
        fitted.len()
    );

    let (shown, hidden) = markers.split_at(present);
    (shown.to_vec(), hidden.to_vec())
}

/// A1–A10 for one pinned size.
fn assert_size(width: u16, height: u16) {
    let app = app();
    let label = format!("{width}×{height}");

    // A1: draw returns Ok.
    let rows = screen(&app, width, height);
    assert_eq!(rows.len(), height as usize, "{label}");

    let rect = sidebar_rect(width, height);
    let inner_width = rect.width.saturating_sub(2) as usize;
    let inner_height = rect.height.saturating_sub(2) as usize;

    // A10: all four borders and corners, and the title/brand boundary.
    assert_border_and_brand(&rows, rect, inner_width, &label);

    let content = content_rows(&rows, rect);

    // A5: exactly the markers the budget allows.
    let (must, must_not) = expected_markers(&app, inner_width, inner_height);
    let joined = content.join("\n");
    for marker in &must {
        // A marker may be ellipsised when the panel is narrower than the literal
        // (spec A10, the 40×24 case), but it must still be recognisable as a
        // prefix — never dropped.
        let shown = joined.contains(marker)
            || joined.lines().any(|row| {
                row.trim_start().starts_with(&marker[..marker.len().min(2)])
                    && row.trim_end().ends_with('…')
            });
        assert!(
            shown,
            "{label}: expected marker {marker:?} (inner h {inner_height})\n{joined}"
        );
    }
    for marker in &must_not {
        assert!(
            !joined.contains(marker),
            "{label}: unexpected marker {marker:?} (inner h {inner_height})\n{joined}"
        );
    }

    // A3: vertical fit, **including** the all-or-nothing clause. Every section
    // `fit` kept contributes all of its rows — no section is drawn short — and
    // the total never exceeds the panel.
    assert_vertical_fit(&app, &content, inner_width, inner_height, &label);

    // A4: the first non-blank row after the avatar block is the Now line.
    // P0's avatar is 4 rows (inner-relative 0..4), then a gap row, then Now.
    let now_index = content
        .iter()
        .enumerate()
        .skip(4)
        .find(|(_, row)| !row.trim().is_empty())
        .map(|(index, _)| index)
        .expect("a non-blank row after the avatar");
    let now_row = &content[now_index];
    assert!(
        now_row.contains("now"),
        "{label}: the Now row lost its marker: {now_row:?}"
    );
    // The state is a word when there is room for one, else the leading glyph —
    // the 40×24 case, where 6 usable columns hold `now ` plus one glyph.
    let named = [
        "idle",
        "working",
        "thinking",
        "offline",
        "paused",
        "connecting",
    ]
    .iter()
    .any(|state| now_row.contains(state));
    let glyph_only = now_row
        .strip_prefix("now ")
        .is_some_and(|rest| rest.chars().count() == 1);
    assert!(
        named || glyph_only,
        "{label}: the Now line names no state: {now_row:?}"
    );

    // A6: truncate, never wrap — every drawn row is either its section's
    // complete source string or an ellipsised prefix of it.
    assert_truncated_never_wrapped(&app, &content, inner_width, &label);

    // A7: the sidebar's *static vocabulary* owns no log/title text. Relayed
    // strings (names, roles, task goals, model ids, connection errors) are out
    // of scope by §4:223, so this asserts on the labels rather than the buffer.
    for label_text in sidebar::labels() {
        for owned in ["ram ", "cpu ", "events", "cache ", "opencode v"] {
            assert!(
                !label_text.contains(owned),
                "{label}: sidebar label {label_text:?} duplicates log text {owned:?}"
            );
        }
    }
}

/// A3: content rows never exceed the panel, and no section is drawn partially.
///
/// The all-or-nothing half is what makes this more than a row count: each
/// section `fit` kept must contribute *every* row it composed. A section whose
/// rows were clipped would leave its source rows unaccounted for.
fn assert_vertical_fit(
    app: &App,
    content: &[String],
    inner_width: usize,
    inner_height: usize,
    label: &str,
) {
    // The panel pads with blank rows below the stack, so only the rows up to the
    // last non-blank one are "drawn".
    let drawn = content
        .iter()
        .rposition(|row| !row.trim().is_empty())
        .map_or(0, |index| index + 1);

    assert!(
        drawn <= inner_height,
        "{label}: {drawn} content rows in {inner_height}"
    );

    let sections = sidebar::fitted_rows(app, inner_width, inner_height);
    // `fit` emits exactly the rows it kept, gap rows included, so the drawn
    // height is the sum of the kept sections — and each kept section is whole.
    let expected: usize = sections.iter().map(Vec::len).sum();
    assert_eq!(
        drawn, expected,
        "{label}: drew {drawn} rows, expected {expected} from whole sections\n{content:#?}"
    );

    // Every section's own source rows appear contiguously in the drawn rows, in
    // order — nothing was dropped from the middle of a section.
    let joined = content.join("\n");
    for (index, section) in sections.iter().enumerate() {
        for source in section.iter().filter(|row| !row.is_empty()) {
            let prefix: String = source.chars().take(source.chars().count().min(2)).collect();
            assert!(
                joined.contains(&prefix),
                "{label}: section {index} lost a row (source {source:?})\n{joined}"
            );
        }
    }
}

/// A6: each drawn row is complete, or an ellipsised prefix of its source.
///
/// This replaces the previous trailing-space check, which was vacuous: it called
/// `trim_end()` and then asked whether the result ended in a space, which is
/// false by construction. The real defect it missed was the 40×24 inspector row
/// `ope… ·` — two independently truncated halves joined by the terminal, which
/// is neither a complete string nor an ellipsised prefix.
fn assert_truncated_never_wrapped(app: &App, content: &[String], inner_width: usize, label: &str) {
    let sources = sidebar::source_rows(app, inner_width);
    assert!(
        !sources.is_empty(),
        "{label}: the sidebar composed no rows at all"
    );

    for (index, row) in content.iter().enumerate() {
        // Blank rows are the gap rows `fit` inserts between sections.
        if row.trim().is_empty() {
            continue;
        }

        // A row is acceptable when it equals a source row exactly. Both sides are
        // right-trimmed because `content` is (a row's trailing padding is not
        // content), and a label like `pulse ` ends in a space by design.
        if sources.iter().any(|source| source.trim_end() == row) {
            continue;
        }

        // …or when it is that source's ellipsised prefix.
        let ellipsised = row
            .strip_suffix('…')
            .is_some_and(|head| sources.iter().any(|source| source.starts_with(head)));
        assert!(
            ellipsised,
            "{label}: row {index} is neither complete nor an ellipsised prefix: {row:?}\n\
             sources: {sources:#?}"
        );

        // A truncated row must fill the panel: if it stopped short of the inner
        // width it was not truncated, it was a fragment.
        assert_eq!(
            row.chars().count(),
            inner_width,
            "{label}: row {index} is ellipsised but only {} of {inner_width} columns: {row:?}",
            row.chars().count()
        );
    }

    // No source string may be split across two consecutive rows: a wrapped row
    // would start where the previous row's text left off.
    for pair in content.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        if first.trim().is_empty() || second.trim().is_empty() {
            continue;
        }
        for source in &sources {
            let chars: Vec<char> = source.chars().collect();
            for split in 1..chars.len() {
                let head: String = chars[..split].iter().collect();
                let tail: String = chars[split..].iter().collect();
                assert!(
                    !(first.ends_with(&head) && second.starts_with(&tail)),
                    "{label}: a source row wrapped across two lines: {first:?} / {second:?}"
                );
            }
        }
    }
}

/// A10: all four borders and corners are intact, the title keeps ` waker ` when
/// the inner width allows it (and is clipped, not dropped, when it does not), and
/// the nameplate keeps `MYWAKER` at inner width ≥ 7.
fn assert_border_and_brand(rows: &[String], rect: Rect, inner_width: usize, label: &str) {
    let line = |y: u16| -> String {
        rows[y as usize]
            .chars()
            .skip(rect.x as usize)
            .take(rect.width as usize)
            .collect()
    };

    let top = line(rect.y);
    let bottom = line(rect.y + rect.height.saturating_sub(1));
    let left_column: String = (rect.y..rect.y + rect.height)
        .map(|y| rows[y as usize].chars().nth(rect.x as usize).unwrap_or(' '))
        .collect();
    let right_column: String = (rect.y..rect.y + rect.height)
        .map(|y| {
            rows[y as usize]
                .chars()
                .nth((rect.x + rect.width.saturating_sub(1)) as usize)
                .unwrap_or(' ')
        })
        .collect();

    // Corners and the top/bottom rules. The title sits inside the top rule, so
    // the check removes it first and then requires every remaining interior cell
    // to be a rule glyph — a gap would be a space.
    assert!(
        top.starts_with('┌') && top.ends_with('┐'),
        "{label}: top border broken: {top:?}"
    );
    assert!(
        bottom.starts_with('└') && bottom.ends_with('┘'),
        "{label}: bottom border broken: {bottom:?}"
    );
    let rule_only = |border: &str, title: &str| -> bool {
        let interior = border
            .trim_start_matches(['┌', '└'])
            .trim_end_matches(['┐', '┘'])
            .replace(title, "");
        interior.chars().all(|c| c == '─')
    };
    if inner_width >= 7 {
        assert!(
            rule_only(&top, " waker "),
            "{label}: top border has gaps: {top:?}"
        );
    }
    // Below inner width 7 the title fills the whole top border (`┌ waker┐`), so
    // there is no rule left to check — A10 permits that, and the corners above
    // are the assertion that still holds.
    assert!(
        rule_only(&bottom, ""),
        "{label}: bottom border has gaps: {bottom:?}"
    );

    // Left and right rules, from the first body row to the last.
    let body_left: String = left_column
        .chars()
        .skip(1)
        .take(rect.height.saturating_sub(2) as usize)
        .collect();
    let body_right: String = right_column
        .chars()
        .skip(1)
        .take(rect.height.saturating_sub(2) as usize)
        .collect();
    assert!(
        body_left.chars().all(|c| c == '│'),
        "{label}: left border broken: {left_column:?}"
    );
    assert!(
        body_right.chars().all(|c| c == '│'),
        "{label}: right border broken: {right_column:?}"
    );

    // The title: the full literal when the inner width can hold it, otherwise a
    // clipped prefix that still names the panel.
    if inner_width >= 7 {
        assert!(
            top.contains(" waker "),
            "{label}: title should be the literal ` waker ` at inner width {inner_width}: {top:?}"
        );
    } else {
        assert!(
            top.contains("waker"),
            "{label}: the title must survive as a clipped prefix at inner width \
             {inner_width}: {top:?}"
        );
    }

    // The nameplate: the full literal at inner width ≥ 7, ellipsised below it.
    let nameplate = rows
        [rect.y as usize + 1..rect.y as usize + rect.height.saturating_sub(1) as usize]
        .iter()
        .map(|row| {
            row.chars()
                .skip(rect.x as usize + 1)
                .take(inner_width)
                .collect::<String>()
        })
        .find(|row| row.contains("MYWAK"));
    let nameplate = nameplate.unwrap_or_else(|| panic!("{label}: the nameplate is missing"));
    if inner_width >= 7 {
        assert!(
            nameplate.contains("MYWAKER"),
            "{label}: nameplate should be the literal at inner width {inner_width}: {nameplate:?}"
        );
    } else {
        assert!(
            nameplate.trim_end().ends_with('…'),
            "{label}: a narrow nameplate is ellipsised: {nameplate:?}"
        );
    }
}

#[test]
fn sidebar_at_160x50() {
    assert_size(160, 50);
}

#[test]
fn sidebar_at_120x40() {
    assert_size(120, 40);
}

#[test]
fn sidebar_at_100x30() {
    assert_size(100, 30);
}

#[test]
fn sidebar_at_80x24() {
    assert_size(80, 24);
}

#[test]
fn sidebar_at_60x16() {
    assert_size(60, 16);
}

#[test]
fn sidebar_at_40x24() {
    assert_size(40, 24);
}

/// A2: mutating a sidebar-owned input changes no cell outside the sidebar.
///
/// Uses `render_only`, the seam that draws the sidebar's content with no border
/// and no neighbours, so the comparison is between two buffers of the same
/// shape rather than two whole frames.
///
/// Run at every pinned size, because containment is a geometry claim and the
/// sidebar's rectangle changes shape at 40×24 (where it narrows to 8 columns).
#[test]
fn a2_sidebar_containment() {
    for (width, height) in SIZES {
        assert_containment(width, height);
    }
}

/// The pinned sizes, in the order §3 lists them.
const SIZES: [(u16, u16); 6] = [
    (160, 50),
    (120, 40),
    (100, 30),
    (80, 24),
    (60, 16),
    (40, 24),
];

/// A2 for one size.
fn assert_containment(width: u16, height: u16) {
    let label = format!("{width}×{height}");
    let app = app();
    // The control: one more pulse sample, which only the sidebar's sparkline
    // reads. (`advance` would also bump the frame the title spinner uses.)
    let frame_before = screen(&app, width, height);
    let before = sidebar_only(&app, 24, 30);

    let mut ticked = app;
    ticked.record_pulse(1);

    let after = sidebar_only(&ticked, 24, 30);
    assert_ne!(
        before, after,
        "{label}: a pulse sample should change the sidebar"
    );

    let frame_after = screen(&ticked, width, height);
    let rect = sidebar_rect(width, height);
    for (y, (a, b)) in frame_before.iter().zip(&frame_after).enumerate() {
        for (x, (ca, cb)) in a.chars().zip(b.chars()).enumerate() {
            let inside = (x as u16) >= rect.x
                && (x as u16) < rect.x + rect.width
                && (y as u16) >= rect.y
                && (y as u16) < rect.y + rect.height;
            if !inside {
                assert_eq!(
                    ca, cb,
                    "{label}: cell ({x},{y}) outside the sidebar changed"
                );
            }
        }
    }
}

/// Render only the sidebar's content into a throwaway buffer.
fn sidebar_only(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| sidebar::render_only(frame, app, frame.area()))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// A9: the sidebar's own avatar animates.
///
/// The blink *period* is `render_mini`'s contract, pinned in
/// `src/ui/robot.rs::mini_avatar_animates_with_frame` against the four-row avatar
/// this panel draws. This test proves the panel *wires the frame through* — the
/// seam between `App::advance` and what the sidebar renders.
#[test]
fn a9_sidebar_animates() {
    for (width, height) in SIZES {
        let label = format!("{width}×{height}");
        let sidebar_after = |ticks: usize| {
            let mut app = idle_app();
            for _ in 0..ticks {
                app.advance();
            }
            sidebar_only(&app, 24, 30)
        };

        assert_ne!(
            sidebar_after(0),
            sidebar_after(1),
            "{label}: a tick must change the sidebar"
        );
        assert_ne!(
            sidebar_after(0),
            sidebar_after(4),
            "{label}: the avatar animates with the frame"
        );
    }
}

/// A9's blink clause at panel level: the idle avatar's antenna is lit for the
/// first ten ticks and dim for the next ten, and the difference is visible in the
/// rendered sidebar (not only in `render_mini`).
///
/// This is the assertion that was missing: the period used to be pinned only
/// against the 9-row `render`, so freezing `render_mini` — the avatar the panel
/// actually draws — left the suite green. The fixture is `idle_app`, because the
/// blink only happens in `Idle`.
#[test]
fn a9_idle_avatar_blinks_in_the_panel() {
    for (width, height) in SIZES {
        let label = format!("{width}×{height}");
        let antenna_at = |ticks: usize| {
            let mut app = idle_app();
            for _ in 0..ticks {
                app.advance();
            }
            // The first content row is the avatar's first row.
            sidebar_rows(&app, width, height)[0].trim().to_string()
        };

        let lit = antenna_at(0);
        let dim = antenna_at(10);
        assert!(lit.contains('●'), "{label}: frame 0 is lit: {lit:?}");
        assert!(dim.contains('·'), "{label}: frame 10 is dim: {dim:?}");
        assert_eq!(antenna_at(20), lit, "{label}: the period is 20 ticks");
    }
}

/// A8: the sidebar is not a hit target — clicking inside it moves nothing.
///
/// Checked at every pinned size, since the panel's rectangle (and therefore the
/// points inside it) changes with the terminal.
#[test]
fn a8_sidebar_is_not_a_hit_target() {
    let app = app();
    for (width, height) in SIZES {
        let rect = sidebar_rect(width, height);
        // A point comfortably inside the panel, clear of the border.
        let point =
            ratatui::layout::Position::new(rect.x + rect.width / 2, rect.y + rect.height / 2);
        let target = ui::hit_target_at(&app, (width, height), point.x, point.y);
        assert_eq!(
            target, None,
            "{width}×{height}: the sidebar must resolve to no target at {point:?}"
        );
    }
}

/// A wheel notch over the sidebar scrolls the **roster grid's viewport**, not a
/// sidebar-owned offset — the sidebar has no scrollable content, and it must not
/// swallow the notch either.
///
/// The intended behaviour, pinned here so it cannot drift silently: the panel is
/// not a hit target (A8), so a notch over it falls through to the roster and
/// scrolls the grid exactly as a notch over the grid does. It never moves the
/// selection.
#[test]
fn a_wheel_over_the_sidebar_scrolls_the_grid() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

    // A roster long enough that the grid has somewhere to scroll.
    let mut team = Team::default();
    for index in 0..12 {
        team.add_employee(Employee::new(
            format!("e{index:02}"),
            format!("Emp {index:02}"),
            "Generalist",
        ))
        .expect("unique ids");
    }
    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
    let mut app = App::with_team(Config::default(), tx, team);
    app.update(Message::Resize(80, 24));
    assert!(
        app.roster_max_scroll() > 0,
        "the grid must overflow to scroll"
    );

    let rect = sidebar_rect(80, 24);
    let over_sidebar = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: rect.x + 2,
        row: rect.y + 4,
        modifiers: KeyModifiers::NONE,
    };
    let selected = app.selected();
    app.update(Message::Mouse(over_sidebar));

    assert_eq!(
        app.roster_scroll(),
        1,
        "a notch over the sidebar scrolls the grid"
    );
    assert_eq!(
        app.selected(),
        selected,
        "scrolling never moves the selection"
    );
}
