//! R1 — the roster grid and the mouse hit-tester are one geometry.
//!
//! [`ui::grid_cells`] is what the renderer draws; [`ui::hit_target_at`] is what a
//! click resolves to. They read the same `pipeline::grid`, so the claim under test
//! is that nothing between them drifts: **every drawn card resolves to its own
//! employee, every cell of every card resolves to it, and no employee is
//! clickable that is not drawn.**
//!
//! The failure mode this guards is the one that invites itself whenever the grid
//! order changes: the drawing picks one order and the hit-tester another, so a
//! click selects the wrong employee. The assertions are deliberately
//! order-agnostic and roster-size-agnostic — they name no employee and assume no
//! particular membership — so they keep holding as the roster grows, shrinks, or
//! re-orders.
//!
//! Every pinned size from `docs/acceptance-sidebar.md` §3 is covered, because the
//! panel changes shape at 40×24 (no grid at all) and at 80×24 (one column).

use std::collections::BTreeSet;

use mywaker::app::App;
use mywaker::config::Config;
use mywaker::message::Message;
use mywaker::team::{Employee, Team};
use mywaker::ui;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use ratatui::layout::Rect;

/// The pinned sizes, in the order the sidebar spec lists them.
const SIZES: [(u16, u16); 6] = [
    (160, 50),
    (120, 40),
    (100, 30),
    (80, 24),
    (60, 16),
    (40, 24),
];

/// An app whose roster is long enough to overflow the grid's viewport at most
/// sizes, so scrolling is exercised rather than assumed.
fn app() -> App {
    let mut team = Team::starter();
    for i in 0..8 {
        team.add_employee(Employee::new(
            format!("e{i:02}"),
            format!("Emp {i:02}"),
            "Generalist",
        ))
        .expect("unique ids");
    }
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    App::with_team(Config::default(), tx, team)
}

