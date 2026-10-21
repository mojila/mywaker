//! The ratatui renderer: title bar, sidebar, employee grid, prompt, chat, log,
//! help.

mod chat;
pub(crate) mod hit;
pub(crate) mod input;
pub(crate) mod layout;
mod modal;
mod models;
pub(crate) mod pipeline;
mod projects;
mod prompt;
mod robot;
pub mod sidebar;
mod status;
pub use status::thinking_line;
mod tasks;
pub(crate) mod theme;
mod wires;

pub use hit::Target;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
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
    let regions = layout::Regions::of(frame.area(), app.mode(), app.prompt_employee_is_thinking());

    render_title(frame, app, regions.title);
    render_main(frame, app, regions.main);
    prompt::render(frame, app, regions.prompt);
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
///
/// The band heights here are the *resting* ones (no thinking indicator): the
/// panels are the same height whether or not an employee is mid-turn, which is
/// the invariant the acceptance matrix pins.
pub fn sidebar_area(size: (u16, u16)) -> Rect {
    let main = layout::Regions::of(Rect::new(0, 0, size.0, size.1), Mode::Roster, false).main;
    layout::roster_areas(main)[0]
}

/// The team panel's rectangle for a terminal of `size` cells.
///
/// Public for the same reason as [`sidebar_area`]: the grid acceptance tests read
/// the panel the renderer draws into instead of recomputing the `24 | fill | 32`
/// split.
pub fn team_area(size: (u16, u16)) -> Rect {
    let main = layout::Regions::of(Rect::new(0, 0, size.0, size.1), Mode::Roster, false).main;
    layout::roster_areas(main)[1]
}

/// The roster prompt bar's rectangle for a terminal of `size` cells — the band
/// between `main` and the log, where a goal is typed.
pub fn prompt_area(size: (u16, u16)) -> Rect {
    layout::Regions::of(Rect::new(0, 0, size.0, size.1), Mode::Roster, false).prompt
}

