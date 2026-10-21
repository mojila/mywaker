# Phase B — the per-employee queue block: row spec

**Status:** design spec, rev 2. Normative. Nine of the ten findings in §10 have landed
in the working tree; **F6 (the height-budget overshoot) is open**.
**Scope:** the rows of the queue block inside the tasks panel, at a **fixed 32-column
panel / 30 usable columns**.
**Owns:** the row grid, the indent, the glyph column, the word column, the summary
budget, the `+N more` overflow row, the trailing held row, and the block's height
budget.
**Does not own:** the panel's width (`layout.rs:68-75`), the block's presence, the
per-employee board's rows, or the summary's own 48-char cap (`app::task_summary`).

Design stage: **audit → spec**. No implementation here; it is a contract for
`@writer` and a check sheet for `@tester`.

**Citation convention.** A bare `§n` is a section of *this* file. Every reference to
another document is prefixed with its filename (`keymap.md §2.6`, `acceptance-ux.md
C3`), because this file's §4.1, §6.1, §7 and §7.1 mean different things from
`keymap.md`'s and the two must not be confused.

---

## 0. What is inherited, and not up for renegotiation

| Inherited | Source | Consequence for this spec |
|---|---|---|
| Panel is **32 columns**, `Borders::ALL`, so **30 usable** | `src/ui/layout.rs:68-75`; `src/ui/tasks.rs:28-31` | **No row exceeds 30.** R1 is exactly 30 because its summary is a fixed budget; R0/R2/R3 are as wide as their content and never padded. |
| Task states are a **triple** — glyph, word, role | `keymap.md` §2.6 | Glyphs and words come from that table and nowhere else. |
| A task row is `<glyph> <status word padded to 9> <summary>` | `keymap.md` §7 (`task row`, which cites the queue row `src/ui/tasks.rs:83-92` by line) | The word **stays**. See §4.1. |
| A status surface drops detail → word → glyph → itself, **keyed to usable width** | `keymap.md` §2.7 | The word is dropped below usable 7. At usable 30 nothing is dropped, so dropping it here would be a divergence, not a degradation. |
| `⏸ ▷ ▹ ▸` are width-risky and legal **only** in the tasks panel, the palette, the transcript | `keymap.md` §2.7 rule 2 | `▷` and `⏸` are legal here. Still 1 cell; see §4.4. |
| Within one surface, two **states** may not share a glyph | `keymap.md` §2.7 rule 1 | `▶` is the *employee*; a task running is `▷`. `✓`/`✕` appear in both vocabularies and `keymap.md` §2.6 says why that is deliberate. |
| Head grammar `▶ Aria ◐ ×2` — the depth is `×<n>` | `keymap.md` §6.4 | R0's count is `×<n>`, never `3 queued` or `(3)`. |
| Spacing is 0/1/2/4, and there is no fifth value | `keymap.md` §6.3 | Two blank rows per block, both SPACE_1: one before R3, one closing the block (§6.4, §7.3). |
| The panel is **read-only** — the grid is the selection surface | `src/ui/tasks.rs:1-3` | Nothing in this block is selectable. See §5.1. |
| At 60×16 the panel carries the queue count and the receipt | `acceptance-ux.md` U6 @ 60×16, C3/C5 | `×N` on R0 is not decoration; it is the floor's only host for the count. |

**The width is unchanged.** This spec adds no column and removes none: it
redistributes the 30 cells the panel already has.

---

## 1. The row grid (normative)

Four row types, plus the panel's blank rows. Columns are counted from 0 inside the
borders.

| Row | Grammar, by column range | Width |
|---|---|---|
| **R0** group header | `0` glyph · `1` space · `2…` name ≤ 12 · `1` space · `×N` — the count **follows** the name, so its column varies | ≤ 20 |
| **R1** item | `0–1` indent · `2` glyph · `3` space · `4–12` word `9` · `13` space · `14–29` summary `16` | **30** |
| **R2** overflow | `0–13` empty · `14–22` `+N more` ≤ 9 | ≤ 23 |
| **R3** held | `0–1` indent · `2` glyph · `3` space · `4–12` word `9` · `13` space · `14–18` `×N` | ≤ 19 |

