# Verification matrix — U1–U12 × 4 sizes (skeleton, pre-T1.1)

Generated restatement of [`acceptance-ux.md`](acceptance-ux.md) §4 against its §3
breakpoints. Companion to that file's §4.1, which reserves this artifact for the
tester (T4.2). Conventions — column order, evidence form, and the rule that a
cell cites a symbol rather than a description — are copied from
[`acceptance-sidebar.md`](acceptance-sidebar.md) §4.1/§4.2 so the two matrices can
be read the same way.

> **Status: skeleton — 48 cells, 0 filled, 48 `not evidenced`, 0 `blocked`.**
>
> This file exists so the criteria are **checkable before T1.1 starts**. It
> pre-fills nothing that requires a passing test: every Evidence cell reads
> `not evidenced` and names the test §6 already froze. A cell becomes *filled*
> only in run 1, with a symbol that exists and output that was observed
> (`acceptance-ux.md` §7 rule 2).
>
> **§4 wins.** If a row here disagrees with `acceptance-ux.md` §4, this file is
> wrong and is regenerated. It is a restatement, not a second definition.
>
> **Verification of this revision.** All 48 rows were compared against
> `acceptance-ux.md` §4.1 by script at the pinned bytes and are **byte-identical
> (48/48)**; no row reads `blocked`. The rule references were re-checked against
> the spec's §7 and are correct as written — §4.1 generation is **rule 5**, and
> changing a criterion is **rule 1** (rule 3 is the sidebar-budget boundary, which
> this file does not invoke).

**Provenance.** Regenerated from `docs/acceptance-ux.md` at `shasum 26347292…`
(96,606 bytes), the **rev 17** revision. The eight tree-pin hashes it records were
verified against the working tree at the previous pin and are unaffected by rev
17, which changes no source file except one comment: `src/app.rs` `a95d056f`,
`src/ui/mod.rs` `4402b0e5`, `src/ui/layout.rs` `c1fdd460`, `src/ui/hit.rs`
`6f9bfb21`, `src/ui/sidebar.rs` `36dfd841`, `src/ui/pipeline.rs` `a39e722e`,
`src/ui/tasks.rs` `cb5a9772`, `src/team.rs` `fc4d28ea`. The retry comment corrected
in rev 17 sits in `src/harness.rs`, which carries no pin hash.

> **Pin note.** The pinned bytes (`26347292`, 96,606) contain **rev 17**. Earlier
> passes of this file were labelled "rev 3" over bytes that already contained rev
> 4, then "rev 12" over bytes containing rev 13, and were asked to target rev 8,
> rev 9 (`776df44b`) and rev 11 (`ae103994`) at various points — a revision
> *number* is a label that drifts when the spec is edited concurrently, whereas the
> `shasum` and byte count are reproducible. Revs 10–15 changed no criterion, no
> cell and no pass condition, and the 48 cells were byte-identical to what rev 9
> would have produced, verified by script. **Rev 17 does change cells**: U12 gained
> §3's scope clause in §4, and all four U12 rows were restated to carry it — the
> other 44 rows are unchanged, and all 48 still match §4.1 byte-for-byte, verified
> by script at the bytes above.

The 48 rows in §3 below are **extracted from §4.1 by script, not retyped**
(`acceptance-ux.md` §7 rule 5). If the two ever differ, this file is stale and is
regenerated.

**What rev 5 changed here.** Both previously-`blocked` cells were decided: U5 and
U9 at 60×16 now name the roster's **notice row** (the log band's first content
row) as the host, so the grid's 4-column floor no longer makes them undecidable.
The cells moved `blocked` → `not evidenced`, and **no requirement was weakened** —
but the criteria did grow: each gained a host clause, and U9 gained the
"never both and never neither" obligation, which forbids the count going missing
as well as being duplicated. Recorded as a strengthening, not a neutral edit.

**What rev 9 changed here.** The notice row is now **reserved** rather than
borrowed: while the selected employee is thinking, the prompt band does not take
its third row and the thinking indicator yields, so `main` is byte-identical in
every combination and the notice row survives. Both U5 and U9 @ 60×16 gained the
clause *"the row is **reserved** while thinking (§4.2)"*, which is why those two
cells read differently from this file's rev 8 copy. §6 gained a name for it,
`u5_notice_survives_the_thinking_state`, taking the frozen count from 31 to 32
(§4.4).

**Open items for the author (not defects in the cells).** The two coverage
questions this matrix surfaced are now **settled** (spec rev 15): §4.4.1 records
that U4's `searchable /` and "any dismiss key closes it without side effects"
clauses now each have a frozen name, and that `help_overlay_is_a_window` is §2's
bordered-title marker assertion rather than a scroll-only seam. §4.4 therefore
records **two** uncited names, both deliberate unit-level seams labelled as such in
`acceptance-ux.md` §6 — not gaps. See §4.4.1 for the decision and its rationale.

---

## 1. Axes

| Axis | Values | Source |
|---|---|---|
| Criterion | U1 … U12 (12) | `acceptance-ux.md` §4 |
| Size | 120×40 · 100×30 · 80×24 · 60×16 (4) | §3 |
| **Cells** | **12 × 4 = 48** | §4.1 |