/// The employee cards on screen for a terminal of `size` cells, as
/// `(employee index, cell rect)` in draw order.
///
/// This reads the same `pipeline::grid` geometry the renderer draws from, with
/// the app's current scroll offset, so an acceptance test can click the middle of
/// a card and know which employee it should select. The order is the app's two
/// sections — the active employees first, then the idle ones. Jev is not an
/// employee and never appears here.
pub fn grid_cells(app: &App, size: (u16, u16)) -> Vec<(usize, Rect)> {
    let sections = app.card_sections();
    pipeline::grid(team_area(size), &sections, app.roster_scroll())
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
/// cells and a given roster split.
///
/// Crate-internal: [`App::roster_max_scroll`] is the seam the tests use, and it
/// exists so the clamp and the drawing share one number.
pub(crate) fn roster_max_scroll(size: (u16, u16), sections: &pipeline::Sections) -> usize {
    pipeline::shape(team_area(size), sections).max_offset
}

/// How many whole bands fit on screen — the `PgUp`/`PgDn` step.
pub(crate) fn roster_page(size: (u16, u16), sections: &pipeline::Sections) -> usize {
    pipeline::shape(team_area(size), sections).visible_rows
}

/// Columns in the roster grid at this terminal size: three normally, one when
/// the panel is too narrow for readable cards.
pub(crate) fn roster_cols(size: (u16, u16)) -> usize {
    pipeline::shape(team_area(size), &pipeline::Sections::default()).cols
}

/// The offset that reveals the card at `position` (in section order), scrolling
/// no further than necessary.
pub(crate) fn reveal_roster(
    size: (u16, u16),
    sections: &pipeline::Sections,
    position: usize,
    offset: usize,
) -> usize {
    pipeline::reveal(team_area(size), sections, position, offset)
}

fn render_title(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Min(24), Constraint::Length(32)]).areas(area);

    let title = Line::from({
        let mut spans = vec![
            Span::styled(
                " mywaker ",
                Style::default()
                    .bg(theme::ACCENT)
                    .fg(theme::ON_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " a team of AI employees ",
                Style::default().add_modifier(Modifier::DIM),
            ),
        ];
        if let Some(project) = app.active_project() {
            spans.push(Span::styled(format!(" · {}", project.name), theme::TEXT));
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
            Span::styled("✕ ", theme::ERR),
            Span::styled(err.to_string(), theme::ERR),
        ]);
    }
    if let Some(version) = app.server_version() {
        return Line::from(vec![
            Span::styled("● ", theme::OK),
            Span::raw(format!("opencode v{version}")),
            Span::styled(format!(" · {} agents", app.agent_count()), theme::TEXT),
        ]);
    }
    Line::from(vec![
        Span::styled(spinner_at(app.frame()).to_string(), theme::WARN),
        Span::styled(" connecting…".to_string(), theme::TEXT),
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
    // The band's content rows (the two border rows are not content).
    let content_rows = area.height.saturating_sub(2) as usize;
    let mut text: Vec<Line> = Vec::with_capacity(content_rows.max(2));

    // The latest error leads the panel: a failed turn must never be invisible
    // because the roster's log has room for little else, so it is surfaced here
    // rather than buried in the dim progress lines.
    //
    // K7 (`docs/keymap.md` §3.8): the glyph and the class word carry the error
    // colour, but the **prose is body text**. Drawing the whole line in `Red`
    // measured 2.85:1 against the terminal's background; `err` is reserved for
    // the marker and the word, which is what a reader scans for.
    let latest_error = app.log().iter().rev().find(|line| line.error).cloned();
    if let Some(error) = &latest_error {
        text.push(Line::from(vec![
            Span::styled(
                "✕ ",
                Style::default().fg(theme::ERR).add_modifier(Modifier::BOLD),
            ),
            Span::styled("error ", Style::default().fg(theme::ERR)),
            Span::styled(error.text.clone(), Style::default().fg(theme::TEXT)),
        ]));
    }

    // Telemetry follows; it is dropped off the bottom on a short band once the
    // error and the recent messages have filled it.
    text.push(stream_stats(app));
    text.push(telemetry_stats(app));

    // Then the remaining recent log lines, newest first, errors in red. The
    // first error this sweep meets is the latest one — already drawn at the top —
    // so it is skipped rather than drawn twice.
    let mut budget = content_rows.saturating_sub(text.len());
    let mut skipped_error = false;
    for line in app.log().iter().rev() {
        if budget == 0 {
            break;
        }
        if !skipped_error && line.error {
            skipped_error = true;
            continue;
        }
        let style = if line.error {
            Style::default().fg(theme::ERR).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::MUTED)
        };
        text.push(Line::from(Span::styled(line.text.clone(), style)));
        budget -= 1;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::BORDER)
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
        Span::styled(" usage ", theme::ACCENT),
        Span::styled(
            format!(
                "{}/{} in/out",
                fmt_count(usage.input_tokens),
                fmt_count(usage.output_tokens)
            ),
            theme::TEXT,
        ),
        Span::raw(" · "),
        Span::styled(
            format!("cache {hit:.0}% ({})", fmt_count(usage.cache_read_tokens)),
            theme::TEXT,
        ),
        Span::raw(" · "),
        Span::styled(format!("ram {ram}"), theme::TEXT),
        Span::raw(" · "),
        Span::styled(format!("cpu {:.1}%", metrics.cpu_percent()), theme::TEXT),
    ])
}

fn stream_stats(app: &App) -> Line<'static> {
    let mut spans: Vec<Span> = vec![
        Span::styled(" stream ", theme::ACCENT),
        Span::styled(format!("{} agents", app.agent_count()), theme::TEXT),
        Span::raw(" · "),
        Span::styled(format!("{} events", app.event_count()), theme::TEXT),
    ];
    if let Some(event) = app.last_event() {
        spans.push(Span::raw(" · "));
        spans.push(Span::styled(format!("last: {event}"), theme::TEXT));
    }
    Line::from(spans)
}

