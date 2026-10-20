//! The left sidebar: the `waker` avatar plus the team's vitals.
//!
//! The roster's left column used to be the robot and nothing else, while the
//! other two columns carried live data. It is now an inspector + vitals column —
//! an aggregate "now" line, the selected employee, an activity sparkline, queue
//! vitals, runtime, and alerts — stacked under a compact avatar.
//!
//! **Sections are whole units.** `fit` keeps the longest prefix of the frozen
//! priority order whose cumulative height fits, so a short terminal loses whole
//! sections from the bottom rather than drawing half of one. That rule, the
//! per-section budget, and the marker each section draws are frozen in
//! `docs/acceptance-sidebar.md` §2 — this module implements it.
//!
//! **The sidebar is not a hit target.** `ui::hit` never resolves a point inside
//! this panel, so a click here moves nothing (acceptance A8).

use std::time::Duration;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::ui::robot::{MINI_NAMEPLATE, RobotState, render_mini};
use crate::ui::status;

/// The title drawn in the panel's top border.
const TITLE: &str = " waker ";

/// The frozen section order and the marker each one draws, highest priority
/// first.
///
/// `docs/acceptance-sidebar.md` §2's budget table is derived from this list, so
/// adding or reordering a section is a spec change. A marker is a literal that
/// appears in its section's first line and nowhere else in the frame — that is
/// what makes A5 decidable from a buffer.
pub(crate) const MARKERS: [&str; 7] = [
    "MYWAKER", "now", "who", "pulse", "vitals", "uptime", "alert",
];

/// The frozen marker list, for the acceptance tests in `tests/sidebar.rs` to
/// assert A5 against. Public so the matrix cites one symbol, not a copy.
pub fn markers() -> &'static [&'static str] {
    &MARKERS
}

/// The sidebar's **static vocabulary** — every literal this module composes
/// itself, as opposed to text it merely relays from the model layer.
///
/// Acceptance A7 asserts that none of the log/title-owned tokens (`ram `, `cpu `,
/// `events`, `cache `, `opencode v`) appear in *this* list. Scoping it here is
/// deliberate: an employee's name or role, a task goal, a model id, or a
/// connection error may legitimately contain any of those tokens, so scanning
/// the rendered buffer would fail on relayed text the sidebar does not author.
pub fn labels() -> &'static [&'static str] {
    &LABELS
}

/// Every fixed string the sidebar draws. Keep in step with the section builders
/// below; a new literal belongs here so A7 keeps covering it.
const LABELS: [&str; 16] = [
    TITLE,
    MINI_NAMEPLATE,
    "now ",
    "who ",
    "pulse ",
    "vitals ",
    "uptime ",
    "alert ",
    "queue empty",
    "no finished work",
    "session live",
    "session —",
    "connection ok",
    "✓ none",
    "none",
    "jev ",
];

/// The section stack `fit` keeps for `height`, as source rows — one `Vec` per
/// drawn section, in priority order, with the blank gap row above each section
/// represented as an empty string.
///
/// This is the acceptance matrix's single seam (A3, A5, A6): the expected rows
/// are the ones `fit` actually kept, so a budget or builder change shows up as a
/// test failure instead of a stale table in the docs. Each section is present
/// whole or not at all — that is what makes A3's all-or-nothing clause decidable.
pub fn fitted_rows(app: &App, width: usize, height: usize) -> Vec<Vec<String>> {
    fit_sections(sections(app, width.max(1)), height)
        .into_iter()
        .map(|section| {
            let mut rows = Vec::with_capacity(section.sources.len() + 1);
            if section.priority > 0 {
                rows.push(String::new());
            }
            rows.extend(section.sources);
            rows
        })
        .collect()
}

/// One stacked section: a marker, the rows beneath it, and the height it needs
/// *including* the blank row above it.
#[derive(Debug, Clone)]
pub(crate) struct Section {
    /// Position in [`MARKERS`] — 0 is drawn first and dropped last.
    pub(crate) priority: usize,
    /// The section's body rows (never including the gap row), already truncated
    /// to the panel width. The first one carries [`Self::marker`].
    pub(crate) lines: Vec<Line<'static>>,
    /// The same rows **before** truncation, one string each.
    ///
    /// A6 is only decidable against these: "this row is complete, or it is an
    /// ellipsised prefix of the source" needs the source. Keeping them beside
    /// the truncated lines means the acceptance test reads what the panel
    /// composed rather than re-deriving it.
    pub(crate) sources: Vec<String>,
}

