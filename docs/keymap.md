# mywaker — keymap and visual specification (T0.2 → T0.3)

Frozen design spec for the **binding table** and the **visual language**: the
role-key palette, status semantics that never rely on colour, type/colour/spacing
tokens, the four breakpoints, and the focus-mode layout.

This is a **specification, not an implementation**. It states what the software
engineer builds against; it does not describe what the code does today, and a
mismatch is resolved by moving the code — except where §8 records a conflict with
another frozen file, which is resolved by amending that file first.

> **Status: frozen, rev 9 (T0.3). Two decisions taken by the user on 2026-10-21
> and unchanged here:**
>
> 1. **Palette model — lazygit-style role keys.** A small set of semantic role
>    keys, with dark/light override blocks, mapped to ANSI names. The *model* is
>    lazygit's; the **ANSI-only value syntax is mywaker's narrowing** — lazygit also
>    accepts hex (§2.2). OpenCode's semantic token tree is *not* ported: mywaker
>    renders through its own ratatui UI (§1.1), so that tree cannot be inherited —
>    only reimplemented.
> 2. **Colour depth — 16 ANSI only, degrade gracefully.** Truecolour is not a
>    requirement. Every status stays legible by glyph and word alone.
>
> **Revs 2–9 are a design review of rev 1 against the tree and against the frozen
> acceptance files.** They keep rev 1's structure and every externally cited
> section number, and change what the arithmetic or the code proves wrong. §0.2
> lists the thirteen design defects (D1–D13); §8 lists the six cross-file conflicts
> (C1–C6); §10 records the nine passes, including revs 6–9's evidence corrections.