/// The bottom hint bar: at most five key pairs for the focused region, with
/// `? help` reserved first (`docs/help-palette.md` §4.1–§4.2, `docs/keymap.md`
/// §3.2).
///
/// Two rules make this decidable rather than taste:
///
/// 1. **Keys only.** `wheel`/`click` are pointer hints, not `KeyCode`s, so they
///    live in a separate list (`docs/keymap.md` §3.9) and are admitted only after
///    every key hint — they never displace `? help`.
/// 2. **Whole pairs drop, never a clipped pair** (U3b). The bar is composed by
///    measuring, so a narrow terminal loses pairs from the bottom of the priority
///    order instead of cutting one mid-description — today's failure, where the
///    terminal cut `Ctrl-Y copy` and then `m`.
fn hint_bar(app: &App, width: usize) -> Line<'static> {
    // `? help` is reserved out of the budget before anything else is admitted,
    // so U3(c) cannot fail by arithmetic (§3.2's reading of the two rules).
    const HELP: (&str, &str) = ("?", "help");
    let help_width = pair_width(HELP);

    // Priority order: the roster's frozen five-slot list (§3.2) leads, and each
    // mode's own hints follow in the order a user needs them.
    let hints: &[(&str, &str)] = match app.mode() {
        Mode::Roster if app.prompt_focused() => &[
            ("Enter", "send"),
            ("↑/↓", "recall"),
            ("Esc", "back"),
            ("Ctrl-F", "focus"),
        ],
        Mode::Roster => &[
            ("↑↓←→", "move"),
            ("i", "goal"),
            ("Enter", "chat"),
            ("f", "focus"),
            ("Space", "pause"),
        ],
        Mode::Models => &[
            ("type", "search"),
            ("↑/k", "up"),
            ("↓/j", "down"),
            ("Enter", "assign"),
            ("Esc", "back"),
        ],
        Mode::Projects => &[
            ("↑/k", "up"),
            ("↓/j", "down"),
            ("Enter", "select"),
            ("Esc", "back"),
        ],
        Mode::Chat => &[
            ("Enter", "send"),
            ("↑/↓", "history"),
            ("PgUp/PgDn", "scroll"),
            ("Esc", "back"),
            ("Ctrl-C", "stop"),
        ],
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = help_width;

    // Admit whole pairs in priority order until the next would overflow. The cap
    // of five slots is inclusive of the reserved help slot, so at most four
    // others are drawn.
    for (key, description) in hints.iter().take(4) {
        let width_needed = pair_width((key, description));
        if used + width_needed > width {
            break;
        }
        used += width_needed;
        spans.push(format!(" {key} ").bold().fg(theme::ACCENT));
        spans.push(format!("{description} ").fg(theme::MUTED));
    }

    // Pointer hints come last, and only if they fit after every admitted key
    // hint — never at the expense of `? help` (§3.9).
    for (key, description) in pointer_hints(app) {
        let width_needed = pair_width((key, description));
        if used + width_needed > width {
            break;
        }
        used += width_needed;
        spans.push(format!(" {key} ").fg(theme::MUTED));
        spans.push(format!("{description} ").fg(theme::MUTED));
    }

    spans.push(format!(" {} ", HELP.0).bold().fg(theme::ACCENT));
    spans.push(format!("{} ", HELP.1).fg(theme::MUTED));

    Line::from(spans)
}

/// The display width of one ` key ` + `description ` pair.
fn pair_width((key, description): (&str, &str)) -> usize {
    1 + key.chars().count() + 1 + description.chars().count() + 1
}

/// Pointer hints for the focused region — not keys, so they are a separate kind
/// (`docs/keymap.md` §3.9).
fn pointer_hints(app: &App) -> &'static [(&'static str, &'static str)] {
    match app.mode() {
        Mode::Roster if !app.prompt_focused() => &[("wheel", "scroll"), ("click", "select")],
        Mode::Chat => &[("wheel", "scroll")],
        _ => &[],
    }
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(hint_bar(app, area.width as usize)), area);
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
                .fg(theme::ACCENT)
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
                .fg(theme::WARN)
                .add_modifier(Modifier::BOLD),
        ))
    };

    vec![
        head(" global"),
        // K3: `q` quits from anywhere, but `Esc` does not — it restores focus
        // mode, dismisses an overlay, or closes chat first (§3.6's precedence).
        // The old single row `q · Esc — quit (any screen)` was false.
        key("q", "quit (any screen)"),
        key("Esc", "back · restore"),
        // K3: `?` is roster-only help; in chat the discovery path is `/help`,
        // and in a text field `?` types a `?`.
        key("?", "help (roster · chat via /help)"),
        key("Ctrl-P", "command palette"),
        Line::from(""),
        head(" roster"),
        key("↑↓←→ h/j/k/l", "move around the employee grid"),
        key("PgUp/PgDn", "move a page of cards"),
        key("i · Tab", "type a goal for the selected employee"),
        key("Enter", "chat with the selected employee"),
        key("m · M", "pick a model"),
        key("p · P", "choose the working project"),
        key("Space", "pause / resume the task engine"),
        key("s", "pause / resume the selected employee's task"),
        key("f", "focus mode (Ctrl-F in a text field)"),
        Line::from(""),
        head(" pointer"),
        // §3.9: pointer hints are their own kind, so they get their own section
        // rather than sitting among the keys.
        key("click", "select"),
        key("wheel", "scroll (the selection stays put)"),
        Line::from(""),
        head(" goal prompt"),
        key("Enter", "send the goal · Shift+Enter newline"),
        key("↑/↓", "recall a previous goal"),
        key("Esc · Tab", "back to the roster keys"),
        Line::from(""),
        head(" chat"),
        key("Enter", "send · Shift+Enter newline"),
        key("↑/↓", "command history · PgUp/PgDn scroll"),
        key("Esc", "back · Ctrl-C stop · /help commands"),
        Line::from(""),
        head(" session picker (on chat open)"),
        key("↑/↓", "choose: start fresh or resume a previous session"),
        key("Enter", "open the highlighted choice"),
        key("n", "skip the list — start fresh now"),
        key("Esc", "leave the chat"),
        Line::from(""),
        head(" model picker"),
        key("type", "filter by id / name / provider"),
        key("Enter", "assign · Ctrl-U clear · Esc back"),
    ]
}