`×N`'s 5-cell allowance is the width of `×9999`; the drawn form is `×3`. R0's 20 is
the widest *legal* row, not the drawn one: `● Aria ×3` is 9.

**R0's count is not in a fixed column, and that is deliberate.** Padding the name to
12 to freeze the count at col 15 would put up to nine blank cells between `Aria` and
`×3` in a 30-column panel, to align a number that appears at most once per employee.
R1/R3's *word* column is rigid because it holds a frozen `keymap.md` §2.6 word on every row and the
summaries must line up; the header's count holds a number that changes with the name
in front of it. Alignment is a means, not a rule to apply where it buys nothing.

Three columns, and each holds exactly one kind of thing. That is the whole design:

- **col 2 — the state column.** One glyph, always present on R1 and R3, always from
  `keymap.md` §2.6. Empty on R2, because R2 has no state (§5.1).
- **col 4–12 — the word column.** A frozen `keymap.md` §2.6 word, padded to 9 so the column is
  rigid across `pending`/`retrying`/`succeeded`. A non-word in this column is a
  defect.
- **col 14–29 — the detail column.** The only free-text column. R1's goal summary,
  R3's count, R2's count.

R0 breaks the grid deliberately, because it is the only row that names a person
rather than a state: its glyph is the **employee** glyph at col 0 and its name starts
at col 2 — which is exactly where an item's glyph sits. That is the alignment rule
in §2.

**Nothing is padded to 30.** A row is as wide as its content; the terminal fills the
rest with the panel's background. Padding a row out to 30 would be invisible on a
default background and would paint the panel's full width on a themed one — and no
row in this spec needs the padding to be correct.

### 1.1 The block, drawn at 100×30

Illustrative; the tables are normative.

```
┌ tasks ───────────────────────┐
│queue · engine paused         │  heading            (Phase A)
│● Aria ×2                     │  R0   Aria has 2 queued
│  ○ pending   draft the migr… │  R1   pending
│  ▷ running   read the change…│  R1   running
│● Bruno ×2                    │  R0   Bruno has 2
│  ◐ retrying  regenerate exam…│  R1   retrying
│  ⏸ paused    fix the broken …│  R1   paused
│              +2 more         │  R2   Dex's 2, held
│                              │  blank (SPACE_1)
│  ⏸ paused    ×2              │  R3   two held
│                              │  closing blank (SPACE_1)
│● Aria · Research lead        │  board head         (Phase A)
│  draft the migration plan    │  board activity     (Phase A)
└──────────────────────────────┘
```

The invariant of §5.2 holds on this frame: 4 drawn items + `+2 more` = 2 + 2 + 2 = the
six items that exist, and each printed `×N` equals its employee's true depth. Dex is
counted, not drawn, so no header is printed for it — and that is exactly the case §5.2
separates: the ledger closes, while `+2 more` refers to an employee the block never
names. The block is `A = 10` rows (heading 1 + groups 6 + `+N` 1 + blank 1 + held 1),
leaving 10 of the panel's 20 content rows to the board.

---

## 2. The indent

**Exactly 2 cells**, cols 0–1, on R1 and R3. Zero on R0.

It is **not a spacing token**. It is R0's own `glyph + space`, so an item's glyph
(col 2) lands **directly under the first character of the name** (col 2 of R0). The
indent is derived from that alignment, not chosen from `keymap.md` §6.3's scale. A block whose
item glyphs do not sit under the name column is a defect.

