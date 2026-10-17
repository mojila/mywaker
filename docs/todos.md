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
- [x] 💻 Right-side "tasks" panel (`src/ui/tasks.rs`) — every employee with its latest task + live status; `last_task` is set on prompt send/delegation (`send_prompt`/`flush_outbox`) and retained (tinted done/error) until `/new`·`/clear` or the next task. Layout is now robot (24) · pipeline (fill) · tasks (32). The panel also renders the persisted scheduled-task queue above the board (`App::tasks()` ordered by `order_tasks()` — active work first, then terminal, newest-first; the heading appears only when tasks exist).
- [x] 🧪 unit: `task_summary` (first line, whitespace collapse, 48-char truncate), `Team::set_last_task`; render smoke tests assert the panel shows a task and degrades at narrow width — plus `order_tasks` ordering, the `App::tasks()` accessor, and a queue-section render test

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

## M7 — Orchestration (multi-employee fan-out)

- [x] 💻 Orchestrator: split one goal across `N` employees → parallel sessions (`tokio::spawn` + `JoinSet`)
  - `@parallel <goal>` (aliases `@all`/`@team`/`@fanout`) fans a goal out to every teammate but the source and the aggregator; each worker gets its own session created in parallel. The `/parallel <goal>` chat command triggers the same from the user.
  - `src/workflow.rs` (`is_parallel`, `strip_directives`) + `src/app.rs` (`fan_out`, `FanOut` state).
- [x] 💻 Handoff: employee output fed as input to the next (role chain)
  - Peer-to-peer delegation protocol: an assistant reply ends with line-leading `@<id> <instruction>` directives; the app relays each to the named employee (creating their session on demand). `@user`/`@deliver`/`@done` marks the final answer, routed back to the origin chat.
  - `src/workflow.rs` (`parse_delegations`, `is_final`, `team_context`) + app dispatch (`dispatch_delegations`/`send_to_employee`/`flush_outbox`), idempotent per message id, capped at 64 hand-offs to stop `A → B → A` cycles.
- [x] 💻 First prompt to each employee carries a team-context preamble (roster + protocol) so anyone can delegate to anyone.
- [x] 💻 `form.created` auto-answer: the event loop resolves session forms via Jev (`jev::answer_field_or_default` — Jev's confident value for choice-shaped fields, else the deterministic `default_answer_field`), so delegated sessions never hang waiting on a human. (OpenCode v2 renamed `question` → `form`; the reply endpoint is `POST /api/session/{id}/form/{formID}/reply`.)
- [x] 💻 Jev as a delegated answerer: `@jev <question>` routes to Jev (`jev::answer_text_or_default`, a binary yes/no `choice` falling back to `FALLBACK_VERDICT`) and posts the verdict back to the asking employee's chat. The pipeline diagram (`src/ui/pipeline.rs`) renders a top-down tree — planner top, “jev” middle, researcher/writer/reviewer on the bottom row — with an on/off state.
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

- [x] 💻 Help popup + keybinding footer (q quit, ←/→ stage, Enter chat, m model, p projects, ? help) — the footer is data (`ui::hint_bar`: one `(key, description)` list per screen, styled by `ui::hint_line`), the overlay body is `ui::help_lines`, and the chat's slash commands live in one table (`app::CHAT_COMMANDS`) that both the completion menu and the `/help` listing read, so the menu and the listing can't drift
- [ ] 💻 Tabbed layout: Dashboard / Tasks / (Chat is modal-on-Enter) — superseded by the workflow-pipeline + right-hand tasks panel (see M4 note)
- [x] 💻 Theming/colors consistent (cyan/green/red/yellow-dim per architecture §5) — the status → glyph/colour/spinner mapping is shared from `src/ui/status.rs` by the pipeline, the tasks panel, and the avatar, so the panels can't drift apart
- [x] 💻 Mouse input — click selects whatever is under the pointer (a pipeline node, a picker row), the wheel steps the selection and pages the chat transcript, and `Enter` still activates, so a stray click changes nothing. Capture is released on every exit path (including the panic hook). The geometry is shared: `ui/layout.rs` (frame regions, list windows) and `ui/pipeline.rs::tree` (node rectangles) are read by the renderer *and* `ui/hit.rs::target_at`, so a click can only land where something was drawn
- [x] 🧪 mouse — tests click what they read off the rendered frame (a node's title, a list row), including a scrolled picker; the list window keeps the selection visible; motion is not input
- [ ] 💻 `tui-shimmer` idle "waiting for work…" states — deferred: self-contained Braille spinner already animates thinking/working (see M3 note)
- [x] 📄 `README.md` — install, config, keybindings, OpenCode setup, scheduling, sample agents (screenshots/gif deferred)
- [x] 📄 Sample agent definitions checked in — `examples/opencode.agents.jsonc` + `examples/agents/{planner,researcher,writer,reviewer}.md`
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
| `POST /api/session` rejects bare-string `model` (`Expected Model.Ref | null`) | ✅ Resolved | `create_session` now sends `model` as `{ "id", "providerID" }`; provider persisted on assignment (`model_provider`) and backfilled for legacy rosters | M5.5 |
| `experimental/session/{id}/wait` signature drift | ✅ Resolved | Scheduled-task runner calls `wait` best-effort then verifies completion from `GET /api/session/{id}/message` (the drift fallback) | M6 |
| Live model inference blocked by weekly budget | ✅ Resolved | Budget restored; live conversation loop can be verified against the service | M5 |
| No Rust SDK (by design) | ✅ Resolved | Thin `opencode` module isolates the HTTP surface | M1 |