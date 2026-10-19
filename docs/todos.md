# mywaker — TODO Checklist

Full implementation checklist. Mark rows `[x]` as done. Milestones map to
[`PLAN.md`](../PLAN.md) and [`architecture.md`](architecture.md).

Legend: 🧩 design · 🔧 install · 💻 code · 🧪 test · 📄 docs · ✅ ship

---

## M0 — Toolchain & scaffold

- [x] 🔧 Install Rust via rustup (stable, edition 2024) — done (1.99.1, aarch64-apple-darwin)
- [x] 🔧 Verify `rustc 1.99.1` · `cargo 1.99.1` · `rustfmt 1.10.0` · `clippy 0.1.99`
- [x] 🔧 Bin + lib split: thin `main.rs`, logic in `lib.rs` (`proj-lib-main-split`)
- [x] 💻 Write `Cargo.toml` (edition 2024, MSRV 1.88, ratatui 0.30.2 / crossterm 0.29 resolved)
- [x] 💻 Add release profile (`opt-level=3`, `lto=true`, `codegen-units=1`, `panic="abort"`, `strip=true`)
- [x] 💻 Add `[profile.dev.package."*"] opt-level = 3` (fast deps in dev)
- [x] 💻 Add clippy config: `#![deny(clippy::correctness)]` in `lib.rs` (warn groups deferred)
- [x] 📄 `.gitignore` (target/, .agents/, data/, *.log, .DS_Store)
- [x] ✅ `cargo build` green · `cargo clippy` clean · `cargo fmt --check` clean · `--help` runs

> Note: M0 shipped with the core TUI stack only. Integration deps (reqwest, SSE,
> cron, uuid, chrono) are added in M1/M6 via `cargo add` to resolve correct versions.

## M1 — OpenCode client module (`src/opencode/`)

- [x] 🧩 SPIKE: service discovery + auth — **done**: `opencode service status` → `:random-port`; `Authorization: Basic base64("opencode:" + password)`; password in `~/.config/opencode/service.json` (XDG path on macOS)
- [x] 🧩 SPIKE: SSE shape — **done**: `data: {json}\n\n` + `: heartbeat`; events `{id, created?, type, location?, data}`; types `server.connected`, `model.updated`, `shell.exited`, `session.tool.success`
- [x] 💻 `models.rs` — serde types (`AgentInfo`, `SessionInfo`, `Event`, `ServerInfo`, `Location`, `ListResponse`) + explicit `xxxID` renames (`workspaceID`, `projectID`, `parentID`)
- [x] 💻 `service.rs` — discover via `opencode service status` + read password (XDG-aware path)
- [x] 💻 `mod.rs` client — reqwest (no TLS: `--no-default-features --features json,stream`), health `GET /api/info`, `agents()`, `create_session`, `prompt`, `wait`, `interrupt`
- [x] 💻 `events.rs` — hand-rolled SSE parser (async-stream + reqwest `bytes_stream`); no `reqwest-eventsource` dep
- [x] 🧪 unit: serde round-trips + `basic_auth` + camelCase/ID-suffix fixtures
- [x] 🧪 live smoke test (`#[ignore]`): connect → `server_info` → list agents (passes against real service)
- [x] 🧪 integration: `tests/` fake OpenCode server (axum) exercising info/agents/session + prompt/wait/messages flow
- [x] ✅ `cargo test` green · `cargo clippy` clean · `cargo fmt` clean

