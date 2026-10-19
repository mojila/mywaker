//! The ratatui renderer: title bar, sidebar, employee grid, chat, log, help.

mod chat;
pub(crate) mod hit;
mod layout;
mod modal;
mod models;
mod pipeline;
mod projects;
mod robot;
pub mod sidebar;
mod status;
mod tasks;

pub use hit::Target;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{App, Mode};
use crate::usage::{fmt_bytes, fmt_count};

/// The connecting/shutdown indicator spins at half the roster cadence, so it
/// indexes the shared frames directly rather than via [`status::spinner`].
fn spinner_at(frame: u64) -> &'static str {
    status::SPINNER[(frame as usize / 2) % status::SPINNER.len()]
}

/// Truncate `text` to `width` display columns, appending an ellipsis when
/// clipped. Shared by the panels that render fixed-width rows.
pub(crate) fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn render(frame: &mut Frame, app: &App) {
    let regions = layout::Regions::of(frame.area());

    render_title(frame, app, regions.title);
    render_main(frame, app, regions.main);
    render_log(frame, app, regions.log);
    render_help(frame, app, regions.help);

    if app.is_shutting_down() {
        render_shutdown(frame, app, frame.area());
        return;
    }

    // The reopen prompt sits above the help overlay: answering it is the first
    // thing to do, and it is modal until it is answered.
    if app.resume_prompt().is_some() {
        modal::render(frame, app, frame.area());
        return;
    }

    if app.show_help() {
        render_help_overlay(frame, frame.area());
    }
}

/// The sidebar's rectangle for a terminal of `size` cells.
///
/// Public so the sidebar acceptance tests (`tests/sidebar.rs`) read the *same*
/// geometry the renderer draws into, rather than recomputing the `24 | fill | 32`
/// split by hand — which is what `docs/acceptance-sidebar.md` §3 requires.
pub fn sidebar_area(size: (u16, u16)) -> Rect {
    let main = layout::Regions::of(Rect::new(0, 0, size.0, size.1)).main;
    layout::roster_areas(main)[0]
}

/// The team panel's rectangle for a terminal of `size` cells.
///
/// Public for the same reason as [`sidebar_area`]: the grid acceptance tests read
/// the panel the renderer draws into instead of recomputing the `24 | fill | 32`
/// split.
pub fn team_area(size: (u16, u16)) -> Rect {
    let main = layout::Regions::of(Rect::new(0, 0, size.0, size.1)).main;
    layout::roster_areas(main)[1]
}

/// The employee cards on screen for a terminal of `size` cells, as
/// `(employee index, cell rect)` in draw order.
///
/// This reads the same `pipeline::grid` geometry the renderer draws from, with
/// the app's current scroll offset, so an acceptance test can click the middle of
/// a card and know which employee it should select. The Jev header is not a card
/// and never appears here.
pub fn grid_cells(app: &App, size: (u16, u16)) -> Vec<(usize, Rect)> {
    let order = app.workflow_order();
    pipeline::grid(team_area(size), &order, app.roster_scroll())
        .cells
        .into_iter()
        .map(|cell| (cell.employee, cell.area))
        .collect()
}

/// What a point on screen points at, for the acceptance tests' hit-target
/// assertions. Re-exported so callers don't need the private `hit` module.
pub fn hit_target_at(app: &App, size: (u16, u16), x: u16, y: u16) -> Option<Target> {
    hit::target_at(app, size, x, y)
}

/// The largest scroll offset the roster grid accepts for a terminal of `size`
/// cells and a roster of `count` employees.
///
/// Crate-internal: [`App::roster_max_scroll`] is the seam the tests use, and it
/// exists so the clamp and the drawing share one number.
pub(crate) fn roster_max_scroll(size: (u16, u16), count: usize) -> usize {
    pipeline::shape(team_area(size), count).max_offset
}

/// How many whole card rows fit on screen — the `PgUp`/`PgDn` step.
pub(crate) fn roster_page(size: (u16, u16), count: usize) -> usize {
    pipeline::shape(team_area(size), count).visible_rows
}