**Not axes, and why** (§3):

- **160×50** — ceiling reference only. Drawn by the smoke tests so a criterion
  cannot be satisfied by a layout that only works when cramped. No cell is claimed.
- **40×24** — team panel is zero-width, so U2–U12 are undecidable there. U1 (no
  panic) stays with [`acceptance-sidebar.md`](acceptance-sidebar.md) A1.
- **employee state (idle / thinking)** — **not an axis, deliberately.** Rev 9 makes
  the notice row reserved, so U5 and U9 are asserted *identically* in every state;
  no cell varies by it. The state dimension is asserted by a seam instead
  (`u5_notice_survives_the_thinking_state`, §4.4).

**Geometry at the four axes** (widths read from the renderer's own seams —
`ui::sidebar_area`, `ui::team_area`, `ui::grid_cells` — never recomputed by hand;
re-derived from `layout::roster_areas` and `pipeline::shape`). Every column agrees
with the tree except the 60×16 `card` **height**, which is the C6 amendment below:
the widths are measured, the floor's 1-row height is contracted.

| Size | main | grid | card | cols | sidebar | tasks | notice row |
|---|---|---|---|---|---|---|---|
| 120×40 | 64×32 | 64 | 19 | 3 | 24×32 | 32 | 118 |
| 100×30 | 44×22 | 44 | 13 | 3 | 24×22 | 32 | 98 |
| 80×24 | 24×16 | 24 | 23 | 1 | 24×16 | 32 | 78 |
| 60×16 | 4×8 | 4 | 3 · rows 1 | 1 | 24×8 | 32 | **58** |

The 60×16 `card` cell reads `3 · rows 1` and **frameless at the floor**
([`keymap.md`](keymap.md) §5.5, conflict **C6**): below `MIN_CELL_WIDTH` the node
keeps the same `Rect` width but its height drops from `STAGE_HEIGHT` (7) to **1**,
it draws no frame, and it takes no `ROW_GAP`, giving `<glyph> <name>` in 3 usable
columns. The earlier `card 3` figure measured the framed 7-row card and so
described a rectangle the floor no longer draws. The width is unchanged, which is
why `ui::grid_cells` and hit-testing are unaffected.

> **Measurement caveat — C6 is a spec amendment, not yet a rendered fact.** The
> renderer in this tree still draws every card at `STAGE_HEIGHT` (7 rows): there is
> no frameless branch in `ui::pipeline::shape`/`grid` and no `MIN_CELL_WIDTH`-keyed
> height change (`src/ui/pipeline.rs:302`, `:506`). So `card 3 · rows 1` is the
> **contract** §5.5 freezes, not a measurement of current output. Until T1.x
> implements it, a test asserting a 1-row entry at 60×16 would fail against the
> tree — which is exactly the kind of false pass this column exists to prevent, in
> the other direction. Recorded rather than smoothed over.

Two results look wrong until derived, and are correct: 80×24's card is **wider**
(23) than 120×40's (19) because the grid trades three columns for one, and 60×16's
grid is 4 columns wide with a 3-column card. Both follow from `MIN_CELL_WIDTH`
(12, `src/ui/pipeline.rs:102`): `chrome = 3·riser + 2·gap = 5`, `shared =
(width − 5) / 3`, and a `shared` below 12 falls back to one column of
`width − riser`. The notice-row column is `terminal − 2` (§4.2), which is what
makes U5's 34-column longest callout and U9's 21-column `hidden N` line readable
at the 60×16 floor.

## 2. How to read a cell

Each cell must be decidable from **one** of three observables, named in the row:

- **`buffer`** — the rendered `TestBackend` frame at that size, read through the
  renderer's own geometry (`ui::team_area` / `ui::sidebar_area` / `ui::grid_cells`).
- **`registry`** — the `Action` catalogue (`src/ui/actions.rs`, does not exist yet)
  and the absence of `KeyCode::` outside it and the text editors.
- **`model`** — app state before drawing (queues, receipt states, admission ids),
  for the clauses a buffer cannot show.

A cell whose criterion needs an observable that no seam exposes is recorded as
**blocked**, with the missing seam named — not marked pass.

Status vocabulary: **`not evidenced`** (no test exists; §6 name frozen) ·
**`blocked`** (needs a spec decision or a seam first — none remain as of rev 5) ·
**`filled`** (run 1, symbol + observed output).

