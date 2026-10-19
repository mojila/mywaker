//! State-driven animated ASCII robot avatar (no external art dependencies).

use ratatui::text::Line;

use crate::ui::status;

/// Aggregate team mood the robot reflects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotState {
    Idle,
    Thinking,
    Working,
    Error,
}

/// The full 9-row avatar, kept for wide/tall use and for the render tests that
/// pin its animation contract. The sidebar always uses [`render_mini`].
#[allow(dead_code)]
pub fn render(state: RobotState, frame: u64) -> Vec<Line<'static>> {
    let spinner = status::spinner(frame);

    let antenna = match state {
        RobotState::Error => "✕",
        RobotState::Idle => {
            if frame % 20 < 10 {
                "●"
            } else {
                "·"
            }
        }
        RobotState::Thinking | RobotState::Working => spinner,
    };

    let eyes = match state {
        RobotState::Idle => "●   ●",
        RobotState::Thinking => {
            if frame % 8 < 4 {
                "◉   ◉"
            } else {
                "◍   ◍"
            }
        }
        RobotState::Working => "▶   ◀",
        RobotState::Error => "✕   ✕",
    };

    let mouth = match state {
        RobotState::Thinking => spinner,
        RobotState::Error => "⋁",
        _ => "▂",
    };

    let lines = [
        format!("        {antenna}         "),
        "   ▄▄▄▄▄▄▄▄▄▄▄▄   ".to_string(),
        "   █            █   ".to_string(),
        format!("   █    {eyes}    █   "),
        format!("   █      {mouth}      █   "),
        "   █▄▄▄▄▄▄▄▄▄▄▄▄▄▄█   ".to_string(),
        "   ┃   MYWAKER   ┃   ".to_string(),
        "  ┌┴──────────────┴┐  ".to_string(),
        "  └────────────────┘  ".to_string(),
    ];

    lines.into_iter().map(Line::raw).collect()
}

/// The mini avatar: the four rows the sidebar allots it (see
/// `docs/acceptance-sidebar.md` §2.1).
///
/// Row 1 carries the antenna and eyes — the only rows the animation assertions
/// read — row 2 the mouth and face frame, row 3 the `MYWAKER` nameplate (the
/// sidebar's P0 marker), row 4 the shoulders. It keeps [`render`]'s animation
/// contract: the `Idle` antenna blinks on a 20-tick period, and `Thinking` /
/// `Working` advance the Braille spinner every tick.
///
/// Width is the caller's problem — the sidebar passes its inner width and
/// truncates the nameplate itself, because `Line` cannot know the viewport.
pub fn render_mini(state: RobotState, frame: u64) -> Vec<Line<'static>> {
    let spinner = status::spinner(frame);

    let antenna = match state {
        RobotState::Error => "✕",
        RobotState::Idle => {
            if frame % 20 < 10 {
                "●"
            } else {
                "·"
            }
        }
        RobotState::Thinking | RobotState::Working => spinner,
    };

    let eyes = match state {
        RobotState::Idle => "●   ●",
        RobotState::Thinking => {
            if frame % 8 < 4 {
                "◉   ◉"
            } else {
                "◍   ◍"
            }
        }
        RobotState::Working => "▶   ◀",
        RobotState::Error => "✕   ✕",
    };

    let mouth = match state {
        RobotState::Thinking => spinner,
        RobotState::Error => "⋁",
        _ => "▂",
    };

    let lines = [
        format!("  {antenna}   {eyes}"),
        format!("  ┌─{mouth}─┐"),
        format!("  {MINI_NAMEPLATE}"),
        "  └─────┘".to_string(),
    ];

    lines.into_iter().map(Line::raw).collect()
}

