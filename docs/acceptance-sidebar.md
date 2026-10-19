# mywaker — Sidebar acceptance criteria

Frozen verification contract for the left sidebar (the `waker` panel turned
inspector + vitals column). Companion to [`acceptance.md`](acceptance.md), which
owns pause/continue (M6.5–M6.6); this file owns the sidebar and **nothing else**.

> **Status: frozen spec — implemented; evidence log open.**
>
> The reviewer verifies against *this file*, never against a conversation turn. A
> criterion is never weakened to make a build pass (see §7).

Related: [`architecture.md`](architecture.md) §5 (styling), [`todos.md`](todos.md)
M4, and the design plan in [`PLAN.md`](../PLAN.md).

**Tree pin (rev 7).** Every `file:line` citation in this document was resolved
against the working tree at hand-off, not against a commit — the sidebar work is
uncommitted, so a bare commit id would not reproduce them. To make "pinned to the
tree" checkable:

- Base commit: `76b6dac` (`fix jev reply form`); 22 files are modified on top of it.
- Content at hand-off, `shasum` prefixes: `src/ui/sidebar.rs` `e3f4534`,
  `src/ui/robot.rs` `91feaea`, `src/ui/pipeline.rs` `608b49c`,
  `src/app/tests.rs` `e878cd5`, `tests/sidebar.rs` `b2da68a`.

If a citation does not resolve, compare those hashes first: a mismatch means the
tree moved after hand-off, and the line numbers should be re-derived rather than
trusted.

**D1 — fixed in this change, outside this spec's rows.** The short-panel defect
(at 80×20 the pipeline's fixed row lengths exceeded the main area, so the planner
box lost its bottom border) is closed. The pipeline panel now degrades by dropping
whole bands instead of letting the layout squash the first one: the bands go in
the order that costs the user least — the Jev header first, then whole card rows —
so a card is only counted when its full `STAGE_HEIGHT` fits and is never drawn
with a top border and no body (`ui::pipeline::shape`, `src/ui/pipeline.rs:131-193`).
The guard is `grid_drops_the_header_before_a_card_row` (`src/ui/pipeline.rs:589`);
`grid_cards_are_disjoint_complete_and_inside_the_area` (`:500`) asserts the general
case across roster sizes and panel heights.

Evidence — 80×20, the size that used to fail (`main` is 12 rows). Verbatim apart
from the live RAM figure, right-trimmed, from `TestBackend` — the `ram` cell in the
telemetry line is a live process measurement, so it will differ between runs (this
capture read 3.8 MiB; a re-run may read 4.0 MiB). Same caveat as
[`RELEASING.md`](../RELEASING.md) §2.3 ("If the release touches the UI"), which
calls the `ram`/`cpu` readout the one known nondeterminism in a cell dump:

```
 0| mywaker  a team of AI employees                                   ⠋ connecting…|
 1|┌ waker ───────────────┐ team · 4 · rows 1-1/4  ┌ tasks ───────────────────────┐|
 2|│  ⠋   ◉   ◉           │ jev ◆ off              │● Dex · Planner               │
 3|│  ┌─⠋─┐               │┌ PLANNER ─────────────┐│  turn the last meeting i…    │
 4|│MYWAKER               ││● Dex                 ││                              │
 5|│  └─────┘             ││Planner               ││● Aria · Research lead        │
 6|│                      ││idle                  ││  turn the last meeting i…    │
 7|│now ⠋ connecting      ││none                  ││                              │
 8|│                      │└──────────────────────┘│● Bruno · Writer              │
 9|│                      │                        │  turn the last meeting i…    │
10|│                      │                        │                              │
11|│                      │                        │● Cleo · Reviewer             │
12|└──────────────────────┘                        └──────────────────────────────┘|
13|┌ log ─────────────────────────────────────────────────────────────────────────┐|
14|│ stream 0 agents · 0 events                                                   │
15|│ usage 0/0 in/out · cache 0% (0) · ram 3.8 MiB · cpu 0.0%                     │
16|│connecting to OpenCode…                                                       │
17|│                                                                              │
18|└──────────────────────────────────────────────────────────────────────────────┘|
19| ↑↓←→ move  wheel scroll  Enter chat  p projects  m model  Space pause  s task
```

The planner card is complete — top border row 3, bottom border row 8 — with the
Jev strip above it and the grid title reporting how much of the roster is on
screen (`rows 1-1/4`: one of four card rows visible, which is also the scroll
affordance). Only whole rows are drawn: the remaining three cards are off-screen
and reachable by wheel/arrows, not clipped into half a box.

**No §2 or §3 row was relaxed to make this pass:** the fix is confined to the
pipeline panel, and §2's line budget, §3's presence table, and every sidebar
assertion are unchanged by it.

> **Citation note (rev 4).** An earlier draft of this note cited
> `pipeline::tree`'s level-dropping at `src/ui/pipeline.rs:77-101` and the test
> `tree_degrades_by_dropping_levels_when_the_area_is_short` at `:433`. Those no
> longer exist — the tree was replaced by the card grid in the same change that
> fixed D1, and the drop-the-band-first rule moved into `shape`. The citations
> above are the ones a reader can follow in this tree.

---

## 1. Scope

The sidebar is the **left-most column of Roster mode** — today the 24-column
`waker` robot panel (`ui::layout::roster_areas`, `[0]`). This spec covers:

- the section stack it draws and the priority order it degrades in,
- the size → expected-section matrix,
- the render assertions (A1–A10) at six pinned terminal sizes,
- the non-render criteria (SB1–SB6) for the model-layer additions.

Out of scope: chat/projects/models screens, the pipeline grid, the tasks panel,
the log band, and every dispatch, session, or persistence behaviour. The sidebar
stays a **non-hit-test target**: it is drawn, never clicked (A8).

## 2. Section line budget (frozen)