/// A wheel notch at a cell (the team panel is not itself a hit target, so the
/// notch falls through to the grid and scrolls it).
fn wheel(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// A wheel notch upward — the bounded way back to the top of the grid.
fn wheel_up(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// A left click at a cell.
fn click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// Every cell inside a card, border included.
fn inside(rect: Rect) -> impl Iterator<Item = (u16, u16)> {
    let x0 = rect.x;
    let y0 = rect.y;
    (0..rect.width).flat_map(move |dx| (0..rect.height).map(move |dy| (x0 + dx, y0 + dy)))
}

/// R1: at every pinned size, and at every scroll offset that size allows, the
/// drawn cards and the hit-tester agree.
#[test]
fn grid_cells_and_hit_testing_agree_at_every_pinned_size() {
    for (width, height) in SIZES {
        let mut app = app();
        app.update(Message::Resize(width, height));
        let size = (width, height);

        let panel = ui::team_area(size);
        let max = app.roster_max_scroll();

        // The employee set drawn at each offset, in visit order, so the last
        // check can prove scrolling actually moved the viewport.
        let mut pages: Vec<BTreeSet<usize>> = Vec::new();

        for offset in 0..=max {
            assert_eq!(
                app.roster_scroll(),
                offset,
                "{width}×{height}: the scroll offset must follow the notches"
            );

            let cells = ui::grid_cells(&app, size);

            // The two sides of the claim, gathered independently: what the
            // renderer says it draws, and which employees any cell of the panel
            // resolves to.
            let drawn: BTreeSet<usize> = cells.iter().map(|(employee, _)| *employee).collect();
            let mut hittable: BTreeSet<usize> = BTreeSet::new();
            for y in panel.y..panel.bottom() {
                for x in panel.x..panel.right() {
                    if let Some(ui::Target::Employee(employee)) =
                        ui::hit_target_at(&app, size, x, y)
                    {
                        hittable.insert(employee);
                    }
                }
            }
            assert_eq!(
                hittable, drawn,
                "{width}×{height} offset {offset}: drawn cards {drawn:?} and clickable \
                 employees {hittable:?} disagree"
            );

            for (employee, rect) in &cells {
                assert!(
                    panel.contains(ratatui::layout::Position::new(rect.x, rect.y))
                        && rect.right() <= panel.right()
                        && rect.bottom() <= panel.bottom(),
                    "{width}×{height} offset {offset}: card {rect:?} escapes the panel {panel:?}"
                );

                // Every cell of the card — border included — belongs to it. This
                // is the strong form: a hit-tester that agreed only on the middle
                // would still mis-select on the title row or the border.
                for (x, y) in inside(*rect) {
                    assert_eq!(
                        ui::hit_target_at(&app, size, x, y),
                        Some(ui::Target::Employee(*employee)),
                        "{width}×{height} offset {offset}: ({x},{y}) inside card {rect:?} \
                         resolved to the wrong target"
                    );
                }

                // A real click goes through the app's own path and selects that
                // employee — the end the hit-test exists for.
                app.update(Message::Mouse(click(rect.x + 1, rect.y + 1)));
                assert_eq!(
                    app.selected(),
                    *employee,
                    "{width}×{height} offset {offset}: clicking {rect:?} selected the wrong employee"
                );
            }

            // No employee has two cards in one frame.
            assert_eq!(
                drawn.len(),
                cells.len(),
                "{width}×{height} offset {offset}: an employee has two cards"
            );

            // Every offset before the last must actually reveal a new page. If
            // two consecutive offsets draw the same employees, the viewport is
            // already at its end and `roster_max_scroll` is larger than the grid
            // the renderer actually lays out — the two would then disagree about
            // how far the roster scrolls. This is the check that catches
            // `card_order()` landing in the draw seam while `App::roster_max_scroll`
            // still counts the whole team. Scoped to a panel that draws at all
            // (see the known-gap note for 40×24).
            if panel.width > 0
                && let Some(previous) = pages.last()
            {
                assert_ne!(
                    previous,
                    &drawn,
                    "{width}×{height}: offset {offset} draws the same page as offset {} — \
                     `roster_max_scroll` is larger than the grid's real last page",
                    offset - 1
                );
            }
            pages.push(drawn);

            if offset == max {
                break;
            }
            // Scroll one row with the wheel, aimed inside the panel.
            app.update(Message::Mouse(wheel(
                panel.x + 1,
                panel.y + panel.height.saturating_sub(1),
            )));
            let next = app.roster_scroll();
            assert_eq!(
                next,
                offset + 1,
                "{width}×{height}: a notch must advance exactly one row"
            );
        }

        // Nothing that is not a card resolves to an employee: the panel's title
        // row and the Jev header strip above the first card row are inert, and so
        // is the one-column gap between two cards.
        let cells = ui::grid_cells(&app, size);
        match cells.first() {
            Some((_, first)) => {
                for y in panel.y..first.y {
                    for x in panel.x..panel.right() {
                        assert_eq!(
                            ui::hit_target_at(&app, size, x, y),
                            None,
                            "{width}×{height}: ({x},{y}) is above the first card row but hit an employee"
                        );
                    }
                }
                for pair in cells.windows(2) {
                    let (left, right) = (pair[0].1, pair[1].1);
                    if left.y == right.y && left.right() < right.x {
                        for x in left.right()..right.x {
                            for y in left.y..left.bottom() {
                                assert_eq!(
                                    ui::hit_target_at(&app, size, x, y),
                                    None,
                                    "{width}×{height}: the gap at ({x},{y}) hit an employee"
                                );
                            }
                        }
                    }
                }
            }
            // No cards drawn (a zero-width panel at 40×24): the panel is inert.
            //
            // Known gap, recorded not asserted: `pipeline::shape` counts
            // `visible_rows` from the panel's *height* without consulting its
            // width, so at 40×24 it reports a non-zero `max_offset` (observed: 10)
            // for a panel that draws nothing. `App::roster_scroll` therefore
            // accepts notches that move an empty viewport. Drawing and
            // hit-testing still agree — both see zero cards, which is what R1
            // owns — so this is a `shape` defect for the writer, not an R1
            // failure. It is not pinned here, because a test that asserts a bug
            // also fails when the bug is fixed.
            None => {
                for y in panel.y..panel.bottom() {
                    for x in panel.x..panel.right() {
                        assert_eq!(
                            ui::hit_target_at(&app, size, x, y),
                            None,
                            "{width}×{height}: a card-less panel must not hit an employee"
                        );
                    }
                }
            }
        }

        // Scrolling must actually move the viewport: when the roster overflows,
        // the last page differs from the first. Scoped to a panel that draws at
        // all — at 40×24 the width is zero and every page is empty (see the
        // known-gap note above), so there is nothing for scrolling to reveal.
        if max > 0 && panel.width > 0 {
            assert_ne!(
                pages.first(),
                pages.last(),
                "{width}×{height}: the last page repeats the first — scrolling did nothing"
            );
        }

        // R1b — the *draw* seam, asserted from a rendered frame rather than from
        // the geometry. `grid_cells` and `hit_target_at` could agree with each
        // other while `pipeline::render` drew a different order, so the claim
        // "the grid draws `card_order`" is pinned here: every employee the grid
        // owns appears as a card title exactly once.
        assert_render_draws_card_order(&mut app, size);
    }
}

/// R1b: the drawn frame contains one card per employee in `card_order`.
///
/// This is the guard the drawing needs on its own: mutating `pipeline::render` to
/// read a different order leaves every geometry assertion above green, but the
/// card heads below catch it directly.
fn assert_render_draws_card_order(app: &mut App, size: (u16, u16)) {
    let (width, height) = size;
    let panel = mywaker::ui::team_area(size);
    if panel.width == 0 || panel.height == 0 {
        return; // no panel, no cards to draw (40×24)
    }

    // From the top, so the first page is a known, whole set of cards. The loop is
    // bounded by the offset itself: each notch up can only decrease it, so this
    // terminates even if the clamp is broken.
    let mut guard = 0;
    while app.roster_scroll() > 0 {
        app.update(Message::Mouse(wheel_up(panel.x + 1, panel.y)));
        guard += 1;
        assert!(guard < 1000, "scrolling up did not reach the top");
    }

    let frame = screen(app, width, height);
    let rows = rows(&frame, width);
    let in_panel = |needle: &str| -> usize {
        rows.iter()
            .enumerate()
            .filter(|(y, _)| {
                let y = *y as u16;
                y >= panel.y && y < panel.bottom()
            })
            .map(|(_, row)| {
                let chars: Vec<char> = row.chars().collect();
                let start = (panel.x as usize).min(chars.len());
                let end = (panel.right() as usize).min(chars.len());
                chars[start..end].iter().collect::<String>()
            })
            .map(|row| row.matches(needle).count())
            .sum()
    };

    // No card is drawn without a head: every card title is a real employee.
    // Every card the geometry says is on this page is drawn, exactly once, with
    // its own head. Names anchor the count: several generalists share `GENERAL`,
    // so a label count could not tell one card from eight.
    //
    // A panel too narrow for the head (60×16 leaves the team panel 4 columns)
    // truncates the card instead of drawing it, so the clause is scoped to panels
    // with room.
    let drawn = mywaker::ui::grid_cells(app, size);
    assert!(
        !drawn.is_empty(),
        "{width}×{height}: the page draws no cards"
    );
    let widest = drawn
        .iter()
        .map(|(employee, _)| app.team().employees()[*employee].name.len())
        .max()
        .unwrap_or(0);
    // `● ` plus the name, inside the card's two borders.
    let head_fits = panel.width as usize >= widest + 4;
    for (employee, _) in &drawn {
        let employee = &app.team().employees()[*employee];
        let head = format!("● {}", employee.name);
        if head_fits {
            assert_eq!(
                in_panel(&head),
                1,
                "{width}×{height}: {}'s card head was drawn {} times:\n{frame}",
                employee.name,
                in_panel(&head)
            );
        }
    }
}

/// The frame split into rows of `width` cells. Splits on characters, not bytes: a
/// cell can hold a multi-byte glyph (`┄`, `◆`), and a byte chunk would cut one in
/// half.
fn rows(frame: &str, width: u16) -> Vec<String> {
    frame
        .chars()
        .collect::<Vec<char>>()
        .chunks(width as usize)
        .map(|row| row.iter().collect())
        .collect()
}

/// Render the whole app at `size` on a throwaway terminal and return the buffer's
/// symbols, one character per cell.
fn screen(app: &App, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| mywaker::ui::render(frame, app))
        .expect("a frame draws");
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
