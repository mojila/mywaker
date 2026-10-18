# mywaker — Acceptance Criteria: Pause / Continue tasks

> **Status: frozen spec.** Companion to [`PLAN.md`](../PLAN.md),
> [`architecture.md`](architecture.md), and [`todos.md`](todos.md).
>
> Scope: the "pause / continue tasks" feature (milestone **M6.5**) and its
> close/reopen slice (milestone **M6.6** — quit holds the queue, the next launch
> offers it back). This file is the fixed verification contract — the reviewer
> checks against it, not against out-of-band context. Do not weaken a criterion to
> make a build pass; change the spec deliberately and record the change below (§6).

---

## 1. Scope

Pause and continue **autonomous tasks** (`[[schedules]]` cron fires and `[[watches]]`
file-change fires) at two levels:

- the **queue as a whole** — an engine gate that holds new work; and
- an **individual task** — in-flight or queued.

The cron scheduler and file watcher are **unchanged**: they keep emitting events and the
app queues. Interactive chat turns and delegation hand-offs are **out of scope**.

### 1.1 The close/reopen slice (M6.6)

Quitting closes the loop between the two levels, and the next launch reopens it:

- **On quit** every **non-terminal** task (`Pending`, `Running`, `Retrying`) is paused
  with `PauseReason::Shutdown` and the session it was running in is **interrupted** —
  pause first, interrupt second, so the interrupt's trailing completion meets the
  paused-task guard (`handle_task_attempt`) instead of flipping the task. Terminal
  tasks are untouched; a task the user had already paused keeps `PauseReason::User`.
- **On start** the tasks paused *at shutdown* — and only those — are collected into one
  reopen prompt, in `order_tasks` order. A normal launch (nothing shutdown-paused) shows
  no prompt; a deliberate pause never appears in it.
- **Accepting** the prompt resumes each collected task through the ordinary per-task
  resume path; **declining** leaves them paused and logs how to continue one later. The
  engine gate is not touched either way, and **quit itself stays immediate** (there is no
  quit-time confirmation modal).

Pass conditions for the slice are the criteria numbered **AC4a**, **AC4b**, and **AC7**
below, plus the two invariants the evidence log records for the close path: a quit
pauses every non-terminal task, and only shutdown-paused tasks are ever offered back.

---

## 2. Frozen acceptance criteria

| # | Criterion | How to verify | Pass condition |
|---|---|---|---|
| **AC1** | A cron/watch fire while paused lands as `pending` in the task queue and starts **no** session. | Pause the engine, then trigger a schedule/watch. Inspect `store.json` and the tasks panel. | Task recorded with status `pending`; no new OpenCode session created; log shows `⏸ queued (paused)`. |
| **AC2** | Resuming the engine dispatches queued tasks in queue order. | Queue ≥ 2 tasks while paused, then resume (`Space`). | Every queued task runs; dispatch order matches `order_tasks()` (active work first). |
| **AC3** | Pausing a running task interrupts its session, marks it `paused`, and a **late** attempt-finish does not flip it. | `s` on a running task; allow the interrupt's trailing `TaskAttemptFinished` to arrive. | Task stays `paused`; the session is interrupted; no late `succeeded`/`failed` overwrite. |
| **AC4** | Resuming a paused task returns it to `running` and **re-dispatches it as a new attempt**. | `s` again on a paused task, or accept the reopen prompt. | Status `running`; a **new** session id is created and stored; the log shows `▶ resumed task`. Not "the previous session continues". |
| **AC4a** | Resume is **not session-continuous**. | Resume a task that had already run, then inspect the stored id. | `task.session_id != <pre-pause id>`; the fresh id comes from `create_session_with_agent_fallback`; the old id appears only in the interrupt call. |
| **AC4b** | The resume preamble is applied **iff** the task had a prior attempt. | Resume once a task that had started, and once a task that never reached a session. | Previously started → the prompt begins with `harness::resume_preamble()`; never started (`session_id == None`) → the bare goal; asserted on `run_task_attempt`'s preamble argument, not on the result string. |
| **AC5** | The engine pause state survives a restart. | Pause, quit, relaunch. | Still paused on start, loaded from `store.json` (`paused: true`). |
| **AC6** | The build is clean. | `cargo test` · `cargo clippy` · `cargo fmt --check`. | All three green. |
| **AC7** | The reopen prompt re-runs **all** shutdown-paused tasks, **in order**, each in a fresh session. | Hold ≥ 2 shutdown-paused tasks, accept the prompt. | Every shutdown-paused task resumes, in `order_tasks` order; each gets its **own** fresh session id — none adopts another's, none keeps its pre-pause id. |

