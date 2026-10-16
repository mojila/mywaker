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
- **A team, not one chat box** — an orchestrator fans one goal out across multiple
  employees (parallel OpenCode sessions) and aggregates the result.

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
| Animation | Braille spinner + state-driven ASCII (self-contained) | `tui-shimmer` deferred; swap in if we want richer shimmer |

## 3. Architecture (overview)

```
crossterm event stream ──┐
OpenCode SSE stream ─────┤──► tokio::select! ──► Message ──► App::update ──► View
cron scheduler ──────────┘
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
- **M6 — Harness**: task queue, retry, persistence, scheduler (autonomous).
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