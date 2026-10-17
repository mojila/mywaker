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
- [ ] 🧪 integration: `tests/` fake OpenCode server (deferred to M5/M6, where prompt/wait flows are exercised)
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
- [ ] 🧪 unit: config parsing (valid file + unknown-field + missing-field errors) — deferred (config is a single field today)
- [x] ✅ compiles, renders, `q`/`Esc` quits (verified via `TestBackend` render tests)

## M3 — Robot avatar widget (`src/ui/robot.rs`)

- [x] 💻 State-driven ASCII frames per mood (`RobotState::{Idle,Thinking,Working,Error}`) — eyes/antenna/mouth vary
- [x] 💻 `Idle` + `Thinking` + `Working` + `Error` variants as static ASCII templates
- [x] 💻 "thinking" animation via Braille spinner (self-contained) — `tui-shimmer` dep dropped to avoid an unverified-API dependency; swap in later if we want richer shimmer
- [x] 💻 Blink/spin driven by tick `frame` counter (deterministic)
- [ ] 🧪 unit: phase→frame mapping — covered indirectly by render smoke tests; add explicit mapping test in M6 when statuses actually transition
- [x] ✅ avatar renders and animates in the left panel, reflects connection/team state (`connecting`→`thinking`, error→`✕`, connected+idle→blinking antenna)

## M4 — Team dashboard (`src/ui/dashboard.rs`)

- [x] 💻 Roster list (employees + status + avatar color) via `List`/`ListState` — extracted into `ui/dashboard.rs`
- [x] 💻 Left panel wiring + keyboard navigation (↑↓/jk)
- [x] 💻 Per-employee detail line (name · role · model · onboarded · task · status); `model`/`last_task` fields added to `Employee`
- [x] 🧪 unit: selection clamp (`move_selection`), status text mapping (`EmployeeStatus::label`)
- [x] ✅ dashboard shows the team and live status
- [x] 💻 Right-side "tasks" panel (`src/ui/tasks.rs`) — every employee with its latest task + live status; `last_task` is set on prompt send/delegation (`send_prompt`/`flush_outbox`) and retained (tinted done/error) until `/new`·`/clear` or the next task. Layout is now robot (24) · pipeline (fill) · tasks (32).
- [x] 🧪 unit: `task_summary` (first line, whitespace collapse, 48-char truncate), `Team::set_last_task`; render smoke tests assert the panel shows a task and degrades at narrow width

## M5 — Chat & streaming (`src/ui/chat.rs`)

- [x] 🧩 SPIKE: streaming event names — resolved from the installed binary: `session.text.{started,delta,ended}`, `message.part.{delta,updated,removed}`, `session.reasoning.{started,delta,ended}`, `session.execution.{started,succeeded,failed}`, `session.step.*`, `session.tool.*`
- [x] 🧩 SPIKE: `create_session` response is wrapped in `{"data":{…}}` (was a `{id}` assumption) — fixed with `DataEnvelope`
- [x] 💻 Create OpenCode session on `Enter` (workdir from `config.workdir`, else cwd)
- [x] 💻 Input box (single-line) → send prompt → `POST /api/session/{id}/prompt`
- [x] 💻 Stream SSE deltas into a live transcript (`session.text.delta` / `message.part.delta`), then replace with the canonical `GET …/message` transcript on completion (tolerant double-write)
- [x] 💻 Status transitions fuel the avatar (reasoning→thinking, execution.started→working, succeeded→done, failed→error)
- [x] 💻 `messages()` client + `SessionMessage`/`Part`/`MessageError` models (confirmed against a real payload)
- [x] 🧪 unit: delta assembly in order, session scoping, status transitions, transcript mapping
- [x] 🧪 integration: `tests/opencode_fake.rs` (axum) drives the client over a scripted SSE server + `/message`
- [ ] ✅ full live conversation loop (type → send → watch stream → result) — **blocked**: model inference hit the weekly budget cap (reset 2026-10-19); the loop is exercised via the fake server + unit tests until then

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

- [ ] 💻 `task.rs` — `TaskId` newtype, `TaskStatus` state machine, `try_retry` (cap 3)
- [ ] 💻 `persistence.rs` — JSON store (tasks + memory) via `dirs`, atomic single-write
- [ ] 💻 Wire `wait` endpoint as completion signal; detect failure → retry preamble
- [ ] 💻 `scheduler.rs` — `tokio-cron-scheduler` from `[[schedules]]` → dispatch
- [ ] 💻 Watch-hook (optional): file-change trigger → dispatch (`notify` crate)
- [ ] 🧪 unit: task state transitions (Pending→Running→Succeeded/Failed/Retrying)
- [ ] 🧪 unit: retry cap; persistence round-trip
- [ ] ✅ scheduled task fires autonomously, survives restart via persistence

## M7 — Orchestration (multi-employee fan-out)

