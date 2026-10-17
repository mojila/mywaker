# Releasing mywaker

A release is two things: **one `chore: bump version to X.Y.Z` commit**, and **one
annotated tag `vX.Y.Z` pointing at that commit**. The version lives in exactly one
place — `Cargo.toml` — and everything else derives it (`make` reads it with `sed`;
the `packaging/*` templates carry `@VERSION@`), so a release cannot drift from what
was built.

Validated on [`v0.11.0`](#worked-example-v0110).

---

## 1. Decide the version

SemVer, read for a pre-1.0 project:

| What shipped | Bump |
|---|---|
| A user-visible feature, or a deliberate change to existing behaviour | **minor** (`0.x.0`) |
| Bug fixes only | **patch** (`0.x.y`) |
| `refactor:` / `docs:` / `chore:` only — nothing a user can notice | **no release** |

Rules that keep that honest:

- A deliberate behaviour change is a feature, not a fix: state it in the commit
  body and let it drive a minor bump. (This is why the help overlay gaining a line
  shipped as `feat:`, and why the refactor chain before it bumped nothing.)
- The bump is its own commit, never folded into a feature commit, so the version
  history stays one line per release:
  `git log --oneline --grep='^chore: bump version'`. That has been the practice
  since `0.10.0`; the `0.7`–`0.9` releases folded the version into a feature
  subject (`feat: … (v0.9.0)`) — don't go back to that. (Note that
  `git log -- Cargo.toml` is *not* the same list: dependency edits touch it too.)
- Subjects are `type: imperative` (`feat:`, `fix:`, `refactor:`, `docs:`,
  `chore:`) with a prose body that stands on its own — the log *is* the release
  notes' source. (`2caa808 fix cursor` is the one legacy subject that doesn't;
  don't add more.)

Start from the work itself: `git log --oneline vPREV..HEAD` is the candidate list.

## 2. Verify the tree

Run all of these on the commit you are about to bump. Each one must be clean.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
cargo build --release
```

Expected at the time of writing (`0.11.0` + the unreleased mouse feature):

- `cargo test` — **190 pass, 0 fail, 3 ignored**: `src/lib.rs` 179,
  `tests/opencode_fake.rs` 5, `tests/render.rs` 5, `tests/watch_hook.rs` 1, plus
  `src/main.rs` and doc-tests 0. The three ignored are the ones that need a live
  OpenCode service (`opencode::tests::live_smoke_*`,
  `tests/live_employees.rs` — the last also wants inference budget); they are run
  by hand, not as part of a release. The counts grow every release — what has to
  hold is **0 failed** and **only the live tests ignored**.
- `cargo doc --no-deps` — exactly **3** warnings, all deliberate: `ensure_started`
  links the private `START_READY_TIMEOUT`, and two unresolved `` `connect` `` links
  in `src/opencode/mod.rs`. Fix anything new rather than adding to the list.

Confirm the user-facing documents describe what you are shipping:

- `README.md` — new flags, keys, config keys, behaviour;
- `docs/architecture.md` — module map and invariants that moved;
- `docs/todos.md` — the rows this work closes, ticked, with what closed them. Every
  row still open should either need a live OpenCode service or say why it is
  deferred (`superseded …`, `deferred …`). An open row with no note is unfinished
  work, not a release.

### If the release touches the UI

"It still looks right" is not verification. Dump the rendered cells and compare
against the previous tree — symbol, foreground, background, underline colour, and
modifiers, for every cell, across the screens and sizes the change could affect:

1. Add a temporary `#[test] fn zz_dump_frames()` to `src/app/tests.rs`. Being
   in-crate, it can reach private state, so it can seed a roster, projects, models,
   chat transcripts, and a deeply selected (scrolled) picker; draw each frame with
   `ratatui::backend::TestBackend` and print one line per cell.
2. Capture its output on the current tree, and again on the tree before the change
   (stash the change, or check out the previous commit in a worktree).
3. Diff the two dumps. Only the cells you meant to change may differ. The live
   `ram`/`cpu` readout in the telemetry line is the one known nondeterminism —
   ignore those cells. Anything else is a regression, tests passing or not.
4. Delete the temporary test before committing: it is scaffolding, not a test.

