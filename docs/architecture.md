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
    ├── route/                cross-provider router (provider-agnostic)
    │   ├── mod.rs            orchestration (gather_candidates, route_roster, auto_route)
    │   ├── models.rs         serde types (ModelInfo, Pricing, Modalities)
    │   ├── router.rs         Role, TaskComplexity, Router, ModelChoice, TaskClass
    │   ├── sources.rs        OpenCode provider model → ModelInfo adapter
    │   └── kenari.rs         Kenari adapter: catalog client + optional Jev classifier
    ├── task.rs               (planned M6) Task model, queue, retry, state machine
    ├── persistence.rs        (planned M6) JSON store (tasks, memory)
    ├── scheduler.rs          (planned M7) cron → dispatch events
    ├── opencode/
    │   ├── mod.rs            client + request helpers (server_info/agents/create_session/prompt/wait/interrupt/events)
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
- `ModelInfo` ← `GET /api/model`

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

## 9. Cross-provider model router (`src/route/`)

`mywaker` routes each employee to the best model across **providers** — Kenari's
catalog plus the user's existing OpenCode providers (OpenRouter, Clario, local
MLX, …) — chosen live rather than hardcoding ids. The router is
provider-agnostic: every model is normalized into a single `ModelInfo` shape
and scored the same way regardless of which gateway it came from, and task
classification is a local heuristic (no network, no provider dependency).

- **Catalog** — `GET https://kenari.id/v1/models` (public, no API key) returns
  `{ object:"list", data:[…] }`. Per model: `id`, `owned_by` (provider),
  `context_length`, `max_output_tokens`, `endpoints`, `modalities`,
  `reasoning`/`reasoning_options`, `tool_call`, `sunset_at` (unix ts), and
  `pricing.unit = "micro_idr_per_1m_tokens"` (so IDR = micro / 1e6). `serde` uses
  `rename_all = "snake_case"`.

- **Routing** — `Router::route(role, models)` (medium complexity) and
  `Router::route_with(role, complexity, models)` filter first (chat endpoint,
  `tool_call`, not sunset, `context_length ≥ min_context`, `require_reasoning`,
  price caps) and then rank by a weighted score:

  `score = w_capability·capability + w_reasoning·reasoning + w_context·context + w_vision·vision + w_cheap·cheap`

  where `capability = provider_tier(owned_by) × model_grade(id)`. `provider_tier` is a
  brand prior (anthropic/openai/deepseek > … > qwen > unknown); `model_grade` is a
  name-derived intra-provider prior (`fable`/`opus` > `sonnet` > `sol`/`pro`/`max` >
  `haiku`/`luna` > `flash`/`mini`). Both are documented **heuristics** — the catalog
  exposes no benchmark score, and they exist only so a cheap `haiku` doesn't tie a
  `sonnet`/`opus` whose other metadata is identical.

- **Complexity** — `TaskComplexity::{Low,Medium,High}` drives the *budget band* (and
  thus the model tier) on top of the role's trait weights, in `RoleProfile::for_task`:
  - **Low** → `input ≤ Rp5k`, `output ≤ Rp25k`, no reasoning required, 8k context,
    weights nudged toward cheap.
  - **Medium** → the role's own caps/weights.
  - **High** → uncapped, reasoning required, ≥128k context, weights nudged toward
    capability.
  `TaskComplexity::infer(goal)` classifies a description via cue words with a length
  fallback.

- **Roles** — `Role` maps from the employee id (`researcher`/`writer`/`reviewer`/
  `planner`, else `General`) and can also be inferred from a task description
  (`Role::from_goal`). Default picks against the live catalog: planner→`claude-opus`,
  researcher→`claude-sonnet`, writer/reviewer→`claude-haiku` (capability roles pay for
  quality, execution roles stay cheap). Weights/caps are tunable in
  `RoleProfile::for_role`/`for_task`.

- **Auto-route** — `route::auto_route(goal)` gathers candidates and classifies the
  task in one step; the classification is `TaskClass::infer(goal)`, a local
  heuristic (`Role::from_goal` + `TaskComplexity::infer`) with **no network or
  provider dependency**. `Router::auto_route` does the same over an
  already-fetched model list. Kenari's Jev (System One) classifier survives as an
  *optional* adapter (`route::kenari::JevClient`) a caller may overlay out of
  band — it is **not** on the default routing path.

- **Wiring** — `KenariClient::list_models()` (`route::kenari`, needs reqwest
  `rustls`; added in this milestone so the client speaks HTTPS) and
  `route_roster(employees)` glue the results to the roster. The TUI assigns on `AgentsLoaded` (`Message::ModelsAssigned`) so each
  employee's `model` shows in the dashboard; `mywaker models` prints the live routing
  and `mywaker route "<task>"` prints the auto-route for a task description, both
  without starting the TUI.