//! The chat panel — a scrollable, markdown-aware transcript (fenced code
//! blocks, headings, blockquotes, and inline `` `code` ``, `**bold**`,
//! `*italic*`, `~~strike~~`, and `[text](url)` links), a slash-command
//! autocomplete menu, and a cursor-editing input prompt.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem as WidgetListItem, ListState, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, TranscriptLine, TranscriptRole};
use crate::ui::truncate;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [transcript_area, input_area] = super::layout::chat_areas(area);

    render_transcript(frame, app, transcript_area);
    render_input(frame, app, input_area);
    render_command_menu(frame, app, area);
    render_session_picker(frame, app, area);
}

// --- transcript ---

fn render_transcript(frame: &mut Frame, app: &App, area: Rect) {
    let name = app.chat_employee_name();
    let inner_width = area.width.saturating_sub(2) as usize;
    let inner_height = area.height.saturating_sub(2) as usize;

    // The thinking indicator is a *pinned* row, not part of the transcript: it
    // says the employee is busy right now, so it must stay visible while the user
    // scrolls back through history (and it is not a message, so it must not enter
    // the scrollback either).
    let thinking = app.thinking_line(inner_width);
    let body_height = inner_height.saturating_sub(usize::from(thinking.is_some()));

    let lines = transcript_lines(app.transcript(), name, app.streaming_text(), inner_width);

    // Scroll to the visible window (stick to the live tail unless the user has
    // paged up to read back through history).
    let scroll = app.chat_scroll();
    let max = lines.len().saturating_sub(body_height);
    let start = max.saturating_sub(scroll).min(max);
    let mut visible: Vec<Line> = lines.into_iter().skip(start).take(body_height).collect();
    if let Some(thinking) = thinking {
        visible.push(thinking);
    }

    let title = match app.chat_employee_status() {
        Some(status) => format!(" chat · {} · {} ", name.unwrap_or("chat"), status.label()),
        None => format!(" chat · {} ", name.unwrap_or("chat")),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray)
        .title(title)
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(Paragraph::new(visible).block(block), area);
}

/// Build the transcript's display lines: one blank spacer `Line` before each
/// message after the first, and before a non-empty streaming tail, so messages
/// read as visually distinct blocks. Spacers are ordinary lines — the scroll
/// math in `render_transcript` needs no special-casing for them.
fn transcript_lines(
    entries: &[TranscriptLine],
    name: Option<&str>,
    streaming: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for entry in entries {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        transcript_block(&mut lines, entry, width, name);
    }
    if !streaming.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        tagged_block(
            &mut lines,
            "◔ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            streaming,
            Style::default().fg(Color::Yellow),
            None,
            width,
        );
    }
    lines
}

/// Render one transcript entry into wrapped, styled display lines.
fn transcript_block(
    lines: &mut Vec<Line>,
    entry: &TranscriptLine,
    width: usize,
    name: Option<&str>,
) {
    let (tag, tag_style, base) = role_mapping(entry.role, name);
    tagged_block(
        lines,
        &tag,
        tag_style,
        &entry.text,
        base,
        entry.meta.as_deref(),
        width,
    );
}