/// A row as its parts are composed: `(text, style)` pairs, untruncated.
type Row = Vec<(String, Style)>;

impl Section {
    /// A section at `priority`, composed from `rows` and truncated to `width` in
    /// **one pass per row** (A6: truncate, never wrap).
    ///
    /// The marker is [`MARKERS`]`[priority]`, so the heading and the frozen table
    /// cannot drift.
    ///
    /// The marker must appear in the section's own rows — for every section but
    /// P0 that is its first row, while P0's avatar carries `MYWAKER` on its
    /// third (acceptance spec §2.1). The marker may be *ellipsised* when the
    /// panel is narrower than the literal, which is why this is asserted at a
    /// normal width rather than here (A10 allows the narrow case).
    fn new(priority: usize, rows: Vec<Row>, width: usize) -> Self {
        let mut lines = Vec::with_capacity(rows.len());
        let mut sources = Vec::with_capacity(rows.len());
        for row in rows {
            sources.push(row.iter().map(|(text, _)| text.as_str()).collect());
            lines.push(truncate_spans(row, width));
        }
        Self {
            priority,
            lines,
            sources,
        }
    }

    /// The rows this section occupies *including* its leading gap row — except
    /// for the first section, which has no gap above it.
    fn height(&self) -> usize {
        self.lines.len() + usize::from(self.priority > 0)
    }
}

/// The sections that fit in `height` rows, as drawable lines.
///
/// Keeps the **longest prefix** of the priority order whose cumulative height
/// does not exceed `height`, and never emits a partial section. A section that
/// would overflow is dropped along with every lower-priority section — the
/// stack degrades from the bottom, which is what makes the behaviour predictable
/// instead of "whatever happened to fit".
pub(crate) fn fit(sections: Vec<Section>, height: usize) -> Vec<Line<'static>> {
    fit_with_sources(sections, height).0
}

/// The sections `fit` keeps for `height`, whole and in priority order.
///
/// Split out from [`fit_with_sources`] because the acceptance matrix needs the
/// *sections* (to assert all-or-nothing, A3) while the renderer needs only the
/// lines.
fn fit_sections(sections: Vec<Section>, height: usize) -> Vec<Section> {
    let mut ordered = sections;
    ordered.sort_by_key(|section| section.priority);

    let mut out = Vec::new();
    let mut used = 0usize;
    for section in ordered {
        let needed = section.height();
        if used + needed > height {
            break;
        }
        used += needed;
        out.push(section);
    }
    out
}

/// [`fit`], also returning each drawn row's untruncated source (A6's evidence).
///
/// The two vectors are parallel: `sources[i]` is the string `lines[i]` was
/// composed from, so "complete, or an ellipsised prefix" is checkable.
pub(crate) fn fit_with_sources(
    sections: Vec<Section>,
    height: usize,
) -> (Vec<Line<'static>>, Vec<String>) {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    for section in fit_sections(sections, height) {
        if section.priority > 0 {
            out.push(Line::from(""));
            sources.push(String::new());
        }
        out.extend(section.lines);
        sources.extend(section.sources);
    }
    (out, sources)
}

/// Format an uptime as `59s` / `5m` / `1h 12m` / `2d 3h`.
///
/// Deliberately coarse: the sidebar has one row for it, and seconds only matter
/// in the first minute.
pub(crate) fn fmt_uptime(uptime: Duration) -> String {
    let secs = uptime.as_secs();
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h {}m", mins % 60);
    }
    format!("{}d {}h", hours / 24, hours % 24)
}

