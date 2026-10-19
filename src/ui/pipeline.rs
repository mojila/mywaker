//! The team grid: a fixed three-column, one-column-gap grid of employee cards,
//! ordered by workflow role (see [`Employee::workflow_role`] →
//! [`Role::order`](crate::role::Role::order)).
//!
//! Jev (the question answerer) is a shared resource, not an employee, so it is
//! not a card: it survives as a one-row header strip above the grid. That keeps
//! the "every employee delegates questions to Jev" relationship visible without
//! spending a card on a non-employee.
//!
//! The grid is a **viewport**: the wheel scrolls it, the arrows move the
//! selection and reveal it, and only whole rows are ever drawn. [`shape`] and
//! [`grid`] are the single source of that geometry — the renderer draws from
//! them and `ui::hit` hit-tests against them — so a click can only land on a
//! card that is actually on screen, on the row it points at.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::team::Employee;
use crate::ui::{status, truncate};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    // The grid draws the pipeline stages only: the orchestrator is the root
    // above them, not a card among them (see [`App::card_order`]).
    let order = app.card_order();

    if order.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" team ".bold().cyan());
        frame.render_widget(
            Paragraph::new("no employees — add one with `mywaker employee add`")
                .block(block)
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let employees = app.team().employees();
    let grid = grid(area, &order, app.roster_scroll());

    render_title(frame, &grid);
    render_jev_header(frame, app, grid.header());

    for cell in &grid.cells {
        render_stage(
            frame,
            app,
            &employees[cell.employee],
            cell.area,
            cell.employee == app.selected(),
        );
    }
}

/// Number of content rows inside a stage box (plus two border rows): the head
/// (status glyph + name), the role, the live status, and the model. Every card
/// fills all of them, so the cards line up.
const CONTENT_LINES: u16 = 4;
const STAGE_HEIGHT: u16 = CONTENT_LINES + 2;

/// Columns in the grid. Fixed: the roster is a team, not a responsive gallery,
/// and three cards read comfortably at the widths the panel actually gets.
const COLS: u16 = 3;

/// One blank column between cards, so adjacent borders never fuse into one box.
const COL_GAP: u16 = 1;

/// A card narrower than this is unreadable — the stage label alone would
/// overflow its title tab — so the grid falls back to a single full-width
/// column rather than squeezing three.
const MIN_CELL_WIDTH: u16 = 12;

/// One blank row between card rows, so cards never touch vertically.
const ROW_GAP: u16 = 1;

/// Distance between the tops of two adjacent card rows.
const ROW_PITCH: u16 = STAGE_HEIGHT + ROW_GAP;

/// One drawn card: the employee it stands for (an index into the team, in
/// workflow order) and the rectangle it occupies.
pub(crate) struct Node {
    pub(crate) employee: usize,
    pub(crate) area: Rect,
}

/// The fixed part of the grid for one panel size and roster size: the column
/// split, the header bands, and how many whole rows the remaining height holds.
///
/// Split out from [`Grid`] because three callers need the counts without
/// drawing anything — the reveal rule, the scroll clamp, and the title.
pub(crate) struct Shape {
    pub(crate) cols: usize,
    pub(crate) cell_width: u16,
    pub(crate) title: Rect,
    pub(crate) header: Rect,
    /// The `y` of the first card row when the viewport is at offset 0.
    pub(crate) first_row: u16,
    /// Whole card rows that fit in the viewport.
    pub(crate) visible_rows: usize,
    pub(crate) total_rows: usize,
    /// The largest offset that still fills the viewport with whole rows.
    pub(crate) max_offset: usize,
}

/// The grid's geometry for one panel, roster, and scroll offset: the shape, the
/// cards on screen, and the offset actually used.
pub(crate) struct Grid {
    pub(crate) cells: Vec<Node>,
    pub(crate) shape: Shape,
    pub(crate) offset: usize,
    /// How many employees the roster holds — the title's `N`.
    pub(crate) count: usize,
}

impl Grid {
    pub(crate) fn title(&self) -> Rect {
        self.shape.title
    }

    pub(crate) fn header(&self) -> Rect {
        self.shape.header
    }
}