---

## 3. Continuation semantics — task resume is not session-continuous

**Reopening mywaker and accepting the prompt re-runs each affected task in a brand-new
OpenCode session.** The interrupted session is only ever *interrupted*: its transcript
is not replayed, and its `session_id` is overwritten by the new attempt. There is no
session continuity across a restart — "resume" resumes the **task**, not the session.

That is why the prompt's body copy says the tasks **restart from the beginning**, and why
the "start over" preamble (`harness::resume_preamble`) is applied only to a task that had
an earlier attempt to start over *from* (AC4b). Interactive chat, by contrast, **does**
reuse its session: re-entering an employee's chat continues the same `session_id`. The
two paths are deliberately different, and nothing in the tree should describe the task
path as "picking up where you left off".

---

## 4. Non-goals

- Pausing the cron scheduler or file watcher themselves (the app queues instead).
- Pausing interactive chat turns or delegation hand-offs.
- Bulk / multi-select pause, or CLI `pause` / `resume` subcommands.
- Session-continuous task resume (see the future-work entry in [`todos.md`](todos.md)).
- A quit-time confirmation modal: quit stays immediate.

---

## 5. Evidence log

The reviewer fills this in. Replace `verdict` with `PASS` or `FAIL` and cite the exact
evidence — a command and its output, a `store.json` entry, or a rendered screen.

All `src/…:line` citations below were **refreshed to the live tree** (working tree
over HEAD `7578757`) on 2026-10-18; earlier rows no longer cite their original
verification commit.

