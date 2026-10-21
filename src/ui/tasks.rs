//! The "tasks" panel: the persisted queue block above a per-employee board.
//! Read-only — the grid stays the selection surface; this panel just surfaces
//! what each teammate is up to.
//!
//! Above the board sits the queue block, specified row-for-row in
//! [`docs/phase-b-queue-rows.md`](../../docs/phase-b-queue-rows.md): a group
//! header per employee, one glyph-led row per queued task, a single `+N more`
//! overflow row, and a trailing held-prompts row. The panel is a fixed 32
//! columns (`Borders::ALL`), so every row is budgeted against **30 usable cells**
//! and nothing may exceed it — a row that runs long is silently clipped by the
//! terminal, which is the failure the spec's budgets exist to prevent.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::theme;
use crate::app::{self, App};
use crate::task::{Task, TaskStatus};
use crate::team::Employee;
use crate::ui::{status, truncate};

/// Usable cells inside the panel's borders. Every row sums to this.
const USABLE: usize = 30;

/// R0's name budget — cols 2–13. The same cap the board's head row uses, so a
/// name truncates at the same column in both blocks (§3).
const HEADER_NAME: usize = 12;

/// The board's head row: `glyph(1) + space(1) + name(12) + " · "(3) + role(13)`.
/// The role gets 13 because the longest shipped role, `Research lead`, is 13 —
/// budgeting it 12 ellipsised a real role (F2).
const BOARD_ROLE: usize = USABLE - 2 - HEADER_NAME - 3;

/// R1's word column: the frozen §2.6 word padded to 9, cols 4–12 (§4.1).
const WORD: usize = 9;

/// R1's summary budget — cols 14–29.
/// `30 − (indent 2 + glyph 1 + space 1 + word 9 + space 1)`.
const SUMMARY: usize = USABLE - 2 - 1 - 1 - WORD - 1;

