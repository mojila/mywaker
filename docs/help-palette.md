# mywaker — palette and help specification (T1.2)

Frozen design spec for the two **discovery** surfaces: the command palette and the
help overlay. Both are generated from one action registry (T1.1); this file states
what they look like, what they contain, and how they behave.

This is a **specification, not an implementation**. It does not describe what the
code does today, and a mismatch is resolved by moving the code.

> **Status: frozen, rev 6 (T1.2). Decisions taken on 2026-10-21**, with two of them
> narrowing what the comparable TUIs do rather than copying it (lazygit-style role
> keys; **16 ANSI, degrade gracefully** — mywaker's restriction, since lazygit also
> accepts hex).
>
> The sources are those gathered as research input to T0.2 — **lazygit v0.66.2**
> and **k9s v0.51.0** — pinned per claim in §0.1. opencode **v1.19.3** is pinned
> too, but contributes nothing to *this* file; the behaviour this spec series
> borrows from opencode (`+N more`, per-toast action) is **v2.0.0+** and belongs to
> the error surface. Palette, colour, and error conventions are frozen in
> [`keymap.md`](keymap.md); this file applies them to the two discovery surfaces.

Companion to [`acceptance-ux.md`](acceptance-ux.md) — **U1–U4 are the criteria this
file specifies** — and [`keymap.md`](keymap.md) (role keys, status semantics, the
binding table this surface renders).

---

## 0. Provenance and baseline

### 0.1 Conventions pin

The discovery surfaces here borrow from **one** TUI — lazygit — and cite the other
two only to say what they do *not* contribute. The versions matter, so they are
pinned per claim rather than as one bundle:

| Source | Pinned at | What this file takes from it |
|---|---|---|
| lazygit | **v0.66.2** | **scoped `?` help**: one global `optionMenu` binding whose contents are drawn from `Context().Current()`, grouped Local / Global / Navigation, with filter-as-you-type; the bottom line always carries hints (§2.1, §4) |
| k9s | **v0.51.0** | the centred single-line flash only. Its `?` is **not** scoped — see below — so §2.1 takes no help-scoping convention from it |
| opencode | **v1.19.3** | **nothing.** Its two pinned contributions — the 5000 ms toast default and the abort filter — belong to the error surface in [`keymap.md`](keymap.md) §4 |

**k9s's `?` is a fixed table, not a scoped one — and this is the correction that
matters most in this file.** v0.51.0's `?` overlay is a **fixed 19-row table**: one
`RESOURCE` column that does change with the selection, plus hardcoded `GENERAL` (19
rows) and `NAVIGATION` (11 rows) blocks. It has **no scrolling, no search, and no
row counter**. Its genuinely scoped surface is a different one — the **6-row bottom
`Menu`** that names the actions live on the current view.

So the two projects are not the same convention, and rev 1–2 were wrong to cite
them together:

| | lazygit v0.66.2 | k9s v0.51.0 |
|---|---|---|
| `?` contents | from `Context().Current()` | fixed table; one `RESOURCE` column varies |
| grouping | Local / Global / Navigation | hardcoded `GENERAL` (19) + `NAVIGATION` (11) |
| search | filter-as-you-type | none |
| scroll / counter | yes / yes | neither |
| the *scoped* surface | `?` itself | the 6-row bottom `Menu`, not `?` |

§2.1 therefore follows **lazygit**, and borrows nothing about help scoping from
k9s. What k9s contributes to this file is the flash line, which is §0.1's other row
and lives in `keymap.md` §4.

**Four things this pin is careful about.**

1. **Nothing here is attributed to opencode v2.0.0+.** The toast queue with `+N more`
   and its per-toast action button — which the *error* surface borrows in
   [`keymap.md`](keymap.md) §4.2 — are **v2.0.0+** behaviour (verified at v2.1.1),
   and absent from v1.19.3, whose TUI toast is a single `currentToast` slot. This
   file does not rely on them, and neither does any criterion it specifies.
2. **The palette is specified, not cited.** §3's four kinds, its ranking, its
   geometry and its teaching rule come from U2 and from what the surface has to do;
   no comparable TUI is offered as evidence for them. That is deliberate: an
   attribution here would read as support for a design decision that was actually
   made from the criterion.
