//! The team grid: a fixed three-column, one-column-gap grid of employee cards,
//! ordered by workflow role (see [`Employee::workflow_role`] →
//! [`Role::order`](crate::role::Role::order)).
//!
//! The cards are split into **two sections**: the employees that are mid-turn on
//! top (`◆ working`), and everyone else below (`idle`). Each section is a heading
//! band followed by its card rows, so a glance at the top of the panel answers
//! "who is working right now" without reading every card's status line.
//!
//! Above both sits the **Jev strip** (the shared decision answerer, with its
//! model) over a dashed **bus** that every card hangs off by its own one-column
//! **riser**. Employees are peers, so a hand-off between any two of them travels
//! that path — source riser → bus → destination riser — and `ui::wires` draws the
//! packets. Jev is not an employee, so it is the strip, not a card.
//!
//! The grid is a **viewport**: the wheel scrolls it, the arrows move the
//! selection and reveal it, and only whole bands are ever drawn. [`shape`] and
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
use crate::ui::{status, truncate, wires};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let sections = app.card_sections();

    if sections.len() == 0 {
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
    let grid = grid(area, &sections, app.roster_scroll());

    render_title(frame, &grid);
    render_strip(frame, app, grid.strip());
    render_bus(frame, &grid);

    // The headings go down before the wires, so a packet crossing a section band
    // is drawn on top of it rather than under it.
    for &(section, rect) in &grid.headings {
        let count = match section {
            Section::Active => sections.active.len(),
            Section::Idle => sections.idle.len(),
        };
        render_heading(frame, section, rect, count);
    }

    wires::render(frame, app, &grid);

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

/// The one-column gutter immediately left of each card column, where that
/// column's delegation riser is drawn.
const RISER_WIDTH: u16 = 1;

/// A card narrower than this is unreadable — the stage label alone would
/// overflow its title tab — so the grid falls back to a single full-width
/// column rather than squeezing three.
const MIN_CELL_WIDTH: u16 = 12;

/// One blank row between card rows, so cards never touch vertically.
const ROW_GAP: u16 = 1;

/// Which section a card belongs to. Active employees are drawn above idle ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    /// Mid-turn: `Thinking` or `Working`.
    Active,
    /// Everyone else.
    Idle,
}

impl Section {
    /// The heading printed above the section's cards.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Section::Active => "working",
            Section::Idle => "idle",
        }
    }

    /// The heading's glyph: a lit dot for work in flight, a hollow one for rest.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Section::Active => "◆",
            Section::Idle => "◇",
        }
    }
}

/// The roster split into the two sections the panel draws, each in card order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Sections {
    /// Employees mid-turn, in workflow order.
    pub(crate) active: Vec<usize>,
    /// Everyone else, in workflow order.
    pub(crate) idle: Vec<usize>,
}

impl Sections {
    /// Split `order` (already in workflow order) by each employee's status.
    pub(crate) fn new(order: &[usize], is_active: impl Fn(usize) -> bool) -> Self {
        let mut active = Vec::new();
        let mut idle = Vec::new();
        for &index in order {
            if is_active(index) {
                active.push(index);
            } else {
                idle.push(index);
            }
        }
        Self { active, idle }
    }

    /// Every employee, active section first — the order the arrow keys walk.
    pub(crate) fn flat(&self) -> Vec<usize> {
        self.active
            .iter()
            .chain(self.idle.iter())
            .copied()
            .collect()
    }

    /// How many employees the two sections hold.
    pub(crate) fn len(&self) -> usize {
        self.active.len() + self.idle.len()
    }

    /// The card rows the panel draws, top to bottom: the active section's rows
    /// first, then the idle section's.
    fn rows(&self, cols: usize) -> Vec<Row> {
        let cols = cols.max(1);
        let mut rows = Vec::new();
        for (section, employees) in [(Section::Active, &self.active), (Section::Idle, &self.idle)] {
            for row in 0..employees.len().div_ceil(cols) {
                rows.push(Row {
                    section,
                    first: row * cols,
                    // Only the section's first row carries its heading, so a
                    // heading can never be scrolled away from the cards it
                    // labels.
                    heading: row == 0,
                });
            }
        }
        rows
    }
}

/// One card row, plus the section heading that may sit above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    section: Section,
    /// Index of the row's first employee within the section's list.
    first: usize,
    /// True when this is its section's first row, so the heading is drawn above
    /// it.
    heading: bool,
}

impl Row {
    /// The row's height including its heading, before the gap that separates it
    /// from the previous row.
    fn block(self) -> u16 {
        STAGE_HEIGHT + if self.heading { 1 } else { 0 }
    }

    /// The row's height plus the gap above it (none above the first row).
    fn span(self, index: usize) -> u16 {
        self.block() + if index == 0 { 0 } else { ROW_GAP }
    }
}