Companion to [`acceptance-ux.md`](acceptance-ux.md) (U1–U12 and UX1–UX6),
[`acceptance-sidebar.md`](acceptance-sidebar.md) (the sidebar's frozen budget),
[`help-palette.md`](help-palette.md) (the two discovery surfaces),
[`verification-matrix-ux.md`](verification-matrix-ux.md) (the 48-cell matrix), and
[`architecture.md`](architecture.md) §5 (styling).

---

## 0. Tree pin

Every `file:line` citation was re-resolved against the working tree at rev 2:

- Base commit: `d0756f8`. Untracked on top of it: `docs/acceptance-ux.md`,
  `docs/help-palette.md`, `docs/keymap.md`, `docs/verification-matrix-ux.md`.
- Content at hand-off, `shasum` prefixes: `src/app.rs` `a95d056f`,
  `src/ui/mod.rs` `4402b0e5`, `src/ui/layout.rs` `c1fdd460`,
  `src/ui/status.rs` `5561c7ac`, `src/ui/pipeline.rs` `a39e722e`,
  `src/ui/sidebar.rs` `36dfd841`, `src/ui/hit.rs` `6f9bfb21`,
  `src/team.rs` `fc4d28ea`, `src/ui/prompt.rs` `afdef4be`.
- Measured counts on that tree: production `KeyCode::` literals are **75 lines /
  93 occurrences**, all in `src/app.rs`. `src/ui/prompt.rs` adds **4 / 4**, but
  every one is inside `#[cfg(test)] mod tests` (module at `:136`), so they are test
  code and are not counted as production. Totals including `src/app/tests.rs`
  (87 / 87): **166 lines / 184 occurrences**.
  `Color::` **220 occurrences across 12 files** in `src/ui/`. `Color::Rgb` and
  `Color::Indexed`: **0**.

If a citation does not resolve, compare those hashes first.

### 0.1 Evidence base

The conventions this spec adopts come from a comparison of three comparable TUIs,
gathered as the research input to T0.2: **lazygit v0.66.2**, **k9s v0.51.0**, and
**opencode v1.19.3**. The first two are cited at those releases. opencode is pinned
at **v1.19.3** (tag `e3c4eb2`) for its toast default and its abort semantics, and
cited at **v2.0.0+** for queueing, which does **not** exist in v1.19.3: that
release's TUI toast is a single `currentToast` slot with no queue and no `+N more`,
and queueing lands in **v2.0.0** (verified at **v2.1.1**, which is also the local
binary). Every row says which release it came from. The final row is this spec's
own decision, listed so the boundary between "measured" and "chosen" is visible — a
reader should be able to tell, without following a link, whether a rule was
observed somewhere or decided here.

| Source | Convention adopted | Where it lands |
|---|---|---|
| lazygit v0.66.2 | Role keys in `gui.theme`, with **hex or ANSI** values plus attributes; `colorScheme: auto` with `darkTheme`/`lightTheme` override blocks. The *model* is adopted; the **value syntax is narrowed to ANSI names** (§2.2) | §2.1, §2.3 |
| lazygit v0.66.2 | `?` is one global binding whose contents come from `Context().Current()` — help **for the focused context**, grouped Local / Global / Navigation, filter-as-you-type — not a global dump; the bottom line always shows hints. The **only** source of help-scoping in this spec: k9s's `?` is a fixed table, so it contributes nothing here (`help-palette.md` §0.1) | §3.6, `help-palette.md` §2 |
| lazygit v0.66.2 | Flash duration **differs by severity**: 2 s for info, 4 s for an error | §4.1 |
| k9s v0.51.0 | Flash line: centred single line, auto-clears at **6 s**; three levels with distinct colour **and** a non-colour channel. **Nothing about help scoping**: its `?` is a fixed 19-row table (one `RESOURCE` column, hardcoded `GENERAL`/`NAVIGATION`, no scroll/search/counter) and its scoped surface is the 6-row bottom `Menu` | §4.1, §4.2 |
| opencode v1.19.3 | Toast default **5000 ms**; **four variants** (`success`/`error`/`info`/`warning`) | §4.1 |
| opencode v1.19.3 | A user-initiated abort is not an error | §4.4 |
| opencode **v2.0.0+** (queueing; verified at v2.1.1) | Toast **queue** with `+N more`, and an optional **action button** per toast. **Not in v1.19.3**, whose TUI toast is a single `currentToast` slot with no queue | §4.2 |
| **mywaker** (this spec) | Transient-inline for recoverable, modal only for blocking; toast window **4–6 s, default 5 s**; palette role keys restricted to **ANSI names, no hex** | §2.2, §4.1 |

**The column that matters is the first one.** A row attributed to `lazygit v0.66.2`
or `opencode v1.19.3` can be checked against that release; a row attributed to
`opencode v2.0.0+` is checked against v2.1.1, the newest release available here and
the local binary's version. The design keeps both opencode conventions anyway —
they are good — but `acceptance-ux.md`'s pass conditions rest on the rendered
buffer and the registry, not on what any of these three projects does.

**The v1.19.3 "optional action" claim is about the web toast, not the TUI.** At
v1.19.3 the action button exists in the **web** toast component
(`packages/ui/src/components/toast.tsx`); the TUI toast does not render one. So the
per-toast action button is a v2.0.0+ TUI behaviour *and* a v1.19.3 web behaviour,
and citing v1.19.3 for it without naming the surface would be right about the
version and wrong about the program. §4.2 borrows the TUI queue, so it cites
v2.0.0+.

**Three claims rev 1 got wrong, and what replaced them.** They are recorded here
rather than silently corrected, because each one had been used to justify a
decision:

1. **opencode's toast queue, `+N more`, and per-toast action button are not in
   v1.19.3.** Queueing lands at **v2.0.0**; at v1.19.3 the TUI keeps one
   `currentToast` and has no queue, and its action button is web-only. The design
   keeps both conventions (they are good), but the attribution is now
   "opencode v2.0.0+", not "opencode v1.19.3" and not "master".
2. **There is no "all three picked 4–6 s".** lazygit does *not* sit in that range:
   it clears an info flash at **2 s** and an error at **4 s**. The band is
   mywaker's own choice (§4.1) — bounded below by lazygit's error window and above
   by k9s's flash — not a convergence, and rev 1 presented it as one.
3. **"ANSI names only" is mywaker's narrowing, not lazygit's rule.** lazygit accepts
   hex (and ANSI names) in `gui.theme`. §2.2 is a decision about *this* palette —
   16 colours, `NO_COLOR`-degradable — and rev 1 overstated it as an inherited
   convention.

**One convention deliberately rejected.** k9s makes `q` go *back* and `:q` quit.
mywaker's `q` quits today (`src/app.rs:1094`), and UX3 forbids removing a binding
in this change. `q` keeps quitting; the `Esc`-goes-back convention is adopted
instead — but see §3.6, where `Esc` needs an explicit precedence stack, because
the roster's `Esc` currently quits and rev 1 claimed both.

### 0.2 What rev 2 changed, and why

Each item is a defect in rev 1 or a place where the tree contradicts it. Nothing
here is a matter of taste; each cites a number or a line.

| # | Rev 1 said | Rev 2 says | Why |
|---|---|---|---|
| D1 | §2.3: "only `selected_bg` differs between the dark and light blocks … the 16-colour palette is already background-agnostic" | §2.3 overrides **eight** roles on light, and `ok`/`warn`/`info` collapse to the default foreground | Measured: `Gray` on `#f5f5f5` is **1.16:1**, `Cyan` 1.81, `Green` 1.98, `Yellow` 1.56. Rev 1's claim would ship an unreadable light theme |
| D2 | §2.1 implies every role colours text | §2.3 adds the **marker rule**: a role below 4.5:1 may colour a glyph, border, or ≤3-word label, never a sentence | `Red` on dark is **2.85:1** and the log band draws whole error sentences in it (`src/ui/mod.rs:241-243`) |
| D3 | §5.1: at 120×40 "cards drop the `waiting on` detail before the glyph" | The **marker** `waiting on` is required at 120×40 and 100×30; the target is ellipsised, and only 60×16 loses the marker | `acceptance-ux.md` §3/U6 keys the relaxation to card width < 12. At 120×40 the card is 19 — rev 1 dropped a required marker at the *largest* pinned size |
| D4 | §5.1 listed 160×50 inside the breakpoint table | 160×50 is a **ceiling reference**, not an axis | `acceptance-ux.md` §3 and the matrix both exclude it |
| D5 | §3.4 bound `f` in chat | chat uses **`Ctrl-F`**; `f` is roster-only | Chat's input is a text field: `f` types `f`. A hint for `f` there is a U3 failure, and U7's own wording ("`f` toggles") needs the amendment in C1(b) |
| D6 | §5.2 rule 6: "`Esc` restores" focus mode, and §3.6: "`Esc` dismisses, else back" | §3.6 adds an explicit **precedence stack**; `Esc` quits only on the roster with nothing else up | Today roster `Esc` quits (`src/app.rs:1094`); rev 1 asserted three meanings with no order |
| D7 | §3.2/§3.5 omitted `P`/`M` and the picker asymmetry | §3.8 records them as **undocumented aliases** and a spec-vs-tree mismatch | `src/app.rs:1105-1106` binds `P`/`M`; the models picker has no `j`/`k` (`:1062-1070`) while projects does (`:1046-1053`) |
| D8 | §4.2 toast actions were `r` / `c` / `?` | **`Ctrl-R` / `Ctrl-Y` / `Ctrl-O`**, with bare `r` as a roster-only alias; `c` is dropped | In chat every bare letter types. Rev 1 would advertise dead keys on the surface that most needs them; `Ctrl-Y` already exists and `c` duplicated it |
| D9 | §2.6 task statuses had one glyph | Every task, receipt, connection and error-class state has a **glyph, word and role** | UX1 is a triple for *every* status; rev 1 left five task states, four receipt states and three error classes word-only |
| D10 | §2.7 "the glyph is the state" globally | Glyph uniqueness is **surface-scoped**, and below `MIN_CELL_WIDTH` a card becomes a 1-row entry rather than a 3-wide box | `●` means idle on a card and connected in the title; at 60×16 a framed 3-wide card has a usable width of 1, so it spends 8 rows on one glyph |
| D11 | §6 had type tokens only | §6.3 adds a **spacing scale** (1/2/4), §6.4 a card **row grammar**, and the tool row loses its four class hues | "Use whitespace and alignment" is not implementable without numbers; four hues on the row under the status colour competed with the one signal that matters |
| D12 | — | §7 adds the **component state table** | A spec that names no states leaves default/hover/empty/loading/error to the implementer |
| D13 | §4.1 added a `toast: Rect` one line above the help bar | The toast shares the log band's pinned **notice row** with the callout and the fit report, with a frozen precedence | `acceptance-ux.md` §4.2 decided the notice row while rev 1 was being written; a second one-line band would be two hosts for one class of message, and a new `Regions` field risks U7's byte-identical rects |

---

## 1. Scope

- **§2** the palette: role keys, ANSI-only values, dark/light overrides, the
  marker rule, `NO_COLOR`, the status-semantics tables, and the narrow-glyph ladder.
- **§3** the keymap: every current binding → new binding, alias, deprecation, and
  the precedence stack.
- **§4** error and notice conventions (the surface T1.2's help and U11's taxonomy share).
- **§5** the four breakpoints, the degradation ladder, and the focus-mode layout.
- **§6** type, colour, and spacing tokens.
- **§7** component states.
- **§8** conflicts with the other frozen files.

Out of scope: implementing any of it (T1.1–T4.3), the registry's code shape (T1.1),
and any change to the sidebar's frozen line budget.

### 1.1 Why the token tree is not inherited

mywaker renders through **its own** ratatui UI: 15 modules under `src/ui/`,
`ratatui 0.30` + `crossterm 0.29` (`Cargo.toml:10-11`), and no reference to
OpenCode's TUI anywhere in `src/` or `Cargo.toml`. OpenCode's semantic token tree
lives in its own TypeScript TUI, so it cannot be imported — only reimplemented.
Porting it would mean owning a new subsystem (token tree, hue scales, truecolour
detection) for a TUI that needs eleven colours.

---

## 2. Palette — role keys (frozen)

### 2.1 The role keys

Eleven roles, plus one foreground (`on_accent`). Every colour the UI draws
resolves through one of them; a `Color::` literal outside the palette module is a
defect (§2.5).

| Role key | Meaning | ANSI default (dark) | Today's literal |
|---|---|---|---|
| `accent` | the one brand/active colour: titles, focused borders, the nameplate | `Cyan` | `Cyan` (51) |
| `border` | an unfocused panel's border | `DarkGray` | `DarkGray` (67) |
| `text` | body text | `Gray` | `Gray` (33) |
| `text_bright` | emphasised body text, the selected name | `White` | `White` (3) |
| `muted` | secondary text, roles, placeholders, disabled | `DarkGray` | `DarkGray` (shares `border`'s) |
| `selected_bg` | the selected row/card background | `Blue` | *none today* |
| `selected_fg` | text drawn **on** `selected_bg` | `White` | *none today* |
| `ok` | success, connected, done | `Green` | `Green` (17) |
| `warn` | in-progress caution, paused, retrying | `Yellow` | `Yellow` (26) |
| `err` | failure, disconnected, error | `Red` | `Red` (16) |
| `info` | neutral notice, delegating, trace | `Magenta` | `Magenta` (2) |
| `on_accent` | the ` mywaker ` title chip's foreground | `Black` | `Black` (4, all on `Cyan`) |

`Blue` appears **once** today — the MCP tool-class prefix
(`src/ui/pipeline.rs:954`) — and it is not a role: the tool row's four class hues
collapse into roles (§6.4), which is what makes the single-literal rule reachable
at all. The counts in the last column are occurrences in `src/ui/`, including the
in-file test fixtures (one of `Magenta`'s two is a test case at
`pipeline.rs:1607`); they are a measure of the migration, not of production use.

### 2.2 Values are ANSI names only — mywaker's narrowing

This is **mywaker's decision, not an inherited convention**. lazygit's `gui.theme`
accepts hex as well as ANSI names; §0.1 records that difference so nobody mistakes
the rule below for "what the comparable TUIs do". mywaker narrows to ANSI names
because it must survive `NO_COLOR` and a 1-bit terminal (§2.4), and a hex value has
no meaning to a terminal that cannot render one — so accepting hex would buy a
config surface whose values silently do nothing.

1. **No `Color::Rgb`.** Zero occurrences exist today and zero may be added.
2. **No `Color::Indexed`.** An index is terminal-dependent; a name is not.
3. **The palette is the only place a colour is named.** `status::color` and
   `status::task_color` (`src/ui/status.rs:30-38`, `:117-126`) become lookups into
   it, not their own tables.
4. **A role is never drawn as a background unless it is `selected_bg`.** Every
   other role is a foreground chosen against the terminal's own background.

If a future theme wants hex, that is a **new decision with a degradation story**
(what a hex becomes under `NO_COLOR`, and what it becomes on a 16-colour terminal),
not an extension of this one. The rule is deliberately the narrower of the two
models the evidence base offers.

### 2.3 Dark and light overrides — and the marker rule

A theme may override any role per scheme. mywaker ships **two** blocks and no more:

```toml
[ui.theme]
color_scheme = "auto"        # auto | dark | light

[ui.theme.dark]
selected_bg = "Blue"
selected_fg = "White"

[ui.theme.light]
text        = "Black"        # Gray is 1.16:1 on light — unusable
text_bright = "Black"        # + BOLD, which is what carries the emphasis
accent      = "Blue"         # the only hue that is legible on both
ok          = "Black"        # collapsed: no ANSI green is legible on light
warn        = "Black"        # collapsed
info        = "Black"        # collapsed
selected_bg = "Blue"
selected_fg = "White"
```

- `auto` selects the block from the terminal's background luminance. Until that
  detection exists, `auto` **resolves to `dark`** — the documented fallback.
- An override may only name a value from §2.2's allowed set. An unknown role or an
  RGB value is a **config error at load**, consistent with `deny_unknown_fields`.
- `border`, `muted`, `err`, and `on_accent` are **not** overridden: their dark
  values already clear the light background (`err` 5.36:1, `on_accent` 19.26:1) or
  are marker colours either way (`border`, `muted` 3.67:1).

**Why the light block is not just `selected_bg`.** Rev 1 claimed the 16-colour set
is background-agnostic. It is not. Measured against a `#f5f5f5` light background
(§2.8):

| Role | dark `#1e1e1e` | light `#f5f5f5` |
|---|---|---|
| `text` = `Gray` | 13.23:1 | **1.16:1** |
| `text_bright` = `White` | 16.67:1 | **1.09:1** |
| `accent` = `Cyan` | 8.43:1 | **1.81:1** |
| `ok` = `Green` | 7.73:1 | **1.98:1** |
| `warn` = `Yellow` | 9.79:1 | **1.56:1** |
| `err` = `Red` | **2.85:1** | 5.36:1 |
| `info` = `Magenta` | **3.55:1** | 4.30:1 |
| `muted` = `DarkGray` | **4.16:1** | **3.67:1** |

No ANSI green, yellow, or cyan reaches 4.5:1 on a light background; no ANSI
foreground other than `Black` (19.26:1) or `Blue` (8.62:1) is comfortably legible
there. So on light, `ok`/`warn`/`info` **collapse to the default foreground** and
the palette deliberately loses three hues. This costs nothing, because of §2.6:
the glyph and the word already carry every state. A theme that "keeps the colours"
on a light terminal is a defect, not a preference — it would be choosing five
unreadable hues over one readable screen.

**The marker rule (new in rev 2).** A role whose contrast is below 4.5:1 is a
**marker colour**: it may colour a glyph, a border, a 1-cell rule, or a label of at
most three words. It may **never** colour a sentence. On a dark terminal that
applies to `err` (2.85), `info` (3.55), and `muted` (4.16). Consequence: the log
band draws error **prose** in `text`, with `✕` and the word `error` in `err`
(`src/ui/mod.rs:241-243` draws the whole line in `Red` today — a defect).

`muted` is the same case: it is for metadata that the reader can afford to lose
(a role, a timestamp, a placeholder), never the only copy of a fact.

### 2.4 `NO_COLOR` and graceful degradation

| Condition | Behaviour |
|---|---|
| `NO_COLOR` set (any value, including empty) | **No foreground or background colour is emitted at all.** Structure survives on borders, glyphs, and words — `BOLD`/`DIM` are retained, because they are not colour |
| Terminal without 16-colour support | Same path as `NO_COLOR`: colour is dropped, structure kept |
| `NO_COLOR` unset | §2.1's defaults for the active scheme |

The requirement is **degradation, not detection**: the app never needs to know
whether the terminal is colour-capable. `NO_COLOR` handling does not exist in
`src/` today and is added by T4.1.

**The test for "graceful"** is `acceptance-ux.md` UX1: every status is a triple of
glyph, word, and colour, and each is sufficient alone. Because `BOLD`/`DIM`
survive, the nine style tokens of §6.1 keep their hierarchy with colour off —
which is the point of expressing hierarchy as *modifiers plus role*, never as hue
alone.

### 2.5 The single-literal rule

After T4.1, `grep -rn "Color::" src/ui` outside the palette module returns **0**
lines. Baseline: **220 occurrences across 12 files**. This is checkable and is the
palette's acceptance condition.

### 2.6 Status semantics — never colour alone (frozen)

The triple for every state. **No row may rely on its colour cell to be
distinguishable**, and no two rows may share both a glyph and a word.

**Employee status** (today: `src/ui/status.rs:30-58`, `src/team.rs:22-32`)

| Status | Glyph | Word | Role |
|---|---|---|---|
| `Idle` | `●` | `idle` | `muted` |
| `Thinking` | spinner (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) | `thinking` | `warn` |
| `Working` | `▶` | `working` | `accent` |
| `Done` | `✓` | `done` | `ok` |
| `Error` | `✕` | `error` | `err` |

**Task status** (today: `src/ui/status.rs:117-126`, `src/task.rs`) — glyphs added
in rev 2, because UX1 is a triple for every state, not just the employee's:

| Status | Glyph | Word | Role | Note |
|---|---|---|---|---|
| `Pending` | `○` | `pending` | `muted` | |
| `Running` | `▷` | `running` | `accent` | **not** `▶`: the tasks panel draws employee status in the same panel, and `▶` there means the *employee* is working |
| `Paused` | `⏸` | `paused` | `warn` | width-risky: tasks panel only, never a card (§2.7) |
| `Retrying` | `◐` | `retrying` | `warn` | distinguished from `paused` by **word** |
| `Succeeded` | `✓` | `succeeded` | `ok` | |
| `Failed` | `✕` | `failed` | `err` | |

`Paused` and `Retrying` deliberately share a colour: both are "not moving, not
failed", and the word carries the difference. Sharing a colour is permitted;
sharing a **word** is not. `Succeeded`/`Failed` do share a glyph with the
employee's `Done`/`Error`, and that is deliberate rather than sloppy: in the tasks
panel the queue section and the per-employee board are separate blocks with their
own headings, and a terminal state means the same thing in both.

**Connection** (new — U10)

| State | Glyph | Word | Role |
|---|---|---|---|
| connected | `●` | `connected` | `ok` |
| reconnecting | spinner | `reconnecting` | `warn` |
| down | `✕` | `down` | `err` |

**Error class** (new — U11)

| Class | Glyph | Word | Role |
|---|---|---|---|
| transient | `◌` | `transient` | `warn` |
| actionable | `◆` | `actionable` | `info` |
| fatal | `✕` | `fatal` | `err` |

**Delivery receipt** (new — U12)

| State | Glyph | Word | Role |
|---|---|---|---|
| queued | `○` | `queued` | `muted` |
| sent | `▹` | `sent` | `text` |
| acknowledged | `◐` | `acknowledged` | `info` |
| completed | `✓` | `completed` | `ok` |

The receipt reuses the task vocabulary rather than inventing four more glyphs: a
receipt *is* a lifecycle, and a second vocabulary would double the glyph inventory
for the same four states. The one glyph it does **not** reuse is `▶`, which is
already the card head's own `working` glyph: a receipt drawn beside it uses `▹`, so
one row never carries two arrows meaning two different things. The tasks panel shows
an employee's `▶` and a queued task's `▷` in the same panel for the same reason —
the subject differs, so the mark must.

### 2.7 The narrow-glyph ladder (revised)

A status surface **drops its detail, then its word, then its glyph, then
itself** — and dropping the surface is reported (§5.4). Colour is never the thing
that survives: it is the first thing dropped (§2.4).

Two rules govern which glyph may appear where:

1. **Uniqueness is surface-scoped.** `●` is `idle` on a card and `connected` in the
   title; `◆` is `actionable` in a toast and the active-section marker in the grid.
   That is allowed because the two never appear in the same surface. Within one
   surface, two states may not share a glyph.
2. **Width-risky glyphs stay off fixed-width surfaces.** `⏸`, `▹`, `▷`, `▸` and any
   emoji have terminal-dependent width (they are East-Asian *ambiguous*); they are
   drawn only in the tasks panel, the palette, and the transcript, never on a card.
   `⚠` is removed from composed text entirely (§4.6), because its width varies most
   of all. Card glyphs are limited to `● ○ ▶ ◐ ✓ ✕ ◆ ◇ ◌ × …`, each asserted to
   measure **1 column** with `unicode-width` (already a dependency,
   `Cargo.toml:12`).

**The receipt on a card is drawn only when it adds something.** `sent` has no card
glyph: the card's own `▶` already says work is moving, and a second arrow beside it
would be two marks for one fact — and `▹` is exactly the width-risky glyph rule 2
keeps off a card. So the head row carries `○` (queued), `◐` (acknowledged) or `✓`
(completed), and `sent` is the absence of a receipt glyph. The full four-step
receipt lives in the tasks panel, where `▹` is safe.

The card's own ladder, by **usable width** — `card − 2` while the card is framed,
`card` when it is frameless (§5.5). The head row carries the glyph, so the activity
row never repeats it:

| Usable | Head row | Activity row | Also |
|---|---|---|---|
| ≥ 21 | `▶ Aria ◐ ×2` | `waiting on researcher` | model + tool rows |
| 12–20 | `▶ Aria ◐ ×2` | `waiting on resea…` — marker intact, target ellipsised | model ellipsised, tool row |
| 10–11 | `▶ Aria ×2` | `waiting on` — the target drops, the marker does not | tool row only; the model drops first |
| 7–9 | `▶ Aria` | the status word (`working`) | nothing else |
| 4–6 | `▶ A…` | — (dropped) | nothing else |
| 3 | `▶ A` — one character of the name | — | frameless (§5.5) |
| 1–2 | `▶` | — | frameless |

`waiting on` is 10 columns and is the frozen marker (`acceptance-ux.md` §2, U6); it
therefore survives down to usable 10, and the **target is dropped before it is**. At
usable 11 — 100×30 — the marker fits exactly and the target cannot, which is why the
U6 relaxation does not need to reach that size (§8, C2).

### 2.8 Measured contrast

Reference values, `xterm`-style ANSI colours, WCAG 2.1 relative luminance. Dark
background `#1e1e1e`, light `#f5f5f5`. These are the numbers the palette was
chosen against; a different terminal will differ, which is why no state depends on
them.

| Foreground | on dark | on light |
|---|---|---|
| `Black` | — | 19.26 |
| `Gray` | 13.23 | 1.16 |
| `White` | 16.67 | 1.09 |
| `DarkGray` | 4.16 | 3.67 |
| `Blue` | 1.77 | 8.62 |
| `Cyan` | 8.43 | 1.81 |
| `Green` | 7.73 | 1.98 |
| `Yellow` | 9.79 | 1.56 |
| `Red` | 2.85 | 5.36 |
| `Magenta` | 3.55 | 4.30 |
| **`White` on `Blue`** (selection) | 9.40 | 9.40 |
| **`Black` on `Cyan`** (the chip) | 10.61 | 10.61 |

**Four pairs are close enough to the line to state explicitly.** `err` on dark
(2.85) and `muted` on dark (4.16) are below 4.5 and are **marker colours** only
(§2.3). `muted` on light (3.67) is below 4.5 in *both* schemes, which is why it is
the one role that is never load-bearing. And `White` on `LightBlue` (4.74) —
rev 1's light selection — is **replaced by `White` on `Blue`** (9.40), because it
cleared the line by 0.24 and would not survive a slightly warmer terminal theme.

---

## 3. Keymap — current → new, with aliases (frozen)

### 3.1 The rule

New bindings are **primary**; every binding that exists at hand-off keeps working
through **0.22** and is removed in **0.23** (UX3). The registry carries a
`deprecated_in` field so removal is mechanical, and help marks a deprecated binding
with `(deprecated)`.

Status vocabulary used in every table below:

| Status | Meaning |
|---|---|
| `current` | exists today, unchanged |
| `alias` | exists today as a secondary binding for the same action; stays through 0.22 |
| `redefined` | the key exists today but its action or scope changes |
| `new` | added by this work |
| `chord` | a `Ctrl-` binding that works **even while a text field has focus** |
| `deprecated` | documented, still works, removal scheduled in §3.7 |

Bindings added — **four new actions, six new keys, one roster-only alias, and one
redefined target**:

| Key | Action | Context | Rationale |
|---|---|---|---|
| `Ctrl-P` | command palette (T1.2) | every `Mode`; **not** while a text field has focus | `Ctrl-K` is the plan's key; both open the same surface |
| `Ctrl-K` | palette alias | as `Ctrl-P` | alias, not deprecated — two conventions, one surface |
| `f` | focus mode (U7) | roster, prompt **blurred** | frees the grid at 60–80 columns |
| `Ctrl-F` | focus mode (U7) | **roster and chat**, including while a text field has focus | the only form that works while typing (§3.4) |
| `Ctrl-R` | retry the bound error (U11) | global chord; bare `r` on the roster | the action the taxonomy names |
| `Ctrl-O` | error detail (U11) | global chord | opens the error panel (§4.3) |
| `Ctrl-Y` | copy the bound error, else the error log | global chord | already exists; its **target** is redefined |

That is six new keys for four new actions — `Ctrl-P`/`Ctrl-K` and `f`/`Ctrl-F` are
each one action with two forms, which is the alias pattern UX3 already sanctions,
applied to a chord instead of a letter.

`c` (rev 1's toast copy) is **dropped**: `Ctrl-Y` already copies, and two keys for
one action on the surface that appears during a failure is how a hint bar starts
lying.

### 3.2 Roster — unfocused (the default screen)

`src/app.rs:1092-1110`.

| Key | Action | Status |
|---|---|---|
| `↑` `↓` `←` `→` | move the grid selection | `current` |
| `h` `j` `k` `l` | move the grid selection | `alias` (deprecated 0.23) |
| `PgUp` `PgDn` | move a page of cards | `current` |
| `wheel` | scroll the grid (selection stays put) | `current` — **pointer**, not a key (§3.9) |
| `Enter` | open chat with the selected employee | `current` |
| `i` · `Tab` | focus the goal prompt | `current` |
| `p` | open the project picker | `current` |
| `m` | open the model picker | `current` |
| `P` · `M` | the same two pickers, uppercased | `alias`, undocumented today (§3.8) |
| `Space` | pause / resume the task engine | `current` |
| `s` | pause / resume the selected employee's task | `current` |
| `Ctrl-Y` | copy the error log (or the bound error) | `redefined` |
| `Ctrl-R` · `r` | retry the bound error | `new`, conditional on an `actionable` error |
| `Ctrl-O` | error detail | `new`, conditional |
| `?` | help, scoped to the roster | `current`, **redefined by T1.2** |
| `Ctrl-P` · `Ctrl-K` | command palette | `new` |
| `f` · `Ctrl-F` | toggle focus mode | `new` |
| `q` · `Q` | quit | `current` (`Q` is an `alias`) |
| `Esc` | quit — **only** when no overlay, palette, error panel, or focus mode is up | `current`, precedence fixed in §3.6 |

**The roster's hint bar, in priority order.** `help-palette.md` §4 owns the rule
(at most **five slots** for the focused region, with `? help` reserved inside that
budget). This is the one list the rule admits, so two implementers cannot pick
different five:

| Slot | Pair | Width | Running total |
|---|---|---|---|
| 5 (reserved) | `? help` | 8 | 8 |
| 1 | `↑↓←→ move` | 11 | 19 |
| 2 | `i goal` | 8 | 27 |
| 3 | `Enter chat` | 12 | 39 |
| 4 | `f focus` | 9 | 48 |

Widths are `1 + key + 1 + label + 1` per `help-palette.md` §4.2. Four pairs plus
the reserved escape hatch is **48 columns**, so **60×16 keeps the bar whole** and
nothing is ever clipped at a pinned size. `Space pause` is the first pair that does
not make the cut — not because of width but because the cap is five slots — and it
is reachable from help and the palette, which is exactly the trade U3 makes:
five honest hints over eleven clipped ones. Today's bar is 120 columns and loses
`? help` below 120 (B2); this one is 48 and loses nothing.

**`? help` is always drawn**, not only when a pair was dropped. `help-palette.md`
§4.1 rule 4 says it is "the last slot when anything was dropped" while §4.2 says it
is "reserved first, out of the budget before anything else is admitted"; the two
readings differ only in the case where nothing is dropped, and reserving it
unconditionally is the one that cannot fail U3(c) and cannot depend on arithmetic.
That is the reading frozen here.

### 3.3 Roster — prompt focused

`src/app.rs:1115-1141`.

| Key | Action | Status |
|---|---|---|
| `Enter` | send the goal | `current` |
| `Shift-Enter` | newline | `current` |
| `↑` `↓` | recall a previous goal | `current` |
| `Esc` · `Tab` | return to the roster keys | `current` |
| `Ctrl-W` · `Ctrl-U` | delete word / to line start | `current` |
| `Ctrl-A` · `Ctrl-E` | home / end | `current` |
| `Ctrl-H` · `Backspace` | delete backward | `current` (`^H` is backspace on some terminals) |
| `Home` `End` `Delete` `←` `→` | cursor movement / delete | `current` |
| `Ctrl-F` | toggle focus mode | `new` — the bar keeps the full width it already has; the grid behind it grows |
| `Ctrl-P` · `Ctrl-K` | **not bound here** | deliberate: line-editing keys in a readline shell |
| `Ctrl-R` · `Ctrl-O` · `Ctrl-Y` | error actions | `chord` — still live, because the bar is a text field |
| `?` | **types `?`** | `current`; `?` is not help while a text field has the keyboard (`help-palette.md` §2.1) |

**The one context where the palette is not bound.** In the goal prompt, `Ctrl-P`
and `Ctrl-K` are line-editing keys in common readline setups, so the prompt keeps
them unbound; the palette is reached by blurring with `Esc`. The models picker's
filter is the same case (§3.5). Chat is **not** in this exception: its history is
`↑`/`↓`, so `Ctrl-P` displaces nothing there, and excluding chat would make the
palette unreachable in a whole `Mode` (§8, C1a).

**`Ctrl-F` is the focus toggle here, not `f`.** The bar has the keyboard, so `f`
types an `f` — which is why the same action is bound twice rather than the spec
pretending a letter works in a text field (§8, C1b).

### 3.4 Chat

`src/app.rs:1246-1315`.

| Key | Action | Status |
|---|---|---|
| `Enter` | send, or complete the highlighted command when the menu is open | `current` |
| `Shift-Enter` | newline | `current` |
| `↑` `↓` | history, or the command menu when open | `current` |
| `PgUp` `PgDn` | scroll the transcript | `current` |
| `Esc` | dismiss the command menu, else close chat | `current` |
| `Ctrl-C` | interrupt the running generation (does **not** quit) | `current` |
| `Tab` | complete the highlighted command (a no-op when the menu is closed — never advertised) | `current` |
| `Ctrl-W` `Ctrl-U` `Ctrl-A` `Ctrl-E` `Ctrl-H` | line editing | `current` |
| `/help` `/stop` `/new` `/clear` `/skills` `/skill add` `/skill` `/parallel` `/trace` `/copy` | slash commands (10) | `current` |
| `Ctrl-P` · `Ctrl-K` | command palette | `new` |
| `Ctrl-F` | toggle focus mode | `new` |
| `Ctrl-R` · `Ctrl-O` · `Ctrl-Y` | error actions | `chord` |
| `f` | **types `f`** | `current` — rev 1 bound focus mode here, which cannot work; focus mode is `Ctrl-F` |
| `?` | **types `?`** | `current`; chat's discovery path is `/help` (`help-palette.md` §2.1) |

`/help` and the `?` overlay are generated from the same registry (U1), so the
slash command and the key cannot drift apart.

### 3.5 Pickers, the session picker, and the reopen prompt

**Projects** (`src/app.rs:1045-1057`) and **models** (`:1058-1087`):

| Key | Action | Status |
|---|---|---|
| `↑` `↓` · `k` `j` | move the selection | `current` for projects; **`k`/`j` are new for models** (§3.8) |
| `Enter` | select / assign | `current` |
| `Esc` | back (models also clears the filter) | `current` |
| `type` · `Backspace` · `Ctrl-H` | filter (models only) | `current` |
| `?` | **types `?`** in the filter, exactly as in the goal prompt | `current` (`help-palette.md` §2.1) |
| `Ctrl-U` | clear the filter (models only) | `current` |
| `n` | skip the session list, start fresh | `current` (session picker) |
| `Ctrl-P` · `Ctrl-K` | palette — **live** in the projects picker, **not** in the models filter | `new`, exception as §3.3 |
| `Ctrl-F` | **not bound here** — focus mode is a roster/chat flag, and a picker is neither (§5.2 rule 7) | deliberate |
| `Ctrl-R` · `Ctrl-O` · `Ctrl-Y` | error actions | `chord` |

**Session picker** (`src/app.rs:1982-2003`) — drawn over chat on open:

| Key | Action | Status |
|---|---|---|
| `↑` `↓` · `k` `j` | move the selection | `current` |
| `Enter` | start fresh (row 0) or resume the highlighted session | `current` |
| `n` | start fresh immediately | `current` |
| `Esc` | close the picker and leave the chat | `current` |
| everything else | swallowed | `current` |

**Reopen prompt** (`src/app.rs:1148-1169`) — drawn over everything on launch:

| Key | Action | Status |
|---|---|---|
| `y` · `Enter` | resume every shutdown-paused task | `current` |
| `n` · `Esc` | keep them paused | `current` |
| `q` | keep them paused and quit | `current` |
| `Ctrl-C` | keep them paused and quit | `current` |
| everything else | swallowed | `current` |

### 3.6 Global keys and the precedence stack

**Precedence, highest first.** A key is offered to each row in turn and the first
row that claims it wins. This table replaces rev 1's three contradictory claims
about `Esc` (D6).

| # | Surface | Keys it claims | Everything else |
|---|---|---|---|
| 1 | session picker | §3.5 | swallowed |
| 2 | reopen prompt | §3.5 | swallowed |
| 3 | error panel | `Esc` · `Ctrl-O` close; `Ctrl-Y` copy | swallowed |
| 4 | help overlay | `↑↓` `PgUp` `PgDn` `/` `a` `Esc` · `?` · `q` | **dismisses without acting** |
| 5 | palette | `help-palette.md` §3.6 | swallowed |
| 6 | focus mode, `Esc` | restores the resting layout, exactly | falls through |
| 7 | the mode's own keys | §3.2–§3.5 | falls through |
| 8 | global chords | `Ctrl-C` · `Ctrl-P` · `Ctrl-K` · `Ctrl-F` · `Ctrl-R` · `Ctrl-O` · `Ctrl-Y` | — |

`Ctrl-F` is in row 8 so that a **text field** cannot swallow it — that is the whole
reason it exists alongside `f`. It is a chord of `Mode::Roster` and `Mode::Chat`
only, because focus mode is a flag on those two modes (§5.2 rule 7) and a picker is
neither; the hint bar must therefore not advertise it in a picker (U3a). Inside
focus mode `Ctrl-F` toggles back, and `Esc` (row 6) is a second path to the same
restore, not a competing meaning.

Consequences, stated so they cannot be re-litigated:

1. **`Esc` quits only on the roster with nothing else up.** Inside focus mode it
   restores; inside an overlay it dismisses; in chat it closes the chat. The
   roster's `Esc`-quit survives because UX3 forbids removing it, and it is now the
   *last* meaning rather than a competing one.
2. **A modal swallows.** No key reaches the screen under an open modal, so a
   keypress meant for the overlay can never act beneath it (today any key
   dismisses the help overlay, `src/app.rs:1000-1001`).
3. **`Ctrl-C` quits everywhere except chat**, where it interrupts. It is below the
   modals, so it cannot quit out from under an unanswered reopen prompt.
4. **`Ctrl-R`/`Ctrl-O`/`Ctrl-Y` are chords**: they work while a text field has
   focus, which is what makes the toast's actions honest (§4.2).

### 3.7 Deprecation register

| Binding | State in 0.22 | Removed in | Replacement |
|---|---|---|---|
| `h j k l` (roster) | works, marked alias | 0.23 | arrow keys |
| `Q` (roster) | works, marked alias | 0.23 | `q` |
| `P` `M` (roster) | works, marked alias | 0.23 | `p` `m` |
| `r` (bare, error retry) | works on the roster only | 0.23 | `Ctrl-R` |
| `Ctrl-K` | works, **not** deprecated | not scheduled | `Ctrl-P` (both are first-class) |
| `Ctrl-Y` (copy log) | works | not scheduled | none — kept, target widened |

Nothing else is deprecated. **No binding is removed in this change.**

### 3.8 As-built mismatches the writer fixes

Spec-vs-tree gaps found while writing rev 2. Each is small; none is optional.

| # | Where | Today | Spec |
|---|---|---|---|
| K1 | models picker, `src/app.rs:1062-1070` | `↑`/`↓` only | add `k`/`j`, matching projects (`:1046-1053`) |
| K2 | roster, `src/app.rs:1105-1106` | `P`/`M` bound, absent from README and help | record as `alias` (§3.2), show `(deprecated)` in help |
| K3 | `?` and `q · Esc`, `src/app.rs:1017` and help's first row (`src/ui/mod.rs:436`) | `?` is global except chat, but help says only "this help overlay"; and the first row reads `q · Esc — quit (any screen)`, which the precedence stack (§3.6) makes false — `Esc` restores focus mode and dismisses overlays before it quits. Nothing says that `?` is *not* help in a text field, so the overlay's own row implies it works there | say `?` is `roster · chat via /help`, and that it types `?` in a text field; split the row into `q quit (any screen)` and `Esc back · restore` |
| K4 | hint bar, `src/ui/mod.rs:331-345` | 11 pairs, one of them `wheel` | ≤5 pairs, all of them keys; pointer hints are a separate kind (§3.9) |
| K5 | help overlay, `src/ui/mod.rs:423-471` | 35 body rows in a fixed `68×30` popup, one of them `click · wheel`; any key dismisses it (`src/app.rs:999-1003`) | registry-generated, context-scoped, scrollable, with pointer rows in their own section; `Esc` dismisses without side effects (`help-palette.md` §2) |
| K6 | chat `Tab`, `src/app.rs:1306` | no-op with no menu | never advertised; still bound |
| K7 | log band, `src/ui/mod.rs:241-243` | error prose in `Red` (2.85:1) | prose in `text`, `✕` + word in `err` (§2.3) |
| K8 | list highlight, `src/ui/projects.rs:88`, `models.rs:118`, `chat.rs:767`, `:837` | `bg(DarkGray)` with the item's own foreground: as low as **3.18:1** where that foreground is `Gray` | `selected_bg` **and** `selected_fg` (9.40:1) |
| K9 | `f` in chat | claimed by rev 1 | `Ctrl-F` (D5) |
| K10 | toast/error actions | no bindings exist | `Ctrl-R` / `Ctrl-Y` / `Ctrl-O` (§3.6) |
| K11 | `README.md` Controls table (`README.md:208-227`) | 13 rows; no `f`, `Ctrl-F`, `Ctrl-P`, `Ctrl-K`, `Ctrl-R`, `Ctrl-O`, no `P`/`M`, and `?` is described as unconditional | add the six new keys and the two aliases, and scope `?` to non-chat. UX6 makes a binding present in one doc and absent in another a **defect in the docs**, so this is part of the same change, not a follow-up |

### 3.9 Pointer hints are not keys

`wheel`, `click`, and `double-click` appear in today's hint bar and help
(`src/ui/mod.rs:335`, `:438`). They are **not** `KeyCode`s, so a registry whose
`keys` field is `&[KeyBinding]` cannot hold them, and U3's "every advertised key is
bound" cannot be satisfied by a bar that mixes the two.

Frozen: the registry carries a second, separate list, `pointers: &[PointerHint]`,
and the hint bar admits pointer hints **after** every key hint — a pointer hint is
never the thing that displaces `? help`.

---

## 4. Error and notice conventions (frozen)

### 4.1 The three tiers

| Tier | Surface | Duration | Requires ack | Used for |
|---|---|---|---|---|
| **status** | the log band's dim line | 2 s | no | routine progress (`resumed <title>`) |
| **toast** | the log band's **notice row** (below) | **4–6 s** (default **5 s**) | no | recoverable failures, and any `transient` class |
| **modal** | centred box, `Clear`d | until answered | yes | destructive or unrecoverable — `fatal` only |

**Where the numbers come from — three sources, three different answers.** Rev 1
claimed all three conventions converged on 4–6 s. They do not:

| Source | Measured | Where it sits |
|---|---|---|
| lazygit v0.66.2 | **2 s** for an info flash, **4 s** for an error | its *error* window is our lower bound; its info window is below it |
| k9s v0.51.0 | **6 s** for a flash line | our upper bound |
| opencode v1.19.3 | **5000 ms** toast default (four variants) | inside the range |

So **5 s is mywaker's own choice, not a convergence.** The band is defined by the
two measured extremes that carry a failure: **4 s** is lazygit's error window and
the shortest time we are willing to give a user to reach `Ctrl-R`; **6 s** is k9s's
flash and the longest a non-blocking notice may occupy a row. 5 s happens to be
opencode's default, and it is the default here because it sits inside that band —
not because three projects agreed, and not because opencode's reasoning was ours to
inherit.

Two consequences are stated rather than left implicit:

- **lazygit's info flash (2 s) is deliberately below our floor.** A 2-second
  message is a status line, not a toast, and §4.1 already has a status tier for
  exactly that: routine progress belongs in the dim log line, where its short life
  costs nothing.
- **lazygit's *severity-dependent* duration is not adopted.** A toast here carries
  its class as a word and a glyph, so a user who looks up late must still find the
  same failure. Making the window depend on severity would mean an `actionable`
  error vanishes faster than a `fatal` one — the opposite of what the taxonomy is
  for.

**The placement rule — mywaker's decision, not a cited convention.** No source in
the evidence base states this rule, so it is recorded as a judgement rather than an
inheritance: if the user would be *annoyed* to acknowledge it, it is a toast; if it
is *fine for them to miss it*, it is a status line. Only a `fatal` class earns a
modal. The three sources all *practise* the distinction — each has a transient
inline surface and a blocking one — but none of them writes the criterion down, so
the sentence above is this spec's, and it is the one a reviewer should argue with.

**Where the toast draws: the notice row, not a new band.** The roster's log band
gains **one** notice row — a pinned content row, below the pinned error — and that
row is the host for every one-line notice about the roster: a degraded-state
callout (U5), a `hidden N` fit report (U9), and a live toast (U11). This adopts
[`acceptance-ux.md`](acceptance-ux.md) §4.2's decision rather than re-litigating
it, and it is better than rev 1's "one line above the help bar" for three reasons:

- **No new `Regions` field**, so `main` never moves: the frozen invariant that the
  prompt band must not shrink the panels (`src/ui/layout.rs:210-231`, test
  `the_roster_prompt_band_comes_out_of_the_log`) is untouched, and U7's
  byte-identical rect restoration holds by construction.
- **The log is already the band that gives way** (`src/ui/layout.rs:34-38`) and
  already pins its most important message first (`src/ui/mod.rs:233-249`). The
  notice row generalises a convention that already exists.
- **A healthy app with nothing to say consumes no row**, so the band is exactly
  what it is today.

Rev 1's objection — "the log is history, and a failure the user must act on cannot
scroll away" — does not apply to a *pinned* row. The row never scrolls; only the
tail below it does.

**Precedence on that one row, strict:**

1. **a live toast** (U11) — a failure with an action outranks a hint;
2. **the degraded-state callout** (U5) — blocking and actionable;
3. **the fit report `hidden N`** (U9) — appended when it fits.

A lower item is **dropped whole**, never clipped (U3b's rule), and returns when the
row above it clears. A toast expiring restores whatever it displaced. The **pinned
error is a separate row above** the notice row and is never displaced by any of the
three, so no notice can push a failure out of the band — that is
`acceptance-ux.md` §4.2's precedence, adopted unchanged.

**One row of contention, stated.** While the selected employee is thinking the band
is 3 rows tall — two borders and **one** content row (`src/ui/layout.rs:41-45`) — so
the pinned error and the notice row cannot both be drawn. The pinned error wins, the
notice is dropped whole, and it comes back when thinking ends. That is the correct
direction: a failure the user is looking at must never be traded for a hint, and a
toast that expired while suppressed is not re-shown (its class, cause and action are
already in the log, and `Ctrl-O` still opens the panel).

`acceptance-ux.md` §4.2 owns the widths; §5.5 records where the floor puts it.

### 4.2 Toast anatomy

```
✕ delivery to Aria failed · actionable · Ctrl-R retry · Ctrl-Y copy · Ctrl-O detail
```

Four parts, in order, each optional except the first two:

1. **glyph** — from §2.6's class table (`◌` `◆` `✕`)
2. **class word** — `transient` · `actionable` · `fatal`
3. **cause** — one line, truncated with `…`, never wrapped
4. **bound actions** — only those **live in the current context** (§3.6). While a
   text field has focus, only chords qualify, which is why the actions are chords
   and not letters (D8)

A toast carries **at most three** actions. If a fourth would be needed, the surface
is wrong: it belongs in the error panel.

**Degradation, left to right, whole units.** At a narrow width the cause is
ellipsised first, then the actions drop from the right, then the class word. The
glyph and the class word are the last things to go; if both cannot fit, the toast
is not drawn and the pinned error row keeps the failure. A toast never ends
mid-word and never truncates an action label.

**What each pinned width actually shows**, since the notice row is `terminal − 2`
columns and a toast is long. The units drop right to left:

| Size | usable | drawn | dropped |
|---|---|---|---|
| 120×40 | 118 | the full 83-column toast, all three actions | — |
| 100×30 | 98 | the full 83-column toast, all three actions | — |
| 80×24 | 78 | `✕ delivery to Aria failed · actionable · Ctrl-R retry · Ctrl-Y copy` (67) | `Ctrl-O detail` |
| 60×16 | 58 | `✕ delivery to Aria failed · actionable · Ctrl-R retry` (53) | `Ctrl-Y copy`, `Ctrl-O detail` |

At the floor the class word and **one** action survive, which is the minimum that
makes the toast actionable rather than merely alarming. `Ctrl-R` is the action kept
because it is the one the taxonomy names for both `transient` and `actionable`; the
error panel is still reachable by `Ctrl-O` from anywhere, so dropping its *hint*
never drops the path.

**Queueing — one row, so one toast at a time.** The notice row shows the **newest**
toast; anything behind it waits. The row's last slot reads `+N more` when N others
are waiting, so a burst cannot push the screen off and cannot hide its own depth.

This borrows opencode's `+N more` **queueing** convention, with the attribution
stated precisely: that queue (and the per-toast action button it pairs with) is
**v2.0.0+** — verified at **v2.1.1**, which is also the local binary — and it does
**not** exist in v1.19.3, whose TUI toast is a single `currentToast` slot with no
queue and no `+N more`. At v1.19.3 the optional action button is the **web** toast
(`packages/ui/src/components/toast.tsx`), not the TUI one. So this is a design
reference to a later release, not a property of the pinned tag (§0.1). It is
borrowed because a bounded count is better than either alternative: silently
dropping the fourth failure hides a problem, and stacking four rows over the grid
costs the user the screen. Applied to a single-row host it reads as "one toast,
plus how many are behind it", which is exactly what the row can say. Rev 1's "3
visible" was a consequence of its own
three-row toast band, and that band does not exist.

A toast that repeats an identical `(class, cause)` within its own lifetime is **not**
re-shown; the visible one's count increments (`✕ delivery failed ×3`), so a retry
loop reads as one failure getting worse rather than as a wall of identical lines.

### 4.3 The error panel

`Ctrl-O` on a toast, or any `fatal`, opens the error panel: a centred box listing
the error's **stable id**, class, cause, the upstream detail verbatim, and the
trace id when one exists. Dismissible (`Esc`, `Ctrl-O`) and copyable (`Ctrl-Y`).
This is where a stack or a long upstream message lives, so the toast never wraps.

**The stable id** is a dotted literal — `delivery.failed`, `session.create_failed`,
`store.save_failed`, `team.save_failed`, `opencode.unavailable` — derived from the
call site, not from the message text. All 9 `push_error_log` sites must carry one.

### 4.4 The abort filter

A **user-initiated abort is not an error.** `Ctrl-C` in chat, a `/stop`, and a
paused task's interrupt produce no toast, no error class, and no `✕`. They are
status lines at most. Treating the user's own deliberate stop as a failure trains
them to ignore the error surface.

### 4.5 Class assignment rules

| Class | Test | Default action |
|---|---|---|
| `transient` | retrying the same operation may succeed, and the app does so automatically | none (auto-retried); `Ctrl-R` offered after the last attempt |
| `actionable` | the user must do something — assign a model, pick a project, install a skill, reconnect | `Ctrl-R` when retryable, `Ctrl-Y` always |
| `fatal` | continuing would corrupt state or silently lose work | modal, `Ctrl-Y`, `Ctrl-O` |

The 9 existing sites map as: `OpenCode unavailable`, `session failed`, and
`delivery failed` → `actionable`; `store save failed` and `could not save team`
(×2) → `fatal`; `<name> failed` and `LogError` → classified at the call site,
never defaulted. **No site may ship unclassified** (U11).

### 4.6 Glyph hygiene in message text

The app composes emoji into log and transcript strings today: `👁` (`src/app.rs:962`),
`⚡` (`:2346`, `:2353`, `:2375`, `:2400`, `:2417`), `📋` (`:2505`, `:2507`),
`🔧` (`:2866`), `📩` (`:2952`), `🤖` (`:3172`), `🔁` (`:3436`, `:3676`), and `⚠`
(`src/harness.rs:117-118`). Emoji width is terminal-dependent, so each one can push
a wrapped log line out of its band — and the log band is the one place where a
wrapped line costs a fact.

Frozen replacements, all drawn from §6.2:

| Today | Becomes | Why |
|---|---|---|
| `⚡ /new` | `» /new` | a command being echoed is a quotation, not a state |
| `🔧 bash` | `» bash` | a tool call is a quotation too; the tool row already has the class word |
| `📩` (delegation) | `→` | a hand-off is a direction |
| `🤖 @jev:` | `◆ @jev:` | Jev is the active answerer (§6.2) |
| `👁 watched task` | `◇ watched task` | a watch is an *inactive* trigger until it fires |
| `🔁 retrying` | `◐ retrying` | the retrying glyph (§2.6) |
| `📋 copied` | `✓ copied` | a successful copy is `ok` |
| `⚠ no deliverable` | `! no deliverable` | ASCII, deliberately — see below |

`!` is the one replacement that is not from §6.2, and the exception is recorded
rather than glossed: `⚠` is the glyph whose rendered width varies most between
fonts, and a warning marker that draws two cells breaks every column count in the
log. `!` is 1 column in every terminal. If a later pass wants a prettier mark, it
must be a glyph `unicode-width` reports as 1.

---

## 5. Breakpoints and the focus-mode layout

### 5.1 The four breakpoints

Four sizes are pinned as matrix axes, in `acceptance-ux.md` §3. Geometry below is
read from the renderer's own seams, not recomputed by hand.

| Size | main | grid | card | usable | cols | Design intent |
|---|---|---|---|---|---|---|
| **120×40** | 64×32 | 64 | 19 | 17 | 3 | every surface at rest; `waiting on` fits, the target is ellipsised |
| **100×30** | 44×22 | 44 | 13 | 11 | 3 | `waiting on` fits exactly; the target and the model id are ellipsised (§2.7) |
| **80×24** | 24×16 | 24 | 23 | 21 | 1 | grid falls to one column, which makes the card **wider** than at 120×40 — the fallback trades columns for legibility, and this is the only unfocused size where `waiting on researcher` fits whole |
| **60×16** | 4×8 | 4 | 3 | 3 (frameless) | 1 | a card becomes a **row** here (§5.5): **7 rows** of `<glyph> <name>` instead of one boxed glyph; **focus mode is the recommended state** |
| *160×50* | *104×42* | *104* | *33* | *31* | *3* | **ceiling reference only — not an axis** (D4). Drawn by the smoke tests so no criterion can be satisfied by a layout that only works when cramped |

**The transition rule**, so behaviour *between* the four is defined rather than
left to whoever reads the table:

- `grid = terminal.width − 24 − 32` (sidebar + tasks, `layout.rs:68-75`).
- `card = (grid − (3·1 + 2·1)) / 3` while `card ≥ 12`; otherwise `1` column of
  `grid − 1` (`pipeline.rs:295-315`, `MIN_CELL_WIDTH = 12`).
- `main.height = terminal.height − 1 − 2 − 4 − 1` on the roster (title + prompt +
  log + help), minus 1 more from the log while the selected employee is thinking.
- A card row is 7 rows (`CONTENT_LINES 5 + 2`), plus 1 for its section heading and
  1 for the gap above it.
- **Below `MIN_CELL_WIDTH` a card is 1 row, not 7** (§5.5). The threshold is the
  same one that drops three columns to one, so a size never has both a narrow card
  and a framed card.

There are no other special cases. A size between two breakpoints gets the layout
those three formulas produce — which is why 100×30 and 120×40 both keep three
columns and 80×24 does not.

### 5.2 Focus mode (U7)

**Purpose:** at 60–80 columns the roster's `24 | fill | 32` split leaves the grid
4–24 columns. Focus mode gives the grid (or chat) the full width.

**Layout when focused** (roster):

```
┌ mywaker ──────────────────────────────────── focus mode · f to restore ┐
│                                                                        │
│   the grid, full width (or the chat transcript, full width)            │
│                                                                        │
├────────────────────────────────────────────────────────────────────────┤
│ > goal                                                                  │
├────────────────────────────────────────────────────────────────────────┤
│ log (4 rows, unchanged)                                                 │
└ ? help  f restore ──────────────────────────────────────────────────────┘
```

**Geometry per breakpoint.** The bands are unchanged; only `main`'s horizontal
split collapses, so focus mode is a change to one layout function and nothing else.

| Size | focused `main` | grid | card | usable | cols | card-row capacity |
|---|---|---|---|---|---|---|
| 120×40 | 120×32 | 120 | 38 | 36 | 3 | 3 |
| 100×30 | 100×22 | 100 | 31 | 29 | 3 | 2 |
| 80×24 | 80×16 | 80 | 25 | 23 | 3 | 1 |
| 60×16 | 60×8 | 60 | 18 | 16 | 3 | 1 (no heading, no strip, no bus) |

Read from `pipeline::shape`'s own arithmetic: `below = main − 1`, then the strip
and the bus each cost a row only when they do not cost a card row
(`pipeline.rs:340-352`), then a card row is 7 (+1 heading, +1 gap). The capacity
column is the **panel's** capacity, computed for a 12-employee roster with three of
them active (five card rows at 160×50); a smaller roster draws fewer rows and the
capacity is unchanged, which is what makes it a property of the breakpoint rather
than of the team.

The 60×16 row is the point of the feature, and it is a **trade rather than a gain
in count**: unfocused at that size the grid draws **7 one-line entries** (§5.5) and
focus mode draws **3 full cards** — three employees, each with its name, status
word, `waiting on <target>`, model id and tool row, instead of seven glyphs with one
letter of a name. The Jev strip and the delegation bus are dropped there because
keeping either would cost the only card row (`pipeline.rs:340-352`); the heading
goes too, for the same reason.

**Focus mode also un-floors the card.** §5.5's 1-row frameless entry is keyed to
`card < MIN_CELL_WIDTH`, so at 60×16 focused the card is 18 wide and comes back as a
full 7-row framed card. A criterion that is undecidable unfocused at that size —
U6's queue count, U12's receipt glyph — is decidable **in focus mode**, which is
exactly why §5.2 calls focus mode "the recommended state" at the floor rather than a
nicety: it is the only state at 60×16 where the card is a card.

Frozen rules:

1. **The sidebar and tasks panel collapse to zero width** — they are not squeezed.
   Two panels squeezed to 6 columns each is worse than two panels absent.
2. **The bands between `main` and the help bar are unchanged**: the prompt bar is
   never hidden (U7), because it is how work is started, and the log keeps its 4
   rows. A failure the user must read is not worth three more card rows.
3. **The title reports `focus mode`** — this is U7's marker, drawn in `meta` style
   after the project name.
4. **The help bar advertises `f restore`** — or `Ctrl-F restore` when the prompt has
   the keyboard, so the exit is always visible and always live.
5. **Remembered per mode**: roster-focus and chat-focus are independent flags.
6. **`Esc` restores**, and restoration is *exact*: the panel rects are byte-identical
   to the pre-focus frame. This is asserted, not approximated.
7. **Focus mode is not a new `Mode`.** It is a flag that changes the geometry inside
   `Mode::Roster` / `Mode::Chat`, so `Mode`-based persistence and every existing
   mode-scoped binding keep working (UX4).
8. **Hit-testing reads the same flag.** `ui::hit::target_at` resolves against
   `layout::Regions::of` + `roster_areas`; both must take the focus flag, or a click
   lands on the card that *would* have been there. This is the one implementation
   requirement that is easy to miss and fails silently.

Rule 7 is the important one: a fifth `Mode` variant would fork every
`match self.mode` and every persistence path. A flag changes one layout function.
`help-palette.md` §3.6 cites rule 7 by number for exactly this reason, so the
numbering is part of the contract and a rule is merged rather than inserted.

### 5.3 Chat in focus mode

The transcript takes the full width; the input bar, the log, the hint bar, and the
title are unchanged. There is no third layout: chat is already `main` + a 4-row
input, so focus mode only widens `main`. `Ctrl-F` is the toggle (D5), and `f`
types.

### 5.4 The degradation ladder (frozen)

Nothing is dropped silently, and the order is the same everywhere:

| Step | What goes | Trigger | Reported? |
|---|---|---|---|
| 1 | **colour** | `NO_COLOR`, or a 1-bit terminal | no — the triple covers it |
| 2 | **decoration** — the progress bar, the delegation bus, the Jev strip, section headings | the band or row would cost a card row | no — the information is elsewhere |
| 3 | **detail** — the waiting target, the event age, the model id, the tool name | the row's usable width | no — it is ellipsised with `…` |
| 4 | **word** — a status word, a class word, a receipt word | usable width < 7 (§2.7) | no — the glyph carries it |
| 5 | **glyph** | usable width < 1 — i.e. the node itself is 0 wide | no — the surface is already gone |
| 6 | **the surface** — a panel, the toast, the title counter | the frame cannot hold it | **yes** — `hidden N` (U9) or a `+N more` (toasts) |

Steps 2 and 3 are the ones rev 1 left implicit, and they are where most of the
60×16 behaviour lives: a heading is decoration, a waiting target is detail, and
both go before a word does.

### 5.5 The floor: what 60×16 unfocused actually draws

At 60×16 the grid panel is **4 columns**, of which a bordered card spends 2 on its
side walls. One 3-wide bordered card therefore has a **usable width of 1**, and its
7 rows are **every row the panel has below its title** — the whole grid, one glyph,
one of four employees.

Frozen for the floor: **below `MIN_CELL_WIDTH` a card becomes a row.** The node
keeps the same `Rect` *width* (so `grid_cells` and hit-testing read the same
function) but its height drops from `STAGE_HEIGHT` (7) to **1**, it draws no frame,
and it takes **no `ROW_GAP`** — adjacent text rows do not fuse the way adjacent
borders do, so the gap that exists to separate boxes is not needed between lines.
One employee per row: `<glyph> <name>`, truncated to the node's width.

At 60×16 that is **7 employees visible instead of 1** (the grid's 7 rows below its
title), each as `▶ A`, `● B` — usable width 3 instead of 1, so the glyph and one
character of the name survive together, where a framed card had room for the glyph
alone. This is the whole degradation ladder of §5.4 applied at once: decoration (the
frame, the bar, the headings, the gaps) goes first, then detail, and the
glyph-plus-name row is what is left.

The **section headings** cannot survive here for a reason that is not about cost:
`◆ working · 2` is 14 columns against a 4-column panel, and an ellipsised `◆ w…`
labels nothing. They are dropped whole, as §5.4 step 2 requires.

Two things this deliberately does not buy:

- **A title counter at the floor.** The grid title is 4 columns and renders `tea`
  today; `hidden N` is 8. For the default four-employee roster nothing is hidden at
  60×16 — all four rows fit — so U9 is satisfied *vacuously* there. For a roster
  larger than the seven rows the floor shows, the count does not fit in the title,
  so it moves to the **notice row** (below). U9 is therefore decidable at 60×16 for
  any roster: the title above 24 columns, the notice row below it.
- **No queue count, and no receipt glyph.** `▶ ×2` is 4 cells and `▶ ◐` is 4 against
  a usable width of 3, so U6's "glyph **and** count" clause and U12's "on the card"
  clause cannot hold unfocused at this size (§8, C3, C5). The tasks panel (32
  columns) carries both, and focus mode brings the card back (§5.2).

A card-shaped criterion is therefore not fully decidable at 60×16 unfocused. That
is not a relaxation of the criterion: it is a statement that the floor has rows
where other sizes have cards, and the matrix already records this size as "every
surface at its floor". Because it *is* a change to drawn geometry, it is registered
as a conflict (C6) rather than assumed: the matrix's measured `card 3 / cols 1` row
and `acceptance-ux.md` §3's card-width column describe the current 7-row card, and
both need the one-line amendment.

**The notice row.** At the floor, prose *about* the roster has no host in the grid,
so it is drawn on the log band's pinned **notice row**, below the pinned error: one
line carrying the degraded-state callout (U5), a live toast (U11), and, when the
title cannot hold it, `hidden N` (U9). It adds no `Regions` field — `main` never
moves, so U7's rect restoration is untouched — and a healthy app with nothing to say
draws no notice row. [`acceptance-ux.md`](acceptance-ux.md) §4.2 owns the rule and
the widths; this section only records where the floor puts it. This is what unblocks
U5 and U9 at 60×16 (C4).

---

## 6. Type, colour, and spacing tokens

There is no font scale in a TUI: the type system is the **style triple**, the
**row grammar**, and the **spacing scale**.

### 6.1 The style tokens

| Token | Style | Used for |
|---|---|---|
| `title` | `accent` + `BOLD` | panel titles, the nameplate |
| `heading` | `warn` + `BOLD` | section headings in help and panels |
| `body` | `text` | ordinary text |
| `emphasis` | `text_bright` + `BOLD` | the selected employee's name |
| `meta` | `muted` + `DIM` | roles, attribution, telemetry |
| `key` | `accent` + `BOLD` | a key in help and the hint bar |
| `rule` | `border` | an unfocused border, the bus, a separator |
| `selected` | `selected_fg` on `selected_bg` + `BOLD` | the selected row in a list |
| `on_accent` | `on_accent` on `accent` | the ` mywaker ` title chip |

`BOLD` and `DIM` survive `NO_COLOR`; only the colour component drops (§2.4). That
is why hierarchy is never expressed by hue alone: with colour off, `title`,
`heading`, and `key` are still bold, `meta` is still dim, and `body` is still plain.

**Four weights, no more.** A TUI has plain, `BOLD`, `DIM`, and (undrawn) nothing
else. `ITALIC` is not used: terminal support is inconsistent and it survives
neither `NO_COLOR` nor a plain font.

### 6.2 The glyph set

Frozen, because these are what make status legible without colour. Every glyph
drawn **on a card** is asserted to be **1 column** (§2.7 rule 2); `⏸` is the one
entry that is not, and it is confined to the tasks panel for that reason.

| Glyph | Meaning | Where |
|---|---|---|
| `●` | idle / connected / present | card head, title |
| `○` | pending / queued | tasks panel, card head |
| `▶` | working (an employee) | card head, tasks panel |
| `▷` | running (a task) — distinct from an employee working | tasks panel only |
| `◐` | retrying / acknowledged | tasks panel, card head |
| `✓` | done / succeeded / completed | card head, tasks panel |
| `✕` | error / failed / down / fatal | card head, toast, log marker |
| `⏸` | paused | **tasks panel only** — the one width-risky glyph that is kept |
| `◆` | active / actionable | section heading, toast, palette kind |
| `◇` | inactive / idle section / not present | section heading, palette kind |
| `◌` | transient | toast, log marker |
| `!` | warning marker | log, transcript |
| `×` | queue depth (`×2`) | card head |
| `…` | truncation | every truncated row |
| `▹` | sent (a receipt that has left the hand) | tasks panel only — the card's `▶` already means the same thing |
| `▸` | the selected-row gutter | palette, lists |
| `»` | a quoted command or tool call | log, transcript |
| `→` | a hand-off, a direction | transcript |
| `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` | thinking / connecting / reconnecting | card head, title, thinking row |
| `·` | separator | titles, hint bar |
| `┄` | the delegation bus | grid |
| `█` `░` | the card's progress bar | card activity row |
| `│ ┌ ┐ └ ┘ ─ ├ ┤` | frames | panels, modals |
| `↑ ↓ ← →` | arrows in hints | help, hint bar |

**No emoji, and no `⚠`.** Emoji width is terminal-dependent and would break the
fixed-width card geometry; `⚠` is the one symbol whose width varies most between
fonts (§4.6). Both are replaced, not tolerated. `⏸` survives because it is the only
paused marker that reads as "paused" at a glance, and it never touches a card.

### 6.3 The spacing scale

Four values. There is no fifth.

| Token | Cells | Where it is the *only* legal gap |
|---|---|---|
| `SPACE_0` | 0 | inside a border; a card's content is flush with the border's inner edge |
| `SPACE_1` | 1 | the gap between cards (`COL_GAP`), between card rows (`ROW_GAP`), every border, and the single row the notice occupies inside the log band |
| `SPACE_2` | 2 | the prompt band's height (`PROMPT_HEIGHT`) |
| `SPACE_4` | 4 | the log band's height |

A 3-row or 5-row band is a defect, not a preference. **Cards get no inner
padding**: at 13 columns, padding would cost 2 of the 11 usable cells, and
alignment against the border reads cleaner than an inset box.

**Adjacent panels share one wall.** `roster_areas` gives the sidebar, grid, and
tasks panel no gap, so today each draws its own border and every seam is a
double rule `││`. Frozen: the grid draws `Borders::TOP | Borders::BOTTOM` only;
the sidebar's right border and the tasks panel's left border are its side walls.
Zero columns change, the rects stay identical (so hit-testing is untouched), and
the seam becomes a single hairline. A frame containing `││` at a panel seam is a
defect.

### 6.4 The card row grammar

A card is 7 rows: two border rows and five content rows (`CONTENT_LINES = 5`),
always all five, so cards line up. Each row has one job, one token, and one
truncation rule.

| Row | Content | Token | At narrow width |
|---|---|---|---|
| head | `<glyph> <name>` + `<receipt glyph>` + `×<queue>` | `emphasis` for the name | name first, then the receipt, then the count, then the name itself (§2.7) |
| role | the employee's role | `meta` | ellipsised; dropped below usable 8 |
| activity | `<word>`, or `waiting on <target>`, or `stalled`; the bar when nothing else needs the row | the status role | §2.7's ladder — the glyph is already in the head, so this row never repeats it |
| model | the model id, or `none` | `accent`, or `meta` for `none` | ellipsised from the right (the provider survives longest) |
| tool | `tool <name>`, with the class as a word rather than a hue | `meta` for the whole row | the class word drops whole before the name (`pipeline.rs:899`) |

**Five slots, always emitted.** A row whose content does not fit is drawn empty
rather than omitted, so the card's height never depends on what it happens to be
saying and a row of cards always lines up. That is what makes `STAGE_HEIGHT` a
constant and the grid's row arithmetic (§5.1) exact.

**The tool row loses its four class hues.** Today `skill:` is `Cyan`, `subagent:`
`Magenta`, `mcp:` `Blue`, `plugin:` `Yellow` (`pipeline.rs:950-958`) — four colours
on the one row that sits directly under the status colour, competing with the only
signal that matters. The class is already spelled out in the prefix; a hue adds
nothing a reader can name. Frozen: the prefix is `meta`, the name is `text`, and
the card has exactly **one** coloured row (activity). This is the restraint rule
applied to a real regression: one accent, not five.

The **section heading** above the first card row of each section is
`<glyph> <word> · <count>` in the section's role, and it is decoration: it is the
first thing dropped when it would cost a card row.

---

## 7. Component states

What each surface looks like in each state. A state not listed does not exist —
`hover` is not one, because a terminal has no pointer-over.

| Component | States |
|---|---|
| **card** | `default` (border `border`, title `DIM`) · `selected` (border `accent` + `BOLD`, title `BOLD`) · `busy` (status role border, animated bar) · `waiting` (activity row = `waiting on <target>`) · `stalled` (activity row = `stalled`, `warn`) · `error` (border `err`, head glyph `✕`) · `no model` (model row = `none`, `meta`) · **`floor`** (below `MIN_CELL_WIDTH`: one frameless row, `<glyph> <name>`, §5.5) |
| **panel** | `unfocused` (border `border`, title `title`) · `focused` (border `accent` + `BOLD`) · `empty` (the grid at its floor draws `◇`; the callout naming **one** action goes on the notice row, U5) · `overflowing` (title carries `rows a-b/c · hidden N`, or the notice row carries `hidden N` when the title cannot, U9) |
| **list row** | `default` (`body`) · `selected` (`selected` + `▸` gutter) · `disabled` (`meta`) |
| **prompt bar** | `blurred` (border `border`, no caret) · `focused` (border `accent`, caret) · `thinking` (one extra row, `thinking_line`) · `empty` (`> ` and the caret only) |
| **hint bar** | `full` (5 slots: four pairs + `? help`, 48 columns) · `reduced` (whole pairs dropped from the right, `? help` never) · `minimal` (only `? help`, when the terminal is narrower than 8) |
| **toast** | `transient` (`◌`) · `actionable` (`◆`) · `fatal` (`✕`, never auto-dismisses) · `queued` (`+N more`) · `repeated` (`×3`) |
| **notice row** | `silent` (nothing to say, no row drawn) · `callout` (U5) · `toast` (U11) · `fit` (U9) — in the precedence order of §4.1 |
| **task row** (tasks panel) | `default` — `<glyph> <status word padded to 9> <summary>`; the glyph is what today's word-only row (`src/ui/tasks.rs:83-92`) lacks, and padding the word to 9 keeps the summaries in one column across `pending`/`retrying`/`succeeded` |
| **palette row** | `default` · `selected` (`selected` + `▸`) · `no match` (one line) |
| **help row** | `default` · `heading` · `deprecated` (label + `(deprecated)`) · `searching` (the counter shows the filtered total) |

**Loading** is never a spinner on a surface that has content: the card's own
status glyph *is* the loading state (`Thinking`), because a second spinner would
mean two clocks on one card. A panel with no data yet draws nothing and its title
carries no counter — an empty panel is not a loading panel.

### 7.1 What it must not do

- **No colour-only distinction** anywhere (§2.6), and no colour on prose when the
  role is below 4.5:1 (§2.3).
- **No emoji, no `⚠`, no glyph of unknown width** (§6.2), and no glyph above 1
  column on a card.
- **No second accent.** `accent` is one role; a screen with two competing "active"
  hues has none.
- **No border around a single glyph**, and no box that spends more cells on its
  frame than on its content (§5.5).
- **No inner padding on cards**, no 3-row band, no gap of 5 (§6.3), and no
  `ROW_GAP` between two floor rows — the gap separates *boxes*, and the floor draws
  none.
- **No double rule at a panel seam** (§6.3).
- **No gradient, shadow, or blur** — a terminal has none, and a TUI that imitates
  one spends cells to look worse.
- **No hint for a key that is not live** in the current context (U3), and no hint
  for a pointer action in the key list (§3.9).
- **No silent drop**: anything hidden is counted (§5.4, step 6).

---

## 8. Conflicts with the frozen files

Rev 2 was written against `acceptance-ux.md` and the matrix, and found six places
where this spec and those files cannot both be true. Each needs a one-line
amendment in the file that owns it — **not** a silent resolution here, because
`acceptance-ux.md` §7 rule 1 forbids weakening a criterion to make a build pass,
and rule 3 forbids this file from reaching into the sidebar's budget.

**None of the six changes a pass condition.** C1 names an action more precisely,
C2–C3 and C5 relocate a *host* or state a width threshold, C4 is already closed, and
C6 corrects a measurement. That is the test each one had to pass to be written down
at all: if a conflict could only be resolved by making a criterion easier to
satisfy, it would be a defect in this spec, not in the other file.

| # | Files | Conflict | Recommended resolution |
|---|---|---|---|
| **C1** | `acceptance-ux.md` U2 (and its §6 test `u2_palette_is_unbound_while_typing`), U7 | **Two keys, one problem: a bare letter named in a mode that types.** (a) U2's exception is written as "the goal prompt **or** a picker's filter" — an enumeration, so chat is excluded — but the generic phrase "while a text field has the keyboard" invites the opposite reading, and a literal reading would make the palette unreachable in `Mode::Chat` and fail `u2_palette_opens_from_every_mode`. (b) U7 binds **`f`**, which in chat types `f`, so "`f` toggles focus mode" is unassertable there. | One amendment to U2: state that **chat is not in the exception** — its history is `↑`/`↓`, so `Ctrl-P` displaces nothing. One to U7: `f` on the roster and **`Ctrl-F`** in chat and while the prompt has the keyboard (both forms in the registry, one action). No pass condition moves; the criteria name the same action, more precisely |
| **C2** | `acceptance-ux.md` U6 @ 100×30 | "`waiting on <target>` … must be ellipsised, never dropped" cannot hold: the card is 13 wide, usable 11, and `waiting on` alone is 10 — the target cannot be shown even ellipsised to one character. | Amend to: the **marker** `waiting on` is required; the target is required only when the usable width exceeds it. This is §2.7's ladder, and it keeps the marker assertable at every size |
| **C3** | `acceptance-ux.md` U6 @ 60×16; matrix §4.2 | "Glyph **and** queue-depth count" needs 4 cells. The floor's entry is `<glyph> <name>` in 3. | Amend to: at the floor the **glyph** is required and the count is carried by the tasks panel. Pair it with C4 |
| **C4** | matrix §4.1 (U5, U9 blocked), §3 | The matrix recorded two blocked cells at 60×16 and called the rest decidable, but the floor has **rows, not cards** (§5.5), so every card-shaped clause is drawn differently there. | **Resolved 2026-10-21** by `acceptance-ux.md` §4.2 and §5.5's notice row: U5 and U9 are no longer blocked at 60×16 — the callout and `hidden N` are drawn on the log band's first content row (58 usable columns against a 34-column callout). U6 and U12 remain card-shaped and are carried by the tasks panel at the floor (C3, C5) |
| **C5** | `verification-matrix-ux.md` U12 @ 60×16 | "the card carrying the glyph" is impossible: a receipt glyph would make the entry 4 cells wide against 3 (§5.5). | Amend to: the tasks panel carries the receipt at the floor; the card carries it from 100×30 up (usable 11) |
| **C6** | `verification-matrix-ux.md` §1 and §3 (the `card` / `cols` columns); `acceptance-ux.md` §3's card-width column | Both tables measure the **framed 7-row card** and report `card 3, cols 1` at 60×16. §5.5 makes that card a **1-row, frameless entry** whose usable width is 3, so the measured column describes a rectangle nothing draws any more. | Amend both tables' 60×16 row to `card 3 · rows 1 · cols 1` and add the note "frameless at the floor, §5.5". This is a measurement correction, not a criterion change — no pass condition moves |

**What this file will not do.** It does not edit those files. C1–C6 are the
amendment list for whoever owns them, and until they land the matrix's 60×16 column
is the place a reviewer should expect a false pass.

**C4 is the one already closed.** `acceptance-ux.md` §4.2 decided it on
2026-10-21 (the notice row), and §4.1 of this file adopts that decision rather than
keeping rev 1's separate toast band. It stays in the table because the *reason* it
was blocked — the grid cannot host prose at the floor — is what makes C3, C5 and C6
necessary, and a reader who finds one should find the others.

---

## 9. Open items

1. **`color_scheme = "auto"` has no luminance detection yet** (§2.3). It resolves
   to `dark` until T4.1 adds detection. Recorded so the fallback is a decision,
   not a bug.
2. **The error panel's exact height** is not pinned; it grows to content, clamped
   to the terminal. T3.2 owns it.
3. **`Ctrl-P` in the goal prompt and the models filter** is unbound by design
   (§3.3, §3.5). If user testing shows the palette is expected there, that is a
   spec change recorded in §10 — and it would collide with readline history-up,
   which mywaker does not implement but a user's muscle memory does.
4. **`selected_bg` and `selected_fg` are the only roles without a current
   literal** — they are new, so there is no "today" value to match.
5. **`◐` and `○` are untested against real fonts.** They are Geometric Shapes and
   should be 1 column, but the width assertion (§2.7 rule 2) is the check, and it
   does not run until T1.1 adds the palette module.
6. **The notice row costs one log line at rest and the whole log while the selected
   employee is thinking** (the band is 2 content rows, 1 while thinking —
   `src/ui/layout.rs:41-45`). The precedence is frozen in `acceptance-ux.md` §4.2:
   pinned error, then notice, then telemetry and recent lines, and while thinking
   the pinned error wins outright (§4.1). If that proves too costly in use, the
   answer is a taller log band, not a callout inside the grid — which is
   unsatisfiable at the floor by construction.
7. **The floor's 1-row card is the biggest behaviour change in this spec** (§5.5)
   and it is a *geometry* change, not a colour or copy one: `grid_cells`,
   `tests/grid.rs`'s click-to-card assertions, and the matrix's measured `card`
   column all read the same function. C6 is the amendment that must land with it,
   and `tests/grid.rs` must keep passing unchanged — if it does not, the row card
   has broken the seam it was supposed to leave alone.

---

## 10. Revision log

| Rev | Date | Change |
|---|---|---|
| 1 | 2026-10-21 | Created for T0.2 from the version-pinned TUI conventions (lazygit v0.66.2, k9s v0.51.0, opencode v1.19.3). Records the user's two decisions: lazygit-style role keys (§2) and 16 ANSI with graceful degradation (§2.4). Freezes the ten role keys, the single-literal rule, dark/light overrides, the full status-semantics triple table, the current→new keymap with a deprecation register, the three-tier error model with toast anatomy and the abort filter, the four breakpoints, and the focus-mode layout as a flag rather than a fifth `Mode`. |
| 2 | 2026-10-21 | **T0.3 design review of rev 1 against the tree and the frozen acceptance files.** Thirteen defects fixed, each numbered in §0.2: the light theme's contrast failure (D1) and the marker rule that follows from it (D2); `waiting on` restored at 120×40 (D3); 160×50 demoted to a ceiling reference (D4); focus mode moved off `f` in chat (D5); `Esc` given an explicit precedence stack (D6); `P`/`M` and the picker asymmetry recorded (D7); toast actions moved to chords with `c` dropped (D8); glyphs for every task state (D9); surface-scoped glyph uniqueness and a 1-row floor card (D10); a spacing scale, a card row grammar, and the tool row losing its four class hues (D11); a component-state table (D12); the toast folded into the notice row (D13). Adds §2.8's measured contrast, §3.8's eleven as-built mismatches, §3.9's separation of pointer hints from keys, §4.6's emoji replacement table, §5.4's six-step degradation ladder, §5.5's floor design, §7's states and prohibitions, and §8's cross-file conflicts with recommended amendments. Every section number cited by `help-palette.md`, `acceptance-ux.md`, and `verification-matrix-ux.md` is preserved. |
| 3 | 2026-10-21 | **The 60×16 host question is answered (was open item C4).** U5 and U9 at the floor were blocked because the grid panel is 4 columns and a card is 3, so neither a callout nor `hidden N` can be drawn inside the grid. The fix is the **notice row**: the log band's pinned row carries the degraded-state callout and, where the grid title can no longer hold it, the `hidden N` fit report. §5.5's "a title counter at the floor" bullet no longer says U9 stays blocked — it says the count moves. C4 is marked resolved; U6 and U12 stay card-shaped and are carried by the tasks panel at the floor (C3, C5 unchanged). §6.4's `panel` states are amended to match. The authoritative rule is `acceptance-ux.md` §4.2, which owns it; this file records only where the floor puts it. No binding changed. |
| 4 | 2026-10-21 | **The toast joins the notice row, and the floor card becomes a row.** §4.1 no longer adds a `toast: Rect`: rev 1's separate one-line band above the help bar is replaced by the same pinned log row that carries the callout and the fit report, with a frozen precedence (toast, callout, fit) under the pinned error. Queueing follows from one row — newest shown, `+N more`, identical repeats counted — and §4.2 now states what each pinned width actually draws (83/83/67/53 columns). §5.5 makes a card **1 row and frameless** below `MIN_CELL_WIDTH` (7 employees visible at 60×16 instead of 1, usable width 3 instead of 1) and registers the measurement change as **C6** against the matrix's `card`/`cols` columns. §2.7's ladder is re-keyed to *usable* width so the frameless case is expressible, and §7 gains the `floor` card state and the `notice row` component. §8's C4 is annotated as the one conflict already closed by `acceptance-ux.md` §4.2. |
| 5 | 2026-10-21 | **Final consistency pass over the whole file.** Adds the roster's **hint-bar table** (§3.2) with measured widths — five slots, 48 columns, so the bar is whole at 60×16 and `? help` is reserved unconditionally, resolving the one place `help-palette.md` §4.1 and §4.2 read differently. Fixes five cross-references that had drifted: `Ctrl-F` is a **roster/chat** chord and not bound in a picker (§3.1, §3.6, §5.2); the log band's *thinking* case, where the pinned error and the notice row contend for one content row (§4.1); the `? help`/hint-bar and `tool`-row line numbers; the light-theme override count (eight roles, not five); and the help overlay's row count (§3.8, K3, K5). Splits three glyphs that would otherwise collide **inside one surface** — `▷` for a queued task's `running` (the tasks panel also draws an employee's `▶`), `▹` for a receipt's `sent` (drawn beside a card head's `▶`), `▸` kept for the selection gutter — and adds §6.2's `»`/`→`, which §4.6 had introduced without listing. Extends §3.8 with K11 (the README Controls table must gain the new keys — UX6 makes doc drift a defect) and §7 with the tasks panel's row grammar. §2.7's ladder gains the usable-3 and usable-1 rows so the floor is expressible in the same table as every other width. |
| 6 | 2026-10-21 | **Evidence correction. No design decision changes.** Three citations were wrong and each had been used to justify a decision, so each is corrected and the correction is recorded rather than folded in silently. (a) **§0.1**: opencode's toast queue, `+N more`, and per-toast action button are **master only**, post-v1.19.3 — the row is relabelled `opencode master (post-v1.19.3)` and v1.19.3 keeps only the 5000 ms default and the abort filter. (b) **§0.1**: the "all three picked 4–6 s" row is **deleted** — lazygit clears an info flash at 2 s and an error at 4 s, so there was no convergence; §4.1 now carries a three-source table with the measured values, and states that **5 s is mywaker's choice** inside a band bounded below by lazygit's error window (4 s) and above by k9s's flash (6 s), with lazygit's severity-dependent window explicitly rejected and why. (c) **§0.1 and §2.2**: "ANSI names only" is **mywaker's narrowing**, not lazygit's rule — lazygit accepts hex — so §2.2 now says so, gives the `NO_COLOR`/1-bit reason for the narrowing, and records that accepting hex would be a new decision with its own degradation story. Also: §0.1's table gains a `mywaker (this spec)` row so "measured" and "chosen" are visibly separate; §4.1's placement rule is re-attributed from "the conventions' explicit criterion" to **mywaker's decision**, because none of the three sources writes that criterion down; and a duplicated "one convention deliberately rejected" paragraph from rev 5 is removed. Design decisions — 4–6 s default 5 s, `+N more`, the 16-colour palette — are unchanged. |
| 7 | 2026-10-21 | **Help-scoping attribution corrected — lazygit only.** §0.1's lazygit row now states the mechanism (one global binding, contents from `Context().Current()`, Local/Global/Navigation, filter-as-you-type) and the k9s row says it contributes **nothing about help scoping**: k9s v0.51.0's `?` is a fixed 19-row table (one scoped `RESOURCE` column, hardcoded `GENERAL` 19 and `NAVIGATION` 11, no scroll/search/counter) and its scoped surface is the 6-row bottom `Menu`. The full argument, the side-by-side table, and the two explicit conventions now live in `help-palette.md` §0.1 and §2.1: that **"`global` first" is mywaker's inversion** (both sources put the scoped section first), and that **`?` is not help while a text input is focused**. No binding, geometry, or criterion changed. |
| 8 | 2026-10-21 | **§0.2's `KeyCode::` counts corrected — production is one file.** The line read "**79 by line** (75 `src/app.rs`, 4 `src/ui/prompt.rs`), **97 by occurrence**; 166/184 including `src/app/tests.rs`", which counted the four `src/ui/prompt.rs` literals as production. They are all inside `#[cfg(test)] mod tests` (module at `:136`; literals at `:157`, `:202`, `:232`, `:282`), so production is **75 lines / 93 occurrences, all `src/app.rs`**, and `prompt.rs` contributes **4 / 4** test-only. Totals including `src/app/tests.rs` (87 / 87) are unchanged at **166 / 184**. The `Color::` figures in the same paragraph were re-checked and hold exactly: **220 occurrences across 12 files** in `src/ui/`, `Color::Rgb` and `Color::Indexed` **0**. This matches [`acceptance-ux.md`](acceptance-ux.md) §0 B4 (its rev 7) and [`help-palette.md`](help-palette.md) §0.2 B4 (its rev 4), so all three files now agree. No binding, breakpoint, token, or decision changed. |
| 9 | 2026-10-21 | **A pinned-tag attribution corrected — not a design change.** §0.1 credited opencode **v1.19.3** with the toast **queue** and `+N more`. Verified at tag `v1.19.3` (`e3c4eb2`): that release's TUI toast is a **single `currentToast` slot — no queue, no `+N more`**; queueing lands at **v2.0.0** (verified at **v2.1.1**, which is also the local binary). The opencode rows are now split: v1.19.3 keeps **"Toast default 5000 ms; four variants"** and the abort semantics, while queue + `+N more` + per-toast action move to **opencode v2.0.0+ (verified at v2.1.1)** with the absence in v1.19.3 stated in the row itself. §0.1 also now states that the **"optional action" at v1.19.3 is the web toast** (`packages/ui/src/components/toast.tsx`), not the TUI one — the version was right and the *program* was wrong. The "master" framing is retired everywhere: a branch is not checkable, a release is. §4.2's borrowing paragraph carries the same attribution, and its §4.1 source table names the four variants. No binding, breakpoint, token, tier, or decision changed — the design still borrows the queue, it is simply attributed to the release that has it. |