/// Lay the fixed grid out in `area` for a roster of `count` employees.
///
/// **Degradation.** The bands are dropped in the order that costs the user
/// least: the Jev header first (it is a status line, not work), then whole card
/// rows. The header is kept only when it is free — if drawing it would cost a
/// card row, the strip goes instead. A row is only counted when its full
/// [`STAGE_HEIGHT`] fits, so a card is never drawn with a top border and no body
/// — the failure mode the old tree had to special-case.
pub(crate) fn shape(area: Rect, count: usize) -> Shape {
    // `(width - gaps) / 3` is the shared width the plan specifies; when even that
    // is under 12 columns the cards would be unreadable, so go to one column.
    let shared = area.width.saturating_sub(COL_GAP * (COLS - 1)) / COLS;
    let (cols, cell_width) = if shared >= MIN_CELL_WIDTH {
        (COLS as usize, shared)
    } else {
        (1, area.width)
    };

    let total_rows = count.div_ceil(cols.max(1));

    let title_height = area.height.min(1);
    let title = Rect::new(area.x, area.y, area.width, title_height);
    let below_title = area.height.saturating_sub(title_height);

    // `n` rows need `n * ROW_PITCH - 1` lines: the gap sits *between* rows, not
    // after the last one. Capped at the roster's own row count, so a panel with
    // room to spare keeps the header: only a header that would push a card row
    // off the screen is worth dropping.
    let rows_in = |height: u16| {
        if total_rows == 0 {
            0
        } else {
            (((height + 1) / ROW_PITCH) as usize).min(total_rows)
        }
    };
    let without_header = rows_in(below_title);
    let with_header = rows_in(below_title.saturating_sub(1));
    // The strip costs a row the moment it would push the last card row off the
    // panel, so it is only drawn when the row count is the same either way.
    let header_height = u16::from(with_header == without_header && below_title > STAGE_HEIGHT);
    let header = Rect::new(area.x, area.y + title_height, area.width, header_height);

    let available = below_title.saturating_sub(header_height);
    // A zero-width panel draws no cards at all (see [`grid`]), so it also has no
    // rows to scroll: `visible_rows` and `max_offset` must agree with what is
    // drawn, or the wheel would accept notches over an empty viewport.
    let visible_rows = if cell_width == 0 {
        0
    } else {
        rows_in(available)
    };

    Shape {
        cols,
        cell_width,
        title,
        header,
        first_row: area.y + title_height + header_height,
        visible_rows,
        total_rows,
        // With no row on screen there is nothing to scroll *to*, so the only
        // legal offset is the top. Without this a short panel would accept
        // notches that move an invisible viewport.
        max_offset: if visible_rows == 0 {
            0
        } else {
            total_rows.saturating_sub(visible_rows)
        },
    }
}

/// Lay the grid's cards out for `order` (employee indices in workflow order) and
/// a scroll `offset` in rows.
///
/// The renderer draws from this and mouse hit-testing reads it, so a click lands
/// on the card it points at. Only whole rows are emitted, and the offset is
/// clamped here as well as in [`App::roster_scroll`], so geometry stays sane
/// even for a caller holding a stale offset.
pub(crate) fn grid(area: Rect, order: &[usize], offset: usize) -> Grid {
    let shape = shape(area, order.len());
    let offset = offset.min(shape.max_offset);

    let mut cells = Vec::new();
    let end = (offset + shape.visible_rows).min(shape.total_rows);
    // A zero-width panel (a terminal narrower than the fixed side panels) draws
    // nothing: emitting zero-width cards would only give hit-testing rectangles
    // that can never contain a point.
    if shape.cell_width > 0 {
        for row in offset..end {
            for col in 0..shape.cols {
                let Some(&employee) = order.get(row * shape.cols + col) else {
                    break; // the last row can be ragged — never pad it with ghosts
                };
                let x = area.x + (col as u16) * (shape.cell_width + COL_GAP);
                let y = shape.first_row + (row - offset) as u16 * ROW_PITCH;
                cells.push(Node {
                    employee,
                    area: Rect::new(x, y, shape.cell_width, STAGE_HEIGHT),
                });
            }
        }
    }

    Grid {
        cells,
        shape,
        offset,
        count: order.len(),
    }
}

