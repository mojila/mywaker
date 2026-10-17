# mywaker — Architecture

Detailed technical design. Companion to [`PLAN.md`](../PLAN.md) and the checklist in
[`todos.md`](todos.md).

---

## 1. Crate & module layout

```
mywaker/
├── Cargo.toml
├── .gitignore
├── PLAN.md
├── docs/
│   ├── architecture.md        (this file)
│   └── todos.md
├── .opencode/                  (created in M4 — OpenCode agent defs, the "employees")
│   └── agents/
│       ├── researcher.md        (planned)
│       ├── writer.md            (planned)
│       ├── reviewer.md          (planned)
│       └── planner.md           (planned)
├── opencode.jsonc            (planned in M4 — OpenCode project config)
└── src/
    ├── main.rs               entry — color_eyre, tracing, clap, spawn background, run
    ├── lib.rs                lib facade + clippy lint
    ├── app.rs                App model + Message + update + opencode_background (TEA)
    ├── message.rs            Message enum (UI keys + background results)
    ├── event.rs              event loop: crossterm + mpsc + tick → select!
    ├── tui.rs                terminal init/restore, frame tick
    ├── config.rs             load ~/.config/mywaker/config.toml (tick_rate)
    ├── logging.rs            tracing → file
    ├── team.rs               Employee/roster model (maps to OpenCode agents)
    ├── workflow.rs           delegation protocol (parse `@id` directives, team preamble)
    ├── jev.rs                Jev (System One) client — answers `@jev` + `form.created`
    ├── role.rs               Role enum — workflow stage order/labels for the pipeline
    ├── task.rs               (planned M6) Task model, queue, retry, state machine
    ├── persistence.rs        (planned M6) JSON store (tasks, memory)
    ├── scheduler.rs          (planned M7) cron → dispatch events
    ├── opencode/
    │   ├── mod.rs            client + request helpers (server_info/agents/create_session/prompt/wait/interrupt/reply_form/events)
    │   ├── service.rs        discover `opencode service status`, read password
    │   ├── events.rs         SSE subscribe + typed event parse
    │   └── models.rs         serde types (Agent, Session, Event, ServerInfo)
    └── ui/
        ├── mod.rs            layout orchestration (view)
        ├── robot.rs          animated robot avatar (state-driven ASCII)
        ├── dashboard.rs      (planned M4) roster + status
        ├── chat.rs           (planned M5) conversation + streaming
        ├── tasks.rs          (planned M6) task list + retry
        └── components.rs     (planned) popups, help, status bar
```

Note: `opencode/client.rs` and `opencode/session.rs` from the original sketch were
folded into `opencode/mod.rs` (the client is small enough to live in one file); the
session `messages()` endpoint lands in M5.

Rationale for split: `opencode/` is a self-contained integration layer (could later
be its own crate); `ui/` is pure render; `app.rs` is the state machine. Modules are
organized **by feature** (rust-skills `proj-mod-by-feature`).

---

## 2. Data model

### 2.1 Domain types (this app)

```rust
/// A named AI employee — maps 1:1 to an OpenCode agent.
struct Employee {
    id: String,              // stable id ("researcher")
    name: String,            // display name
    role: String,            // human role ("Research lead")
    agent_id: String,        // OpenCode agent id (researcher.md)
    model: Option<String>,   // override, e.g. "anthropic/claude-sonnet-4-5#high"
    avatar: AvatarKind,      // robot variant / color
    onboarded: String,       // onboarding date (ISO) — for the "fun" roster field
}

enum EmployeeStatus { Idle, Thinking, Working, Done, Error(String) }

/// A unit of work handed to the harness.
struct Task {
    id: TaskId,                 // newtype(uuid)
    goal: String,               // natural-language goal
    assigned_to: Option<String>,// employee id (None = orchestrator)
    status: TaskStatus,
    attempts: u8,
    session_id: Option<String>, // OpenCode session id
    created_at: String,
    finished_at: Option<String>,
    transcript: Vec<ChatLine>,
}

enum TaskStatus { Pending, Running, Succeeded, Failed, Retrying }
```

Newtypes over `uuid`/`String` ids (rust-skills `type-newtype-ids`); no stringly-typed
flow. `TaskStatus` as an exhaustive enum (`type-enum-states`).

### 2.2 OpenCode types (serde models)

