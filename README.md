# mywaker

> Run a team of **AI employees** from your terminal.

**mywaker** is a native Rust terminal app that turns [OpenCode](https://opencode.ai)
into a roster of on-screen employees — each with an animated robot avatar, a live
task stream, and an explicit model selector that lets you assign a model to every
employee.

```
┌─ mywaker · project: acme ───────────────────────────────────────┐
│       ┌─[◌ ◌]─┐                                                 │
│       │  ───  │   ┌─ PLANNER ─┐   ┌ RESEARCHER ─┐   ┌ WRITER ┐ │
│       ├──┬─┬──┤   │  ● Dex    │─▶ │  ◔ Aria     │─▶ │○ Bruno │ │
├────────┴───────┴──┴───────────┴───┴─────────────┴───┴────────┴─┤
│  log ┃ stream 4 agents · 132 events · connected                │
├─────────────────────────────────────────────────────────────────┤
│  ←/→ stage │ Enter chat │ p projects │ q quit                   │
└─────────────────────────────────────────────────────────────────┘
```

The team renders as a **workflow pipeline** — a left → right diagram ordered
plan → research → write → review. Each stage is a box (name, role, live status,
model) that lights up in its status colour (yellow thinking, cyan working,
green done) while the robot avatar mirrors the aggregate state.

## What it does

- **Workflow pipeline** — the team rendered as a left → right diagram (plan →
  research → write → review), one stage box per employee with live status +
  model; the running stage lights up in its status colour.
- **Live task board** — a right-hand panel listing every employee with their
  latest task and status, updated as work streams in (per-employee `last_task`).
- **Animated robot avatar** — a state-driven ASCII robot that idles, thinks,
  works, and wraps up in real time as streaming deltas arrive.
- **Chat & live streaming** — talk to an employee and watch the response stream
  in over SSE, edge-cased to the parts OpenCode actually emits.
- **Model selector** — list the models OpenCode's providers expose
  (`mywaker model list`) and assign one per employee (`mywaker model set` or the
  TUI picker via `m`). The chosen model is persisted to `team.toml` and applied
  to that employee's sessions.

## Requirements

- Rust `1.88+` (edition 2024)
- A running OpenCode v2 background service (`opencode service start`)

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
mywaker employee skill qa playwright     # attach an installed skill
mywaker employee unskill qa playwright   # detach a skill
mywaker employee rm qa

# skills (delegates to `npx skills`)
mywaker skills find react testing        # search the ecosystem (interactive)
mywaker skills list                      # list installed skills
mywaker skills add vercel-labs/agent-skills@react-best-practices -g  # install
```

## Controls

| Key | Action |
|---|---|
| `↑` / `↓` or `j` / `k` | Select an employee |
| `Enter` | Chat with the selected employee |
| `m` | Pick a model for the selected employee |
| `Esc` | Back / leave chat |
| `q` | Quit |

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

## Architecture

TEA (The Elm Architecture): `Model → Message → Update → View`, driven by a
`tokio::select!` event loop that combines crossterm input, OpenCode SSE, and a
tick. See [`docs/architecture.md`](docs/architecture.md) for the full breakdown,
[`PLAN.md`](PLAN.md) for the vision and roadmap, and
[`docs/todos.md`](docs/todos.md) for the milestone checklist.

## License

[MIT](LICENSE)