## 3. Matrix — 48 cells

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
| U12 | 120×40 | Every prompt shows queued → sent → `acknowledged` → completed per employee, **on the card and in the tasks panel**; a failed delivery offers in-place retry; retry re-sends the **same** admission id. The card is 19 wide, usable 17, inside §2.7's 12–20 band where the head row carries the receipt — so §3's scope clause does not apply and both surfaces are required. | buffer + model | `u12_receipt_walks_queued_to_completed` · `u12_retry_reuses_the_admission_id` — **not evidenced** |
| U12 | 100×30 | §3's scope clause applies here too: card 13 wide, usable 11, and [`keymap.md`](keymap.md) §2.7's ladder drops the receipt glyph at usable 10–11 (`▶ Aria ×2`, no receipt). So the **tasks panel** (32) must carry `acknowledged`; the card carries the count and the name, and its absence of a receipt is the contract. | buffer + tasks panel | same symbols — **not evidenced** |
| U12 | 80×24 | As above; card 23 wide (single-column fallback), usable 21, inside §2.7's ≥21 band — the receipt and the count both fit, so both surfaces are required and §3's scope clause does not apply. | buffer | same symbols — **not evidenced** |
| U12 | 60×16 | §3's scope clause applies to U12: the card is a 1-row frameless 3-column entry, so a receipt glyph would make it 4 cells and cannot be drawn there (`docs/keymap.md` §5.5, §8 C5). The receipt is therefore **not** required on the card; the **tasks panel** (32 of 60) is the surface that must carry `acknowledged`, and the card's absence is the contract, not a failure. | buffer + tasks panel | same symbols — **not evidenced** |

---

## 4. Checkability audit — what must be settled before these cells can pass

Recorded now, because a skeleton whose cells cannot be decided is not a skeleton.
None of these is a weakening; each is a question the matrix surfaces.

### 4.1 Blocked cells — **none remain** (2 resolved in rev 5)

| Cell | Was blocked because | Resolution |
|---|---|---|
| **U5 @ 60×16** | The grid panel is **4 columns** wide (2 usable); an empty roster renders `no` there, and none of the four markers (10–19 chars) fits. "Exactly one primary action" was not assertable from a buffer. | **Decided (rev 5):** the callout is drawn on the roster's **notice row** (log band, first content row) at *every* size, so the host no longer varies with grid width. 58 columns usable at the floor against a 34-column longest callout. |
| **U9 @ 60×16** | The grid title is 4 columns and renders `tea`; neither `rows a-b/c` nor `hidden N` fits. | **Decided (rev 5):** `hidden N` is drawn on the grid title while the title can hold it, and on the **notice row** below that — one host or the other, never both, never neither. At 60×16 the notice row carries `◇ hidden 11 · f focus` (21 of 58 columns). |

Both cells are now `not evidenced`, not `blocked`. No requirement was relaxed:
each gained a host clause, and U9 gained the "never both and never neither"
obligation — a strengthening, not a neutral edit.

**Checked against the specs written after this skeleton began:**

- [`keymap.md`](keymap.md) §5.1 pins the same four breakpoints and the same card
  widths measured here (`33/19/13/23/3`).
- [`help-palette.md`](help-palette.md) §"at 60×16" specifies the help overlay's
  clamp and its `rows a-b/n` counter, which covers **U4** at that size.

### 4.2 At-risk but decidable

- **U9 @ 80×24** — title is 24 columns and already renders `team · 12 · 1-1/12`
  with the `rows ` word dropped. `hidden N` fits on the title at 80×24 and up per
  §4.2's 22-column test, but the test must assert the counter's *presence*, not its
  full adjacency to `rows a-b/c`.
- **U11 @ 60×16** — class + id + cause + action in 60 columns on a 4-row band. If
  the row must be ellipsised, the class must be what survives (it is the
  load-bearing token); the test should assert the class, not the whole string.
- **U12 @ 60×16 and 100×30** — the card cannot carry the receipt at either size,
  and since rev 17 that is not a cell-level judgement but **U12's own scope
  clause** (§3): at the floor the entry is 3 usable columns and a receipt glyph
  would make it 4 (C5); at 100×30 the card is 13 wide → usable 11, and
  `keymap.md` §2.7's frozen ladder drops the receipt at usable 10–11 (`▶ Aria ×2`).
  The criterion therefore requires the tasks panel (32 of 60 / 32 of 100) to carry
  `acknowledged` and does not require the receipt on the card at either size.
  Asserting "on the card" there is a false pass waiting to happen, so the cell is
  asserted per surface, with the card's *absence* of a receipt as the contract.
  At 120×40 (usable 17) and 80×24 (usable 21) the ladder keeps the receipt, so both
  surfaces are required and no clause applies.
- **U6 @ 60×16** — same shape (C3): the queue count is carried by the tasks panel,
  not the card. A test that greps the card for `×2` will fail against a correct
  floor; a test that greps the *tasks panel* is the right one.
- **U8 @ 60×16** — the card is 3 columns wide and the grid panel 4, so the
  clickable target is ~1–2 usable cells. "No click resolves to a target that was
  not drawn" is assertable, but a *double*-click needs two events landing on the
  same 3-wide rect; the test must drive the pair at the rect's centre rather than
  by scanning, or it will pass vacuously on an empty grid.
- **U2/U4 @ 120×40** — §3 says palette and help "fit without scrolling" here. U4
  requires a counter regardless, so the counter must be asserted even where no
  scrolling is needed (the counter states a fact, it does not only appear on
  overflow).
- **U5/U9 host coupling** — both depend on the **notice row** existing in the log
  band. A test that asserts `no service` or `hidden N` must read the log band's
  first-or-second content row, and U11's error row is pinned above it (§4.2 order:
  pinned error, then notice). A regression that lets the error row consume the
  notice row would fail U5/U9 while leaving U11 green, so the three criteria are
  coupled and should be run together.