/// One drawn card: the employee it stands for (an index into the team, in
/// workflow order) and the rectangle it occupies.
pub(crate) struct Node {
    pub(crate) employee: usize,
    pub(crate) area: Rect,
    /// The `x` of this card's riser — the one-column gutter to its left, where
    /// the wire from the bus meets the card's top border.
    pub(crate) riser_x: u16,
}

/// The fixed part of the grid for one panel size and roster: the column split,
/// the pipeline bands, and how many card rows the remaining height holds.
///
/// Split out from [`Grid`] because three callers need the counts without
/// drawing anything — the reveal rule, the scroll clamp, and the title.
pub(crate) struct Shape {
    pub(crate) cols: usize,
    pub(crate) cell_width: u16,
    /// Where each column starts (the card's left edge, riser aside).
    pub(crate) column_x: Vec<u16>,
    pub(crate) title: Rect,
    /// The Jev strip: the shared answerer's row.
    pub(crate) strip: Rect,
    /// The dashed bus the risers hang from.
    pub(crate) bus: Rect,
    /// The `y` of the first row when the viewport is at offset 0.
    pub(crate) first_row: u16,
    /// The card rows themselves, in draw order (active section first).
    rows: Vec<Row>,
    /// Card rows that fit in the viewport, starting from the current offset.
    pub(crate) visible_rows: usize,
    /// The largest offset that still shows whole rows.
    pub(crate) max_offset: usize,
    /// Card rows in the whole roster — the title's total.
    pub(crate) total_card_rows: usize,
    /// Card rows in the visible window.
    pub(crate) visible_card_rows: usize,
    /// How many employees the active section holds.
    pub(crate) active_count: usize,
    /// The height the rows may occupy (below the title, strip, and bus).
    pub(crate) available: u16,
}

/// The grid's geometry for one panel, roster, and scroll offset: the shape, the
/// cards on screen, and the offset actually used.
pub(crate) struct Grid {
    pub(crate) cells: Vec<Node>,
    pub(crate) shape: Shape,
    pub(crate) offset: usize,
    /// How many employees the roster holds — the title's `N`.
    pub(crate) count: usize,
    /// The section headings on screen, as `(section, rect)`.
    pub(crate) headings: Vec<(Section, Rect)>,
}

impl Grid {
    pub(crate) fn title(&self) -> Rect {
        self.shape.title
    }

    pub(crate) fn strip(&self) -> Rect {
        self.shape.strip
    }

    pub(crate) fn bus(&self) -> Rect {
        self.shape.bus
    }
}

/// Lay the fixed grid out in `area` for a roster split into `sections`.
///
/// **Degradation.** The bands are dropped in the order that costs the user
/// least: the bus first, then the Jev strip, and only when drawing them would
/// cost a card row. A row is only counted when its full [`STAGE_HEIGHT`] fits,
/// so a card is never drawn with a top border and no body.
pub(crate) fn shape(area: Rect, sections: &Sections) -> Shape {
    // Each column is `riser | card`, with one gap between columns. Three columns
    // therefore need `3 * riser + 3 * card + 2 * gap`; when even a 12-cell card
    // doesn't fit in that budget the grid falls back to one column rather than
    // squeezing three unreadable ones.
    let chrome = RISER_WIDTH * COLS + COL_GAP * (COLS - 1);
    let shared = area.width.saturating_sub(chrome) / COLS;
    let (cols, cell_width) = if shared >= MIN_CELL_WIDTH {
        (COLS as usize, shared)
    } else {
        (1, area.width.saturating_sub(RISER_WIDTH))
    };

    // Each column is `riser | card`, the columns laid out left to right with one
    // gap after each card.
    let column_x: Vec<u16> = (0..cols as u16)
        .map(|col| {
            let pitch = cell_width + RISER_WIDTH + COL_GAP;
            area.x + col * pitch + RISER_WIDTH
        })
        .collect();

    let rows = sections.rows(cols);
    let total_card_rows = rows.len();

    let title_height = area.height.min(1);
    let title = Rect::new(area.x, area.y, area.width, title_height);
    let below_title = area.height.saturating_sub(title_height);

    // How many whole card rows fit in `height`, counting each row's heading.
    let rows_fitting = |height: u16| {
        let mut used = 0u16;
        let mut count = 0usize;
        for (index, row) in rows.iter().enumerate() {
            let need = row.span(index);
            if used + need > height {
                break;
            }
            used += need;
            count += 1;
        }
        count
    };

    // The pipeline costs up to two rows (strip + bus). Each is kept only when it
    // does not cost a card row, and the bus goes before the strip: the strip is
    // Jev, the bus is the wire between employees.
    let without = rows_fitting(below_title);
    let with_strip = rows_fitting(below_title.saturating_sub(1));
    let strip_height = u16::from(with_strip == without && below_title > STAGE_HEIGHT);
    let with_bus = rows_fitting(below_title.saturating_sub(strip_height + 1));
    let bus_height =
        u16::from(strip_height == 1 && with_bus == without && below_title > STAGE_HEIGHT);

    let strip = Rect::new(area.x, area.y + title_height, area.width, strip_height);
    let bus = Rect::new(
        area.x,
        area.y + title_height + strip_height,
        area.width,
        bus_height,
    );

    let available = below_title.saturating_sub(strip_height + bus_height);

    // The bottom-most scroll position is the *first* one whose tail — every row
    // from there to the end, plus the sticky heading it needs — still fits whole.
    // That is the last page: it ends exactly on the last row, and scrolling
    // further would only add blank rows.
    let tail_fits = |offset: usize| {
        let used: u16 = (offset..rows.len())
            .map(|index| rows[index].span(index - offset))
            .fold(sticky_rows(&rows, offset), |sum, need| {
                sum.saturating_add(need)
            });
        used <= available
    };

    // A zero-width panel draws no cards at all (see [`grid`]), so it also has no
    // rows to scroll: `visible_rows` and `max_offset` must agree with what is
    // drawn, or the wheel would accept notches over an empty viewport.
    let (visible_rows, max_offset) = if cell_width == 0 || rows.is_empty() {
        (0, 0)
    } else {
        let visible_rows = rows_fitting(available.saturating_sub(sticky_rows(&rows, 0)));
        let max_offset = if visible_rows == 0 {
            0 // nothing drawn, so there is nowhere to scroll to
        } else {
            (0..rows.len())
                .find(|&offset| tail_fits(offset))
                .unwrap_or(rows.len().saturating_sub(1))
        };
        (visible_rows, max_offset)
    };

    let active_count = sections.active.len();
    let visible_card_rows = visible_rows.min(rows.len());

    Shape {
        cols,
        cell_width,
        column_x,
        title,
        strip,
        bus,
        first_row: area.y + title_height + strip_height + bus_height,
        rows,
        visible_rows,
        max_offset,
        total_card_rows,
        visible_card_rows,
        active_count,
        available,
    }
}

