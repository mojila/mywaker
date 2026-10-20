# mywaker

> Run a team of **AI employees** from your terminal.

**mywaker** is a native Rust terminal app that turns [OpenCode](https://opencode.ai)
into a roster of on-screen employees — each with an animated robot avatar, a live
task stream, and an explicit model selector that lets you assign a model to every
employee.

```
 mywaker  a team of AI employees                                                                   ⠋ connecting…
┌ waker ───────────────┐ team · 4 · 2 working                                   ┌ tasks ───────────────────────┐
│  ⠋   ◉   ◉           │jev ◆ off                                               │⠋ Dex · Planner               │
│  ┌─⠋─┐               │┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄│  plan the migration          │
│MYWAKER               │ ◆ working · 2                                          │                              │
│  └─────┘             │┆┌ PLANNER ──────┐ ┆┌ WRITER ───────┐                   │● Aria · Research lead        │
│                      │┆│⠋ Dex          │ ┆│▶ Bruno        │                   │  —                           │
│now ⠋ connecting      │┆│Planner        │ ┆│Writer         │                   │                              │
│                      │┆│thinking       │ ┆│working        │                   │▶ Bruno · Writer              │
│who ⠋ Dex             │┆│none           │ ┆│none           │                   │  —                           │
│Planner               │┆└───────────────┘ ┆└───────────────┘                   │                              │
│none                  │┆                  ┆                                    │● Cleo · Reviewer             │
│— · session —         │ ◇ idle · 2                                             │  —                           │
│                      │┆┌ RESEARCHER ───┐ ┆┌ REVIEWER ─────┐                   │                              │
│pulse ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁│┆│● Aria         │ ┆│● Cleo         │                   │                              │
│                      │┆│Research lead  │ ┆│Reviewer       │                   │                              │
│vitals queue empty    │┆│idle           │ ┆│idle           │                   │                              │
│no finished work      │┆│none           │ ┆│none           │                   │                              │
└──────────────────────┘┆└───────────────┘ ┆└───────────────┘                   └──────────────────────────────┘
 to  Dex · thinking ─────────────────────────────────────────────────────────────────────────────────◆ 2 active
>
⠋ thinking
┌ log ─────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ stream 0 agents · 0 events                                                                                   │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
 ↑↓←→ move  wheel scroll  i goal  Enter chat  p projects  m model  Space pause  s task  Ctrl-Y copy log  ? help
```

The team renders as an **employee grid** — three cards per row, ordered plan →
research → write → review. Each card is a box (name, role, live status, model,
the classified tool it is running) that lights up in its status colour (yellow
thinking, cyan working, green done) while the sidebar's robot mirrors the
aggregate state. Jev, the shared question answerer, is a one-row header above the
grid rather than a card of its own.
The grid is split in two: employees who are **working** sit on top under a
`◆ working` heading, and everyone else sits below under `◇ idle`, so the top of
the panel always answers "who is busy right now".
`i` (or `Tab`) puts a goal bar under the panels: what you type there goes to the
selected employee, and its answer comes back to the log without opening a chat.

## What it does

- **Employee grid** — the team as a fixed three-column grid (one column on a
  narrow panel), ordered plan → research → write → review within each of two
  sections — `◆ working` above, `◇ idle` below — one card per employee with live
  status + model, plus the **tool** it is running, classified: `tool shell` for a
  built-in, `tool skill:opencode`, `tool subagent:backend`, or the bare
  `tool db-audit.run_select_query` for a namespaced MCP/plugin tool (no `mcp:` or
  `plugin:` prefix is claimed — the event cannot tell those apart, so the row says
  only what it knows). A dim `tool -` stands in before the employee calls
  anything; the row is cleared when the next turn starts and by `/new`, and kept
  through `done`/`error` so a finished card still says what it did; a running
  employee lights up in its status colour and moves to
  the top section. The wheel scrolls the grid, the arrows move the selection and
  reveal it, and
  the title reads `team · N · rows a-b/c` while the roster overflows.
- **Live task board** — a right-hand panel listing every employee with their
  latest task and status, updated as work streams in (per-employee `last_task`).
- **Inspector sidebar** — the left column carries more than the avatar: a compact
  robot over an aggregate "now" line, the selected employee's model and provider,
  an activity sparkline, queue vitals, uptime with Jev's state, and any alerts.
  Sections drop whole as the terminal gets shorter, so a 24-row window still shows
  the team's state without clipping (spec:
  [`docs/acceptance-sidebar.md`](docs/acceptance-sidebar.md)).