| Criterion | Verdict | Cited evidence |
|---|---|---|
| AC1 — fire while paused → `pending`, no session | PASS | `handle_autonomous_task` (`src/app.rs:2346`) reads `self.store.paused` (`:2357`) and **returns before dispatch**: the fire is upserted `Pending`, persisted, and logged `⏸ queued (paused)` (`:2361`) — no session started. Test `paused_engine_queues_a_fire_without_dispatching` (`src/app/tests.rs:1142`) asserts `status == Pending`, `session_id == None`, and the `⏸ queued (paused)` log line; green on the live tree. |
| AC2 — resume dispatches queued tasks in order | PASS | `Space` (`src/app.rs:587`) → `toggle_engine_pause` (`:2495`) → `resume_pending` (`:2509`) dispatches `resumable_tasks` (`:2533`), which is `order_tasks` (`:3154`; active-first, then newest-first) filtered to `Pending && !paused`. Tests `resuming_the_engine_dispatches_queued_tasks_in_queue_order` (`src/app/tests.rs:1167`: 2 queued → both `Running` after resume) and `resumable_tasks_follow_order_tasks_and_skip_held_work` (`:1190`: order `["newer","older"]`, individually-paused + terminal excluded); both green on the live tree. |
| AC3 — pause interrupts session; late finish does not flip it | PASS | `s` (`src/app.rs:588`) → `toggle_task_pause` (`:2545`) → `pause_task` (`:2580`): `Task::pause` sets `Paused` + `paused = true` + `pause_reason = Some(User)` (`src/task.rs:180`, via `pause_for` `:169`) **and then** interrupts the session (`src/app.rs:2586`). **Trailing finish guard:** `handle_task_attempt` (`:2447`) returns early for a paused task, logging `⏸ ignored late attempt for paused task` (`:2454`); a late `TaskAttemptStarted` is interrupted, not adopted, via `note_task_attempt` (`:2429`). Test `pausing_a_running_task_survives_a_late_attempt_finish` (`src/app/tests.rs:1219`) delivers a trailing `Message::TaskAttemptFinished { succeeded: true, result: "done anyway" }` and asserts the task stays `Paused` with `result.is_none()`; `a_paused_task_does_not_adopt_a_late_session` (`:1259`) covers the late-session edge. Both green on the live tree. |
| AC4 — resume a paused task → `running`, re-dispatched as a **new** attempt | PASS | `resume_task` (`src/app.rs:2598`) → `Task::resume` clears the flag and reason (`src/task.rs:187`) → `dispatch_task` (`src/app.rs:2616`). The recording-double test `resuming_a_paused_task_returns_it_to_running` (`src/app/tests.rs:1393`, `#[tokio::test]`, fake OpenCode server at `src/app/tests.rs:1281`) **witnesses** the pass condition rather than inferring it from task state: it asserts status `Running`, `!paused`, the `▶ resumed task` log, the stored id **replaced** (`ses_old` → `ses_fresh`), **exactly one** fresh session minted, and the prompt it was sent is `harness::resume_preamble() + "\n\n" + goal`. The never-started case is `resuming_a_task_that_never_ran_omits_the_preamble` (`:1449`). Both AC4 tests are green. |
| AC5 — pause state survives restart | PASS | `Store.paused` (`src/persistence.rs:28`) is `#[serde(default)]`; `toggle_engine_pause` persists it (`src/app.rs:2503`) and `enable_persistence` reloads `store.json` at startup (`:401-407`). Test `engine_pause_survives_a_round_trip` (`src/persistence.rs:129`) writes a store with `paused: true`, reloads it, and asserts `loaded.paused`; legacy JSON with no key → `false`. Task-level `paused`/`pause_reason` also round-trip (`src/task.rs:372` for the flag, `src/persistence.rs:155` for the reason). Green on the live tree. |
| AC6 — `cargo test` · `clippy` · `fmt --check` clean | PASS | On the live tree (working tree over HEAD `7578757`, 2026-10-18): `cargo test` → **213 passed + 2 ignored** (215 lib) + 11 integration (5 `opencode_fake` + 5 `render` + 1 `watch_hook`; 1 `live_employees` ignored), 0 failed; `cargo clippy --all-features -- -D warnings` → exit 0; `cargo fmt --check` → exit 0. (An earlier snapshot in this window had the AC7 test red at `src/app/tests.rs:1705`; the tester fixed its prompt lookup and the tree went green.) The branch also carries a separate, **already-committed** fix at `b0fa282` — `src/opencode/models.rs`, nested `form.created` `sessionID` resolution, with its own test `form_created_is_answered_from_the_nested_payload` (`src/app/tests.rs:2466`). It is **not** a working-tree change (the tree is clean for that path) and is **not** part of this slice; see §6. |
| **Close path** — quit holds the queue (M6.6) | PASS | On quit, `initiate_shutdown` (`src/app.rs:450`) runs `pause_active_tasks` (`src/app.rs:2648`) **before** flushing the store: the `pause_for(PauseReason::Shutdown)` loop (`:2652-2665`) completes for every non-terminal task *before* any session is interrupted (`:2667-2669`), so the interrupt's trailing completion meets `handle_task_attempt`'s guard (`src/app.rs:2447`) and is ignored. Terminal tasks are skipped and a user-paused task keeps `PauseReason::User` — both because `pause_for` refuses a `Paused`/terminal state (`src/task.rs:169`; `paused_at_shutdown`, `src/task.rs:199`). Test `shutdown_holds_the_queue_but_respects_deliberate_pauses` (`src/app/tests.rs:1740`, `#[tokio::test]`) drives `initiate_shutdown` over a mixed queue (Running+session, Pending, Retrying, terminal, user-paused): the three non-terminal tasks take `PauseReason::Shutdown` (`:1779-1799`), the terminal task is untouched (`:1802-1811`), the user pause keeps `PauseReason::User` (`:1814-1823`), the double records exactly the one session interrupt (`ses_run`) (`:1827-1837`), and the trailing `TaskAttemptFinished` the test then delivers leaves the held task `Paused`/`Shutdown` (`:1848-1861`). **Residual limit — does not affect the verdict:** the strict *temporal* order of the HTTP `interrupt` call against the in-memory pause is not instrumented (the pause loop `:2652-2665` runs to completion before any interrupt is spawned, `:2667-2669` — code-read). That ordering is the mechanism, not a pass condition; both recorded invariants (quit holds every non-terminal task; only shutdown-paused tasks are offered back) and the guard's observable consequence are witnessed. Green. |
| **Open path** — only shutdown pauses are offered (M6.6) | PASS | `prepare_resume_prompt` (`src/app.rs:2682`, called from `enable_persistence` `src/app.rs:406`) collects only `paused_at_shutdown()` tasks, in `order_tasks` order, and leaves `resume_prompt` `None` when there are none — a deliberate (`PauseReason::User`) pause never qualifies. Modal precedence: shutdown overlay → reopen prompt → help (`src/ui/mod.rs:57-68`); body copy at `src/ui/modal.rs:34-40`. Declining drops the prompt and leaves the engine gate alone (`dismiss_resume_prompt`, `src/app.rs:2713`). Tests `reopen_prompt_body_carries_the_restart_disclaimer` (`src/app/tests.rs:1453`), `reopen_prompt_resumes_shutdown_pauses_but_not_deliberate_ones` (`:1478`, asserts the deliberate task stays `Paused`), `reopen_prompt_declining_keeps_the_tasks_held` (`:1507`); green. |
| **AC4a** — resume is not session-continuous | PASS | `run_task_attempt` mints the session via `create_session_with_agent_fallback` (`src/app.rs:3212`); `note_task_attempt` overwrites the stored id (`src/app.rs:2437`), so the pre-pause id survives only as an interrupt target. The recording-double test `resuming_a_paused_task_returns_it_to_running` (`src/app/tests.rs:1393`) asserts the id **changed** (`ses_old` → `ses_fresh`) against the fake client. Green. |
| **AC4b** — preamble iff there was a prior attempt | PASS | `resume_task` (`src/app.rs:2615`) applies `harness::resume_preamble()` (`src/harness.rs:80`) only when `task.session_id.is_some()`; the preamble is asserted on the **prompt the fake server received**, not on the result string. Test `resuming_a_paused_task_returns_it_to_running` (`src/app/tests.rs:1393`) asserts `preamble + "\n\n" + goal` for a started task; `resuming_a_task_that_never_ran_omits_the_preamble` (`:1449`) asserts the bare goal for a never-started one. Both green. |
| **AC7** — reopen prompt re-runs all shutdown-paused tasks, in order, each in a fresh session | PASS | `resume_shutdown_paused` (`src/app.rs:2698`) walks the prompt's ids in order and calls the ordinary `resume_task` per task. The batch test `reopen_prompt_resumes_each_task_in_order_with_a_fresh_session` (`src/app/tests.rs:1556`, `#[tokio::test]`, recording double) holds two shutdown-paused tasks, each pre-loaded with its **own** stale id, and asserts the criterion point by point: `order_tasks` is `[newer, older]` (`:1582-1586`); the collected prompt is that order (`:1589-1593`); the synchronous `▶ resumed task` dispatch sequence is that order (`:1609-1612`); both tasks end `Running` / `!paused` / `pause_reason == None` (`:1641-1660`); each stored id differs from **its own** stale id and never equals the other's (`:1675-1690`) and the two fresh ids are pairwise distinct (`:1691-1694`); and the double minted **exactly two** sessions (`:1698-1700`) whose `(session, prompt)` records prove each goal's prompt went to the session that goal stored (`:1712-1727`). **Residual limit — does not affect the verdict:** session *minting* runs concurrently inside `dispatch_task`/`run_task_attempt` (`src/app.rs:2399`, `:3211`), so the test deliberately asserts the *dispatch* order (synchronous and logged) rather than the *mint* order of the session ids; the criterion ("resumes, in order, each in a fresh session") does not require the id sequence to follow `order_tasks`. Green on the live tree. |