/// The rows reserved above the window for a sticky section heading: one when the
/// window starts mid-section (so the label is never scrolled away from its
/// cards), none when it starts on a section's first row.
fn sticky_rows(rows: &[Row], offset: usize) -> u16 {
    match rows.get(offset) {
        Some(row) if !row.heading => 1,
        _ => 0,
    }
}

/// The window drawn at `offset`: the sticky heading it needs (if any) and how
/// many whole card rows fit below it.
fn window(shape: &Shape, offset: usize) -> (Option<Section>, usize) {
    let sticky = shape
        .rows
        .get(offset)
        .filter(|row| !row.heading)
        .map(|row| row.section);
    let height = shape.available.saturating_sub(u16::from(sticky.is_some()));
    let mut used = 0u16;
    let mut count = 0usize;
    for (index, row) in shape.rows.iter().enumerate().skip(offset) {
        let need = row.span(index - offset);
        if used + need > height {
            break;
        }
        used += need;
        count += 1;
    }
    (sticky, count)
}

/// Lay the grid's cards out for `sections` and a scroll `offset` in card rows.
///
/// The renderer draws from this and mouse hit-testing reads it, so a click lands
/// on the card it points at. Only whole rows are emitted, and the offset is
/// clamped here as well as in [`App::roster_scroll`], so geometry stays sane
/// even for a caller holding a stale offset.
pub(crate) fn grid(area: Rect, sections: &Sections, offset: usize) -> Grid {
    let shape = shape(area, sections);
    let offset = offset.min(shape.max_offset);

    let mut cells = Vec::new();
    let mut headings = Vec::new();
    let mut y = shape.first_row;

    // A zero-width panel (a terminal narrower than the fixed side panels) draws
    // nothing: emitting zero-width cards would only give hit-testing rectangles
    // that can never contain a point.
    if shape.cell_width > 0 {
        let (sticky, count) = window(&shape, offset);
        if let Some(section) = sticky {
            headings.push((section, Rect::new(area.x, y, area.width, 1)));
            y = y.saturating_add(1);
        }
        let end = (offset + count).min(shape.rows.len());
        for (index, row) in shape.rows.iter().enumerate().take(end).skip(offset) {
            // The first drawn row starts flush with `first_row`; every later row
            // is separated from it by one gap.
            if index > offset {
                y = y.saturating_add(ROW_GAP);
            }
            if row.heading {
                headings.push((row.section, Rect::new(area.x, y, area.width, 1)));
                y = y.saturating_add(1);
            }
            let employees = match row.section {
                Section::Active => &sections.active,
                Section::Idle => &sections.idle,
            };
            for col in 0..shape.cols {
                let Some(&employee) = employees.get(row.first + col) else {
                    break; // the last row can be ragged — never pad it
                };
                let Some(&x) = shape.column_x.get(col) else {
                    break;
                };
                cells.push(Node {
                    employee,
                    area: Rect::new(x, y, shape.cell_width, STAGE_HEIGHT),
                    riser_x: x.saturating_sub(RISER_WIDTH),
                });
            }
            y = y.saturating_add(STAGE_HEIGHT);
        }
    }

    Grid {
        cells,
        shape,
        offset,
        count: sections.len(),
        headings,
    }
}