3. **`?` is not help while a text input is focused — a convention both projects
   share, and mywaker adopts.** lazygit's `?` is one global binding that is not
   reachable while a text field has the keyboard; k9s's overlay is likewise not
   raised from an input. mywaker generalises it: **`?` types `?` in every text
   field** — the goal prompt, chat, and a picker's filter — and the help overlay is
   opened by blurring first, exactly as the palette is
   ([`keymap.md`](keymap.md) §3.3). The consequence is already frozen in the binding
   table: chat's discovery path is `/help`, and `?` on the roster is scoped help.
   This is a convention the file previously left implicit, and leaving it implicit
   is how a hint bar ends up advertising a dead key (U3a).
4. **No pass condition depends on a version.** U1–U4 are decidable from this file
   and the registry contract (§5) alone. The sources above explain where two ideas
   came from; they are not evidence for whether the built surface is correct, and a
   reader who disagrees with an attribution has not found a defect in a criterion.

### 0.2 Tree pin and baseline

- Base commit: `d0756f8`; `docs/acceptance-ux.md` and `docs/keymap.md` are
  untracked on top of it.
- `src/app.rs` `a95d056f`, `src/ui/mod.rs` `4402b0e5`.

**The measured baseline this spec moves away from** (from
[`acceptance-ux.md`](acceptance-ux.md) §0 B1–B4, verified at hand-off):

| # | Today |
|---|---|
| B1 | Three hand-written sources of truth: `hint_bar` (11 pairs, `src/ui/mod.rs:331-345`), `help_lines` (24 key rows + 6 headings + 5 blanks = 35 rows, `:423-471`), `CHAT_COMMANDS` (10 commands, `src/app.rs:268`) |
| B2 | The hint bar is **120 columns, never truncated** — at 80 columns it loses `s task · Ctrl-Y copy log · ? help · q quit` |
| B3 | The help overlay is a fixed `68×30` popup (`:476`) holding 35 rows with no scrolling: at 60×16 it shows **12 content rows for 35** |
| B4 | Production `KeyCode::` literals live in **one** file, `src/app.rs`, across 7 `match key.code` blocks: **75 lines / 93 occurrences**. The 4 literals in `src/ui/prompt.rs` are all inside `#[cfg(test)] mod tests` (module at `:136`; literals at `:157`, `:202`, `:232`, `:282`), so they are test code, not production. Totals: **166 lines / 184 occurrences** including `src/app/tests.rs` (87 / 87). No catalogue to search |

There is no registry today (`src/ui/actions.rs` does not exist) and no palette.

---

## 1. Scope

- **§2** the help overlay: what it contains, how it is scoped, scrolled, searched,
  and how it fits at 60×16 (U4).
- **§3** the command palette: binding, contents, filtering, ranking, and the
  teaching rule (U2).
- **§4** the hint bar, which shares the registry and the truncation rule (U3).
- **§5** the registry contract both surfaces read (U1) — the *interface* only;
  T1.1 owns the implementation.

Out of scope: implementing the registry (T1.1), the error surfaces
([`keymap.md`](keymap.md) §4), and any change to the sidebar's frozen budget.

---

## 2. Help (U4)

### 2.1 Scoped, not global

**lazygit v0.66.2 is the source of this rule, and the only one of the two comparable
TUIs that has it.** Its help is a single global `?` binding whose contents come from
`Context().Current()`, grouped Local / Global / Navigation, with filter-as-you-type.
k9s v0.51.0's `?` is **not** scoped: a fixed 19-row table with one `RESOURCE` column
that varies, plus hardcoded `GENERAL` (19 rows) and `NAVIGATION` (11), no scroll, no
search, no counter. Its scoped surface is the **6-row bottom `Menu`**, a different
surface entirely.

mywaker follows lazygit: help opens on the **focused region**, not as one 35-row
dump.

- `?` opens help **for the current context**, in the two contexts where it is help
  at all — the table below is the complete list.
- A `show all` affordance (`a`) widens to every action, for the user who is looking
  for something they cannot name.

**Where `?` is help, exactly.** Frozen, because "a text field owns the keyboard" is
otherwise a judgement call at each call site:

| Context | `?` does | Reaches help by |
|---|---|---|
| roster, prompt blurred | opens scoped help | `?` |
| projects picker | opens scoped help | `?` |
| goal prompt (focused) | **types `?`** | `Esc`, then `?` |
| models picker (its filter always has the keyboard) | **types `?`** | `Esc`, then `?` |
| chat | **types `?`** | `/help` |
| session picker, reopen prompt | swallowed | dismiss the modal first |