Mirror of the OpenCode HTTP API shapes (confirmed against
<https://opencode.ai/v2/docs/api>):

- `AgentInfo` ← `GET /api/agent`
- `SessionInfo` ← `POST /api/session` / `GET /api/session/{id}`
- `SessionMessage` ← `GET /api/session/{id}/message`
- `Event` ← `GET /api/event` (SSE), discriminated by `type`
- `ServerInfo` ← `GET /api/info` (version/health check)
- `ProviderModel` ← `GET /api/model`

---

## 3. OpenCode integration

### 3.1 Transport

- `opencode serve` listens on `http://localhost:4096` by default.
- All requests under `/api/...`; SSE event stream at `GET /api/event`.

### 3.2 Service discovery & auth (✅ CONFIRMED against v2.0.30)

Verified against a running `opencode serve --service` on macOS:

1. **Endpoint**: `opencode service status` → `http://127.0.0.1:<random-port>`.
   The port is ephemeral and is **not** recorded in `service.json`, so discovery
   shells out to `opencode service status` (OpenCode's own path). No managed-child
   launch is needed while the background service is up.
2. **Credentials**: `~/.config/opencode/service.json` holds a `password` (plus an
   `env` map). OpenCode uses the **XDG layout** even on macOS, so this path is
   `$XDG_CONFIG_HOME/opencode/service.json` or `$HOME/.config/opencode/service.json`
   — *not* what `dirs::config_dir()` returns (`~/Library/Application Support`).
3. **Auth header**: `Authorization: Basic base64("opencode:" + password)`.
   The username is the literal `opencode` (any other username returns 401).

### 3.3 Event stream (`GET /api/event`)

- Transport: **SSE** — `data: {json}\n\n` frames, `: heartbeat` keep-alives.
- Every event: `{ id, created?, type, location?, data }`.
- Confirmed event types: `server.connected`, `model.updated`, `shell.exited`,
  `session.tool.success`. Message-text streaming event names are confirmed during
  M5 (chat); M1 consumes events generically.
- **Serde gotcha**: OpenCode uses Go-style `xxxID` suffixes (`projectID`,
  `workspaceID`, `sessionID`, `assistantMessageID`). serde's `rename_all =
  "camelCase"` emits `xxxId`, so id-suffixed fields need explicit
  `#[serde(rename = "...ID")]`.

**Required endpoints** (confirmed against API reference):

| Purpose | Method + path |
|---|---|
| Health / version | `GET /api/info` |
| List agents (employees) | `GET /api/agent` |
| Create session | `POST /api/session` body `{ "location": { "directory": "..." } }` |
| Send prompt | `POST /api/session/{id}/prompt` |
| Read messages | `GET /api/session/{id}/message` |
| Block until idle | `POST /api/experimental/session/{id}/wait` |
| Interrupt | `POST /api/session/{id}/interrupt` |
| Live events | `GET /api/event` (SSE) |
| List models | `GET /api/model` |

### 3.4 Event-driven loop

The event loop `select!`s over three sources (`async-select-racing`,
`async-cancel-safety`):

```rust
select! {
    Some(Ok(ev)) = term_events.next()      => handle_key(ev),      // crossterm
    Some(ev)       = oc_events.next()      => handle_oc(ev),       // OpenCode SSE
    _ = tick.tick()                         => handle_tick(),      // 80–120ms animation
    Some(task)     = cron_rx.recv()        => dispatch(task),      // scheduler
}
```

OpenCode events drive: streaming text into the chat view, `EmployeeStatus`
transitions (idle→thinking→working→done), and task completion (for the harness's
wait/retry logic). SSE is consumed in a dedicated task feeding an mpsc channel.

### 3.5 Harness (retry + verification)

- `POST /api/experimental/session/{id}/wait` blocks until the session is idle —
  the primary "did it finish" signal.
- On failure/failure detection: increment `attempts`, mark `Retrying`, re-prompt with
  a short "the previous attempt failed, retry" preamble (capped, e.g. 3 attempts).
- On success: snap the transcript into the task, mark `Succeeded`, flush to
  persistence (`perf-io-buffering`: single write).

---

## 4. Robot avatar widget

`StatefulWidget` (`robot.rs`) rendering an ASCII robot whose face/mouth/antenna change
with `EmployeeStatus`, plus a `tui-shimmer` line for `Thinking`/`Working`.

- `Idle` → neutral eyes `◉ ◉`, flat mouth.
- `Thinking` → blinking `◉ ◯` + shimmer "thinking…".
- `Working` → `‿ ‿` eyes, shimmer "working…".
- `Done` → `^ ^` + green check.
- `Error` → `x x` + red.

Animation is **time-driven and pausable** using the `_at_phase` shimmer variant with
phase stored in the model (keeps render pure, per skill). Frames are `&'static [&str]`
consts; no per-frame allocation.

---

## 5. Styling

- Use `Stylize` helpers (`.cyan()`, `.green()`, `.red()`, `.dim()`, `.bold()`).
- Palette: cyan primary, green success, red error, yellow warnings (sparing), dim/mute.
- True-color with automatic fallback.

---

## 6. Config

`~/.config/mywaker/config.toml` (via `dirs`):

```toml
[opencode]
endpoint = "http://localhost:4096"
workdir  = "~"                 # default location.directory for sessions

[[employees]]
id = "researcher"
name = "Rex"
role = "Research lead"
agent_id = "researcher"
model = "anthropic/claude-sonnet-4-5"

[[schedules]]                  # cron triggers for autonomous start
cron = "0 9 * * 1-5"
employee = "planner"
goal = "Summarize yesterday's commits into a morning briefing"
```

Employees without a `schedule` are manual/chat-only. Configuration is validated at
load (serde `deny_unknown_fields`).

---

## 7. Terminal safety

- `color_eyre::install()` **before** terminal init so the terminal is restored before
  the report prints (per skill).
- Use `ratatui::run()` / `init` + `restore`; no hand-written panic hook.

---

## 8. Testing strategy

- Unit tests in `#[cfg(test)] mod tests` per module (`opencode/models` serde round-trip,
  `task` state machine transitions, `robot` phase math).
- Integration tests in `tests/` against a fake OpenCode HTTP server (`wiremock`-style
  or `axum` test server) for client/SSE behavior, so CI needs no real OpenCode.
- `tokio::test` for async paths.

---

## 9. Model selector

`mywaker` does **not** auto-route employees to models. Each employee's model is
chosen *explicitly* by the user via a **model selector**:

- **List** — the available models come from OpenCode's own providers
  (`GET /api/model` → `ProviderModel`), so the list is exactly what the local
  service can run. `mywaker model list` prints them (id, provider, name, cost,
  context, tool/reasoning flags).
- **Assign** — a model is persisted to `team.toml` as the employee's `model`
  field, via `mywaker model set <employee> <model>`, or interactively in the TUI
  (Roster → `m`) which lists models and assigns on Enter.
- **Apply** — when a session is created for an employee
  (`OpenCode::create_session`), the employee's chosen `model` is passed through as
  the session's model override (alongside the bound agent). No model → the
  agent's own default.

The previous cross-provider *router* (Kenari catalog + weighted capability/cost
scoring + task-complexity inference) was removed: automatic routing was assigning
models the user never actually selected, and the routed `model` was never wired to
the session. The role enum survives only as `role::Role`, used to order the team
as a pipeline in the UI. Jev (System One) survives as `jev::JevClient`, used for
`@jev` questions and `form.created` auto-answer — it is a *decision answerer*,
unrelated to model selection.

---

## 10. Delegation (employee ↔ employee hand-off)

Employees collaborate by **delegating**, not by a single orchestrator scheduling a
fixed role chain. Any employee can hand a sub-task to any other, and any employee
can hand the final result back to the user.

1. **Protocol** (`src/workflow.rs`) — an assistant reply ends with one or more
   line-leading directives of the form `@<id> <instruction>` (`@writer draft the
   intro`). Only line-leading `@` lines count, so inline mentions are ignored. The
   special targets `@user` / `@deliver` / `@done` mean "this is the final answer";
   `@jev` (or `@judge` / `@decide`) means "ask the decision model a yes/no
   question". `team_context()` renders the roster + protocol as a preamble so each
   employee learns its teammates on first contact.

2. **Bootstrap** — the first prompt sent to an employee has `team_context()`
   prepended (`Chat::bootstrapped`), so everyone knows the roster and the protocol
   before doing any work.

3. **Dispatch** (`app.rs`) — when a transcript load completes, `dispatch_delegations`
   scans the finished assistant reply for directives (idempotent per message id via
   `Chat::handled_message_ids`). Teammate targets are relayed via
   `send_to_employee` → `flush_outbox` (creating the target's session on demand and
   queueing instructions until it is ready); final targets reroute the answer back to
   the origin chat (`App::origin_of`, a per-chain map larger than the old single
   `delegation_origin` global). Two caps stop `A → B → A` cycles: a per-chain
   `depth_of` bound (`MAX_CHAIN_DEPTH`) and a run-wide `DELEGATION_TOTAL_LIMIT`.

4. **Form auto-answer** — `form.created` events, which pause a session until the
   user supplies typed input for one or more fields, are resolved out-of-band in
   the event loop (`spawn_form_answer`): Jev (`JevClient::answer_field`) picks
   the value for choice-shaped fields (options / booleans) when available and
   confident, with a deterministic `default_answer_field` fallback (field default,
   else affirmative/first option), then the client calls `reply_form` so the
   session resumes. This keeps an autonomous delegation chain from ever stalling
   on a human prompt. (OpenCode v2 renamed its `question` flow to `form`; the
   reply endpoint is `POST /api/session/{id}/form/{formID}/reply` with an
   `{"answer": {fieldKey: value}}` body.)

5. **Delegating to Jev** — an employee can hand a question straight to the decision
   model with `@jev <question>` (`route_to_jev`): the question is framed as a binary
   yes/no `choice` (`JevClient::answer_text`), and the verdict is posted back into
   the asking employee's chat (`Message::JevAnswer`). With no Jev client configured
   it answers "yes" deterministically. The **pipeline diagram** (`src/ui/pipeline.rs`)
   shows Jev as a node reached from the planner above and the employees below:
   the diagram is a top-down tree — planner on top, **Jev** in the middle, and the
   remaining employees (researcher, writer, reviewer, …) fanned out along the
   bottom — with an on/off state, so the "every employee delegates questions to
   Jev" relationship is visible in the UI.

6. **Parallel fan-out + aggregation** — `@parallel <goal>` (aliases `@all` /
   `@fanout` / `@team`) broadcasts one goal to every teammate except the source
   and the aggregator. Each worker starts its own session concurrently (the
   transport issues `N` sessions and `N` prompts in parallel; verified by
   `tests/opencode_fake.rs`). `fan_out` registers a `FanOut { workers,
   aggregator, outputs }`; as each worker's transcript completes,
   `note_fanout_result` records its deliverable (directives stripped), and once
   all workers contribute, `aggregate_fanout` hands the collected outputs to the
   aggregator — the reviewer, else the planner, else the source — which
   synthesizes one deliverable and returns it with `@user`. The `/parallel
   <goal>` chat command drives the same fan-out directly from the user.

7. **Agent binding** — each employee's session is created with its OpenCode agent
   id (`Employee::agent_id()`, which defaults to the employee id, or an explicit
   `agent` override in `team.toml`). This is what gives an employee a
   role-specific system prompt — e.g. a `planner` agent that only plans — rather
   than everyone running on OpenCode's default agent. The agents themselves are
   defined in the target project's OpenCode config (`agents` in
   `opencode.json(c)`; see `examples/opencode.agents.jsonc`). `team_context()`
   additionally inlines the role boundaries so the planner is explicitly told to
   delegate implementation (code/prose) to the software engineer / writer.

## 11. Reliability (timeouts, retry, fan-out completion)

Hand-offs are fire-and-forget by nature: a worker that goes silent used to hang the
chain or the fan-out forever. `src/harness.rs` adds the small primitives that close
those gaps, driven from the tick loop so the app stays in its TEA shape.

- **Pending delivery** (`Chat::pending: Option<PendingDelivery>`) tracks the in-flight
  delegated instruction on a target, its retry attempt count, and a liveness
  timestamp. `flush_outbox` attaches it on send; `handle_stream_event` heartbeats it on
  every SSE event for that session, so a healthy but slow worker is never abandoned.
- **Deadline + retry + abandon** — each tick, `tick_reliability` finds pending
  deliveries that have been silent past `HANDOFF_TIMEOUT`. `retry_or_abandon` re-sends
  the original instruction with a `retry_preamble` up to `MAX_ATTEMPTS`, then
  `abandon` posts a deterministic `⚠ no deliverable from <name> (<reason>)` note.
- **Delivery failure surfacing** — `prompt` errors now emit `Message::DeliveryFailed`
  (instead of a silent log line). Delegated hand-offs retry; a direct user prompt just
  surfaces the error in the chat for a manual retry.
- **Per-chain isolation** — the single `delegation_origin` global is replaced by
  `origin_of`/`depth_of` maps so concurrent chains don't clobber one another; a
  `@user` deliverable routes to its own chain's origin.
- **Fan-out guarantee** — `force_worker_contribution` injects the `⚠` note into any
  fan-out still waiting on an abandoned/failed worker, and `aggregate_ready_fanouts`
  (shared by `note_fanout_result`) aggregates every fan-out the moment all workers
  have reported, real or synthetic — so a fan-out can no longer deadlock.

## 12. Telemetry (token usage, cache hit, RAM, CPU)

The log panel's second status line shows the aggregate token usage, prompt-cache hit
rate, process RSS, and exponentially-averaged process CPU%.

- **Token usage** (`src/usage.rs::Usage`) — accumulated from `SessionMessage::tokens`
  (`openocode/models.rs::TokenUsage` / `CacheUsage`), deduped by message id so
  transcript re-loads don't double-count. Cache-hit rate is `cache.read ÷ input`,
  clamped to `1.0`.
- **Process metrics** (`src/usage.rs::ProcessMetrics`) — `sysinfo` reads the current
  process's RSS and CPU%, refreshed at most once a second from the tick loop; CPU is
  smoothed with an EMA (`usage::Ema`).
- **Note** — the exact `tokens` payload shape from OpenCode is experimental; a
  `#[serde(default)]`-tolerant `TokenUsage` keeps a mismatch from breaking
  deserialization, and the readout degrades to `0`/`—` when the field is absent.