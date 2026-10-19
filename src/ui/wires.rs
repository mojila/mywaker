//! The delegation wires: the strip's port, the dashed bus, the per-column risers,
//! and the packets that travel them.
//!
//! The path is fixed geometry (one route per card column), and `path` is a pure
//! function of the grid and a card's position, so the renderer and the tests
//! agree on where a wire is. The animation is the only moving part: a trace's
//! packet walks the path by its `progress`, and the cells it has already crossed
//! are lit in the trace's colour, so a hand-off reads as a signal running down the
//! tree and a report reads as one running back up.
//!
//! Direction is the whole story:
//!
//! ```text
//!            ◆ root                        strip
//!   ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄      bus
//!            ┆          ┆          ┆
//!        ┌───┴──┐   ┌───┴──┐   ┌───┴──┐       risers → card tops
//!        │ card │   │ card │   │ card │
//! ```

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, DelegationTrace, TraceKind};

use super::pipeline::Grid;

/// One cell of the dashed bus. `┄` is the horizontal dashed line, which reads as
/// a wire rather than as a border.
pub(crate) const BUS: &str = "┄";

/// The vertical riser, dashed to match the bus.
pub(crate) const RISER: &str = "┆";

/// The packet head: the glyph at the front of a travelling trace.
pub(crate) const PACKET: &str = "◆";

/// Where a wire runs, as the cells it passes through, in travel order.
///
/// `to_card` is the card's rectangle; the path ends on its top border, which is
/// where the work visibly arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub cells: Vec<Position>,
}

impl Path {
    /// The cell the packet's head is on at `progress` (`0.0..=1.0`), or the last
    /// cell once it has arrived.
    pub fn head(&self, progress: f32) -> Option<Position> {
        if self.cells.is_empty() {
            return None;
        }
        let last = self.cells.len() - 1;
        let index = ((progress.clamp(0.0, 1.0) * last as f32).round() as usize).min(last);
        self.cells.get(index).copied()
    }

    /// How many cells the packet has already crossed at `progress` — the part of
    /// the wire drawn lit behind the head.
    pub fn travelled(&self, progress: f32) -> usize {
        let Some(head) = self.head(progress) else {
            return 0;
        };
        self.cells
            .iter()
            .position(|cell| *cell == head)
            .map(|index| index + 1)
            .unwrap_or(0)
    }
}

/// The route from the strip to a card: out of the strip's port onto the bus, along
/// the bus to the card's riser, then down the riser to the card's top border.
///
/// Pure: same grid and card, same path. A report travels it reversed
/// ([`reverse`]), so both directions share one geometry.
///
/// The path starts *on the bus*, not on the strip row: the port is where the wire
/// emerges from under the strip, and drawing a packet on the strip row itself
/// would overwrite the root's name.
pub fn path(grid: &Grid, card: Rect, riser_x: u16) -> Path {
    let bus = grid.bus();
    let strip = grid.strip();
    let mut cells = Vec::new();

    if bus.height == 0 {
        // No bus (a very short panel): the wire is just the riser, so a packet
        // still visibly reaches the card.
        for y in bus.bottom()..card.y {
            cells.push(Position::new(riser_x, y));
        }
        cells.push(Position::new(riser_x, card.y));
        return Path { cells };
    }

    // The port: the root's column, where the wire leaves the strip.
    let port_x = strip.x.saturating_add(2);
    let bus_y = bus.y;

    // Along the bus to the card's riser column, walking from the port toward the
    // riser so every step is one cell.
    let mut x = port_x;
    cells.push(Position::new(x, bus_y));
    while x != riser_x {
        x = if x < riser_x { x + 1 } else { x - 1 };
        cells.push(Position::new(x, bus_y));
    }

    // Down the riser to the card's top border.
    for y in bus_y + 1..card.y {
        cells.push(Position::new(riser_x, y));
    }

    // The card's top border is where the work arrives.
    cells.push(Position::new(riser_x, card.y));
    cells.dedup();

    Path { cells }
}

/// A path travelled the other way — a report climbing back to the root.
pub fn reverse(path: &Path) -> Path {
    Path {
        cells: path.cells.iter().rev().copied().collect(),
    }
}