**`?` is not help while a text input is focused — a convention both projects share,
and mywaker adopts it rather than inventing it.** lazygit's `?` is one global binding
that is not reached from inside a text field, and k9s's overlay is not raised from
one either. mywaker generalises it to every text field it has, and the rule is the
same one the palette follows ([`keymap.md`](keymap.md) §3.3): blur first. This is
what makes `/help` chat's only key-reachable discovery path, and why §2.5's
one-surface rule is load-bearing rather than tidy — in chat, the two surfaces are not
alternatives, one of them is the only way in.

**A `global` section is shown first — and that is mywaker's inversion, not an
inherited convention.** Both sources put the *scoped* section first, on the reasoning
that a user who pressed `?` is asking about where they are. mywaker inverts it for a
reason the sources do not have: at 60×16 the overlay has **12 content rows** against
35 today and 41 once every action is catalogued (§2.3), and the one thing a lost user
cannot recover from is not seeing the escape hatch. Global keys are the only rows
true everywhere, so they earn the first screen; the scoped section follows
immediately, and `a` reaches the rest. Recorded as a deliberate divergence so a later
reader does not "fix" it back to match lazygit.

This is the difference from today, where `help_lines` is one fixed list and the
roster, chat, and pickers are all in it regardless of where the user is.

### 2.2 Contents are generated

**Frozen:** every row comes from the registry. No hand-written help text.

| Column | Content |
|---|---|
| key | the primary binding, styled `key` (`accent` + `BOLD`) |
| label | the action's label |
| deprecated | `(deprecated)` appended when `deprecated_in` is set (UX3) |
| context | shown only in `show all`, as the section heading |

The section headings are the registry's **contexts**. `global` is hoisted to the
top (§2.1's inversion) and the remaining contexts follow in registry order. A
section with no actions in the current context is omitted — an empty `model
picker` heading at the roster is noise.

### 2.3 It must fit, and say how much it does not

At 60×16 the terminal is 60×16; the overlay is clamped to the terminal minus a
one-cell margin, which yields a **58×14** box and **12 content rows**. The
criterion (U4) is not "everything fits" — it cannot, at 35+ rows — it is:

1. **A visible window counter** of the form `rows a-b/n`, drawn in the border
   title: ` help · rows 1-12/41 `.
2. **Scrolling** (`↑↓` one row, `PgUp`/`PgDn` a page) with the counter tracking.
3. **Search** (`/`) filtering the list in place, with the counter showing the
   filtered total.

A help overlay that silently shows 12 of 35 rows — today's behaviour — fails U4.

### 2.4 Layout

```
┌ help · rows 1-12/41 ─────────────────────────────── / search ─┐
│ global                                                         │
│   q · Esc   quit (any screen)                                  │
│   ?         this help overlay                                  │
│   Ctrl-P    command palette                                    │
│                                                                │
│ roster                                                         │
│   ↑↓←→      move around the employee grid                      │
│   …                                                            │
│                                                                │
│ a show all · / search · ↑↓ scroll · PgUp/PgDn page · Esc close  │
└────────────────────────────────────────────────────────────────┘
```

Frozen rules:

1. **The footer is a fixed row**, always visible, naming the four ways out of the
   overlay. It is not part of the scroll.
2. **The border title carries the counter**, so it survives scrolling.
3. **The block uses `Clear`** behind it (as today, `src/ui/mod.rs:475`), so the
   screen underneath never bleeds through.
4. **`Esc` closes without side effects** (U4). Today any key dismisses
   (`src/app.rs:1000-1001`), which is why a keypress meant for the overlay can act
   on the screen beneath it — the dismissal must be explicit.
5. **Help never becomes modal-with-state.** Opening and closing it twice leaves the
   app in the state it started in.

### 2.5 `/help` and `?` are one surface

The chat's `/help` and the `?` overlay read the same registry (U1).

Today they do not: `/help` prints `chat_help_text()` — the **slash-command table**
(`CHAT_COMMANDS`, `src/app.rs:268`), which the completion menu also reads — while
`?` draws `help_lines()`, a separate hand-written list of keys
(`src/ui/mod.rs:423`). Neither covers the other: `/help` has no keys, and `?` has
no slash commands. That split is B1.

Frozen: both are generated from the registry, which carries **actions and slash
commands as one catalogue**. `/help` renders the chat context into the transcript
(through `push_command_line`, `src/app.rs:2536`, so a `/help` typed at the roster
prompt also lands in the log where the user is looking); `?` renders the same
content as an overlay. The difference is the surface, not the content.

---

## 3. Command palette (U2)

### 3.1 Binding

`Ctrl-P` is primary; `Ctrl-K` is an alias (both open the same surface —
[`keymap.md`](keymap.md) §3.1).

**Not bound in a text field.** While the goal prompt or a picker's filter is
focused, `Ctrl-P`/`Ctrl-K` are line-editing keys and stay unbound
([`keymap.md`](keymap.md) §3.3). The palette is reached by blurring first. This is
the one deliberate exception, and U3 requires the hint bar to respect it.

### 3.2 It searches four kinds

| Kind | Source | `Enter` does |
|---|---|---|
| **action** | the registry | runs it |
| **employee** | the roster | opens chat with that employee |
| **project** | `app.projects()` | selects it as the active project |
| **model** | `app.models()` | assigns it to the selected employee |

Each row is prefixed with its kind, so a result is never ambiguous:

```
 ◆ actions      focus mode                          f
 ● employee     Aria · Research lead
 ◆ project      mywaker
 ● model        deepseek-v4-1-flash · kenari
```

The kind glyph reuses [`keymap.md`](keymap.md) §6.2's set (`◆` actionable/active,
`●` present) rather than inventing new ones.