**Why the indent is worth 2 summary cells.** The obvious alternative is indent 0 —
glyph at col 0, word at 2–10, summary 12–29 (**18** cells instead of 16). It is
rejected: in a frameless 30-column panel, indentation is the *only* hierarchy signal
there is, and without it a queue item is flush with the header above it and reads as
its sibling rather than as its contents. Two cells of goal text are the cheaper
price. (The glyph itself is not optional — `keymap.md` §7 names it as the thing today's row
lacks — so the real comparison is indent-and-16 against no-indent-and-18, not
against today's 18.)

**R0 is flush** because it is the parent. A header that indents has nothing to be
indented under.

---

## 3. The group header (R0)

```
● Aria ×3
```

- **glyph** — the *employee* glyph from `keymap.md` §2.6 (`● ▶ ✓ ✕`, spinner while
  thinking), in
  its status role. It is the employee's mark; the block's item rows carry the task's.
  `keymap.md` §2.6 permits both vocabularies in one panel because the queue section and the board
  are separate blocks, and `keymap.md` §2.7 rule 1 is about two *states* sharing a glyph.
- **name** — ≤ **12** cells, cols 2–13, ellipsised from the right with `…`. The same
  cap the board's head row uses, so a name truncates at the same column in both
  blocks.
- **`×N`** — the employee's **true** queue depth (`app::tasks_for(index).len()`),
  never the number of rows drawn. `×0` is never drawn: a group with no items is not
  drawn at all. `×N` is the panel's only host for U6's queue count at the 60×16 floor
  (`acceptance-ux.md` C3), which is why it is here and not only on the board.
- **No role.** The role is `meta` decoration (`keymap.md` §5.4 step 2) and the board's head row
  three rows below already draws it. Spending 13 of 30 cells to repeat it is the
  duplication this block exists to avoid; the count is the thing R0 can say that the
  board cannot.
- **`×N` is `text`, the name is `BOLD`** — `keymap.md` §6.1's `emphasis` is `text_bright + BOLD`
  and belongs to the *selected* name; a header here is never selected (the panel is
  read-only), so its name takes `BOLD` alone and the count takes `text`. The count is
  not `muted`: it is the only copy of the depth (U6).

**The unassigned group** has no employee, so its header is the literal `— unassigned`
in `meta`, matching the board's existing placeholder. `—` is a placeholder, not a
state word, so `keymap.md` §2.6's vocabulary is untouched. It has no glyph column entry and no
`×N` (nothing is being counted *for* anyone), so the row starts at col 0.

---

## 4. The item row (R1)

### 4.1 The word stays — the one decision in this spec most likely to be questioned

An earlier draft of this file dropped the status word to buy 10 cells of goal text
(26 instead of 16). **That is rejected**, on three citations:

1. `keymap.md` §7 freezes the tasks-panel row as `<glyph> <status word padded to 9>
   <summary>` and cites the queue row (`src/ui/tasks.rs:83-92`) by line as the row it
   is describing. A spec does not get to overwrite a frozen grammar by arguing about
   it; it amends the file first (keymap's own preamble), and this file is not the
   place that amendment is made.
2. `keymap.md` §2.7's ladder drops the **word** only below usable 7. At usable 30 the ladder
   drops nothing. Dropping the word here is a divergence dressed as a degradation.
3. `keymap.md` §2.6 defines every state as a triple, and `Paused`/`Retrying` are told apart by
   **word** as well as glyph. A row carrying two of three is legible but is not the
   frozen row.

The arithmetic, so the cost is visible rather than assumed:

| Grammar | Chrome | Summary |
|---|---|---|
| today (`queued_task_line`: indent 2 + word 9, no glyph) | 11 | **18** (of 29 used — one cell wasted) |
| **R1 (this spec)**: indent 2 + glyph 1 + space 1 + word 9 + space 1 | 14 | **16** |
| draft (glyph, no word) | 4 | 26 — *requires a `keymap.md` §7 amendment* |

So the glyph costs 2 cells of goal text: the row grows from 29 used cells to 30, and
the glyph plus its space take 2 of them. If 26 is wanted later, the change is an
amendment to `keymap.md` §7 first, recorded as a conflict in that file's §8 — not a
silent divergence here.

### 4.2 Summary budget and truncation

- **16 cells, cols 14–29.** Fed by `app::task_summary` (`src/app.rs:4824`), which
  already takes the first non-empty line, collapses whitespace, and caps at 48 — so
  the input is single-line and bounded before it reaches the row.
- **Right ellipsis** with `…`, via `ui::truncate` (`src/ui/mod.rs:42-52`).
  Character-granular, not word-granular: a goal is free prose with no stable word
  boundary, and `keymap.md` §4.2 permits exactly this for a *cause* while reserving the
  no-mid-word rule for labels and actions.
- **Never wraps.** One row is one item. A wrapped row desynchronises every row below
  it and breaks the col-2 alignment.
- **Never truncation without `…`.** A goal ending flush at col 29 reads as a complete
  goal that happens to be short — a silent drop (`keymap.md` §5.4 step 6).

### 4.3 Colour

- **The summary is `text` (`body`), never the state role.** This is `keymap.md` §2.3's marker
  rule: `err` is 2.85:1 and `muted` 4.16:1 on dark, so a summary in the state colour
  would be a *sentence* in a marker colour, which `keymap.md` §2.3 forbids outright. `text` on
  dark is 13.23:1; on light it is overridden to Black, 19.26:1.
- **The glyph is the state role** (`status::task_color`). A marker role may colour a
  glyph — that is what a marker colour is for.
- `Succeeded` / `Failed` rows are **not dimmed or struck**. Terminal rows stay in
  `text` so the block reads as one column of goals; the glyph alone marks the end
  state. Dimming finished work makes it look like absent work.

### 4.4 Width

Every glyph in the state column must measure **1** under `unicode-width`'s default
(non-CJK) mode, which `▷` and `⏸` do. In a CJK-configured terminal they may measure
2; `keymap.md` §2.7 rule 2 already names that as the accepted risk of keeping `⏸`, and it is
confined to this panel by design. **Do not** pad the column to 2 to absorb it — that
would spend a column on every row, for every user, to serve one configuration.

### 4.5 Order and truncation rule

- Groups appear in `app::workflow_order()` order — the panel's existing employee
  order — then the unassigned group.
- Within a group, items follow `app::task_order_key` (active first, then
  newest-first), which `app::tasks_for` already applies.
- **Truncation is a tail cut, not a filter.** Once a row cannot be drawn, every item
  after it in the block's order is counted into `N`. A block that hides a `running`
  row while drawing a later `succeeded` one is a defect: `+N more` must always mean
  "everything after the last drawn row".
- **A group is drawn whole or counted.** A header with no item under it would read as
  "nothing queued" for an employee who has work waiting. So a group is drawn only if
  its header **and at least one item** fit alongside the overflow row its own
  remainder makes necessary.

---

## 5. The `+N more` overflow row (R2)

**One row for the block**, drawn only when `N > 0`, at **col 14** — the detail
column.

```
              +2 more
```

### 5.1 Grammar and why

- **No glyph, no word.** It has no state, and it is not a frozen `keymap.md` §2.6 word. Cols 2–12
  are therefore *empty by rule*, not by accident: the state column and the word
  column are reserved for `keymap.md` §2.6 vocabulary, and a non-state in either is a defect.
  The detail column is the block's only free-text column, so that is where it goes.
- **`text`, not `muted`.** `keymap.md` §2.3: `muted` is "never the only copy of a fact", and `N`
  is the only copy of how many rows are hidden. Subordination comes from having no
  glyph and no word, not from being dimmer.
- **Not interactive.** The panel is read-only (§1), so R2 is a **report, not an
  affordance**. It must not be `accent`, must not carry a key hint, and must not look
  like a button: `keymap.md` §7.1 forbids a hint for a key that is not live, and a button-shaped
  row that does nothing is the same lie in visual form. There is no "expand" — the
  answer to "how do I see the rest" is the same as for any truncated panel.
- **`N` in full.** `+999 more` is 9 cells against a 16-cell slot, so there is no
  width reason to abbreviate. Never rounded, never `99+`, never `+0 more`.

### 5.2 The counting invariant

`N` = the number of queue items **not drawn**, counted from the same accessors the
rows are drawn from. Two equations hold, and they are different:

> **A. Every item is counted once: `Σ(drawn item rows) + N = total items`.**
>
> **B. A drawn header tells the truth: for every R0 on screen, `×N` = that
> employee's true depth, drawn rows or not.**

`A` is the ledger. `B` is the honesty of the marks.

The two are **not** interchangeable, and the difference is the whole reason `N` is a
block-level row rather than a per-group one. A group dropped whole prints **no
header** — so its items are in `N` but their `×N` is nowhere on screen. At 80×24 with
`writer ×5` fully drawn and `reviewer ×2` dropped whole, `N = 2` while the only
visible header says `×5`: `+2 more` is counting items for an employee the block never
names. That is correct behaviour (§4.5: a group is drawn whole or counted), and it is
why `N` may exceed the remainder of the headers above it.

A `+N more` that disagrees with `A` — that drops an item from both the rows and the
count — is the silent drop `keymap.md` §5.4 step 6 forbids. `N` is therefore derived from **one**
accumulator, incremented at the single point where a row is skipped, never recomputed
from a second traversal of the same data.

---

## 6. The trailing held row (R3)

The last row of the block: one row, block-level, counting **every held task**.

```
  ⏸ paused    ×3
```

### 6.1 Grammar

indent 2 · glyph `⏸` (col 2) · space · `paused` padded to 9 (cols 4–12) · space ·
`×N` (from col 14).

- **The word is `paused`, never `held`.** `keymap.md` §2.6 freezes one word per state, and a
  synonym would invent a second word for `Paused`. The row is *called* the held row
  because that is its job; what it *draws* is the frozen word.
- **`×N` trailing, not `<count> paused`.** Two reasons: `keymap.md` §6.4's depth idiom in this
  panel is `×<n>` after the thing counted (`▶ Aria ×3`), and count-first puts the
  word at col 6 or 7 depending on the number of digits, which breaks the word
  column. Trailing keeps `paused` at col 4 on every count.
- **The whole row is `warn`** — glyph, word and count. `warn` is 9.79:1 on dark and
  collapses to Black (19.26:1) on light, so it is not a marker colour on either
  scheme and may colour a phrase (`keymap.md` §2.3). Note that this row is drawn in its *state
  role*, which is not one of `keymap.md` §6.1's nine text tokens: its tokens express
  hierarchy, `keymap.md` §2.6's roles express state, and a state row uses its role. R1's glyph
  does the same.

### 6.2 What it counts — and what it must not

Counts tasks with `status == Paused` (`app::held_tasks`), and nothing else.

- **Not `Pending` work behind a paused engine gate.** Those are queued, not held;
  they dispatch on resume. Counting them would overstate what needs a human.
- **Not `Retrying`.** "Not moving, not failed" (`keymap.md` §2.6), but the app is acting on it.
- **Not split by `PauseReason`.** `User` vs `Shutdown` is a real distinction, but it
  is the reopen prompt's job at startup. Splitting it here would need a second word
  for one state.
- **`count == 0` draws no row and no blank row.** A `0 paused` row is the same defect
  as U9's forbidden `hidden 0`.

### 6.3 Vocabulary: this block draws tasks, not prompts

The row is often called the *held-prompts* row. That name is wrong here and is worth
correcting once, because the two queues in this program are easy to confuse
(`architecture.md` §14.1):

| | this block | the prompt queue |
|---|---|---|
| holds | persisted scheduled **tasks** (`app.tasks()`) | prompts `Chat::queue_running` holds |
| drawn as | one R1 row per task, R3 counts `Paused` | `×N` depth on the head row |
| vocabulary | `keymap.md` §2.6 **task** glyphs | `keymap.md` §2.6 **delivery receipt** glyphs (`○ ▹ ◐ ✓`) |

They must never share the glyph column (`keymap.md` §2.7 rule 1): `○` means `pending` in this
block and `queued` in a receipt. If the block is ever switched to prompts, the whole
glyph column switches with it — it is not extended.

### 6.4 Unmistakable from an item row

R3 shares R1's grid, so three rules separate it:

1. **A blank row (SPACE_1) always precedes it.** R3 is never adjacent to an R1 row.
   This is the block's only internal gap, and it is the sanctioned 1-cell value.
2. Its detail column holds a **count**, not prose.
3. It is the **only** row with no employee above it — it closes the block.

### 6.5 The `⏸`-twice case (Phase A, now normative)

When `app.is_paused()` is set, the heading draws the gate; R3 draws the count. Both
are `Paused`, so this is not a `keymap.md` §2.7 rule-1 violation (one state, two marks), but two
marks for two different facts is noise in a 30-column panel. **The heading drops the
glyph and keeps the word** — `queue · engine paused` — because R3 now carries the
glyph for the state. This is a one-line change in `queue_heading`, and it is
normative here rather than advisory because the row that justifies it is in this
spec.

---

## 7. Height budget and the degradation ladder

### 7.1 The budget

Panel content rows `H = main.height − 2`. Block allowance `A = ⌊H/2⌋`, so the board
always keeps at least as many rows as the queue. `A` counts **every block row except
the closing blank** (§7.3).

| Size | main | H | A | heading | held (blank+row) | groups + items + `+N` | block rows | board rows |
|---|---|---|---|---|---|---|---|---|
| 120×40 | 64×32 | 30 | 15 | 1 | 2 | 12 | 16 | 14 |
| 100×30 | 44×22 | 20 | 10 | 1 | 2 | 7 | 11 | 9 |
| 80×24 | 24×16 | 14 | 7 | 1 | 2 | 4 | 8 | 6 |
| 60×16 | 4×8 | 6 | 3 | 1 | **1** (blank released) | **1** (the `+N` row) | 4 | 2 |

`block rows` is `A + 1` (the closing blank) and `board rows` is `H − block rows`;
`A = heading + held + (groups + items + `+N`)` on every row above. The `+N` row comes
**out of** the group/item budget, not on top of it (§7.3). At 60×16 the ladder's step
1 releases the blank before R3, which buys exactly one row; that row becomes the
`+N more` row, and no item fits. The board's own overflow
at 100×30 (4 employees need 12 rows, 9 are available) is pre-existing and Phase A's.

### 7.2 The ladder (frozen order)

Drop in this order and stop as soon as the block fits:

1. **The blank separator before R3.** Saves 1 row. R3 becomes adjacent to the last
   drawn row — accepted, because a hidden goal is worse than a crowded held row.
2. **Item rows, from the tail** (§4.5), with the remainder reported by `+N more`.
3. **Whole groups**, counted into `N`.
4. **The held row.** Last to go, and only when `A < 2`, at which point the block
   itself is being dropped.

Colour is never the thing that survives (`keymap.md` §2.7); nothing here changes a glyph's
presence except dropping the whole row.

### 7.3 The reservation invariant (the row that must not be spent twice)

> **`heading + [blank] + held + drawn group rows + (N>0 ? 1 : 0) + closing blank ≤
> H`, and the group/item rows ≤ `A − heading − [blank] − held`.**

The overflow row is **reserved before any item row**. If the queue does not fit
whole, the budget available to groups and items is `A − heading − held − 1`; only
when the queue fits whole is it `A − heading − held`. Spending the full budget and
*then* appending `+N more` overshoots by one row every time a group is dropped whole
after the budget was exactly consumed — the block then eats a row the board was
promised. This is live in the tree today (§10, F6).

### 7.4 The floor: what 60×16 draws

`A = 3`; step 1 releases the blank before R3, so the group budget is `1` and it is
spent on the `+N more` row:

```
┌ tasks ───────────────────────┐
│queue · engine paused         │  heading
│              +7 more         │  R2   N = the whole queue
│  ⏸ paused    ×2              │  R3
│                              │  closing blank
└──────────────────────────────┘
```

`+7 more` with zero rows above it is **correct**: `N` counts rows not drawn, so with
none drawn it is the whole queue. This is the same reading as the toast's `+N more` —
"how many are behind what you can see" — and it is what makes U6's floor clause true.
A separate `○ N queued` row is unnecessary and is rejected: one count, one row type,
one grammar.

Focus mode collapses the panel (`keymap.md` §5.2), so nothing is lost: the floor's
recommended state is focus mode.

---

## 8. Component states

`hover` does not exist — a terminal has no pointer-over (`keymap.md` §7).

| State | Drawn |
|---|---|
| `default` | R0/R1 rows as in §1.1 |
| `empty` | no items and no held tasks and no paused gate → **no block at all**, no heading, no blank. An empty block is not an empty-state panel; the board below already says `—`. |
| `overflowing` | R2 present, `N > 0` |
| `held` | R3 present, `count > 0` |
| `gate paused` | heading carries the word, no glyph (§6.5) |
| `unassigned` | a final `— unassigned` group (§3) |
| `error` | per-row via `✕`; the block has no error state of its own — a failed *load* is the panel's or the notice row's problem (`keymap.md` §4.1) |
| `loading` | does not exist. Per `keymap.md` §7, a panel with no data yet draws nothing and its title carries no counter. The block never draws a spinner: the state column already carries `Retrying`, and a second clock on one row is two clocks for one fact. |

---

## 9. What it must not do

- **No glyph in the overflow row**, and no word in it either (§5.1).
- **No status word on a header**, and no role on a header (§3).
- **No `▶` in the state column.** It is the employee's mark, and this panel draws it
  in R0 and in the board's head (`keymap.md` §2.7 rule 1).
- **No second glyph column for a delivery receipt.** When U12 lands, the receipt
  takes the **trailing 2 cells of the detail column** and the summary budget drops
  **16 → 14 on every row**, reserved unconditionally — a receipt on some rows and not
  others would leave a ragged right edge. Reserving it today would waste 2 of 30
  columns, so it is deferred, not dropped.
- **No prose in a state colour** (§4.3).
- **No dimming of terminal rows** (§4.3).
- **No blank row between item rows.** One gap before R3, one closing the block. A gap
  between every item would halve the density to buy nothing.
- **No row wider than 30**, and no row padded to fill it (§1).
- **No `0 paused` row, no `+0 more` row** (§6.2, §5.1).
- **No width change.** 32 stays 32.

---

## 10. Findings against the current tree (audit)

The writer's in-flight implementation followed rev 1 of this file; F1–F5 and F7–F10
have since landed in the working tree, and the list is kept as the record of what was
wrong and why. Each finding is a delta to `src/ui/tasks.rs`; none is a matter of taste.

| # | Where | Defect | Rule | State |
|---|---|---|---|---|
| **F1** | `queued_task_line`, `SUMMARY` | The status **word was missing** and the summary was 26 wide. Rev 1 bought 10 cells by dropping a frozen component; `keymap.md` §7 freezes the word and its §2.7 drops it only
below usable 7. Now `<glyph> <word 9> <summary 16>`. | §4.1 | **fixed** |
| **F2** | `employee_header`, `HEADER_NAME`/`HEADER_ROLE` | Budgets were 13/12, so the shipped role **`Research lead` (13 cells) was ellipsised** to `Research le…` — and the unit test asserted `truncate("researcher", 12)`, which is an *id*, not a role, so the test passed while the real row was cut. Now name **12**, role **13**. | §3 | **fixed** |
| **F3** | R0 = `employee_header(...)` | The group header was the board's head row verbatim, so `● Aria · Research lead` was drawn **twice in one panel**, 5 rows apart, and the block's header said nothing about the queue. Now `● Aria ×3` via `group_header`, carrying the true depth (`items.len()`, not rows drawn). | §3 | **fixed** |
| **F4** | `overflow_line` | `+N more` was drawn at indent **4**, i.e. inside the word column, which is reserved for frozen `keymap.md` §2.6 words. Now at `DETAIL_COL` (14). | §5.1 | **fixed** |
| **F5** | `held_line` | Count-first (`3 paused`) put the word at col 6 and broke the word column. Now `⏸ paused    ×N`, word at col 4, count in the detail column. | §6.1 | **fixed** |
| **F6** | `queue_block` (`remaining = budget`, then append) | The overflow row is **not reserved**: the budget is spent in full, then `+N more` is appended after the loop. Reproduced with a throwaway probe at 80×24 (5 items for one employee + 2 for another): heading 1 + group 6 + `+N` 1 = **8 counted rows against `A = 7`**. A *single* oversized group does not reproduce it — the `remaining >= 3` branch already subtracts the overflow row — so it only appears when one group fills the budget legitimately and the next is dropped whole. | §7.3 | **open** |
| **F7** | `render` | The block was followed by **no blank row**, so `⏸ paused ×3` sat directly above `● Aria · Research lead` and read as a queue row for Aria. Now one closing SPACE_1 row, outside `A`. | §7.3, §1.1 | **fixed** |
| **F8** | `queue_heading` | The heading was `" queue"` — indented 1 cell that no other row in the panel shares; R0 is at col 0. Now `queue`. | §1 | **fixed** |
| **F9** | `groups_hidden_total` | Dead helper (0 call sites), superseded by the `hidden` accumulator. Deleted — two sources for one count is exactly the disagreement §5.2 forbids. | §5.2 | **fixed** |
| **F10** | `header_budget_sums_to_thirty` | The test asserted `2 + HEADER_NAME + 3 + HEADER_NAME` — the name budget twice — and checked `truncate("researcher", …)`, an *id*. It would have passed with the F2 defect restored. Now `board_header_budget_sums_to_thirty`, asserting `2 + HEADER_NAME + 3 + BOARD_ROLE == USABLE` and that `"Research lead"` survives whole. | §3, §11.1 | **fixed** |

Not defects, recorded so they are not "fixed" later: the `⏸` in the heading is
dropped in favour of `engine paused` (§6.5) ✓; `SUMMARY`, `DETAIL_COL` and `WORD` are
derived constants rather than literals ✓; the summary is `text` on every row ✓;
`✓`/`✕` sharing the employee vocabulary is deliberate per `keymap.md` §2.6 ✓; the closing blank
is pushed unconditionally, including on the empty path it cannot reach ✓.

---

## 11. Check sheet for `@tester`

At **120×40, 100×30, 80×24, 60×16**:

1. No drawn row exceeds **30** cells and none wraps; the panel is 32 wide at every
   size. R1 measures exactly 30; R0/R2/R3 are shorter and unpadded.
2. Item glyphs sit at **col 2**, under the first character of the name above; item
   words start at **col 4**; summaries start at **col 14**.
3. No `▶` appears in the state column; R0's glyph is the employee's, R1's the task's.
4. A queue of 20 items at 100×30 draws ≤ 7 group/item rows and a `+N more` row with
   `N = 20 − drawn`; Σ(drawn items) + N = the true total (§5.2 A); the board below
   still shows employees.
5. The block never exceeds `A` counted rows (plus the one closing blank). The fixture
   that catches an overshoot is **two groups where the first fills the budget exactly
   and the second does not fit at all** — a single oversized group cannot, because the
   `remaining >= 3` branch already reserves the overflow row. Today 80×24 with
   `writer ×5` + `reviewer ×2` draws 8 against `A = 7` (F6).
6. At 60×16 the block draws heading + `+N more` + `⏸ paused ×N` + one blank, and
   **no** item rows; `N` equals the whole queue.
7. `count == 0` → no held row and no blank row. `N == 0` → no overflow row.
8. A paused task in the queue and the held row both draw `⏸`; the held row's word is
   `paused`, never `held`.
9. Exactly one blank row separates the block from the board, and no blank row sits
   between two item rows.
10. With `NO_COLOR`, every state is still distinguishable from glyph and word alone.
11. A group is never drawn as a header with no item under it.
12. `R0`'s `×N` equals the number of that employee's items **in total**, not the
    number drawn: with `+N more` present, `×N` must exceed the rows under it.

### 11.1 Three unit assertions the block must carry

These are cheap, and each catches a defect that already occurred once:

- **`board_header_budget_sums_to_thirty`** asserts `2 + HEADER_NAME + 3 + BOARD_ROLE ==
  USABLE` and that `truncate("Research lead", BOARD_ROLE)` returns the string
  **unchanged**. The original asserted the name budget twice and truncated
  `"researcher"` — an employee *id*, not a role — so it passed while the real role was
  cut (F10).
- **`every_row_fits_the_usable_width`** must include `group_header` and the board's
  `employee_header`, not only the queue rows: the header is where the F2 overflow was.
  `group_header_carries_the_depth_and_rows_align` also asserts that every status's
  summary starts at `DETAIL_COL` — the padded word column's whole purpose.
- **`the_block_never_outgrows_its_allowance`** must be driven by **two groups where the
  first exactly fills the group budget and the second does not fit at all**. A single
  oversized group cannot catch F6: with one group, `remaining >= 3` already subtracts
  the overflow row (`room = remaining - 2`), so the bug only appears when the budget is
  consumed *legitimately* and the overflow row is appended afterwards.

---

## 12. Open items this file does not decide

1. **Name duplication between the blocks.** R0 (`● Aria ×3`) and the board head
   (`● Aria · Research lead`) both name the employee. With F3 applied the two rows
   carry *different* facts, which is acceptable, but the stronger design is to merge
   the queue items under the board's own head row (head, items, activity — one name).
   That changes the panel's block structure and is Phase A's call.
2. **The board's height cap.** The board has no overflow row of its own, so at
   100×30 four employees need 12 rows against 9 available and the last one is
   clipped silently. Out of this block's scope; noted so it is not lost.
3. **`⏸`/`▷` at 2 cells in a CJK terminal** shifts the row by one. Accepted per
   `keymap.md` §2.7 rule 2; a 1-cell-only glyph set would be a different decision.