**M6.6 PASSES on the live tree** (working tree over HEAD `7578757`, 2026-10-18). AC1–AC7
all PASS, and both M6.6 invariants hold. **The two items my previous pass left unclear are
now closed**, each by the test the corresponding row above cites:

1. **AC7 — the ≥ 2-task batch test now exists and is verified.**
   `reopen_prompt_resumes_each_task_in_order_with_a_fresh_session`
   (`src/app/tests.rs:1556`) holds two shutdown-paused tasks with distinct stale ids and
   witnesses the whole criterion: `order_tasks` order for the prompt *and* the dispatch
   sequence, each task's own fresh session, and pairwise-distinct ids tied to each goal by
   the double's `(session, prompt)` record. The one thing it does not — and need not —
   assert is the *mint order* of the fresh ids, because session creation is concurrent; the
   criterion does not require it.
2. **Close path — the invariant is no longer code-reading only.**
   `shutdown_holds_the_queue_but_respects_deliberate_pauses` (`src/app/tests.rs:1740`)
   drives `initiate_shutdown` over a mixed queue and witnesses both recorded invariants
   (every non-terminal task held `Paused`/`Shutdown`; only shutdown-paused tasks offered
   back) plus the trailing-`TaskAttemptFinished` guard. The strict temporal order of the
   HTTP interrupt against the in-memory pause is still not instrumented — the pause loop's
   synchronous-before-spawn structure is code-read — but that ordering is not itself a pass
   condition, and the guard it exists to protect is witnessed.

