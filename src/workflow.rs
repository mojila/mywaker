//! Employee-to-employee delegation: the shared hand-off protocol.
//!
//! Employees talk to each other by ending an assistant reply with lines of the
//! form `@<id> <instruction>`. The app scans each completed reply, routes each
//! directive to the named employee (creating their session on demand), and
//! awaits their result — which may itself contain further directives. This is
//! what turns a single "ask the planner" into a planner → researcher → writer →
//! reviewer → user chain (and any employee may delegate back to any other).

use crate::team::Team;

/// A single hand-off extracted from an employee's reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// Lower-cased target employee id, or the special targets `user` / `done` /
    /// `deliver` which mean "this is the final answer for the user".
    pub target: String,
    /// The instruction relayed to the target.
    pub message: String,
}

/// Parse every line-leading `@<id>[:] <text>` directive out of an assistant
/// reply. Only lines whose first non-whitespace character is `@` are treated as
/// directives, so an inline mention (e.g. "…ask @writer about…") is ignored.
pub fn parse_delegations(text: &str) -> Vec<Delegation> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix('@') else {
            continue;
        };

        // The target is the leading run of id characters (`a-z`, `A-Z`, `0-9`,
        // `_`, `-`); everything after an optional `:`/`：` separator and any
        // whitespace is the instruction.
        let split = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(rest.len());

        let target = &rest[..split];
        let message = rest[split..]
            .trim_start_matches(|c: char| c == ':' || c == '：' || c.is_whitespace())
            .trim()
            .to_string();

        if target.is_empty() {
            continue;
        }
        // `@user`, `@done`, `@deliver` carry the final answer; a bare mention
        // with no instruction is not a hand-off.
        if message.is_empty() && !is_final(&target.to_ascii_lowercase()) {
            continue;
        }

        out.push(Delegation {
            target: target.to_ascii_lowercase(),
            message,
        });
    }
    out
}

/// True when a directive target means "hand this back to the user".
pub fn is_final(target: &str) -> bool {
    matches!(target, "user" | "done" | "deliver" | "delivered" | "result")
}

/// True when a directive targets the Jev answerer (the shared question/decision
/// node) rather than a teammate or the final `@user` sink.
pub fn is_jev(target: &str) -> bool {
    matches!(target, "jev" | "judge" | "decide")
}

/// True when a directive means "fan this goal out to the whole team in parallel".
pub fn is_parallel(target: &str) -> bool {
    matches!(target, "parallel" | "fanout" | "all" | "team")
}