- **The reserved row is a *layout* claim, not a buffer claim (rev 9).** That the
  notice survives thinking is asserted by `main` being byte-identical in every
  combination — i.e. by the two `main`-invariant guards §6 cites as-is
  (`the_prompt_band_does_not_shrink_the_panels`, `the_roster_prompt_band_comes_out_of_the_log`)
  plus `u5_notice_survives_the_thinking_state`. A U5/U9 cell cannot prove it,
  because a buffer at a given size looks the same whether the row is reserved or
  the panel silently moved. The test must compare `main` across states, not just
  read the notice row.

### 4.3 Seams the tests need, which do not exist yet

Re-checked against the working tree for this revision; the status column is
current, not inherited.

| Seam | Needed by | Status |
|---|---|---|
| `src/ui/actions.rs` — `Action` catalogue | U1, U2, U3, U4, U8 | **does not exist** (§6 freezes `every_action_has_a_label_and_a_context`, `action_ids_are_unique`, `deprecated_bindings_still_resolve`) |
| `tests/ux.rs` | all 48 cells | **does not exist** |
| **Notice row** in the log band, **reserved** while thinking | U5, U9 | **does not exist** — today the band is `4` rows at rest and `3` while thinking (`src/ui/layout.rs:42-43`, `PROMPT_HEIGHT` = 2 / `PROMPT_THINKING_HEIGHT` = 3), so thinking leaves **1** content row and there is no notice slot at all. The rev 9 contract to implement: when a grid-scoped notice exists, `Regions::of` receives `thinking && notice.is_none()`, the band stays at `PROMPT_HEIGHT` (2), the log keeps 4 rows / **2 content rows**, and `main` is byte-identical in every combination. The row is *reserved*, not borrowed — taken back the moment the notice clears, with the indicator returning. T1.1+ implements it; the two `main`-invariant guards are already in the tree (§6) |
| Per-mode focus-mode rect read-back | U7 | not present — no `focus_mode`/`FocusMode` anywhere in `src/`; `ui::team_area`/`sidebar_area` return the resting rects only |
| Connection state (`connected`/`reconnecting`/`down`) | U10 | not present — no such state in `src/`; connection is a glyph (B9) |
| Receipt **state** (`queued → sent → acknowledged → completed`) | U12 | not present — no `Receipt`/`acknowledged` state in `App`, `Task`, or `Message` |
| Admission-id minting | U12 | **exists, but the retry does not reuse it** — `harness::admission_id` (`src/harness.rs:212`) with `ADMISSION_PREFIX` (`:188`) and same-delivery stability pinned by its unit tests (`:269`, `:315`). The retry path is not that case: `retry_or_abandon` bumps `attempt` (`src/app.rs:3429`) and re-sends through `send_to_employee` → `Outgoing::new`, which mints a **fresh** `delivery_token` (`src/app.rs:101-108`), with the kind changed to `PromptKind::Retry` (`:3446`) — three of the five id inputs differ, so the retry gets a different id and the service admits a second message rather than deduplicating. `src/harness.rs:279` said this as a blanket claim ("a retry of the same hand-off is a *different* delivery"), corrected in `acceptance-ux.md` rev 17 to the narrower true statement. U12's "re-sends the same id" is therefore **not** assertable today: the visible receipt state is missing *and* the retry path must be changed to preserve the original token, attempt, and kind |
| Error class + stable id | U11 | `push_error_log` takes a free-form `String` (`src/app.rs:4231`) — B8 |

**Two line-number corrections against an earlier draft of this file** (recorded,
not silently fixed): `ADMISSION_PREFIX` is at `src/harness.rs:188`, not `:182`, and
the uniqueness rule is evidenced by the unit tests at `:269`/`:315`, not by a
`:195-204` range. The seam itself is unaffected. (The prefix test read `:311`
before spec rev 17; the four comment lines rev 17 added above the attempt-bump
assertion shifted it to `:315`, which is where `docs/architecture.md` cites it.)

**Marker-absence re-check.** §2's "absent today" column was re-measured on
`src/ui/` for this revision. All of `actions`, `focus mode`, `hidden`,
`reconnecting`, `acknowledged`, `transient`, `actionable`, `fatal`,
`waiting on`, `stalled`, `no service`, `no team`, `no model assigned`,
`no project assigned` are **0 occurrences** in `src/ui/`, so no marker can be
satisfied by a surface that already draws the word. U4's ` help ` is the documented
exception (§2): it is already drawn as a bordered title, which is why that marker
is asserted on structure rather than on a substring.

### 4.4 Frozen-name coverage — 29 of 34 new names back a cell

`acceptance-ux.md` §6 lists **38 symbols: 34 new names plus 4 pre-existing tests
cited as guards** (rev 9 added `u5_notice_survives_the_thinking_state`, making 32
new where there were 31; rev 10 added the two `main`-height guards; rev 15 added
`u4_help_search_filters_in_place` and `u4_help_dismiss_key_has_no_side_effects`,
making 34; rev 17 added `admission_ids_are_stable_per_delivery_and_distinct_across_deliveries`,
`src/harness.rs:269`, as the fourth guard). The audit below covers the **34 new
names**, because only those are
obligations — the 4 guards already exist and are cited to stay green, so they can
back no cell by construction. Scripted cross-check against the 48 cells in §3 — and
**only** §3, since a citation means a cell cites the symbol: **29 of 34 new names
back a cell; 3 are named only in §4.3's seam table; 2 are named by no cell.**

