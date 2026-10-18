# mywaker — Acceptance Criteria: Pause / Continue tasks

> **Status: frozen spec.** Companion to [`PLAN.md`](../PLAN.md),
> [`architecture.md`](architecture.md), and [`todos.md`](todos.md).
>
> Scope: the "pause / continue tasks" feature (milestone **M6.5**). This file is the
> fixed verification contract — the reviewer checks against it, not against out-of-band
> context. Do not weaken a criterion to make a build pass; change the spec deliberately
> and record the change below.

---

## 1. Scope

Pause and continue **autonomous tasks** (`[[schedules]]` cron fires and `[[watches]]`
file-change fires) at two levels:

- the **queue as a whole** — an engine gate that holds new work; and
- an **individual task** — in-flight or queued.

The cron scheduler and file watcher are **unchanged**: they keep emitting events and the
app queues. Interactive chat turns and delegation hand-offs are **out of scope**.

---

## 2. Frozen acceptance criteria

| # | Criterion | How to verify | Pass condition |
|---|---|---|---|
| **AC1** | A cron/watch fire while paused lands as `pending` in the task queue and starts **no** session. | Pause the engine, then trigger a schedule/watch. Inspect `store.json` and the tasks panel. | Task recorded with status `pending`; no new OpenCode session created; log shows `⏸ queued (paused)`. |
| **AC2** | Resuming the engine dispatches queued tasks in queue order. | Queue ≥ 2 tasks while paused, then resume (`Space`). | Every queued task runs; dispatch order matches `order_tasks()` (active work first). |
| **AC3** | Pausing a running task interrupts its session, marks it `paused`, and a **late** attempt-finish does not flip it. | `s` on a running task; allow the interrupt's trailing `TaskAttemptFinished` to arrive. | Task stays `paused`; the session is interrupted; no late `succeeded`/`failed` overwrite. |
| **AC4** | Resuming a paused task returns it to `running` and re-dispatches. | `s` again on a paused task. | Status `running`; a fresh attempt starts, prefixed with the resume preamble. |
| **AC5** | The engine pause state survives a restart. | Pause, quit, relaunch. | Still paused on start, loaded from `store.json` (`paused: true`). |
| **AC6** | The build is clean. | `cargo test` · `cargo clippy` · `cargo fmt --check`. | All three green. |

---

## 3. Non-goals

- Pausing the cron scheduler or file watcher themselves (the app queues instead).
- Pausing interactive chat turns or delegation hand-offs.
- Bulk / multi-select pause, or CLI `pause` / `resume` subcommands.

---

## 4. Evidence log

The reviewer fills this in. Replace `verdict` with `PASS` or `FAIL` and cite the exact
evidence — a command and its output, a `store.json` entry, or a rendered screen.

| Criterion | Verdict | Cited evidence |
|---|---|---|
| AC1 — fire while paused → `pending`, no session | PASS | `handle_autonomous_task` (`src/app.rs:2273`) reads `self.store.paused` (`:2284`) and **returns before dispatch**: the fire is upserted `Pending`, persisted, and logged `⏸ queued (paused)` (`:2285-2290`) — no session started. Test `paused_engine_queues_a_fire_without_dispatching` (`src/app/tests.rs:1142`) asserts `status == Pending`, `session_id == None`, and the `⏸ queued (paused)` log line; green. |
| AC2 — resume dispatches queued tasks in order | PASS | `Space` → `toggle_engine_pause` (`src/app.rs:547`, `:2422`) → `resume_pending` (`:2436`) dispatches `resumable_tasks` (`:2460`), which is `order_tasks` (`:2982`; active-first, then newest-first) filtered to `Pending && !paused`. Tests `resuming_the_engine_dispatches_queued_tasks_in_queue_order` (`src/app/tests.rs:1167`: 2 queued → both `Running` after resume) and `resumable_tasks_follow_order_tasks_and_skip_held_work` (`:1190`: order `["newer","older"]`, individually-paused + terminal excluded); both green. |
| AC3 — pause interrupts session; late finish does not flip it | PASS | `s` → `toggle_task_pause` (`src/app.rs:548`, `:2472`) → `pause_task` (`:2507`): `Task::pause` sets `Paused` + `paused = true` (`src/task.rs:144`) and interrupts the session (`src/app.rs:2512`). **Trailing finish guard:** `handle_task_attempt` (`:2374`) returns early for a paused task, logging `⏸ ignored late attempt for paused task` (`:2379`); a late `TaskAttemptStarted` is interrupted, not adopted, via `note_task_attempt` (`:2356`). Test `pausing_a_running_task_survives_a_late_attempt_finish` (`src/app/tests.rs:1219`) delivers a trailing `Message::TaskAttemptFinished { succeeded: true, result: "done anyway" }` and asserts the task stays `Paused` with `result.is_none()`; `a_paused_task_does_not_adopt_a_late_session` (`:1259`) covers the late-session edge. Both green. |
| AC4 — resume a paused task → `running` + re-dispatch | PASS | `resume_task` (`src/app.rs:2525`): `Task::resume` returns `Running` and clears `paused` (`src/task.rs:154`), then `dispatch_task(&task, Some(harness::resume_preamble()))` (`:2539`; preamble at `src/harness.rs:80`). Test `resuming_a_paused_task_returns_it_to_running` (`src/app/tests.rs:1278`) asserts status `Running`, `!paused`, and the `▶ resumed task` log; green. |
| AC5 — pause state survives restart | PASS | `Store.paused` (`src/persistence.rs:28`) is `#[serde(default)]`; `toggle_engine_pause` persists it (`src/app.rs:2430`) and `enable_persistence` reloads `store.json` at startup (`:376-380`). Test `engine_pause_survives_a_round_trip` (`src/persistence.rs:129`) writes a store with `paused: true`, reloads it, and asserts `loaded.paused` — the reloaded `paused: true`; legacy JSON with no key → `false`. Task-level `paused` also round-trips (`src/task.rs:331`). Green. |
| AC6 — `cargo test` · `clippy` · `fmt --check` clean | PASS | At HEAD `7ca21ee` + working tree: `cargo test` → 202 unit + 11 integration passed, 0 failed (2 ignored live-smoke); `cargo clippy --all-features -- -D warnings` → exit 0, zero warnings; `cargo fmt --check` → exit 0. Note: `cargo fmt --check` was transiently **red** during this verification window, confined to another agent's in-flight `src/agent.rs` + `src/main.rs` (`EmployeeCmd::Agent`/`agent::ensure`); it is green as recorded. |

**Verdict: M6.5 PASSES AC1–AC6.** No criterion failed, so there is no weakening to note — the criteria stand as written. Verified at HEAD `7ca21ee` plus the working tree on 2026-10-18. Caveat: the tree was live during verification (a second checkout is open at `/private/tmp/mywaker-verify`, and the other agent's `src/agent.rs`/`src/main.rs` changed mid-run), so this log pins a point-in-time snapshot, not a commit. `docs/acceptance.md` is itself still untracked.
