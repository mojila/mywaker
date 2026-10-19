//! The one-line text editor view, shared by the chat input bar and the roster's
//! prompt bar.
//!
//! Both surfaces show the same thing — a single line of text with a cursor, in a
//! horizontally scrolled window when the text overflows — so the windowing math
//! lives here once. The callers supply only their own prefix and framing, which
//! keeps the two cursors from drifting apart (one showing a character the other
//! would have scrolled past).

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The cursor glyph: a solid block, so the caret is visible without relying on
/// terminal cursor placement (which ratatui does not drive here).
pub(crate) const CURSOR: &str = "█";

/// The horizontal window of an input line: what is drawn left of the cursor
/// (already trimmed to the scroll offset) and what is drawn from the cursor on.
pub(crate) struct Window<'a> {
    /// The text before the cursor that survives the scroll, trimmed to a
    /// grapheme boundary.
    pub before: String,
    /// The text at and after the cursor.
    pub after: &'a str,
}

/// Byte offset of grapheme `cursor` in `text` (the end of the string when the
/// cursor sits past the last cluster).
pub(crate) fn byte_offset(text: &str, cursor: usize) -> usize {
    text.grapheme_indices(true)
        .nth(cursor)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// The scroll offset that keeps the cursor inside a `body_width`-column window,
/// given the offset the app last recorded.
///
/// The app keeps the offset up to date as the text changes
/// ([`App::follow_input_cursor`](crate::app::App)), but a render can land between
/// an edit and that update, so this re-derives the same window from the text and
/// cursor rather than trusting the stored value.
pub(crate) fn scroll(text: &str, cursor: usize, body_width: usize, scroll: usize) -> usize {
    if body_width == 0 {
        return 0;
    }
    let cursor_col = text[..byte_offset(text, cursor)].width();
    let mut scroll = scroll;
    if cursor_col < scroll {
        scroll = cursor_col;
    }
    if cursor_col >= scroll + body_width {
        scroll = cursor_col + 1 - body_width;
    }
    // Mirror the app's shrink retraction: pull the scroll back to the minimal
    // in-view value, so a shortened input does not render as a bare cursor.
    scroll.min(cursor_col.saturating_add(1).saturating_sub(body_width))
}

/// The windowed body for a cursor at `cursor` and a scroll offset in columns.
pub(crate) fn window(text: &str, cursor: usize, scroll: usize) -> Window<'_> {
    let byte = byte_offset(text, cursor);
    let (before, _) = trim_leading_columns(&text[..byte], scroll);
    Window {
        before,
        after: &text[byte..],
    }
}

/// The drawn line: `prefix` in cyan, the visible text, the cursor block, then the
/// rest of the text.
///
/// `show_cursor` is false while the bar is not focused — an unfocused field has
/// no caret, which is what tells the user where the keyboard is going.
pub(crate) fn line<'a>(
    text: &'a str,
    cursor: usize,
    scroll: usize,
    prefix: &'a str,
    show_cursor: bool,
) -> Line<'a> {
    let window = window(text, cursor, scroll);
    let mut spans = vec![Span::styled(prefix, Style::default().fg(Color::Cyan))];
    if !window.before.is_empty() {
        spans.push(Span::raw(window.before));
    }
    if show_cursor {
        spans.push(Span::styled(CURSOR, Style::default().fg(Color::Cyan)));
    }
    if !window.after.is_empty() {
        spans.push(Span::raw(window.after));
    }
    Line::from(spans)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_offset_walks_graphemes_not_bytes() {
        assert_eq!(byte_offset("abc", 0), 0);
        assert_eq!(byte_offset("abc", 2), 2);
        assert_eq!(byte_offset("abc", 9), 3, "past the end clamps to len");
        // "é" as one cluster is 2 bytes: the second cursor position is byte 2.
        assert_eq!(byte_offset("éa", 1), 2);
    }

    #[test]
    fn trim_leading_columns_stops_at_grapheme_boundary() {
        assert_eq!(trim_leading_columns("abc", 1), ("bc".to_string(), 1));
        // A 2-column emoji can't be half-trimmed: trimming one column leaves it.
        assert_eq!(trim_leading_columns("😀ab", 1), ("😀ab".to_string(), 0));
        assert_eq!(trim_leading_columns("😀ab", 2), ("ab".to_string(), 2));
        assert_eq!(trim_leading_columns("abc", 9), (String::new(), 3));
    }

    /// The window follows the cursor: it scrolls right when the cursor leaves the
    /// right edge and back left when it returns.
    #[test]
    fn scroll_keeps_the_cursor_inside_the_body() {
        // A cursor at column 0 never needs to scroll.
        assert_eq!(scroll("hello", 0, 4, 0), 0);
        // At the end of a 5-column text in a 4-column body the window starts at 2.
        assert_eq!(scroll("hello", 5, 4, 0), 2);
        // A cursor back inside the window retracts the scroll to the minimal
        // in-view value: column 1 needs none at all.
        assert_eq!(scroll("hello", 1, 4, 2), 0);
        // A cursor at column 5 keeps the window at 2 even when the stored offset
        // was stale.
        assert_eq!(scroll("hello", 5, 4, 0), 2);
        // A zero-width body has nowhere to scroll.
        assert_eq!(scroll("hello", 5, 0, 3), 0);
    }

    /// The drawn line is prefix, body, caret, rest — in that order, with the
    /// caret between the two halves of the text.
    #[test]
    fn line_splits_at_the_cursor() {
        let line = line("hello", 2, 0, "> ", true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "> he█llo");

        let spans: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(spans, vec!["> ", "he", CURSOR, "llo"]);
    }

    /// An unfocused field has no caret: the same text renders without the block.
    #[test]
    fn an_unfocused_line_has_no_cursor() {
        let line = line("hello", 2, 0, "> ", false);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "> hello");
    }

    /// An empty input still draws the prefix and the caret, so a focused empty
    /// field is visibly focused.
    #[test]
    fn an_empty_input_shows_the_prefix_and_caret() {
        let line = line("", 0, 0, "> ", true);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "> █");
    }

    /// The window is a slice of the text, not a copy of the whole line: scrolling
    /// past the left edge drops exactly the columns it has to.
    #[test]
    fn window_trims_the_text_left_of_the_scroll() {
        let window = window("abcdef", 6, 4);
        assert_eq!(window.before, "ef");
        assert_eq!(window.after, "", "the cursor is at the end");
    }
}