For a refactor whose whole point is that behaviour does not move, this is the
evidence that it didn't — put the frame count and the result in the commit body
("a differential cell dump over 34 frames differs only in …", or "is
byte-identical apart from the live RAM figure").

## 3. Bump

```bash
git status                  # the work is already committed: expect a clean tree
$EDITOR Cargo.toml          # version = "X.Y.Z"
cargo check                 # refreshes the mywaker entry in Cargo.lock
git diff --stat             # expect exactly: Cargo.toml, Cargo.lock
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to X.Y.Z"
```

Nothing else embeds the version: `make` derives it (`VERSION := $(shell sed …)`)
and `packaging/debian/control`, `packaging/PKGBUILD`, and `packaging/mywaker.spec`
carry `@VERSION@`, substituted by the `pkg-*` targets. If `git diff --stat` shows a
third file, stop and look.

## 4. Tag and push

```bash
git tag -a vX.Y.Z -m "vX.Y.Z — <one line on what changed>"
git push origin main
git push origin vX.Y.Z
```

- Annotated (`-a`) tags only: they carry a tagger and a date.
- Tag the **bump commit**, so `git describe` and `git log vPREV..vNEW` read
  correctly — the range then excludes the previous bump and includes this one.
- `v0.10.1` and `v0.11.0` were annotated with a bare version line. Prefer a short
  body: `git show vX.Y.Z` is the first thing anyone reads about a release.
- Push the tag explicitly, or use `git push --follow-tags`; a plain `git push`
  sends commits but not tags.

## 5. Optionally publish a GitHub release and packages

So far the releases are tags only: `v0.10.1` and `v0.11.0` have no GitHub release
attached. If you want one:

```bash
gh release create vX.Y.Z --title "vX.Y.Z" --notes-file /tmp/notes.md
```

Keep the notes to user-visible changes — the commit list from step 1 is the source.

Distro packages come from the same bump, each built on its own kind of machine:

```bash
make pkg-deb     # Debian/Ubuntu
make pkg-rpm     # Fedora/RHEL
make pkg-arch    # Arch (makepkg)
```

They stamp `Cargo.toml`'s version into their template, so build them **after** step
3 — and if a package needs fixing after tagging, ship a patch release rather than
moving the tag.

## Gotchas

- **Never move a published tag.** If someone may have built from it, ship a patch
  release instead. `git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z` is only
  for a tag you created seconds ago and nobody has fetched.
- **Don't bump inside a feature commit** — the per-release version history is the
  point of doing it separately.
- **A tag whose commit was never pushed is invisible.** Push `main` first, then the
  tag.
- **Published history was rewritten once.** `v0.10.1` and the release before it were
  rewritten on `main` after being published: `f57c1b0` and `bf9211c` became `9b03da1`
  and `2caa808`, and the tag was re-cut onto a new bump commit (`3ef2660`). Other
  clones needed `git reset --hard origin/main`, and the replaced commits now exist
  only in a reflog. Don't force-push `main` past a tag; if you ever must, assume
  every clone is out of date.

## Worked example: `v0.11.0`

The release this document was validated against — a minor bump for two features
and a fix:

```bash
# 1. the work, already committed and pushed to main
23f54a5 feat: file-change watch hooks ([[watches]]) for autonomous dispatch
783952d feat: task-queue panel + serialized prompts to a busy employee
2caa808 fix cursor

# 2. verified: cargo fmt --check; cargo clippy --all-targets -- -D warnings;
#    cargo test (all suites, only the live ones ignored); cargo doc --no-deps
#    (the 3 known warnings); cargo build --release

# 3. bump
$EDITOR Cargo.toml        # 0.10.1 -> 0.11.0
cargo check               # Cargo.lock: mywaker 0.10.1 -> 0.11.0
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.11.0"     # -> 41fd93f

# 4. tag and push
git tag -a v0.11.0 -m "v0.11.0"
git push origin main
git push origin v0.11.0
```

The result is the shape every release should have:

```bash
$ git log --oneline v0.10.1..v0.11.0
41fd93f chore: bump version to 0.11.0     # the tag points here
783952d feat: task-queue panel + serialized prompts to a busy employee
23f54a5 feat: file-change watch hooks ([[watches]]) for autonomous dispatch
2caa808 fix cursor

$ git log --oneline --grep='^chore: bump version'
41fd93f chore: bump version to 0.11.0
3ef2660 chore: bump version to 0.10.1
c3d3db8 chore: bump version to 0.10.0
```