/// The column every row's detail starts at — R1's summary, R2's count, R3's
/// count (§1). Derived so the three can never drift apart.
const DETAIL_COL: usize = 2 + 1 + 1 + WORD + 1;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let employees = app.team().employees();
    let order = app.workflow_order();
    let tasks = app.tasks();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::BORDER)
        .title(" tasks ".bold().cyan());

    if order.is_empty() {
        frame.render_widget(Paragraph::new("no employees").block(block), area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // The queue block. Height-capped, so it can never grow until the board
    // below is clipped off the panel (D1).
    //
    // It also reports the goals it actually **drew**, which is what lets the
    // board below drop a duplicate without ever hiding a goal the block capped
    // away behind `+N more`.
    let drawn: Drawn = if !tasks.is_empty() || app.is_paused() {
        let (block_lines, drawn) = queue_block(app, area);
        lines.extend(block_lines);
        drawn
    } else {
        Drawn::default()
    };

    for &index in &order {
        let employee = &employees[index];
        let selected = index == app.selected();
        lines.push(employee_header(employee, app.frame(), selected));
        // The board row yields only when this exact goal is already on screen
        // in the queue block above (see `task_line`).
        let in_queue = employee
            .last_task
            .as_deref()
            .is_some_and(|goal| drawn.contains(index, goal));
        lines.push(task_line(employee, in_queue));
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

// ---------------------------------------------------------------------------
// The queue block
// ---------------------------------------------------------------------------

/// One group of the block: an employee's header plus the tasks drawn under it.
struct Group<'a> {
    /// The roster index that owns this group, or `None` for the `—` unassigned
    /// group (which no board row can match).
    owner: Option<usize>,
    header: Line<'static>,
    items: Vec<&'a Task>,
}

/// The goals the queue block **actually drew**, per employee.
///
/// Not "the tasks the employee has" — the ones on screen. The block is
/// height-capped, so a queued goal can be counted into `+N more` instead of
/// drawn; the board must still show that goal, because otherwise it would
/// disappear from the panel entirely. Comparing against the drawn set (rather
/// than against `app.tasks_for`) is what keeps "drop the duplicate" from becoming
/// "drop the only copy".
///
/// Keys are roster indices; the `—` unassigned group is not keyed because no
/// board row can own it.
#[derive(Default)]
struct Drawn {
    goals: Vec<(usize, String)>,
}

impl Drawn {
    fn push(&mut self, employee: usize, task: &Task) {
        self.goals
            .push((employee, app::task_summary(&task.goal)));
    }

    /// True when `employee`'s `goal` is one of the goals drawn in the block.
    ///
    /// Both sides are compared at `task_summary` granularity — the same function
    /// that produced `Employee::last_task` — so the comparison is between what
    /// the two rows *mean*, before either is truncated for display.
    fn contains(&self, employee: usize, goal: &str) -> bool {
        self.goals
            .iter()
            .any(|(owner, drawn)| *owner == employee && drawn == goal)
    }
}

/// The whole block, fitted to the panel's height.
///
/// Returns the rows to draw and the goals that were drawn, so the board can drop
/// a duplicate without hiding a goal the cap removed.
///
/// Rows are laid out in the spec's frozen ladder order (§7.2): the blank
/// separator before the held row goes first, then item rows from the tail of
/// each group, then whole groups, and only then the held row itself.
fn queue_block<'a>(app: &'a App, area: Rect) -> (Vec<Line<'static>>, Drawn) {
    let employees = app.team().employees();

    // Groups in workflow order, then the `—` group of unassigned work.
    let mut groups: Vec<Group<'a>> = Vec::new();
    for &index in &app.workflow_order() {
        let employee = &employees[index];
        let items = app.tasks_for(index);
        if items.is_empty() {
            continue;
        }
        groups.push(Group {
            owner: Some(index),
            // R0 carries the employee's true depth, not the rows drawn (F3).
            header: group_header(employee, app.frame(), items.len()),
            items,
        });
    }
    let unassigned = app.unassigned_tasks();
    if !unassigned.is_empty() {
        groups.push(Group {
            owner: None,
            header: placeholder_header(),
            items: unassigned,
        });
    }

    let held = app.held_tasks();

    let mut lines = vec![queue_heading(app.is_paused())];

    // `H` is the panel's content height; the block may take half of it so the
    // board below always keeps at least as many rows as the queue (§7.1).
    let h = area.height.saturating_sub(2) as usize;
    let mut allowance = h / 2;

    // The held row (blank separator + row) is reserved up front and released
    // only if the block cannot otherwise fit anything (§7.2 step 4).
    let held_rows = if held > 0 { 2 } else { 0 };
    let heading_rows = 1;
    if allowance < heading_rows + held_rows {
        allowance = heading_rows + held_rows;
    }
    let mut budget = allowance.saturating_sub(heading_rows + held_rows);
    let mut separated = held > 0;

    // Step 1: drop the blank separator when one row is what the block needs.
    if separated && budget == 0 && groups.iter().any(|g| !g.items.is_empty()) {
        budget += 1;
        separated = false;
    }

    // Step 2/3: draw groups while they fit; count the rest.
    //
    // `hidden` is the single source of truth for `+N`: every item not drawn is
    // added exactly once, so the count can never disagree with the rows above it.
    //
    // The `+N more` row is **reserved before any item row** (§7.3), so it
    // is the last thing to be dropped, never the first. The reservation is spent
    // where the row is actually needed — `room = remaining - 2` below — rather
    // than subtracted up front, which would cost an item row for nothing.
    let mut remaining = budget;

    let mut hidden = 0usize;
    let mut pending_groups: Vec<Line<'static>> = Vec::new();
    // Once a group cannot be drawn whole, nothing below it is drawn: a header
    // followed by a gap would read as "nothing queued" (§4.3).
    let mut stopped = false;

    // The goals this call actually draws. Filled alongside `pending_groups`, so
    // the two can never disagree about what is on screen.
    let mut drawn = Drawn::default();

    for group in groups {
        if stopped {
            hidden += group.items.len();
            continue;
        }

        let wanted = 1 + group.items.len();
        if wanted <= remaining {
            pending_groups.push(group.header);
            for task in &group.items {
                pending_groups.push(queued_task_line(task));
                if let Some(owner) = group.owner {
                    drawn.push(owner, task);
                }
            }
            remaining -= wanted;
            continue;
        }

        // Does not fit whole. Draw it only when its header plus at least one
        // item fits *alongside* the overflow row its own remainder makes
        // necessary — a bare header would imply an employee has no work queued.
        if remaining >= 3 {
            pending_groups.push(group.header);
            let room = remaining - 2; // header, then the reserved `+N more` row
            for task in group.items.iter().take(room) {
                pending_groups.push(queued_task_line(task));
                if let Some(owner) = group.owner {
                    drawn.push(owner, task);
                }
            }
            hidden += group.items.len() - room;
        } else {
            hidden += group.items.len();
        }
        stopped = true;
        remaining = 0;
    }

    lines.extend(pending_groups);
    if hidden > 0 {
        lines.push(overflow_line(hidden));
    }

    if held > 0 {
        if separated {
            lines.push(Line::from(""));
        }
        lines.push(held_line(held));
    }

    // A closing blank always separates the block from the board. Without it the
    // last queue row sits directly above `● Aria · Research lead` and reads as a
    // queue row for Aria (F7). It is *not* counted against the block's allowance
    // (§7.1: `A` excludes the closing blank).
    lines.push(Line::from(""));

    (lines, drawn)
}

/// The block's heading: `scheduled`, plus the engine-gate word when set.
///
/// **Not `queue`.** The block lists persisted **scheduled tasks** — cron and
/// watch fires recorded in the store — and it draws them in every state,
/// including `succeeded` and `failed`. Calling a block that lists finished work a
/// "queue" is a misnomer: finished work is not queued, and the row beneath the
/// heading may read `succeeded`. `scheduled` names what the rows are (tasks with
/// a trigger), and it stays true across all six states. The engine gate keeps its
/// word because it *is* about queuing: while paused, fires are held.
///
/// Flush at col 0, like every other row in the panel (F8). The `⏸` glyph is
/// dropped here because R3 carries it for the same state — two marks for one fact
/// is noise in a 30-column panel (§6.5).
fn queue_heading(paused: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "scheduled",
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )];
    if paused {
        spans.push(Span::styled(
            " · engine paused",
            Style::default()
                .fg(theme::WARN)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// One queue item (R1): indent, state glyph, frozen word, then the goal summary.
///
/// The word **stays**. `keymap.md` §7 freezes this row as
/// `<glyph> <word padded to 9> <summary>` and cites this very row; §2.7 drops the
/// word only below usable 7, and this panel has 30. The glyph therefore costs two
/// cells of goal text rather than replacing the word (§4.1).
fn queued_task_line(task: &Task) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            status::task_glyph(task.status),
            Style::default().fg(status::task_color(task.status)),
        ),
        Span::raw(" "),
        // The word column is rigid: padded to 9 so `pending`/`retrying`/
        // `succeeded` all put the summary at col 14.
        Span::styled(
            format!("{:<WORD$}", task.status.label()),
            Style::default().fg(status::task_color(task.status)),
        ),
        // The separator is **not optional**. The word is padded to a fixed 9, so
        // a 9-character word (`succeeded`) ends flush against the summary and the
        // two render as one token — `succeededfinished long ago`. This space is
        // what makes the padded column a column rather than a concatenation.
        Span::raw(" "),
        // Prose is always `text`, never the state role: `err` and `muted` are
        // marker colours (2.85:1 / 4.16:1 on dark) and may colour a glyph but
        // never a sentence (§4.3).
        Span::styled(
            truncate(&app::task_summary(&task.goal), SUMMARY),
            theme::TEXT,
        ),
    ])
}