/// Columns in the roster grid at this terminal size: three normally, one when
/// the panel is too narrow for readable cards.
pub(crate) fn roster_cols(size: (u16, u16)) -> usize {
    pipeline::shape(team_area(size), 0).cols
}

/// The offset that reveals the card at workflow `position`, scrolling no further
/// than necessary.
pub(crate) fn reveal_roster(
    size: (u16, u16),
    count: usize,
    position: usize,
    offset: usize,
) -> usize {
    pipeline::reveal(team_area(size), count, position, offset)
}

fn render_title(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Min(24), Constraint::Length(32)]).areas(area);

    let title = Line::from({
        let mut spans = vec![
            Span::styled(
                " mywaker ",
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " a team of AI employees ",
                Style::default().add_modifier(Modifier::DIM),
            ),
        ];
        if let Some(project) = app.active_project() {
            spans.push(Span::styled(format!(" · {}", project.name), Color::Gray));
        }
        spans
    });
    frame.render_widget(Paragraph::new(title), left);

    frame.render_widget(
        Paragraph::new(connection_line(app)).alignment(Alignment::Right),
        right,
    );
}

fn connection_line(app: &App) -> Line<'static> {
    if let Some(err) = app.connection_error() {
        return Line::from(vec![
            Span::styled("✕ ", Color::Red),
            Span::styled(err.to_string(), Color::Red),
        ]);
    }
    if let Some(version) = app.server_version() {
        return Line::from(vec![
            Span::styled("● ", Color::Green),
            Span::raw(format!("opencode v{version}")),
            Span::styled(format!(" · {} agents", app.agent_count()), Color::Gray),
        ]);
    }
    Line::from(vec![
        Span::styled(spinner_at(app.frame()).to_string(), Color::Yellow),
        Span::styled(" connecting…".to_string(), Color::Gray),
    ])
}

fn render_main(frame: &mut Frame, app: &App, area: Rect) {
    match app.mode() {
        Mode::Chat => chat::render(frame, app, area),
        Mode::Projects => projects::render(frame, app, area),
        Mode::Models => models::render(frame, app, area),
        Mode::Roster => {
            let [sidebar_area, team_area, tasks_area] = layout::roster_areas(area);
            sidebar::render(frame, app, sidebar_area);
            pipeline::render(frame, app, team_area);
            tasks::render(frame, app, tasks_area);
        }
    }
}