/// Draw the wires: the static risers for every visible card column, then each
/// live trace's lit wire and packet.
///
/// A trace whose card is scrolled out of the viewport is skipped: a wire to a
/// card that is not on screen has nothing to point at.
pub fn render(frame: &mut Frame, app: &App, grid: &Grid) {
    render_risers(frame, grid);

    let frame_no = app.frame();
    for trace in app.traces() {
        let Some(card) = card_for(grid, trace) else {
            continue;
        };
        let route = path(grid, card.area, card.riser_x);
        // Work going out runs strip → card; a report runs card → strip. A
        // fan-out is outbound too — it is the root broadcasting, not answering.
        let route = match trace.kind {
            TraceKind::Out | TraceKind::Fanout => route,
            TraceKind::Report => reverse(&route),
        };
        draw_trace(frame, grid, &route, trace, frame_no);
    }
}

/// The static risers: one dim dashed column in the gutter left of each visible
/// card column, running from the bus down past the cards, so the tree's shape is
/// legible even when nothing is moving.
///
/// The riser is the *gutter*, not the gap between rows: it runs alongside the
/// cards it feeds, which is what makes a column of three cards read as hanging
/// off one wire.
fn render_risers(frame: &mut Frame, grid: &Grid) {
    let bus = grid.bus();
    if bus.height == 0 {
        return;
    }
    let style = Style::default().fg(Color::DarkGray);
    let mut drawn: Vec<u16> = Vec::new();
    for cell in &grid.cells {
        // One riser per column, not one per card: a column of three cards hangs
        // off the same wire.
        if drawn.contains(&cell.riser_x) {
            continue;
        }
        drawn.push(cell.riser_x);
        // From the bus to the last card in this column, so the wire visibly
        // reaches every card it feeds.
        let bottom = grid
            .cells
            .iter()
            .filter(|other| other.riser_x == cell.riser_x)
            .map(|other| other.area.bottom())
            .max()
            .unwrap_or(cell.area.bottom());
        for y in bus.bottom()..bottom {
            put(frame, Position::new(cell.riser_x, y), RISER, style);
        }
    }
}

/// The card a trace points at — the *destination* for an outgoing hand-off and
/// the *origin* for a report.
fn card_for<'a>(grid: &'a Grid, trace: &DelegationTrace) -> Option<&'a super::pipeline::Node> {
    let employee = match trace.kind {
        TraceKind::Report => trace.from,
        TraceKind::Out | TraceKind::Fanout => trace.to,
    };
    grid.cells.iter().find(|cell| cell.employee == employee)
}

/// Paint one trace: the wire behind the head lit in the trace colour, the head as
/// a packet.
fn draw_trace(
    frame: &mut Frame,
    grid: &Grid,
    route: &Path,
    trace: &DelegationTrace,
    frame_no: u64,
) {
    let progress = trace.progress(frame_no);
    let colour = trace.kind.color();
    let travelled = route.travelled(progress);

    // The lit wire: one cell at a time, clipped to the panel.
    for cell in route.cells.iter().take(travelled) {
        if grid.shape.title.contains(*cell) {
            continue;
        }
        put(frame, *cell, "┄", Style::default().fg(colour));
    }

    // The head.
    if let Some(head) = route.head(progress)
        && !grid.shape.title.contains(head)
    {
        put(
            frame,
            head,
            PACKET,
            Style::default()
                .fg(colour)
                .add_modifier(ratatui::style::Modifier::BOLD),
        );
    }
}

/// Write one symbol at an absolute cell, if the buffer covers it.
fn put(frame: &mut Frame, at: Position, symbol: &str, style: Style) {
    let area = frame.area();
    if at.x >= area.right() || at.y >= area.bottom() {
        return;
    }
    let cell = Rect::new(at.x, at.y, 1, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(symbol.to_string(), style))),
        cell,
    );
}