/// A `width`-cell progress bar filled to `ratio` (clamped to `0.0..=1.0`).
///
/// A half-block marks a partially filled final cell, so a nearly-full bar does
/// not read as full. `width == 0` yields an empty string; every other width
/// yields exactly `width` cells.
pub(crate) fn bar(ratio: f32, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let ratio = if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    };
    let cells = ratio * width as f32;
    let full = (cells.floor() as usize).min(width);

    let mut out = String::with_capacity(width * 3);
    for _ in 0..full {
        out.push('█');
    }
    if full < width {
        if cells - full as f32 >= 0.5 {
            out.push('▌');
        } else {
            out.push('░');
        }
        for _ in full + 1..width {
            out.push('░');
        }
    }
    out
}

/// A block-glyph sparkline of `samples`, at most `width` cells.
///
/// Takes the **most recent** `width` samples when there are more, and scales to
/// the window's own maximum so a quiet period still shows structure. An all-zero
/// window draws flat low blocks rather than an empty string, so the row never
/// looks like a rendering failure.
pub(crate) fn sparkline(samples: &[u64], width: usize) -> String {
    const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return String::new();
    }
    if samples.is_empty() {
        return "▁".repeat(width);
    }

    let start = samples.len().saturating_sub(width);
    let window = &samples[start..];
    let max = window.iter().copied().max().unwrap_or(0);

    let mut out = String::with_capacity(width * 3);
    if max == 0 {
        out.push_str(&"▁".repeat(window.len()));
    } else {
        for &sample in window {
            // Scale into 0..=7 so the tallest sample reaches the full block.
            let level = ((sample as f64 / max as f64) * (GLYPHS.len() - 1) as f64).round() as usize;
            out.push(GLYPHS[level.min(GLYPHS.len() - 1)]);
        }
    }
    // Left-pad when the window is shorter than the row, so the trend hugs the
    // right edge and "recent" reads as "near the border".
    format!("{}{out}", " ".repeat(width.saturating_sub(window.len())))
}

/// Truncate a run of styled spans to `width` display columns **as one string**,
/// appending an ellipsis to the span that gets cut.
///
/// Composing first and truncating once is what keeps A6 true. A row built from
/// two independently-truncated halves can be clipped by the terminal at a point
/// that lands mid-composition — at 40×24 the provider/session row rendered
/// `ope… ·`, which is neither a complete source string nor an ellipsised prefix
/// of one. One truncation pass per row makes the invariant structural instead of
/// something each builder has to remember.
fn truncate_spans(spans: Vec<(String, Style)>, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }
    let total: usize = spans.iter().map(|(text, _)| text.chars().count()).sum();
    if total <= width {
        return Line::from(
            spans
                .into_iter()
                .map(|(text, style)| Span::styled(text, style))
                .collect::<Vec<_>>(),
        );
    }

    // Reserve the last column for the ellipsis, so the truncated row is exactly
    // `width` cells and never one over (which the terminal would clip, losing
    // the ellipsis that tells the reader the text continues).
    let room = width - 1;
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for (text, style) in spans {
        let len = text.chars().count();
        if used + len <= room {
            out.push(Span::styled(text, style));
            used += len;
            continue;
        }
        // This span is the one that gets cut.
        let mut piece: String = text.chars().take(room - used).collect();
        piece.push('…');
        out.push(Span::styled(piece, style));
        return Line::from(out);
    }

    // Unreachable for a non-empty over-wide row, but keep the total bounded.
    out.push(Span::styled("…".to_string(), Style::default()));
    Line::from(out)
}

