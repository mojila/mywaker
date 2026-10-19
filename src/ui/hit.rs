//! Mouse hit-testing: what a screen coordinate points at.
//!
//! Every answer comes from the geometry in [`super::layout`] — the same
//! rectangles the renderer drew into — so a click can only land on something
//! that is on screen (and on the row it actually points at).

use ratatui::layout::{Position, Rect};

use crate::app::{App, Mode};

use super::{layout, pipeline};

/// What a point on screen points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An employee's card in the roster grid (an index into the team).
    Employee(usize),
    /// A row of the project picker (an index into [`App::projects`]).
    Project(usize),
    /// A row of the model picker (an index into [`App::filtered_models`]).
    Model(usize),
}

/// Resolve a point against the layout of the frame drawn for a terminal of
/// `size` cells.
pub fn target_at(app: &App, size: (u16, u16), x: u16, y: u16) -> Option<Target> {
    let main = layout::Regions::of(Rect::new(0, 0, size.0, size.1)).main;
    let point = Position::from((x, y));
    if !main.contains(point) {
        return None;
    }

    match app.mode() {
        Mode::Roster => employee_at(app, layout::roster_areas(main)[1], point),
        Mode::Projects => {
            let [_, list] = layout::project_areas(main);
            row_at(
                list,
                app.projects().len(),
                app.project_selected(),
                point.y,
                Target::Project,
            )
        }
        Mode::Models => {
            let [_, _, list] = layout::model_areas(main);
            row_at(
                list,
                app.filtered_models().len(),
                app.model_selected(),
                point.y,
                Target::Model,
            )
        }
        Mode::Chat => None,
    }
}

/// The employee whose grid card covers the point. The grid's own geometry
/// decides — with the app's scroll offset — so a click agrees with the card it
/// was aimed at. The Jev header and the gaps between cards stand for no employee
/// and answer `None`.
fn employee_at(app: &App, area: Rect, point: Position) -> Option<Target> {
    let order = app.workflow_order();
    pipeline::grid(area, &order, app.roster_scroll())
        .cells
        .into_iter()
        .find(|cell| cell.area.contains(point))
        .map(|cell| Target::Employee(cell.employee))
}

/// The list row under `y`, for a list scrolled to the window its selection asks
/// for.
fn row_at(
    list: Rect,
    count: usize,
    selected: usize,
    y: u16,
    target: fn(usize) -> Target,
) -> Option<Target> {
    let offset = layout::list_window(list, count, selected);
    layout::list_row(list, count, offset, y).map(target)
}