/// The colour a trace is drawn in — exposed so tests can assert the mapping
/// without reaching into the renderer.
#[cfg(test)]
pub fn trace_color(kind: TraceKind) -> Color {
    kind.color()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn grid_at(width: u16, height: u16, count: usize) -> Grid {
        let order: Vec<usize> = (0..count).collect();
        super::super::pipeline::grid(Rect::new(0, 1, width, height), &order, 0)
    }

    /// The route is contiguous: every cell is one step from the previous one, so
    /// the packet never jumps a gap in the wire.
    #[test]
    fn the_path_is_contiguous() {
        let grid = grid_at(64, 24, 6);
        for cell in &grid.cells {
            let route = path(&grid, cell.area, cell.riser_x);
            assert!(route.cells.len() > 1, "a card has a wire");
            for pair in route.cells.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let step = a.x.abs_diff(b.x) + a.y.abs_diff(b.y);
                assert_eq!(step, 1, "{a:?} → {b:?} is not one step");
            }
        }
    }

    /// The route starts on the bus at the root's port and ends on the card's top
    /// border — the two ends the user watches.
    #[test]
    fn the_path_runs_from_the_strip_to_the_card_top() {
        let grid = grid_at(64, 24, 3);
        let card = &grid.cells[0];
        let route = path(&grid, card.area, card.riser_x);

        let first = route.cells.first().expect("a wire has cells");
        assert_eq!(first.y, grid.bus().y, "the wire starts on the bus");
        assert_eq!(
            first.x,
            grid.strip().x + 2,
            "at the root's port, under the strip"
        );

        let last = route.cells.last().expect("a wire has cells");
        assert_eq!(last.y, card.area.y, "and ends on the card's top border");
        assert_eq!(last.x, card.riser_x, "at the card's own riser");
    }

    /// No wire cell lands on the strip's row: a packet there would overwrite the
    /// root's name.
    #[test]
    fn no_wire_cell_covers_the_strip() {
        let grid = grid_at(64, 24, 6);
        let strip = grid.strip();
        assert!(strip.height > 0, "the fixture draws the strip");
        for cell in &grid.cells {
            let route = path(&grid, cell.area, cell.riser_x);
            for point in &route.cells {
                assert!(
                    !strip.contains(*point),
                    "{point:?} is on the strip row {strip:?}"
                );
            }
        }
    }

    /// Every wire crosses the bus exactly once — it is the one row all risers
    /// hang from.
    #[test]
    fn every_path_crosses_the_bus() {
        let grid = grid_at(64, 24, 6);
        let bus_y = grid.bus().y;
        for cell in &grid.cells {
            let route = path(&grid, cell.area, cell.riser_x);
            let on_bus = route.cells.iter().filter(|p| p.y == bus_y).count();
            assert!(on_bus >= 1, "the wire misses the bus: {:?}", route.cells);
        }
    }

    /// The head advances monotonically and never leaves the path.
    #[test]
    fn the_packet_walks_the_path() {
        let grid = grid_at(64, 24, 3);
        let card = &grid.cells[1];
        let route = path(&grid, card.area, card.riser_x);

        let mut previous = 0;
        for step in 0..=10 {
            let progress = step as f32 / 10.0;
            let travelled = route.travelled(progress);
            assert!(travelled >= previous, "the packet went backwards");
            previous = travelled;
            assert!(route.head(progress).is_some());
        }
        assert_eq!(
            route.travelled(1.0),
            route.cells.len(),
            "a finished packet has crossed the whole wire"
        );
    }

    /// Reversing is the same wire travelled the other way, cell for cell.
    #[test]
    fn a_report_travels_the_same_wire_backwards() {
        let grid = grid_at(64, 24, 3);
        let card = &grid.cells[0];
        let forward = path(&grid, card.area, card.riser_x);
        let backward = reverse(&forward);

        assert_eq!(backward.cells.first(), forward.cells.last());
        assert_eq!(backward.cells.last(), forward.cells.first());
        assert_eq!(backward.cells.len(), forward.cells.len());
    }

    /// The three kinds have three distinct colours: cyan out, green report,
    /// yellow broadcast.
    #[test]
    fn trace_kinds_have_distinct_colours() {
        assert_eq!(trace_color(TraceKind::Out), Color::Cyan);
        assert_eq!(trace_color(TraceKind::Report), Color::Green);
        assert_eq!(trace_color(TraceKind::Fanout), Color::Yellow);
    }

    /// A path with no cells has no head, rather than panicking on the empty case.
    #[test]
    fn an_empty_path_has_no_head() {
        let empty = Path { cells: Vec::new() };
        assert_eq!(empty.head(0.5), None);
        assert_eq!(empty.travelled(0.5), 0);
    }
}