/// Render the sidebar into `area` (a bordered panel).
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(Span::styled(
            TITLE,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    // The content area is inside the borders; `fit` works in content rows.
    let inner_width = area.width.saturating_sub(2) as usize;
    let inner_height = area.height.saturating_sub(2) as usize;
    let lines = fit(sections(app, inner_width), inner_height);

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render only the sidebar's **content** — no borders, no title — into `area`.
///
/// This is the A2 seam: a test renders the sidebar twice, with one sidebar-owned
/// input changed, and diffs the two buffers without the border or any other
/// panel in the way. It shares the section builder with [`render`], so the seam cannot
/// drift from what the real panel draws.
pub fn render_only(frame: &mut Frame, app: &App, area: Rect) {
    let lines = fit(sections(app, area.width as usize), area.height as usize);
    frame.render_widget(Paragraph::new(lines), area);
}

/// Build the section stack for the current app state.
///
/// `width` is the usable inner width; every row is truncated to it so nothing
/// wraps (acceptance A6).
fn sections(app: &App, width: usize) -> Vec<Section> {
    let width = width.max(1);
    vec![
        Section::new(0, avatar(app), width),
        Section::new(1, vec![now_row(app, width)], width),
        Section::new(2, inspector(app), width),
        Section::new(3, vec![pulse_row(app, width)], width),
        Section::new(4, vitals(app, width), width),
        Section::new(5, runtime(app, width), width),
        Section::new(6, alerts(app, width), width),
    ]
}

/// The untruncated source of every row the sidebar draws for this app state, at
/// `width` (used for the width-dependent branch: `now` only names its state
/// above 10 columns).
///
/// Public for the acceptance tests: A6 asks whether a drawn row is complete or an
/// ellipsised prefix of its source, which is undecidable from the buffer alone.
/// This returns exactly the strings the panel composed — the same `sections`
/// call the renderer makes.
pub fn source_rows(app: &App, width: usize) -> Vec<String> {
    fit_with_sources(sections(app, width.max(1)), usize::MAX).1
}

/// The aggregate mood the mini avatar reflects.
pub(crate) fn robot_state(app: &App) -> RobotState {
    if app.connection_error().is_some() {
        return RobotState::Error;
    }
    if !app.is_connected() {
        return RobotState::Thinking;
    }
    if app.team().is_busy() {
        return RobotState::Working;
    }
    RobotState::Idle
}

/// P0's avatar: the four mini rows, nameplate kept as the P0 marker.
///
/// `render_mini` already emits exactly the four rows §2.1 pins, with the
/// nameplate on row 3; the row is re-styled here so the brand reads in the panel
/// accent. Truncation happens in `Section::new`, so a narrow panel ellipsises the
/// nameplate (`MYWAK…` at 40×24) rather than clipping it mid-glyph.
fn avatar(app: &App) -> Vec<Row> {
    let mut rows: Vec<Row> = render_mini(robot_state(app), app.frame())
        .iter()
        .map(|line| {
            vec![(
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>(),
                Style::default(),
            )]
        })
        .collect();
    if let Some(nameplate) = rows.get_mut(2) {
        *nameplate = vec![(
            MINI_NAMEPLATE.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];
    }
    rows
}

/// The aggregate "now" line: what the team is doing, in one row.
///
/// Ordered by how much the user needs to know: a connection failure outranks a
/// paused engine, which outranks live work, which outranks idle.
///
/// **Narrow panels keep the marker and the glyph.** `now` (3) + a gap + the
/// longest state word (`working`, 7) needs 11 usable columns; at the 6 columns
/// a 40-column terminal leaves, the marker and the glyph are all that fit — and
/// the glyph is itself the state (`●` idle, `▶` working, `✕` offline, `⏸`
/// paused, the spinner while thinking). The marker is never dropped, because
/// A5 is a structural assertion; A4's narrow-width clause accepts the glyph.
fn now_row(app: &App, width: usize) -> Row {
    let (glyph, text, color) = if app.connection_error().is_some() {
        ("✕", "offline".to_string(), Color::Red)
    } else if !app.is_connected() {
        (
            status::spinner(app.frame()),
            "connecting".to_string(),
            Color::Yellow,
        )
    } else if app.is_paused() {
        ("⏸", "paused".to_string(), Color::Yellow)
    } else {
        let busy = app.busy_count();
        let thinking = app.thinking_count();
        if busy > 0 {
            let working = busy.saturating_sub(thinking);
            let mut parts = Vec::new();
            if working > 0 {
                parts.push(format!("{working} working"));
            }
            if thinking > 0 {
                parts.push(format!("{thinking} thinking"));
            }
            ("▶", parts.join(" · "), Color::Cyan)
        } else {
            ("●", "idle".to_string(), Color::DarkGray)
        }
    };

    // The marker is structural (A5), so it always leads the row. The state
    // word follows when there is room for it; below 10 columns only the glyph
    // fits, and the glyph *is* the state (spec A4's narrow-width clause).
    //
    // Composed then truncated once (A6): truncating the word on its own and
    // appending it to the marker could still overflow the panel, which the
    // terminal would clip mid-word.
    let mut spans = vec![
        ("now ".to_string(), Style::default().fg(Color::DarkGray)),
        (glyph.to_string(), Style::default().fg(color)),
    ];
    if width >= 10 {
        spans.push((" ".to_string(), Style::default().fg(color)));
        spans.push((text, Style::default().fg(color)));
    }
    spans
}

/// P2: the selected employee's identity, model, provider, and live session.
fn inspector(app: &App) -> Vec<Row> {
    let Some(employee) = app.team().employees().get(app.selected()) else {
        return vec![vec![(
            "who — no employee".to_string(),
            Style::default().fg(Color::DarkGray),
        )]];
    };

    let mut live = format!(
        "session {}",
        if app.live_sessions() > 0 {
            "live"
        } else {
            "—"
        }
    );
    if app.queued_prompts() > 0 {
        live.push_str(&format!(" · {} queued", app.queued_prompts()));
    }

    // What the employee is actually doing: their latched task summary, or a
    // muted placeholder. This is the row that answers "what is Bruno up to?"
    // without opening the chat.
    let doing = (
        employee
            .last_task
            .clone()
            .unwrap_or_else(|| "no active task".to_string()),
        Style::default().fg(status::text_color(employee.status)),
    );

    vec![
        vec![
            ("who ".to_string(), Style::default().fg(Color::DarkGray)),
            (
                status::icon_span(employee.status, app.frame()).to_string(),
                Style::default(),
            ),
            (" ".to_string(), Style::default()),
            (
                employee.name.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ],
        vec![(employee.role.clone(), Style::default().fg(Color::DarkGray))],
        vec![
            ("doing ".to_string(), Style::default().fg(Color::DarkGray)),
            doing,
        ],
        vec![(
            employee.model.clone().unwrap_or_else(|| "none".to_string()),
            Style::default().fg(Color::Cyan),
        )],
        vec![
            (
                employee
                    .model_provider
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
                Style::default().fg(Color::Gray),
            ),
            (" · ".to_string(), Style::default().fg(Color::DarkGray)),
            (live, Style::default().fg(Color::Gray)),
        ],
    ]
}

/// P3: a one-row activity sparkline.
fn pulse_row(app: &App, width: usize) -> Row {
    let label = "pulse ";
    let room = width.saturating_sub(label.chars().count());
    vec![
        (label.to_string(), Style::default().fg(Color::DarkGray)),
        (
            sparkline(app.pulse(), room),
            Style::default().fg(Color::Cyan),
        ),
    ]
}

/// P4: the task queue's roll-up, two rows — what is in flight, and how much has
/// finished.
fn vitals(app: &App, _width: usize) -> Vec<Row> {
    let stats = app.task_stats();

    let in_flight = if stats.active() == 0 {
        "queue empty".to_string()
    } else {
        let mut parts = Vec::new();
        if stats.running > 0 {
            parts.push(format!("{} running", stats.running));
        }
        if stats.pending > 0 {
            parts.push(format!("{} pending", stats.pending));
        }
        if stats.retrying > 0 {
            parts.push(format!("{} retrying", stats.retrying));
        }
        if stats.paused > 0 {
            parts.push(format!("{} paused", stats.paused));
        }
        parts.join(" · ")
    };

    // The finished work as a success bar: filled share = succeeded / finished.
    // Green fill on a dim track, so health reads at a glance without counting
    // the ✓/✕ numbers (which stay for precision).
    let finished = stats.finished();
    let done = if finished == 0 {
        DoneRow::Plain("no finished work".to_string())
    } else {
        let ratio = stats.succeeded as f32 / finished as f32;
        let track = bar(ratio, 8);
        DoneRow::Rich(vec![
            ("health ".to_string(), Style::default().fg(Color::DarkGray)),
            (track, Style::default().fg(Color::Green)),
            (
                format!(" {}✓", stats.succeeded),
                Style::default().fg(Color::Green),
            ),
            (
                format!(" {}✕", stats.failed),
                Style::default().fg(if stats.failed > 0 {
                    Color::Red
                } else {
                    Color::DarkGray
                }),
            ),
        ])
    };

    vec![vec![
        ("vitals ".to_string(), Style::default().fg(Color::DarkGray)),
        (in_flight, Style::default().fg(Color::Gray)),
    ]]
    .into_iter()
    .chain(
        // `done` is either one row (no finished work) or a rich coloured row.
        match done {
            DoneRow::Plain(text) => vec![vec![(text, Style::default().fg(Color::Gray))]],
            DoneRow::Rich(row) => vec![row],
        },
    )
    .collect()
}

/// The second vitals row: either a placeholder string or a pre-styled row.
enum DoneRow {
    Plain(String),
    Rich(Row),
}

/// P5: uptime, plus Jev's state and model.
/// P5: uptime, Jev's state, and the connected MCP servers.
///
/// The MCP row is why an employee's tools answer — when it reads `mcp 5/5` all
/// the integrations (filesystem, browser, Blender, …) are up; a red `mcp 3/5`
/// says two are down and which to check.
fn runtime(app: &App, _width: usize) -> Vec<Row> {
    let uptime = fmt_uptime(app.uptime());
    let jev = match app.jev_model() {
        Some(model) if app.jev_enabled() => format!("jev {model}"),
        _ if app.jev_enabled() => "jev on".to_string(),
        _ => "jev off".to_string(),
    };

    let mut rows = vec![
        vec![
            ("uptime ".to_string(), Style::default().fg(Color::DarkGray)),
            (uptime, Style::default().fg(Color::Gray)),
        ],
        vec![(jev, Style::default().fg(Color::Gray))],
    ];
    if let Some((up, total)) = app.mcp_summary() {
        rows.push(vec![
            ("mcp ".to_string(), Style::default().fg(Color::DarkGray)),
            (
                format!("{up}/{total}"),
                Style::default().fg(if up == total {
                    Color::Green
                } else {
                    Color::Yellow
                }),
            ),
        ]);
    }
    rows
}

/// P6: what is broken, two rows — a failed task and the connection state.
fn alerts(app: &App, _width: usize) -> Vec<Row> {
    let failure = match app.last_failure() {
        Some(goal) => vec![
            ("alert ".to_string(), Style::default().fg(Color::DarkGray)),
            ("✕ ".to_string(), Style::default().fg(Color::Red)),
            (goal.to_string(), Style::default().fg(Color::Red)),
        ],
        None => vec![
            ("alert ".to_string(), Style::default().fg(Color::DarkGray)),
            ("✓ none".to_string(), Style::default().fg(Color::DarkGray)),
        ],
    };
    let connection = match app.connection_error() {
        Some(err) => vec![(err.to_string(), Style::default().fg(Color::Red))],
        None => vec![(
            "connection ok".to_string(),
            Style::default().fg(Color::DarkGray),
        )],
    };
    vec![failure, connection]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::message::Message;
    use crate::team::Team;

    fn app() -> App {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
        App::new(Config::default(), tx)
    }

    /// A section of `lines` rows at `priority`, built directly so `fit` can be
    /// exercised without the app. Each row carries the marker so the
    /// `Section::new` invariant holds.
    fn section(priority: usize, lines: usize) -> Section {
        let marker = MARKERS[priority];
        Section::new(
            priority,
            (0..lines)
                .map(|index| vec![(format!("{marker}{index}"), Style::default())])
                .collect(),
            80,
        )
    }

    /// `fit` keeps a prefix: once a section doesn't fit, nothing after it is
    /// drawn — the stack degrades from the bottom, never in the middle.
    #[test]
    fn fit_drops_whole_sections_by_priority() {
        // 2 rows, then 2+1, then 2+1 → cumulative 2, 5, 8.
        let build = || vec![section(0, 2), section(1, 2), section(2, 2)];

        let all = fit(build(), 8);
        assert_eq!(all.len(), 8);
        assert!(all.iter().any(|line| line.to_string().contains(MARKERS[2])));

        // Room for P0+P1 (5) but not P2 (8).
        let two = fit(build(), 7);
        assert_eq!(two.len(), 5);
        assert!(two.iter().any(|line| line.to_string().contains(MARKERS[1])));
        assert!(!two.iter().any(|line| line.to_string().contains(MARKERS[2])));

        // Room for P0 only.
        let one = fit(build(), 4);
        assert_eq!(one.len(), 2);
        assert!(!one.iter().any(|line| line.to_string().contains(MARKERS[1])));
    }

    /// The height is a hard ceiling: whatever the budget, `fit` never exceeds it.
    #[test]
    fn fit_never_exceeds_the_height() {
        let heights = [4, 1, 4, 1, 2, 2, 2];
        for height in 0..40usize {
            let sections = heights
                .iter()
                .enumerate()
                .map(|(priority, &lines)| section(priority, lines))
                .collect();
            let lines = fit(sections, height);
            assert!(
                lines.len() <= height,
                "height {height} produced {} lines",
                lines.len()
            );
        }
    }

    /// The frozen budget from the acceptance spec — cumulative 6, 11, 13, 16, 19,
    /// 22 — reproduces §3's size table through the derivation rule.
    #[test]
    fn fit_reproduces_the_frozen_budget() {
        let stack = || {
            [4usize, 1, 4, 1, 2, 2, 2]
                .iter()
                .enumerate()
                .map(|(priority, &lines)| section(priority, lines))
                .collect()
        };

        // (inner height, expected rendered rows) for the pinned sizes.
        for (inner, expected) in [(40, 22), (30, 22), (20, 19), (14, 13), (6, 6)] {
            assert_eq!(
                fit(stack(), inner).len(),
                expected,
                "inner height {inner} should draw {expected} rows"
            );
        }
    }

    #[test]
    fn fmt_uptime_formats() {
        assert_eq!(fmt_uptime(Duration::from_secs(0)), "0s");
        assert_eq!(fmt_uptime(Duration::from_secs(59)), "59s");
        assert_eq!(fmt_uptime(Duration::from_secs(60)), "1m");
        assert_eq!(fmt_uptime(Duration::from_secs(5 * 60)), "5m");
        assert_eq!(fmt_uptime(Duration::from_secs(59 * 60)), "59m");
        assert_eq!(fmt_uptime(Duration::from_secs(3600)), "1h 0m");
        assert_eq!(fmt_uptime(Duration::from_secs(3600 + 12 * 60)), "1h 12m");
        assert_eq!(fmt_uptime(Duration::from_secs(26 * 3600)), "1d 2h");
    }

    #[test]
    fn sparkline_is_width_safe() {
        assert_eq!(sparkline(&[], 0), "");
        assert_eq!(sparkline(&[1, 2, 3], 0), "");
        // Empty samples still draw a flat row of the requested width.
        assert_eq!(sparkline(&[], 3).chars().count(), 3);
        // More samples than width keeps the most recent ones.
        let many: Vec<u64> = (0..100).collect();
        assert_eq!(sparkline(&many, 10).chars().count(), 10);
        // One sample fills one cell.
        assert_eq!(sparkline(&[5], 1).chars().count(), 1);
        // All-zero is flat, not empty.
        assert_eq!(sparkline(&[0, 0, 0], 3).chars().count(), 3);
        // The tallest sample reaches the full block.
        assert!(sparkline(&[0, 10], 2).ends_with('█'));
    }

    #[test]
    fn bar_clamps() {
        assert_eq!(bar(0.0, 4), "░░░░");
        assert_eq!(bar(1.0, 4), "████");
        assert_eq!(bar(0.5, 4), "██░░");
        // Out-of-range and NaN are clamped, never panicking.
        assert_eq!(bar(-1.0, 4), "░░░░");
        assert_eq!(bar(2.0, 4), "████");
        assert_eq!(bar(f32::NAN, 4), "░░░░");
        // Zero width is empty, not a panic.
        assert_eq!(bar(0.5, 0), "");
        // Every result is exactly `width` cells.
        for ratio in [0.0, 0.1, 0.33, 0.66, 0.99, 1.0] {
            assert_eq!(bar(ratio, 7).chars().count(), 7, "ratio {ratio}");
        }
    }

    /// Every marker is unique across the sidebar and drawn on its section's first
    /// line — the acceptance spec's rule 6, and what makes A5 decidable.
    #[test]
    fn markers_are_unique_to_their_section() {
        let sections = sections(&app(), 22);
        assert_eq!(sections.len(), MARKERS.len());

        let mut seen = std::collections::HashSet::new();
        for section in &sections {
            let marker = MARKERS[section.priority];
            assert!(seen.insert(marker), "duplicate marker {marker}");
            assert!(
                section
                    .lines
                    .iter()
                    .any(|line| line.to_string().contains(marker)),
                "section {marker} must draw its marker"
            );
        }
    }

    /// At the narrowest pinned size (40×24, inner width 6) the nameplate is
    /// ellipsised rather than dropped — A10's documented relaxation.
    #[test]
    fn narrow_nameplate_is_ellipsised_not_dropped() {
        let line = truncate_spans(
            vec![(MINI_NAMEPLATE.to_string(), Style::default().fg(Color::Cyan))],
            6,
        );
        let nameplate = line.to_string();
        assert!(
            nameplate.starts_with("MYW"),
            "the marker must survive as a prefix: {nameplate:?}"
        );
        assert!(
            nameplate.ends_with('…'),
            "a truncated nameplate is ellipsised: {nameplate:?}"
        );
        assert_eq!(nameplate.chars().count(), 6, "it fits the panel exactly");
    }

    /// A composite row truncated as one string ends in an ellipsis — the defect
    /// A6 caught at 40×24, where two independently truncated halves rendered
    /// `ope… ·` and the terminal clipped the join.
    #[test]
    fn a_composite_row_truncates_as_one_string() {
        let line = truncate_spans(
            vec![
                ("openai".to_string(), Style::default()),
                (" · ".to_string(), Style::default()),
                ("session —".to_string(), Style::default()),
            ],
            6,
        );
        let text = line.to_string();
        assert_eq!(text, "opena…", "one truncation, ending in an ellipsis");
    }

    /// A row that fits is left exactly as composed, ellipsis-free.
    #[test]
    fn a_row_that_fits_is_untouched() {
        let line = truncate_spans(
            vec![
                ("openai".to_string(), Style::default()),
                (" · ".to_string(), Style::default()),
                ("session".to_string(), Style::default()),
            ],
            22,
        );
        assert_eq!(line.to_string(), "openai · session");
    }

    /// The inspector names the selected employee's model, which is the whole
    /// point of P2. Selection starts on the first card in workflow order — the
    /// planner — so that is the employee whose model the inspector shows.
    #[test]
    fn inspector_shows_the_selected_employees_model() {
        let mut team = Team::starter();
        let planner = team
            .employees()
            .iter()
            .position(|employee| employee.id == "planner")
            .expect("the starter roster has a planner");
        team.set_model(
            planner,
            "openai/gpt-6-luna-pro".into(),
            Some("openai".into()),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(1);
        let app = App::with_team(Config::default(), tx, team);
        assert_eq!(
            app.selected(),
            planner,
            "the first card in workflow order is selected first"
        );

        let text: String = inspector(&app)
            .iter()
            .flatten()
            .map(|(text, _)| text.as_str())
            .collect();
        assert!(text.contains("Dex"), "{text}");
        assert!(text.contains("openai/gpt-6-luna-pro"), "{text}");
        assert!(text.contains("openai"), "{text}");
    }

    /// A7's vocabulary is the sidebar's own labels; it must not contain the
    /// log/title-owned tokens the assertion forbids.
    #[test]
    fn labels_avoid_log_owned_tokens() {
        for label in labels() {
            for owned in ["ram ", "cpu ", "events", "cache ", "opencode v"] {
                assert!(
                    !label.contains(owned),
                    "label {label:?} duplicates log text {owned:?}"
                );
            }
        }
    }
}