- [x] 💻 Orchestrator: split one goal across `N` employees → parallel sessions (`tokio::spawn` + `JoinSet`)
  - `@parallel <goal>` (aliases `@all`/`@team`/`@fanout`) fans a goal out to every teammate but the source and the aggregator; each worker gets its own session created in parallel. The `/parallel <goal>` chat command triggers the same from the user.
  - `src/workflow.rs` (`is_parallel`, `strip_directives`) + `src/app.rs` (`fan_out`, `FanOut` state).
- [x] 💻 Handoff: employee output fed as input to the next (role chain)
  - Peer-to-peer delegation protocol: an assistant reply ends with line-leading `@<id> <instruction>` directives; the app relays each to the named employee (creating their session on demand). `@user`/`@deliver`/`@done` marks the final answer, routed back to the origin chat.
  - `src/workflow.rs` (`parse_delegations`, `is_final`, `team_context`) + app dispatch (`dispatch_delegations`/`send_to_employee`/`flush_outbox`), idempotent per message id, capped at 64 hand-offs to stop `A → B → A` cycles.
- [x] 💻 First prompt to each employee carries a team-context preamble (roster + protocol) so anyone can delegate to anyone.
- [x] 💻 `form.created` auto-answer: the event loop resolves session forms via Jev (`JevClient::answer_field`) with a deterministic `default_answer_field` fallback, so delegated sessions never hang waiting on a human. (OpenCode v2 renamed `question` → `form`; the reply endpoint is `POST /api/session/{id}/form/{formID}/reply`.)
- [x] 💻 Jev as a delegated answerer: `@jev <question>` routes to Jev (`JevClient::answer_text`, a binary yes/no `choice`) and posts the verdict back to the asking employee's chat. The pipeline diagram (`src/ui/pipeline.rs`) renders a top-down tree — planner top, “jev” middle, researcher/writer/reviewer on the bottom row — with an on/off state.
- [x] 💻 Aggregate results into a single deliverable view + review pass (`reviewer`)
  - Once every worker in a fan-out finishes (`note_fanout_result`), their stripped deliverables are collected and handed to the aggregator — the reviewer, else the planner — which synthesizes one deliverable and `@user`s it back (`aggregate_fanout`).
- [x] 🧪 integration: fake server verifies N parallel prompts + aggregation
  - `tests/opencode_fake.rs::fans_out_n_parallel_prompts_via_joinset` drives `N` concurrent `create_session`+`prompt` calls over a `JoinSet` against a counting fake server.
- [ ] ✅ one goal → team works in parallel → aggregated result — **blocked**: live model inference still under the weekly budget cap (resets 2026-10-19); the end-to-end chain is exercised via unit + fake-server tests until then.

## M8 — Polish & ship

- [ ] 💻 Help popup + keybinding footer (q quit, ↑↓ navigate, Enter chat, d dispatch, r retry, t tab)
- [ ] 💻 Tabbed layout: Dashboard / Tasks / (Chat is modal-on-Enter)
- [ ] 💻 Theming/colors consistent (cyan/green/red/yellow-dim per architecture §5)
- [ ] 💻 `tui-shimmer` idle "waiting for work…" states
- [ ] 📄 `README.md` — install, config, keybindings, OpenCode setup, screenshots(gif)
- [ ] 📄 Sample `.opencode/agents/*.md` + `opencode.jsonc` checked in
- [ ] ✅ `cargo fmt --check`
- [ ] ✅ `cargo clippy --all-features -- -D warnings` clean
- [ ] ✅ no `unwrap()` outside tests (colors-eyre context everywhere)
- [ ] ✅ terminal restored on all exit paths (incl. panic)
- [ ] ✅ `cargo build --release` succeeds; binary size noted

---

## Definition of done (each milestone)

- [ ] Compiles with `cargo build`
- [ ] `cargo clippy` clean, `cargo fmt` applied
- [ ] Focused unit/integration tests pass
- [ ] Behavior verified manually in the TUI
- [ ] Checklist row(s) flipped to `[x]`

## Open risk register

| Risk | Status | Mitigation | Owner milestone |
|---|---|---|---|
| OpenCode auth/discovery differs on macOS | ✅ Resolved | `Basic opencode:<password>` + `opencode service status` discovery confirmed | M1 |
| SSE event schema is undocumented/experimental | ✅ Resolved (shape) | `data: {json}\n\n` + `: heartbeat` confirmed; generic `Event` tolerates unknown fields | M1 |
| Message-text streaming event names | ✅ Resolved | Extracted from the installed binary: `session.text.*`, `message.part.{delta,updated,removed}`, `session.reasoning.*`, `session.execution.*`, `session.step.*`; plus canonical-poll fallback | M5 |
| `create_session` response `{"data":{…}}` envelope | ✅ Resolved | Verified live; `DataEnvelope<T>` added | M5 |
| `experimental/session/{id}/wait` signature drift | ⚠️ Open | Fallback to polling `GET /api/session/{id}/message` | M6 |
| Live model inference blocked by weekly budget | ⚠️ Open | `$10.03/$10.00` used (resets 2026-10-19). M5 verified via fake-server integration + unit tests; re-run live conversation once budget resets | M5 |
| No Rust SDK (by design) | ✅ Resolved | Thin `opencode` module isolates the HTTP surface | M1 |