/// The nameplate text [`render_mini`] draws on its third row — the sidebar's
/// P0 marker, exported so the sidebar and the render tests agree on the literal
/// instead of each hardcoding `"MYWAKER"`.
pub const MINI_NAMEPLATE: &str = "MYWAKER";

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a line back into its visible text (for assertions).
    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// The first non-whitespace character of a rendered line.
    fn first_char(line: &Line) -> char {
        text(line).trim().chars().next().unwrap_or(' ')
    }

    /// The antenna glyph of the **9-row** avatar (first char of its first line).
    fn antenna(state: RobotState, frame: u64) -> char {
        first_char(&render(state, frame)[0])
    }

    /// The antenna glyph of the **mini** avatar the sidebar actually draws.
    ///
    /// A second helper, not a shared one: the two avatars are separate
    /// functions, and a single helper would only ever pin one of them. Before
    /// this existed the blink period was asserted against `render` alone, so
    /// freezing `render_mini` left the whole suite green while the sidebar's
    /// avatar stopped animating.
    fn mini_antenna(state: RobotState, frame: u64) -> char {
        first_char(&render_mini(state, frame)[0])
    }

    #[test]
    fn every_state_renders_nine_lines() {
        for state in [
            RobotState::Idle,
            RobotState::Thinking,
            RobotState::Working,
            RobotState::Error,
        ] {
            for frame in [0u64, 1, 7, 10, 19, 20, 12345] {
                assert_eq!(
                    render(state, frame).len(),
                    9,
                    "state {state:?} frame {frame}"
                );
            }
        }
    }

    #[test]
    fn idle_antenna_blinks_on_a_20_tick_period() {
        // Frames 0..=9 light the antenna; frames 10..=19 dim it.
        assert_eq!(antenna(RobotState::Idle, 0), '●');
        assert_eq!(antenna(RobotState::Idle, 9), '●');
        assert_eq!(antenna(RobotState::Idle, 10), '·');
        assert_eq!(antenna(RobotState::Idle, 19), '·');
        assert_eq!(antenna(RobotState::Idle, 20), '●');
    }

    #[test]
    fn thinking_and_working_antenna_spins_with_frame() {
        // The Braille spinner advances every tick.
        assert_ne!(
            antenna(RobotState::Thinking, 0),
            antenna(RobotState::Thinking, 1)
        );
        assert_ne!(
            antenna(RobotState::Working, 0),
            antenna(RobotState::Working, 1)
        );
        // …and wraps back around every 10 frames.
        assert_eq!(
            antenna(RobotState::Thinking, 0),
            antenna(RobotState::Thinking, 10)
        );
    }

    #[test]
    fn error_state_is_stable() {
        // Error is not animated: antenna stays ✕ regardless of frame.
        assert_eq!(antenna(RobotState::Error, 0), '✕');
        assert_eq!(antenna(RobotState::Error, 50), '✕');
        // Error eyes are also fixed ✕.
        assert!(text(&render(RobotState::Error, 7)[3]).contains('✕'));
    }

    /// §6 / §2.1: the mini avatar is exactly the four rows the sidebar allots it,
    /// and its third row keeps the `MYWAKER` nameplate that A10 and A5 read.
    ///
    /// Without this, `render_mini` could grow or lose a row and the sidebar's P0
    /// budget (§2: 4 content lines) would silently disagree with what is drawn.
    #[test]
    fn mini_avatar_is_four_rows_and_keeps_the_nameplate() {
        for state in [
            RobotState::Idle,
            RobotState::Thinking,
            RobotState::Working,
            RobotState::Error,
        ] {
            for frame in [0u64, 1, 7, 10, 19, 20, 12345] {
                let lines = render_mini(state, frame);
                assert_eq!(
                    lines.len(),
                    4,
                    "the mini avatar is four rows (state {state:?} frame {frame})"
                );
                assert!(
                    text(&lines[2]).contains(MINI_NAMEPLATE),
                    "row 3 is the nameplate (state {state:?} frame {frame})"
                );
            }
        }
        // The literal the sidebar truncates is the one asserted above.
        assert_eq!(MINI_NAMEPLATE, "MYWAKER");
    }

    /// §6 / A9: the mini avatar animates with the frame, on the same contract as
    /// the 9-row `render` — the `Idle` antenna blinks on the 20-tick period and
    /// `Thinking`/`Working` advance the spinner every tick.
    ///
    /// This is the assertion the sidebar's own avatar depends on: `render_mini`
    /// is what `ui::sidebar` draws, and until this test existed its animation was
    /// only pinned for `render`, the avatar the sidebar does not use.
    #[test]
    fn mini_avatar_animates_with_frame() {
        // Idle: lit for frames 0..=9, dim for 10..=19, lit again at 20.
        assert_eq!(mini_antenna(RobotState::Idle, 0), '●');
        assert_eq!(mini_antenna(RobotState::Idle, 9), '●');
        assert_eq!(mini_antenna(RobotState::Idle, 10), '·');
        assert_eq!(mini_antenna(RobotState::Idle, 19), '·');
        assert_eq!(mini_antenna(RobotState::Idle, 20), '●');
        assert_eq!(mini_antenna(RobotState::Idle, 21), '●');

        // Thinking and Working spin every tick and wrap every 10 frames.
        for state in [RobotState::Thinking, RobotState::Working] {
            assert_ne!(
                mini_antenna(state, 0),
                mini_antenna(state, 1),
                "{state:?} must advance its spinner each tick"
            );
            assert_eq!(
                mini_antenna(state, 0),
                mini_antenna(state, 10),
                "{state:?} wraps every 10 frames"
            );
        }

        // Error is static.
        assert_eq!(mini_antenna(RobotState::Error, 0), '✕');
        assert_eq!(mini_antenna(RobotState::Error, 50), '✕');

        // The whole avatar — not just the antenna — changes between ticks, which
        // is what A9 reads on the panel.
        assert_ne!(
            render_mini(RobotState::Thinking, 0),
            render_mini(RobotState::Thinking, 1),
            "consecutive frames differ"
        );
    }
}