/// The single overflow row (R2): `+N more` in the detail column.
///
/// No glyph and no word: it has no state and is not a frozen §2.6 word, so cols
/// 2–12 are empty *by rule* (§5.1). `text`, not `muted`: `N` is the only copy of
/// how many rows are hidden, and `muted` is never the sole carrier of a fact.
fn overflow_line(hidden: usize) -> Line<'static> {
    Line::from(vec![
        Span::raw(" ".repeat(DETAIL_COL)),
        Span::styled(format!("+{hidden} more"), theme::TEXT),
    ])
}

/// The trailing held row (R3): `⏸ paused    ×N`.
///
/// Shares R1's grid so the word sits at col 4 on every count; the number trails
/// as `×N`, the panel's depth idiom (§6.1). The word is the frozen `Paused` word,
/// never `held` — the row is only *called* the held row. The whole row is `warn`,
/// which is not a marker colour on either scheme, so it may colour a phrase.
fn held_line(held: usize) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            status::task_glyph(TaskStatus::Paused),
            Style::default().fg(theme::WARN),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:<WORD$}", TaskStatus::Paused.label()),
            Style::default().fg(theme::WARN),
        ),
        Span::raw(" "),
        Span::styled(format!("×{held}"), Style::default().fg(theme::WARN)),
    ])
}