/// The scroll offset that brings workflow `position` inside the viewport.
///
/// Scrolling is minimal: a move that is already visible changes nothing, one
/// past the bottom scrolls by a single row, and one above the top scrolls up to
/// its row. With no rows on screen there is nothing to reveal.
pub(crate) fn reveal(area: Rect, count: usize, position: usize, offset: usize) -> usize {
    let shape = shape(area, count);
    if shape.visible_rows == 0 {
        return 0;
    }
    let offset = offset.min(shape.max_offset);
    let row = position / shape.cols.max(1);
    if row < offset {
        row
    } else if row >= offset + shape.visible_rows {
        (row + 1 - shape.visible_rows).min(shape.max_offset)
    } else {
        offset
    }
}

/// The title row: `team · N`, plus ` · rows a-b/c` once the roster overflows the
/// viewport, so the user can tell there is more above or below.
///
/// The readout is graded: the full form, then a compact ` · a-b/c`, then nothing
/// — the count itself always survives. A 24-column panel (80×24) has room for
/// exactly the full form on the first page, and none for the wide one deeper in,
/// so a fixed string would silently lose the row numbers.
fn render_title(frame: &mut Frame, grid: &Grid) {
    let area = grid.title();
    if area.height == 0 || area.width == 0 {
        return;
    }

    let head = format!(" team · {}", grid.count);
    let mut spans = vec![
        Span::styled(
            " team ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("· {}", grid.count),
            Style::default().fg(Color::Gray),
        ),
    ];

    if grid.shape.visible_rows > 0 && grid.shape.total_rows > grid.shape.visible_rows {
        let first = grid.offset + 1;
        let last = (grid.offset + grid.shape.visible_rows).min(grid.shape.total_rows);
        let total = grid.shape.total_rows;
        let room = (area.width as usize)
            .saturating_sub(head.chars().count())
            .saturating_sub(1); // one column of breathing room before the next panel
        let text = [
            format!(" · rows {first}-{last}/{total}"),
            format!(" · {first}-{last}/{total}"),
        ]
        .into_iter()
        .find(|text| text.chars().count() <= room);
        if let Some(text) = text {
            spans.push(Span::styled(text, Style::default().fg(Color::DarkGray)));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The Jev header strip: one row, above the grid, saying whether the decision
/// answerer is on and which model it asks. Dropped wholesale when the panel is
/// too short — see [`shape`].
fn render_jev_header(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }

    let enabled = app.jev_enabled();
    let accent = if enabled {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let mut spans = vec![
        Span::styled(
            " jev ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("◆", Style::default().fg(accent)),
        Span::styled(
            if enabled { " on — @jev" } else { " off" },
            Style::default().fg(if enabled {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
    ];

    // The model id only when there is room for it: the on/off state is the part
    // that must survive a narrow panel.
    if enabled && let Some(model) = app.jev_model() {
        let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let remaining = (area.width as usize).saturating_sub(used + 3);
        if remaining > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
            spans.push(Span::styled(truncate(model, remaining), Color::Cyan));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_stage(frame: &mut Frame, app: &App, employee: &Employee, area: Rect, selected: bool) {
    let tick = app.frame();
    let accent = status::color(employee.status);
    let width = area.width.saturating_sub(2) as usize;

    let title = Span::styled(
        format!(" {} ", employee.stage_label()),
        Style::default().fg(accent).add_modifier(if selected {
            Modifier::BOLD
        } else {
            Modifier::DIM
        }),
    );
    let border = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(accent)
    };

    let lines = vec![
        node_head(
            status::icon_span(employee.status, tick),
            &employee.name,
            width,
        ),
        Line::from(Span::styled(
            truncate(&employee.role, width),
            Color::DarkGray,
        )),
        Line::from(Span::styled(employee.status.label(), accent)),
        model_line(employee.model.as_deref(), width, Color::Cyan),
    ];

    render_node(frame, area, title, border, lines);
}

/// Render one card: a bordered box with a coloured title tab and left-aligned
/// content. Every card is the same widget, so its geometry and styling live
/// here.
fn render_node(
    frame: &mut Frame,
    area: Rect,
    title: Span<'static>,
    border: Style,
    lines: Vec<Line<'static>>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(title);

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left),
        area,
    );
}

/// A card's head row: the status glyph, a gap, then the employee's name in bold,
/// truncated to the box's inner width.
fn node_head(glyph: Span<'static>, name: &str, inner_width: usize) -> Line<'static> {
    Line::from(vec![
        glyph,
        Span::raw(" "),
        Span::styled(
            truncate(name, inner_width.saturating_sub(2)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])
}

/// A card's model row: the model id in `accent`, or a dimmed `none` when
/// nothing is assigned. Truncated to the box's inner width like every other
/// row, so a long id can't push the box out of shape.
fn model_line(model: Option<&str>, inner_width: usize, accent: Color) -> Line<'static> {
    match model {
        Some(model) => Line::from(Span::styled(truncate(model, inner_width), accent)),
        None => Line::from(Span::styled(
            "none",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::jev::JevClient;
    use crate::message::Message;
    use crate::team::Team;
    use ratatui::{Terminal, backend::TestBackend};

    /// A panel with room for the title, the header, and several card rows.
    fn area() -> Rect {
        Rect::new(4, 1, 64, 30)
    }

    fn order(count: usize) -> Vec<usize> {
        (0..count).collect()
    }

    /// Draw one widget on a throwaway terminal and return the buffer's symbols —
    /// one place builds the box the content assertions read.
    fn node_text(draw: impl FnOnce(&mut Frame)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(draw).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    /// G1: three cards share a row, and the next row starts one `ROW_PITCH`
    /// below it.
    #[test]
    fn grid_lays_cards_out_three_to_a_row() {
        let grid = grid(area(), &order(7), 0);

        assert_eq!(grid.shape.cols, 3);
        assert_eq!(grid.cells.len(), 7, "every employee gets a card");

        assert_eq!(grid.cells[0].area.width, (area().width - 2) / 3);
        assert_eq!(grid.cells[0].area.height, STAGE_HEIGHT);

        assert_eq!(grid.cells[0].area.y, grid.cells[1].area.y);
        assert_eq!(grid.cells[1].area.y, grid.cells[2].area.y);
        assert_eq!(grid.cells[1].area.x, grid.cells[0].area.right() + COL_GAP);
        assert_eq!(grid.cells[2].area.x, grid.cells[1].area.right() + COL_GAP);

        // The fourth employee opens the second row.
        assert_eq!(
            grid.cells[3].area.y,
            grid.cells[0].area.y + STAGE_HEIGHT + ROW_GAP
        );
        assert_eq!(grid.cells[3].area.x, grid.cells[0].area.x);
        // A ragged last row simply stops early.
        assert_eq!(grid.cells[6].area.x, grid.cells[0].area.x);
    }

    /// G2/G8: cards never overlap, never leave the panel, and are never
    /// vertically clipped — in three columns and in the one-column fallback.
    #[test]
    fn grid_cards_are_disjoint_complete_and_inside_the_area() {
        // A wide panel (three columns) and the 24-column team panel of an 80×24
        // terminal (one column), at every height the roster actually sees.
        for area_width in [64u16, 24] {
            for count in [1usize, 3, 4, 7, 12, 13] {
                for height in 1..=34u16 {
                    let area = Rect::new(4, 1, area_width, height);
                    let grid = grid(area, &order(count), 0);

                    for cell in &grid.cells {
                        assert_eq!(cell.area.height, STAGE_HEIGHT, "a card is never cut");
                        assert!(cell.area.right() <= area.right(), "{:?}", cell.area);
                        assert!(cell.area.bottom() <= area.bottom(), "{:?}", cell.area);
                        assert!(cell.area.width >= MIN_CELL_WIDTH.min(area_width));
                    }
                    for (index, cell) in grid.cells.iter().enumerate() {
                        for other in &grid.cells[index + 1..] {
                            assert!(
                                !cell.area.intersects(other.area),
                                "{:?} ∩ {:?}",
                                cell.area,
                                other.area
                            );
                        }
                    }
                }
            }
        }
    }

    /// G3/G8: a scroll offset shows whole rows only, starting at that row.
    #[test]
    fn grid_clips_whole_rows_at_a_non_zero_offset() {
        let area = Rect::new(0, 1, 64, 16); // title + header + two card rows
        let grid = grid(area, &order(12), 1);

        assert_eq!(grid.shape.visible_rows, 2);
        assert_eq!(grid.offset, 1);
        assert_eq!(grid.cells.len(), 6, "two full rows, nothing partial");
        assert_eq!(grid.cells[0].employee, 3, "the second row comes first");
        for cell in &grid.cells {
            assert_eq!(cell.area.height, STAGE_HEIGHT);
            assert!(cell.area.bottom() <= area.bottom());
        }
    }

    /// G3: the offset clamps to the last page that still shows whole rows — no
    /// blank-row overscroll.
    #[test]
    fn grid_clamps_the_offset_to_the_last_full_page() {
        let area = Rect::new(0, 1, 64, 16); // two visible rows
        let grid = grid(area, &order(12), 99);

        assert_eq!(grid.shape.total_rows, 4);
        assert_eq!(grid.shape.max_offset, 2);
        assert_eq!(grid.offset, 2);
        assert_eq!(grid.cells.first().map(|cell| cell.employee), Some(6));
        assert_eq!(
            grid.cells.last().map(|cell| cell.employee),
            Some(11),
            "the viewport ends on the last employee, not past it"
        );
        for cell in &grid.cells {
            assert!(cell.area.bottom() <= area.bottom(), "{:?}", cell.area);
        }
    }

    /// G7: below a 12-cell card the grid degrades to one full-width column
    /// instead of squeezing three unreadable ones.
    #[test]
    fn grid_falls_back_to_one_column_below_twelve_cell_width() {
        let wide = grid(Rect::new(0, 1, 38, 30), &order(4), 0);
        assert_eq!(wide.shape.cols, 3);
        assert_eq!(wide.cells[0].area.width, 12);

        let narrow = grid(Rect::new(0, 1, 37, 30), &order(4), 0);
        assert_eq!(narrow.shape.cols, 1);
        assert_eq!(narrow.cells[0].area.width, 37);
        assert_eq!(narrow.cells[0].area.x, 0);
        assert_eq!(narrow.cells[1].area.x, 0);
        assert_eq!(
            narrow.cells[1].area.y,
            narrow.cells[0].area.y + STAGE_HEIGHT + ROW_GAP
        );
    }

    /// G9: the Jev header is drawn when a card row still fits beneath it, and
    /// dropped before that row.
    #[test]
    fn grid_drops_the_header_before_a_card_row() {
        // 8 rows = title + header + one card row.
        let with_header = shape(Rect::new(0, 1, 64, 8), 4);
        assert_eq!(with_header.header.height, 1);
        assert_eq!(with_header.visible_rows, 1);

        // 7 rows: the header would cost the only card row, so it goes.
        let without = shape(Rect::new(0, 1, 64, 7), 4);
        assert_eq!(without.header.height, 0);
        assert_eq!(without.visible_rows, 1);

        // 6 rows: not even one complete card fits.
        let empty = shape(Rect::new(0, 1, 64, 6), 4);
        assert_eq!(empty.visible_rows, 0);
        assert_eq!(
            empty.max_offset, 0,
            "nothing on screen means nowhere to scroll to"
        );
        assert!(grid(Rect::new(0, 1, 64, 6), &order(4), 0).cells.is_empty());
    }

    /// A zero-width panel draws no cards, so it must not offer scroll either: at
    /// 40×24 the team panel collapses and the old geometry still reported ten
    /// rows of scroll over an empty viewport.
    #[test]
    fn a_zero_width_panel_has_no_rows_and_no_scroll() {
        let empty = shape(Rect::new(0, 1, 0, 23), 12);
        assert_eq!(empty.cell_width, 0);
        assert_eq!(empty.visible_rows, 0, "nothing is drawn, so nothing fits");
        assert_eq!(empty.max_offset, 0, "an empty viewport cannot scroll");

        let grid = grid(Rect::new(0, 1, 0, 23), &order(12), 5);
        assert!(grid.cells.is_empty());
        assert_eq!(grid.offset, 0, "the offset clamps back to the top");
    }

    /// A one-row panel keeps the title and nothing else; a zero-row panel draws
    /// nothing at all.
    #[test]
    fn grid_degrades_to_nothing_without_height() {
        let title_only = grid(Rect::new(0, 1, 64, 1), &order(4), 0);
        assert_eq!(title_only.title().height, 1);
        assert_eq!(title_only.header().height, 0);
        assert!(title_only.cells.is_empty());

        let nothing = grid(Rect::new(0, 1, 64, 0), &order(4), 0);
        assert_eq!(nothing.title().height, 0);
        assert!(nothing.cells.is_empty());
    }

    /// An empty roster has no rows to scroll and no cards to draw.
    #[test]
    fn grid_of_an_empty_roster_is_empty() {
        let grid = grid(area(), &[], 3);
        assert!(grid.cells.is_empty());
        assert_eq!(grid.shape.total_rows, 0);
        assert_eq!(grid.shape.max_offset, 0);
        assert_eq!(grid.offset, 0);
    }

    /// Every card is tall enough for all [`CONTENT_LINES`] content rows, which is
    /// what keeps the cards level.
    #[test]
    fn stage_boxes_fit_their_content_rows() {
        assert_eq!(STAGE_HEIGHT, CONTENT_LINES + 2);

        let grid = grid(area(), &order(7), 0);
        for cell in &grid.cells {
            assert_eq!(cell.area.height, STAGE_HEIGHT, "{:?}", cell.area);
        }
    }

    /// The reveal rule scrolls by the minimum: an already-visible selection does
    /// not move the viewport, one past the bottom scrolls by a row.
    #[test]
    fn reveal_scrolls_the_minimum_distance() {
        let area = Rect::new(0, 1, 64, 16); // two visible rows of three columns

        assert_eq!(reveal(area, 12, 0, 0), 0, "already visible");
        assert_eq!(reveal(area, 12, 2, 0), 0, "last cell of the first row");
        assert_eq!(reveal(area, 12, 3, 0), 0, "the second row is visible too");
        assert_eq!(reveal(area, 12, 6, 0), 1, "the third row scrolls by one");
        assert_eq!(reveal(area, 12, 9, 0), 2, "clamped to the last page");
        assert_eq!(reveal(area, 12, 6, 2), 2, "already visible near the end");
        assert_eq!(reveal(area, 12, 3, 2), 1, "above the viewport scrolls up");
    }

    /// The model row carries the assigned id — truncated to the box's inner
    /// width — and a dimmed `none` when nothing is assigned.
    #[test]
    fn model_row_truncates_to_the_inner_width_and_placeholders_none() {
        let assigned = model_line(Some("deepseek-v4.1-flash-free"), 40, Color::Cyan);
        assert_eq!(assigned.spans[0].content, "deepseek-v4.1-flash-free");

        let clipped = model_line(Some("deepseek-v4.1-flash-free"), 10, Color::Cyan);
        assert_eq!(clipped.spans[0].content, "deepseek-…");
        assert!(clipped.spans[0].content.chars().count() <= 10);

        let unassigned = model_line(None, 40, Color::Cyan);
        assert_eq!(unassigned.spans[0].content, "none");
    }

    /// A card prints its assigned model id inside the box.
    #[test]
    fn stage_renders_the_assigned_model() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
        let mut team = Team::starter();
        team.set_model(1, "kenari/jev-1-13-free".to_string(), None);
        let app = App::with_team(Config::default(), tx, team);

        let employee = app.team().employees()[1].clone();
        let text = node_text(|frame| {
            let area = frame.area();
            render_stage(frame, &app, &employee, area, false);
        });

        assert!(text.contains("kenari/jev-1-13-free"), "{text}");
    }

    /// The Jev header prints its decision model next to the on/off state, and a
    /// bare `off` while Jev is disabled.
    #[test]
    fn jev_header_renders_its_decision_model() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
        let mut app = App::new(Config::default(), tx);

        let off = node_text(|frame| {
            let area = frame.area();
            render_jev_header(frame, &app, area);
        });
        assert!(off.contains("off"), "{off}");
        assert!(!off.contains("@jev"), "no delegation target while off");

        app.set_jev(Some(
            JevClient::new("key", "https://example.test", "jev-1-13-free").unwrap(),
        ));
        let on = node_text(|frame| {
            let area = frame.area();
            render_jev_header(frame, &app, area);
        });
        assert!(on.contains("on — @jev"), "{on}");
        assert!(on.contains("jev-1-13-free"), "{on}");
    }
}