| Where the name appears | Count | Names |
|---|---|---|
| **Cited by a cell in §3** | **29** | the 27 `tests/ux.rs` symbols cited by cells, `card_keeps_its_glyph_below_twelve_columns` (`src/ui/pipeline.rs`), and `help_overlay_is_a_window` (`src/ui/mod.rs` — §2's bordered-title marker, cited by all four U4 cells since rev 15) |
| **Named only in §4.3's seam table** | **3** | `every_action_has_a_label_and_a_context`, `action_ids_are_unique`, `deprecated_bindings_still_resolve` — all `src/ui/actions.rs` |
| **Named by no cell** | **2** | `hint_bar_keeps_the_help_hint` (`src/ui/mod.rs` — U3(c) at unit level), `u5_notice_survives_the_thinking_state` (`tests/ux.rs` — the reserved-row seam, see below) |
| | **34** | new names audited |
| **Pre-existing guards (outside this audit)** | **4** | `clicking_outside_a_target_keeps_the_selection` (`src/app/tests.rs:4219`), `the_prompt_band_does_not_shrink_the_panels` (`src/app/tests.rs:2205`), `the_roster_prompt_band_comes_out_of_the_log` (`src/ui/layout.rs:213`), and — added in rev 17 — `admission_ids_are_stable_per_delivery_and_distinct_across_deliveries` (`src/harness.rs:269`) — all exist; cited to stay green, not to evidence a cell |
| | **38** | total §6 symbols |

**Rev 17 added the fourth guard without adding a frozen name.**
`admission_ids_are_stable_per_delivery_and_distinct_across_deliveries`
(`src/harness.rs:269`) already exists, so §6 cites it as-is: it backs no cell and
is not one of the 34 obligations, which is why the audit above is unchanged at
**34 new names** while the as-is inventory reads **34 new + 4 existing** (38
symbols). It guards a *claim*, not a surface — the corrected admission-id claim
behind U12's dedup clause (§4.3).

**Rev 15 moved `help_overlay_is_a_window` into the cited column.** It was
previously "named by no cell"; it is now cited by all four U4 cells because the
author decided it *is* §2's bordered-title marker assertion, not a scroll-only
seam. The two names still uncited are both deliberate seams, and §6 labels them so:
`hint_bar_keeps_the_help_hint` restates U3(c) at unit level, and
`u5_notice_survives_the_thinking_state` asserts the one thing no size cell can —
that the reserved notice row still exists in the thinking state.

The middle row is **not** a cell citation. §4.3 records those three as the unit
tests that will police the `Action` catalogue itself, which U1 depends on but no
cell asserts; listing them there does not evidence U1 at any size. Counting them
as "backing a cell" — as an earlier draft of this section did, by searching the
whole file rather than §3 — overstates coverage by three.

The two names no cell cites, with a verdict on each:

| Frozen name | Backs a cell? | Verdict |
|---|---|---|
| `hint_bar_keeps_the_help_hint` (`src/ui/mod.rs`) | **no** | **Seam, status open.** Restates U3(c) at unit level; U3's cells cite the three `tests/ux.rs` symbols, which are the criterion-level assertion. Whether that restatement is deliberate is the author's call. |
| `u5_notice_survives_the_thinking_state` (`tests/ux.rs`) | **no — and that is correct** | **Deliberate seam, not a gap.** See below. |

`help_overlay_is_a_window` was in this table before rev 15; it is now cited by all
four U4 cells — two explicitly, two by "same five symbols" — so it has moved to the
cited column above.

**Why `u5_notice_survives_the_thinking_state` is a seam and not a gap.** The matrix
axes are criterion × *size*; there is no employee-state axis, so no cell can
express "while thinking" without inventing one. Rev 9 explicitly keeps the state
dimension out of the cells — the U5 @ 60×16 cell says the assertion holds
*"identically whether or not the selected employee is thinking"* — which is a
statement that the cell does **not** vary by state. The name therefore asserts the
one thing the grid cannot: that the reserved row still exists in the thinking
state, i.e. that `main` is byte-identical across states. That is the same shape as
[`acceptance-sidebar.md`](acceptance-sidebar.md)'s
`a9_idle_avatar_blinks_in_the_panel`, which exists precisely because no size cell
reaches the `Idle` branch. So: **a deliberate seam, correctly outside the 48
cells.** It is listed here so the count is honest, not because it is a defect.

The other two are recorded, not resolved: either they are deliberate unit-level
reinforcements (in which case §6 should say so, as the sidebar spec does for its
unit-level seams) or one of them is the *only* real assertion for a clause and a
cell should cite it instead. Run 1 cannot decide that, because neither symbol
exists. Flagged for the spec author rather than patched here, since editing §6 is
a spec change (`acceptance-ux.md` §7 rule 1).

#### 4.4.1 U4's search and dismiss clauses — **closed in rev 15 of the spec**

The check above only asks whether each frozen name is cited. Asking the converse —
**is each clause of §4 U4 backed by any frozen name?** — found a hole the count
table above does not cover. **The author decided it on 2026-10-21 (spec rev 15):
§6 gains the two names, and U4's wording is unchanged.**

§4 U4 asserts four things: (1) generated from the registry, covering every action
and inventing none; (2) scrollable `↑↓`/`PgUp`/`PgDn`; (3) **searchable `/`**; (4)
**any dismiss key closes it without side effects**. §6 now freezes four U4 names
plus the marker assertion:

| U4 clause | Frozen name that asserts it | Status |
|---|---|---|
| (1) covers every action, invents none | `u4_help_covers_every_action` | frozen |
| (2) scrolls; the counter tracks the window | `u4_help_fits_and_counts_at_60x16` | frozen — fit and counter |
| **(3) searchable `/`** | **`u4_help_search_filters_in_place`** | **frozen (rev 15)** — filters in place, counter tracks the filtered total |
| **(4) any dismiss key closes, no side effects** | **`u4_help_dismiss_key_has_no_side_effects`** | **frozen (rev 15)** — closes and leaves the screen beneath unchanged |
| §2's marker: the bordered title ` help ` | **`help_overlay_is_a_window`** | **frozen** — labelled as the marker assertion, not a scroll-only seam |

**Why adding names was preferred to narrowing U4.** Both clauses are real
requirements of the overlay, and `docs/help-palette.md` §2.3 specifies search and
§2.4 rule 4 specifies the explicit-dismiss behaviour, so dropping them from §4
would have desynchronised the criterion from the design spec that implements it.
Adding names is additive: no cell's pass condition changed, and rule 1 (never
weaken a criterion to make a build pass) was not invoked.

