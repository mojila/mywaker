//! The chat panel — a scrollable, markdown-aware transcript, a slash-command
//! autocomplete menu, and a cursor-editing input prompt.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, TranscriptLine, TranscriptRole};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [transcript_area, input_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(area);

    render_transcript(frame, app, transcript_area);
    render_input(frame, app, input_area);
    render_command_menu(frame, app, area);
}

// --- transcript ---

fn render_transcript(frame: &mut Frame, app: &App, area: Rect) {
    let name = app.chat_employee_name().unwrap_or("chat");
    let inner_width = area.width.saturating_sub(2) as usize;
    let inner_height = area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();
    for entry in app.transcript() {
        transcript_block(&mut lines, entry, inner_width);
    }
    let streaming = app.streaming_text();
    if !streaming.is_empty() {
        tagged_block(
            &mut lines,
            "◔ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            streaming,
            Style::default().fg(Color::Yellow),
            None,
            inner_width,
        );
    }
    let reasoning = app.reasoning_text();
    if !reasoning.is_empty() {
        tagged_block(
            &mut lines,
            "… ",
            Style::default().fg(Color::DarkGray),
            reasoning,
            Style::default().fg(Color::DarkGray),
            None,
            inner_width,
        );
    }

    // Scroll to the visible window (stick to the live tail unless the user has
    // paged up to read back through history).
    let scroll = app.chat_scroll();
    let max = lines.len().saturating_sub(inner_height);
    let start = max.saturating_sub(scroll).min(max);
    let visible: Vec<Line> = lines.into_iter().skip(start).take(inner_height).collect();

    let title = match app.chat_employee_status() {
        Some(status) => format!(" chat · {name} · {} ", status.label()),
        None => format!(" chat · {name} "),
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

/// Render one transcript entry into wrapped, styled display lines.
fn transcript_block(lines: &mut Vec<Line>, entry: &TranscriptLine, width: usize) {
    let (tag, tag_style, base) = role_mapping(entry.role);
    tagged_block(
        lines,
        tag,
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

fn role_mapping(role: TranscriptRole) -> (&'static str, Style, Style) {
    match role {
        TranscriptRole::User => (
            "you ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Style::default(),
        ),
        TranscriptRole::Assistant => (
            "wkr ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            Style::default(),
        ),
        TranscriptRole::Error => (
            "err ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Red),
        ),
        TranscriptRole::Info => ("", Style::default(), Style::default().fg(Color::DarkGray)),
    }
}

/// Lightweight markdown: fenced code blocks (cyan/dim), headings (bold),
/// blockquotes (dim), and inline `` `code` `` (cyan). Everything word-wraps to
/// `width`; code lines are chunked without collapsing indentation.
fn markdown_lines(text: &str, width: usize, base: Style) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code = false;

    for raw in text.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            out.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }
        if in_code {
            for seg in chunk_lines(raw, width) {
                out.push(Line::from(Span::styled(
                    seg,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
                )));
            }
            continue;
        }
        if let Some(heading) = heading_text(raw) {
            for seg in wrap_text(heading, width) {
                out.push(Line::from(Span::styled(
                    seg,
                    base.add_modifier(Modifier::BOLD),
                )));
            }
            continue;
        }
        let (style, body) = if raw.trim_start().starts_with('>') {
            (
                base.add_modifier(Modifier::DIM),
                raw.trim_start()
                    .strip_prefix('>')
                    .unwrap_or(raw)
                    .trim_start(),
            )
        } else {
            (base, raw)
        };
        for seg in wrap_text(body, width) {
            out.push(inline_code_line(&seg, style));
        }
    }

    out
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

/// Style inline `` `code` `` spans (odd segments) in cyan.
fn inline_code_line(seg: &str, base: Style) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut toggle = false;
    for part in seg.split('`') {
        let style = if toggle {
            Style::default().fg(Color::Cyan)
        } else {
            base
        };
        spans.push(Span::styled(part.to_string(), style));
        toggle = !toggle;
    }
    Line::from(spans)
}

// --- input ---

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let text = app.input();
    let cursor = app.input_cursor();
    let byte = text
        .grapheme_indices(true)
        .nth(cursor)
        .map(|(i, _)| i)
        .unwrap_or(text.len());

    // Body width: the panel's two side borders plus the `> ` prompt.
    let body_width = area.width.saturating_sub(4) as usize;

    // Effective scroll keeps the cursor visible (mirrors `App`'s own follow, so
    // the window is correct even on a render that precedes the next edit).
    let cursor_col = text[..byte].width();
    let mut scroll = app.input_scroll();
    if cursor_col < scroll {
        scroll = cursor_col;
    }
    if body_width > 0 && cursor_col >= scroll + body_width {
        scroll = cursor_col + 1 - body_width;
    }
    // Mirror `App`'s shrink retraction: pull the scroll back to the minimal
    // in-view value so a shortened input doesn't render as a bare cursor block.
    if body_width > 0 {
        scroll = scroll.min(cursor_col.saturating_add(1).saturating_sub(body_width));
    }

    let (before, _) = trim_leading_columns(&text[..byte], scroll);
    let after = &text[byte..];

    let line = Line::from(vec![
        Span::styled("> ", Color::Cyan),
        Span::raw(before),
        Span::styled("█", Color::Cyan),
        Span::raw(after.to_string()),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Color::DarkGray);
    frame.render_widget(Paragraph::new(line).block(block), area);
}

/// Trim the leading `columns` display columns from `text`, stopping at a
/// grapheme-cluster boundary. Returns the trimmed remainder and the columns
/// actually removed (≤ `columns`).
fn trim_leading_columns(text: &str, columns: usize) -> (String, usize) {
    if columns == 0 {
        return (text.to_string(), 0);
    }
    let mut removed = 0;
    for (idx, g) in text.grapheme_indices(true) {
        let w = g.width();
        if removed + w > columns {
            return (text[idx..].to_string(), removed);
        }
        removed += w;
    }
    (String::new(), removed)
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

    let items: Vec<ListItem> = options
        .iter()
        .map(|opt| {
            ListItem::new(Line::from(vec![
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn inline_code_splits_on_backticks() {
        let line = inline_code_line("see `let x = 1;` here", Style::default());
        // plain / code / plain(empty) / plain — at least 3 spans.
        assert!(line.spans.len() >= 3);
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

    #[test]
    fn trim_leading_columns_stops_at_grapheme_boundary() {
        assert_eq!(trim_leading_columns("abc", 1), ("bc".to_string(), 1));
        // A 2-column emoji can't be half-trimmed: trimming one column leaves it.
        assert_eq!(trim_leading_columns("😀ab", 1), ("😀ab".to_string(), 0));
        assert_eq!(trim_leading_columns("😀ab", 2), ("ab".to_string(), 2));
        assert_eq!(trim_leading_columns("abc", 9), (String::new(), 3));
    }
}
