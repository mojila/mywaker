//! Screen geometry: the rectangles a frame is drawn into, computed in one place
//! and read by both the renderer and mouse hit-testing — so a click can only land
//! on something that was actually drawn.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::Mode;

/// A frame's stacked bands: the title bar, the active screen (`main`), the log,
/// and the hint bar.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Regions {
    pub title: Rect,
    pub main: Rect,
    /// The roster's prompt band: the orchestrator bar on the main screen. Zero
    /// height on every other screen, so `main` keeps the height it had before
    /// the bar existed.
    pub prompt: Rect,
    pub log: Rect,
    pub help: Rect,
}

/// The prompt band's height on the roster screen: two rows, one per line of the
/// bordered bar (who you are talking to, then what you are typing).
pub(crate) const PROMPT_HEIGHT: u16 = 2;

impl Regions {
    /// Split the whole terminal `area` into its bands for `mode`.
    ///
    /// The log is the band that gives way: the roster spends two of its rows on
    /// the prompt bar (and two fewer on the log), so `main` — the panels the user
    /// reads — is exactly as tall as it was before the bar existed. Every other
    /// screen keeps a six-row log and no prompt band.
    pub(crate) fn of(area: Rect, mode: Mode) -> Self {
        let (prompt_height, log_height) = match mode {
            Mode::Roster => (PROMPT_HEIGHT, 4),
            Mode::Chat | Mode::Projects | Mode::Models => (0, 6),
        };

        let [title, main, prompt, log, help] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(prompt_height),
            Constraint::Length(log_height),
            Constraint::Length(1),
        ])
        .areas(area);

        Self {
            title,
            main,
            prompt,
            log,
            help,
        }
    }
}

/// The roster's three panels, left to right: the sidebar, the team grid, and the
/// tasks board.
pub(crate) fn roster_areas(main: Rect) -> [Rect; 3] {
    Layout::horizontal([
        Constraint::Length(24),
        Constraint::Fill(1),
        Constraint::Length(32),
    ])
    .areas(main)
}

/// The chat panel: the transcript above the input bar.
pub(crate) fn chat_areas(main: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(main)
}

/// The project picker: its header above the project list.
pub(crate) fn project_areas(main: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(main)
}

/// The model picker: the header, the search line, then the model list.
pub(crate) fn model_areas(main: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(main)
}

/// The first item a bordered, one-row-per-item list shows, given its item count
/// and the selection it has to keep visible.
///
/// The pickers set their `ListState` offset from this and mouse hit-testing reads
/// it back, so a click lands on the row under the pointer rather than on a row
/// the list scrolled to by itself.
pub(crate) fn list_window(list: Rect, count: usize, selected: usize) -> usize {
    let rows = list.height.saturating_sub(2) as usize; // drop the borders
    if rows == 0 || count == 0 {
        return 0;
    }
    selected.min(count - 1).saturating_sub(rows - 1)
}

