//! The delegation wires: the dashed bus, the per-column risers, and the packets
//! that travel them.
//!
//! Employees are peers, so a hand-off runs from one card's riser up to the bus,
//! along the bus, and down the destination's riser — and `path` is a pure
//! function of the grid and the two cards, so the renderer and the tests agree on
//! where a wire is. The animation is the only moving part: a trace's packet walks
//! the path by its `progress`, and the cells it has already crossed are lit in the
//! trace's colour.
//!
//! Direction is the whole story:
//!
//! ```text
//!   ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄      bus
//!      ┆          ┆          ┆
//!  ┌───┴──┐   ┌───┴──┐   ┌───┴──┐       risers → card tops
//!  │ card │   │ card │   │ card │
//! ```

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, DelegationTrace};

use super::pipeline::Grid;
use super::theme;

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

/// The route between two cards: up from the source's riser to the bus, along the
/// bus to the destination's riser, then down to the destination's top border.
///
/// Pure: same grid and cards, same path.
pub fn path(grid: &Grid, from: Rect, from_riser_x: u16, to: Rect, to_riser_x: u16) -> Path {
    let bus = grid.bus();
    let mut cells = Vec::new();

    if bus.height == 0 {
        // No bus (a very short panel): a hand-off between cards in the same
        // column is just the riser, and anything else has no wire to draw.
        if from_riser_x == to_riser_x {
            for y in from.y..=to.y {
                cells.push(Position::new(from_riser_x, y));
            }
        }
        return Path { cells };
    }

    let bus_y = bus.y;

    // Up from the source's card top to the bus.
    for y in (bus_y..=from.y).rev() {
        cells.push(Position::new(from_riser_x, y));
    }

    // Along the bus to the destination's riser.
    let mut x = from_riser_x;
    while x != to_riser_x {
        x = if x < to_riser_x { x + 1 } else { x - 1 };
        cells.push(Position::new(x, bus_y));
    }

    // Down the riser to the destination's top border.
    for y in bus_y + 1..=to.y {
        cells.push(Position::new(to_riser_x, y));
    }

    cells.dedup();
    Path { cells }
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
        let (Some(from), Some(to)) = (card_for(grid, trace.from), card_for(grid, trace.to)) else {
            continue;
        };
        let route = path(grid, from.area, from.riser_x, to.area, to.riser_x);
        if route.cells.is_empty() {
            continue;
        }
        draw_trace(frame, grid, &route, trace, frame_no);
    }
}

/// The static risers: one dim dashed column in the gutter left of each visible
/// card column, running from the bus down past the cards, so the wiring is
/// legible even when nothing is moving.
///
/// The riser is the *gutter*, not the gap between rows: it runs alongside the
/// cards it feeds, which is what makes a column of three cards read as hanging
/// off one wire. A section heading in its path is skipped — the wire resumes
/// below it, so the heading stays readable.
fn render_risers(frame: &mut Frame, grid: &Grid) {
    let bus = grid.bus();
    if bus.height == 0 {
        return;
    }
    let style = Style::default().fg(theme::MUTED);
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
            let at = Position::new(cell.riser_x, y);
            if grid.headings.iter().any(|(_, rect)| rect.contains(at)) {
                continue; // never draw a wire through a section heading
            }
            put(frame, at, RISER, style);
        }
    }
}