**What this closes, and what it does not.** The U4 @ 60×16 cell was previously
*unfillable as written* — it demanded search while citing only names scoped to
coverage, fit and the counter. It now cites the clause-level symbols, so it can be
filled once they exist. The cells remain `not evidenced`: none of the new symbols
resolves yet.

**The reasoning that found it, kept as history.** Neither clause was reachable by
renaming or re-reading an existing name, which is why new names were needed rather
than a citation fix:

- No frozen name contained `search`, `dismiss`, `close`, or `clos`. The only
  `search`-adjacent name is `u2_palette_teaches_its_keys` (U2's palette, not the
  overlay); the only close-adjacent one is `u2_palette_esc_restores_mode_and_focus`
  (also U2, and it asserts *restoration* of mode and focus rather than absence of
  side effects on the screen beneath).
- `u4_help_fits_and_counts_at_60x16` is scoped by its own name to **fit** and the
  **counter**, while the U4 @ 60×16 cell demanded *"scrolling and `/` search must
  both work"*. The cell's pass condition was therefore broader than the symbols it
  cited — **unfillable as written** under `acceptance-ux.md` §7 rule 2, not merely
  unevidenced.
- `help_overlay_is_a_window` (`src/ui/mod.rs`) was the one caveat: a unit-level
  seam whose scope §6 did not state, so it *could* have been the intended home for
  search. The author has since decided it is the **bordered-title marker**
  assertion, not search and not a scroll-only seam — which is why it now sits in
  the cited column and the two clause names were added separately.

**Consequence for run 1.** U4's search and dismiss clauses should be recorded as
**not evidenced** until the two new symbols exist — which is already the status of
every U4 cell. Nothing about this decision changes a cell's pass condition; it
changes only which symbols a cell may cite.

**The same converse check on the other eleven criteria** found no second case of
this exact shape — a cell whose own text names a clause while its cited symbol's
name scopes that clause out. That is not a clean bill of health, and the limit is
worth stating:

- Name-level matching is **weak evidence** of clause coverage. A single test may
  legitimately assert several clauses, and a name need not contain a clause's
  keyword to cover it. So "no name mentions X" does **not** establish "X is
  uncovered" — which is why U4 was argued from the *cell text* demanding `/`
  search, not from a keyword scan alone.
- A keyword pass over §4 U1–U12 against the frozen names does surface other
  clauses with no keyword-matching name — e.g. U7's "prompt bar is never hidden"
  and "remembered **per mode**", U11's `fatal`/`transient` distinction, U8's
  `wheel`, U2's `Enter`/alias. Each may well be covered by an existing name; run 1
  cannot tell, because none of the symbols exists. They are listed as **questions
  for the author, not findings**, and are not scored as gaps.

### 4.5 C1–C6 reconciliation with `docs/keymap.md` §8

`docs/keymap.md` §8 lists six conflicts between that spec and the frozen files.
All six are now landed. **None changed a pass condition** — each names an action
more precisely, relocates a host, states a width threshold, or corrects a
measurement.