// ---------------------------------------------------------------------------
// The per-employee board
// ---------------------------------------------------------------------------

/// Line 1 of the **board**: status icon + name (+ role, dim).
///
/// `glyph(1) + space(1) + name(12) + " · "(3) + role(13)` = 30, so the row can
/// never be clipped. The role keeps 13 cells because `Research lead` — the longest
/// shipped role — is exactly 13; budgeting it 12 ellipsised a real role (F2).
fn employee_header(employee: &Employee, tick: u64, selected: bool) -> Line<'static> {
    let name_style = if selected {
        Style::default()
            .fg(theme::TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    // A header's own `glyph + space` is 2 cells, which is why an item's indent
    // is 2: its glyph lands under the name's first character (§2).
    let name = truncate(&employee.name, HEADER_NAME);
    let role = truncate(&employee.role, BOARD_ROLE);
    Line::from(vec![
        status::icon_span(employee.status, tick),
        Span::raw(" "),
        Span::styled(name, name_style),
        Span::styled(" · ", theme::MUTED),
        Span::styled(role, theme::MUTED),
    ])
}

/// R0 — the **queue block's** group header: `● Aria ×3`.
///
/// Deliberately *not* [`employee_header`]. That row is the board's, and drawing it
/// here repeated `● Aria · Research lead` twice in one panel, five rows apart, while
/// saying nothing about the queue (F3). R0 carries the one fact only this block
/// knows: the employee's **true** queue depth, `×N` — never the number of rows
/// drawn, so a group that is counted still reports its real size (§3).
///
/// `×0` is never drawn: a group with no items is not drawn at all.
fn group_header(employee: &Employee, tick: u64, depth: usize) -> Line<'static> {
    Line::from(vec![
        status::icon_span(employee.status, tick),
        Span::raw(" "),
        Span::styled(
            truncate(&employee.name, HEADER_NAME),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(format!("×{depth}"), theme::TEXT),
    ])
}

/// The header for the unassigned group: the board's `—` placeholder in `meta`.
///
/// `—` is a placeholder, not a state word, so §2.6's vocabulary is untouched. It
/// has no glyph entry and no `×N` — nothing is being counted *for* anyone.
fn placeholder_header() -> Line<'static> {
    Line::from(vec![
        Span::styled("—", theme::MUTED),
        Span::styled(" unassigned", theme::MUTED),
    ])
}