/// Wrap `text` (markdown-aware) into display lines, prefixing `tag` on the
/// first line and indenting continuation lines to keep alignment.
fn tagged_block(
    lines: &mut Vec<Line>,
    tag: &str,
    tag_style: Style,
    text: &str,
    base: Style,
    meta: Option<&str>,
    width: usize,
) {
    let tag_width = tag.width();
    let body_width = width.saturating_sub(tag_width);
    let body = markdown_lines(text, body_width, base);

    for (i, line) in body.into_iter().enumerate() {
        let mut spans: Vec<Span> = Vec::new();
        if i == 0 && !tag.is_empty() {
            spans.push(Span::styled(tag.to_string(), tag_style));
        } else if i > 0 {
            spans.push(Span::raw(" ".repeat(tag_width)));
        }
        spans.extend(line.spans);
        if i == 0
            && let Some(meta) = meta
        {
            spans.push(Span::styled(
                format!(" · {meta}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(spans));
    }
}

/// Map a transcript role to its `(tag, tag_style, base_style)`. The assistant
/// tag carries the employee's actual name (falling back to `"assistant"` when
/// unknown) so a reply reads as the named worker speaking, not an opaque "wkr".
fn role_mapping(role: TranscriptRole, name: Option<&str>) -> (String, Style, Style) {
    match role {
        TranscriptRole::User => (
            "you ".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Style::default(),
        ),
        TranscriptRole::Assistant => {
            let label = name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("assistant");
            (
                format!("{label} "),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                Style::default(),
            )
        }
        TranscriptRole::Error => (
            "error ".to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Red),
        ),
        TranscriptRole::Info => (
            "".to_string(),
            Style::default(),
            Style::default().fg(Color::DarkGray),
        ),
    }
}

/// Lightweight markdown: fenced code blocks (cyan/dim, with the language tag
/// as a header line), headings (bold, sized by level), blockquotes (a dim `▌ `
/// gutter), horizontal rules (a dim line), tables (styled gutters), and inline
/// styling — `` `code` ``, `**bold**`, `*italic*`, `~~strike~~`, and
/// `[text](url)` links. Everything word-wraps to `width`; code lines are
/// chunked without collapsing indentation.
fn markdown_lines(text: &str, width: usize, base: Style) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code = false;
    let mut list_indent: Option<usize> = None;
    let marker_style = Style::default().fg(Color::Cyan);
    let gutter = Style::default().fg(Color::DarkGray);

    for raw in text.lines() {
        let trimmed = raw.trim_start();

        // Fence toggle ends any open list run. The opening fence names the
        // language when one is given — that tag tells the reader *what* the
        // block is before they parse its contents.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let opening = !in_code;
            in_code = !in_code;
            list_indent = None;
            if opening {
                let lang = trimmed.trim_start_matches(['`', '~']).trim();
                if !lang.is_empty() {
                    out.push(Line::from(Span::styled(
                        format!("─ {lang} "),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                out.push(Line::from(Span::styled(
                    "┌".to_string() + &"─".repeat(width.saturating_sub(1)),
                    gutter,
                )));
            } else {
                out.push(Line::from(Span::styled(
                    "└".to_string() + &"─".repeat(width.saturating_sub(1)),
                    gutter,
                )));
            }
            continue;
        }
        if in_code {
            for seg in chunk_lines(raw, width.saturating_sub(2)) {
                out.push(Line::from(vec![
                    Span::styled("│ ", gutter),
                    Span::styled(
                        seg,
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
                    ),
                ]));
            }
            continue;
        }
        // A blank line ends a list item/run.
        if raw.trim().is_empty() {
            out.push(Line::default());
            list_indent = None;
            continue;
        }
        // A horizontal rule reads as a visual separator, not three dashes.
        if trimmed.len() >= 3
            && trimmed
                .chars()
                .all(|c| c == '-' || c == '_' || c == '*' || c == ' ')
        {
            out.push(Line::from(Span::styled(
                "─".repeat(width),
                Style::default().fg(Color::DarkGray),
            )));
            list_indent = None;
            continue;
        }
        // Headings are bold; level 1–2 additionally underline, so the document
        // structure survives even a fast scroll.
        if let Some(heading) = heading_text(raw) {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            let mut style = base.add_modifier(Modifier::BOLD);
            if level <= 2 {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            for seg in wrap_text(heading, width) {
                out.push(Line::from(Span::styled(seg, style)));
            }
            list_indent = None;
            continue;
        }
        // A table row (the header row or a divider): render with gutter bars so
        // the columns read as a grid rather than a run of pipes in prose.
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            let cells: Vec<&str> = trimmed[1..trimmed.len() - 1].split('|').collect();
            let divider = cells
                .iter()
                .all(|c| c.trim().chars().all(|ch| ch == ':' || ch == '-'));
            let style = if divider {
                gutter
            } else {
                base.add_modifier(Modifier::BOLD)
            };
            let mut spans: Vec<Span> = vec![Span::styled("│ ", gutter)];
            let cell_width = width.saturating_sub(2) / cells.len().max(1);
            for (i, cell) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("│ ", gutter));
                }
                let last = i + 1 == cells.len();
                let available = if last {
                    width.saturating_sub(2 + i * (cell_width + 2))
                } else {
                    cell_width
                };
                spans.push(Span::styled(
                    truncate(&pad_cell(cell.trim(), available), available),
                    style,
                ));
            }
            out.push(Line::from(spans));
            list_indent = None;
            continue;
        }
        // A list item: hang the marker, align wrapped continuations under the
        // first word, and remember the indent for subsequent prose lines.
        if let Some(item) = list_item(trimmed) {
            let indent = item.width;
            let content_width = width.saturating_sub(indent);
            let segments = wrap_text(item.content, content_width);
            if let Some((first, rest)) = segments.split_first() {
                let mut spans = vec![Span::styled(item.marker.clone(), marker_style)];
                spans.extend(inline_line(first, base).spans);
                out.push(Line::from(spans));
                for seg in rest {
                    let mut spans = vec![Span::raw(" ".repeat(indent))];
                    spans.extend(inline_line(seg, base).spans);
                    out.push(Line::from(spans));
                }
            } else {
                out.push(Line::from(Span::styled(item.marker, marker_style)));
            }
            list_indent = Some(indent);
            continue;
        }
        // Blockquotes get a dim gutter bar: the quote's extent is visible even
        // across wrapped lines.
        if trimmed.starts_with('>') {
            let body = trimmed.strip_prefix('>').unwrap_or(trimmed).trim_start();
            for seg in wrap_text(body, width.saturating_sub(2)) {
                let mut spans = vec![Span::styled("▌ ", Style::default().fg(Color::Cyan))];
                spans.extend(inline_line(&seg, base.add_modifier(Modifier::DIM)).spans);
                out.push(Line::from(spans));
            }
            list_indent = None;
            continue;
        }
        // A non-empty prose line immediately after a list item is a continuation
        // of that item, kept at the item indent.
        if let Some(indent) = list_indent {
            for seg in wrap_text(raw, width.saturating_sub(indent)) {
                let mut spans = vec![Span::raw(" ".repeat(indent))];
                spans.extend(inline_line(&seg, base).spans);
                out.push(Line::from(spans));
            }
            continue;
        }
        // Plain prose.
        for seg in wrap_text(raw, width) {
            out.push(inline_line(&seg, base));
        }
    }

    // A code block left open at the end of the message (a truncated stream)
    // still gets its closing rule, so the transcript never ends inside a box.
    if in_code {
        out.push(Line::from(Span::styled(
            "└".to_string() + &"─".repeat(width.saturating_sub(1)),
            gutter,
        )));
    }

    out
}

/// Pad a table cell to `width` columns (space-right) for aligned columns.
fn pad_cell(cell: &str, width: usize) -> String {
    let len = cell.width();
    if len >= width {
        cell.to_string()
    } else {
        format!("{cell}{}", " ".repeat(width - len))
    }
}

/// A single Markdown list item: the rendered marker (with one trailing space)
/// plus the text that follows it.
struct ListItem<'a> {
    marker: String,
    content: &'a str,
    width: usize,
}

/// Detect a Markdown list item on an already-left-trimmed line, or `None`.
///
/// Unordered items start with `- `, `* `, or `+ `; ordered items start with one
/// or more digits then `. ` or `) `. The marker must be followed by whitespace,
/// so `---`, `***`, `snake_case`, and URLs are never misread as lists.
fn list_item(trimmed: &str) -> Option<ListItem<'_>> {
    let bytes = trimmed.as_bytes();
    let first = *bytes.first()?;

    // Unordered: `-`, `*`, or `+` followed by whitespace.
    if matches!(first, b'-' | b'*' | b'+') {
        let next = trimmed[1..].chars().next();
        if !next.is_some_and(|c| c.is_whitespace()) {
            return None;
        }
        let marker = format!("{} ", first as char);
        return Some(ListItem {
            width: marker.width(),
            marker,
            content: trimmed[1..].trim_start(),
        });
    }

    // Ordered: one or more digits followed by `.` or `)`, then whitespace.
    if first.is_ascii_digit() {
        let digit_len = bytes.iter().take_while(|&&b| b.is_ascii_digit()).count();
        let delim = bytes.get(digit_len).copied()?;
        if !matches!(delim, b'.' | b')') {
            return None;
        }
        let after = trimmed.get(digit_len + 1..)?;
        if !after.chars().next().is_some_and(|c| c.is_whitespace()) {
            return None;
        }
        let marker = format!("{}{} ", &trimmed[..digit_len], delim as char);
        return Some(ListItem {
            width: marker.width(),
            marker,
            content: after.trim_start(),
        });
    }

    None
}