| # | Where it landed | What changed |
|---|---|---|
| **C1a** | `acceptance-ux.md` U2 + this matrix's U2 @ 120×40 | Chat is stated **not** to be in the text-field exception: its history is `↑`/`↓`, so `Ctrl-P` displaces nothing and the palette opens from `Mode::Chat`. Without this, `u2_palette_opens_from_every_mode` would be unassertable. |
| **C1b** | `acceptance-ux.md` U7 + U7 @ 120×40 | `f` on the roster, **`Ctrl-F`** in chat and with the prompt focused — one action, two forms. A bare `f` types `f` there, so "`f` toggles" was unassertable in chat. |
| **C2** | `acceptance-ux.md` U6 + U6 @ 100×30 | At 13 columns (usable 11) the **marker** `waiting on` (10) is required and the **target** drops. "Ellipsised, never dropped" could not hold: marker + one target character is 12 > 11. |
| **C3** | `acceptance-ux.md` U6 + U6 @ 60×16 | At the floor the **glyph** is required; the **queue count** is carried by the tasks panel. `▶ ×2` is 4 cells against 3 usable, so "glyph **and** count" was impossible. |
| **C4** | already closed (§4.1) | U5/U9 unblocked at 60×16 by the notice row. |
| **C5** | `acceptance-ux.md` U12 + U12 @ 60×16 and 100×30 | The **tasks panel** carries the receipt wherever the card cannot: at the floor, because a receipt glyph makes the entry 4 cells against 3; at 100×30, because `keymap.md` §2.7's ladder drops the receipt at usable 10–11. Since spec rev 17 this is **U12's own §3 scope clause**, not a cell-level amendment. |
| **C6** | `acceptance-ux.md` §3 + this matrix §1 | The 60×16 `card` cell now reads `3 · rows 1`, marked **frameless at the floor**. |

**The 60×16 column is where a false pass was most likely, and the re-run found
one — in the spec's own favour.** Measured on the pinned tree (temporary
`TestBackend` probe at all four axes, deleted after use):

| Probe | Measured at 60×16 | Contract (`keymap.md` §5.5, C6) |
|---|---|---|
| `ui::grid_cells` card height | **7** (`STAGE_HEIGHT`) | **1** (frameless) |
| employees drawn | **1** | **7** |
| card width | 3 (framed, 1 usable) | 3 (frameless, 3 usable) |
| `ui::team_area` | `Rect { x: 24, y: 1, width: 4, height: 8 }` | same |

So §5.5's frameless floor is **not implemented in this tree** — there is no
height branch keyed to `MIN_CELL_WIDTH` in `ui::pipeline::shape`/`grid`
(`src/ui/pipeline.rs:302`, `:506`). Consequences for run 1, recorded rather than
papered over:

1. **`card 3 · rows 1` is a contract, not a measurement.** The §1 table now says
   so explicitly. A test asserting a 1-row entry at 60×16 would **fail** against
   today's tree; one asserting `height == STAGE_HEIGHT` would pass and encode the
   wrong contract.
2. **U6/U12 @ 60×16 cannot be filled yet** even though `tests/ux.rs` will exist:
   their cells now depend on the tasks panel carrying the count/receipt, which is
   assertable, but the *card* half depends on §5.5 landing.
3. **This is the inverse of a false pass** — the cell is stricter than the code, so
   it fails loudly rather than passing wrongly. That is the correct direction for a
   pre-implementation matrix, and it is why C6 had to be recorded as an amendment
   rather than silently "fixed" in the table.

Everything else in the re-run held: all 48 cells remain byte-identical to
`acceptance-ux.md` §4.1, no cell reads `blocked`, and the two amended axes
(`card`/`cols`) match `keymap.md` §5.1's `33/19/13/23/3`.

### 4.6 Baseline the first run diffs against

Measured on the pinned tree, so run 1 has something to move from. These are
**observations, not verdicts** — no cell is scored on them.

| Fact | 120×40 | 100×30 | 80×24 | 60×16 |
|---|---|---|---|---|
| hint pairs **drawn whole** | 11 | 8 | 7 | 5 |
| last pair on the line | `q quit` | `Ctrl-Y copy` **(cut mid-description)** | `Space pause` | `m` **(cut: key only)** |
| hint width / terminal | 119/120 | 99/100 | 78/80 | 59/60 |
| `? help` in bar | present | **absent** | **absent** | **absent** |
| grid title | `rows 1-3/4` | `rows 1-2/4` | `1-1/12` | `tea` |
| `hidden` in title | no | no | no | no |
| card / cols | 19 / 3 | 13 / 3 | 23 / 1 | 3 / 1 |

Counting note: a pair counts only when its **whole** literal is drawn. At 100×30
the line ends `Ctrl-Y copy` (the description `log` is cut) and at 60×16 it ends
`m` (the key with no description at all) — both are the clipping U3(b) forbids, and
a looser count that credited partial pairs would hide exactly the defect the
criterion is about. The summed span width is **120** columns, re-derived from
`hint_bar`'s 11 `(key, description)` pairs at `src/ui/mod.rs:331-345`.

U3 fails today at **all four** sizes, for different reasons — which is worth
stating precisely, since a single "U3 fails" would hide that the failure mode
changes:

- **120×40** — 11 pairs against the ≤5 cap, and `? help` is present only because
  nothing was dropped. Fails the count clause.
- **100×30, 80×24, 60×16** — `? help` is gone while pairs were dropped, so clause
  (c) fails; at 100×30 and 60×16 the last pair is also cut mid-description, so
  clause (b) fails too.

U9 fails at all four (`hidden` absent everywhere, and the title degrades to `tea`
at 60×16). That is consistent with §0's claim that every criterion fails at
hand-off.