- **Animated robot avatar** — a state-driven ASCII robot that idles, thinks,
  works, and wraps up in real time as streaming deltas arrive.
- **Chat & live streaming** — talk to an employee and watch the response stream
  in over SSE, edge-cased to the parts OpenCode actually emits.
- **Model selector** — list the models OpenCode's providers expose
  (`mywaker model list`) and assign one per employee (`mywaker model set` or the
  TUI picker via `m`). The chosen model is persisted to `team.toml` and applied
  to that employee's sessions.
- **Mouse or keyboard** — click a card or picker row to select it, wheel to
  scroll the grid or page a transcript; every control also has a key, so the TUI
  is fully usable either way.
- **Autonomous triggers** — cron schedules (`[[schedules]]`) and file-change
  watch hooks (`[[watches]]`) fire tasks at employees with no chat required. Each
  task persists across restarts and retries up to three times on failure.

## Requirements

- Rust `1.88+` (edition 2024)
- An OpenCode v2 background service. On startup mywaker will start it
  automatically if it isn't running (`opencode service start`), showing a
  spinner until it's ready.

## Install

Either with plain Cargo:

```bash
cargo build --release
```

or via the `Makefile` (`make build` + `make install`):

```bash
make build        # build the release binary
make install      # install to a system $PATH dir (uses sudo when required)
make install-user # install to ~/.local/bin (no root)
make info         # show the detected platform & install path
```

`make install` picks a sensible default prefix per platform — `/opt/homebrew`
(macOS arm64 with Homebrew), `/usr/local` (macOS Intel / Linux) — and works on
macOS, Debian/Ubuntu, Fedora, and Arch. Override it with `PREFIX`/`DESTDIR`:

```bash
make install PREFIX=/custom/path       # install somewhere else
make install DESTDIR=/tmp/stage        # stage for packaging
```

Distro packages are also available via `make pkg-deb` (Debian/Ubuntu),
`make pkg-rpm` (Fedora/RHEL), and `make pkg-arch` (Arch) — run each on its
native distro.

## Run

```bash
cargo run                  # launch the TUI
mywaker model list         # list models from OpenCode's providers
mywaker model set qa openai/gpt-6-luna-pro   # assign a model to an employee
```

## Manage employees & skills

The roster is persisted to `team.toml` (under your config dir) and used by both
the TUI and the CLI.

```bash
# employees
mywaker employee list
mywaker employee add qa -n Quinn -r Tester -m openai/gpt-6-luna-pro
mywaker employee set qa --role "QA lead"
mywaker employee agent qa                # (re)write the agent definition: qa.md
mywaker employee skill qa playwright     # attach an installed skill
mywaker employee unskill qa playwright   # detach a skill
mywaker employee rm qa

# skills (delegates to `npx skills`)
mywaker skills find react testing        # search the ecosystem (interactive)
mywaker skills list                      # list installed skills
mywaker skills add vercel-labs/agent-skills@react-best-practices -g  # install
```

### Agent definitions (roles)

Each employee maps 1:1 to an OpenCode agent whose id matches the employee id.
Define role-specific system prompts so every employee runs with its own role
instead of the shared default.