/// Line 2: the employee's latest task, or a muted placeholder when idle.
///
/// **The board row yields to the queue block.** `last_task` is a *latched
/// summary* — set on prompt send, delegation, and dispatch, and kept after the
/// turn ends — so when the same goal is already listed in the queue block above,
/// drawing it here again puts one piece of work on two rows of one panel and
/// reads as two. `in_queue` says whether the block already shows this employee's
/// goal, in which case this row keeps its shape (the panel's height never depends
/// on what it says) but draws nothing.
///
/// Prose is `text`, not `status::text_color`: for an `Error` employee that
/// returns `err`, which would draw a sentence at 2.85:1 (D5).
fn task_line(employee: &Employee, in_queue: bool) -> Line<'static> {
    match &employee.last_task {
        // Already listed in the queue block: the row is drawn empty rather than
        // omitted, so the panel's geometry stays constant (F5, "five slots").
        Some(_) if in_queue => Line::from(""),
        Some(task) => Line::from(vec![
            Span::raw("  "),
            Span::styled(truncate(task, 26), theme::TEXT),
        ]),
        None => Line::from(Span::styled("  —", theme::MUTED)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(goal: &str, status: TaskStatus, created_at: i64) -> Task {
        let mut task = Task::new(goal, Some("writer".into()));
        task.status = status;
        task.created_at = created_at;
        task
    }

    /// The block never exceeds its height allowance — the defect this whole
    /// spec exists to fix (D1). A queue far longer than the panel must be capped,
    /// not allowed to push the per-employee board off the bottom.
    ///
    /// §7.1's contract is `A = ⌊H/2⌋`: the block may take at most half the
    /// panel's content rows, so the board below always keeps at least as many.
    /// Asserted on the block itself at all four pinned sizes, because a rendered
    /// frame cannot distinguish "the block is capped" from "the board happens to
    /// fit" — at 80×24 four employees do not fit in the board's own half.
    #[test]
    fn the_block_never_outgrows_its_allowance() {
        use crate::app::App;
        use crate::config::Config;
        use crate::message::Message;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(4);
        let mut app = App::new(Config::default(), tx);
        // Far more queued work than any panel can draw, seeded through the
        // public autonomous-fire path (`store` is private to `app`).
        for n in 0..40 {
            app.update(Message::WatchedTask {
                employee: "writer".into(),
                goal: format!("queued goal {n}"),
            });
            let _ = rx.try_recv();
        }
        assert_eq!(app.tasks().len(), 40, "the fixture queued no work");

        // `H` per `keymap.md` §5.1 at each pinned size.
        for (width, height, h) in [
            (32u16, 32u16, 30usize),
            (32, 22, 20),
            (32, 16, 14),
            (32, 8, 6),
        ] {
            let area = Rect::new(0, 0, width, height);
            let (lines, drawn) = queue_block(&app, area);
            // `A` excludes the closing blank, so the block is at most `H/2`
            // counted rows plus that one separator (§7.1, §7.3).
            assert!(
                lines.len() <= h / 2 + 1,
                "{width}x{height}: the block drew {} rows, over its {} allowance",
                lines.len(),
                h / 2
            );
            // And it reports the rest rather than silently dropping them.
            assert!(
                lines
                    .iter()
                    .any(|line| { line.spans.iter().any(|span| span.content.contains(" more")) }),
                "{width}x{height}: the capped block drew no overflow count"
            );
            // The reported set is what is on screen, not what the employee has:
            // a capped block draws fewer goals than it counts.
            assert!(
                drawn.goals.len() < 40,
                "{width}x{height}: every goal was reported drawn despite the cap"
            );
        }
    }

    /// **Regression: the word column needs its own separator.** The status word is
    /// padded to 9 cells with no gap before the summary, so `succeeded` + a goal
    /// beginning `f…` rendered as the single token `succeededfinished long ago`.
    /// Each of the six words must be followed by a space.
    #[test]
    fn the_status_word_cannot_run_into_its_goal() {
        for (status, word) in [
            (TaskStatus::Pending, "pending"),
            (TaskStatus::Running, "running"),
            (TaskStatus::Paused, "paused"),
            (TaskStatus::Retrying, "retrying"),
            (TaskStatus::Succeeded, "succeeded"),
            (TaskStatus::Failed, "failed"),
        ] {
            let mut task = Task::new("finished long ago", Some("writer".into()));
            task.status = status;
            let text: String = queued_task_line(&task)
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            // The word must be followed by whitespace before the goal text.
            assert!(
                text.contains(&format!("{word} ")),
                "{status:?}: the word runs into the goal: {text:?}"
            );
            assert!(
                !text.contains(&format!("{word}finished")),
                "{status:?}: `{word}finished` must be impossible: {text:?}"
            );
        }
    }

    /// Every drawn row fits the panel's 30 usable cells. A row wider than this
    /// is silently clipped by the terminal — the defect the budgets exist to
    /// prevent.
    #[test]
    fn every_row_fits_the_usable_width() {
        let lines = vec![
            queue_heading(false),
            queue_heading(true),
            queued_task_line(&task(
                "a very long goal that will need truncating",
                TaskStatus::Running,
                1,
            )),
            overflow_line(20),
            held_line(3),
            placeholder_header(),
        ];
        for line in lines {
            let width: usize = line
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum();
            assert!(width <= USABLE, "{:?} is {width} cells", line.spans.first());
        }
    }

    /// The board header's budgets add up: `glyph(1) + space(1) + name(12) +
    /// " · "(3) + role(13) = 30`. The longest shipped role fits whole.
    #[test]
    fn board_header_budget_sums_to_thirty() {
        const SEPARATOR: usize = 3;
        assert_eq!(2 + HEADER_NAME + SEPARATOR + BOARD_ROLE, USABLE);
        // The longest shipped role (`Research lead`, 13) is never cut (F2).
        assert_eq!(truncate("Research lead", BOARD_ROLE), "Research lead");
        // And the item row's own budget sums the same way.
        assert_eq!(2 + 1 + 1 + WORD + 1 + SUMMARY, USABLE);
    }

    /// R0 carries the employee's true depth, never the rows drawn (F3), and the
    /// word column is rigid so the summary starts at the same column on every
    /// item row.
    #[test]
    fn group_header_carries_the_depth_and_rows_align() {
        let employee = Employee::new("writer", "Bruno", "Writer");
        let header = group_header(&employee, 0, 3);
        let text: String = header.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("×3"), "{text}");
        assert!(text.contains("Bruno"), "{text}");
        // The role is the board's fact, not the block's — no duplication (F3).
        assert!(
            !text.contains("Writer"),
            "R0 must not repeat the role: {text}"
        );

        // Every item row puts its summary at DETAIL_COL, whatever the word's
        // length — that is what the padded word column buys (§1).
        for status in [
            TaskStatus::Pending,
            TaskStatus::Retrying,
            TaskStatus::Succeeded,
        ] {
            let line = queued_task_line(&task("goal", status, 1));
            let prefix: usize = line.spans[..line.spans.len() - 1]
                .iter()
                .map(|span| span.content.chars().count())
                .sum();
            assert_eq!(prefix, DETAIL_COL, "{status:?} summary is misaligned");
        }
    }

    /// An item's glyph sits at col 2 — directly under the first character of the
    /// name — because both rows spend `glyph + space` before their text.
    #[test]
    fn item_glyph_aligns_under_the_name_column() {
        let employee = Employee::new("writer", "Bruno", "Writer");
        let header = group_header(&employee, 0, 1);
        let item = queued_task_line(&task("x", TaskStatus::Pending, 1));

        let header_glyph: usize = header.spans[0].content.chars().count();
        let item_indent = item.spans[0].content.chars().count();
        assert_eq!(header_glyph, 1);
        assert_eq!(item_indent, 2, "indent must be the header's glyph + space");
    }

    /// The queue block never draws the employee's `▶`: that mark belongs to the
    /// board's head row, and the block's rows are tasks (§2.1, §2.7 rule 1).
    #[test]
    fn no_employee_glyph_in_the_queue_block() {
        for status in [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Paused,
            TaskStatus::Retrying,
            TaskStatus::Succeeded,
            TaskStatus::Failed,
        ] {
            let line = queued_task_line(&task("x", status, 1));
            let glyph = &line.spans[1].content;
            assert_ne!(glyph, "▶", "{status:?} drew the employee glyph");
            assert_eq!(glyph, status::task_glyph(status));
        }
    }

    /// The held row draws the frozen `Paused` triple: glyph `⏸`, word `paused`,
    /// and the count trailing as `×N` in the detail column (§6.1).
    #[test]
    fn held_row_uses_the_frozen_word() {
        let line = held_line(3);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("⏸"), "{text}");
        assert!(text.contains("paused"), "{text}");
        assert!(text.contains("×3"), "the count must trail as ×N: {text}");
        assert!(!text.contains("held"), "the word must be frozen: {text}");
        // The word sits at col 4 on every count, because the column is padded:
        // indent(2) + glyph(1) + space(1) = 4.
        let prefix: usize = line.spans[..3]
            .iter()
            .map(|span| span.content.chars().count())
            .sum();
        assert_eq!(prefix, 4, "the word must start at col 4");
    }

    /// The overflow row carries no glyph — it has no state (§8).
    #[test]
    fn overflow_row_has_no_glyph() {
        let line = overflow_line(20);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text.trim(), "+20 more");
        assert_eq!(line.spans.len(), 2, "indent + text, no glyph column");
    }

    /// Summary prose is `text` on every row, never the state role — `err` on a
    /// sentence is 2.85:1 (D5).
    #[test]
    fn summary_prose_is_never_the_state_role() {
        for status in [TaskStatus::Failed, TaskStatus::Pending] {
            let line = queued_task_line(&task("x", status, 1));
            let summary = line.spans.last().expect("summary span");
            assert_eq!(summary.style.fg, Some(theme::TEXT), "{status:?}");
            assert_ne!(summary.style.fg, Some(theme::ERR));
            assert_ne!(summary.style.fg, Some(theme::MUTED));
        }
    }
}
