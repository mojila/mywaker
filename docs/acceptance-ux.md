# mywaker — UX acceptance criteria

Frozen verification contract for the **interaction model**: how an action is
discovered, how state is made legible, and how a failure is surfaced. Companion
to [`acceptance.md`](acceptance.md) (pause/continue, M6.5–M6.6),
[`acceptance-sidebar.md`](acceptance-sidebar.md) (the sidebar and its section
budget), and the chat transcript's own file. This file owns the UX overhaul
described in [`PLAN.md`](../PLAN.md) §5 and **nothing else**.

> **Status: frozen spec — awaiting implementation. Evidence log: run 1 recorded
> (0/48 evidenced; 46 *not evidenced*, 2 *blocked*, later decided in rev 5).**
>
> The reviewer verifies against *this file*, never against a conversation turn and
> never against a test's own doc comment. A criterion is never weakened to make a
> build pass; changing one is a deliberate edit recorded in §9 (see §7).

Related: [`architecture.md`](architecture.md) §5 (styling) and §7 (terminal
safety), [`todos.md`](todos.md), and the phase plan this file freezes —
**P1 Foundation**, **P2 Ease of use**, **P3 Reliability**, **P4 Polish**.

**Tree pin (rev 1).** Every `file:line` citation below was resolved against the
working tree at hand-off, not against a commit, because this document was written
before any of its subjects existed. To make "pinned to the tree" checkable:

- Base commit: `d0756f8` (`add ui uxdesigner`); **0 files modified** on top of it.
- Content at hand-off, `shasum` prefixes: `src/app.rs` `a95d056f`,
  `src/ui/mod.rs` `4402b0e5`, `src/ui/layout.rs` `c1fdd460`,
  `src/ui/hit.rs` `6f9bfb21`, `src/ui/sidebar.rs` `36dfd841`,
  `src/ui/pipeline.rs` `a39e722e`, `src/ui/tasks.rs` `cb5a9772`,
  `src/team.rs` `fc4d28ea`.

If a citation does not resolve, compare those hashes first: a mismatch means the
tree moved after hand-off, and the line numbers should be re-derived rather than
trusted. This is the same convention as [`acceptance-sidebar.md`](acceptance-sidebar.md)
§"Tree pin".

---

## 0. This document is the target, not the status quo

The rules below state what the app **must** do. They are not a description of the
code, and a mismatch is resolved by moving the code — not by editing a rule to
match it.

That distinction has teeth here, because every criterion below **fails today**.
The measured baseline at hand-off is recorded so nobody mistakes this file for a
description of what shipped, and so the first tester run has something to diff
against:

| # | Measured today | Evidence |
|---|---|---|
| B1 | **Three sources of truth for "what can I press".** The hint bar lists 11 key pairs, the help overlay 24 key rows under 6 headings, and the chat's `/help` 10 commands — each written by hand. | `hint_bar` (`src/ui/mod.rs:331-345`), `help_lines` (`:423-471`), `CHAT_COMMANDS` (`src/app.rs:268`) |
| B2 | **The hint bar is 120 columns and is never truncated.** It is one unclipped `Line` of `" key "` + `"description "` spans, so the terminal cuts it mid-pair. At 100 columns it loses `log · ? help · q quit`; at 80 it loses `s task · Ctrl-Y copy log · ? help · q quit`; at 60 it loses everything after `m`. The help hint — the one escape hatch for a user who cannot find a key — is among the first casualties. | `hint_line` (`src/ui/mod.rs:375-382`), `("?", "help")` at `:343`; 120 is the summed span width of the roster list |
| B3 | **The help overlay cannot fit.** It is a fixed `68×30` popup clamped to the terminal, holding 35 body rows (24 key + 6 heading + 5 blank), with no scrolling and no search. At 60×16 the clamp yields a 58×14 box — **12 content rows for 35** — and the remaining 23 are silently unreachable. | `render_help_overlay` (`src/ui/mod.rs:474-483`), `centered` (`:389`), `help_lines` (`:423-471`) |
| B4 | **Bindings are scattered, not catalogued.** Production `KeyCode::` literals live in **one** file, `src/app.rs`, across 7 `match key.code` blocks and 4 `Mode` variants. Counted on the pinned tree, **by line / by occurrence**: **75 / 93** in `src/app.rs`; **4 / 4** in `src/ui/prompt.rs`, all inside `#[cfg(test)] mod tests` (module opens at `:136`; the four literals are at `:157`, `:202`, `:232`, `:282`), so none of them is production; **87 / 87** in `src/app/tests.rs`. Totals: **166 lines / 184 occurrences**. Several lines hold two literals (an arrow key aliased to a letter, for example), which is why the occurrence count exceeds the line count. `?` and `Ctrl-Y` are handled globally *outside* every mode block. There is no list to generate a palette from. | `src/app.rs:80-85`, `:1017`, `:1027`, the 7 `match key.code` sites |
| B5 | **Mouse selects but never acts.** Only `Down(Left)` is handled; there is no `Up`, no double-click, and `Target` has four variants, all of them selections. | `src/app.rs:1204`, `Target` (`src/ui/hit.rs:15-24`) |
| B6 | **Nothing says what to do next.** Degraded states are handled ad hoc, per panel, and inconsistently: the grid prints `no employees — add one with \`mywaker employee add\`` (`src/ui/pipeline.rs:43`), while the tasks panel prints the bare `no employees` (`src/ui/tasks.rs:33`); the project picker prints `no projects — create a directory there and reopen with  p`, the model picker prints `no models — make sure OpenCode has providers configured`. Only one of the three names an action, one names a *shell* command the TUI cannot run, one names none — and "the service is down" has no callout at all, only the title's glyph. | `src/ui/pipeline.rs:43`, `src/ui/tasks.rs:33`, `src/ui/projects.rs:62`, `src/ui/models.rs:49` |
| **B7** | **Fit hides what it drops.** `fit_sections` breaks out of the loop and returns the prefix, reporting nothing; the sidebar's sections simply vanish. The grid does better — its title reports `rows a-b/c` when it overflows — but that is a ratio, not a count, and the sidebar has no equivalent. | `fit_sections` (`src/ui/sidebar.rs:176-191`), called by `fit` (`:167`); grid title `src/ui/pipeline.rs:621-634` |
| B8 | **Errors are unclassified and unbounded in count.** 9 `push_error_log` call sites, each a free-form `String`, into a roster log band of 4 rows (3 while thinking). No severity, no stable id, no bound action. | `src/app.rs:913, 978, 2112, 2757, 2882, 3519, 3935, 4045, 4071`; `push_error_log` (`:4231`); `src/ui/layout.rs:42-43` |
| B9 | **Connection health is a glyph, not a state.** The title shows one of three lines — error, version, or spinner — with no age and no banner. | `connection_line` (`src/ui/mod.rs:194-212`) |

**Consequence for the reader:** U1–U12 are not derived from the implementation.
They are a decision, made once, and the implementation is expected to move to
them. If a rule below looks written to match what the code happens to do, that is
a defect in this file, and §9 is where to say so.

---

## 1. Scope

The **interaction layer** of the TUI:

- **Discovery** — how a user finds an action they do not know (palette, hint bar,
  help), and the single registry those surfaces are generated from (P1).
- **Legibility** — what the roster says about an employee's state, and what the
  app says when it is degraded or has hidden something (P2).
- **Reliability** — connection health, error classification, delivery receipts,
  and in-place recovery (P3).
- **Polish** — status that never relies on colour alone, and `NO_COLOR` (P4).

Out of scope, deliberately:

- **Rendering of the sidebar's sections.** [`acceptance-sidebar.md`](acceptance-sidebar.md)
  owns that, including its frozen line budget (§2) and markers (A5). U9 states
  explicitly where this file may *not* reach into it (§4, U9).
- Pause/continue and the reopen prompt ([`acceptance.md`](acceptance.md)).
- Chat transcript rendering, including fenced blocks.
- Model routing, the task store's on-disk format, and `team.toml`'s schema.
- Web or desktop surfaces, and any new runtime dependency.

---

## 2. Surfaces and markers (frozen)

Every criterion that is asserted **from a rendered buffer** needs a marker: a
literal string that appears in the surface's first line and **nowhere else in the
frame**. Markers are what U2, U4–U7 and U9–U12 are asserted on.

Three criteria are deliberately **not** marker-based, because their subject is
behaviour rather than a drawn surface, and a marker would prove nothing:

- **U1** (one registry) is structural: it is asserted on the registry's contents
  and on the absence of key literals elsewhere.
- **U3** (hint bar honesty) is asserted on the bar's *composition* — that no pair
  is clipped and that no advertised key is a no-op — not on a single string.
- **U8** (mouse parity) is asserted on hit-testing and dispatch, using the
  `ui::hit` seam and the same geometry the renderer draws from.

Uniqueness is part of the contract (§7 rule 4). Each candidate below was checked
against the tree at hand-off: the "absent today" column is the count of that
literal in `src/ui/` — `0` means the marker cannot collide with anything the
renderer already draws.

| ID | Surface | Marker | Absent today | Owner phase |
|---|---|---|---|---|
| **U2** | Command palette | `actions` | 0 | P1 |
| **U4** | Help overlay | bordered title ` help ` | see note | P1 |
| **U5** | Degraded-state callout | `no service` · `no model assigned` · `no team` · `no project assigned` | 0 · 0 · 0 · 0 | P2 |
| **U6** | Card legibility | `waiting on` · `stalled` | 0 · 0 | P2 |
| **U7** | Focus mode | `focus mode` | 0 | P2 |
| **U9** | Hidden-fit counter | `hidden` | 0 | P2 |
| **U10** | Connection banner | `reconnecting` | 0 | P3 |
| **U11** | Error taxonomy | `transient` · `actionable` · `fatal` | 0 · 0 · 0 | P3 |
| **U12** | Delivery receipts | `acknowledged` | 0 | P3 |

**U4's marker note.** `help` already appears in the hint bar as a description
(`("?", "help")`, `src/ui/mod.rs:343`), so the bare word is not unique. The marker
is therefore the **bordered title** ` help ` — the string drawn inside a `Block`
with `Borders::ALL` and a `Cyan` border, which no other surface draws. The
assertion reads the bordered row, not a substring search of the frame. This is a
deliberate exception to the literal-marker rule and is why U4 is the one criterion
whose §6 tests assert on structure rather than on a string.