fn render_log(frame: &mut Frame, app: &App, area: Rect) {
    let mut text: Vec<Line> = vec![stream_stats(app), telemetry_stats(app)];

    for line in app.log().iter().rev().take(3) {
        text.push(Line::from(Span::styled(line.clone(), Color::DarkGray)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(" log ".bold().cyan());

    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn telemetry_stats(app: &App) -> Line<'static> {
    let usage = app.usage();
    let metrics = app.metrics();

    let ram = metrics
        .memory_bytes()
        .map(fmt_bytes)
        .unwrap_or_else(|| "—".to_string());
    let hit = usage.cache_hit_rate() * 100.0;

    Line::from(vec![
        Span::styled(" usage ", Color::Cyan),
        Span::styled(
            format!(
                "{}/{} in/out",
                fmt_count(usage.input_tokens),
                fmt_count(usage.output_tokens)
            ),
            Color::Gray,
        ),
        Span::raw(" · "),
        Span::styled(
            format!("cache {hit:.0}% ({})", fmt_count(usage.cache_read_tokens)),
            Color::Gray,
        ),
        Span::raw(" · "),
        Span::styled(format!("ram {ram}"), Color::Gray),
        Span::raw(" · "),
        Span::styled(format!("cpu {:.1}%", metrics.cpu_percent()), Color::Gray),
    ])
}

fn stream_stats(app: &App) -> Line<'static> {
    let mut spans: Vec<Span> = vec![
        Span::styled(" stream ", Color::Cyan),
        Span::styled(format!("{} agents", app.agent_count()), Color::Gray),
        Span::raw(" · "),
        Span::styled(format!("{} events", app.event_count()), Color::Gray),
    ];
    if let Some(event) = app.last_event() {
        spans.push(Span::raw(" · "));
        spans.push(Span::styled(format!("last: {event}"), Color::Gray));
    }
    Line::from(spans)
}

/// The bottom hint bar: the active screen's keys, as `(key, description)` pairs
/// in display order. The bar is data so a screen's hints are one list, not a run
/// of hand-built spans.
fn hint_bar(app: &App) -> Line<'static> {
    let hints: &[(&str, &str)] = match app.mode() {
        Mode::Roster => &[
            ("↑↓←→", "move"),
            ("wheel", "scroll"),
            ("Enter", "chat"),
            ("p", "projects"),
            ("m", "model"),
            ("Space", "pause"),
            ("s", "task"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Mode::Models => &[
            ("type", "search"),
            ("Ctrl-U", "clear"),
            ("↑", "/"),
            ("↓", "move"),
            ("Enter", "assign"),
            ("Esc", "back"),
        ],
        Mode::Projects => &[
            ("↑/k", "move"),
            ("↓/j", "move"),
            ("Enter", "select"),
            ("Esc", "back"),
        ],
        Mode::Chat => &[
            ("Enter", "send"),
            ("↑/↓", "history"),
            ("PgUp/PgDn", "scroll"),
            ("/help", "cmds"),
            ("Esc", "back"),
            ("Ctrl-C", "stop"),
        ],
    };
    hint_line(hints)
}

/// One hint bar line: every key in bold cyan with its padding, every description
/// dim and trailing a space, so the pairs run together without gaps.
fn hint_line(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::with_capacity(hints.len() * 2);
    for (key, description) in hints {
        spans.push(format!(" {key} ").bold().cyan());
        spans.push(format!("{description} ").dim());
    }
    Line::from(spans)
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(hint_bar(app)), area);
}

/// Centre a fixed-size popup within `area`, clamped to the terminal size.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    let [_, vertical_center, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, popup, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(vertical_center);
    popup
}

/// One `<key>  <description>` pair as an overlay body line: the key in bold cyan,
/// the description plain. Shared by the help overlay and the reopen prompt so
/// every overlay spells its keys the same way.
pub(crate) fn key_hint(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            key,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  {description}")),
    ])
}

/// The help overlay's body lines.
fn help_lines() -> Vec<Line<'static>> {
    let key = key_hint;
    let head = |t: &'static str| {
        Line::from(Span::styled(
            t,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    };

    vec![
        head(" global"),
        key("q · Esc", "quit (any screen)"),
        key("?", "this help overlay"),
        key("click · wheel", "select · scroll"),
        Line::from(""),
        head(" roster"),
        key("↑↓←→ h/j/k/l", "move around the employee grid"),
        key("PgUp/PgDn", "move a page of cards"),
        key("wheel", "scroll the grid (the selection stays put)"),
        key("Enter", "chat with the selected employee"),
        key("m", "pick a model"),
        key("p", "choose the working project"),
        key("Space", "pause / resume the task engine"),
        key("s", "pause / resume the selected employee's task"),
        Line::from(""),
        head(" chat"),
        key("Enter", "send · Shift+Enter newline"),
        key("↑/↓", "command history · PgUp/PgDn scroll"),
        key("Esc", "back · Ctrl-C stop · /help commands"),
        Line::from(""),
        head(" model picker"),
        key("type", "filter by id / name / provider"),
        key("Enter", "assign · Ctrl-U clear · Esc back"),
    ]
}

/// A full-screen help overlay, centred over the current view.
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let popup = centered(area, 68, 20);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::Cyan)
        .bg(Color::Black)
        .title(" help ".bold().cyan());
    frame.render_widget(Paragraph::new(help_lines()).block(block), popup);
}

/// A full-screen "shutting down…" overlay with an animated spinner, shown for a
/// few frames between quit and terminal restore.
fn render_shutdown(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let popup = centered(area, 44, 8);

    let spinner = spinner_at(app.shutdown_frame());
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                spinner.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  shutting down…",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "flushing state…",
            Style::default().fg(Color::Gray),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .bg(Color::Black)
        .title(" mywaker ".bold().cyan());

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center),
        popup,
    );
}
