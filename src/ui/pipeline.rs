//! The team pipeline: a top-down tree diagram — the planner on top, **Jev**
//! (the question answerer) in the middle, and the remaining employees
//! (researcher, writer, reviewer, …) fanned out along the bottom. One box per
//! employee with a live status and spinner. Jev is a shared, non-employee node
//! every employee delegates questions to via `@jev`.
//!
//! Stages are ordered by routing role (see [`Employee::workflow_role`] →
//! [`Role::order`](crate::role::Role::order)): the first stage (the planner) is
//! the tree's root and the rest form the leaf row.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::team::Employee;
use crate::ui::{status, truncate};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let order = app.workflow_order();
    let employees = app.team().employees();

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

    // Root = the planner (first stage); leaves = everyone else.
    let top = order[0];
    let bottom = &order[1..];
    render_tree(frame, app, area, top, bottom, employees);
}

/// Number of content rows inside a stage box (plus two border rows).
const CONTENT_LINES: u16 = 3;
const STAGE_HEIGHT: u16 = CONTENT_LINES + 2;

/// Maximum width of a single node box (the root and Jev node are centred to
/// this, while leaf nodes share the full row).
const MAX_NODE_WIDTH: u16 = 42;

/// One drawn node: the employee it stands for (the shared Jev node stands for
/// none) and the rectangle it occupies.
pub(crate) struct Node {
    pub(crate) employee: Option<usize>,
    pub(crate) area: Rect,
}

/// The pipeline's geometry: the nodes in draw order (root, Jev, then the leaves)
/// and the connector rows between them.
pub(crate) struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) arrows: Vec<Rect>,
}

/// Lay the tree out in `area`: the root on top, the shared Jev node below it, and
/// the leaves fanned out along the bottom.
///
/// The renderer draws from this and mouse hit-testing reads it, so a click lands
/// on the node it points at.
pub(crate) fn tree(area: Rect, root: usize, leaves: &[usize]) -> Tree {
    let node_width = (area.width / (leaves.len().max(1) as u16)).clamp(14, MAX_NODE_WIDTH);

    // Rows (top → bottom): [fill, planner, arrow, jev, arrow, leaves, fill].
    let mut constraints = vec![
        Constraint::Fill(1),
        Constraint::Length(STAGE_HEIGHT), // planner
        Constraint::Length(1),            // arrow
        Constraint::Length(STAGE_HEIGHT), // jev
    ];
    if !leaves.is_empty() {
        constraints.push(Constraint::Length(1)); // arrow
        constraints.push(Constraint::Length(STAGE_HEIGHT)); // leaves
    }
    constraints.push(Constraint::Fill(1));

    let rows = Layout::vertical(constraints).split(area);
    let mut nodes = vec![
        Node {
            employee: Some(root),
            area: centered(rows[1], node_width),
        },
        Node {
            employee: None,
            area: centered(rows[3], node_width),
        },
    ];
    let mut arrows = vec![rows[2]];

    if !leaves.is_empty() {
        arrows.push(rows[4]);
        let leaf_areas = Layout::horizontal(vec![Constraint::Fill(1); leaves.len()]).split(rows[5]);
        for (index, &employee) in leaves.iter().enumerate() {
            nodes.push(Node {
                employee: Some(employee),
                area: leaf_areas[index],
            });
        }
    }

    Tree { nodes, arrows }
}

fn render_tree(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    top: usize,
    bottom: &[usize],
    employees: &[Employee],
) {
    let tree = tree(area, top, bottom);

    for arrow in &tree.arrows {
        render_arrow_down(frame, *arrow);
    }
    for node in &tree.nodes {
        match node.employee {
            Some(index) => render_stage(
                frame,
                app,
                &employees[index],
                node.area,
                index == app.selected(),
            ),
            None => render_jev_stage(frame, app, node.area),
        }
    }
}

/// Centre `area` to `width` columns (leaving it untouched when narrower).
fn centered(area: Rect, width: u16) -> Rect {
    if area.width <= width {
        return area;
    }
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(area);
    center
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
    ];

    render_node(frame, area, title, border, lines);
}