**Which symbol asserts the marker (decided, rev 15).** The bordered title is
asserted by `help_overlay_is_a_window` (`src/ui/mod.rs`), so U4's cells cite it
alongside `u4_help_covers_every_action` and `u4_help_fits_and_counts_at_60x16`.
That name is therefore **not** a scroll-only seam: it carries the marker clause
(§2) *and* the window/counter chrome, which is why it lives with the renderer
rather than with `tests/ux.rs`. §6 labels it accordingly. No third name is needed
for the border.

**Why markers and not IDs.** No surface draws its criterion ID, so "U6 present"
is not decidable from a buffer at all; "`waiting on` present" is. This is the same
device as the sidebar spec's §2 marker column, for the same reason — and, as there,
a marker must be a literal the surface **composes**, so it cannot be satisfied by
relayed text (§7 rule 4).

**Relayed text is out of scope.** An employee's name, role, task goal, model id, or
an upstream error message may contain any marker's words. The assertions are
scoped to strings the surface **composes**, not text it relays — the lesson
[`acceptance-sidebar.md`](acceptance-sidebar.md) §8 records under A7.

---

## 3. Breakpoints (frozen)

**Four breakpoints are pinned, and the matrix runs on exactly those four** (U1–U12
× 4 = 48 cells, §4.1). They are the plan's breakpoints (P0/T0.3). A fifth size,
160×50, is listed below as a **ceiling reference only**: it is drawn in the
smoke tests and it is *not* a matrix axis, so no cell is claimed for it. Its job is
to stop a criterion being satisfied by a layout that only works when cramped.

| Size | main (w×h) | grid width | card width | cols | What changes here |
|---|---|---|---|---|---|
| **160×50** *(ceiling ref.)* | 104×42 | 104 | 33 | 3 | every surface has room |
| **120×40** | 64×32 | 64 | 19 | 3 | palette and help fit without scrolling |
| **100×30** | 44×22 | 44 | 13 | 3 | grid keeps 3 columns, cards are narrow |
| **80×24** | 24×16 | 24 | 23 | 1 | grid falls to **1 column**; hint bar already overflows (B2) |
| **60×16** | 4×8 | 4 | 3 | 1 | every surface is at its floor — card 3 · **rows 1** · cols 1, **frameless at the floor** (`docs/keymap.md` §5.5) |

Derivation, so the table is checkable rather than asserted: `main.height = terminal.height − 1 − 2 − 4 − 1`
(title + roster prompt + log + help, `src/ui/layout.rs:42-43`) and
`grid width = terminal.width − 24 − 32` (`roster_areas`, `:68-75`). Card width and
column count are `pipeline::shape`'s `cell_width` / `cols` after the riser and gap
(`src/ui/pipeline.rs:295-315`): three columns need `3·riser + 3·card + 2·gap`, and
below `MIN_CELL_WIDTH` (12, `:102`) the grid falls back to one full-width column
(`:302-306`). That is why 80×24 is the single-column case and why its card is
*wider* than 120×40's — the fallback trades columns for legibility.

**The only width-driven relaxation in this spec.** At 60×16 the card is **3 columns
wide** and, per [`keymap.md`](keymap.md) §5.5, is drawn as a **1-row frameless
entry** rather than a 7-row framed card — so U6 requires the status glyph but not
the `waiting on` **text** and not the queue count, which the tasks panel carries:
`waiting on researcher` cannot fit in 3 columns, and a truncated `wai…` tells the
user nothing. The relaxation is keyed to the drawn card
width being below `MIN_CELL_WIDTH` (12, `src/ui/pipeline.rs:102`), which at every
size in §3 is **60×16 and no other** — 80×24 falls back to a single *23-column*
card, which has room for the text. This is a rendering relaxation,
never a presence relaxation: the criterion is still asserted at that size, on the
glyph and the count. It is modelled on A4's narrow-width clause in the sidebar
spec, and it is the *only* one here.

**One size is deliberately not pinned: 40×24.**

- **40×24** — the team panel is zero-width at this size, so no card is drawn and
  no card-shaped criterion (U2–U12) is decidable there. Recorded as a known
  limitation, not a pass. It is not pinned in this file; the render-smoke clause
  (drawing must not panic) stays with
  [`acceptance-sidebar.md`](acceptance-sidebar.md) A1, which already covers it.

---

## 4. Criteria (U1–U12)

Every pass condition is fixed here. §4 is the single source of truth for its
wording; the matrix (§4.1) is a generated restatement, and if the two disagree,
**§4 wins**.