/// The text of a Markdown heading (`# ` … `###### `), or `None`.
fn heading_text(raw: &str) -> Option<&str> {
    let trimmed = raw.trim_start();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes).is_some_and(|c| c == ' ') {
        Some(trimmed[hashes..].trim_start())
    } else {
        None
    }
}

/// Split `text` into word-wrapped lines of at most `width` display columns,
/// preserving explicit newlines and hard-wrapping overlong words.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line: Vec<String> = Vec::new();
        let mut len = 0usize;
        for word in raw.split_whitespace() {
            let wlen = word.width();
            if !line.is_empty() && len + 1 + wlen > width {
                flush_line(&mut out, &mut line, &mut len);
            }
            if wlen > width {
                // Hard-wrap an overlong word (only possible at line start now).
                for seg in chunk_lines(word, width) {
                    let slen = seg.width();
                    if !line.is_empty() && len + 1 + slen > width {
                        flush_line(&mut out, &mut line, &mut len);
                    }
                    line.push(seg);
                    len = if len == 0 { slen } else { len + 1 + slen };
                }
            } else {
                line.push(word.to_string());
                len = if len == 0 { wlen } else { len + 1 + wlen };
            }
        }
        flush_line(&mut out, &mut line, &mut len);
    }
    out
}

/// Push the accumulated `line` into `out` and reset it (no-op when empty).
fn flush_line(out: &mut Vec<String>, line: &mut Vec<String>, len: &mut usize) {
    if !line.is_empty() {
        out.push(line.join(" "));
        line.clear();
        *len = 0;
    }
}