/// The Jev (System One) answerer node — a shared, non-employee stage every
/// employee delegates questions to via `@jev`. Styled distinctly from employees.
fn render_jev_stage(frame: &mut Frame, app: &App, area: Rect) {
    let enabled = app.jev_enabled();
    let accent = if enabled {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let width = area.width.saturating_sub(2) as usize;

    let title = Span::styled(
        " jev ",
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    );

    let lines = vec![
        node_head(Span::styled("◆", accent), "jev", width),
        Line::from(Span::styled("answers questions", Color::DarkGray)),
        Line::from(Span::styled(
            if enabled { "on — @jev" } else { "off" },
            if enabled {
                Color::Green
            } else {
                Color::DarkGray
            },
        )),
    ];

    render_node(frame, area, title, Style::default().fg(accent), lines);
}

/// Render one pipeline node: a bordered box with a coloured title tab and
/// left-aligned content. An employee stage and the shared Jev node are the same
/// widget — they differ only in title, accent, and content — so the node's
/// geometry and styling live here.
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

/// A node's head row: the status glyph, a gap, then the node's name in bold,
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

/// A downward connector (planner → jev → leaves).
fn render_arrow_down(frame: &mut Frame, area: Rect) {
    let arrow = Paragraph::new(Line::from(Span::styled(
        "▼",
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);

    frame.render_widget(arrow, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(4, 1, 80, 30)
    }

    /// The nodes the renderer draws are the nodes hit-testing searches: one per
    /// employee plus the shared Jev node, all inside the area and none overlapping.
    #[test]
    fn tree_nodes_are_disjoint_and_inside_the_area() {
        let tree = tree(area(), 3, &[0, 1, 2]);

        let employees: Vec<Option<usize>> = tree.nodes.iter().map(|n| n.employee).collect();
        assert_eq!(
            employees,
            vec![Some(3), None, Some(0), Some(1), Some(2)],
            "root, the shared Jev node, then the leaves in order"
        );
        assert_eq!(
            tree.arrows.len(),
            2,
            "one connector per step below the root"
        );

        for node in &tree.nodes {
            assert!(node.area.width > 0 && node.area.height > 0);
            assert!(node.area.right() <= area().right(), "{:?}", node.area);
            assert!(node.area.bottom() <= area().bottom(), "{:?}", node.area);
        }
        for (index, node) in tree.nodes.iter().enumerate() {
            for other in &tree.nodes[index + 1..] {
                assert!(
                    !node.area.intersects(other.area),
                    "{:?} ∩ {:?}",
                    node.area,
                    other.area
                );
            }
        }
        for arrow in &tree.arrows {
            for node in &tree.nodes {
                assert!(
                    !arrow.intersects(node.area),
                    "an arrow must not cover a node"
                );
            }
        }
    }

    /// Root and Jev are centred on the panel and capped in width; the leaves share
    /// the bottom row edge to edge.
    #[test]
    fn tree_centres_the_root_and_tiles_the_leaves() {
        let tree = tree(area(), 3, &[0, 1, 2]);
        let (root, jev) = (&tree.nodes[0], &tree.nodes[1]);
        let leaves = &tree.nodes[2..];

        assert!(root.area.width <= MAX_NODE_WIDTH && jev.area.width <= MAX_NODE_WIDTH);
        assert_eq!(root.area.x, area().x + (area().width - root.area.width) / 2);
        assert_eq!(jev.area.width, root.area.width);
        assert_eq!(leaves[0].area.x, area().x);
        assert_eq!(leaves[2].area.right(), area().right());
        for pair in leaves.windows(2) {
            assert_eq!(pair[0].area.right(), pair[1].area.x, "leaves are adjacent");
        }
        assert_eq!(leaves[0].area.y, leaves[2].area.y, "leaves share a row");
    }

    /// A team of one has no leaf row: just the root and the connected Jev node.
    #[test]
    fn tree_without_leaves_is_root_and_jev_only() {
        let tree = tree(area(), 0, &[]);

        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.arrows.len(), 1);
        assert_eq!(tree.nodes[0].employee, Some(0));
        assert_eq!(tree.nodes[1].employee, None);
        assert!(!tree.nodes[0].area.intersects(tree.nodes[1].area));
    }
}
