# mywaker — Plan

A terminal app that runs a team of **AI employees** backed by **OpenCode**, visualized
with an animated **robot avatar**. Reproduction of the QoderWake concept ("Autonomous
AI Employees, On the Job") as a native Rust TUI.

> Status: **Planning** — see [`docs/todos.md`](docs/todos.md) for the full checklist.

---

## 1. Vision

Turn OpenCode from a developer tool into a *team of on-screen employees*:

- **On the job, in the terminal** — you talk to an employee, it starts work, streams
  its progress, and hands back a delivered result.
- **Robot avatars** — each employee has a live avatar that *feels* alive: idle,
  thinking (shimmer), working, done, or erroring.
- **Autonomous** — besides manual chat, an employee can start on a **schedule**
  (cron) or a **watch hook**.
- **A team, not one chat box** — employees delegate to each other directly (peer to
  peer, or a parallel fan-out), and the roster shows the hand-offs as animated wires.

## 2. Tech Stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust (edition 2024, MSRV 1.88) | Native, single binary |
| TUI | `ratatui` 0.30.x + `crossterm` 0.29 | Terminal UI SDK (per directive) |
| Async | `tokio` (full) + `futures` | Background tasks + SSE + event loop |
| Agent backend | **OpenCode v2.0.30** HTTP API + SSE | No official Rust SDK; HTTP is the stable surface |
| HTTP | `reqwest` 0.13 (json+stream, TLS deferred) | HTTP to `opencode serve` (localhost, plain HTTP) |
| SSE | hand-rolled parser (`async-stream` + `reqwest::bytes_stream`) | `reqwest-eventsource` dropped (unverified API); simpler + testable |
| CLI | `clap` 4 (derive) | `--config`, `--debug` today; `serve`/`run` subcommands in M6/M7 |
| Config | `serde` + `toml` | `~/.config/mywaker/config.toml` |
| Persistence | JSON on disk (`dirs`) | Tasks + memories survive restart |
| Errors | `color-eyre` (app), `thiserror` (lib-ish) | Idiomatic + terminal-safe |
| Logging | `tracing` + `tracing-subscriber` | Structured, quiet in TUI |
| Scheduling | `tokio-cron-scheduler` | cron triggers |
| Watch hooks | `notify` | file-change triggers (debounced) |
| Animation | Braille spinner + state-driven ASCII (self-contained) | `tui-shimmer` deferred; swap in if we want richer shimmer |

## 3. Architecture (overview)

```
crossterm event stream ──┐
OpenCode SSE stream ─────┤──► tokio::select! ──► Message ──► App::update ──► View
cron scheduler ──────────┤
file watchers ───────────┘
```

TEA (The Elm Architecture): `Model → Message → Update → View`, the standard
`async-app`/`component-app` pattern. Full breakdown in
[`docs/architecture.md`](docs/architecture.md).

## 4. What it looks like

```
┌─ mywaker · team: 4 employees ────────────────────────────┐
│      ┌─[◉ ◉]─┐                                          │
│      │  ───  │   ██ thinking… ▓▓▓▓▓▓░░░░ (researcher)   │
│      ├──┬─┬──┤                                          │
├───────┴───────┴──────────────────────────────────────────┤
│  EMPLOYEES            │  TASK STREAM                     │
│  ● researcher   ▸ on  │  > turn last meeting into a plan │
│  ● writer       ▸ busy│  ✓ read context (3 files)        │
│  ○ reviewer     idle  │  ▶ drafting outline…             │
│  ○ planner      idle  │                                  │
├────────────────────────┴─────────────────────────────────┤
│  q quit │ ↑↓ select │ Enter chat │ d dispatch │ r retry   │
└───────────────────────────────────────────────────────────┘
```

## 5. Deliverables (milestone summary)

- **M0 — Scaffold**: Rust toolchain, cargo project, deps, template layout.
- **M1 — OpenCode client**: HTTP + SSE + session/prompt/wait + service discovery.
- **M2 — App skeleton**: TEA loop, config, terminal safety, keybindings.
- **M3 — Robot avatar**: animated, state-driven ASCII robot.
- **M4 — Team dashboard**: roster + status.
- **M5 — Chat & streaming**: talk to an employee, stream response.
- **M6 — Harness**: task queue, retry, persistence, autonomous triggers
  (cron scheduler + file-change watch hooks).
- **M7 — Orchestration**: fan-out to multiple employees + aggregate.
- **M8 — Polish & ship**: help, theming, release build, lint/fmt.

## 6. Key decisions & assumptions

| Decision | Choice | Note |
|---|---|---|
| OpenCode integration | HTTP API (port 4096) + SSE, **not** subprocess `opencode run` | Real-time streaming is core to the "alive" avatar |
| Auth | Reuse `opencode serve` service registration (token) | **Spike in M1** to confirm exact discovery path on macOS |
| Employees = OpenCode agents | Each employee maps 1:1 to an `agent` defined in `opencode.jsonc` / `.opencode/agents/*.md` | Permissions & model come from OpenCode |
| No Rust SDK exists | Wrap the HTTP API in a small `opencode` module | Confirmed against v2 docs (TS-only client/SDK) |
| Robot avatar | ASCII/Unicode text, no image crate initially | `ratatui-image`/`chafa` optional later |

---

## 7. Out of scope (v1)

- IM/chat-platform connectors (Slack/Linear) — manual chat + cron/hook only.
- Multi-tenant / enterprise auth / audit UI.
- Long-term "forget/retain" memory store — v1 keeps per-employee memory JSON only.

---

## 8. Orchestrator-rooted team + main-screen prompt + animated delegation (**superseded**)

> **Retired.** The orchestrator employee was removed after it shipped. It was
> meant to fan a goal out across the roster, but in practice it only delegated to
> its own OpenCode sub-agents: the roster's cards never lit up, so the live
> delegation diagram showed nothing while work was happening. The team is flat
> again — every employee is a card, any employee may delegate to any other, and
> the roster's goal bar addresses the selected card. What survives from this
> section: the main-screen prompt (`i` / `Tab`), the animated delegation wires
> (now peer-to-peer between cards), and the Jev strip above the grid. The notes
> below are kept as the record of what was built and why it did not work.

Turn the *team* into an explicit tree with one **orchestrator** at the root, give the
main (roster) screen a prompt input that routes to it, and draw delegation as
**animated dashed lines**. Everything a flat roster's employees do today moves under
the orchestrator.

### 8.1 Orchestrator becomes the root employee

- `src/team.rs` — `ORCHESTRATOR_ID = "orchestrator"`. `Team::starter()` gains the
  orchestrator first (`Otto`), then researcher/writer/reviewer/planner (5 total).
  New `Team::ensure_orchestrator()` inserts it at index 0 when absent; called by
  `starter()` and `load_or_starter()` so an existing `team.toml` gains one on launch.
  `remove_employee("orchestrator")` is refused. `App::with_team` takes `mut team` and
  calls `ensure_orchestrator()`, preserving the invariant for every fixture.
- `src/role.rs` — `Role::Orchestrator` at order 0 (Planner 1 → Researcher 2 →
  Engineer 3 → Writer 4 → Reviewer 5 → General 6); `from_label` aliases `lead`/`root`.
- `src/agent.rs` + `examples/agents/orchestrator.md` — new `ORCHESTRATOR` prompt
  (decompose → `@<id>` → synthesize → only it uses `@user`). The other role prompts
  are rewritten to report finished work upward via `@orchestrator`. The example file
  is byte-locked to the const (like the modeler test).

### 8.2 Delegation reports upward (bubbling)

- `src/workflow.rs` — `is_orchestrator()`; `team_context()` rendered as a tree with
  the rule "you report to `@orchestrator`; `@user` is the orchestrator's alone".
- `src/app.rs` — a worker's final `@user` is **relayed to the orchestrator** as a
  report (instead of ending the chain); the orchestrator's own `@user` delivers to the
  user. `fan_out`'s aggregator becomes the orchestrator (fallback reviewer/planner);
  the orchestrator is excluded from the worker set. Existing `MAX_CHAIN_DEPTH` /
  `DELEGATION_TOTAL_LIMIT` remain the cycle guards — and a report **spends** the
  same budget rather than bypassing it, because it is a hand-off like any other. At
  either limit the report is logged and dropped (becoming an abandon note for any
  round waiting on it), never delivered: the one-mouth invariant holds hardest at
  the limits, where a worker's partial answer would be most misleading. When the
  orchestrator finishes with plain text, the result is logged to the main screen —
  unless that turn delegated or contributed to a round, since then the goal is not
  done yet.
- Scheduled and watched tasks are **isolated jobs, not chains**: they run in their
  own session with no `origin_of`/`depth_of` entry, so a `@user` in their reply has
  nowhere to bubble. They are briefed with the roster and the reporting rule *and*
  told to finish the job themselves (`task_brief`), and their deliverable is the
  task's result — not a chat delivery.
- Selection: `card_order()` (= roster minus orchestrator) drives the grid;
  `workflow_order()` still lists everyone for the tasks panel. `selected` defaults to
  the orchestrator; `↑` from the first card row reaches it, `Enter`/`m`/`s` work on it.

### 8.3 Main-screen prompt

- `src/ui/layout.rs` — `Regions` gains a `prompt` band; mode-aware:
  roster `title 1 · main fill · prompt 2 · log 4 · help 1`
  (others: `prompt 0 · log 6`, so `main` height is unchanged in roster mode).
- New `src/ui/prompt.rs` — a 2-row bordered bar (`orchestrator` + status + model,
  `> ` + input + cursor; `◆ N delegating` while traced). Shared cursor/scroll line
  renderer extracted to `ui/input.rs` (chat + prompt share it).
- `src/app.rs` — `prompt_focused`, `prompt_history`; `i`/`Tab`/click focuses, `Esc`
  blurs, `Enter` submits, `↑/↓` recalls goals. One shared `input` buffer, cleared on
  mode switch. `send_prompt` → `submit_to(employee, text, origin)`; new `submit_goal()`
  routes the roster prompt to the orchestrator.

### 8.4 Animated dashed delegation

- `src/ui/pipeline.rs` — team panel bands `title · orchestrator strip · bus · cards`;
  Jev moves into the strip's right side (keeps 2 card rows at 80×24). `Shape` gains
  `strip`/`bus` rects; a 1-column riser gutter left of each card column; `Node` gains
  `riser_x`. Degradation drops the bus then the strip only when they'd cost a card row.
- New `src/ui/wires.rs` — strip, dashed bus (`┄`), and per-trace paths (strip port →
  bus → riser → card top; reversed for reports). `path(...)` is a pure function.
  Animation: `progress = age/TTL`, cells behind the `◆` head lit in the trace colour
  (cyan out, green report, yellow fan-out).
- `src/app.rs` — `DelegationTrace { from, to, kind, started }`, capped at 12, recorded
  on every hand-off/report, pruned by `tick_traces()` (from `Tick` and `advance()`),
  TTL ≈ 12 ticks (~1.2 s); `app.traces()` + `DelegationTrace::age()` keep the UI pure.

### 8.5 Tests & docs

- Unit/integration: role/team order + migration + remove-refusal; workflow wording;
  orchestrator definition parity; `submit_goal` + bubbling + fan-out aggregator; prompt
  focus key routing + paste; traces prune + packet movement; `wires::path` contiguity;
  layout keeps `main` height. Update existing fixtures for the 5-employee roster and
  the new geometry (`app/tests`, `team`, `pipeline`, `tests/sidebar`, `tests/render`).
- Docs: README screenshot + orchestrator/`i`/`o` sections; `docs/architecture.md`
  module list + §10 delegation rewrite + layout notes; `docs/todos.md`; `PLAN.md`
  vision bullet.

### 8.6 Decisions & risks

- **Focus:** `i`/click focuses the prompt; every existing single-letter shortcut stays
  unchanged when unfocused. **Drawing:** strip + bus + risers (one column per card).
  **Routing:** bubble worker results to the orchestrator. **Delivery:** only a user
  prompt steers; every background hand-off queues (`harness::delivery_for`), and a
  delivery's silence clock starts at `session.execution.started`, so a queued prompt
  is never mistaken for a silent one and re-admitted.
- Index shift (orchestrator at 0) makes `app/tests` positional assertions noisy;
  risers cost one column per card; bubbling adds an orchestrator turn per `@user`
  (bounded by the existing caps). `mode: all` frontmatter is intentionally unchanged
  until `POST /api/session` behaviour with `mode: subagent` is verified.