> Note: ID-suffix fields use Go-style `xxxID` (not serde's `camelCase` `xxxId`).
> `session.rs` was folded into `mod.rs`; `messages()` endpoint is added in M5 when
> transcript rendering lands.

## M2 — App skeleton (TEA)

- [x] 💻 `main.rs` — `color_eyre::install()`, tokio main, clap CLI (`--config`, `--debug`), spawns `opencode_background`, wires mpsc channel. Harness subcommands (`serve`, `run <goal>`, `--employee`, `--embedded`) land with M6/M7.
- [x] 💻 `config.rs` — load via `config` crate (`tick_rate` default 100 ms). Strict `deny_unknown_fields` deferred until the config surface grows.
- [x] 💻 `app.rs` — `App` model + `Message` enum + `update()` + `opencode_background` task (connect → agents → SSE forward loop)
- [x] 💻 `event.rs` — `select!` loop: crossterm `EventStream` + `mpsc::Receiver<Message>` (OpenCode) + tick interval
- [x] 💻 `tui.rs` — terminal init/restore with manual panic hook (justified: manual `Terminal<CrosstermBackend>`, per skill exception)
- [x] 💻 `team.rs` — `Employee`, starter roster, `EmployeeStatus`, `is_busy`
- [x] 💻 `agent.rs` — write an employee's OpenCode agent definition (role prompt) so a new hire never hits `Agent not found`: `employee add` calls `agent::ensure` (writes only when absent), `employee agent <id>` (re)generates, `--force` overwrites
- [x] 🧪 unit: config parsing (valid file + unknown-field + malformed-value errors) — `deny_unknown_fields` now enabled on `Config`/`Schedule`; adds `[[schedules]]`
- [x] ✅ compiles, renders, `q`/`Esc` quits (verified via `TestBackend` render tests)

## M3 — Robot avatar widget (`src/ui/robot.rs`)

- [x] 💻 State-driven ASCII frames per mood (`RobotState::{Idle,Thinking,Working,Error}`) — eyes/antenna/mouth vary
- [x] 💻 `Idle` + `Thinking` + `Working` + `Error` variants as static ASCII templates
- [x] 💻 "thinking" animation via Braille spinner (self-contained) — `tui-shimmer` dep dropped to avoid an unverified-API dependency; swap in later if we want richer shimmer
- [x] 💻 Blink/spin driven by tick `frame` counter (deterministic)
- [x] 🧪 unit: phase→frame mapping — explicit render tests assert line count, idle blink period, spinner rotation, stable error glyph
- [x] ✅ avatar renders and animates in the left panel, reflects connection/team state (`connecting`→`thinking`, error→`✕`, connected+idle→blinking antenna)

## M4 — Team dashboard (`src/ui/dashboard.rs`)

- [x] 💻 Roster list (employees + status + avatar color) via `List`/`ListState` — extracted into `ui/dashboard.rs`
- [x] 💻 Left panel wiring + keyboard navigation (↑↓/jk)
- [x] 💻 Per-employee detail line (name · role · model · onboarded · task · status); `model`/`last_task` fields added to `Employee`
- [x] 🧪 unit: selection clamp (`move_selection`), status text mapping (`EmployeeStatus::label`)
- [x] ✅ dashboard shows the team and live status
- [x] 💻 Right-side "tasks" panel (`src/ui/tasks.rs`) — every employee with its latest task + live status; `last_task` is set on prompt send/delegation (`send_prompt`/`flush_outbox`) and retained (tinted done/error) until `/new`·`/clear` or the next task. Layout is now **sidebar (24) · roster grid (fill) · tasks (32)**: the left column is the inspector + vitals sidebar (`ui/sidebar.rs`), the centre is the card grid (`ui/pipeline.rs`), and the right is this panel. The panel also renders the persisted scheduled-task queue above the board (`App::tasks()` ordered by `order_tasks()` — active work first, then terminal, newest-first; the heading appears only when tasks exist).
- [x] 🧪 unit: `task_summary` (first line, whitespace collapse, 48-char truncate), `Team::set_last_task`; render smoke tests assert the panel shows a task and degrades at narrow width — plus `order_tasks` ordering, the `App::tasks()` accessor, and a queue-section render test
- [x] 💻 **Left sidebar** (`src/ui/sidebar.rs`) — the 24-column `waker` panel becomes an inspector + vitals column: mini avatar, aggregate Now line, selected-employee inspector, activity sparkline, queue vitals, uptime/Jev, alerts — dropped whole by a priority `fit` as height shrinks. Model layer adds `started_at`/`Pulse` and the aggregate accessors. The sidebar stays a **non-hit-test target**. Verification contract: [`acceptance-sidebar.md`](acceptance-sidebar.md) (A1–A10 × six sizes, SB1–SB6) — frozen *before* implementation, so the matrix is cited rather than re-invented. Landed with `tests/sidebar.rs` (`sidebar_at_*`, `a2_sidebar_containment`, `a8_sidebar_is_not_a_hit_target`, `a9_sidebar_animates`) plus `src/ui/sidebar.rs` unit tests for `fit`/`fmt_uptime`/`bar`/`sparkline`/markers. Evidence rows are the tester's to fill

## M5 — Chat & streaming (`src/ui/chat.rs`)

- [x] 🧩 SPIKE: streaming event names — resolved from the installed binary: `session.text.{started,delta,ended}`, `message.part.{delta,updated,removed}`, `session.reasoning.{started,delta,ended}`, `session.execution.{started,succeeded,failed}`, `session.step.*`, `session.tool.*`
- [x] 🧩 SPIKE: `create_session` response is wrapped in `{"data":{…}}` (was a `{id}` assumption) — fixed with `DataEnvelope`
- [x] 💻 Create OpenCode session on `Enter` (workdir from `config.workdir`, else cwd)
- [x] 💻 Input box (single-line) → send prompt → `POST /api/session/{id}/prompt`; a prompt typed while the employee is mid-turn (or before its session is ready) is queued (`Chat::queue_running`) and sent one per completed turn by `pump_employee_queue` (hooked into `handle_transcript`/`handle_session_created`/`handle_delivery_failed`), with `/new` dropping the old queue
- [x] 🧪 unit: a prompt to a busy employee is queued (still transcribed + recalled), `next_queued` gating (idle **and** live session), and a no-op pump without a client — the gate itself is one predicate, `App::can_prompt` (ready session + idle), shared by `send_prompt` and `next_queued`
- [x] 💻 Stream SSE deltas into a live transcript (`session.text.delta` / `message.part.delta`), then replace with the canonical `GET …/message` transcript on completion (tolerant double-write)
- [x] 💻 Status transitions fuel the avatar (reasoning→thinking, execution.started→working, succeeded→done, failed→error)
- [x] 💻 `messages()` client + `SessionMessage`/`Part`/`MessageError` models (confirmed against a real payload)
- [x] 🧪 unit: delta assembly in order, session scoping, status transitions, transcript mapping
- [ ] ✅ full live conversation loop (type → send → watch stream → result) — budget restored; verify against the live service

## M5.5 — model selector (superseded the Kenari models router)

> **Superseded.** The cross-provider router below was removed in favour of an
> explicit **model selector**: list models via `mywaker model list` (OpenCode
> `GET /api/model`) and assign one per employee with `mywaker model set
> <employee> <model>` or the TUI picker (Roster → `m`). The chosen model is
> persisted to `team.toml` and actually passed to `create_session`. `role::Role`
> (workflow ordering) and `jev::JevClient` (decision answerer) survive.

（The following items record the original router work, now removed.）

- [x] 💻 `kenari/models.rs` — serde types for `GET /v1/models` (`ModelInfo`, `Pricing`, `Modalities`); `sunset_at` is a unix timestamp, `pricing.unit = micro_idr_per_1m_tokens`
- [x] 💻 `kenari/router.rs` — `Role` (mapped from employee id), `RoleProfile` (weights + price caps), `Router::route` (filter → weighted score), `ModelChoice` + human-readable `reason`
- [x] 💻 `kenari/mod.rs` — `KenariClient::list_models()` (public endpoint, HTTPS) + `route_roster(employees)`
- [x] 💻 reqwest TLS: `rustls` feature added (0.13 renames `rustls-tls`→`rustls`; aws-lc-rs provider) so the client can reach `kenari.id`
- [x] 💻 CLI `mywaker models` subcommand — prints each employee → routed model (live)
- [x] 💻 TUI wiring — `Message::ModelsAssigned` sets each `Employee.model` on `AgentsLoaded` (shown in the roster detail)
- [x] 🧪 unit: role requires reasoning, budget caps, chat/tool/sunset filtering, context floor, role-profile differentiation
- [x] ✅ `mywaker models` routes live: researcher→`claude-sonnet-5`, writer→`claude-haiku-5-5`, reviewer→`claude-haiku-5-5`, planner→`claude-opus-5-5`
- [x] 💻 `TaskComplexity::{Low,Medium,High}` (+ `infer` cue-word/length heuristic) — complexity drives the budget band so simple tasks → cheap models, hard tasks → top-tier
- [x] 💻 `RoleProfile::for_task(role, complexity)` — Low/High bands adjust the role's price caps + weights
- [x] 💻 `Role::from_goal` + `Router::auto_route(role, goal, models)` + `kenari::auto_route(goal)` — auto-find the best route for a task description
- [x] 💻 CLI `mywaker route "<task>"` — prints inferred role + complexity + chosen model (live)
- [x] 🧪 unit: complexity inference, role inference, Low→cheap / Medium·High→premium tier selection
- [x] ✅ live: trivial task → `claude-haiku-5-5`, complex/research task → `claude-opus-5-5`
- [x] 💻 cross-provider routing: `opencode::ProviderModel` (+ `OpenCode::models()` → `GET /api/model`) and `kenari::sources::from_provider_model` normalize the user's *existing* providers into `ModelInfo` (USD→IDR via `IDR_PER_USD = 16_000`; skip disabled; skip `providerID == "kenari"` mirror; missing cost → free)
- [x] 💻 `kenari::gather_candidates()` merges Kenari catalog + OpenCode model list; `auto_route`/`route_roster` rank across all providers (best-effort if OpenCode is down)
- [x] ✅ live cross-provider: researcher/planner → `anthropic/claude-opus-5.5`, writer/reviewer → `openai/gpt-6-luna-pro` (an existing OpenAI provider, not just Kenari/Anthropic)

## M6 — Harness: tasks, retry, persistence, scheduler

- [x] 💻 `task.rs` — `TaskId` newtype, `TaskStatus` state machine, `try_retry` (cap 3)
- [x] 💻 `persistence.rs` — JSON store (tasks + memory) via `dirs`, atomic single-write
- [x] 💻 Wire `wait` endpoint as completion signal; detect failure → retry preamble
- [x] 💻 `scheduler.rs` — `tokio-cron-scheduler` from `[[schedules]]` → dispatch
- [x] 💻 Watch-hook: file-change trigger → dispatch (`notify` crate) — `[[watches]]` config (`path`/`employee`/`goal`/`recursive`); `src/watcher.rs` arms a `notify` watcher per entry, ignores pure reads, debounces a save burst (`Coalescer`, 500 ms), and emits `Message::WatchedTask` (goal templated with `{path}`, `~` expanded) into the same autonomous dispatch path as cron (`App::handle_autonomous_task`). Wired in `main.rs` alongside the scheduler.
- [x] 🧪 unit: watch debounce coalescing, `{path}` templating, `~` expansion, access-event filtering, and a synthetic burst → `Message::WatchedTask` (`watch_one`) — plus `tests/watch_hook.rs`: a real filesystem change reaches `watcher::run` as a `Message::WatchedTask` (no service/budget needed)
- [x] 🧪 unit: task state transitions (Pending→Running→Succeeded/Failed/Retrying)
- [x] 🧪 unit: retry cap; persistence round-trip
- [ ] ✅ scheduled task fires autonomously, survives restart via persistence — history round-trips; live fire needs a running service

## M6.5 — Pause & resume

> Frozen spec: [`acceptance.md`](acceptance.md). Pause holds new autonomous work —
> a cron or watch fire while paused lands as `pending` in the task queue and starts
> no session. The scheduler and watcher are unchanged; the app queues instead.
> Interactive chat and delegation hand-offs are out of scope.

- [x] 💻 `TaskStatus::Paused` + `Task::pause`/`resume` (legal only from `Pending`/`Running`); a `paused` flag on the task and an engine gate on the store, both `#[serde(default)]` so `store.json` round-trips (AC5)
- [x] 💻 Engine gate: `handle_autonomous_task` records a paused fire as `pending`, starts no session, and logs `⏸ queued (paused)` (AC1); `Space` toggles it and `resume_pending` dispatches queued work in `order_tasks` order (AC2)
- [x] 💻 Per-task control: `s` pauses/resumes the selected employee's current task — `pause_task` interrupts the session, `resume_task` re-dispatches with `harness::resume_preamble` (AC3/AC4)
- [x] 💻 Race guard: `Message::TaskAttemptStarted` records the attempt's session (and interrupts it if the task was paused first), and `Message::TaskAttemptFinished` is a no-op for a paused task (AC3)
- [x] 💻 Indicators: `Space`/`s` in the hint bar, help overlay, and README; the queue heading shows `⏸ paused` and a paused task renders `paused`

### Acceptance

The criteria for this milestone (**AC1–AC6**) are frozen in
[`acceptance.md`](acceptance.md) — the reviewer verifies against that file, not
against out-of-band context.

## M6.6 — Pause on close, resume on reopen

> Spec: [`acceptance.md`](acceptance.md) §1.1 plus AC4a / AC4b / AC7. Quitting holds
> the whole queue and interrupts what it was running; the next launch offers back
> only what mywaker paused itself, and resume **re-runs** each task in a new session.

- [x] 💻 `PauseReason::{User,Shutdown}`, `Task::pause_for` (with `pause()` delegating as
  `User`, so every existing caller keeps its meaning), the `Retrying → Paused` edge, and
  `paused_at_shutdown()`; `resume()` clears the reason with the flag. The field is
  `#[serde(default)]`, so a store written before it existed loads as `None`.
- [x] 💻 Close path: `pause_active_tasks` pauses every non-terminal task in `order_tasks`
  order and interrupts each session **after** its task is paused, so the existing
  `handle_task_attempt` guard absorbs the trailing completion; called from
  `initiate_shutdown` before the store is flushed, so the pause survives
  `panic = "abort"`.
- [x] 💻 Open path: `prepare_resume_prompt` collects only shutdown-paused tasks into one
  `ResumePrompt` (unset on a normal launch); `resume_shutdown_paused` resumes them through
  the ordinary `resume_task` → `dispatch_task` path; `dismiss_resume_prompt` keeps them
  held and leaves the engine gate alone.
- [x] 💻 Key routing: the prompt answers `y`/`Enter`, `n`/`Esc`, and `q`/`Ctrl-C`, and
  swallows every other key (`handle_key`, ahead of the help overlay and the global quit
  bindings); mouse input is swallowed while it is up. Quit itself stays immediate.
- [x] 💻 Modal: `src/ui/modal.rs` — a centred bordered block drawn with the same
  `centered`/`key_hint` helpers as the other overlays, precedence shutdown → reopen prompt
  → help; the body says the tasks **re-run from the beginning**.
- [x] 💻 Preamble correctness (AC4b): only a task that had a prior attempt is resumed with
  `harness::resume_preamble`; a never-started task is re-run with its bare goal.
- [x] 📄 Docs — `acceptance.md` (§1.1, §3, AC4a/AC4b/AC7, §6), this block,
  `architecture.md` §13, `README.md` (behaviour + modal keys).

## M7 — Orchestration (multi-employee fan-out)

- [x] 💻 Orchestrator: split one goal across `N` employees → parallel sessions (`tokio::spawn` + `JoinSet`)
  - `@parallel <goal>` (aliases `@all`/`@team`/`@fanout`) fans a goal out to every teammate but the source and the aggregator; each worker gets its own session created in parallel. The `/parallel <goal>` chat command triggers the same from the user.
  - `src/workflow.rs` (`is_parallel`, `strip_directives`) + `src/app.rs` (`fan_out`, `FanOut` state).
- [x] 💻 Handoff: employee output fed as input to the next (role chain)
  - Peer-to-peer delegation protocol: an assistant reply ends with line-leading `@<id> <instruction>` directives; the app relays each to the named employee (creating their session on demand). `@user`/`@deliver`/`@done` marks the final answer, routed back to the origin chat.
  - `src/workflow.rs` (`parse_delegations`, `is_final`, `team_context`) + app dispatch (`dispatch_delegations`/`send_to_employee`/`flush_outbox`), idempotent per message id, capped at 64 hand-offs to stop `A → B → A` cycles.
- [x] 💻 First prompt to each employee carries a team-context preamble (roster + protocol) so anyone can delegate to anyone.
- [x] 💻 `form.created` auto-answer: the event loop resolves session forms via Jev (`jev::answer_field_or_default` — Jev's confident value for choice-shaped fields, else the deterministic `default_answer_field`), so delegated sessions never hang waiting on a human. (OpenCode v2 renamed `question` → `form`; the reply endpoint is `POST /api/session/{id}/form/{formID}/reply`.) The event nests the form under `data.form` (`{"data":{"form":{id, sessionID, title, fields}}}`, observed live on v2.0.32) with no top-level `sessionID`; `Event::form_request` unwraps that container (and `properties.form`, plus the older flat shape) so the form is never missed.
- [x] 💻 Jev as a delegated answerer: `@jev <question>` routes to Jev (`jev::answer_text_or_default`, a binary yes/no `choice` falling back to `FALLBACK_VERDICT`) and posts the verdict back to the asking employee's chat. The team panel (`src/ui/pipeline.rs`) renders a fixed 3-column grid of employee cards (one column below a 12-cell card) with Jev as a one-row header above it — `jev ◆ on — @jev` plus its decision model — so the relationship stays visible without spending a card on a non-employee.
- [x] 💻 Aggregate results into a single deliverable view + review pass (`reviewer`)
  - Once every worker in a fan-out finishes (`note_fanout_result`), their stripped deliverables are collected and handed to the aggregator — the reviewer, else the planner — which synthesizes one deliverable and `@user`s it back (`aggregate_fanout`).
- [ ] ✅ one goal → team works in parallel → aggregated result — budget restored; verify against the live service.

## M7.5 — Reliability & telemetry

- [x] 💻 Reliability primitives (`src/harness.rs`): `PendingDelivery`, `MAX_ATTEMPTS=3`, `MAX_CHAIN_DEPTH=8`, `DELEGATION_TOTAL_LIMIT=64`, `HANDOFF_TIMEOUT=5m`, `retry_preamble`/`abandon_note`.
- [x] 💻 Per-chain isolation: `origin_of`/`depth_of` maps replace the single `delegation_origin` global; final answers route to their own chain's origin.
- [x] 💻 Deadline/retry/abandon driven from `Tick`: silent hand-offs retried (bounded) then abandoned with a `⚠` note; SSE events heartbeat the deadline.
- [x] 💻 Delivery failure surfacing: `Message::DeliveryFailed` replaces silent `Log` on prompt errors; delegated hand-offs auto-retry, user prompts surface the error.
- [x] 💻 Fan-out completion guarantee: `force_worker_contribution` + `aggregate_ready_fanouts` let a fan-out finish even when a worker fails/abandons.
- [x] 💻 Telemetry readout (`src/usage.rs` + UI log line): token in/out, cache-hit %, process RAM (K/M), EMA CPU % via `sysinfo`.
- [x] 🧪 unit: token-usage dedup + cache-hit clamp; deadline retry→abandon; abandoned worker still aggregates; `fmt_bytes`/`fmt_count`/`Ema`.

## M8 — Polish & ship

- [x] 💻 Help popup + keybinding footer (q quit, ↑↓←→ move, wheel scroll, Enter chat, m model, p projects, ? help) — the footer is data (`ui::hint_bar`: one `(key, description)` list per screen, styled by `ui::hint_line`), the overlay body is `ui::help_lines`, and the chat's slash commands live in one table (`app::CHAT_COMMANDS`) that both the completion menu and the `/help` listing read, so the menu and the listing can't drift
- [ ] 💻 Tabbed layout: Dashboard / Tasks / (Chat is modal-on-Enter) — superseded by the employee-grid + right-hand tasks panel (see M4 note)
- [x] 💻 Theming/colors consistent (cyan/green/red/yellow-dim per architecture §5) — the status → glyph/colour/spinner mapping is shared from `src/ui/status.rs` by the grid, the tasks panel, and the avatar, so the panels can't drift apart
- [x] 💻 Mouse input — click selects whatever is under the pointer (a grid card, a picker row), the wheel scrolls the roster grid's viewport (without moving the selection) and pages the chat transcript, and `Enter` still activates, so a stray click changes nothing. Capture is released on every exit path (including the panic hook). The geometry is shared: `ui/layout.rs` (frame regions, list windows) and `ui/pipeline.rs::grid` (card rectangles, at the app's scroll offset) are read by the renderer *and* `ui::hit::target_at`, so a click can only land where something was drawn
- [x] 🧪 mouse — tests click what they read off the rendered frame (a card's title, a list row), including a scrolled picker; the list window keeps the selection visible; motion is not input. The grid adds `wheel_scrolls_the_employee_grid` (clamped at both ends, selection untouched), `grid_hit_testing_matches_the_drawn_cards_at_any_offset`, `arrows_move_around_the_grid_and_reveal_the_selection`, and `page_keys_move_by_a_viewport`
- [x] 💻 Roster grid is a **scrollable viewport** — `App::roster_scroll` plus `pipeline::reveal`: the wheel scrolls, arrows move and reveal, `PgUp`/`PgDn` move a page of cards, and the title reads `team · N · rows a-b/c` while the roster overflows. Only whole rows are drawn and the offset clamps to the last full page, so no blank-row overscroll and no half-drawn card. Public seams for tests: `App::update`, `App::roster_scroll`, `App::roster_max_scroll`, `ui::team_area`, `ui::grid_cells`, `ui::Target`
- [ ] 💻 `tui-shimmer` idle "waiting for work…" states — deferred: self-contained Braille spinner already animates thinking/working (see M3 note)
- [x] 📄 `README.md` — install, config, keybindings, OpenCode setup, scheduling, sample agents (screenshots/gif deferred)
- [x] 📄 Sample agent definitions checked in — `examples/opencode.agents.jsonc` + `examples/agents/{planner,researcher,writer,reviewer,modeler}.md`; a test asserts a generated `modeler` definition is byte-identical to the checked-in copy, so the two can't drift
- [x] 📄 `RELEASING.md` — the release is one `chore: bump version to X.Y.Z` commit plus an annotated tag on it (feat → minor, fix → patch); the version lives only in `Cargo.toml`, which the `Makefile` and `packaging/*` templates derive, so nothing else needs editing. The checklist spells out the pre-tag verification (`fmt` / `clippy -D warnings` / `test` / `doc` / release build) and the differential cell dump that a UI-touching release needs as evidence
- [x] ✅ `cargo fmt --check`
- [x] ✅ `cargo clippy --all-features -- -D warnings` clean
- [x] ✅ no `unwrap()` outside tests (colors-eyre context everywhere)
- [x] ✅ terminal restored on all exit paths (incl. panic)
- [x] ✅ `cargo build --release` succeeds; binary size noted (6.4M `target/release/mywaker`)

---

## Definition of done (each milestone)

- [x] Compiles with `cargo build`
- [x] `cargo clippy` clean, `cargo fmt` applied
- [x] Focused unit/integration tests pass
- [ ] Behavior verified manually in the TUI — pending a live run (deferred by choice, not covered here)
- [x] Checklist row(s) flipped to `[x]`

## Open risk register

| Risk | Status | Mitigation | Owner milestone |
|---|---|---|---|
| OpenCode auth/discovery differs on macOS | ✅ Resolved | `Basic opencode:<password>` + `opencode service status` discovery confirmed | M1 |
| SSE event schema is undocumented/experimental | ✅ Resolved (shape) | `data: {json}\n\n` + `: heartbeat` confirmed; generic `Event` tolerates unknown fields | M1 |
| Message-text streaming event names | ✅ Resolved | Extracted from the installed binary: `session.text.*`, `message.part.{delta,updated,removed}`, `session.reasoning.*`, `session.execution.*`, `session.step.*`; plus canonical-poll fallback | M5 |
| `create_session` response `{"data":{…}}` envelope | ✅ Resolved | Verified live; `DataEnvelope<T>` added | M5 |
| `POST /api/session` rejects bare-string `model` (`Expected Model.Ref \| null`) | ✅ Resolved | `create_session` now sends `model` as `{ "id", "providerID" }`; provider persisted on assignment (`model_provider`) and backfilled for legacy rosters | M5.5 |
| Employee added with no OpenCode agent → `400 Agent not found` | ✅ Resolved | `employee add` writes the agent definition (`src/agent.rs`, global agents dir) before persisting the roster; `employee agent <id>` (re)generates it, and the runtime fallback still covers rosters that predate this | M2 |
| `experimental/session/{id}/wait` signature drift | ✅ Resolved | Scheduled-task runner calls `wait` best-effort then verifies completion from `GET /api/session/{id}/message` (the drift fallback) | M6 |
| `form.created` payload shape | ✅ Resolved | Forms are nested under `data.form` on v2.0.32 (no top-level `sessionID`); `Event::form_request` unwraps `form` from `data`/`properties` and still accepts the older flat shape, so a paused session is answered instead of hanging | M7 |
| Live model inference blocked by weekly budget | ✅ Resolved | Budget restored; live conversation loop can be verified against the service | M5 |
| No Rust SDK (by design) | ✅ Resolved | Thin `opencode` module isolates the HTTP surface | M1 |

---

## Future work

### Session-continuation policy

> **Continuation is technically supported and well-defined, but the policy is
> unexplored.** OpenCode's `POST /session/{id}/prompt` durably admits input to an existing
> session and starts a new turn *even after an interrupt* (verified live, v2.0.32): the
> interrupted turn persists as a partial assistant message (`finish:"error"`,
> `error.type:"aborted"`, "Step interrupted") followed by an `idle` marker
> `outcome:"interrupted"`, and a later prompt appends on top. Same-`session_id` resume is
> therefore feasible — the open questions are product policy: whether to reopen vs start
> fresh, how to treat the retained aborted turn (`completion_status`/`task_outcome`
> currently grade it `Error`), and which `delivery`/`resume` knobs to use.

**Finding — prompting an interrupted session is well-defined.** `session.prompt` is an
*admission* call ("Durably admit one session input and schedule agent-loop execution unless
`resume` is false") and `session.interrupt` returns "false for the idle no-op" with a
`resume` knob, per the live OpenAPI of the running service (v2.0.32). Observed live on
probe session `ses_eb304f82bffeTv0LOcpn4KYuKr` (free model, zero cost): prompt →
interrupt → the cut-off turn retained as `assistant(finish:"error", error.type:"aborted")`
followed by `idle(outcome:"interrupted")`; a second prompt on the same now-idle session ran
a new turn (`user → assistant(finish:stop) → idle(succeeded)`) — **no error, no clean
slate**. Client refs: `SessionMessage`'s `finish`/`outcome`/`error`
(`src/opencode/models.rs:338-342`) and `is_incomplete()` (`:384`), `completion_status`
(`src/app.rs:3167`), and `task_outcome` (`src/app.rs:3247`).

**Open policy questions.** (1) Reopen the stored session or start fresh. (2) What to do
with the retained aborted `finish:"error"` turn — steer past it, hide it, or accept it as
context (mywaker grades it `Error` today). (3) Which admission knobs to use —
`delivery: steer` vs `queue`, and the `resume` flag on prompt/interrupt.

**Acceptance signal.** A resumed task either continues the same `session_id` with a defined
treatment of the aborted turn, or the feature explicitly documents "start over" as the
chosen policy. It is the latter today — see [`acceptance.md`](acceptance.md) §3 and AC4a —
and the three places that would have to change are `run_task_attempt`'s
`create_session_with_agent_fallback` (`src/app.rs:3212`), the `session_id` overwrite in
`note_task_attempt` (`src/app.rs:2437`), and `harness::resume_preamble`
(`src/harness.rs:80`).

**Status: not supported today** — deliberate, and frozen as such by AC4a. Citations above
are as of this entry; the finding as it was recorded cited `src/opencode/models.rs:300-352`,
`src/app.rs:2995-3008` / `:3079-3092`, and `src/app.rs:3040` / `:2364` for the same three
call sites, before this slice's edits shifted them.