/// Split `text` into hard chunks of at most `width` display columns, breaking
/// on grapheme-cluster boundaries (a cluster wider than `width` is kept whole).
fn chunk_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for g in text.graphemes(true) {
        let w = g.width();
        if !cur.is_empty() && cur_w + w > width {
            out.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push_str(g);
        cur_w += w;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Style inline markdown within an already-wrapped segment: `` `code` ``
/// (cyan), `**bold**` / `__bold__`, `*italic*` / `_italic_`, `~~strike~~`, and
/// `[text](url)` links (rendered as just `text`, cyan and underlined).
///
/// Single-pass and deterministic, matching markers in precedence order — code,
/// then bold, then strikethrough, then italic, then links — so an unbalanced
/// marker falls back to literal text.
fn inline_line(seg: &str, base: Style) -> Line<'static> {
    let chars: Vec<char> = seg.chars().collect();
    let n = chars.len();

    let mut spans: Vec<Span> = Vec::new();
    let mut cur = String::new();
    let flush = |spans: &mut Vec<Span>, cur: &mut String| {
        if !cur.is_empty() {
            spans.push(Span::styled(std::mem::take(cur), base));
        }
    };

    let mut i = 0;
    while i < n {
        if chars[i] == '`'
            && let Some(j) = find_pair(&chars, i, '`')
        {
            flush(&mut spans, &mut cur);
            spans.push(Span::styled(
                chars[i + 1..j].iter().collect::<String>(),
                Style::default().fg(Color::Cyan),
            ));
            i = j + 1;
            continue;
        }
        if i + 1 < n
            && chars[i] == chars[i + 1]
            && (chars[i] == '*' || chars[i] == '_')
            && let Some(j) = find_double(&chars, i + 2, chars[i])
        {
            flush(&mut spans, &mut cur);
            spans.push(Span::styled(
                chars[i + 2..j].iter().collect::<String>(),
                base.add_modifier(Modifier::BOLD),
            ));
            i = j + 2;
            continue;
        }
        if i + 1 < n
            && chars[i] == '~'
            && chars[i + 1] == '~'
            && let Some(j) = find_double(&chars, i + 2, '~')
        {
            flush(&mut spans, &mut cur);
            spans.push(Span::styled(
                chars[i + 2..j].iter().collect::<String>(),
                base.add_modifier(Modifier::CROSSED_OUT),
            ));
            i = j + 2;
            continue;
        }
        if (chars[i] == '*' || chars[i] == '_')
            && italic_open_ok(&chars, i)
            && let Some(j) = find_italic_end(&chars, i)
        {
            flush(&mut spans, &mut cur);
            spans.push(Span::styled(
                chars[i + 1..j].iter().collect::<String>(),
                base.add_modifier(Modifier::ITALIC),
            ));
            i = j + 1;
            continue;
        }
        if chars[i] == '['
            && let Some((j, k)) = find_link(&chars, i)
        {
            flush(&mut spans, &mut cur);
            spans.push(Span::styled(
                chars[i + 1..j].iter().collect::<String>(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            i = k + 1;
            continue;
        }
        cur.push(chars[i]);
        i += 1;
    }
    flush(&mut spans, &mut cur);

    Line::from(spans)
}

/// Index of the next single `marker` after `open`, if any.
fn find_pair(chars: &[char], open: usize, marker: char) -> Option<usize> {
    chars[open + 1..]
        .iter()
        .position(|&c| c == marker)
        .map(|offset| open + 1 + offset)
}

/// Index of the next `marker` doubled (`**`/`__`/`~~`) on or after `from`.
fn find_double(chars: &[char], from: usize, marker: char) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == marker && chars[i + 1] == marker {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Whether `chars[i]` may open an italic span. `*` always may; `_` may only
/// when it is not wedged between two word characters (`snake_case` stays
/// literal).
fn italic_open_ok(chars: &[char], i: usize) -> bool {
    if chars[i] == '*' {
        return true;
    }
    !(i > 0 && i + 1 < chars.len() && is_word_char(chars[i - 1]) && is_word_char(chars[i + 1]))
}

/// Index of the italic span's closing marker, or `None`. The content must be a
/// non-empty, non-space run; an underscore closer that is itself intraword is
/// rejected too.
fn find_italic_end(chars: &[char], open: usize) -> Option<usize> {
    let marker = chars[open];
    let mut j = open + 1;
    while j < chars.len() {
        if chars[j] == marker {
            let content = &chars[open + 1..j];
            let non_space = !content.is_empty() && content.iter().all(|&c| !c.is_whitespace());
            let closing_ok = marker != '_'
                || !(j + 1 < chars.len()
                    && is_word_char(chars[j - 1])
                    && is_word_char(chars[j + 1]));
            if non_space && closing_ok {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

/// The `]` and `)` positions bounding a `[text](url)` link, or `None`.
fn find_link(chars: &[char], open: usize) -> Option<(usize, usize)> {
    let mut j = open + 1;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    if j == chars.len() || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' {
        k += 1;
    }
    (k < chars.len() && j > open + 1).then_some((j, k))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// --- input ---

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let text = app.input();
    let cursor = app.input_cursor();
    // Body width: the panel's two side borders plus the `> ` prompt.
    let body_width = area.width.saturating_sub(4) as usize;
    let scroll = super::input::scroll(text, cursor, body_width, app.input_scroll());

    let line = super::input::line(text, cursor, scroll, "> ", true);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray);
    frame.render_widget(Paragraph::new(line).block(block), area);
}

/// Render the slash-command autocomplete menu as a floating list anchored just
/// above the input bar (visible only while `app.command_options()` is `Some`).
fn render_command_menu(frame: &mut Frame, app: &App, area: Rect) {
    let Some((options, selected)) = app.command_options() else {
        return;
    };

    let content_width = options
        .iter()
        .map(|opt| opt.command.len() + opt.description.len() + 4)
        .max()
        .unwrap_or(0);
    let width = (content_width + 2).clamp(12, area.width as usize) as u16;
    let height = (options.len() + 2) as u16;

    // Anchor just above the input bar (the bottom 4 rows of the chat area).
    let input_top = area.y + area.height.saturating_sub(4);
    let y = input_top.saturating_sub(height);
    let menu_rect = Rect {
        x: area.x + 1,
        y,
        width: width.min(area.width.saturating_sub(2)),
        height,
    };

    let items: Vec<WidgetListItem> = options
        .iter()
        .map(|opt| {
            WidgetListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<12}", opt.command),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(opt.description, Style::default().fg(Color::Gray)),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Color::DarkGray)
                .title(" commands ")
                .title_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("» ");

    frame.render_widget(Clear, menu_rect);
    frame.render_stateful_widget(list, menu_rect, &mut state);
}

/// Render the resume picker as a centred modal over the chat: row 0 is "start
/// a fresh session", the rows below it are the employee's previous sessions,
/// newest first. Shown only while `app.session_picker()` is `Some`.
fn render_session_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some((sessions, selected)) = app.session_picker() else {
        return;
    };

    let rows = sessions.len() + 1;
    // Two border rows plus the hint row.
    let height = (rows as u16 + 3).min(area.height.saturating_sub(2));
    let width = 64.min(area.width.saturating_sub(2));
    let [_, vertical_center, _] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Fill(1),
        ratatui::layout::Constraint::Length(height),
        ratatui::layout::Constraint::Fill(1),
    ])
    .areas(area);
    let [_, popup, _] = ratatui::layout::Layout::horizontal([
        ratatui::layout::Constraint::Fill(1),
        ratatui::layout::Constraint::Length(width),
        ratatui::layout::Constraint::Fill(1),
    ])
    .areas(vertical_center);

    let mut items: Vec<WidgetListItem> = Vec::with_capacity(rows);
    for (index, label) in std::iter::once("Start a fresh session".to_string())
        .chain(
            sessions
                .iter()
                .map(|s| s.title.clone().unwrap_or_else(|| s.id.clone())),
        )
        .enumerate()
    {
        let fresh = index == 0;
        items.push(WidgetListItem::new(Line::from(vec![
            Span::styled(
                if fresh { "✚ " } else { "↺ " },
                Style::default().fg(if fresh { Color::Green } else { Color::Cyan }),
            ),
            Span::styled(label, Style::default().fg(Color::White)),
        ])));
    }

    let mut state = ListState::default();
    state.select(Some(selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Color::Cyan)
                .title(" previous sessions ")
                .title_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("» ");

    let hints = super::key_hint("↑/↓ move · Enter open · n fresh · Esc back", "")
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(Clear, popup);
    // The hint sits inside the border: render the list into the full popup, then
    // draw the hint over its last inner row (Clear would erase the border, so
    // the hint line simply overwrites the text area).
    frame.render_stateful_widget(list, popup, &mut state);
    let hint_rect = Rect {
        x: popup.x + 1,
        y: popup.y + popup.height.saturating_sub(2),
        width: popup.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(hints, hint_rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(role: TranscriptRole, text: &str) -> TranscriptLine {
        TranscriptLine {
            role,
            text: text.to_string(),
            meta: None,
        }
    }

    #[test]
    fn wrap_text_word_wraps_and_preserves_blank_lines() {
        let out = wrap_text("hello world foo\n\nbar", 8);
        assert_eq!(out, vec!["hello", "world", "foo", "", "bar"]);
    }

    #[test]
    fn wrap_text_hard_wraps_overlong_words() {
        let out = wrap_text("abcdefghij", 4);
        assert_eq!(out, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn chunk_lines_respects_utf8_boundaries() {
        // "é" is 2 bytes; chunking by chars must not split it.
        let out = chunk_lines("ééa", 2);
        assert_eq!(out, vec!["éé", "a"]);
    }

    #[test]
    fn heading_detection() {
        assert_eq!(heading_text("# Title"), Some("Title"));
        assert_eq!(heading_text("### Deep"), Some("Deep"));
        assert_eq!(heading_text("plain"), None);
        assert_eq!(heading_text("#NoSpace"), None);
    }

    #[test]
    fn inline_line_styles_bold_italic_and_strike() {
        let line = inline_line("a **bold** and *italic* and ~~gone~~", Style::default());
        let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(
            texts,
            vec!["a ", "bold", " and ", "italic", " and ", "gone"]
        );
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert!(line.spans[3].style.add_modifier.contains(Modifier::ITALIC));
        assert!(
            line.spans[5]
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
    }

    #[test]
    fn inline_line_styles_code() {
        let line = inline_line("run `cargo test` now", Style::default());
        let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(texts, vec!["run ", "cargo test", " now"]);
        assert_eq!(line.spans[1].style.fg, Some(Color::Cyan));
    }

    #[test]
    fn inline_line_renders_link_text_only() {
        let line = inline_line("see [the docs](https://example.com) here", Style::default());
        let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
        // The `(url)` is dropped; `text` is cyan and underlined.
        assert_eq!(texts, vec!["see ", "the docs", " here"]);
        assert_eq!(line.spans[1].style.fg, Some(Color::Cyan));
        assert!(
            line.spans[1]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn inline_line_leaves_intraword_underscore_literal() {
        let line = inline_line("snake_case stays", Style::default());
        let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(texts, vec!["snake_case stays"]);
    }

    #[test]
    fn inline_line_underscore_italics_when_flanked_by_space() {
        let line = inline_line("say _hello_ now", Style::default());
        let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(texts, vec!["say ", "hello", " now"]);
        assert!(line.spans[1].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn inline_line_unbalanced_markers_fall_back_to_literal() {
        for input in ["**unclosed", "*half", "~~miss", "`oops", "and [link](never"] {
            let line = inline_line(input, Style::default());
            let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
            assert_eq!(texts, vec![input.to_string()], "input {input:?}");
        }
    }

    #[test]
    fn role_mapping_labels_assistant_with_employee_name() {
        let (tag, style, base) = role_mapping(TranscriptRole::Assistant, Some("writer"));
        assert_eq!(tag, "writer ");
        assert_eq!(style.fg, Some(Color::Green));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(base, Style::default());

        // Unknown / blank name falls back to a generic label.
        assert_eq!(
            role_mapping(TranscriptRole::Assistant, None).0,
            "assistant "
        );
        assert_eq!(
            role_mapping(TranscriptRole::Assistant, Some("  ")).0,
            "assistant "
        );
    }

    #[test]
    fn role_mapping_expands_error_tag() {
        let (tag, _style, base) = role_mapping(TranscriptRole::Error, None);
        assert_eq!(tag, "error ");
        assert_eq!(base.fg, Some(Color::Red));
    }

    #[test]
    fn transcript_lines_insert_spacers_between_messages() {
        let entries = [
            entry(TranscriptRole::User, "hi"),
            entry(TranscriptRole::Assistant, "hello"),
        ];
        // user block + spacer + assistant block.
        let lines = transcript_lines(&entries, Some("writer"), "", 80);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].spans.is_empty());

        // The first message never gets a leading spacer.
        let one = transcript_lines(&entries[..1], Some("writer"), "", 80);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn transcript_lines_inserts_spacer_before_streaming() {
        let entries = [entry(TranscriptRole::User, "hi")];

        // user block + spacer + streaming block.
        let lines = transcript_lines(&entries, None, "still going", 80);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].spans.is_empty());

        // Empty streaming text adds nothing (no trailing spacer).
        let none = transcript_lines(&entries, None, "", 80);
        assert_eq!(none.len(), 1);

        // Streaming with an empty transcript starts immediately.
        let empty = transcript_lines(&[], None, "streaming...", 80);
        assert_eq!(empty.len(), 1);
        assert!(!empty[0].spans.is_empty());
    }

    #[test]
    fn wrap_text_measures_wide_glyphs_in_columns() {
        // "中" is 2 columns, "a"/"b" 1: six columns of wide text wrap at 4.
        let out = wrap_text("ab 中中中", 4);
        assert_eq!(out, vec!["ab", "中中", "中"]);
    }

    #[test]
    fn chunk_lines_breaks_on_display_width() {
        // Three two-column glyphs in a 4-column budget → 2 + 1.
        let out = chunk_lines("中中中", 4);
        assert_eq!(out, vec!["中中", "中"]);

        // Emoji are two columns wide.
        let out = chunk_lines("😀😀😀", 4);
        assert_eq!(out, vec!["😀😀", "😀"]);
    }

    #[test]
    fn chunk_lines_keeps_combining_marks_attached() {
        // "é" (e + U+0301) is one cluster, one column: chunking by columns must
        // not split the base from its combining mark.
        let decomposed = "e\u{301}e\u{301}e\u{301}";
        let out = chunk_lines(decomposed, 2);
        assert_eq!(out, vec!["e\u{301}e\u{301}", "e\u{301}"]);
    }

    fn span_texts(line: &Line) -> Vec<String> {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn list_item_parses_bullets_and_numbers() {
        let item = list_item("- first").unwrap();
        assert_eq!(item.marker, "- ");
        assert_eq!(item.content, "first");
        assert_eq!(item.width, 2);

        let item = list_item("* star").unwrap();
        assert_eq!(item.marker, "* ");
        assert_eq!(item.content, "star");

        let item = list_item("+ plus").unwrap();
        assert_eq!(item.marker, "+ ");

        let item = list_item("10. step").unwrap();
        assert_eq!(item.marker, "10. ");
        assert_eq!(item.content, "step");
        assert_eq!(item.width, 4);

        let item = list_item("7) step").unwrap();
        assert_eq!(item.marker, "7) ");
        assert_eq!(item.width, 3);
    }

    #[test]
    fn list_item_rejects_rules_and_prose_constructs() {
        for input in [
            "---",
            "***",
            "*emphasis*",
            "snake_case",
            "https://example.com",
        ] {
            assert!(
                list_item(input).is_none(),
                "misread {input:?} as a list item"
            );
        }
        // Ordered markers need trailing whitespace after the delimiter.
        assert!(list_item("1.2").is_none());
        assert!(list_item("1)item").is_none());
        // A delimiter without a separator is not a bullet.
        assert!(list_item("-").is_none());
        assert!(list_item("1.").is_none());
    }

    /// Code blocks render inside a drawn box: an opening rule (with the
    /// language tag when the fence names one), `│ `-prefixed content lines, and
    /// a closing rule. The raw fence lines never reach the transcript.
    #[test]
    fn code_blocks_render_inside_a_boxed_rule() {
        let lines = markdown_lines("```rust\nlet x = 1;\n```", 20, Style::default());
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();

        assert_eq!(text.len(), 4, "{text:?}");
        assert!(text[0].contains("rust"), "language tag: {text:?}");
        assert!(text[1].starts_with('┌'), "opening rule: {text:?}");
        assert!(
            text[2].contains("│ ") && text[2].contains("let x = 1;"),
            "{text:?}"
        );
        assert!(text[3].starts_with('└'), "closing rule: {text:?}");
    }

    /// Headings are bold; levels 1–2 are also underlined so structure survives
    /// a scroll, and the `#` markers themselves are stripped.
    #[test]
    fn headings_bold_and_top_level_underlined() {
        let lines = markdown_lines("# Title\n### Detail", 30, Style::default());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "Title");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(lines[1].spans[0].content, "Detail");
        assert!(
            !lines[1].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    /// Blockquotes carry a cyan `▌ ` gutter on every wrapped line, so the
    /// quote's extent is visible.
    #[test]
    fn blockquotes_get_a_gutter_bar() {
        let lines = markdown_lines("> quoted thought", 30, Style::default());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "▌ ");
        assert!(lines[0].spans.iter().any(|s| s.content.contains("quoted")));
    }

    /// A line of only dashes/underscores/asterisks is a horizontal rule, not
    /// prose — rendered as a full-width dim line.
    #[test]
    fn horizontal_rules_render_as_a_line() {
        let lines = markdown_lines("---", 12, Style::default());
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "─".repeat(12));
    }

    /// Table rows render as gutter-separated columns: bold cells for data rows,
    /// dim for the `---` divider, all inside the card's width.
    #[test]
    fn tables_render_with_gutters_and_a_dim_divider() {
        let md = "| name | qty |\n|---|---|\n| bolt | 4 |";
        let lines = markdown_lines(md, 24, Style::default());
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();

        assert_eq!(lines.len(), 3, "{text:?}");
        assert!(
            text[0].contains("name") && text[0].contains("qty"),
            "{text:?}"
        );
        assert!(text[1].contains('-'), "divider row: {text:?}");
        assert!(
            text[2].contains("bolt") && text[2].contains("4"),
            "{text:?}"
        );
    }

    /// A message whose code block never closes (a truncated stream) still gets
    /// its closing rule — the transcript never ends inside an open box.
    #[test]
    fn an_unclosed_code_block_still_closes() {
        let lines = markdown_lines("```py\nprint(1)", 20, Style::default());
        let last = lines.last().unwrap();
        let text: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with('└'), "{text:?}");
    }

    #[test]
    fn markdown_lines_unordered_item_wraps_with_hanging_indent() {
        let lines = markdown_lines("- one two three", 8, Style::default());
        let texts: Vec<Vec<String>> = lines.iter().map(span_texts).collect();
        assert_eq!(
            texts,
            vec![
                vec!["- ".to_string(), "one".to_string()],
                vec!["  ".to_string(), "two".to_string()],
                vec!["  ".to_string(), "three".to_string()],
            ]
        );
        // The bullet is cyan; the continuation indent is unstyled.
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(lines[1].spans[0].style, Style::default());
    }

    #[test]
    fn markdown_lines_ordered_items_indent_by_marker_width() {
        let lines = markdown_lines("1. alpha beta\n2. gamma delta", 12, Style::default());
        let texts: Vec<Vec<String>> = lines.iter().map(span_texts).collect();
        assert_eq!(
            texts,
            vec![
                vec!["1. ".to_string(), "alpha".to_string()],
                vec!["   ".to_string(), "beta".to_string()],
                vec!["2. ".to_string(), "gamma".to_string()],
                vec!["   ".to_string(), "delta".to_string()],
            ]
        );
    }

    #[test]
    fn markdown_lines_double_digit_marker_indents_by_its_width() {
        let lines = markdown_lines("10. alpha beta", 12, Style::default());
        let texts: Vec<Vec<String>> = lines.iter().map(span_texts).collect();
        // `10. ` is four columns wide, so continuations indent by four.
        assert_eq!(
            texts,
            vec![
                vec!["10. ".to_string(), "alpha".to_string()],
                vec!["    ".to_string(), "beta".to_string()],
            ]
        );
    }

    #[test]
    fn markdown_lines_continuation_line_joins_list_item() {
        let lines = markdown_lines("- one\n continued words here", 30, Style::default());
        let texts: Vec<Vec<String>> = lines.iter().map(span_texts).collect();
        assert_eq!(
            texts,
            vec![
                vec!["- ".to_string(), "one".to_string()],
                vec!["  ".to_string(), "continued words here".to_string()],
            ]
        );
    }

    #[test]
    fn markdown_lines_blank_line_ends_list() {
        let lines = markdown_lines("- one\n\nnext prose", 20, Style::default());
        let texts: Vec<Vec<String>> = lines.iter().map(span_texts).collect();
        assert_eq!(
            texts,
            vec![
                vec!["- ".to_string(), "one".to_string()],
                vec![],
                vec!["next prose".to_string()],
            ]
        );
    }
}