/// Strip line-leading delegation directives (`@…`) and activity markers from a
/// reply, so the remaining text is the worker's actual deliverable, ready for
/// aggregation.
///
/// The markers (`[step]`, `[thinking]`) are transcript decorations rather than
/// content. They must be filtered here as well as not emitted by
/// `SessionMessage::display_text`, because a reply can legitimately *contain* a
/// line that looks like one — and a marker left behind would make a
/// directive-only reply look like it had a payload, losing the directive's actual
/// text.
pub fn strip_directives(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('@') && !is_marker_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// True for a transcript marker line — `[step]`, `[thinking]` — which is
/// presentation, never content.
fn is_marker_line(line: &str) -> bool {
    matches!(line, "[step]" | "[thinking]")
}

/// The brief handed to each worker in a parallel round.
///
/// A fan-out is not a delegation chain: every worker owes its *own* contribution
/// and the aggregator owns the final answer. So the brief asks for the worker's
/// part and forbids handing off. The app enforces the same rule structurally in
/// `App::dispatch_delegations` — the brief is guidance for the model, not the
/// only guard — but stating it up front keeps a well-behaved model from
/// spawning side-chains it would only have to be stopped from.
pub fn fanout_brief(goal: &str) -> String {
    format!(
        "Parallel round — contribute your own part of this goal, from your own role:\n\n\
         GOAL: {goal}\n\n\
         Reply with your contribution as plain text. Do NOT hand off this round: no \
         @user, no @<teammate>, no @parallel. Another employee aggregates the team's \
         contributions into the final answer."
    )
}

/// A worker's contribution to a parallel round: its reply with directives
/// stripped, or — when the reply is *only* a final directive — that directive's
/// payload.
///
/// The second case matters because a worker told not to hand off may still
/// answer `@user <answer>`; that answer is its contribution, not the team's
/// result, and dropping it would lose the work entirely (the reply has no other
/// text).
pub fn contribution(text: &str) -> Option<String> {
    let body = strip_directives(text);
    if !body.is_empty() {
        return Some(body);
    }
    parse_delegations(text)
        .into_iter()
        .filter(|delegation| is_final(&delegation.target))
        .map(|delegation| delegation.message)
        .find(|message| !message.is_empty())
}

/// The preamble injected into each employee's first prompt so every employee
/// knows (a) who its teammates are, (b) the role boundaries that keep jobs with
/// the right teammate, and (c) how to delegate to them.
///
/// **Flat.** There is no root employee above the team: every employee is a peer,
/// so any of them may hand a sub-task sideways and any of them may deliver its
/// own finished work to the user with `@user`.
pub fn team_context(team: &Team, name: &str) -> String {
    let roster = team
        .employees()
        .iter()
        .map(|e| format!("  @{} — {}", e.id, e.role))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are {name}, one employee on a team of AI employees:\n{roster}\n\n\
Stick to YOUR OWN role and delegate anything outside it. In particular:\n\
  - The planner only produces plans, roadmaps, and task breakdowns — it never \
writes the implementation, code, or final prose; it delegates those to the \
teammate whose role covers them (e.g. the software engineer / writer).\n\
  - The researcher investigates and gathers facts, data, and references.\n\
  - The software engineer / writer owns implementation, code, and prose.\n\
  - The UI/UX designer owns layout, visual hierarchy, colour and type scales, and \
design specs; implementation of a screen belongs to the software engineer / writer.\n\
  - The reviewer checks, critiques, and verifies others' work.\n\n\
When a sub-task is better done by a teammate, delegate it by ending your reply \
with one or more lines of the form:\n\n  @<id> <instruction>\n\nFor example:\n  \
@researcher gather the sources\n  @writer implement the feature\n\nTo have the \
whole team work a goal in parallel (each from their own role), end your reply \
with `@parallel <goal>`. To hand the final result back to the user, prefix a \
line with @user or @deliver."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_line_leading_directives() {
        let text = "Here is my plan.\n@researcher: gather citations\n@writer write the intro\n\n@reviewer proofread it";
        let ds = parse_delegations(text);
        assert_eq!(
            ds,
            vec![
                Delegation {
                    target: "researcher".into(),
                    message: "gather citations".into(),
                },
                Delegation {
                    target: "writer".into(),
                    message: "write the intro".into(),
                },
                Delegation {
                    target: "reviewer".into(),
                    message: "proofread it".into(),
                },
            ]
        );
    }

    #[test]
    fn ignores_inline_mentions_and_annotation_noise() {
        let text = "Please ask @writer for help later.\n- bullet @researcher x\n@planner";
        let ds = parse_delegations(text);
        // Only the line-leading `@planner` counted, and it had no instruction.
        assert!(ds.is_empty());
    }

    #[test]
    fn recognizes_final_targets() {
        assert!(is_final("user"));
        assert!(is_final("deliver"));
        assert!(is_final("done"));
        assert!(!is_final("writer"));
    }

    #[test]
    fn recognizes_jev_targets() {
        assert!(is_jev("jev"));
        assert!(is_jev("judge"));
        assert!(is_jev("decide"));
        assert!(!is_jev("writer"));
        assert!(!is_jev("user"));
    }

    #[test]
    fn recognizes_parallel_targets() {
        assert!(is_parallel("parallel"));
        assert!(is_parallel("fanout"));
        assert!(is_parallel("all"));
        assert!(is_parallel("team"));
        assert!(!is_parallel("writer"));
        assert!(!is_parallel("user"));
    }

    #[test]
    fn strip_directives_drops_leading_at_lines_only() {
        let text = "A summary.\n@writer draft the doc\n\nKey point two.";
        assert_eq!(strip_directives(text), "A summary.\nKey point two.");
        // A reply that is *only* a directive yields no deliverable text.
        assert_eq!(strip_directives("@user final answer"), "");
    }

    #[test]
    fn target_case_is_normalized_and_colon_optional() {
        let ds = parse_delegations("@Writer Draft it");
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].target, "writer");
        assert_eq!(ds[0].message, "Draft it");
    }

    #[test]
    fn fanout_brief_forbids_handing_off() {
        let brief = fanout_brief("build the launch plan");
        assert!(brief.contains("GOAL: build the launch plan"));
        assert!(brief.contains("no @user"));
        assert!(brief.contains("no @<teammate>"));
        assert!(brief.contains("no @parallel"));
    }

    /// A worker's reply may be plain text, directives + text, or *only* a
    /// directive. The first two contribute the text; the last contributes the
    /// directive's payload rather than losing the work.
    #[test]
    fn contribution_keeps_plain_text_and_recovers_a_directive_only_reply() {
        assert_eq!(
            contribution("Market research findings.").as_deref(),
            Some("Market research findings.")
        );
        assert_eq!(
            contribution("Findings.\n@writer draft it").as_deref(),
            Some("Findings.")
        );
        assert_eq!(
            contribution("@user **Wake your team.**").as_deref(),
            Some("**Wake your team.**")
        );
        // A directive behind a marker line still contributes its payload: the
        // marker is presentation, so stripping it must not leave the reply looking
        // like it had a body.
        assert_eq!(
            contribution("[step]\n@user answer").as_deref(),
            Some("answer")
        );
        assert_eq!(
            contribution("[thinking]\n[step]\n@user answer").as_deref(),
            Some("answer")
        );
        // A bare non-final directive carries no deliverable.
        assert_eq!(contribution("@writer"), None);
        assert_eq!(contribution(""), None);
        // Markers alone are not a deliverable either.
        assert_eq!(contribution("[step]\n[thinking]"), None);
    }

    #[test]
    fn team_context_lists_roster_and_protocol() {
        let team = Team::starter();
        let ctx = team_context(&team, "Dex");
        assert!(ctx.contains("researcher"));
        assert!(ctx.contains("@writer"));
        assert!(ctx.contains("Dex"));
        assert!(ctx.contains("@user"));
        // The planner is explicitly told to delegate implementation away.
        assert!(ctx.contains("never writes the implementation"));
        assert!(ctx.contains("software engineer / writer"));
        // …and design work is routed to the designer, who does not implement it.
        assert!(ctx.contains("UI/UX designer owns layout"));
        assert!(ctx.contains("design specs"));
    }

    /// Every employee is a peer: the roster is a flat list and each one may
    /// deliver its own finished work, with no root to report to.
    #[test]
    fn team_context_is_flat_and_lets_everyone_deliver() {
        let team = Team::starter();
        let ctx = team_context(&team, "Bruno");

        assert!(ctx.contains("  @writer"), "{ctx}");
        assert!(ctx.contains("  @researcher"), "{ctx}");
        assert!(
            !ctx.contains("orchestrator"),
            "no employee reports to a root any more: {ctx}"
        );
        assert!(ctx.contains("@user"), "{ctx}");
        assert!(ctx.contains("@<id> <instruction>"), "{ctx}");
        assert!(ctx.contains("@parallel <goal>"), "{ctx}");
    }

    /// A marker line is presentation, not content: a reply whose only payload sits
    /// behind a `[step]` marker still contributes that payload, rather than being
    /// reduced to the marker.
    #[test]
    fn a_directive_after_a_marker_still_contributes_its_payload() {
        assert_eq!(
            contribution("[step]\n@user answer").as_deref(),
            Some("answer")
        );
        assert_eq!(
            contribution("[thinking]\n@user answer").as_deref(),
            Some("answer")
        );
        // A real body alongside a marker keeps the body and drops the marker.
        assert_eq!(
            contribution("[step]\nthe actual answer\n@user x").as_deref(),
            Some("the actual answer")
        );
    }
}
