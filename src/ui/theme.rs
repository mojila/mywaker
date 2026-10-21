//! The colour roles — the only place a colour is named (`docs/keymap.md` §2.2
//! rule 3, §2.5).
//!
//! Every surface resolves its colours through one of these, so a scheme swap and
//! a `NO_COLOR` build are one change here rather than a sweep of literals. The
//! values are **ANSI names only**: no `Color::Rgb`, no `Color::Indexed` (§2.2
//! rules 1–2), because a hex value has no meaning on a 1-bit terminal and an
//! index is terminal-dependent.
//!
//! §2.1's table is the contract. `selected_bg`/`selected_fg` are the pair that
//! fixes K8: today's list highlight is `bg(DarkGray)` under the item's own
//! foreground, which is as low as 3.18:1 where that foreground is `Gray`. The
//! pair here is Blue-on-White (9.40:1).

use ratatui::style::Color;

/// The one brand/active colour: titles, focused borders, the nameplate.
pub(crate) const ACCENT: Color = Color::Cyan;

/// An unfocused panel's border.
pub(crate) const BORDER: Color = Color::DarkGray;

/// Body text.
pub(crate) const TEXT: Color = Color::Gray;

/// Emphasised body text, the selected name.
pub(crate) const TEXT_BRIGHT: Color = Color::White;

/// Secondary text: roles, placeholders, disabled.
pub(crate) const MUTED: Color = Color::DarkGray;

/// The selected row/card background.
pub(crate) const SELECTED_BG: Color = Color::Blue;

/// Text drawn **on** [`SELECTED_BG`].
pub(crate) const SELECTED_FG: Color = Color::White;

/// Success, connected, done.
pub(crate) const OK: Color = Color::Green;

/// In-progress caution, paused, retrying.
pub(crate) const WARN: Color = Color::Yellow;

/// Failure, disconnected, error.
pub(crate) const ERR: Color = Color::Red;

/// Neutral notice, delegating, trace.
pub(crate) const INFO: Color = Color::Magenta;

/// The ` mywaker ` title chip's foreground.
pub(crate) const ON_ACCENT: Color = Color::Black;

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.2 rules 1–2: no truecolour and no indexed values anywhere in the
    /// palette. A role added later must not smuggle one in.
    #[test]
    fn roles_are_ansi_names_only() {
        let roles = [
            ACCENT,
            BORDER,
            TEXT,
            TEXT_BRIGHT,
            MUTED,
            SELECTED_BG,
            SELECTED_FG,
            OK,
            WARN,
            ERR,
            INFO,
            ON_ACCENT,
        ];
        for role in roles {
            assert!(
                matches!(
                    role,
                    Color::Black
                        | Color::Red
                        | Color::Green
                        | Color::Yellow
                        | Color::Blue
                        | Color::Magenta
                        | Color::Cyan
                        | Color::Gray
                        | Color::DarkGray
                        | Color::White
                ),
                "{role:?} is not an ANSI name"
            );
        }
    }

    /// The selection pair must be distinguishable: a background is only ever
    /// drawn for `selected_bg` (§2.2 rule 4), and its foreground must not be the
    /// same role.
    #[test]
    fn selection_pair_is_a_real_contrast() {
        assert_ne!(SELECTED_BG, SELECTED_FG);
    }
}