## 5. Non-render criteria (UX1–UX6)

Not per size — skeleton only, so the slots exist before T1.1.

| ID | Criterion | Evidence |
|---|---|---|
| UX1 | Status never colour alone — glyph + word + colour, each sufficient | `not evidenced` |
| UX2 | `NO_COLOR` honoured; palette 16-colour safe | `not evidenced` — no handling in `src/` |
| UX3 | Old bindings keep working through 0.22; deprecation recorded | `not evidenced` — `docs/keymap.md` exists (T0.2, 2026-10-21) and carries the deprecation register (§3.7); no test asserts the old bindings still resolve |
| UX4 | No new runtime dependency; no persistence break | `not evidenced` |
| UX5 | `cargo test` · `cargo clippy --all-features -- -D warnings` · `cargo fmt --check` all exit 0 | `not evidenced` |
| UX6 | README controls, `docs/keymap.md`, and this spec name the same bindings | `not evidenced` — `docs/keymap.md` exists, so this is checkable; no test compares the three surfaces yet |

## 6. Freeze rules (inherited)

1. A criterion is never weakened to make a build pass; changing one is a
   deliberate edit recorded in `acceptance-ux.md` §9.
2. A cell is filled only with a symbol that exists plus observed output. No test →
   `not evidenced`, never pass.
3. Editing a cell by hand is a bug: change §4, regenerate. §3 here is script-
   extracted from `acceptance-ux.md` §4.1.
4. Markers must stay unique to their surface (§2).
5. This file may not reach into `acceptance-sidebar.md`'s §2 budget or §3 table
   (U9's sidebar clause).

## 7. Evidence log

| Run | Date | Sizes | Result | Notes |
|---|---|---|---|---|
| — | — | — | **skeleton** | 48 cells defined, 0 filled, 48 `not evidenced`, 0 `blocked` (rev 5 decided the last two). Regenerated at **rev 17** of the spec; rev 9's reserved-row clause is carried in the U5/U9 @ 60×16 cells, and rev 17's U12 scope clause in the four U12 cells. |

### Suite claim in `acceptance-ux.md` §8 — **confirmed**

§8 states run 1 as *"480 passed, 4 ignored, 0 failed — 441 lib + 1 `grid` + 10
`opencode_fake` + 12 `render` + 4 `scheduler` + 11 `sidebar` + 1 `watch_hook`; the
4 ignored are the live-service tests"*. Every part was checked; the source tree is
byte-identical to the one the suite ran against (`git status` clean on `src/`,
`tests/`, `Cargo.toml`, `Cargo.lock`; newest source mtime `2026-10-20 23:39`,
before the run).

| Clause | Verdict | Evidence |
|---|---|---|
| 480 passed | **confirmed** | `cargo test`: 441 + 1 + 10 + 12 + 4 + 11 + 1 = **480** |
| 0 failed | **confirmed** | every target reported `0 failed` |
| 4 ignored | **confirmed** | 2 lib + 1 `live_employees` + 1 `live_fan_out` = **4** |
| per-target split as listed | **confirmed** | reproduced exactly, including `src/main.rs` = 0 |
| the 4 ignored are the live-service tests | **confirmed independently of the run** | `cargo test -- --list --ignored` enumerates exactly those 4, and all four are `#[ignore = "requires a running… OpenCode service…"]`: `live_smoke_connect_list_agents` (`src/opencode/mod.rs:548`), `live_smoke_event_stream` (`:565`), `every_employee_binds_its_agent_and_replies` (`tests/live_employees.rs:11`), `one_goal_fans_out_in_parallel_and_aggregates` (`tests/live_fan_out.rs:38`) |

Note that the ignored-test *attribution* is not something a `cargo test` summary
can establish — it needs `--list --ignored` plus the `#[ignore]` reasons, which is
how it was confirmed here. The counts alone would have agreed even if an ignored
test were, say, a slow benchmark.

### Pre-T1.1 gate

Nothing below is a test result; each is a precondition for run 1 being able to
fill a cell at all.

| # | Gate | Status |
|---|---|---|
| 1 | `src/ui/actions.rs` exists with the `Action` catalogue | **open** |
| 2 | `tests/ux.rs` exists | **open** |
| 3 | U5 @ 60×16 — host decided (notice row) | **closed** (rev 5) |
| 4 | U9 @ 60×16 — host decided (title, else notice row) | **closed** (rev 5) |
| 5 | Notice row implemented in the log band, **reserved** while thinking (`notice.is_none()` term in `Regions::of`) | **open** (rev 9) |
| 6 | Focus-mode rect read-back for U7 | **open** |
| 7 | Connection state for U10 | **open** |
| 8 | Receipt state for U12, **and** a retry path that reuses the admission id | **open** — the id is minted (`src/harness.rs:212`) but the retry bumps the attempt and mints a fresh token (`src/app.rs:3429`, `:101-108`, `:3446`), so it does not reuse it (spec rev 17, §4.3) |
| 9 | U4's `searchable /` and "any dismiss key closes it without side effects" have a frozen §6 name (or §4 U4 drops them) — §4.4.1 | **closed** (spec rev 15: two names added, U4 unchanged) |