/// The scroll offset that brings `position` (an index into the roster's card
/// order) inside the viewport.
///
/// Scrolling is minimal: a row that is already visible changes nothing, one past
/// the bottom scrolls just far enough to show it, and one above the top scrolls
/// up to it. With no rows on screen there is nothing to reveal.
pub(crate) fn reveal(area: Rect, sections: &Sections, position: usize, offset: usize) -> usize {
    let shape = shape(area, sections);
    if shape.visible_rows == 0 {
        return 0;
    }
    let offset = offset.min(shape.max_offset);

    let Some(row) = row_for(&shape, sections, position) else {
        return offset;
    };

    if row < offset {
        return row;
    }
    if visible(&shape, offset, row) {
        return offset;
    }
    // Scroll down the minimum distance that still shows the whole row.
    (offset + 1..=row)
        .rev()
        .find(|&start| visible(&shape, start, row))
        .map(|start| start.min(shape.max_offset))
        .unwrap_or(row.min(shape.max_offset))
}

/// True when `row` is fully inside the window that starts at `offset`.
fn visible(shape: &Shape, offset: usize, row: usize) -> bool {
    if row < offset {
        return false;
    }
    let (_, count) = window(shape, offset);
    row < offset + count
}

/// The row index the employee at card `position` is drawn in.
fn row_for(shape: &Shape, sections: &Sections, position: usize) -> Option<usize> {
    let cols = shape.cols.max(1);
    let active = sections.active.len();
    let (section, index) = if position < active {
        (Section::Active, position)
    } else {
        (Section::Idle, position - active)
    };
    let row = index / cols;
    shape
        .rows
        .iter()
        .position(|r| r.section == section && r.first / cols == row)
}