/// The item index drawn on row `y` of a list scrolled to `offset`, or `None` when
/// `y` falls on a border or past the last item.
pub(crate) fn list_row(list: Rect, count: usize, offset: usize, y: u16) -> Option<usize> {
    let rows = list.height.saturating_sub(2) as usize;
    if rows == 0 || y < list.y.saturating_add(1) {
        return None;
    }
    let row = (y - list.y - 1) as usize;
    let index = offset.checked_add(row)?;
    (row < rows && index < count).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_at(height: u16) -> Rect {
        Rect::new(0, 10, 40, height)
    }

    /// The property the pickers rely on: whatever we hand `ListState`, the
    /// selection is inside the window, so ratatui draws it as we asked instead of
    /// scrolling to its own offset.
    #[test]
    fn list_window_keeps_the_selection_visible() {
        for count in 1..12usize {
            for height in 3..8u16 {
                let rows = height.saturating_sub(2) as usize;
                for selected in 0..count {
                    let offset = list_window(list_at(height), count, selected);
                    assert!(
                        selected >= offset && selected < offset + rows,
                        "count {count} rows {rows} selected {selected} offset {offset}"
                    );
                }
            }
        }
    }

    #[test]
    fn list_window_clamps_to_the_last_item() {
        // A selection past the end of a shortened list still yields a sane window;
        // a list that fits shows from the top.
        assert_eq!(list_window(list_at(5), 100, 99), 97);
        assert_eq!(list_window(list_at(5), 3, 99), 0);
        assert_eq!(list_window(list_at(5), 0, 0), 0);
        // Too short to show any row: no scrolling to be had.
        assert_eq!(list_window(list_at(2), 10, 9), 0);
    }

    /// A row maps back to the item drawn on it: the inverse of the window, which
    /// is what makes a click land on the highlighted row.
    #[test]
    fn list_row_maps_each_row_to_its_item() {
        let list = list_at(6); // 4 content rows, y = 10, 11..14, border 15
        let (count, selected) = (10usize, 8usize);
        let offset = list_window(list, count, selected);

        for y in 11..=14u16 {
            let index = list_row(list, count, offset, y).expect("a row inside the list");
            assert_eq!(y, 11 + (index - offset) as u16);
        }
        assert_eq!(list_row(list, count, offset, 14), Some(selected));

        // Borders and anything past the list are not rows.
        assert_eq!(list_row(list, count, offset, 10), None);
        assert_eq!(list_row(list, count, offset, 15), None);
        assert_eq!(list_row(list, count, offset, 40), None);
        // Past the last item.
        assert_eq!(list_row(list, 2, 0, 13), None);
    }

    #[test]
    fn roster_panels_tile_the_main_area() {
        let main = Rect::new(0, 1, 120, 30);
        let [sidebar, grid, tasks] = roster_areas(main);

        assert_eq!(sidebar.x, main.x);
        assert_eq!(sidebar.width, 24);
        assert_eq!(grid.x, sidebar.right());
        assert_eq!(tasks.right(), main.right());
        assert_eq!(grid.width + 24 + 32, main.width);
    }

    /// The roster spends two rows on the prompt bar, taken from the log — never
    /// from `main`, whose height is what the grid, sidebar and tasks panel were
    /// sized against.
    #[test]
    fn regions_stack_the_frame() {
        let regions = Regions::of(Rect::new(0, 0, 100, 40), Mode::Chat);

        assert_eq!(regions.title, Rect::new(0, 0, 100, 1));
        assert_eq!(regions.log, Rect::new(0, 33, 100, 6));
        assert_eq!(regions.help, Rect::new(0, 39, 100, 1));
        assert_eq!(regions.main, Rect::new(0, 1, 100, 32));
        assert_eq!(regions.prompt, Rect::new(0, 33, 100, 0));
    }

    /// In roster mode the prompt band exists and the log gives up exactly its
    /// height, so `main` is byte-for-byte the same rectangle as on any other
    /// screen.
    #[test]
    fn the_roster_prompt_band_comes_out_of_the_log() {
        let area = Rect::new(0, 0, 100, 40);
        let roster = Regions::of(area, Mode::Roster);
        let chat = Regions::of(area, Mode::Chat);

        assert_eq!(roster.prompt.height, PROMPT_HEIGHT);
        assert_eq!(roster.prompt, Rect::new(0, 33, 100, PROMPT_HEIGHT));
        assert_eq!(roster.log, Rect::new(0, 35, 100, 4));
        assert_eq!(
            roster.main, chat.main,
            "the prompt band must not shrink the panels"
        );
        assert_eq!(roster.help, chat.help);

        // Every band tiles the frame with no gap and no overlap.
        let bands = [
            roster.title,
            roster.main,
            roster.prompt,
            roster.log,
            roster.help,
        ];
        let mut y = area.y;
        for band in bands {
            assert_eq!(band.y, y, "{band:?}");
            y += band.height;
        }
        assert_eq!(y, area.bottom());
    }

    /// A frame too short for every band still tiles: ratatui shrinks the bands
    /// rather than overlapping them, and the hint bar keeps its row at the
    /// bottom. The prompt band is never taller than it asked for.
    #[test]
    fn the_prompt_band_degrades_on_a_short_frame() {
        let area = Rect::new(0, 0, 40, 3);
        let regions = Regions::of(area, Mode::Roster);

        assert!(regions.prompt.height <= PROMPT_HEIGHT);
        assert_eq!(regions.help.bottom(), area.bottom(), "the hint bar stays");

        // Whatever the split, the bands are disjoint and in order.
        let bands = [
            regions.title,
            regions.main,
            regions.prompt,
            regions.log,
            regions.help,
        ];
        for (index, band) in bands.iter().enumerate() {
            for other in &bands[index + 1..] {
                assert!(!band.intersects(*other), "{band:?} ∩ {other:?}");
            }
        }
    }
}
