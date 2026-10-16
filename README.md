# mywaker

> Run a team of **AI employees** from your terminal.

**mywaker** is a native Rust terminal app that turns [OpenCode](https://opencode.ai)
into a roster of on-screen employees — each with an animated robot avatar, a live
task stream, and a smart router that dispatches every task to the best (and most
cost-efficient) model across your existing AI providers.

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

## What it does

- **Employee roster** — a team of AI employees (researcher, writer, reviewer,
  planner) mapped 1:1 to OpenCode agents, each shown with live status.
- **Animated robot avatar** — a state-driven ASCII robot that idles, thinks,
  works, and wraps up in real time as streaming deltas arrive.
- **Chat & live streaming** — talk to an employee and watch the response stream
  in over SSE, edge-cased to the parts OpenCode actually emits.
- **Cross-provider model routing** — a built-in router reads Kenari's public
  catalog plus your existing OpenCode providers, normalizes them, and scores
  every model by capability vs. cost. Task complexity (simple → cheap,
  hard → top-tier) and employee role set the budget band.

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
cargo run            # launch the TUI
cargo run -- models  # route each employee to its best model
cargo run -- route "research the best async runtime"  # pick a model for a task
```

## Controls

| Key | Action |
|---|---|
| `↑` / `↓` or `j` / `k` | Select an employee |
| `Enter` | Chat with the selected employee |
| `Esc` | Back / leave chat |
| `q` | Quit |

## Cross-provider routing

The router (`src/kenari/`) merges models from **Kenari's public catalog**
(`GET /v1/models`, IDR pricing) and your **existing OpenCode providers**
(`GET /api/model`, USD → IDR) into one normalized list. It filters to
chat-capable tool-using models and ranks them on a weighted score that balances
capability (provider tier + reasoning + context + vision) against cost:

```bash
$ mywaker models
Aria (researcher) → anthropic/claude-opus-5.5 [anthropic] — researcher/medium: reasoning, vision · Rp64000/1M in
Bruno (writer)   → openai/gpt-6-luna-pro      [openai]    — writer/medium: reasoning, vision · Rp1600/1M in
Cleo (reviewer)  → openai/gpt-6-luna-pro      [openai]    — reviewer/medium: reasoning, vision · Rp1600/1M in
Dex (planner)    → anthropic/claude-opus-5.5 [anthropic] — planner/medium: reasoning, vision · Rp64000/1M in
```

## Architecture

TEA (The Elm Architecture): `Model → Message → Update → View`, driven by a
`tokio::select!` event loop that combines crossterm input, OpenCode SSE, and a
tick. See [`docs/architecture.md`](docs/architecture.md) for the full breakdown,
[`PLAN.md`](PLAN.md) for the vision and roadmap, and
[`docs/todos.md`](docs/todos.md) for the milestone checklist.

## License

[MIT](LICENSE)