| ID | Criterion | Pass condition |
|---|---|---|
| **U1** | One registry | Every key binding resolves through one `Action` catalogue. The hint bar, the help overlay, `/help`, and the palette are each **generated** from it: no surface composes a key literal of its own, and adding one action changes all four with no other edit. Decidable two ways: (a) a test asserts each surface's action ids are a subset of the registry's; (b) `KeyCode::` appears **only** inside the registry module and the text editors — 0 occurrences elsewhere. |
| **U2** | Palette | `Ctrl-P` opens the palette from every `Mode`, with `Ctrl-K` as an alias (both open the same surface). It is **not** bound while a text field has the keyboard — the goal prompt or a picker's filter — because `Ctrl-P`/`Ctrl-K` are line-editing keys there; the palette is reached by blurring first. **Chat is not in this exception**: its history is `↑`/`↓`, so `Ctrl-P` displaces nothing, and the palette opens from `Mode::Chat` like every other mode (`docs/keymap.md` §8, C1a). Typing filters, fuzzily, over four kinds: actions, employees, projects, models. Each row shows its bound key when it has one, so the palette teaches as it is used. `Enter` runs the highlighted entry and closes the palette; `Esc` closes it and restores the prior mode and focus exactly. The palette is never *required*: every entry is also reachable by its own binding. |
| **U3** | Hint bar honesty | The bar shows at most **5** action pairs for the focused region, chosen by context, not by mode alone. (a) Every key it advertises is bound and available in that context — a hint for an action that would be a no-op is a FAIL. (b) It **drops whole pairs** when short, never clipping a pair: no drawn line ends inside a description. (c) When pairs were dropped, the last slot says `? help` — the escape hatch is never the thing that is dropped. |
| **U4** | Help, complete and navigable | The overlay is generated from the registry and covers **every** action: no registry action lacks a help row, and no help row names an action that does not exist. It is scrollable (`↑↓` / `PgUp`/`PgDn`) and searchable (`/`), and it fits at 60×16 with a visible window counter of the form `rows a-b/n`. Opening it is not a dead end: any dismiss key closes it without side effects. |
| **U5** | Degraded states | Each of the four degraded states draws a callout with **exactly one** primary action, named on the callout: (1) `no service` — reconnect; (2) `no model assigned` — open the model picker; (3) `no team` — hire or load a roster; (4) `no project assigned` — open the project picker. "Exactly one" is asserted, not "at least one": a callout offering three equal choices is a FAIL. A healthy app draws none of the four. The callout is drawn on the roster's **notice row** (§4.2) at every size, so its host does not change with the grid's width. |
| **U6** | Card legibility | While an employee is `Working`, its card names what it is waiting on (`waiting on <target>`) when it is waiting on another employee or a session, and shows the queue depth when prompts are queued behind it. While `Working` with **no event for 30 s**, the card shows `stalled`. All three are answerable from the card alone — the criterion fails if the answer requires opening chat. Where the drawn card is narrower than `MIN_CELL_WIDTH`, the glyph is required and the `waiting on` text is not (§3). The **marker** `waiting on` is required wherever the usable width holds it (10 columns, `docs/keymap.md` §2.7); the **target** is required only when the usable width exceeds the marker, so at 100×30 the marker survives and the target drops (`docs/keymap.md` §8, C2). At the 60×16 floor the card is a 1-row frameless entry 3 columns wide, which cannot hold a count: the **glyph** is required there and the queue count and receipt are carried by the tasks panel (§5.5, `docs/keymap.md` §8, C3). |
| **U7** | Focus mode | `f` on the roster — and **`Ctrl-F`** in chat and whenever the prompt has the keyboard, because a bare `f` types `f` there — toggles focus mode (both forms are in the registry as one action, `docs/keymap.md` §8, C1b; the palette entry also toggles it): the sidebar and tasks panel collapse and the grid or chat takes the full width. The prompt bar is never hidden. The setting is remembered **per mode** — leaving chat and returning to the roster restores each one's own state. `Esc` restores, and restoration is exact: the panel rects are byte-identical to those before entering focus mode. |
| **U8** | Mouse parity | Single click selects; **double-click opens chat**; a click on the prompt band focuses it; the wheel scrolls. Every pointer action has a key equivalent (U1's registry lists both), and no click resolves to a target that was not drawn — hit-testing keeps reading the renderer's own geometry (`ui::hit`, `src/ui/hit.rs:32-73`). |
| **U9** | Fit is reported | When the roster grid hides card rows, the app reports **how many are hidden**, as `hidden N`, in addition to the existing `rows a-b/c` affordance. It is drawn on the grid's title while the title can hold it, and on the roster's **notice row** (§4.2) below that — one host or the other, never both and never neither. The count must agree with the drawn viewport (`N = total − visible`), and must disappear when nothing is hidden — a `hidden 0` is a FAIL. **The sidebar clause is deliberately deferred** — see the note below. |
| **U10** | Connection health | Connection is a first-class state — `connected` · `reconnecting` · `down` — not a spinner. A banner names the state and the age of the last successful event. On reconnect the app re-subscribes to the event stream and re-syncs the roster **without losing queued work**: a prompt queued while down is still queued (or delivered) afterwards, and is never silently dropped. |
| **U11** | Error taxonomy | Every error carries a class — `transient` · `actionable` · `fatal` — a **stable id**, a one-line cause, and a bound action (`r` retry · `c` copy · `?` detail). Every `push_error_log` call site is classified: 9 sites at hand-off (§0 B8), and **0** may remain unclassified. A `fatal` error is never dismissible into silence; a `transient` one is retried before it is shown twice. |
| **U12** | Delivery receipts | Every prompt shows its delivery state per employee — queued → sent → `acknowledged` → completed — on the card and in the tasks panel. A failed delivery offers in-place retry, and the retry re-sends the **same** admission id, so the service deduplicates it: a retried prompt is never delivered twice. |

### U9's deferred clause — recorded, not quietly dropped

U9 asks that *nothing hides silently* (plan principle 2). On the grid, the title
already reports `rows a-b/c` when the roster overflows (`src/ui/pipeline.rs:621-634`),
so the hidden *count* is technically derivable today — a user can subtract. U9 is
therefore a **legibility** requirement, not a new capability: the count is stated
rather than left as arithmetic, because `rows 2-3/9` does not read as "seven
employees are off screen" to anyone not already holding the total. It is also the
one clause whose marker (`hidden`) can be asserted from a buffer without parsing
the ratio.

On the **sidebar** it is not free, and this file does not get to decide it:

- The panel's title is pinned to the literal ` waker ` by
  [`acceptance-sidebar.md`](acceptance-sidebar.md) A10, so a counter cannot be
  appended to it.
- Every content row is owned by §2's line budget, and A3 requires sections to be
  all-or-nothing, so a counter cannot take a row either.

Reporting sidebar drops therefore requires revising that file's §2 budget and its
§3 size table **together** — which its own rule 3 forbids doing from here. U9
therefore covers the grid only, and the sidebar gap is recorded as an open item in
§8 rather than asserted. It is not a relaxation: it was never in this file's
grant.

### 4.1 Matrix — U1–U12 × 4 sizes (48 cells)

**Generated, and owned by the tester (task T4.2).** The axes are frozen here —
U1–U12 down, the four §3 sizes across — and every cell cites the test symbol that
asserts it, in the form [`acceptance-sidebar.md`](acceptance-sidebar.md) §4.1 uses.
The writer does not pre-fill the cells: a cell filled before a test exists is a
claim, not evidence, and this file's whole purpose is that claims are not enough.

Cells are filled in run 1 (§8). A cell with no covering test is recorded as
*not evidenced* (§7 rule 2).

**Filled in run 1 (2026-10-21) — result: 0 of 48 evidenced; 46 *not evidenced* and
2 *blocked* at run 1, all 48 *not evidenced* as of rev 5 (§4.2 decided the two).**
Every cell cites the §6 symbol that will assert it and that symbol does not resolve
yet (§8 records `resolved=0 of 31` at run 1; §6 gained one more frozen name in rev
9, two guard citations in rev 10, and two U4 names in rev 15, so the current
inventory is **34 unresolved + 3 existing**, none of the 34 resolving). That
is the expected result for a spec frozen before implementation, not a failure of
this run.

Cell status vocabulary, and what each one commits to:

| Status | Meaning |
|---|---|
| **`not evidenced`** | The §6 test that will assert this cell does not exist. Nothing is claimed about the behaviour at this size. |
| **`blocked`** | The criterion is not decidable at this size from any observable the tree exposes, and needs a spec decision first. The reason is given in-cell and in §8. |
| **`PASS`/`FAIL`** | Reserved for a run with a resolving test plus observed output. None is used in run 1. |

#### The two blocked cells — decided, and why they were never a width problem

Both blocks assume the **grid panel is the only host** for two kinds of prose. At
60×16 that panel is 4 columns wide (2 usable) and a card is 3, so no sentence can
be drawn there. That assumption is the defect, not the width. Neither U5 nor U9
names a host, so fixing the host makes both cells decidable **without touching a
pass condition** (§7 rule 1 is not invoked).

**Decision — the notice row.** The roster's log band gains a **notice row**: its
first content row, carrying at most one *grid-scoped notice* — a one-line message
about the roster that the grid itself cannot hold. It sits **below** the pinned
error, never above it (see precedence, below).

- **It is not a new band and not a new `Regions` field.** The notice occupies an
  existing log row, so there is no new rect and U7's byte-identical restoration is
  unaffected. `main` still never moves — but note *how*: when a notice exists the
  prompt band is held at `PROMPT_HEIGHT` instead of taking its thinking row (§b),
  so the fixed bands still sum to the same 8 rows and `main` keeps its height. The
  frozen invariant "the prompt band must not shrink the panels" is asserted in two
  places, cited separately: **the test** is
  `the_prompt_band_does_not_shrink_the_panels` (`src/app/tests.rs:2205`), which
  asserts `main.height == 32` at 120×40 and that the band sits directly under
  `main`; **the layout-level assertion** is the
  `assert_eq!(roster.main, chat.main, "the prompt band must not shrink the panels")`
  inside `the_roster_prompt_band_comes_out_of_the_log` (`src/ui/layout.rs:222`,
  test at `:213`). Both are preserved rather than merely re-asserted — the one
  thing that changes is that `Regions::of` is passed `thinking && notice.is_none()`.
- **The log is already the band that gives way** (`src/ui/layout.rs:34-38`) and
  already pins its most important message first — "a failed turn must never be
  invisible because the roster's log has room for little else"
  (`src/ui/mod.rs:233-236`). The notice row is that convention, generalised.
- **A healthy app with nothing hidden consumes no row**, so the band is exactly
  what it is today — which is U5's "a healthy app draws none of the four".

**Contents and precedence.** One line, ` · `-joined. Two orders apply, and they are
not the same order:

*(a) Within the notice row* — what the line is made of:

1. **the degraded-state callout** (U5) — blocking and actionable, never displaced;
2. **the fit report `hidden N`** (U9) — appended when it fits, because "nothing
   hides silently" is unconditional.

The fit report is dropped only when appending it would truncate the callout, and
**that never occurs at a pinned size** (below). Below the floor the callout wins
and the fit report is dropped **whole**, never clipped — U3(b)'s rule.

*(b) Within the log band* — the notice row's place among the band's rows, and what
it costs. The band's content rows are `log_height − 2` (the borders): **2 at rest**
and **1 while the selected employee is thinking**, because the thinking indicator
takes a third prompt row and the log gives way (`src/ui/layout.rs:41-45`;
`PROMPT_THINKING_HEIGHT` = 3, so `4 − 2 = 2` at rest but `3 − 2 = 1` while
thinking). **One content row cannot hold both a pinned error and a notice**, so as
first written this section reopened the block it had just closed: with the selected
employee thinking, the error would take row 1 and U5/U9 would be undecidable in
exactly the state where a user is most likely to be looking.

**Decision — the notice row is reserved; the thinking indicator yields.** A notice
is a *blocking* condition, and this file already ranks it above a progress line.
So when a grid-scoped notice exists, the prompt band does **not** take its third
row: `thinking` reaches `Regions::of` as
`prompt_employee_is_thinking() && notice.is_none()`, the band stays at
`PROMPT_HEIGHT` (2), and the log keeps 4 rows / **2 content rows**. `main` is
therefore byte-identical in every combination — which is what
`the_prompt_band_does_not_shrink_the_panels` (`src/app/tests.rs:2205`) asserts
(`main.height == 32` at 120×40), and what
`assert_eq!(roster.main, chat.main, …)` inside
`the_roster_prompt_band_comes_out_of_the_log` (`src/ui/layout.rs:222`, test at
`:213`) asserts at the layout level.
The row is *reserved*, not *borrowed*: it is taken back the moment the notice
clears, and the indicator returns with it.

Nothing is lost by suppressing the indicator. The thinking state is still drawn
twice — the employee card's status glyph and the sidebar's `now` line both animate
on the tick — so the prompt band's line is the third copy, and it is the one that
yields. The alternative (shrinking `main` to keep all three) would move the panels
the user reads, which the frozen invariant forbids.

**Consequence: U5 and U9 are decidable in every employee state.** Because the
notice row exists whenever a notice does, "exactly one primary action" (U5) and
`hidden N` (U9) are asserted identically while thinking and while idle. No cell
needs a thinking-specific relaxation, and none is granted.

**What the notice actually costs.** The order is fixed, and the notice never
displaces a failure:

1. **the pinned error** (`src/ui/mod.rs:233-249`) — always the first content row,
   unchanged;
2. **the notice row** — the second when a notice exists, and the first when no
   error is pinned;
3. **the telemetry and recent log lines** — what is left.

So the cost is not "one line at rest": it is *whatever the tail has left*, and
`stream_stats` / `telemetry_stats` are displaced **entirely** in most notice
states, not merely trimmed:

| State | content rows | drawn |
|---|---|---|
| rest, no error, no notice | 2 | `stream`, `usage` |
| rest, error, no notice | 2 | error, `stream` |
| rest, no error, notice | 2 | notice, `stream` |
| rest, error **+** notice | 2 | error, notice — **both stats displaced** |
| thinking, no notice | 1 | error, if any — **both stats displaced** |
| thinking, **+** notice | 2 (reserved) | error (if any), notice — **both stats displaced** |

That is a deliberate trade, and it should be stated as one: **whenever a notice is
up, the telemetry line is gone.** The notice says *why the roster is empty or what
is hidden*, which is blocking and one-shot; `usage` and `stream` are continuous
readouts that return the moment the notice clears. A user who needs them is not
blocked by their absence; a user who cannot see why the roster is empty is. The
same precedence already governs the failure line, and this section applies it one
step further down.

**Consequence for U11 — recorded, not papered over.** U11's cells say the log band is
4 rows at every pinned size, which is true of the *band* but not of its usable
content: with a notice up, `error` + `notice` consume both content rows and the
recent-log tail gets none. That is the precedence above working as intended, but it
means U11's "class + id + cause + action, legible in the band" is asserted on the
**pinned error row**, not on the tail — the error row is always drawn and always
first. U11 needs no relaxation for this: its subject is error *rows*, and the error
row is precisely the one the notice row cannot displace. What U11 may not claim is
that a *historical* log line stays visible while a notice is up; it does not, and
that is the trade this section states.

**The grid stops hosting prose.** At 60×16 the grid carries a **glyph** (`◇`), not
a sentence: [`keymap.md`](keymap.md) §2.7's narrow-glyph rule applied to the panel
itself. This also retires B6's `add one with \`mywaker employee add\`` from the
grid — a TUI that prints a shell command it cannot run is not an action, and was
never the "one named primary action" U5 asks for.

A callout drawn *inside* the grid could not satisfy U5 at 60×16 by construction,
and would duplicate the message at every size that can hold it. Two copies of one
callout is the failure U5 forbids when it says "exactly one".

**The title is not exempt; it hands off.** The grid title keeps `hidden N` while it
fits (` team · 12 · hidden 8 ` is 22 columns → 80×24 and up) and the **notice row
carries it below that** (60×16). U9 is satisfied at every pinned size — relocated,
never relaxed.

| Surface | 160×50 | 120×40 | 100×30 | 80×24 | 60×16 |
|---|---|---|---|---|---|
| grid panel | 104 | 64 | 44 | 24 | **4** |
| card | 33 | 19 | 13 | 23 | **3** |
| notice row usable (terminal − 2) | 158 | 118 | 98 | 78 | **58** |
| longest callout `◆ no project assigned · p projects` | 34 | 34 | 34 | 34 | **34** |
| callout + ` · hidden 11` | 46 | 46 | 46 | 46 | **46** |
| **log content rows when a notice is up** | 2 | 2 | 2 | 2 | **2** |

Every pinned size holds both messages on one row with room to spare — **58 ≥ 46 at
the floor** — so no callout is truncated at a pinned size and "exactly one primary
action" is assertable from the buffer at all four. The cells are **unblocked, not
relaxed**; their pass conditions are unchanged.

Measured geometry for the four axes, read from the renderer's own seams
(`ui::team_area`, `ui::sidebar_area`, `ui::grid_cells`) rather than recomputed:

| Size | main | grid | card | cols | sidebar |
|---|---|---|---|---|---|
| 120×40 | 64×32 | 64 | 19 | 3 | 24×32 |
| 100×30 | 44×22 | 44 | 13 | 3 | 24×22 |
| 80×24 | 24×16 | 24 | 23 | 1 | 24×16 |
| 60×16 | 4×8 | 4 | 3 | 1 | 24×8 |

#### U1 — One registry

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U1 | 120×40 | Hint bar, help overlay, `/help` and palette all derive from one `Action` catalogue; each surface's ids ⊂ registry; `KeyCode::` only in the registry and text editors. | registry | `u1_every_surface_reads_the_registry` · `u1_no_key_literal_outside_the_registry` — **not evidenced** |
| U1 | 100×30 | As above; all four surfaces still render at 44 grid columns, so a surface falling back to a hardcoded literal is visible. | registry | same symbols — **not evidenced** |
| U1 | 80×24 | As above; the hint bar is already over-truncated here (B2), so the registry — not the bar — must be the source. | registry | same symbols — **not evidenced** |
| U1 | 60×16 | As above; the help overlay is the only complete surface at this size, so it must still be registry-derived and scrollable. | registry | same symbols — **not evidenced** |

#### U2 — Palette

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U2 | 120×40 | `Ctrl-P` opens from every `Mode` (`Ctrl-K` alias) — **including chat, which is not in the text-field exception** (`docs/keymap.md` §8, C1a); **not** bound while the goal prompt or a picker's filter has the keyboard; fuzzy filter over actions/employees/projects/models; each row shows its bound key; `Enter` runs + closes; `Esc` restores prior mode **and** focus exactly; palette never required. | buffer + model | `u2_palette_opens_from_every_mode` · `u2_palette_teaches_its_keys` · `u2_palette_esc_restores_mode_and_focus` · `u2_palette_is_unbound_while_typing` — **not evidenced** |
| U2 | 100×30 | As above; the popup must fit 44 grid columns without clipping the highlighted row. | buffer | same symbols — **not evidenced** |
| U2 | 80×24 | As above; popup fits a 24-column grid panel. | buffer | same symbols — **not evidenced** |
| U2 | 60×16 | As above; popup fits 60×16 with the selected row and any counter visible — the floor case. | buffer | same symbols — **not evidenced** |

#### U3 — Hint bar honesty

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U3 | 120×40 | ≤5 pairs for the focused region; every advertised key bound and available (a hint for a no-op is a FAIL); pairs dropped whole, never clipped mid-description; when dropped, last slot is `? help`. | buffer | `u3_hint_bar_never_advertises_a_no_op` · `u3_hint_bar_drops_whole_pairs` · `u3_help_hint_survives_truncation` — **not evidenced** |
| U3 | 100×30 | As above. Today 8 whole pairs in 99 of 100 columns, last one cut mid-description (`Ctrl-Y copy`), **`? help` absent** — clause (b) and (c) both fail. | buffer | same symbols — **not evidenced** |
| U3 | 80×24 | As above. Today 7 whole pairs in 78 of 80 columns, **`? help` absent**. | buffer | same symbols — **not evidenced** |
| U3 | 60×16 | As above. Today 5 whole pairs in 59 of 60 columns, line ends on the bare key `m`, **`? help` absent** — clauses (b) and (c) fail. | buffer | same symbols — **not evidenced** |

#### U4 — Help, complete and navigable

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U4 | 120×40 | Overlay generated from the registry, covers every action and invents none; scrollable `↑↓`/`PgUp`/`PgDn`; searchable `/`; counter `rows a-b/n`; any dismiss key closes with no side effects. The bordered title ` help ` (§2's marker) is asserted by `help_overlay_is_a_window`. | buffer | `u4_help_covers_every_action` · `u4_help_fits_and_counts_at_60x16` · `u4_help_search_filters_in_place` · `u4_help_dismiss_key_has_no_side_effects` · `help_overlay_is_a_window` — **not evidenced** |
| U4 | 100×30 | As above; §3 says it should fit without scrolling here, but the counter must still be truthful (it states a fact, it does not only appear on overflow). | buffer | same five symbols — **not evidenced** |
| U4 | 80×24 | As above; the counter is required here because 35 body rows cannot fit. | buffer | same five symbols — **not evidenced** |
| U4 | 60×16 | As above; the crux case — 58×14 clamped box, 12 content rows for 35 today (B3). Counter must read `rows a-b/n` (e.g. ` help · rows 1-12/41 `), and scrolling and `/` search must both work — the two clauses rev 15 gave their own frozen names, so this cell is now fillable rather than demanding more than it can cite. | buffer | `u4_help_covers_every_action` · `u4_help_fits_and_counts_at_60x16` · `u4_help_search_filters_in_place` · `u4_help_dismiss_key_has_no_side_effects` · `help_overlay_is_a_window` — **not evidenced** |

#### U5 — Degraded states

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U5 | 120×40 | Each of `no service` / `no model assigned` / `no team` / `no project assigned` draws a callout with **exactly one** named primary action; a healthy app draws none of the four. Drawn on the **notice row** (§4.2), not in the grid: 118 columns usable against a 34-column longest callout. | buffer | `u5_each_degraded_state_has_exactly_one_primary_action` · `u5_a_healthy_app_draws_no_callout` — **not evidenced** (host fixed, §4.2) |
| U5 | 100×30 | As above; drawn on the **notice row** (§4.2) at this size too, not in the grid: 98 columns usable against a 34-column longest callout (46 with the `hidden N` suffix), so the marker and its one action are fully readable with room to spare. The grid is 44 columns here and is not the host. | buffer | same symbols — **not evidenced** (host fixed, §4.2) |
| U5 | 80×24 | As above; drawn on the **notice row** (§4.2): 78 columns usable against a 34-column longest callout (46 with the suffix). The grid is only 24 columns at this size — narrower than the 34-column callout, so it could not have hosted the marker at all; the notice row is what makes it readable. | buffer | same symbols — **not evidenced** (host fixed, §4.2) |
| U5 | 60×16 | As above; the grid is 4 columns (2 usable) and carries the `◇` glyph only — the callout is drawn on the **notice row** (§4.2), 58 columns usable for a 34-column callout, so the marker and its one action are both fully readable. The row is **reserved** while thinking (§4.2), so this is asserted identically whether or not the selected employee is thinking. | buffer | `u5_each_degraded_state_has_exactly_one_primary_action` — **not evidenced** (host fixed, §4.2) |

#### U6 — Card legibility

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U6 | 120×40 | While `Working`, card names `waiting on <target>` when waiting on an employee/session, and shows queue depth when prompts are queued; `stalled` after 30 s with no event; all answerable from the card alone. | buffer + model | `u6_card_names_what_it_waits_on` · `u6_stalled_after_30s_without_an_event` — **not evidenced** |
| U6 | 100×30 | As above; card is 13 wide, usable 11 — the **marker** `waiting on` is required (10 columns) and the **target** drops, since 11 cannot hold both (`docs/keymap.md` §2.7, §8 C2). The marker must not be ellipsised away. | buffer | same symbols — **not evidenced** |
| U6 | 80×24 | As above; card is **23** wide (single-column fallback), so the relaxation does **not** apply and `waiting on <target>` is required. | buffer | same symbols — **not evidenced** |
| U6 | 60×16 | **Glyph required**; the queue-depth count is **not** — it is carried by the tasks panel (32 columns). The floor draws a 1-row frameless `<glyph> <name>` entry in 3 usable columns, so a count (`▶ ×2`, 4 cells) cannot fit (`docs/keymap.md` §5.5, §8 C3); `waiting on` text is not required either, as the usable width is 3 (< 10). | buffer + tasks panel | same symbols + `card_keeps_its_glyph_below_twelve_columns` — **not evidenced** |

#### U7 — Focus mode

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U7 | 120×40 | `f` toggles on the roster and **`Ctrl-F`** in chat / with the prompt focused (`docs/keymap.md` §8, C1b); sidebar + tasks collapse, grid/chat take full width; prompt bar never hidden; remembered **per mode**; `Esc` restores **byte-identical** panel rects. | buffer + model | `u7_focus_mode_restores_the_exact_rects` — **not evidenced** |
| U7 | 100×30 | As above; rect equality asserted on this size's rects. | buffer | same symbol — **not evidenced** |
| U7 | 80×24 | As above; the tasks panel is 32 of 80 columns, so collapsing it must actually return those columns to the grid. | buffer | same symbol — **not evidenced** |
| U7 | 60×16 | As above; sidebar 24 + tasks 32 = 56 of 60, so focus mode is the only way to see a usable grid. Rect equality is the assertion. | buffer | same symbol — **not evidenced** |

#### U8 — Mouse parity

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U8 | 120×40 | Single click selects; **double-click opens chat**; click on the prompt band focuses it; wheel scrolls; every pointer action has a key equivalent in the registry; no click resolves to a target that was not drawn. | buffer + registry | `u8_double_click_opens_chat` · `u8_every_pointer_action_has_a_key` — **not evidenced** |
| U8 | 100×30 | As above; hit points read `ui::grid_cells` for this size's 3-column layout. | buffer | same symbols — **not evidenced** |
| U8 | 80×24 | As above; single-column fallback — a click must still resolve to the card under it. | buffer | same symbols — **not evidenced** |
| U8 | 60×16 | As above; cards are 3 wide and the panel 4 — the smallest clickable target in the spec. A double-click must be driven at the rect's centre, or the assertion passes vacuously on an empty grid. | buffer | same symbols — **not evidenced** |

#### U9 — Fit is reported

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U9 | 120×40 | Title reports `hidden N` beside `rows a-b/c`, with `N = total − visible`; `hidden 0` is a FAIL. Today the title reads `team · 12 · rows 1-3/4` — rows hidden, nothing reported. | buffer | `u9_grid_title_reports_hidden_rows` — **not evidenced** |
| U9 | 100×30 | As above; title today `team · 12 · rows 1-2/4`. `hidden N` must fit in 44 columns. | buffer | same symbol — **not evidenced** |
| U9 | 80×24 | As above; title today `team · 12 · 1-1/12` — the `rows ` word is already gone at 24 columns. The test must assert the counter's presence, not its adjacency to a `rows a-b/c` string that is itself clipped. | buffer | same symbol — **not evidenced** |
| U9 | 60×16 | The grid title is 4 columns and carries `◇` only; `hidden N` is drawn on the **notice row** (§4.2) as `◇ hidden 11 · f focus` (21 of 58 columns). The count must agree with the drawn viewport and vanish when nothing is hidden — asserted on the notice row, not the title. The row is **reserved** while thinking (§4.2), so this holds in that state too. | buffer | `u9_grid_title_reports_hidden_rows` — **not evidenced** (host fixed, §4.2) |

#### U10 — Connection health

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U10 | 120×40 | Connection is a state (`connected` · `reconnecting` · `down`), not a spinner; a banner names the state **and the age** of the last successful event; reconnect re-subscribes, re-syncs the roster, and **loses no queued work**. | buffer + model | `u10_banner_names_the_state_and_the_event_age` · `u10_reconnect_keeps_queued_work` — **not evidenced** |
| U10 | 100×30 | As above; banner fits 100 columns. | buffer | same symbols — **not evidenced** |
| U10 | 80×24 | As above; banner fits 80 columns alongside the existing title content. | buffer | same symbols — **not evidenced** |
| U10 | 60×16 | As above; the banner must survive 60 columns — the state word **and** the age, or the age clause is unassertable here. | buffer | same symbols — **not evidenced** |

#### U11 — Error taxonomy

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U11 | 120×40 | Every error carries class (`transient`/`actionable`/`fatal`), stable id, one-line cause, bound action (`r`/`c`/`?`); **0** of the 9 `push_error_log` sites unclassified; `fatal` never dismissible into silence; `transient` retried before shown twice. | model + buffer | `u11_every_error_carries_a_class_and_an_id` · `u11_no_unclassified_error_site` — **not evidenced** |
| U11 | 100×30 | As above; the roster log band is 4 rows, so each classified row must be legible there. | buffer | same symbols — **not evidenced** |
| U11 | 80×24 | As above; log band still 4 rows at this size. | buffer | same symbols — **not evidenced** |
| U11 | 60×16 | As above; the frame is 60×16 and the log keeps 4 rows — class + id + cause + action complete or ellipsised **with the class intact**, since the class is the load-bearing token. | buffer | same symbols — **not evidenced** |

#### U12 — Delivery receipts

| ID | Size | Pass condition at this size | Observable | Evidence |
|---|---|---|---|---|
| U12 | 120×40 | Every prompt shows queued → sent → `acknowledged` → completed per employee, **on the card and in the tasks panel**; a failed delivery offers in-place retry; retry re-sends the **same** admission id. | buffer + model | `u12_receipt_walks_queued_to_completed` · `u12_retry_reuses_the_admission_id` — **not evidenced** |
| U12 | 100×30 | As above; card 13 wide — the receipt must be a glyph/word that fits, with the tasks panel (32) carrying the full text. | buffer | same symbols — **not evidenced** |
| U12 | 80×24 | As above; card 23 wide, tasks panel 32 — both surfaces must show it. | buffer | same symbols — **not evidenced** |
| U12 | 60×16 | As above; the card is a 1-row frameless 3-column entry, so a receipt glyph would make it 4 cells and cannot be drawn there (`docs/keymap.md` §5.5, §8 C5). The **tasks panel** (32 of 60) is the surface that must carry `acknowledged`; the card carries no receipt at this size, and that absence is the contract, not a failure. | buffer + tasks panel | same symbols — **not evidenced** |

---

## 5. Non-render criteria (UX1–UX6)

Separate table — these are not per size. Each carries its own evidence cell, so a
gap is recorded here rather than only in review.

| ID | Criterion | Evidence |
|---|---|---|
| **UX1** | **Status is never colour alone.** Every status is a triple — glyph, word, colour — and each is sufficient on its own. This already holds for employee status (`status::icon` + `EmployeeStatus::label` + `status::color`, `src/ui/status.rs:30-58`); UX1 extends it to connection state, delivery receipts, and error classes. | `not evidenced` |
| **UX2** | **`NO_COLOR` is honoured, and the palette is 16-colour safe.** With `NO_COLOR` set, no cell carries a foreground colour and every status is still distinguishable by glyph and word. No surface requires a 256-colour or truecolour terminal. | `not evidenced` — no `NO_COLOR` handling exists in `src/` at hand-off |
| **UX3** | **Aliases, not amputations.** Every binding that existed at hand-off keeps working through 0.22; the new binding is primary and the old one is marked deprecated in help and in `docs/keymap.md`. Removal is a 0.23 change, recorded there. | `not evidenced` — `docs/keymap.md` exists (T0.2, written 2026-10-21) and carries the deprecation register at **§3.7** (`docs/keymap.md:662`), which is what makes the criterion checkable; no test asserts that the old bindings still resolve |
| **UX4** | **No new runtime dependency, no persistence break.** `Cargo.toml`'s dependency set is unchanged by this work; `team.toml`, `store.json`, and `Mode`-based persistence load unchanged from 0.21.1. | `not evidenced` |
| **UX5** | **The build is clean.** `cargo test` · `cargo clippy --all-features -- -D warnings` · `cargo fmt --check` all exit 0. | `not evidenced` |
| **UX6** | **The docs agree.** `README.md`'s Controls table, `docs/keymap.md`, and this file name the same bindings; a binding in one and not the other is a defect in the docs, not a preference. | `not evidenced` — `docs/keymap.md` now exists, so this is checkable; no test compares the three surfaces yet |

---

## 6. Frozen test names

So the evidence column cites a symbol that must exist rather than describing a
run. The list holds **34 new names plus 3 existing tests**; the 34 do not exist
yet — that is the point of freezing them before implementation, and it is recorded
as a gap rather than glossed (§8). The three that already exist are named in the
final "cited as-is" bullet: **three existing test symbols** —
`clicking_outside_a_target_keeps_the_selection` (the selection invariant) and the
two `main`-height guards added in rev 10,
`the_prompt_band_does_not_shrink_the_panels` and
`the_roster_prompt_band_comes_out_of_the_log` — together with **two guard test
files**, `tests/grid.rs` and `tests/sidebar.rs`, whose suites must stay green. They
are cited so a criterion can name a guard it must not disturb. Counted precisely
because "none of these exist" is false for that bullet: **34 unresolved, 3
resolved**, which is what `resolved=0 of 31` in §4.1 and §8 means — that figure is
run 1's, taken before `u5_notice_survives_the_thinking_state` (rev 9), the two
guards (rev 10), and U4's search and dismiss names (rev 15) were added.

- `tests/ux.rs` — `u1_every_surface_reads_the_registry`,
  `u1_no_key_literal_outside_the_registry`,
  `u2_palette_opens_from_every_mode`, `u2_palette_teaches_its_keys`,
  `u2_palette_esc_restores_mode_and_focus`,
  `u2_palette_is_unbound_while_typing` (the text-field exception in §4 U2),
  `u3_hint_bar_never_advertises_a_no_op`, `u3_hint_bar_drops_whole_pairs`,
  `u3_help_hint_survives_truncation`,
  `u4_help_covers_every_action`, `u4_help_fits_and_counts_at_60x16`,
  `u4_help_search_filters_in_place` (U4 clause (3): typing `/` filters the list in
  place and the counter tracks the filtered total),
  `u4_help_dismiss_key_has_no_side_effects` (U4 clause (4): a dismiss key closes
  the overlay and leaves the screen beneath unchanged — the assertion `src/app.rs`
  currently fails, since any key dismisses),
  `u5_each_degraded_state_has_exactly_one_primary_action`,
  `u5_a_healthy_app_draws_no_callout`,
  `u5_notice_survives_the_thinking_state` (the reserved row: a callout is still
  drawn, and the notice row still exists, while the selected employee is thinking —
  §4.2's decision that the indicator yields rather than the notice),
  `u6_card_names_what_it_waits_on`, `u6_stalled_after_30s_without_an_event`,
  `u7_focus_mode_restores_the_exact_rects`,
  `u8_double_click_opens_chat`, `u8_every_pointer_action_has_a_key`,
  `u9_grid_title_reports_hidden_rows`,
  `u10_banner_names_the_state_and_the_event_age`,
  `u10_reconnect_keeps_queued_work`,
  `u11_every_error_carries_a_class_and_an_id`,
  `u11_no_unclassified_error_site`,
  `u12_receipt_walks_queued_to_completed`,
  `u12_retry_reuses_the_admission_id`.
- `src/ui/actions.rs` (new) — `every_action_has_a_label_and_a_context`,
  `action_ids_are_unique`, `deprecated_bindings_still_resolve`.
- `src/ui/mod.rs` — `hint_bar_keeps_the_help_hint` (U3(c) at unit level: the bar
  keeps its `? help` slot when pairs are dropped), and `help_overlay_is_a_window`
  (**the §2 marker assertion** — the bordered title ` help ` plus the
  `rows a-b/n` window counter — and not a scroll-only seam; §2's note and U4's
  cells both rely on it, so its label is part of the contract).
- `src/ui/pipeline.rs` — `card_keeps_its_glyph_below_twelve_columns`.
- Cited as-is and expected to stay green:
  `tests/grid.rs` (every card resolves to its employee, no employee clickable that
  is not drawn — U8's hit-testing clause), `tests/sidebar.rs` (A1–A10, which U9
  must not disturb), `clicking_outside_a_target_keeps_the_selection`
  (`src/app/tests.rs:4219`) for the selection invariant, and — as the two guards on
  §4.2's reserved notice row — `the_prompt_band_does_not_shrink_the_panels`
  (`src/app/tests.rs:2205`) and `the_roster_prompt_band_comes_out_of_the_log`
  (`src/ui/layout.rs:213`, whose `assert_eq!(roster.main, chat.main, …)` at `:222`
  is the layout-level form). Both assert `main` is unchanged; the reserved-row
  decision holds the prompt band at `PROMPT_HEIGHT`, so both must pass unmodified.

---

## 7. Freeze rules

1. A criterion is **never weakened** to make a build pass; changing one is a
   deliberate edit recorded in §9.
2. The evidence log (§8) and §5's Evidence column are filled by the reviewer with
   test name + file:line + observed output. A criterion with no covering test is
   recorded as *not evidenced*; it is **never** marked pass on inspection alone.
3. **U9 may not reach into the sidebar's budget.** Any change to
   [`acceptance-sidebar.md`](acceptance-sidebar.md) §2 (line budget) or §3 (size
   table) must be made in that file, together, and announced to the tester before
   the sidebar matrix is re-run. This file states the gap; it does not resolve it.
4. **A marker must be unique to its surface** (§2). Adding a marker that also
   appears in another surface's text invalidates that criterion and is a spec
   change, not an implementation detail.
5. **§4.1 is generated by the tester.** Editing a matrix cell by hand is a bug;
   change §4 and regenerate.
6. **The registry is the only place a key is bound** (U1). A surface that composes
   its own key literal is a FAIL even if it happens to display the right key,
   because the drift B1 records is exactly what that allows.
7. **No criterion may require the palette.** U2 is additive; if satisfying U2
   makes any existing action reachable only through it, that is a U1/U3 failure.
8. **Old bindings are not removed in this change** (UX3). Deprecation is recorded;
   amputation is a 0.23 change.

---

## 8. Evidence log

The tester appends one block per run; the reviewer signs each matrix cell and
fills §5's Evidence column.

**§8 is append-only.** A run's row records what *that run* observed, in the state
the tree was in when it ran; it is never backdated or retroactively restated. When
a later revision changes a criterion's live status — as rev 5 did by deciding the
two `blocked` cells — the restatement belongs in that revision's §9 entry and in
the prose below the table, never in the earlier run's Result cell. Editing a
historical Result cell destroys the audit trail this log exists to provide: a
reader could no longer tell whether run 1 saw 46 + 2 or 48, and therefore could not
tell whether §4.2's decision was made before or after the run.

| Run | Date | Sizes | Result | Notes |
|---|---|---|---|---|
| 0 | 2026-10-20 | — | **not run** | Spec frozen before implementation (P0/T0.1). No U-criterion is evidenced; the baseline defects in §0 are measurements of the tree at hand-off, not test results. |
| 1 | 2026-10-21 | 120×40 · 100×30 · 80×24 · 60×16 | **0/48 evidenced** — 46 *not evidenced*, 2 *blocked* | §4.1 filled. Every §6 symbol is still absent (`resolved=0 of 31`), so no cell is claimed. Baseline B1–B9 verified against the live tree; **B4 holds** (its 93 is the `src/app.rs` occurrence count; rev 7 corrected the earlier miscount, which had treated `src/ui/prompt.rs`'s `#[cfg(test)]` literals as production) and **B6 was imprecise** (the tasks panel prints bare `no employees`) — both corrected in §0, B4 in rev 7 and B6 in rev 8. Suite green: 480 passed, 4 ignored, 0 failed. |

**Run 1 — what was executed.** `cargo test` (480 passed, 4 ignored, 0 failed —
441 lib + 1 `grid` + 10 `opencode_fake` + 12 `render` + 4 `scheduler` + 11
`sidebar` + 1 `watch_hook`; the 4 ignored are the live-service tests). No test was
written: this run establishes the matrix and re-checks the baseline, which is what
T4.2 asks for before T1.1. Per-size render probes read the frame through
`ui::team_area` / `ui::sidebar_area` / `ui::grid_cells`, the same geometry the
renderer draws into.

**Why 0/48 is the expected result.** §6 freezes **34 new test names** and none
resolves today — `tests/ux.rs` and `src/ui/actions.rs` do not exist. A cell can
only be filled with a symbol that exists plus observed output (§7 rule 2), so all
48 cells are `not evidenced` — 46 were, and 2 were `blocked` until rev 5 named
their host (§4.2), which weakened no requirement but did add a host clause to each
and a "never both and never neither" obligation to U9. Reporting anything else here
would be a claim dressed as evidence. *(Run 1 measured 31; §6 has since gained
`u5_notice_survives_the_thinking_state` (rev 9), two guard citations (rev 10), and
U4's search and dismiss names (rev 15), so the current inventory is **34 new + 3
existing**. None of the 34 resolves either way.)*

**Marker baseline at the four axes** (a 12-employee roster so the grid overflows
and U9 has rows to hide). **None of the 14 §2 markers appears in any frame:**

| Size | markers present | grid title | `? help` in bar |
|---|---|---|---|
| 120×40 | none | `team · 12 · rows 1-3/4` | present |
| 100×30 | none | `team · 12 · rows 1-2/4` | **absent** |
| 80×24 | none | `team · 12 · 1-1/12` | **absent** |
| 60×16 | none | `tea` | **absent** |

With the help overlay open, **no** `rows a-b/n` counter and **no** search affordance
is drawn at 120×40 or 60×16 — U4's two decisive clauses fail at both.

### Run 1 — §0 baseline verification (B1–B9)

Every claim below was re-derived from the live tree at the pinned revision, not
read back from the doc. **Two claims do not hold; both are reported here rather
than edited into §0**, per the request that a wrong baseline be reported.

| # | Verdict | What was measured |
|---|---|---|
| B1 | **holds, exactly** | `hint_bar` roster list = **11** pairs (`src/ui/mod.rs:331-345`); `help_lines` = **24** `key(` rows under **6** `head(` headings plus 5 blank separators (`:423-471`); `CHAT_COMMANDS` = **10** entries (`src/app.rs:268`). All three cited ranges land on the named functions. |
| B2 | **holds, exactly** | The summed span width is **120** columns (`" {key} " + "{description} "` over the 11 roster pairs). Drawn: 119/120 at 120×40, 99/100 at 100×30, 78/80 at 80×24, 59/60 at 60×16 — i.e. the terminal always clips. The losses match the doc word for word: at 100 the line ends `Ctrl-Y copy` (loses `log · ? help · q quit`); at 80 it ends `Space pause` (loses `s task · Ctrl-Y copy log · ? help · q quit`); at 60 it ends `m`. `? help` is absent at 100, 80 and 60. |
| B3 | **holds, exactly** | `centered(area, 68, 30)` (`:474-483`); `help_lines` = 24 + 6 + 5 = **35** body rows. At 60×16 the clamp gives **58×14** → **12** content rows for 35, 23 unreachable. No scrolling, no search in `render_help_overlay`. |
| **B4** | **holds, once test-module literals are separated** | The four `KeyCode::` literals in `src/ui/prompt.rs` all sit inside `#[cfg(test)] mod tests` (module at `:136`; literals at `:157`, `:202`, `:232`, `:282`), so they are test code, not production bindings. Separated on that basis: **75 lines / 93 occurrences** in `src/app.rs` — production lives in one file — plus **4 / 4** test-only in `src/ui/prompt.rs` and **87 / 87** in `src/app/tests.rs`; totals **166 / 184**. §0's **93** is therefore correct as the `src/app.rs` occurrence count, and the earlier reading that treated `prompt.rs` as production (79 lines / 97 occurrences) was the error. The rest of B4 **holds**: **7** `match key.code` blocks, **4** `Mode` variants, and `?` (`:1017`) and `Ctrl-Y` (`:1027`) are handled globally outside every mode block. |
| B5 | **holds** | Only `MouseEventKind::Down(MouseButton::Left)` is matched (`src/app.rs:1204`); no `Up`, no double-click, no click-count tracking. `Target` has **4** variants — `Employee`, `Prompt`, `Project`, `Model` (`src/ui/hit.rs:15-24`) — all selections. |
| **B6** | **was imprecise — corrected in §0 (rev 8)** | As first written, B6 claimed "the grid and tasks panel print `no employees — add one with \`mywaker employee add\`\`", which is wrong for the tasks panel: it prints the bare **`no employees`** (`src/ui/tasks.rs:33`), not the long string. Only the grid prints the long one (`src/ui/pipeline.rs:43`). The project picker (`:62`) and model picker (`:49`) strings match verbatim. **§0 now attributes each string to its own surface**, so the finding is recorded at the source rather than only here. The substantive point is unchanged — of the four, only the grid names an action, the project picker names a *shell* command the TUI cannot run, the model picker names none, and "service is down" has no callout. |
| B7 | **holds, exactly** | `fit_sections` (`:176-191`) breaks out of the loop and returns the prefix with no report; called from `fit` (`:167`). The grid title does report `rows a-b/c` on overflow (`src/ui/pipeline.rs:621-634`), and degrades to `1-1/12` at 24 columns and to `tea` at 4 — so it is a ratio, and it disappears entirely at floor width. |
| B8 | **holds, exactly** | **9** `push_error_log` call sites, and all nine cited line numbers (`913, 978, 2112, 2757, 2882, 3519, 3935, 4045, 4071`) land on a call. Signature is `fn push_error_log(&mut self, line: String)` (`:4231`) — free-form, no class, no id. Log band 4 rows resting / 3 while thinking (`src/ui/layout.rs:42-43`). |
| B9 | **holds, exactly** | `connection_line` (`:194-212`) returns exactly three shapes — error glyph + message, `● opencode v{version} · N agents`, or spinner + `connecting…`. No age, no banner. |

**Consequence for §0.** **No correction to B4 is needed:** its **93** is the
`src/app.rs` occurrence count and is right, and its 75/166 line figures are right
too — the earlier "93 → 79" proposal came from counting `src/ui/prompt.rs` as
production, when all four of its literals are inside `#[cfg(test)] mod tests`. §0
B4 was rewritten in rev 7 to state the production/test split explicitly, so run 2
inherits exact denominators. The criterion is unaffected either way, because U1
asserts *where* `KeyCode::` appears, not how many. **B6 was corrected in §0 in rev
8**, alongside B4: the sentence now attributes the long string to the grid
(`src/ui/pipeline.rs:43`) and the bare `no employees` to the tasks panel
(`src/ui/tasks.rs:33`). Its conclusion is unchanged — of the four degraded
surfaces, only the grid names an action, the project picker names a *shell*
command the TUI cannot run, the model picker names none, and "the service is down"
has no callout at all.

**Open at freeze — updated by run 1.** These are recorded so they cannot quietly
become passes:

- **Every U-criterion is un-evidenced.** No test in the tree asserts any of them;
  run 1 confirms `resolved=0 of 31` §6 names.
- **The §6 test names are requirements, not symbols — with three exceptions.** Of
  the names frozen there, three exist: `clicking_outside_a_target_keeps_the_selection`
  (`src/app/tests.rs:4219`), and the two layout guards
  `the_prompt_band_does_not_shrink_the_panels` (`src/app/tests.rs:2205`) and
  `the_roster_prompt_band_comes_out_of_the_log` (`src/ui/layout.rs:213`). All three
  are cited to stay green, not as new obligations. Every other name is a
  requirement: `tests/ux.rs` and `src/ui/actions.rs` do not exist.
- **The two former `blocked` cells are decided** (§4.2, rev 5) and are now
  *not evidenced* like the rest. Both were blocked on the assumption that the grid
  panel had to host the prose; it does not. The callout (U5) and the `hidden N`
  fit report (U9) are drawn on the roster's **notice row** — the log band's first
  content row — which is 58 columns usable at 60×16 against a 34-column callout.
  No requirement was weakened; both criteria gained a host clause, and U9 gained
  the "never both and never neither" obligation. T1.1 can now make them decidable,
  and no spec decision is outstanding.
- **U9's sidebar clause is deferred** (§4, U9) and requires a revision of
  [`acceptance-sidebar.md`](acceptance-sidebar.md) §2 and §3 together.
- **`docs/keymap.md` now exists** (T0.2, written 2026-10-21, after the freeze), so
  UX3 and UX6 have become checkable — but no test compares README, `keymap.md`, and
  this spec, so both remain `not evidenced`.
- **40×24 is not pinned** (§3): the team panel is zero-width there, so U2–U12 are
  not decidable at that size.

---

## 9. Revision log

| Rev | Date | Change |
|---|---|---|
| 1 | 2026-10-20 | Created from the UX overhaul plan (P0/T0.1): §0 records the measured baseline (B1–B9) and states that every criterion fails today; §2 freezes nine surface markers with their uniqueness checked against the tree; §3 pins four breakpoints, derives their geometry, and records the single width-driven relaxation and the one size left unpinned; §4 freezes U1–U12; §4.1 leaves the 48-cell matrix to the tester (T4.2) with its axes fixed; §5 adds UX1–UX6 for the cross-cutting and non-render obligations; §6 freezes 31 test names that do not yet exist; §7 rule 3 records the sidebar-budget boundary rather than crossing it. Status: frozen, awaiting implementation. |
| 2 | 2026-10-21 | **U2 changed deliberately, per rule 1** — the palette key is now `Ctrl-P` primary with `Ctrl-K` as an alias, and U2 gained the text-field exception (unbound while the goal prompt or a picker's filter has the keyboard, since both are line-editing keys there). This is not a weakening: the "from every `Mode`" guarantee is intact, and the exception is the *narrower* claim, asserted by a new frozen test (`u2_palette_is_unbound_while_typing`) rather than dropped. Recorded when the palette and help specs (`docs/keymap.md`, `docs/help-palette.md`) were folded in, so the criteria and the specs cannot disagree. |
| 3 | 2026-10-21 | **Run 1 (T4.2) filled §4.1 and §8.** All 48 cells written: **46 *not evidenced*, 2 *blocked*, 0 claimed**, because every §6 symbol is still absent (`resolved=0 of 31`) — the expected result for a spec frozen before implementation. The 2 blocked cells (U5 and U9 at 60×16) are undecidable at that width and are recorded with the measured reason. §0's baseline was re-derived against the live tree: **B1, B2, B3, B5, B7, B8, B9 hold exactly, and B4 holds too** (its 93 is the `src/app.rs` occurrence count — corrected in rev 7, which separated `src/ui/prompt.rs`'s test-module literals from production); **B6 misdescribed the tasks panel** (it prints bare `no employees`), corrected in §0 in rev 8. No criterion was altered. |
| 4 | 2026-10-21 | **§6's preamble corrected, no criterion changed.** It claimed "**None** of these exist yet" while its own last bullet cites `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:4219`), which is in the tree and green — so the sentence was false as written and made the `31` in §4.1/§8 look like a miscount of the 32 symbols §6 lists. The preamble now states **31 unresolved + 1 already existing**, and that the existing one is cited so a criterion can name a guard it must not disturb. Verified at this revision: all eight §"Tree pin" hashes match; `push_error_log`'s 9 cited lines all land on calls (`:913, 978, 2112, 2757, 2882, 3519, 3935, 4045, 4071`); the four-axis geometry in §4.1 reproduces `24 \| fill \| 32` and `MIN_CELL_WIDTH = 12`; §2's 15 marker literals split **14 absent / 1 already drawn**: the 14 return 0 hits in `src/ui/`, and U4's bordered title ` help ` is already drawn at `src/ui/mod.rs:481` — which is why §8's "14 §2 markers" is correct as written and is not a miscount. U1–U12 and the 48 cells are unchanged. |
| 5 | 2026-10-21 | **The two `blocked` cells are decided by naming a host — no requirement weakened (§4.2).** U5 @ 60×16 and U9 @ 60×16 were blocked on the assumption that the *grid panel* had to host the prose; at the floor that panel is 4 columns and a card is 3, so nothing sentence-shaped can be drawn there. Neither criterion named a host, so the fix is the host: the roster's **notice row** — the log band's first content row, below the pinned error — carries the degraded-state callout (U5) and, where the grid title can no longer hold it, the `hidden N` fit report (U9). It adds no `Regions` field, so `main` never moves and U7's byte-identical rect restoration is untouched; the log is already the band that gives way and already pins its most important line first. At 60×16 the notice row is 58 columns usable against a 34-column longest callout (46 with the fit suffix), so both markers are fully readable and "exactly one primary action" is assertable at all four sizes. The grid now degrades to the `◇` glyph rather than a sentence, which also retires B6's shell-command string from the grid. **U5 and U9 each gained a host clause, and U9 gained a new obligation, so this is not a text-neutral edit:** U5 now fixes its host ("the roster's notice row at every size, so its host does not change with the grid's width"); U9's "its title reports" became "drawn on the grid's title while the title can hold it, and on the notice row below that — **one host or the other, never both and never neither**". That prohibition is *stronger* than the text it replaced — it forbids the count going missing as well as being duplicated — so no requirement was weakened, but the criteria did grow and rule 1's record should say so rather than claim "nothing else" changed. The cells moved from `blocked` to `not evidenced`, so the run-1 count is restated as 48 `not evidenced`; §8's run-1 row keeps its original 46 + 2 with the change noted rather than backdated. |
| 6 | 2026-10-21 | **U5's 100×30 and 80×24 cells restated on the notice row — §4 U5 unchanged.** Rev 5 named the notice row as U5's host at *every* size ("its host does not change with the grid's width") but left two matrix cells measuring the wrong surface: 100×30 still said "callout fits 44 grid columns" and 80×24 said "callout fits 24 columns — the narrowest size at which the longest marker can still be read". The second was **false twice over** — the longest callout is 34 columns, so the 24-column grid at 80×24 could never have held it, and 60×16 has 58 usable notice-row columns, so 80×24 was not the narrowest readable size either. Both cells now state the notice row as host with the §4.2 figures (98 and 78 usable columns against a 34-column callout, 46 with the `hidden N` suffix). The 120×40 cell gained the same host clause so the three "As above" cells no longer point at a referent that never named one. No pass condition, marker, or criterion changed; `docs/verification-matrix-ux.md` already had these rows right and needed no edit. |
| 7 | 2026-10-21 | **B4 corrected at the source — §0 rewritten, and §8's verdict reversed from "COUNT WRONG" to "holds".** Rev 3's run-1 note claimed B4's **93** was wrong and should be **79**. That was the error: all four `KeyCode::` literals in `src/ui/prompt.rs` sit inside `#[cfg(test)] mod tests` (module at `:136`; literals at `:157`, `:202`, `:232`, `:282`), so they are test code, and production literals live in **one** file, `src/app.rs`. §0 B4 now states the split explicitly — **75 lines / 93 occurrences** in `src/app.rs`, **4 / 4** test-only in `src/ui/prompt.rs`, **87 / 87** in `src/app/tests.rs`, totals **166 / 184** — and its 93 is untouched because it is correct. §8's B4 row reads **"holds, once test-module literals are separated"** and records that the earlier 79/97 reading was the mistake; the "93 should be corrected to 79" sentence is deleted; rev 3's entry and run 1's summary row now say B4 holds. Verified at this revision: `grep -c` gives 75/4/87 and `grep -o \| wc -l` gives 93/4/87, and `awk 'NR<136'` finds **no** production `KeyCode::` in `src/ui/prompt.rs`. **One consequence left open, not acted on:** U1's clause (b) says `KeyCode::` may appear "inside the registry module and the text editors", but `src/ui/prompt.rs` has no production literal, so the text-editor carve-out is currently empty. That is a possible wording change to U1, which this edit does not make — no criterion is altered here. |
| 8 | 2026-10-21 | **Baseline corrected, no criterion changed.** Two §0 rows were fixed at the source: **B6** now says the grid prints `no employees — add one with \`mywaker employee add\`` (`src/ui/pipeline.rs:43`) and the tasks panel prints the bare `no employees` (`src/ui/tasks.rs:33`), where it previously attributed the long string to both; its conclusion is kept verbatim — only the grid names an action, the project picker names a shell command the TUI cannot run, the model picker names none, and "the service is down" has no callout. **B4** (already rewritten in rev 7) is the other §0 edit this round: 75/93 in `src/app.rs`, 4/4 test-only in `src/ui/prompt.rs`, 87/87 in `src/app/tests.rs`, totals 166/184, with its 93 untouched. §8's B6 verdict now reads "was imprecise — corrected in §0 (rev 8)" and the "reported, not edited" wording is removed from §8's consequence note, rev 3's entry, and run 1's summary row. **Numbering note:** this entry is rev 8, not rev 6 as requested — rev 6 was already the U5 notice-row restatement and rev 7 the B4 correction, and renumbering would have invalidated the `rev 5`/`rev 7` references §4.1, §8 and `docs/verification-matrix-ux.md` carry. No U-criterion, marker, cell, or pass condition was touched. |
| 9 | 2026-10-21 | **§4.2's notice row is now reserved, so U5/U9 stay decidable while thinking — and the true cost of the row is stated.** Rev 5 put the notice in the log band's second content row without checking the band's height in the thinking state: `log_height` is 3 while thinking (`src/ui/layout.rs:42`) and content rows are `log_height − 2`, so thinking leaves **1** row, the pinned error takes it, and the notice had nowhere to go — the block rev 5 closed reopened in exactly the state a user is most likely to be watching. **Decision: the notice row is reserved and the thinking indicator yields.** When a grid-scoped notice exists, `Regions::of` receives `thinking && notice.is_none()`, the prompt band stays at `PROMPT_HEIGHT` (2) instead of taking `PROMPT_THINKING_HEIGHT` (3), the log keeps 4 rows / 2 content rows, and `main` is unchanged in every combination — so `the_prompt_band_does_not_shrink_the_panels` still holds. The indicator is suppressed only in that state and loses nothing: the card glyph and the sidebar's `now` line both still animate it. U5 and U9 are therefore asserted identically in every employee state; no cell is relaxed and none gained a thinking-specific exception. The `u5_notice_survives_the_thinking_state` name was frozen for it (§6 now 32 new + 1 existing). **The cost claim is corrected too:** "at rest it costs one log line" understated it — with an error pinned, error + notice consume both content rows, so `stream_stats`/`telemetry_stats` are displaced *entirely* whenever a notice is up, at rest as well as thinking. §4.2 now carries a six-state table saying so, and records that U11 is asserted on the pinned error row (never displaced) rather than on the tail (which does yield). No U-criterion, marker, pass condition, or cell status changed. |
| 10 | 2026-10-21 | **§4.2's `main`-invariant citation corrected — no criterion changed.** Rev 9 cited the frozen invariant as "`the_prompt_band_does_not_shrink_the_panels` (`src/ui/layout.rs:195-225`)", which was wrong on both halves: that test is in `src/app/tests.rs:2205`, and `src/ui/layout.rs:195-225` holds two *different* tests — `regions_stack_the_frame` (`:199`) and `the_roster_prompt_band_comes_out_of_the_log` (`:213`). Test and assertion are now cited separately everywhere they appear: **the test** `the_prompt_band_does_not_shrink_the_panels` (`src/app/tests.rs:2205`) asserts `main.height == 32` at 120×40 and that the band sits directly under `main`; **the layout-level assertion** is `assert_eq!(roster.main, chat.main, "the prompt band must not shrink the panels")` inside `the_roster_prompt_band_comes_out_of_the_log` (`src/ui/layout.rs:222`, test at `:213`). Both now also appear in §6's "cited as-is and expected to stay green" list, because rev 9's reserved-row decision holds the prompt band at `PROMPT_HEIGHT` and therefore depends on both passing unmodified. Verified: `grep -rn "fn the_prompt_band_does_not_shrink_the_panels" src/` returns `src/app/tests.rs:2205` and nothing in `src/ui/layout.rs`; the layout test list is `roster_panels_tile_the_main_area` (`:184`), `regions_stack_the_frame` (`:199`), `the_roster_prompt_band_comes_out_of_the_log` (`:213`), `the_prompt_band_degrades_on_a_short_frame` (`:247`). No U-criterion, marker, cell, or pass condition was touched. |
| 11 | 2026-10-21 | **§8's run-1 Result cell restored to what run 1 observed — the rev 5 restatement moves to rev 5's entry, where it belongs.** Rev 5 wrote "§8's run-1 row keeps its original 46 + 2 with the change noted rather than backdated", but the row's Result cell had since been changed to lead with "48 *not evidenced* (as of rev 5…)" — so the log contradicted its own promise and the run-1 row no longer recorded a run-1 observation. The cell now reads **"0/48 evidenced — 46 *not evidenced*, 2 *blocked*"** again, with the rev-5 restatement carried only in rev 5's entry and in §8's prose below the table, which already reconciled the two honestly ("46 were, and 2 were `blocked` until rev 5 fixed their host"). §4.1's header is unchanged: it describes the *current* state ("46 … and 2 … at run 1, all 48 … as of rev 5") and is not a run record, so it is not backdating. **§8 now states the append-only rule explicitly** — a run's row records what that run observed and is never retroactively restated; later changes belong in the revising §9 entry and the prose — because this is the second time a historical row was edited to carry a later decision. No U-criterion, marker, cell, or pass condition was touched. |
| 12 | 2026-10-21 | **Five stale statements corrected — no criterion changed.** (a) The status banner still read "Evidence log empty (run 0)" while §8 has run 1; it now says run 1 recorded, with its 46 + 2 and rev 5's decision. (b) §5 UX3 claimed `docs/keymap.md` "does not exist yet"; it exists (85,253 B, written 2026-10-21) and §8 already said so — UX3 and UX6 now read "now exists … so the criterion is checkable; no test asserts it", matching §8 and `docs/verification-matrix-ux.md` §5, which were already right. (c) §8's "Why 0/48 is the expected result" still said "§6 freezes 31 test names and none resolves today", the same error corrected in §6 in rev 4; it now says 31 *new* names as run 1 saw them, with rev 9's 32 noted as a later addition. (d) §8's "Every §6 test name is a requirement, not a symbol" was false for `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:4219`), which exists and is cited as a guard; the bullet now states that exception, and names the two layout guards as existing tests rather than obligations. (e) `("?", "help")` is at `src/ui/mod.rs:343`, not `:344` — corrected in §0 B2 and in §2's U4 marker note (verified: `sed -n '343p'` is the pair, `:344` is `("q", "quit")`). Every cited line was re-read at this revision; no U-criterion, marker, cell, or pass condition was touched. |
| 13 | 2026-10-21 | **Rev 5's entry reworded so rule 1's record is honest — the criteria did grow.** Rev 5 claimed "no pass condition changed" and that U5/U9 "each gained one clause naming the host and nothing else". Both overstate. §4 U5 gained a normative sentence fixing its host ("the roster's notice row at every size, so its host does not change with the grid's width"), and §4 U9's "its title reports" was replaced by "drawn on the grid's title while the title can hold it, and on the notice row below that — **one host or the other, never both and never neither**", which is a *new prohibition*: it forbids the count going missing as well as being duplicated. No requirement was weakened, so rule 1 was not invoked to relax anything — but the criteria are stronger, not unchanged, and a log that says otherwise would misrepresent what a reviewer is verifying against. Rev 5's entry now reads "no requirement weakened", states both text changes explicitly, and records the U9 prohibition as the strengthening it is. The same overstatement was removed from §8's "Why 0/48" paragraph and from §8's "Open at freeze" bullet ("Neither pass condition changed; only the host is fixed"), which now say the host was named and U9 gained an obligation. Rev 6's "§4 U5 unchanged" is correct and left alone: it changed §4.1 cells, not the §4 criterion. No U-criterion text, marker, cell, or cell status was altered by this entry. |
| 14 | 2026-10-21 | **§6's inventory count corrected to 32 new + 3 existing, and `docs/verification-matrix-ux.md` reconciled to this file.** Rev 10 added two pre-existing guard tests to §6's stay-green list (`the_prompt_band_does_not_shrink_the_panels`, `src/app/tests.rs:2205`; `the_roster_prompt_band_comes_out_of_the_log`, `src/ui/layout.rs:213`) but left §6's own count saying "32 new names plus 1 existing test". The true inventory is **35 symbols: 32 new + 3 existing** (the selection invariant and the two `main`-height guards), so §6's preamble, §4.1's header, §8's "Why 0/48" paragraph and §8's requirements bullet now all say 32 + 3. The matrix companion was regenerated against the current bytes and reconciled: its pin now names **rev 13** rather than the rev 12 label an earlier pass had put on bytes that already contained rev 13; its §4.4 audit now states it covers the 32 *new* names and lists the 3 guards separately as existing tests that can back no cell by construction (verified: no cell cites any of the three); and its rev-5/rev-9 prose was reworded to "no requirement weakened … the criteria did grow", matching rev 13 here. All 48 matrix rows were re-compared to §4.1 by script and are **byte-identical, 48/48**, none `blocked`. The reported stale pin (`fcafe12f`/"rev 3"), the 2 stale `blocked` rows, and the "(freeze rule 3)" reference were already corrected in the concurrently regenerated matrix before this entry — verified absent at this revision — so they are recorded as closed rather than re-fixed. No U-criterion, marker, cell, or cell status was altered. |
| 15 | 2026-10-21 | **Two author decisions taken on U4's frozen names (§6, §2, §4.1) — additive, no criterion weakened.** (a) **U4's `/` search and dismiss clauses gain their own frozen names.** U4 asserted four things but §6 froze only two U4 names, both scoped to coverage/fit/counter; the U4 @ 60×16 cell demanded *"scrolling and `/` search must both work"* while citing symbols whose names scope that out, making the cell **unfillable as written** under rule 2. §6 now also freezes `u4_help_search_filters_in_place` (typing `/` filters in place, counter tracks the filtered total) and `u4_help_dismiss_key_has_no_side_effects` (a dismiss key closes the overlay and leaves the screen beneath unchanged — the behaviour `src/app.rs` currently lacks, since any key dismisses). This follows the sidebar spec's precedent of one name per decisive clause, and no U4 clause was dropped. (b) **`help_overlay_is_a_window` is the bordered-title marker assertion, not a scroll-only seam.** §2 already promised that U4's §6 test "asserts on structure rather than on a string"; this names which symbol does it — the bordered title ` help ` plus the `rows a-b/n` window chrome. U4's cells now cite it alongside the four `tests/ux.rs` names, and §6 labels it (with `hint_bar_keeps_the_help_hint` labelled as U3(c) at unit level). No third name was needed for the border. §6's inventory moves to **34 new + 3 existing = 37**; §4.1's header and §8's "Why 0/48" paragraph follow. Also fixed: **UX3's evidence cell now cites the deprecation register at `docs/keymap.md` §3.7** (`:662`), matching `docs/verification-matrix-ux.md` §5, which already said so — the spec was behind its own restatement. No pass condition, marker, or cell status changed; all 48 cells remain `not evidenced`. |
| 16 | 2026-10-21 | **§6's preamble and §8's "Why 0/48" sentence restated to match the list — no criterion changed.** The preamble said "32 new + **1** existing … 32 unresolved, **1** resolved" and pointed at "the last bullet" as holding "the one that already exists". Neither matched the list, which holds **34 new + 3 existing = 37**: rev 10 had added `the_prompt_band_does_not_shrink_the_panels` and `the_roster_prompt_band_comes_out_of_the_log` to that bullet as reserved-row guards, and rev 15 added U4's two clause names. The preamble now reads **34 unresolved + 3 resolved**, and describes the final "cited as-is" bullet accurately: **three existing test symbols** (the selection invariant plus the two `main`-height guards) and **two guard test files** (`tests/grid.rs`, `tests/sidebar.rs`), which is what that bullet actually names. §8's "Why 0/48 is the expected result" also led with "§6 freezes 31 new test names" while its own parenthetical three lines later said 32 — the leading figure is now **34**, and the parenthetical is rewritten to trace 31 → 34 (rev 9's name, rev 10's two guards, rev 15's two U4 names) instead of stopping at 32. Verified by script against the §6 list: 37 unique symbols, 34 new, 3 existing. §4.1's header already read 34 + 3 and is unchanged. No U-criterion, marker, cell, or pass condition was touched. |