### 3.3 The teaching rule

**Every action row shows its bound key**, right-aligned, in the `key` style. This
is the palette's whole point: it is how a user learns the binding instead of
memorising the palette. An action with no binding shows nothing — and that is a
signal to the author, not to the user.

Frozen consequence: **the palette must never be the only path to an action** (U2,
§7 rule 7 of `acceptance-ux.md`). If an entry has no key, it either gets one or it
is not an action.

### 3.4 Filtering and ranking

- Typing filters **fuzzily** over the label, the kind, and any aliases.
- **Ranking, frozen:** exact prefix > word-start match > subsequence match; ties
  broken by kind (action, employee, project, model) and then registry order. The
  ordering is stable across identical queries, so a repeated query is not a
  moving target.
- An empty query lists **actions only** — the surface's purpose is discovery, and
  a wall of employees is not a menu.
- **No results** draws one line: `no match` — never a blank panel.
- Search is **case-insensitive** and matches `Ctrl-P`-style key strings too, so
  typing `ctrl p` finds the palette itself.

### 3.5 Layout and geometry

```
┌ actions ──────────────────────────────────────────── search ─┐
│ ▸ focus mode                                          f       │
│   palette                                             Ctrl-P  │
│   retry error                                         r       │
│   quit                                                q       │
│                                                               │
│ 4 of 41 · ↑↓ move · Enter run · Esc close                     │
└───────────────────────────────────────────────────────────────┘
```

Frozen rules:

1. **Width:** `min(60, terminal.width − 4)`, centred. At 60 columns that is 56.
2. **Height:** `min(14, terminal.height − 4)`, centred. At 16 rows that is 12.
3. **The footer** names `↑↓`, `Enter`, `Esc` — always visible, never scrolled.
4. **The counter** shows `N of M`, so filtering is legible.
5. **The selected row** uses `selected_bg` ([`keymap.md`](keymap.md) §2.1) *and* a
   `▸` gutter, so selection survives `NO_COLOR`.

### 3.6 Behaviour

| Key | Action |
|---|---|
| `type` | filter |
| `↑` `↓` · `Ctrl-P` `Ctrl-N` | move the selection |
| `Enter` | run the highlighted entry, close the palette |
| `Esc` | close, restoring the **prior mode and focus exactly** (U2) |
| `Ctrl-U` | clear the query |
| `Backspace` | edit the query |

**Restoration is exact** (U2): closing the palette returns the app to the mode,
focus, selection, and scroll it had before opening. The palette is a *lens*, not a
mode — which is why it is not a fifth `Mode` variant (compare
[`keymap.md`](keymap.md) §5.2 rule 7).

---

## 4. The hint bar (U3)

The bar shares the registry and the truncation rule with the two surfaces above.