Hiring an employee **writes the definition for you**: `mywaker employee add qa`
also creates `~/.config/opencode/agents/qa.md` (if it doesn't already exist), so a
new employee never hits `Agent not found`. The write goes to OpenCode's *global*
agents dir — `$XDG_CONFIG_HOME/opencode/agents/`, else `~/.config/opencode/agents/`
— which matches the global roster in `team.toml`. Canonical ids
(`planner`/`researcher`/`writer`/`reviewer`/`engineer`) get the documented role
prompt, and two ids carry a mandate of their own: the `modeler` asset creator gets
the 3D one, and the `designer` gets the UI/UX one (specs and reviews — layout,
hierarchy, colour and type scales — with implementation handed to the writer).
Every other id gets a generic identity prompt you can edit — `image` included,
deliberately: 2D asset work sits outside both mandates.

```bash
mywaker employee add qa -n Quinn -r Tester   # writes ~/.config/opencode/agents/qa.md
mywaker employee agent qa                     # re-create it if it's missing
mywaker employee agent qa --force             # overwrite a hand-edited definition
```

Ready-to-copy definitions are also checked in under [`examples/`](examples/) if you
prefer to write your own:

- [`examples/opencode.agents.jsonc`](examples/opencode.agents.jsonc) — the
  `planner` / `researcher` / `writer` / `reviewer` / `designer` agents as an
  `opencode.jsonc` `agents` block you can drop in or merge.
- [`examples/agents/*.md`](examples/agents) — the same roles as individual
  markdown agent files, plus the `modeler` asset-creator mandate and the
  `designer` UI/UX mandate. Copy them into `~/.config/opencode/agents/` (global,
  available in every project) or a project's `.opencode/agents/` (project-local).
  The `modeler` and `designer` files are byte-identical to the mandates baked into
  `mywaker` (`src/agent.rs`), and a unit test keeps each pair that way.

If an employee's agent id still isn't defined anywhere, the employee starts a
session anyway — it falls back to OpenCode's default agent (with a note in the log)
rather than failing with `Agent not found`.

## Controls

| Key | Action |
|---|---|
| `←` / `→` or `h` / `l` | Move one card left / right in the employee grid |
| `↑` / `↓` or `j` / `k` | Move one card row up / down (same column) |
| `PgUp` / `PgDn` | Move a viewport of cards (the selection follows and is revealed) |
| `Enter` | Chat with the selected employee |
| `i` / `Tab` | Type a goal for the selected employee without leaving the roster |
| `m` | Pick a model for the selected employee |
| `p` | Choose the working project |
| `Space` | Pause / resume the autonomous-task engine (new cron/watch fires queue) |
| `s` | Pause / resume the selected employee's task |
| `y` / `n` (reopen prompt) | Resume all / keep paused the tasks mywaker held at the last close |
| `?` | Open the keybindings help overlay |
| `Esc` | Back / leave chat |
| `q` / `Ctrl-C` | Quit |
| Click | Select the employee, project, or model you clicked |
| Wheel | Scroll the employee grid, or page the chat transcript |

Mouse capture is on, so the terminal's own selection is bypassed — hold `Shift`
while dragging to copy text as usual. Clicking only moves the selection: opening
a chat, adopting a project, and assigning a model still take `Enter`, so a stray
click can't change anything. The wheel scrolls the grid's viewport without moving
the selection, so looking around never changes what `Enter` would open.

## Model selection

Each employee's model is an explicit choice, not an automatic route. The model
selector lists the models your **existing OpenCode providers** expose
(`GET /api/model`) — exactly what the local service can run — and assigns one to
an employee. The assignment is saved to `team.toml` and passed to the session
when that employee's chat starts:

```bash
$ mywaker model list
  openai/gpt-6-luna-pro      [openai]    GPT-6 Luna Pro      $1.60/1M in
✓ anthropic/claude-opus-5.5  [anthropic] Claude Opus 5.5     $64.00/1M in
  meta/muse-spark-open       [openrouter] Muse Spark Open    $1.25/1M in

$ mywaker model set writer openai/gpt-6-luna-pro
assigned model 'openai/gpt-6-luna-pro' to 'writer'
```

In the TUI, press `m` on the roster to open the picker, highlight a model, and
press `Enter` to assign it to the selected employee. Type to filter the list by
id, name, provider, or vendor (`Ctrl-U` clears the query, `Esc` returns).

### Optional Jev answerer

Jev (System One) is Kenari's typed decision model. mywaker uses it in two spots —
answering `@jev <question>` delegations with a yes/no verdict, and auto-answering
OpenCode `form.created` prompts so sessions never stall on human input. It is
*opt-in*: configure any System One-compatible endpoint from
`~/.config/mywaker/config.toml`:

```toml
[jev]
endpoint = "https://kenari.id/v1/systemone"   # any System One-compatible endpoint
model = "jev-1-13-free"                        # decision model id
api_key = "kn-…"                               # or leave unset to use KENARI_API_KEY
```

`endpoint`, `model`, and `api_key` are all optional and default to Kenari's public
endpoint; they can also be set via `MYWAKER_JEV_ENDPOINT`, `MYWAKER_JEV_MODEL`, and
`MYWAKER_JEV_API_KEY`. With no API key available, Jev is simply disabled.

## Autonomous triggers

Employees can start on their own — no chat required. There are two triggers:
**cron schedules** and **file-change watch hooks**. Add either to
`~/.config/mywaker/config.toml`.

### Cron schedules

`cron` takes the familiar 5-field crontab form — `minute hour day-of-month month
day-of-week` — so `0 9 * * 1-5` means 09:00 Monday–Friday. Seconds are implicitly
`0`. The scheduler parses seconds-first, so a 6-field expression (with a leading
`second`) or a 7-field one (six fields plus a trailing `year`) also works and is
used as written. Any other field count is skipped at startup with a warning.

```toml
[[schedules]]
cron = "0 9 * * 1-5"          # 09:00 Mon–Fri (5-field: minutes first)
employee = "planner"
goal = "Summarize yesterday's commits into a morning briefing"

[[schedules]]
cron = "0/15 * * * *"
employee = "researcher"
goal = "Watch the build for regressions and report any failures"
```

### File-change watch hooks

A watch fires when a file or directory changes — e.g. have the reviewer look at
every save. `path` may start with `~`; `recursive` defaults to `true`. The `goal`
is a template: `{path}` is replaced with the path that changed. Editor saves emit
a burst of events, so changes are debounced (coalesced into a single dispatch).

```toml
[[watches]]
path = "~/Projects/mywaker/src"
employee = "reviewer"
goal = "Review the change to {path} for regressions and style issues"
recursive = true

[[watches]]
path = "~/.config/mywaker/config.toml"
employee = "planner"
goal = "Validate the config change in {path}"
```

When a trigger fires, mywaker creates a task, starts a session for that employee,
streams the goal, and reports the deliverable in the log. A task that fails is
retried up to three times; the retry preamble tells the employee what broke. Task
history (and per-employee memory) persists to `store.json` under your config dir,
so it survives restarts.

### Pausing work

Holding work is a first-class action, and mywaker holds it for you when you quit:

- **`Space`** pauses the *engine*. A cron or watch fire while the engine is paused is
  still recorded, but it lands as `pending` and starts no session; pressing `Space`
  again dispatches everything that queued up, in queue order.
- **`s`** pauses or resumes the selected employee's *current* task — pausing interrupts
  the session that task was running in, resuming re-runs it.
- **Quitting pauses every unfinished task** and interrupts the sessions they were running
  in, so no headless work keeps going after the terminal is gone. The pause is written to
  `store.json` on the way out, so it survives the restart.
- **The next launch offers those tasks back** in one modal: `y` / `Enter` resumes them
  all, `n` / `Esc` keeps them held (press `s` later to continue one). Only the tasks
  mywaker held itself are offered — anything you paused on purpose stays held. Quit is
  still immediate; there is no quit-time confirmation step.

Resuming a task **re-runs it from the beginning in a new session**: the interrupted
session's transcript is not replayed, which is why the modal says so instead of promising
a continue where you left off. Chat is the different case — re-entering an employee's chat
carries on that same session.

## Architecture

TEA (The Elm Architecture): `Model → Message → Update → View`, driven by a
`tokio::select!` event loop that combines crossterm input, OpenCode SSE, and a
tick. See [`docs/architecture.md`](docs/architecture.md) for the full breakdown,
[`PLAN.md`](PLAN.md) for the vision and roadmap,
[`docs/todos.md`](docs/todos.md) for the milestone checklist, and
[`RELEASING.md`](RELEASING.md) for how a version is cut.

## License

[MIT](LICENSE)