/// The title row: `team · N`, plus ` · N working` while anyone is mid-turn, and
/// ` · rows a-b/c` once the roster overflows the viewport, so the user can tell
/// there is more above or below.
///
/// The readout is graded: the full form, then a compact one, then nothing — the
/// count itself always survives. A 24-column panel (80×24) has room for exactly
/// the full form on the first page, and none for the wide one deeper in, so a
/// fixed string would silently lose the numbers.
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

    // The active count is the number the two sections exist to surface, so it is
    // the first thing added and the last thing dropped.
    let working = grid.shape.active_count;
    let overflow = grid.shape.total_card_rows > grid.shape.visible_card_rows;
    let room_for = |used: usize| (area.width as usize).saturating_sub(used + 1);

    let working_text = format!(" · {working} working");
    if working > 0 && working_text.chars().count() <= room_for(head.chars().count()) {
        spans.push(Span::styled(
            working_text.clone(),
            Style::default().fg(Color::Green),
        ));
    }

    if overflow {
        let first = grid.offset + 1;
        let last = (grid.offset + grid.shape.visible_rows).min(grid.shape.total_card_rows);
        let total = grid.shape.total_card_rows;
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let text = [
            format!(" · rows {first}-{last}/{total}"),
            format!(" · {first}-{last}/{total}"),
        ]
        .into_iter()
        .find(|text| text.chars().count() <= room_for(used));
        if let Some(text) = text {
            spans.push(Span::styled(text, Style::default().fg(Color::DarkGray)));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A section heading: `◆ working · 2` above the active cards, `◇ idle · 5` above
/// the rest. Drawn between the bus and the section's first card row.
fn render_heading(frame: &mut Frame, section: Section, area: Rect, count: usize) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let accent = match section {
        Section::Active => Color::Green,
        Section::Idle => Color::DarkGray,
    };
    let mut spans = vec![
        Span::styled(
            format!(" {} ", section.glyph()),
            Style::default().fg(accent),
        ),
        Span::styled(
            section.label().to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
    ];
    let tail = format!(" · {count} ");
    if spans
        .iter()
        .map(|s| s.content.chars().count())
        .sum::<usize>()
        + tail.chars().count()
        <= area.width as usize
    {
        spans.push(Span::styled(tail, Style::default().fg(Color::DarkGray)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The Jev strip: the shared decision answerer on one row above the bus, saying
/// whether it is on and which model it asks. Dropped wholesale when the panel is
/// too short — see [`shape`].
///
/// Jev is not an employee: every employee delegates questions to it, so it is a
/// shared resource above the cards rather than one of them.
fn render_strip(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let spans = jev_spans(app);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The strip's content: on/off and, when there is room, its decision model.
fn jev_spans(app: &App) -> Vec<Span<'static>> {
    let enabled = app.jev_enabled();
    let accent = if enabled {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let mut spans = vec![
        Span::styled(
            "jev ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("◆", Style::default().fg(accent)),
        Span::styled(
            if enabled { " on — @jev " } else { " off " },
            Style::default().fg(if enabled {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
    ];
    if enabled && let Some(model) = app.jev_model() {
        spans.push(Span::styled(
            format!("· {model} "),
            Style::default().fg(Color::Cyan),
        ));
    }
    spans
}

/// The bus: one dashed row spanning the panel, the horizontal wire every card's
/// riser hangs from — the shared road a peer-to-peer hand-off travels.
///
/// Drawn dim when nothing is moving. The packets that travel it are
/// [`wires`]' business; this is only the track.
fn render_bus(frame: &mut Frame, grid: &Grid) {
    let area = grid.bus();
    if area.height == 0 || area.width == 0 {
        return;
    }
    let style = Style::default().fg(Color::DarkGray);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            wires::BUS.repeat(area.width as usize),
            style,
        ))),
        area,
    );
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

    /// A panel with room for the title, the strip, and several card rows.
    fn area() -> Rect {
        Rect::new(4, 1, 64, 30)
    }

    /// `count` employees, all idle: one section, drawn exactly like a flat grid.
    fn idle(count: usize) -> Sections {
        Sections {
            active: Vec::new(),
            idle: (0..count).collect(),
        }
    }

    /// `active` employees on top, `idle` below, both in order.
    fn split(active: usize, idle: usize) -> Sections {
        Sections {
            active: (0..active).collect(),
            idle: (active..active + idle).collect(),
        }
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

    /// The grid split into its two sections: the active employees first, in
    /// workflow order, then the idle ones — so the arrow keys and the drawing
    /// agree on one order.
    #[test]
    fn sections_split_active_from_idle_and_keep_workflow_order() {
        let sections = Sections::new(&[3, 1, 4, 0, 2], |index| index == 1 || index == 4);
        assert_eq!(sections.active, vec![1, 4]);
        assert_eq!(sections.idle, vec![3, 0, 2]);
        assert_eq!(sections.flat(), vec![1, 4, 3, 0, 2]);
        assert_eq!(sections.len(), 5);

        // Nobody working: one section, and the flat order is untouched.
        let all_idle = Sections::new(&[3, 1, 4], |_| false);
        assert!(all_idle.active.is_empty());
        assert_eq!(all_idle.flat(), vec![3, 1, 4]);

        // Everybody working: the idle section simply disappears.
        let all_active = Sections::new(&[3, 1, 4], |_| true);
        assert!(all_active.idle.is_empty());
        assert_eq!(all_active.flat(), vec![3, 1, 4]);
    }

    /// The panel draws two sections: a `working` heading over the active cards,
    /// then an `idle` heading over the rest — and never an empty section.
    #[test]
    fn the_grid_draws_a_heading_per_non_empty_section() {
        let both = grid(area(), &split(2, 4), 0);
        let drawn: Vec<Section> = both.headings.iter().map(|(s, _)| *s).collect();
        assert_eq!(drawn, vec![Section::Active, Section::Idle]);

        // The active heading sits above every active card, the idle heading
        // between the two groups.
        let (_, active_heading) = both.headings[0];
        let (_, idle_heading) = both.headings[1];
        let active_bottom = both.cells[..2]
            .iter()
            .map(|cell| cell.area.bottom())
            .max()
            .expect("two active cards");
        assert!(active_heading.y < both.cells[0].area.y);
        assert!(idle_heading.y > active_bottom);
        assert!(idle_heading.y < both.cells[2].area.y);

        // Only the idle section: one heading, no `working` band.
        let idle_only = grid(area(), &idle(4), 0);
        assert_eq!(idle_only.headings.len(), 1);
        assert_eq!(idle_only.headings[0].0, Section::Idle);

        // Only the active section: the idle band is not drawn at all.
        let active_only = grid(area(), &split(4, 0), 0);
        assert_eq!(active_only.headings.len(), 1);
        assert_eq!(active_only.headings[0].0, Section::Active);
    }

    /// G1: three cards share a row, and the next row starts one pitch below it.
    #[test]
    fn grid_lays_cards_out_three_to_a_row() {
        let grid = grid(area(), &idle(7), 0);

        assert_eq!(grid.shape.cols, 3);
        assert_eq!(grid.cells.len(), 7, "every employee gets a card");

        // Three cards plus three risers plus two gaps share the panel width.
        assert_eq!(
            grid.cells[0].area.width,
            (area().width - RISER_WIDTH * 3 - COL_GAP * 2) / 3
        );
        assert_eq!(grid.cells[0].area.height, STAGE_HEIGHT);

        assert_eq!(grid.cells[0].area.y, grid.cells[1].area.y);
        assert_eq!(grid.cells[1].area.y, grid.cells[2].area.y);
        // A card sits one riser plus one gap right of the previous card.
        assert_eq!(
            grid.cells[1].area.x,
            grid.cells[0].area.right() + RISER_WIDTH + COL_GAP
        );
        assert_eq!(
            grid.cells[2].area.x,
            grid.cells[1].area.right() + RISER_WIDTH + COL_GAP
        );

        // The fourth employee opens the second row.
        assert_eq!(
            grid.cells[3].area.y,
            grid.cells[0].area.y + STAGE_HEIGHT + ROW_GAP
        );
        assert_eq!(grid.cells[3].area.x, grid.cells[0].area.x);
        // A ragged last row simply stops early.
        assert_eq!(grid.cells[6].area.x, grid.cells[0].area.x);
    }

    /// A section boundary costs one heading row plus the usual gap, and nothing
    /// else: cards below it are pushed down, not re-laid out.
    #[test]
    fn a_section_boundary_costs_one_heading_row() {
        let flat = grid(area(), &idle(4), 0);
        let split = grid(area(), &split(2, 2), 0);

        assert_eq!(
            flat.cells[0].area, split.cells[0].area,
            "the first card starts at the same place either way"
        );
        // Two headings (`working`, `idle`) plus their gaps separate the groups.
        assert!(
            split.cells[2].area.y > flat.cells[2].area.y,
            "the idle cards sit below the working section's heading"
        );
        // The heading itself is exactly one row, on the panel's left edge.
        let (_, heading) = split.headings[1];
        assert_eq!(heading.height, 1);
        assert_eq!(heading.x, area().x);
        assert_eq!(heading.width, area().width);
    }

    /// G2/G8: cards never overlap, never leave the panel, and are never
    /// vertically clipped — in three columns and in the one-column fallback.
    #[test]
    fn grid_cards_are_disjoint_complete_and_inside_the_area() {
        // A wide panel (three columns) and the 24-column team panel of an 80×24
        // terminal (one column), at every height the roster actually sees, with
        // and without a section split.
        for area_width in [64u16, 24] {
            for (active, idle_count) in [(0usize, 1usize), (0, 3), (2, 2), (0, 7), (3, 9), (12, 1)]
            {
                for height in 1..=34u16 {
                    let area = Rect::new(4, 1, area_width, height);
                    let sections = split(active, idle_count);
                    let grid = grid(area, &sections, 0);

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
                    // A heading never covers a card either.
                    for (_, heading) in &grid.headings {
                        for cell in &grid.cells {
                            assert!(
                                !heading.intersects(cell.area),
                                "heading {heading:?} covers card {:?}",
                                cell.area
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
        let area = Rect::new(0, 1, 64, 16); // title + strip + bus + two rows
        let sections = idle(12);
        let grid = grid(area, &sections, 1);

        assert_eq!(grid.offset, 1);
        assert!(!grid.cells.is_empty());
        assert_eq!(grid.cells.len() % 3, 0, "whole rows only");
        for cell in &grid.cells {
            assert_eq!(cell.area.height, STAGE_HEIGHT);
            assert!(cell.area.bottom() <= area.bottom());
        }
        // The second row comes first. The section heading is sticky: scrolled
        // into the middle of a section, it stays pinned above the cards so the
        // label never leaves them.
        assert_eq!(grid.cells[0].employee, 3);
        assert_eq!(grid.headings.len(), 1, "the section stays labelled");
        assert_eq!(grid.headings[0].0, Section::Idle);
        assert_eq!(grid.headings[0].1.y, grid.shape.first_row);
        assert!(grid.cells[0].area.y > grid.headings[0].1.y);
    }

    /// G3: the offset clamps to the last position whose tail still fits — no
    /// clipped band and no blank-row overscroll.
    #[test]
    fn grid_clamps_the_offset_to_the_last_full_page() {
        let area = Rect::new(0, 1, 64, 16);
        let sections = idle(12);
        let grid = grid(area, &sections, 99);

        assert_eq!(grid.offset, grid.shape.max_offset);
        assert_eq!(
            grid.cells.last().map(|cell| cell.employee),
            Some(11),
            "the viewport ends on the last employee, not past it"
        );
        for cell in &grid.cells {
            assert!(cell.area.bottom() <= area.bottom(), "{:?}", cell.area);
        }
        // Scrolling one further would clip the last card row.
        let further = super::grid(area, &sections, grid.offset + 1);
        assert_eq!(further.offset, grid.offset, "the clamp holds");
    }

    /// G7: below a 12-cell card the grid degrades to one full-width column
    /// instead of squeezing three unreadable ones.
    #[test]
    fn grid_falls_back_to_one_column_below_twelve_cell_width() {
        // Three columns need `3 * (12 + riser + gap) - gap` = 41 columns.
        let wide = grid(Rect::new(0, 1, 41, 30), &idle(4), 0);
        assert_eq!(wide.shape.cols, 3);
        assert_eq!(wide.cells[0].area.width, 12);

        let narrow = grid(Rect::new(0, 1, 40, 30), &idle(4), 0);
        assert_eq!(narrow.shape.cols, 1);
        assert_eq!(
            narrow.cells[0].area.width,
            40 - RISER_WIDTH,
            "the single column keeps its riser"
        );
        assert_eq!(narrow.cells[0].riser_x, 0);
        assert_eq!(narrow.cells[0].area.x, RISER_WIDTH);
        assert_eq!(narrow.cells[1].area.x, narrow.cells[0].area.x);
        assert_eq!(
            narrow.cells[1].area.y,
            narrow.cells[0].area.y + STAGE_HEIGHT + ROW_GAP
        );
    }

    /// Every card has its own one-column riser to its left, and no two cards
    /// share one within a row.
    #[test]
    fn every_card_has_its_own_riser() {
        let grid = grid(area(), &idle(7), 0);
        for cell in &grid.cells {
            assert_eq!(
                cell.riser_x + RISER_WIDTH,
                cell.area.x,
                "the riser is the column immediately left of the card"
            );
        }
        let first_row: Vec<u16> = grid.cells.iter().take(3).map(|c| c.riser_x).collect();
        let unique: std::collections::BTreeSet<u16> = first_row.iter().copied().collect();
        assert_eq!(unique.len(), 3, "one riser per column");
    }

    /// G9: the Jev strip and bus are drawn while a band still fits beneath them,
    /// and dropped — bus first, then strip — before that band.
    #[test]
    fn the_pipeline_bands_yield_before_a_card_row() {
        let sections = idle(4);
        // The first row carries the `idle` heading, so it needs
        // `1 + STAGE_HEIGHT` = 7 rows below the title. With the strip and bus
        // that is 10 panel rows.
        let with_both = shape(Rect::new(0, 1, 64, 10), &sections);
        assert_eq!(with_both.strip.height, 1);
        assert_eq!(with_both.bus.height, 1);
        assert_eq!(with_both.visible_rows, 1);

        // One row shorter: the bus would cost the card row, so it goes; the strip
        // stays.
        let busless = shape(Rect::new(0, 1, 64, 9), &sections);
        assert_eq!(busless.strip.height, 1);
        assert_eq!(busless.bus.height, 0);
        assert_eq!(busless.visible_rows, 1);

        // One shorter again: the strip would cost the row too, so it goes.
        let bare = shape(Rect::new(0, 1, 64, 8), &sections);
        assert_eq!(bare.strip.height, 0);
        assert_eq!(bare.bus.height, 0);
        assert_eq!(bare.visible_rows, 1);

        // Seven rows: not even one complete card (with its heading) fits.
        let empty = shape(Rect::new(0, 1, 64, 7), &sections);
        assert_eq!(empty.visible_rows, 0);
        assert_eq!(
            empty.max_offset, 0,
            "nothing on screen means nowhere to scroll to"
        );
        assert!(grid(Rect::new(0, 1, 64, 7), &sections, 0).cells.is_empty());
    }

    /// The bus is never drawn without the strip above it: it is the wire every
    /// riser hangs from, and a bare bus under the title reads as a stray line.
    #[test]
    fn the_bus_never_outlives_the_strip() {
        let sections = idle(4);
        for height in 1..30u16 {
            let shape = shape(Rect::new(0, 1, 64, height), &sections);
            if shape.bus.height > 0 {
                assert_eq!(
                    shape.strip.height, 1,
                    "bus at height {height} with no strip"
                );
                assert_eq!(
                    shape.bus.y,
                    shape.strip.bottom(),
                    "the bus sits directly under the strip"
                );
            }
        }
    }

    /// A zero-width panel draws no cards, so it must not offer scroll either: at
    /// 40×24 the team panel collapses and the old geometry still reported ten
    /// rows of scroll over an empty viewport.
    #[test]
    fn a_zero_width_panel_has_no_rows_and_no_scroll() {
        let empty = shape(Rect::new(0, 1, 0, 23), &idle(12));
        assert_eq!(empty.cell_width, 0);
        assert_eq!(empty.visible_rows, 0, "nothing is drawn, so nothing fits");
        assert_eq!(empty.max_offset, 0, "an empty viewport cannot scroll");

        let grid = grid(Rect::new(0, 1, 0, 23), &idle(12), 5);
        assert!(grid.cells.is_empty());
        assert_eq!(grid.offset, 0, "the offset clamps back to the top");
    }

    /// A one-row panel keeps the title and nothing else; a zero-row panel draws
    /// nothing at all.
    #[test]
    fn grid_degrades_to_nothing_without_height() {
        let title_only = grid(Rect::new(0, 1, 64, 1), &idle(4), 0);
        assert_eq!(title_only.title().height, 1);
        assert_eq!(title_only.strip().height, 0);
        assert_eq!(title_only.bus().height, 0);
        assert!(title_only.cells.is_empty());

        let nothing = grid(Rect::new(0, 1, 64, 0), &idle(4), 0);
        assert_eq!(nothing.title().height, 0);
        assert!(nothing.cells.is_empty());
    }

    /// An empty roster has no bands to scroll and no cards to draw.
    #[test]
    fn grid_of_an_empty_roster_is_empty() {
        let grid = grid(area(), &Sections::default(), 3);
        assert!(grid.cells.is_empty());
        assert_eq!(grid.shape.max_offset, 0);
        assert_eq!(grid.offset, 0);
    }

    /// Every card is tall enough for all [`CONTENT_LINES`] content rows, which is
    /// what keeps the cards level.
    #[test]
    fn stage_boxes_fit_their_content_rows() {
        assert_eq!(STAGE_HEIGHT, CONTENT_LINES + 2);

        let grid = grid(area(), &split(2, 5), 0);
        for cell in &grid.cells {
            assert_eq!(cell.area.height, STAGE_HEIGHT, "{:?}", cell.area);
        }
    }

    /// The reveal rule scrolls by the minimum: an already-visible card does not
    /// move the viewport, one past the bottom scrolls just far enough.
    #[test]
    fn reveal_scrolls_the_minimum_distance() {
        let area = Rect::new(0, 1, 64, 16);
        let sections = idle(12);
        let max = shape(area, &sections).max_offset;

        assert_eq!(reveal(area, &sections, 0, 0), 0, "already visible");
        assert_eq!(
            reveal(area, &sections, 2, 0),
            0,
            "last cell of the first row"
        );
        assert_eq!(
            reveal(area, &sections, 3, 0),
            0,
            "the second row is visible too"
        );
        assert_eq!(
            reveal(area, &sections, 9, 0),
            max,
            "clamped to the last page"
        );
        assert_eq!(
            reveal(area, &sections, 9, max),
            max,
            "already visible near the end"
        );

        // A card above the window scrolls the window up to its band.
        let down = reveal(area, &sections, 9, 0);
        assert_eq!(reveal(area, &sections, 0, down), 0, "back to the top");
    }

    /// The reveal rule counts the section headings: a card in the idle section
    /// scrolls to the band it is actually drawn in, not to its flat row index.
    #[test]
    fn reveal_accounts_for_the_section_headings() {
        let area = Rect::new(0, 1, 64, 24);
        // One active card, then five idle ones: bands are `working`, card,
        // `idle`, cards…, so the idle cards sit one band lower than a flat grid
        // would put them.
        let sections = split(1, 5);
        let shape = shape(area, &sections);
        assert_eq!(shape.total_card_rows, 3, "one active row + two idle rows");

        // The first idle employee (flat position 1) is in the band *after* the
        // `working` card and the `idle` heading, so it is not visible in a
        // one-band window that starts at the top.
        let tall = Rect::new(0, 1, 64, 9); // title + one band
        assert_eq!(
            reveal(tall, &sections, 0, 0),
            0,
            "the active card is on top"
        );
        assert!(
            reveal(tall, &sections, 1, 0) > 0,
            "the idle card scrolls past the headings"
        );
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

    /// The strip carries Jev — the shared answerer, which is not a card.
    #[test]
    fn the_strip_shows_jev() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
        let mut app = App::new(Config::default(), tx);

        let off = node_text(|frame| {
            let area = frame.area();
            render_strip(frame, &app, area);
        });
        assert!(off.contains("jev"), "Jev's row: {off}");
        assert!(off.contains("off"), "{off}");
        assert!(!off.contains("@jev"), "no delegation target while off");

        app.set_jev(Some(
            JevClient::new("key", "https://example.test", "jev-1-13-free").unwrap(),
        ));
        let on = node_text(|frame| {
            let area = frame.area();
            render_strip(frame, &app, area);
        });
        assert!(on.contains("on — @jev"), "{on}");
        assert!(on.contains("jev-1-13-free"), "{on}");
    }

    /// A heading names its section and its count, and degrades to just the name
    /// on a narrow panel.
    #[test]
    fn a_heading_names_its_section_and_count() {
        let wide = node_text(|frame| {
            let area = frame.area();
            render_heading(frame, Section::Active, area, 3);
        });
        assert!(wide.contains("working"), "{wide}");
        assert!(wide.contains("3"), "{wide}");
        assert!(wide.contains('◆'), "{wide}");

        let idle = node_text(|frame| {
            let area = frame.area();
            render_heading(frame, Section::Idle, area, 5);
        });
        assert!(idle.contains("idle"), "{idle}");
        assert!(idle.contains('◇'), "{idle}");

        // A narrow panel cannot fit `◇ idle · 5`, so the count is what goes.
        let mut terminal = Terminal::new(TestBackend::new(8, 1)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_heading(frame, Section::Idle, area, 5);
            })
            .unwrap();
        let narrow: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(narrow.contains("idle"), "{narrow}");
    }

    /// The bus is a row of dashes, one per column of the panel.
    #[test]
    fn the_bus_spans_the_panel_in_dashes() {
        // A panel tall enough to keep the bus alongside the section heading.
        let panel = Rect::new(0, 1, 40, 40);
        let laid_out = grid(panel, &idle(4), 0);
        assert!(laid_out.bus().height > 0, "the fixture draws the bus");

        let mut terminal = Terminal::new(TestBackend::new(40, 40)).unwrap();
        terminal
            .draw(|frame| {
                let g = grid(panel, &idle(4), 0);
                render_bus(frame, &g);
            })
            .unwrap();
        let bus_y = laid_out.bus().y as usize;
        let row: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .skip(bus_y * 40)
            .take(40)
            .map(|cell| cell.symbol())
            .collect();
        assert_eq!(row, "┄".repeat(40), "{row}");
    }
}