/// A full-screen help overlay, centred over the current view.
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let popup = centered(area, 68, 30);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::ACCENT)
        .bg(theme::ON_ACCENT)
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
                    .fg(theme::WARN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  shutting down…",
                Style::default()
                    .fg(theme::WARN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "flushing state…",
            Style::default().fg(theme::TEXT),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::BORDER)
        .bg(theme::ON_ACCENT)
        .title(" mywaker ".bold().cyan());

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::message::Message;

    /// A disconnected app: enough to render the hint bar and the help overlay,
    /// which read mode, focus and the team — not the service.
    fn app() -> App {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(16);
        App::new(Config::default(), tx)
    }

    /// The pair's width is `1 + key + 1 + description + 1` (`help-palette.md`
    /// §4.2). Pinned because the hint bar's budget arithmetic depends on it.
    #[test]
    fn pair_width_counts_its_padding() {
        assert_eq!(pair_width(("i", "goal")), 8);
        assert_eq!(pair_width(("↑↓←→", "move")), 11);
        assert_eq!(pair_width(("Ctrl-Y", "copy log")), 17);
    }

    /// K4/U3: the bar advertises at most five pairs **including** the reserved
    /// `? help`, and the roster's four lead pairs are the frozen ones from
    /// `docs/keymap.md` §3.2 — 48 columns total, so nothing is clipped at 60×16.
    #[test]
    fn hint_bar_is_five_pairs_and_never_clips() {
        let app = app();
        let line = hint_bar(&app, 60);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.chars().count() <= 60,
            "bar is {} columns: {text}",
            text.chars().count()
        );
        assert!(
            text.ends_with("? help "),
            "the escape hatch is last: {text:?}"
        );
        for expected in ["↑↓←→ move", "i goal", "Enter chat", "f focus"] {
            assert!(text.contains(expected), "missing {expected:?} in {text:?}");
        }
    }

    /// U3(b): a pair is never cut mid-description. At a width too narrow for
    /// every pair, whole pairs drop and `? help` survives.
    #[test]
    fn hint_bar_drops_whole_pairs_and_keeps_help() {
        let app = app();
        let line = hint_bar(&app, 30);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.chars().count() <= 30,
            "bar is {} columns: {text}",
            text.chars().count()
        );
        assert!(text.ends_with("? help "), "help survives: {text:?}");
        // Every drawn description is whole: `focus` never appears as `focu…`.
        for partial in ["focu ", "focu…", "cha ", "ch…"] {
            assert!(!text.contains(partial), "clipped pair in {text:?}");
        }
    }

    /// K3: the help overlay no longer claims `Esc` quits from any screen, and it
    /// says `?` is roster-only.
    #[test]
    fn help_copy_scopes_esc_and_question_mark() {
        let lines: Vec<String> = help_lines()
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let body = lines.join("\n");
        // The false row was the *combined* one; `q` alone may still say
        // "quit (any screen)", which is true.
        assert!(!body.contains("q · Esc"), "K3: combined quit row: {body}");
        assert!(body.contains("back · restore"), "K3: {body}");
        assert!(
            body.contains("help (roster · chat via /help)"),
            "K3: {body}"
        );
    }

    /// K3/§3.9: pointer hints are their own section, not mixed into the keys.
    #[test]
    fn help_separates_pointer_hints() {
        let lines: Vec<String> = help_lines()
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let body = lines.join("\n");
        assert!(body.contains(" pointer"), "no pointer section: {body}");
        assert!(
            !body.contains("click · wheel"),
            "pointer still mixed in: {body}"
        );
    }
}