Width 24 ⇒ **22 usable columns**. `main.height = terminal.height − 8`
(title 1 + log 6 + help 1); `inner = main.height − 2` (the sidebar's borders).

Every section carries a **marker**: a literal string that appears in its first
line and nowhere else in the sidebar. Markers are what A5 asserts on. A section's
**ID is never drawn**, so no "P*n* present" is decidable from a buffer at all;
"`vitals` present" is. (The queue vitals is P4 and its marker is `vitals`. The
example names the ID only to identify the row; the assertion is always on the
marker.)

Markers are chosen to be **unique in the whole frame**, not just the sidebar:
`vitals` rather than `queue` (the tasks panel already draws a `queue` heading)
and `uptime` rather than `up` (too short to be distinctive). A marker that also
appears in another panel would make A5 and A7 disagree, so uniqueness is part of
the contract (§7 rule 6).

| ID | Section | Marker(s) | Gap before | Content lines | Total |
|---|---|---|---|---|---|
| **P0** | Avatar (mini) | `MYWAKER` | — | 4 | 4 |
| **P1** | Now (aggregate) | `now` | 1 | 1 | 2 |
| **P2** | Inspector (selected employee) | `who` | 1 | 4 | 5 |
| **P3** | Pulse (sparkline) | `pulse` | 1 | 1 | 2 |
| **P4** | Queue vitals | `vitals` | 1 | 2 | 3 |
| **P5** | Runtime (uptime · Jev) | `uptime` | 1 | 2 | 3 |
| **P6** | Alerts | `alert` | 1 | 2 | 3 |

Cumulative: **4 · 6 · 11 · 13 · 16 · 19 · 22**.

Each section has exactly one ID. The avatar and the Now line are **separate
sections** — the avatar is P0 and the Now line is P1 — which is what the `fit`
prefix rule and the priority indices in `ui::sidebar::MARKERS` assume
(`src/ui/sidebar.rs:41-43`, priorities 0–6).

**Derivation rule (this replaces hand-computed rows).** For a given `inner h`,
the rendered stack is the **longest prefix of P0…P6 whose cumulative total ≤
inner h**. §3 is the corollary of this rule applied at six sizes — it is stated
there for readability, not as an independent source of truth. A section is
all-or-nothing: it draws all its content lines or it is dropped.

The rule is what makes 60×16 tight and explicit: P0 (the avatar) plus P1 (the Now
line) is exactly 6 rows, and inner h is exactly 6, so P2's gap row would be the
7th row. There is **zero slack** at that size — the avatar must fit in 4 rows (see
§2.1).

### 2.1 The mini avatar (frozen layout)

P0 allots the avatar **4 content rows**, which is a deliberate change from the
current 9-row `robot::render` (`src/ui/robot.rs:19-66`). The spec therefore pins
what those 4 rows are, because A10's `MYWAKER` requirement would otherwise be
unsatisfiable at 60×16:

| Row | Content |
|---|---|
| 1 | antenna + eyes — animated, the only rows A9 reads |
| 2 | mouth + face frame |
| 3 | `MYWAKER` nameplate (the P0 marker; ellipsised only when inner width < 7) |
| 4 | shoulders/base frame |

`robot::render_mini(state, frame) -> Vec<Line>` produces exactly these four rows
and keeps the animation contract of `render` (antenna blink on the 20-tick
period when `Idle`, spinner advance when `Thinking`/`Working`). The 9-row
`render` stays for any future full-height use; the sidebar always uses
`render_mini`. At 40×24 the sidebar's inner width is 6, so row 3 may render as
`MYWAK…` — A10 allows that and says so.

## 3. Size → expected sections (corollary of §2's derivation rule)

| Size | main h | inner h | Sections that MUST appear | Sections that MUST NOT appear |
|---|---|---|---|---|
| 160×50 | 42 | 40 | P0 P1 P2 P3 P4 P5 P6 | — |
| 120×40 | 32 | 30 | P0 P1 P2 P3 P4 P5 P6 | — |
| 100×30 | 22 | 20 | P0 P1 P2 P3 P4 P5 | P6 |
| 80×24 | 16 | 14 | P0 P1 P2 P3 | P4 P5 P6 |
| 60×16 | 8 | 6 | P0 P1 | P2 P3 P4 P5 P6 |
| 40×24 | 16 | 14 | P0 P1 P2 P3 | P4 P5 P6 |

**The 40×24 case, resolved (rev 4).** Vertically, inner h=14 fits P0+P1+P2+P3
(13), so the budget *requires* P2 and P3. Horizontally the nominal
`24 | fill | 32` split does not fit in 40 columns: ratatui gives the sidebar
`Rect::new(0, 1, 8, 16)` (it shrinks the fixed panels and the `Fill(1)` pipeline
collapses to 0). Eight columns is 6 usable.

Earlier revisions made P2/P3 conditional at this size, because 6 usable columns
looked too narrow for the inspector to be legible. **That conditional is retired:
P2 and P3 are mandatory here, exactly as the height budget says.** The panel does
not truncate a section into an unreadable fragment — it ellipsises each row
(`Resea…`, `opena…`), which A6 permits and A10 documents — so the section is
present, complete in row count, and carries its marker. "MUST appear" is no
longer relaxed at any pinned size; the only size-driven relaxation left is the
*rendering* of a narrow row (A4's glyph clause, A10's nameplate clause), never the
presence of a section.

**As built.** The implementation draws P0–P3 at 40×24. Evidence, inner width 6:

```
 1|┌ waker┐|
 2|│  ⠋   │|
 3|│  ┌─⠋─│|
 4|│MYWAK…│|
 5|│  └───│|
 6|│      │|
 7|│now ⠋ │|
 8|│      │|
 9|│who ● │|
10|│Resea…│|
11|│opena…│|
12|│ope… ·│|
13|│      │|
14|│pulse │|
15|│      │|
16|└──────┘|
```

The nameplate renders `MYWAK…` and the Now row renders `now ⠋`: at 6 usable
columns the marker and one glyph are all that fit, which is why A4 accepts the
glyph as the state word. Note row 1's top border: it is `┌ waker┐` — the title
is clipped because the panel is 8 wide and the title literal is 7 plus two
corner cells (see A10's narrow-title clause).

**How A2 and A10 obtain the sidebar `Rect` at every size.** Not from arithmetic
on the nominal split — from `ui::layout::roster_areas(main)[0]`, called with the
same `main` the renderer used (`Regions::of(frame.area()).main`). The test and
the renderer therefore read one rectangle, which is why a narrowed sidebar at
40×24 cannot silently disagree with what was drawn. This is the same rule the
codebase already applies to mouse hit-testing (§7 rule 5).

## 4. Assertions (A1–A10)

Every cell's pass condition is fixed here. §4.1 is a generated restatement, not
an independent definition — see §4.2.

| ID | Assertion | Pass condition |
|---|---|---|
| **A1** | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) |
| **A2** | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical |
| **A3** | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots |
| **A4** | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and names a state — one of `idle · working · thinking · connecting · offline · paused`, or, when fewer than 10 usable columns remain, the state glyph alone (`●` `▶` `✕` `⏸`, or the spinner). Truncation is acceptable; dropping the marker is not |
| **A5** | Section presence | The §2 markers for the sections §3 requires are present, and the markers for those it forbids are absent. A marker is present only when its section also has all its rows |
| **A6** | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows. Row width is deliberately **not** asserted — the terminal clips, so that clause would be vacuous |
| **A7** | No duplication | The sidebar's **static vocabulary** contains none of `ram `, `cpu `, `events`, `cache `, `opencode v` (log/title-owned). Scoped to strings the sidebar itself composes — its labels and fixed wording. Text the sidebar merely *relays* is out of scope: an employee's name or role, a task goal, a model id, or a connection error may contain any of those tokens, and a buffer containing them is not a violation |
| **A8** | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified |
| **A9** | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period |
| **A10** | Border + brand | All four borders/corners intact. Title: the literal ` waker ` when inner width ≥ 7; when the panel is narrower the title may be **clipped** rather than ellipsised — 40×24 has inner width 6 and renders `┌ waker┐`, the literal minus its trailing space, with both corners intact. The nameplate keeps the literal `MYWAKER` when inner width ≥ 7 (below that it may be ellipsised — 40×24 has inner width 6 and renders `MYWAK…`) |

### 4.1 Matrix — A1–A10 × 6 sizes (60 rows)

Generated from §2's derivation rule, §3's size table, and §4's conditions. The
size cell is copied from the §3 header row; the A5 row restates that size's
expectation. Evidence cites the test symbol that asserts the cell, filled in
run 1 (§8); the reviewer signs each row.

| ID | Size | Assertion | Pass condition | Evidence |
|---|---|---|---|---|
| A1 | 160×50 | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) | `sidebar_at_160x50` (passes: `screen()` drew 160×50 and asserted the row count) |
| A2 | 160×50 | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical | `a2_sidebar_containment` → `assert_containment(160, 50)` |
| A3 | 160×50 | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots | `sidebar_at_160x50` → `assert_vertical_fit` (`tests/sidebar.rs:252`); `fit` kept 7 sections in 40 rows |
| A4 | 160×50 | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and contains at least one of `idle · working · thinking · connecting · offline · paused`, or, below 10 usable columns, the state glyph alone (truncated is acceptable) | `sidebar_at_160x50` (first non-blank row after the 4 avatar rows is `now …`) |
| A5 | 160×50 | Section presence | Markers `MYWAKER`, `now`, `who`, `pulse`, `vitals`, `uptime`, `alert` present — §3: inner h=40 | `sidebar_at_160x50` → `expected_markers` (`:114`), derived from `fit`; must `MYWAKER`, `now`, `who`, `pulse`, `vitals`, `uptime`, `alert`; must not — none |
| A6 | 160×50 | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows | `sidebar_at_160x50` → `assert_truncated_never_wrapped` (`:301`) against `sidebar::source_rows` |
| A7 | 160×50 | No duplication | The sidebar's static vocabulary contains none of `ram `, `cpu `, `events`, `cache `, `opencode v`. Relayed text (names, roles, task goals, model ids, connection errors) is out of scope | `sidebar_at_160x50` → `sidebar::labels()` (`src/ui/sidebar.rs:59`); `labels_avoid_log_owned_tokens` (`:893`) |
| A8 | 160×50 | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified | `a8_sidebar_is_not_a_hit_target` (`:652`) at 160×50; cited app test green |
| A9 | 160×50 | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period | `a9_sidebar_animates` (`:594`) + `a9_idle_avatar_blinks_in_the_panel` (`:627`) at 160×50; `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) |
| A10 | 160×50 | Border + brand | All four borders/corners intact; title ` waker ` when inner width ≥ 7, clipped (not ellipsised) below that — 40×24 renders `┌ waker┐`; nameplate keeps `MYWAKER` when inner width ≥ 7, else ellipsised (`MYWAK…` at 40×24) | `sidebar_at_160x50` → `assert_border_and_brand` (`:365`): four borders + title/nameplate boundary |
| A1 | 120×40 | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) | `sidebar_at_120x40` (passes: `screen()` drew 120×40 and asserted the row count) |
| A2 | 120×40 | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical | `a2_sidebar_containment` → `assert_containment(120, 40)` |
| A3 | 120×40 | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots | `sidebar_at_120x40` → `assert_vertical_fit` (`tests/sidebar.rs:252`); `fit` kept 7 sections in 30 rows |
| A4 | 120×40 | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and contains at least one of `idle · working · thinking · connecting · offline · paused`, or, below 10 usable columns, the state glyph alone (truncated is acceptable) | `sidebar_at_120x40` (first non-blank row after the 4 avatar rows is `now …`) |
| A5 | 120×40 | Section presence | Markers `MYWAKER`, `now`, `who`, `pulse`, `vitals`, `uptime`, `alert` present — §3: inner h=30 | `sidebar_at_120x40` → `expected_markers` (`:114`), derived from `fit`; must `MYWAKER`, `now`, `who`, `pulse`, `vitals`, `uptime`, `alert`; must not — none |
| A6 | 120×40 | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows | `sidebar_at_120x40` → `assert_truncated_never_wrapped` (`:301`) against `sidebar::source_rows` |
| A7 | 120×40 | No duplication | The sidebar's static vocabulary contains none of `ram `, `cpu `, `events`, `cache `, `opencode v`. Relayed text (names, roles, task goals, model ids, connection errors) is out of scope | `sidebar_at_120x40` → `sidebar::labels()` (`src/ui/sidebar.rs:59`); `labels_avoid_log_owned_tokens` (`:893`) |
| A8 | 120×40 | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified | `a8_sidebar_is_not_a_hit_target` (`:652`) at 120×40; cited app test green |
| A9 | 120×40 | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period | `a9_sidebar_animates` (`:594`) + `a9_idle_avatar_blinks_in_the_panel` (`:627`) at 120×40; `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) |
| A10 | 120×40 | Border + brand | All four borders/corners intact; title ` waker ` when inner width ≥ 7, clipped (not ellipsised) below that — 40×24 renders `┌ waker┐`; nameplate keeps `MYWAKER` when inner width ≥ 7, else ellipsised (`MYWAK…` at 40×24) | `sidebar_at_120x40` → `assert_border_and_brand` (`:365`): four borders + title/nameplate boundary |
| A1 | 100×30 | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) | `sidebar_at_100x30` (passes: `screen()` drew 100×30 and asserted the row count) |
| A2 | 100×30 | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical | `a2_sidebar_containment` → `assert_containment(100, 30)` |
| A3 | 100×30 | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots | `sidebar_at_100x30` → `assert_vertical_fit` (`tests/sidebar.rs:252`); `fit` kept 6 sections in 20 rows |
| A4 | 100×30 | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and contains at least one of `idle · working · thinking · connecting · offline · paused`, or, below 10 usable columns, the state glyph alone (truncated is acceptable) | `sidebar_at_100x30` (first non-blank row after the 4 avatar rows is `now …`) |
| A5 | 100×30 | Section presence | Markers `MYWAKER`, `now`, `who`, `pulse`, `vitals`, `uptime` present; no marker outside that set present (`alert` absent) — §3: inner h=20 | `sidebar_at_100x30` → `expected_markers` (`:114`), derived from `fit`; must `MYWAKER`, `now`, `who`, `pulse`, `vitals`, `uptime`; must not `alert` |
| A6 | 100×30 | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows | `sidebar_at_100x30` → `assert_truncated_never_wrapped` (`:301`) against `sidebar::source_rows` |
| A7 | 100×30 | No duplication | The sidebar's static vocabulary contains none of `ram `, `cpu `, `events`, `cache `, `opencode v`. Relayed text (names, roles, task goals, model ids, connection errors) is out of scope | `sidebar_at_100x30` → `sidebar::labels()` (`src/ui/sidebar.rs:59`); `labels_avoid_log_owned_tokens` (`:893`) |
| A8 | 100×30 | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified | `a8_sidebar_is_not_a_hit_target` (`:652`) at 100×30; cited app test green |
| A9 | 100×30 | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period | `a9_sidebar_animates` (`:594`) + `a9_idle_avatar_blinks_in_the_panel` (`:627`) at 100×30; `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) |
| A10 | 100×30 | Border + brand | All four borders/corners intact; title ` waker ` when inner width ≥ 7, clipped (not ellipsised) below that — 40×24 renders `┌ waker┐`; nameplate keeps `MYWAKER` when inner width ≥ 7, else ellipsised (`MYWAK…` at 40×24) | `sidebar_at_100x30` → `assert_border_and_brand` (`:365`): four borders + title/nameplate boundary |
| A1 | 80×24 | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) | `sidebar_at_80x24` (passes: `screen()` drew 80×24 and asserted the row count) |
| A2 | 80×24 | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical | `a2_sidebar_containment` → `assert_containment(80, 24)` |
| A3 | 80×24 | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots | `sidebar_at_80x24` → `assert_vertical_fit` (`tests/sidebar.rs:252`); `fit` kept 4 sections in 14 rows |
| A4 | 80×24 | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and contains at least one of `idle · working · thinking · connecting · offline · paused`, or, below 10 usable columns, the state glyph alone (truncated is acceptable) | `sidebar_at_80x24` (first non-blank row after the 4 avatar rows is `now …`) |
| A5 | 80×24 | Section presence | Markers `MYWAKER`, `now`, `who`, `pulse` present; no marker outside that set present (`vitals`, `uptime`, `alert` absent) — §3: inner h=14 | `sidebar_at_80x24` → `expected_markers` (`:114`), derived from `fit`; must `MYWAKER`, `now`, `who`, `pulse`; must not `vitals`, `uptime`, `alert` |
| A6 | 80×24 | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows | `sidebar_at_80x24` → `assert_truncated_never_wrapped` (`:301`) against `sidebar::source_rows` |
| A7 | 80×24 | No duplication | The sidebar's static vocabulary contains none of `ram `, `cpu `, `events`, `cache `, `opencode v`. Relayed text (names, roles, task goals, model ids, connection errors) is out of scope | `sidebar_at_80x24` → `sidebar::labels()` (`src/ui/sidebar.rs:59`); `labels_avoid_log_owned_tokens` (`:893`) |
| A8 | 80×24 | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified | `a8_sidebar_is_not_a_hit_target` (`:652`) at 80×24; cited app test green |
| A9 | 80×24 | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period | `a9_sidebar_animates` (`:594`) + `a9_idle_avatar_blinks_in_the_panel` (`:627`) at 80×24; `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) |
| A10 | 80×24 | Border + brand | All four borders/corners intact; title ` waker ` when inner width ≥ 7, clipped (not ellipsised) below that — 40×24 renders `┌ waker┐`; nameplate keeps `MYWAKER` when inner width ≥ 7, else ellipsised (`MYWAK…` at 40×24) | `sidebar_at_80x24` → `assert_border_and_brand` (`:365`): four borders + title/nameplate boundary |
| A1 | 60×16 | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) | `sidebar_at_60x16` (passes: `screen()` drew 60×16 and asserted the row count) |
| A2 | 60×16 | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical | `a2_sidebar_containment` → `assert_containment(60, 16)` |
| A3 | 60×16 | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots | `sidebar_at_60x16` → `assert_vertical_fit` (`tests/sidebar.rs:252`); `fit` kept 2 sections in 6 rows |
| A4 | 60×16 | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and contains at least one of `idle · working · thinking · connecting · offline · paused`, or, below 10 usable columns, the state glyph alone (truncated is acceptable) | `sidebar_at_60x16` (first non-blank row after the 4 avatar rows is `now …`) |
| A5 | 60×16 | Section presence | Markers `MYWAKER`, `now` present; no marker outside that set present (`who`, `pulse`, `vitals`, `uptime`, `alert` absent) — §3: inner h=6 | `sidebar_at_60x16` → `expected_markers` (`:114`), derived from `fit`; must `MYWAKER`, `now`; must not `who`, `pulse`, `vitals`, `uptime`, `alert` |
| A6 | 60×16 | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows | `sidebar_at_60x16` → `assert_truncated_never_wrapped` (`:301`) against `sidebar::source_rows` |
| A7 | 60×16 | No duplication | The sidebar's static vocabulary contains none of `ram `, `cpu `, `events`, `cache `, `opencode v`. Relayed text (names, roles, task goals, model ids, connection errors) is out of scope | `sidebar_at_60x16` → `sidebar::labels()` (`src/ui/sidebar.rs:59`); `labels_avoid_log_owned_tokens` (`:893`) |
| A8 | 60×16 | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified | `a8_sidebar_is_not_a_hit_target` (`:652`) at 60×16; cited app test green |
| A9 | 60×16 | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period | `a9_sidebar_animates` (`:594`) + `a9_idle_avatar_blinks_in_the_panel` (`:627`) at 60×16; `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) |
| A10 | 60×16 | Border + brand | All four borders/corners intact; title ` waker ` when inner width ≥ 7, clipped (not ellipsised) below that — 40×24 renders `┌ waker┐`; nameplate keeps `MYWAKER` when inner width ≥ 7, else ellipsised (`MYWAK…` at 40×24) | `sidebar_at_60x16` → `assert_border_and_brand` (`:365`): four borders + title/nameplate boundary |
| A1 | 40×24 | No panic | `Terminal::draw` at the size returns `Ok` (the `Ok` is `draw`'s — `ui::render` returns `()`) | `sidebar_at_40x24` (passes: `screen()` drew 40×24 and asserted the row count) |
| A2 | 40×24 | Containment | Render the same app twice, differing only in one sidebar-owned input (one `Pulse` sample appended); every cell **outside** the sidebar `Rect` is identical | `a2_sidebar_containment` → `assert_containment(40, 24)` |
| A3 | 40×24 | Vertical fit | Content rows drawn ≤ inner height, and sections are all-or-nothing vertically: no section contributes fewer rows than §2 allots | `sidebar_at_40x24` → `assert_vertical_fit` (`tests/sidebar.rs:252`); `fit` kept 4 sections in 14 rows |
| A4 | 40×24 | Now line present | The first **non-blank** row after the avatar block (skipping the blank gap row between P0 and P1) is the Now line: it starts with the `now` marker and contains at least one of `idle · working · thinking · connecting · offline · paused`, or, below 10 usable columns, the state glyph alone (truncated is acceptable) | `sidebar_at_40x24` (first non-blank row after the 4 avatar rows is `now …`) |
| A5 | 40×24 | Section presence | Markers `MYWAKER`, `now`, `who`, `pulse` present; no marker outside that set present (`vitals`, `uptime`, `alert` absent) — §3: inner h=14 | `sidebar_at_40x24` → `expected_markers` (`:114`), derived from `fit`; must `MYWAKER`, `now`, `who`, `pulse`; must not `vitals`, `uptime`, `alert` |
| A6 | 40×24 | Truncate, never wrap | Each row is either its section's complete source string or a prefix ending in `…`; no source string is split across two consecutive rows | `sidebar_at_40x24` → `assert_truncated_never_wrapped` (`:301`) against `sidebar::source_rows` |
| A7 | 40×24 | No duplication | The sidebar's static vocabulary contains none of `ram `, `cpu `, `events`, `cache `, `opencode v`. Relayed text (names, roles, task goals, model ids, connection errors) is out of scope | `sidebar_at_40x24` → `sidebar::labels()` (`src/ui/sidebar.rs:59`); `labels_avoid_log_owned_tokens` (`:893`) |
| A8 | 40×24 | Not a hit target | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:2386`) stays green with its assertion unmodified | `a8_sidebar_is_not_a_hit_target` (`:652`) at 40×24; cited app test green |
| A9 | 40×24 | Animation | At `Thinking`/`Working`, frame *n* and *n+1* differ in the avatar rows; `Idle` antenna blinks on the 20-tick period | `a9_sidebar_animates` (`:594`) + `a9_idle_avatar_blinks_in_the_panel` (`:627`) at 40×24; `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) |
| A10 | 40×24 | Border + brand | All four borders/corners intact; title ` waker ` when inner width ≥ 7, clipped (not ellipsised) below that — 40×24 renders `┌ waker┐`; nameplate keeps `MYWAKER` when inner width ≥ 7, else ellipsised (`MYWAK…` at 40×24) | `sidebar_at_40x24` → `assert_border_and_brand` (`:365`): four borders + title/nameplate boundary |

### 4.2 Why the matrix exists

The tests **derive** their expectations from §2 and §3 rather than hardcoding 60
literals, and §4 is the single source of truth for each condition's wording. The
matrix is a review surface: it lets a human read off one row per assertion per
size, and it is regenerated whenever §2/§3/§4 change. If the matrix and §4 ever
disagree, **§4 wins** and the matrix is regenerated — it is a restatement, not a
second definition.

## 5. Non-render criteria (SB1–SB6)

Separate table — these are not per size.

Each criterion carries its own evidence cell, so a gap is recorded here rather
than only in review. `not evidenced` means no test in this tree asserts it.

| ID | Criterion | Evidence |
|---|---|---|
| **SB1** | `fit(sections, height)` implements §2's derivation rule: the longest prefix of P0…P6 whose cumulative total ≤ `height`, never exceeding `height` | `src/ui/sidebar.rs`: `fit_drops_whole_sections_by_priority`, `fit_never_exceeds_the_height`, `fit_reproduces_the_frozen_budget` |
| **SB2** | `Pulse` keeps the last 48 tick samples, pushes 0 on an idle tick, resets cleanly | `src/app/tests.rs`: `pulse_keeps_the_last_48_ticks` — asserts `Pulse::CAPACITY == 48` directly (the literal, not the constant, so the criterion cannot drift), the zero-push on an idle tick, oldest-first eviction past 48, the per-tick delta, and that `reset` clears both the window and the baseline |
| **SB3** | `fmt_uptime` → `59s` / `5m` / `1h 12m`; `sparkline` is width-safe at 0/1/48 samples; `bar` clamps 0.0–1.0 | `src/ui/sidebar.rs`: `fmt_uptime_formats`, `sparkline_is_width_safe`, `bar_clamps`; the uptime clause is also covered end-to-end by `src/app/tests.rs::uptime_advances`. **Partial (sparkline only):** the widths tested are 0, 1, 3, 10 and 100 samples — the **48-sample boundary itself is never passed**. The 100-sample case evidences overflow past the window in general, not the stated boundary |
| **SB4** | Aggregate accessors (`busy_count`, `thinking_count`, `live_sessions`, `queued_prompts`, `task_stats`) agree with a synthetic team + task set | `src/app/tests.rs`: `aggregate_accessors_count_states` — a synthetic roster walks every status (busy vs not, `thinking_count` as a subset of `busy_count`, terminal states not busy), per-chat sessions and queued prompts, and a task set covering all six statuses with `active()`/`finished()` |
| **SB5** | No dispatch/session/persistence behaviour changed. One pre-existing test was deliberately **retired, not silently broken**: `wheel_steps_the_roster_selection` asserted that a wheel notch moves the roster *selection*; the wheel now scrolls the grid viewport and leaves the selection alone, so it is replaced by `wheel_scrolls_the_employee_grid`. Every other pre-existing app test keeps its assertion unmodified | `src/app/tests.rs`: `wheel_scrolls_the_employee_grid` (the replacement); the rest of the app suite green |
| **SB6** | `cargo test` · `cargo clippy --all-features -- -D warnings` · `cargo fmt --check` all exit 0 | run at rev 8: **274 tests pass** (251 lib + 2 ignored + 5 `opencode_fake` + 6 `render` + 11 `sidebar` + 1 `watch_hook`; 1 `live_employees` ignored), 0 failed; `cargo clippy --all-features -- -D warnings` exit 0 (also clean with `--all-targets`); `cargo fmt --check` exit 0 |

## 6. Frozen test names

So the evidence column cites a symbol that must exist rather than describing a
run:

- `tests/sidebar.rs` — `sidebar_at_160x50`, `sidebar_at_120x40`, `sidebar_at_100x30`,
  `sidebar_at_80x24`, `sidebar_at_60x16`, `sidebar_at_40x24` (each asserts A1–A10
  for its size, deriving expectations from §2/§3), plus the non-size seams
  `a2_sidebar_containment`, `a8_sidebar_is_not_a_hit_target`,
  `a9_sidebar_animates`, and `a9_idle_avatar_blinks_in_the_panel` (A9's blink
  clause at panel level, on the `idle_app` fixture), and
  `a_wheel_over_the_sidebar_scrolls_the_grid` (the documented wheel-over-sidebar
  behaviour: the notch falls through to the grid and never moves the selection).
- `src/ui/sidebar.rs` — `fit_drops_whole_sections_by_priority`,
  `fit_never_exceeds_the_height`, `fit_reproduces_the_frozen_budget`,
  `fmt_uptime_formats`, `sparkline_is_width_safe`, `bar_clamps`,
  `markers_are_unique_to_their_section`,
  `narrow_nameplate_is_ellipsised_not_dropped`,
  `inspector_shows_the_selected_employees_model`,
  `a_composite_row_truncates_as_one_string`, `a_row_that_fits_is_untouched`
  (A6's truncation invariant at unit level), and `labels_avoid_log_owned_tokens`
  (A7's vocabulary).
- `src/app/tests.rs` — `clicking_outside_a_target_keeps_the_selection` (A8),
  `clicking_a_grid_card_selects_that_employee`, `page_keys_move_by_a_viewport`,
  `narrow_panel_hit_tests_one_column_cards`, `wheel_scrolls_the_employee_grid`,
  and the sidebar-vitals group: `pulse_keeps_the_last_48_ticks` (SB2),
  `aggregate_accessors_count_states` (SB4), `uptime_advances` (SB3's uptime
  clause).
- `src/ui/pipeline.rs` — `grid_drops_the_header_before_a_card_row` (the D1 guard),
  `grid_cards_are_disjoint_complete_and_inside_the_area`.
- `src/ui/robot.rs` — the mini avatar's own contract:
  `mini_avatar_is_four_rows_and_keeps_the_nameplate` and
  `mini_avatar_animates_with_frame` (the latter pins `Idle`'s 20-tick blink and
  the `Thinking`/`Working` spinner on `render_mini` itself). Also
  `idle_antenna_blinks_on_a_20_tick_period`,
  `thinking_and_working_antenna_spins_with_frame`, and
  `every_state_renders_nine_lines`, which cover the 9-row `render`.
- Cited as-is: `renders_nameplate_and_roster`,
  `renders_narrow_terminal_without_panicking`, `roster_panels_tile_the_main_area`.

> **Gap closed in rev 7 — recorded, not quietly deleted.** Rev 1–3 listed five
> test names that did not exist yet:
> `mini_avatar_is_four_rows_and_keeps_the_nameplate` and
> `mini_avatar_animates_with_frame` (`src/ui/robot.rs`),
> `pulse_keeps_the_last_48_ticks`, `aggregate_accessors_count_states`, and
> `uptime_advances` (`src/app/tests.rs`). Rev 4 recorded the gap rather than
> dropping the names; all five now exist and pass:
>
> | Test | Covers | Evidence |
> |---|---|---|
> | `pulse_keeps_the_last_48_ticks` (`src/app/tests.rs:2886`) | SB2 | asserts the literal 48, the idle-tick zero, oldest-first eviction, the per-tick delta, and `reset` clearing both window and baseline |
> | `aggregate_accessors_count_states` (`src/app/tests.rs:2931`) | SB4 | walks every employee status, per-chat sessions and queued prompts, and a task set covering all six statuses |
> | `uptime_advances` (`src/app/tests.rs:3019`) | SB3's uptime clause | monotonic uptime plus the `Ns` label a live session prints |
> | `mini_avatar_is_four_rows_and_keeps_the_nameplate` (`src/ui/robot.rs:222`) | §2.1, A10 | the mini avatar is exactly four rows in every state, with the nameplate on row 3 |
> | `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) | A9 | `Idle`'s 20-tick blink on `render_mini` itself, plus the `Thinking`/`Working` spinner |
>
> The one clause still not fully evidenced is **SB3's sparkline width**: the
> tested widths are 0, 1, 3, 10 and 100 samples, so the stated **48-sample
> boundary is never passed** (§5 records this as *partial*). The 100-sample case
> covers overflow past the window in general, not the boundary.
>
> **A9 — what closed, and what is still narrow.** A9's blink clause was previously
> asserted only against the 9-row `render` (through the `antenna` helper,
> `src/ui/robot.rs:146`), while the sidebar draws `render_mini`
> (`src/ui/sidebar.rs:431`). Two additions close that:
>
> - `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) pins the blink on
>   `render_mini` itself, at unit level.
> - `a9_idle_avatar_blinks_in_the_panel` (`tests/sidebar.rs:627`) asserts it at
>   panel level, at all six pinned sizes, on the `idle_app` fixture
>   (`tests/sidebar.rs:47`) — which exists precisely because every other render
>   test runs disconnected and so never reaches `render_mini`'s `Idle` branch.
>
> Both were verified to fail when `render_mini` is frozen, so A9's period claim is
> now genuinely covered.
>
> **The narrower finding that remains true:** `a9_sidebar_animates`
> (`tests/sidebar.rs:594`) still passes with the avatar frozen. It compares
> whole-sidebar buffers, and rows 5 and 12 differ between frames on their own — row
> 5 is the Now line's spinner (`now ⠋` → `now ⠙`) and row 12 is the pulse
> sparkline, which shifts as `Pulse` records a sample. So that test proves
> **frame-wiring only**, not avatar animation. It is not wrong; it is simply not
> the assertion its name suggests, and A9's actual coverage now comes from the two
> tests above.

## 7. Freeze rules

1. A criterion is **never weakened** to make a build pass; changing one is a
   deliberate edit recorded in the revision log.
2. The evidence log (§8) and §5's Evidence column are filled by the reviewer with
   test name + file:line + observed output. A criterion with no covering test is
   recorded as *not evidenced*; it is never marked pass on inspection alone.
3. §2's line budget and derivation rule are the contract. If the implementation
   needs a different budget, the budget **and** §3 are revised together, never
   one silently.
4. §3's "must not appear" column is a real assertion: a section shown at 60×16 is
   a **FAIL**, not a nicety.
5. Panel geometry is pinned. Any change to `ui::layout::roster_areas` (the
   `24 | fill | 32` split) must be announced to the tester before the matrix is
   re-run, because A2 and A10 read those rectangles. Tests obtain the sidebar
   `Rect` from `roster_areas(main)[0]`, never by arithmetic (§3).
6. A marker must be unique to its section. Adding a marker that also appears in
   another section's text invalidates A5 and is a spec change, not an
   implementation detail.
7. §4.1 is generated. Editing a matrix cell by hand is a bug; change §2/§3/§4 and
   regenerate.

## 8. Evidence log

The tester appends one block per run; the reviewer signs each row in §4.1 and
fills §5's Evidence column.

| Run | Date | Sizes | Result | Notes |
|---|---|---|---|---|
| 1 | 2026-10-18 | 160×50 · 120×40 · 100×30 · 80×24 · 60×16 · 40×24 | **PASS** | First full run. 60/60 matrix cells filled in §4.1; A2/A8/A9 are asserted at all six sizes, not only at 120×40. SB1–SB6 all evidenced except SB3's sparkline **boundary** clause (still *partial*, unchanged). |

**Run 1 — what was executed.** `cargo test` (274 pass, 0 fail), `cargo clippy
--all-features -- -D warnings` (exit 0; `--all-targets` also exit 0), `cargo fmt
--check` (exit 0). Per size, `sidebar_at_*` asserts A1, A3, A4, A5, A6, A7 and A10
against the rendered buffer; A2, A8 and A9 are asserted per size by their own
loops.

**Observed per size** (the values the matrix's A3/A5 cells cite, captured from
`TestBackend`):

| Size | sidebar `Rect` | inner | sections kept | markers present |
|---|---|---|---|---|
| 160×50 | `(0,1,24,42)` | 22×40 | 7 | `MYWAKER now who pulse vitals uptime alert` |
| 120×40 | `(0,1,24,32)` | 22×30 | 7 | `MYWAKER now who pulse vitals uptime alert` |
| 100×30 | `(0,1,24,22)` | 22×20 | 6 | `MYWAKER now who pulse vitals uptime` (`alert` absent) |
| 80×24 | `(0,1,24,16)` | 22×14 | 4 | `MYWAKER now who pulse` (`vitals uptime alert` absent) |
| 60×16 | `(0,1,24,8)` | 22×6 | 2 | `MYWAKER now` (all others absent) |
| 40×24 | `(0,1,8,16)` | 6×14 | 4 | `MYWAKER now who pulse` (`vitals uptime alert` absent) |

Every row matches §3 exactly, including the two width-driven cases: at 40×24 the
panel narrows to 8 columns (6 usable) and its top border is `┌ waker┐`.

**Three test defects fixed in this run, each verified by mutation.**

1. **A6 was vacuous.** The old block called `trim_end()` and then asked whether
   the result ended in a space — false by construction, so it asserted nothing.
   Meanwhile §3's own evidence showed `ope… ·` at 40×24: two independently
   truncated halves joined by the terminal, which is neither a complete string nor
   an ellipsised prefix. A6 now compares each drawn row against
   `sidebar::source_rows` and requires either an exact match or a prefix ending in
   `…` that fills the panel. The implementation was fixed to match — rows are
   composed and then truncated **once**, as a whole (`truncate_spans`,
   `src/ui/sidebar.rs:311`), so `ope… ·` is now `opena…`. Re-introducing the
   per-span truncation fails `sidebar_at_40x24` with "neither complete nor an
   ellipsised prefix".
2. **A7 was unscoped.** It scanned all sidebar text, contradicting §4:223: an
   employee named `events-bot`, a role or goal containing `events`, or a model id
   containing `cache ` would have failed a correct frame. It now asserts on
   `sidebar::labels()` — the literals the module composes — with
   `labels_avoid_log_owned_tokens` pinning the same list at unit level. Putting
   `events` in a label fails all six sizes.
3. **A10 checked only the top border.** It now asserts all four rules and all four
   corners, and the `inner width ≥ 7` boundary for both the title (` waker ` at
   ≥ 7, clipped `┌ waker┐` below) and the nameplate (`MYWAKER` at ≥ 7, `MYWAK…`
   below). Dropping the right border fails six sizes; renaming the title fails
   six sizes.

**A9's period claim is now genuinely covered.** Two new tests pin `render_mini` —
the avatar the sidebar actually draws — at unit level
(`mini_avatar_animates_with_frame`, `src/ui/robot.rs:254`) and at panel level
(`a9_idle_avatar_blinks_in_the_panel`, `tests/sidebar.rs:627`), the latter on the
new `idle_app` fixture because every other render test runs disconnected and so
never reached `render_mini`'s `Idle` branch. Freezing `render_mini` now fails
both. `a9_sidebar_animates` still passes with the avatar frozen — it compares
whole-sidebar buffers where the Now spinner and the pulse sparkline differ on
their own — so it is recorded as proving **frame-wiring only**; that narrower
finding is unchanged from rev 7.

**Still partial (not a new gap).** SB3's sparkline **48-sample boundary** is never
passed: `sparkline_is_width_safe` exercises 0, 1, 3, 10 and 100 samples. The
100-sample case covers overflow in general, not the boundary. §5 records it as
*partial*.

**Also recorded.** `a_wheel_over_the_sidebar_scrolls_the_grid`
(`tests/sidebar.rs:676`) pins the wheel-over-sidebar behaviour now that the writer
has documented it: the panel is not a hit target (A8), so a notch over it falls
through to the roster and scrolls the grid's viewport without moving the selection
— matching `README.md` and `docs/architecture.md` §5.

## 9. Revision log

| Rev | Date | Change |
|---|---|---|
| 1 | 2026-10-18 | Created from the sidebar plan §1–§7: §2 budget, §3 size table, A1–A10, the 60-row matrix, SB1–SB6, frozen test names, freeze rules. Status: frozen, awaiting implementation. |
| 2 | 2026-10-18 | Made the rows decidable from a buffer: A4 now reads "first non-blank row after the avatar block" (P0's gap row was otherwise the first non-avatar row, so the assertion could never see the Now line); A2's control is now "same app state, one sidebar-owned input mutated" (an empty-team frame changes the pipeline and tasks panels by construction); §2 gained a Marker column and A5 asserts on markers; A1 cites `Terminal::draw`; A3/A6 replaced "mid-glyph"/row-width clauses with symbol predicates; §2 gained the derivation rule and §2.1 pins the 4-row mini avatar (the old 9-row `render` could not satisfy A10's `MYWAKER` at 60×16); the 40×24 P1/P2 contradiction is resolved as width-conditional and §3 states that A2/A10 read `roster_areas(main)[0]`. |
| 3 | 2026-10-18 | Implemented **all seven sections, P0–P6** — the mini avatar, the aggregate Now line, the selected-employee inspector, the pulse sparkline, queue vitals, runtime (uptime · Jev), and alerts — and reconciled the spec with what the tests proved necessary: A4 now accepts the state *glyph* below 10 usable columns (at 40×24 the 6 available columns hold `now ` plus one glyph, so a word-based assertion was unsatisfiable while keeping A5's marker); §3 records that the inspector and pulse render at 40×24 with ellipsised content rather than being dropped; §5 SB1's wording matches `fit` as built. Also added the A2 seam (`sidebar::render_only` + `App::record_pulse`, since `advance` also moves the title spinner) and the A9 seam (`App::advance`). *(Corrected in rev 4: this entry previously read "Implemented P1–P4", which named four sections under an ID scheme that used P0 twice and omitted the Now line and alerts — both of which shipped.)* |
| 4 | 2026-10-18 | **D1 closed; the section IDs, the 40×24 row, A4, A7, A10, §6, and the log itself corrected.** (1) The stale D1 note is replaced by a record that D1 is **fixed** — the pipeline panel now drops whole bands instead of squashing the first (`ui::pipeline::shape`, `src/ui/pipeline.rs:131-193`; guard `grid_drops_the_header_before_a_card_row` at `:589`), with the 80×20 frame as evidence and an explicit statement that **no §2/§3 row was relaxed** to make it pass. (2) §2's IDs are renumbered to a single P0…P6 — the avatar and the Now line were both `P0` — cumulative totals restated as 4 · 6 · 11 · 13 · 16 · 19 · 22, and the derivation rule now reads "longest prefix of P0…P6" (as does SB1). (3) `connecting` is added to A4's state words: it is a real state the sidebar draws (`now_row`, `src/ui/sidebar.rs:465-510` at rev 6) and the old list omitted it, so a connecting frame failed A4 despite being correct. (4) The 40×24 row is resolved in favour of what is built: **P2 (`who`) and P3 (`pulse`) are mandatory**, the "as width allows" conditional is retired, and §3 now states that presence is never relaxed — only a row's *rendering* may be narrow. (5) A10's title clause is relaxed below inner width 7, matching the clipped `┌ waker┐` at 40×24 (the literal minus its trailing space, corners intact) instead of demanding ` waker ` at a size where it cannot fit. (6) A7 is scoped to the sidebar's **static vocabulary**, so an employee name, role, task goal, model id, or connection error containing `ram `/`events`/`cache ` is not a violation. (7) This log is reordered 1, 2, 3, 4 and rev 3's "Implemented P1–P4" is corrected. The 60-row matrix was regenerated from §3/§4. (8) `docs/todos.md`'s M4 layout sentence is corrected to name the sidebar rather than "robot (24)". **(9) §6 corrected:** five frozen test names that never existed are replaced by the symbols that do, with the missing SB2/SB4 coverage recorded as a gap rather than silently dropped. |
| 5 | 2026-10-18 | **Mechanical corrections; SB evidence made explicit.** (1) D1's citation now spans `src/ui/pipeline.rs:131-193` — the old `131-178` stopped mid-comment, before the `Shape` return. (The range was `131-185` when this entry was first written; `shape` grew a header-cost branch in a concurrent edit, so the citation was re-resolved against the tree at hand-off.) (2) §2's illustrative example was re-pointed from `P3` to `P4`. The reason is **not** that `P3` became decidable — no section's ID is ever rendered, so "P*n* present" was never decidable at all. The real reason is that the row whose marker is `vitals` moved from P3 to P4 in the P0–P6 renumbering, so an example naming P3 as the vitals would now point at `pulse`. The sentence itself was also rewritten to say why IDs are undecidable, rather than implying one particular ID had become readable. (3) §5's SB3 parenthetical contradicted its own trailing clause in one sentence; SB2 and SB4 are now the only criteria named as uncovered. (4) §5 gains an **Evidence** column so SB2 and SB4 are marked *not evidenced* in the document, not only in review. (5) The §6 gap note now records A9's consequence: its 20-tick blink clause is asserted against the 9-row `render` (`idle_antenna_blinks_on_a_20_tick_period`, `src/ui/robot.rs:180`, via the `antenna` helper at `:146`), **not** the `render_mini` the sidebar actually draws — the two agree by inspection but nothing pins it, and `a9_sidebar_animates` only proves a frame is wired through. (6) §8 no longer says "awaiting implementation"; it records the suite as green with **no evidence run yet**. (7) SB5 amended: it claimed every pre-existing app test was green "without edits", which is false — `wheel_steps_the_roster_selection` was deliberately retired and replaced by `wheel_scrolls_the_employee_grid`, because the wheel's contract changed from *moving the selection* to *scrolling the viewport without moving it*. The criterion now names that change. (8) A8's citation is corrected from `src/app/tests.rs:2380` to `:2386` in §4 and all six matrix rows, and "unmodified" becomes "assertion unmodified" throughout. |
| 6 | 2026-10-18 | **Three corrections, no criterion changed.** (1) Rev 5's reason for the P3→P4 example was wrong: it claimed `P3 present` had become decidable, but no section ID is ever rendered, so no "P*n* present" was *ever* decidable. The true reason is that the row marked `vitals` moved from P3 to P4 in the P0–P6 renumbering, so the old example would now point at `pulse`. §2's sentence was rewritten to say why IDs are undecidable, and the log item corrected. (2) SB3's evidence cell claimed `sparkline_is_width_safe` covers the 48-sample case; it does not — that test passes 0, 1, 3, 10 and 100 samples. The cell is now marked **partial**: overflow is covered by the 100-sample assertion, the stated 48-sample boundary is not exercised. (3) The 80×20 block is no longer described as a verbatim buffer: the live `ram` figure is nondeterministic (3.8 MiB in the capture, 4.0 MiB on a re-run), so it now reads "verbatim apart from the live RAM figure", citing `RELEASING.md` §2.3's cell-dump caveat. **(4) The §6 A9 note gained the two facts that make that gap worse:** the `tests/sidebar.rs` fixture is never connected, so `robot_state` is always `Thinking` and `render_mini`'s `Idle` branch — the branch A9's blink clause is about — is never executed by any sidebar test; and `a9_sidebar_animates` compares whole-sidebar buffers, where row 5 (the Now spinner) and row 12 (the pulse sparkline) differ every tick on their own, so A9 would pass with the avatar frozen entirely. A9 is therefore evidenced for frame-difference only, not for the blink period, and not for `Idle` at all. |
| 7 | 2026-10-18 | **§5, §6, and the A9 note re-derived against the current tree — the earlier verdicts were written before it moved. No criterion changed; only evidence.** (1) **SB2 is now evidenced** by `pulse_keeps_the_last_48_ticks` (`src/app/tests.rs:2886`) — it asserts the literal 48 (not just the constant), the idle-tick zero, oldest-first eviction, the per-tick delta, and `reset` clearing window *and* baseline — and **SB4** by `aggregate_accessors_count_states` (`:2931`), which walks every status, per-chat sessions, queued prompts, and a task set covering all six statuses. Both *not evidenced* verdicts are removed. `uptime_advances` (`:3019`) now also covers SB3's uptime clause. **SB3's sparkline clause stays partial:** that test still passes 0, 1, 3, 10 and 100 samples, never 48. (2) §6's gap note is rewritten from a gap into a closure table — all five names that previously did not exist now exist and pass. (3) The A9 note is superseded: `render_mini`'s `Idle` branch is reached by `idle_app` (`tests/sidebar.rs:47`), pinned at unit level by `mini_avatar_animates_with_frame` (`src/ui/robot.rs:254`) and at panel level by `a9_idle_avatar_blinks_in_the_panel` (`tests/sidebar.rs:627`); freezing `render_mini` fails both (verified by probe). The narrower finding is kept: `a9_sidebar_animates` (`tests/sidebar.rs:594`) still passes with the avatar frozen, because rows 5 (Now spinner) and 12 (pulse sparkline) differ on their own — it proves frame-wiring only. (4) §6's `robot.rs` list gains the two mini-avatar tests and drops the "both against the 9-row `render`" clause. (5) Citations corrected: `src/ui/sidebar.rs:406` → `:431` (406 is `source_rows`) and `src/ui/robot.rs:169` → `:180`. (6) A **Tree pin** block records the base commit and the content hashes of the cited files, so "pinned to the tree" is checkable rather than asserted — the work is uncommitted, so a commit id alone would not reproduce the lines. |
| 8 | 2026-10-18 | **Evidence run 1: the matrix is filled; three test defects fixed; A9 closed.** (1) **A6 was vacuous** — `trim_end()` followed by `ends_with(' ')` is false by construction — while §3's own 40×24 evidence showed `ope… ·`, which A6 forbids. A6 now compares each drawn row against `sidebar::source_rows` and requires an exact match or an ellipsised prefix that fills the panel. The implementation was corrected to match: rows are composed then truncated **once** as a whole (`truncate_spans`, `src/ui/sidebar.rs:311`), so the composite provider/session row renders `opena…`. Re-introducing per-span truncation fails `sidebar_at_40x24`. (2) **A7 was unscoped**, contradicting §4:223 — a name, role, goal, or model id containing `events` would have failed a correct frame. It now asserts on `sidebar::labels()`, with `labels_avoid_log_owned_tokens` pinning the list at unit level. (3) **A10 checked only the top border**; it now asserts all four rules and corners plus the `inner width ≥ 7` boundary for the title and the nameplate. (4) A2/A8/A9 are asserted at all six sizes rather than only 120×40, so each matrix cell cites a per-size run. (5) `a_wheel_over_the_sidebar_scrolls_the_grid` pins the now-documented wheel behaviour. (6) §8 records run 1 with the observed `Rect`, inner size, kept-section count and marker set per size; all six match §3. **No criterion was weakened**, and SB3's sparkline boundary remains *partial* (unchanged). |
