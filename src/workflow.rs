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

/// Strip line-leading delegation directives (`@…`) from a reply so the remaining
/// text is the worker's actual deliverable, ready for aggregation.
pub fn strip_directives(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('@'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The preamble injected into each employee's first prompt so every employee
/// knows (a) who its teammates are and (b) how to delegate to them.
pub fn team_context(team: &Team, name: &str) -> String {
    let roster = team
        .employees()
        .iter()
        .map(|e| format!("  {} ({})", e.id, e.role))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are {name}, one employee on a team of AI employees:\n{roster}\n\n\
Work together on the goal. When a sub-task is better done by a teammate, \
delegate it by ending your reply with one or more lines of the form:\n\n  \
@<id> <instruction>\n\nFor example:\n  @writer draft a one-page summary\n  @researcher gather sources\n\nDo the parts that fit your own role directly, then \
delegate the rest. To have the whole team work a goal in parallel (each from \
their own role), end your reply with `@parallel <goal>`. To hand the final \
result back to the user, prefix a line with @user or @deliver."
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
    fn team_context_lists_roster_and_protocol() {
        let team = Team::starter();
        let ctx = team_context(&team, "Dex");
        assert!(ctx.contains("researcher"));
        assert!(ctx.contains("@writer"));
        assert!(ctx.contains("Dex"));
        assert!(ctx.contains("@user"));
    }
}