### 4.1 Content: contextual, at most five

Today it is a fixed 11-pair list per mode, 120 columns wide. Frozen:

1. **At most 5 pairs**, chosen for the **focused region** — not the mode. The
   prompt focused and the roster unfocused are different contexts with different
   keys.
2. **Every advertised key is live in that context.** Advertising a key that would
   be a no-op is a FAIL (U3a). This is exactly the class of bug the palette's
   teaching rule prevents, applied in reverse.
3. **Whole pairs drop, never a clipped pair** (U3b). Today the terminal cuts
   mid-description (`m ` at 60 columns); the bar must drop the pair instead.
4. **The last slot is `? help` when anything was dropped** (U3c). The escape hatch
   is the one hint that never gets cut — which is precisely what fails today (B2).

### 4.2 Width accounting

The bar is composed by measuring each pair, then admitting pairs in priority order
until the next would overflow:

```
fits(width) = Σ over admitted pairs (1 + key + 1 + label + 1) ≤ width
```

The `? help` slot is reserved **first** — taken out of the budget before anything
else is admitted — so rule 4 cannot be violated by arithmetic.

---

## 5. The registry contract (U1's interface)

T1.1 implements this; T1.2 reads it. Frozen shape, because both surfaces and
`/help` depend on it:

| Field | Type | Purpose |
|---|---|---|
| `id` | stable dotted string | identity; also the error/telemetry key |
| `label` | short imperative | what the palette and help show |
| `keys` | `&[KeyBinding]` | primary first, then aliases |
| `context` | `Context` enum | which region it applies to; drives scoping |
| `deprecated_in` | `Option<&str>` | the version that removes an alias (UX3) |
| `run` | handler | the dispatch |

**Frozen invariants** (asserted in `acceptance-ux.md` §6's
`every_action_has_a_label_and_a_context`, `action_ids_are_unique`,
`deprecated_bindings_still_resolve`):

1. `id` is unique.
2. `label` is non-empty and imperative (`quit`, not `quits`).
3. `keys` is non-empty for an action that is not palette-only — and there are no
   palette-only actions (§3.3).
4. `context` is set; there is no "any" escape hatch, because "any" is what makes
   the hint bar advertise dead keys.
5. A deprecated binding still resolves while its version is current.

---

## 6. Open items

1. **The registry's module path** (`src/ui/actions.rs` is assumed) is T1.1's call.
2. **Fuzzy-match scoring** is specified by ranking rules, not by an algorithm; any
   implementation satisfying §3.4's ordering is acceptable.
3. **Search inside help** (`/`) is specified as filtering in place. Whether it also
   matches descriptions, not just labels, is left to T1.3 — both satisfy U4.
4. **`a show all`** is specified as an affordance; its exact key is not frozen
   beyond being shown in help's footer.

---

## 7. Revision log