/// The card for a roster index, when it is on screen.
fn card_for(grid: &Grid, employee: usize) -> Option<&super::pipeline::Node> {
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

    // The lit wire: one cell at a time, clipped to the panel and kept off the
    // title and section headings, which must stay readable.
    for cell in route.cells.iter().take(travelled) {
        if grid.shape.title.contains(*cell)
            || grid.headings.iter().any(|(_, rect)| rect.contains(*cell))
        {
            continue;
        }
        put(frame, *cell, "┄", Style::default().fg(colour));
    }

    // The head.
    if let Some(head) = route.head(progress)
        && !grid.shape.title.contains(head)
        && !grid.headings.iter().any(|(_, rect)| rect.contains(head))
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
pub fn trace_color(kind: crate::app::TraceKind) -> ratatui::style::Color {
    kind.color()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn grid_at(width: u16, height: u16, count: usize) -> Grid {
        let sections = super::super::pipeline::Sections {
            active: Vec::new(),
            idle: (0..count).collect(),
        };
        super::super::pipeline::grid(Rect::new(0, 1, width, height), &sections, 0)
    }

    /// The route is contiguous: every cell is one step from the previous one, so
    /// the packet never jumps a gap in the wire.
    #[test]
    fn the_path_is_contiguous() {
        let grid = grid_at(64, 24, 6);
        for (index, from) in grid.cells.iter().enumerate() {
            for to in &grid.cells[index + 1..] {
                let route = path(&grid, from.area, from.riser_x, to.area, to.riser_x);
                assert!(route.cells.len() > 1, "two cards share a wire");
                for pair in route.cells.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let step = a.x.abs_diff(b.x) + a.y.abs_diff(b.y);
                    assert_eq!(step, 1, "{a:?} → {b:?} is not one step");
                }
            }
        }
    }

    /// The route starts at the source card's top and ends at the destination's —
    /// the two ends the user watches.
    #[test]
    fn the_path_runs_from_card_to_card() {
        let grid = grid_at(64, 24, 3);
        let (from, to) = (&grid.cells[0], &grid.cells[2]);
        let route = path(&grid, from.area, from.riser_x, to.area, to.riser_x);

        let first = route.cells.first().expect("a wire has cells");
        assert_eq!(first.y, from.area.y, "the wire starts on the source card");
        assert_eq!(first.x, from.riser_x, "at its own riser");

        let last = route.cells.last().expect("a wire has cells");
        assert_eq!(last.y, to.area.y, "and ends on the destination's top");
        assert_eq!(last.x, to.riser_x, "at its own riser");
    }

    /// No wire cell lands on the strip's row: a packet there would overwrite Jev's
    /// line.
    #[test]
    fn no_wire_cell_covers_the_strip() {
        let grid = grid_at(64, 24, 6);
        let strip = grid.strip();
        assert!(strip.height > 0, "the fixture draws the strip");
        for (index, from) in grid.cells.iter().enumerate() {
            for to in &grid.cells[index + 1..] {
                let route = path(&grid, from.area, from.riser_x, to.area, to.riser_x);
                for point in &route.cells {
                    assert!(
                        !strip.contains(*point),
                        "{point:?} is on the strip row {strip:?}"
                    );
                }
            }
        }
    }

    /// Every wire crosses the bus — it is the one row all risers hang from.
    #[test]
    fn every_path_crosses_the_bus() {
        let grid = grid_at(64, 24, 6);
        let bus_y = grid.bus().y;
        for (index, from) in grid.cells.iter().enumerate() {
            for to in &grid.cells[index + 1..] {
                let route = path(&grid, from.area, from.riser_x, to.area, to.riser_x);
                let on_bus = route.cells.iter().filter(|p| p.y == bus_y).count();
                assert!(on_bus >= 1, "the wire misses the bus: {:?}", route.cells);
            }
        }
    }

    /// The head advances monotonically and never leaves the path.
    #[test]
    fn the_packet_walks_the_path() {
        let grid = grid_at(64, 24, 3);
        let (from, to) = (&grid.cells[0], &grid.cells[1]);
        let route = path(&grid, from.area, from.riser_x, to.area, to.riser_x);

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

    /// A hand-off and its answer travel opposite directions over the same wire:
    /// the report path is the hand-off path reversed.
    #[test]
    fn a_reply_travels_the_same_wire_backwards() {
        let grid = grid_at(64, 24, 3);
        let (a, b) = (&grid.cells[0], &grid.cells[2]);
        let forward = path(&grid, a.area, a.riser_x, b.area, b.riser_x);
        let backward = path(&grid, b.area, b.riser_x, a.area, a.riser_x);

        assert_eq!(backward.cells.first(), forward.cells.last());
        assert_eq!(backward.cells.last(), forward.cells.first());
        assert_eq!(backward.cells.len(), forward.cells.len());
    }

    /// The three kinds have three distinct colours: cyan out, green report,
    /// yellow broadcast.
    #[test]
    fn trace_kinds_have_distinct_colours() {
        use crate::app::TraceKind;
        assert_eq!(trace_color(TraceKind::Out), theme::ACCENT);
        assert_eq!(trace_color(TraceKind::Report), theme::OK);
        assert_eq!(trace_color(TraceKind::Fanout), theme::WARN);
    }

    /// A path with no cells has no head, rather than panicking on the empty case.
    #[test]
    fn an_empty_path_has_no_head() {
        let empty = Path { cells: Vec::new() };
        assert_eq!(empty.head(0.5), None);
        assert_eq!(empty.travelled(0.5), 0);
    }
}