**Genuinely still open: nothing the criteria require.** The two limitations above are
recorded, not waived. Build green: `cargo test` 213 passed / 2 ignored (lib) + 11
integration, 0 failed; `clippy -D warnings` exit 0; `fmt --check` exit 0.

History: AC6 and AC7 were briefly **red** in an earlier snapshot of this window, both
failing on the AC7 batch test's prompt lookup (`src/app/tests.rs:1705`), and went green when
the tester corrected that predicate with no implementation change. The M6.5 log's
"still untracked" caveat was stale and is fixed here (`docs/acceptance.md` landed at
`d426e9a`; the tree was clean at `7b9aa3f`).

Note for the record: **task resume starts a fresh session** — it does not continue the
interrupted one — which is why AC4a/AC4b exist and why the modal body says the tasks are
re-run from the beginning; **interactive chat re-entry** is the only path that reuses a
`session_id`.

---

## 6. Revision log

- **2026-10-18 — M6.6, the close/reopen slice.** A deliberate spec change, recorded here
  as this file requires. **AC4 reworded** from "…returns it to `running` and
  re-dispatches" to "…**re-dispatches it as a new attempt**", because a resume re-runs the
  task in a new session rather than continuing the interrupted one. **AC4a** (resume is not
  session-continuous), **AC4b** (preamble iff there was a prior attempt), and **AC7**
  (resume-all keeps the per-task new-session semantics) were added. The AC4 row's
  pre-slice evidence was voided and re-verified against the new pass condition; §1.1 gained
  the slice's contract, §3 the continuation-semantics paragraph, and the non-goals gained
  session-continuous resume and the explicit no-quit-modal entry.
- **2026-10-18 — reviewer re-verification (live tree).** AC4/AC4a/AC4b and the close/open
  path rows PASS; the AC4 row now cites the recording-double test at
  `src/app/tests.rs:1369` and AC1–AC6 citations were refreshed to the live tree. **Open,
  both test gaps:** AC7 (needs a ≥ 2-task batch test asserting a distinct fresh session per
  task and `order_tasks` order) and the close-path invariant (needs a test driving
  `initiate_shutdown` over a mixed queue).
- **2026-10-18 — reviewer re-verification, strengthened tests (live tree).** AC7 reworded
  to "re-runs **all** shutdown-paused tasks, **in order**, each in a fresh session"; the
  close path gained its witness
  (`shutdown_holds_the_queue_but_respects_deliberate_pauses`, `src/app/tests.rs:1740`), and
  the AC4/AC4a/AC4b rows now cite `src/app/tests.rs:1393`/`:1449`. The strengthened AC7
  batch test (`reopen_prompt_resumes_each_task_in_order_with_a_fresh_session`,
  `src/app/tests.rs:1556`) was briefly **red** at `:1705` (its prompt lookup compared the
  preamble-prefixed prompt to the bare goal), which took AC6/AC7 red for that snapshot; the
  tester corrected the predicate and both went green with no implementation change. Final
  tree: **AC1–AC7 PASS**, `cargo test` 213 passed / 2 ignored, clippy + fmt clean.
- **2026-10-18 — reviewer re-verification (live tree, HEAD `7578757`).** Re-read the two
  strengthened tests against the frozen criteria. **AC7 now PASSES**: the batch test
  (`src/app/tests.rs:1556`) satisfies the criterion as written — its non-assertion of the
  concurrent session-*mint* order is outside the criterion, not a gap. **The close-path row
  now PASSES**: `shutdown_holds_the_queue_but_respects_deliberate_pauses`
  (`src/app/tests.rs:1740`) witnesses both recorded invariants and the trailing-completion
  guard; the un-instrumented pause/interrupt *temporal* order is recorded as a limitation,
  not a pass condition. The "two items not yet clear" from the previous reviewer bullet are
  therefore **closed**. Doc base refreshed from `7b9aa3f` to HEAD `7578757`; the
  `src/opencode/models.rs` nested-`form.created` fix is clarified as **committed at
  `b0fa282`** (not a working-tree change), with its own test
  (`form_created_is_answered_from_the_nested_payload`, `src/app/tests.rs:2466`) — independent
  of this slice and not a landing blocker. AC1–AC6 verdicts unchanged; `cargo test` 213
  passed / 2 ignored (lib) + 11 integration, 0 failed, clippy `-D warnings` exit 0,
  `fmt --check` exit 0.