| Rev | Date | Change |
|---|---|---|
| 1 | 2026-10-21 | Created for T1.2, folding in the TUI conventions (lazygit v0.66.2, k9s v0.51.0, opencode v1.19.3) and the user's two decisions recorded in [`keymap.md`](keymap.md). Freezes help as context-scoped and generated with a window counter and scrolling (§2); the palette as a four-kind fuzzy search that teaches keys, with exact restoration and frozen geometry (§3); the hint bar as five contextual pairs with a reserved `? help` slot (§4); and the registry contract both surfaces read (§5). |
| 2 | 2026-10-21 | **Evidence correction, no design change.** The header attributed this file's conventions to a single version bundle of three projects; it now pins them per claim in §0.1 and says plainly that **opencode v1.19.3 contributes nothing here** — its toast queue with `+N more` and per-toast action button, which [`keymap.md`](keymap.md) §4.2 borrows, are **master only, post-v1.19.3**, and no criterion in this file depends on them. §0 was split into §0.1 (conventions pin) and §0.2 (tree pin and baseline) so the two kinds of provenance cannot be read as one. The header also now says that **16 ANSI is mywaker's narrowing, not lazygit's rule** (lazygit accepts hex). §2.1's lazygit/k9s attribution was already per-source and is unchanged; no pass condition, geometry, or behaviour in §2–§5 changed. |
| 3 | 2026-10-21 | **Help-scoping attribution corrected — lazygit only, and one inversion named.** §2.1's opening sentence said *both* lazygit and k9s make `?` the context-sensitive entry point. They do not: **lazygit v0.66.2 does** — one global `optionMenu` binding, contents from `Context().Current()`, grouped Local / Global / Navigation, filter-as-you-type — while **k9s v0.51.0's `?` is a fixed 19-row table** (one scoped `RESOURCE` column plus hardcoded `GENERAL` 19 and `NAVIGATION` 11, no scroll, no search, no counter); k9s's scoped surface is the **6-row bottom `Menu`**, not the overlay. §0.1's k9s row now says so and keeps only the flash line, and §0.1 gains a side-by-side table so the two cannot be conflated again. Two further things are now explicit rather than implicit: **"a `global` section is always shown first" is mywaker's inversion** — both sources put the scoped section first — with the 60×16 reason recorded so it is not "fixed" back; and the **shared convention that `?` is not help while a text input is focused** (`?` types `?` in the goal prompt, chat, and a picker's filter; blur first, as with the palette), which is what makes `/help` chat's only key-reachable discovery path and why §2.5's one-surface rule is load-bearing. §2.2's heading order now says `global` is hoisted. No pass condition, geometry, or binding changed. |
| 4 | 2026-10-21 | **§0.2's B4 corrected — production is one file, not two.** B4 read "`KeyCode::` literals in production code live in two files only (`src/app.rs`, `src/ui/prompt.rs`) … **79 by line** (75 + 4) … **97 by occurrence** (93 + 4)". The 4 literals in `src/ui/prompt.rs` are all inside `#[cfg(test)] mod tests` (module at `:136`; literals at `:157`, `:202`, `:232`, `:282`), so calling that file "production" was wrong and the 79/97 totals counted test code as production. B4 now states **75 lines / 93 occurrences, all `src/app.rs`**, names the `prompt.rs` four as test-only, and keeps the inclusive totals (**166 lines / 184 occurrences**, with `src/app/tests.rs` at 87 / 87). Verified at this revision: `grep -c` gives 75/4/87, `grep -o` piped to `wc -l` gives 93/4/87, and `awk 'NR<136'` finds **no** production `KeyCode::` in `src/ui/prompt.rs`. This matches [`acceptance-ux.md`](acceptance-ux.md) §0 B4 as corrected in its rev 7, so the two files no longer disagree. No design, geometry, or binding changed. |
| 5 | 2026-10-21 | **opencode attribution corrected — a pinned-tag error, not a design change.** The header and §0.1 item 1 said the `+N more` queue and per-toast action were "post-v1.19.3 / master only". Verified at tag `v1.19.3` (`e3c4eb2`): its TUI toast is a single `currentToast` slot with **no queue and no `+N more`**, and queueing lands at **v2.0.0** (verified at **v2.1.1**, the local binary). Both places now cite **opencode v2.0.0+** and state the v1.19.3 absence, matching [`keymap.md`](keymap.md) §0.1 rev 9. Nothing in this file depended on the old attribution — opencode contributes **nothing** to the palette or help surfaces — so no criterion, geometry, or behaviour changed. |
| 6 | 2026-10-21 | **Help-scoping attributed to lazygit alone, and two conventions made explicit.** §2.1's opening sentence said *both* lazygit and k9s make `?` the context-sensitive entry point; they do not. **lazygit v0.66.2** has scoped help — one global `optionMenu` binding, contents from `Context().Current()`, grouped Local / Global / Navigation, filter-as-you-type — and **k9s v0.51.0 does not**: its `?` is a fixed 19-row table (one scoped `RESOURCE` column plus hardcoded `GENERAL` 19 and `NAVIGATION` 11) with no scroll, no search and no counter, and its scoped surface is the **6-row bottom `Menu`**. §0.1's k9s row now keeps only the flash line and gains a side-by-side table so the two cannot be conflated again. Two further things are now stated instead of implied: **"a `global` section is always shown first" is mywaker's inversion** — both sources put the scoped section first — justified by the 12-row window at 60×16 and recorded so it is not "fixed" back; and the **shared convention that `?` is not help while a text input is focused**, generalised to every text field here (`?` types `?`; blur first, as with the palette), which is what makes `/help` chat's only key-reachable discovery path and §2.5's one-surface rule load-bearing. §2.1 gains a frozen table of exactly where `?` is help, and §2.2 says `global` is hoisted. No pass condition, geometry, or binding changed. |
