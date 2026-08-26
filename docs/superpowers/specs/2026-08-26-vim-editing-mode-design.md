# Vim Editing Mode (`set -o vim`) Design

**Date:** 2026-08-26 (rev 3, after two adversarial review rounds)
**Goal:** Add a third interactive editing mode, `set -o vim`, that provides a
Vim-editor-like editing experience (VISUAL mode, text objects, multi-level
undo/redo, `.` repeat) on top of the existing vi line-editing infrastructure,
while leaving the POSIX-compliant `set -o vi` mode untouched.

## Overview

The existing `set -o vi` mode implements POSIX (IEEE Std 1003.1) vi
line-editing semantics: `v` opens `$EDITOR`, `u` is a readline-style
undo-stack walk, there is no VISUAL mode, and there are no text objects.
Users coming from the Vim editor expect different behavior — pressing `v`
should enter VISUAL mode, `u`/`Ctrl-R` should be multi-level undo/redo,
and `diw` / `ci"` should work.

`set -o vim` is a **non-POSIX extension**. It is a third, mutually
exclusive editing mode alongside `emacs` and `vi`. **The POSIX `vi` mode
and the emacs mode are observably unchanged by this work.** (An earlier
draft applied the typed register of §8 to vi mode as well; that was
withdrawn because linewise `yy`/`p` semantics change single-line POSIX
behavior — `yyp` would create a second logical line. TODO.md residual (g)
therefore remains open for vi mode and is fixed only in vim mode.)

### Fidelity policy

- **Editing operations** (anything that mutates the buffer, changes modes,
  or drives undo) follow **vanilla Vim 9 with default settings** (oracle:
  `/usr/bin/vim --clean`, Vim 9.1). When POSIX vi and Vim disagree on an
  editing operation, vim mode follows Vim.
- **Shell operations** (history navigation/search, submit, completion,
  comment/expansion helpers, alias macros) keep the existing vi-mode shell
  semantics. Vim has no command line to submit and no shell history; forcing
  Vim buffer semantics onto these keys would make the mode useless as a
  shell. The affected keys are listed in §12 (deliberate deviations).
- Behaviors marked "oracle-verified" below were empirically confirmed
  against `vim --clean` during this design; anything this spec leaves at
  the behavioral level is resolved the same way during implementation and
  then encoded in a test.

### Non-goals (v1)

- Blockwise VISUAL (`Ctrl-V`), `gv`, VISUAL `I`/`A`
- Named registers (`"a`–`"z`), numbered registers, clipboard registers
- Marks (`m`, `` ` ``, `'`), jump list
- Ex commands (`:`), buffer-local search (`/` stays history search)
- Undo tree (`g-`, `g+`), `:earlier`/`:later`
- Insert-mode Vim extensions (`Ctrl-O`, `Ctrl-R <reg>`, `Ctrl-A`, …)
- Normal-mode `Ctrl-A`/`Ctrl-X` number increment/decrement
- Motions `{`, `}`, `gg`, `H`, `M`, `L` (`G` remains history-goto)
- `.` repeat of VISUAL-mode changes
- VISUAL `u`/`U` (lower/uppercase selection)
- User-configurable keybindings (tracked separately in TODO.md as
  `~/.inputrc`)

## 1. Option Semantics

### 1.1 `ShellOptions` (`src/env/shell_mode.rs`)

- Add field `pub vim: bool` to `ShellOptions`.
- `set_by_name` gains a `"vim"` arm. Mutual exclusivity mirrors the
  existing `emacs`/`vi` arms:
  - `set -o vim` → `vim = true, vi = false, emacs = false`
  - `set -o vi` → `vi = true, emacs = false, vim = false`
  - `set -o emacs` → `emacs = true, vi = false, vim = false`
  - `set +o vim` → `vim = false` only. As with `set +o vi` today, no mode
    being on falls back to emacs behavior in the REPL (documented deliberate
    bash deviation; same comment applies).
- `all_entries()` gains `("vim", self.vim)` in alphabetical position (after
  `"vi"`), so `set -o` / `set +o` display it.
- `to_flag_string()` (`$-`) does **not** include it, same as vi/emacs.
- No short flag; `-o vim` works at invocation via the existing
  `InvocationOp::Long` path (`main.rs` validates against `set_by_name`, so
  no change needed there).

### 1.2 Completions

`completions/set.toml` currently omits `vi` and `emacs` from the `-o`
value list. Add all three: `vi`, `emacs`, `vim`.

### 1.3 REPL sync (`src/interactive/mod.rs`)

The per-prompt sync becomes three-way:

```rust
let mode = if options.vim { EditMode::Vim }
           else if options.vi { EditMode::Vi }
           else { EditMode::Emacs };
line_editor.set_edit_mode(mode);
```

Additionally, the REPL snapshots the editor command for §10 into the
`LineEditor` before each read: resolve `VISUAL`, else `EDITOR`, else
`"vi"`, **reading only the `ShellEnv` variable store** (which imports the
process environment at startup, so inherited values are visible while
`unset VISUAL` is respected — no separate `std::env` fallback). The
resulting command string is stored on the `LineEditor` (same pattern as
the cached continuation prompt), before `read_line_with_completion` is
called, so it does not conflict with the `&mut ShellEnv` borrow held by
the continuation-prompt closure during the read.

## 2. Architecture

### 2.1 Flavor, not fork

`EditMode` (`src/interactive/vi.rs`) gains a third variant:

```rust
pub enum EditMode { Emacs, Vi, Vim }
```

**Every existing `EditMode::Vi` equality gate becomes a vi-family
predicate.** The dispatch and state code tests `edit_mode == EditMode::Vi`
at several points — reset-for-read in `clear()`, the vi short-circuit at
the top of `handle_key`, emacs undo-save suppression, cursor-style sync,
and the incomplete-Enter multiline path (`line_editor.rs` ~266, ~1556,
~1655, ~2503, ~3004 as of this writing). All of these must switch to
`self.edit_mode.is_vi_family()` (true for `Vi | Vim`); an audit of every
`EditMode::Vi` occurrence is part of Phase 1, and the Phase 1 test suite
must include a "vim mode behaves as vi mode" regression sweep to catch
missed gates.

Internally the vi engine distinguishes flavors:

```rust
pub enum ViFlavor { Posix, Vim }
```

`ViEngine` stores `flavor: ViFlavor`, set from `set_edit_mode`
(`Vi → Posix`, `Vim → Vim`). All shared machinery — count accumulation,
`Pending` operator state, motion math, find-char state, `.`-repeat
recording — remains in `vi.rs` and `line_editor.rs` and is used by both
flavors. Flavor-specific behavior branches on `self.flavor` at the
resolution layer (`resolve_command_key`) and at execution
(`execute_vi_cmd_arm`).

### 2.2 New module: `src/interactive/vim.rs`

Vim-only logic lives in a dedicated module to keep the non-POSIX extension
separate:

- VISUAL selection state and range resolution
- Text object boundary computation (pure functions over `(&[char], pos)`)
- `%` match-pair scanning, `ge`/`gE` motion math
- Linewise range/register computation (§8 algorithms)

`vi.rs` calls into `vim.rs` only when `flavor == Vim`.

### 2.3 Mode state machine

`ViMode` gains a Visual variant:

```rust
pub enum ViMode {
    Insert,
    Command,                 // = Vim "Normal"; name kept for continuity
    Visual { kind: VisualKind, anchor: usize },
}
pub enum VisualKind { Char, Line }
```

- `anchor` is the char index where VISUAL was entered; the selection is
  `min(anchor, pos) ..= max(anchor, pos)` (inclusive, Vim semantics).
  Linewise expands both ends to logical-line boundaries
  (`vi::line_start`/`vi::line_end`).
- **Clamping / empty cases:** on an empty buffer, `v`/`V` still enter
  VISUAL; the normalized selection is the empty range, operators are
  bell-free no-ops (except `c`/`s`, which enter Insert), and nothing is
  highlighted. On a non-empty buffer the inclusive end is clamped to
  `buf.len() - 1`. A linewise selection of an empty logical line selects
  that line (deleting it removes the line and its separator) but renders
  no highlighted cells in v1 (deviation, §12).
- `reset_for_read` continues to reset to Insert.
- Transitions:
  - Command --`v`--> Visual(Char), --`V`--> Visual(Line)
  - Visual --`v`/`V`--> same kind exits to Command; other kind switches
    kind, anchor preserved (oracle-verified Vim behavior)
  - Visual --Esc / operator completion--> Command
  - Visual --`c`/`s`--> Insert (after deleting selection)
- Cursor style (DECSCUSR): Insert = Bar, Command = Block, Visual = Block.
  No textual `-- VISUAL --` indicator in v1; the selection highlight and
  cursor shape are the mode indicators.

## 3. Normal-Mode Keymap (differences from POSIX vi mode)

Everything not listed here behaves exactly as in `set -o vi`.

| Key | POSIX vi mode | vim mode |
|---|---|---|
| `v` | Open `$EDITOR` on buffer/history entry | Enter VISUAL charwise. Count is ignored (deviation, §12) |
| `V` | Bell (unbound) | Enter VISUAL linewise |
| `u` | Pop undo stack (no redo) | Multi-level undo, `[count]u` supported |
| `Ctrl-R` | Fuzzy history search | Redo, `[count]Ctrl-R` supported |
| `U` | Restore line to as-recalled state | Unchanged (kept; approximation of Vim's `U`, §12) |
| `Y` | `y$` (POSIX) | Linewise `yy` (Vim default, oracle-verified) |
| `%` | Bell | Jump to matching `(` `)` `[` `]` `{` `}` (§5.1); inclusive motion under operator |
| `g` | Bell | Prefix key (pending state): `ge`, `gE`; `g`+other → bell |
| `i`/`a` after operator | Bell (no text objects) | Text object pending state (§6) |
| `Ctrl-X` | Fall through to emacs keymap | Prefix: `Ctrl-X Ctrl-E` opens the editor; result loads into buffer, **not** executed (§10). `Ctrl-X`+other → bell. Shadows Vim's number-decrement (deviation, §12) |
| `dd`/`yy`/`Y` | Line text only, count ignored | `[count]` lines; delete consumes a separator (§8.2), linewise register kind |
| `cc`/`S` | Line text only, count ignored | `[count]` lines replaced by one empty line, → Insert (§8.2), linewise register kind |
| `p`/`P` | Insert kill-ring text at cursor | Charwise/linewise-aware put (§8.3) |

Notes:

- `cw`/`cW` already behave as `ce`/`cE` on a non-blank in the existing vi
  implementation (`line_editor.rs` `ViCmd::Op` arm; vi/readline tradition,
  POSIX is silent). This matches Vim; **no flavor difference exists** and
  none is introduced.
- Keys explicitly **unchanged** in vim mode (shell-semantics boundary):
  `Enter` (submit), `/` `?` `n` `N` (history search), `k`/`j`/`-`/`+`/
  arrows (multiline movement with history at buffer edges), `G` (history
  goto), `#` (comment-and-stash), `=` (list expansions), `\` (complete),
  `*` (expand glob), `_` (insert last arg), `@letter` (alias macro), `~`,
  `r`, `x`, `X`, `s`, `D`, `C`, `R`, `.`,
  `f`/`F`/`t`/`T`/`;`/`,`, all existing motions, counts,
  `Ctrl-C`/`Ctrl-D`/`Ctrl-L`/`Ctrl-J`.

## 4. VISUAL Mode

### 4.1 Keymap

| Key | Action |
|---|---|
| motions (`h l w b e 0 ^ $ f t ; ,` `%` `ge` …) | Move `pos`, extending the selection. Counts apply. |
| `iw aw iW aW i" a" i' a' i` a` i( a( ib ab i[ a[ i{ a{ iB aB i< a<` | Text object: with a single-char selection, select the object; with a larger selection, extend it per Vim's visual text-object rules (oracle-verified: `0vwiwd` on `one two three` leaves ` three`) |
| `d` / `x` | Delete selection into unnamed register; → Command |
| `c` / `s` | Delete selection into register; → Insert |
| `y` | Yank selection; cursor to selection start; → Command |
| `D` | Delete the whole logical lines touched by the selection (linewise, §8.2); → Command |
| `C` / `S` / `R` | Change the whole logical lines touched by the selection (linewise change, §8.2); → Insert |
| `Y` | Yank the whole logical lines touched by the selection (linewise); → Command |
| `p` | Replace selection with register contents (§8.3); deleted text swaps into the unnamed register; → Command |
| `P` | As `p`, but the unnamed register is left unchanged (Vim behavior); → Command |
| `r{char}` | Replace every selected character **except `'\n'` separators** with `{char}` (oracle-verified: charwise `rX` over `ab\ncd` → `XX\nXd`; line structure preserved); → Command |
| `~` | Toggle case of selection; → Command |
| `o` | Swap cursor and anchor |
| `v` / `V` | Kind toggle / exit per §2.3 |
| `Esc` | Cancel pending prefix if one is active (stay in Visual); otherwise exit to Command |
| `Ctrl-C` | Exit to Command (Vim behavior; does **not** cancel the line — cancel-line remains a Command-mode/emacs action) |
| `j` / `k` / arrows | **Pure buffer motion.** At buffer edges: no history recall, cursor simply stops (bell-free no-op, matching Vim) |
| `Enter` | Submit the whole line (shell semantics; selection discarded) |
| other | Bell, selection retained |

`Ctrl-D` (EOF) and `Ctrl-L` (clear screen) keep their shell semantics.

**Pending states in Visual:** `f`/`F`/`t`/`T` reuse `Pending::FindChar`
with no operator; on failure the cursor and selection are unchanged
(bell), on success `last_find` is updated (shared with Command mode, so
`;`/`,` work in Visual). `g`, `i`, `a`, `r` each set a one-key pending
prefix. While any pending prefix is active, Esc cancels only the prefix
and stays in Visual; an unknown follow-up key bells, clears the pending
state, and retains the selection.

Register kind: charwise VISUAL yields charwise; linewise VISUAL and the
`D`/`C`/`S`/`R`/`Y` line variants yield linewise (§8).

### 4.2 Operators on linewise selections

`d`/`y` on a `Visual { kind: Line }` selection operate on whole logical
lines using the §8.2 **delete** range (separator consumed). `d` on all
lines of the buffer leaves an empty buffer. `c` uses the §8.2 **change**
range: the selected lines are replaced by one empty logical line and the
editor enters Insert on it (oracle-verified: `cc` on line `b` of
`a\nb\nc` yields `a\n\nc` with Insert on the middle line).

### 4.3 Selection rendering

- The selection is rendered with **reverse video**, overlaid on top of the
  existing syntax-highlight `ColorSpan`s.
- Mechanism: `redraw`/`redraw_content`/`redraw_multiline` receive an
  optional `selection: Option<Range<usize>>` (char indices, end-exclusive
  after normalizing the inclusive selection). The char-walk maintains an
  `in_selection` flag. Entering the selection emits
  `Terminal::set_reverse(true)`. **Because `reset_style()` also clears the
  reverse attribute** (the existing renderers emit `reset_style` +
  `apply_style` at every highlight-span transition), the walk must
  re-emit `set_reverse(true)` immediately after every style transition
  that occurs while `in_selection` is true. Leaving the selection emits
  `reset_style` followed by reapplying the current color style.
- **Boundary cleanup:** a selection that extends to the final buffer
  character never hits a "leaving the selection" cell, so each renderer
  performs an explicit post-loop `reset_style` (+ reapply if a
  suggestion or trailing output follows). In the multiline renderer,
  reverse is switched off before emitting any non-buffer output — row
  breaks and PS2 continuation prompts — and re-asserted at the first
  selected cell of the next row, so prompts and line wraps are never
  reverse-rendered.
- **Partial-repaint interaction:** while a selection is active (and on the
  first frame after it is dismissed), the single-line diff-based partial
  repaint is bypassed — full-line repaint is used. The selection changes on
  every cursor move, so the diff path would buy nothing, and this keeps
  `style_diff_pos` untouched. Multiline already always does a full repaint.
- `MockTerminal` tracks reverse **state** and emits `[REV]` when it turns
  on and `[/REV]` when it turns off — whether via `set_reverse(false)` or
  implicitly via `reset_style()` — so the marker stream reflects the real
  terminal state.

## 5. Added Motions

### 5.1 `%` (match pair)

- Scans forward from the cursor to the end of the current logical line for
  the first of `( ) [ ] { }`; jumps to its match. Matching scans the whole
  buffer (multiline) with nesting.
- **Quote awareness:** Vim's built-in `%` skips bracket characters inside
  quoted strings — both double **and single** quotes (oracle-verified:
  `0%x` on `{ "}" }` and on `{ '}' }` each delete the final `}`, not the
  quoted one). The matcher implements Vim's in-string heuristic (quote
  counting on the bracket's logical line, as in Vim's `findmatchlimit()`);
  backtick handling and remaining edge cases are oracle-resolved.
- No pair char on the rest of the line → bell.
- As an operator target it is an **inclusive** motion (Vim semantics).
- Count is ignored (Vim's `[count]%` goes to a percentage of the file;
  meaningless here — deviation, §12).

### 5.2 `ge` / `gE`

- Move backward to the end of the previous word / WORD. Uses the existing
  `char_class` word machinery.
- Inclusive under an operator, counts apply.

## 6. Text Objects

Available in two contexts: after an operator in Command mode
(`d`/`c`/`y` + object) and in VISUAL mode (§4.1). Grammar:

```
Pending::Op(op, count) + 'i'|'a'  →  Pending::TextObject { op, count, around: bool }
Pending::TextObject + object-char →  resolve range → execute op
Visual + 'i'|'a'                  →  pending prefix → object-char → adjust selection
```

Unknown object char → bell, pending state cleared.

### 6.1 Object set

| Object | Meaning |
|---|---|
| `iw` / `aw` | word (same `char_class` word rules); `aw` includes trailing whitespace, or leading whitespace when there is none trailing (Vim rule) |
| `iW` / `aW` | WORD (whitespace-delimited) |
| `i"` `a"` / `i'` `a'` / `` i` `` `` a` `` | quoted string on the current logical line; `a` includes trailing whitespace after the closing quote (or leading before the opening quote if none trailing — Vim rule). Backslash-escaped quotes are skipped (`quoteescape` default). When the cursor is before the first quote on the line, the object operates on the next quoted span (Vim behavior) |
| `i(` `a(` `ib` `ab` / `i[` `a[` / `i{` `a{` `iB` `aB` / `i<` `a<` | bracket block; cursor on a bracket or inside the pair; nested pairs resolved by matching; **multiline** (brackets may span `'\n'`) |

- **Empty inner objects** (`i(` on `()`, `i"` on `""`): `d`/`y` are
  bell-free no-ops, but `c` enters Insert positioned inside the pair
  (oracle-verified: `ci(X` on `()` yields `(X)`). In VISUAL, an empty
  inner object leaves the selection unchanged with a bell.
- Counts on text objects (`d2aw`) apply per Vim (2 words). Count on quote
  objects is ignored (Vim ignores it too).
- Edge cases beyond this table are oracle-resolved.

### 6.2 Register kind

Text-object deletes/yanks are charwise. (Vim's special case promoting
certain `ip`/`ap` deletes to linewise does not apply — no paragraph
objects in v1.)

## 7. Undo / Redo

### 7.1 Semantics (Vim-granularity, linear)

- One undo unit per: buffer-mutating Command-mode command, VISUAL
  operation, Insert session that changed the buffer, or `Ctrl-X Ctrl-E`
  buffer replacement. A change command that enters Insert (`c`, `s`,
  `cc`, `S`, `C`, VISUAL `c`/`C`/…) forms **one** unit spanning the
  deletion and the inserted text (Vim: `cwfoo<Esc>` then `u` restores the
  original word).
- **History recall resets undo history.** Recalling a history entry
  (`k`/`j` at the edge, `G`, search accept) clears both stacks and starts
  a fresh base — the line-editor analog of Vim's per-buffer undo. `u`
  immediately after a recall bells. (`U` continues to restore the
  as-recalled state, unchanged.)
- `u` undoes, `Ctrl-R` redoes; both accept counts.
- **Commit criterion: the buffer changed** (byte comparison against the
  unit's pre-state). A no-op insert session (`i` then `Esc`) or a failed
  command commits nothing and preserves redo (Vim behavior: after
  `x u i<Esc>`, `Ctrl-R` still redoes the `x`). Deviation: a change
  command whose result equals its input (e.g. `r` with the character
  already under the cursor) also commits nothing, whereas Vim would
  create a unit and clear redo (§12).
- No undo tree. After undo/redo the cursor moves to the position stored
  with the restored snapshot (approximation of Vim's "start of changed
  text", §12).

### 7.2 Implementation

`UndoManager` (`src/interactive/undo.rs`) is extended:

- `undo_stack: Vec<(Vec<char>, usize)>` (existing, cap 256) plus
  `redo_stack: Vec<(Vec<char>, usize)>`.
- `undo(current)` pushes `current` onto redo, pops undo; `redo(current)`
  the reverse. Full-buffer snapshots are fine at line-editor scale.
- Emacs mode and POSIX vi mode keep their existing behavior exactly
  (`u` walks the undo stack; the redo stack is unused there).

**Hook placement (vim flavor only).** The existing code has **no** central
save-before-mutation hook: snapshots are scattered through individual
command arms, and some mutating paths take none. The vim flavor uses a
central compare-and-commit wrapper in `execute_vi_cmd_inner`, with the
per-arm `save()` calls suppressed. Precise rules:

- **Excluded commands** (the wrapper must not treat these as changes):
  `Undo` and `Redo` (they manipulate the stacks themselves), and the
  history navigation/recall commands (`k`/`j`-at-edge, `G`, search
  accept), which instead trigger the §7.1 history-reset. `UndoAll` (`U`)
  is **not** excluded: it goes through the normal wrapper, so its
  restore-to-base commits as a regular unit (undoable, clears redo) —
  replacing the per-arm `save()` it relies on today.
- **Re-entrancy guard:** `ViCmd::Repeat` (`.`) re-invokes
  `execute_vi_cmd_inner` recursively; the wrapper commits only at the
  outermost depth so a repeated change yields exactly one unit. Insert
  staging is likewise depth-gated: only outermost-depth commands may
  stage. `.` replay applies recorded insert text synchronously via
  `vi_replay_insert` without real Insert-mode transitions, so inner-depth
  execution never touches the staging slot, and the wrapper asserts it is
  empty after replay.
- **Normal case:** snapshot `(buf, pos)` before dispatching the arm;
  after the arm, if the buffer differs, push the pre-snapshot as one undo
  unit and clear the redo stack.
- **Commands that enter Insert:** the wrapper does **not** commit;
  its pre-command snapshot transfers into the Insert staging slot. On
  leaving Insert (Esc), the staged snapshot is committed as one unit iff
  the buffer differs from it — covering the §7.1 single-unit rule for
  `c`-family commands.
- **Plain Insert entry** (`i`, `a`, `A`, …) stages a snapshot the same
  way. **The initial Insert session** (every read starts in Insert via
  `reset_for_read`, with no entry transition) is staged by `clear()`
  placing the empty-buffer snapshot in the staging slot for the vim
  flavor.
- `Ctrl-X Ctrl-E` buffer replacement commits one unit through the same
  helper.
- **Incomplete Enter** (structurally incomplete input inserts `'\n'` and
  continues editing; handled in the production loop outside
  `execute_vi_cmd_inner`): in the vim flavor its direct `undo.save()` is
  suppressed. If the editor is already in Insert, the `'\n'` simply joins
  the ongoing staged session. If it is in Command mode, the pre-newline
  snapshot is placed in the staging slot and Insert begins — one unit
  spanning the newline plus the typed continuation (analogous to Vim's
  `o`).
- In vim mode `Ctrl-R` shadows fuzzy history search in Command mode;
  fuzzy search remains on `Ctrl-R` in Insert mode.

## 8. Typed Unnamed Register (vim flavor only)

POSIX vi mode and emacs mode are **not** changed by this section.

```rust
pub struct UnnamedRegister { text: String, kind: RegisterKind }
pub enum RegisterKind { Charwise, Linewise }
```

### 8.1 Writes and interop

- Every vim-flavor delete/change/yank writes the unnamed register with the
  appropriate kind: `dd`/`cc`/`yy`/`S`/`Y`, linewise-VISUAL ops, and the
  VISUAL line variants (`D`/`C`/`S`/`R`/`Y`) → `Linewise` (text carries
  one trailing `'\n'` per line, synthesized for the final line);
  everything else → `Charwise`.
- **Kill-ring interop invariant:** the unnamed register mirrors the kill
  ring's **front entry** after every write. All existing
  `kill_ring.kill(...)` / `kill_ring.prepend(...)` call sites (emacs and
  POSIX-vi included) route through a `record_kill` helper that performs
  the ring write — including its consecutive-kill append/prepend merging
  — and then copies the resulting front entry into the unnamed register
  (kind `Charwise`, or the vim-flavor kind when the write came from a
  vim-flavor operation). Thus text killed in emacs mode is `p`-puttable
  after `set -o vim`, and a merged `Ctrl-W Ctrl-W` kill puts as the
  merged whole, preserving today's interop. (Emacs `Alt-Y` yank-pop
  rotation does not update the register; it mirrors writes only — §12.)
- vim-flavor `p`/`P` read only the unnamed register; emacs `Ctrl-Y` reads
  only the kill ring.
- The register persists across reads (like the kill ring), so `yy` on one
  command line and `p` on the next works.

### 8.2 Linewise ranges

For logical lines *i..=j* of the buffer (char indices via
`vi::line_start`/`vi::line_end`):

- **Delete range** (`dd`, VISUAL-line `d`, `D`): `start = line_start(i)`,
  `end = line_end(j)`. If `end < buf.len()` (a separator follows), the
  range is `start .. end+1` (consume the trailing `'\n'`). Else if
  `start > 0`, the range is `start-1 .. end` (consume the preceding
  separator). Else (whole buffer) `start .. end`. Cursor after delete:
  first non-blank of the line now at position *i* (or the new last line
  when the tail was deleted; or 0 on an empty buffer).
- **Change range** (`cc`, `S`, VISUAL-line `c`, VISUAL `C`/`S`/`R`):
  `line_start(i) .. line_end(j)` — **no separator is consumed**. The
  selected lines collapse to one empty logical line, the cursor sits on
  it, and the editor enters Insert (oracle-verified, §4.2).
- **Register text** (both cases): the selected lines joined with `'\n'`
  plus one trailing `'\n'`, regardless of which separator the delete
  range consumed.
- `[count]dd`/`cc`/`yy` select lines *i..=i+count-1* clamped to the last
  line.

### 8.3 Put

- Charwise register: `p`/`P` insert after/before the cursor (existing
  behavior), `count` repetitions.
- Linewise register (text `T` always ends in `'\n'`; with `[count]`, the
  pasted block is `T` repeated `count` times, then treated as one `T`
  below): `p` inserts `'\n' + T[..len-1]` at `line_end(cursor line)`;
  `P` inserts `T` at `line_start(cursor line)`. Cursor to the first
  non-blank of the first pasted line (Vim).
- **VISUAL `p`/`P`:** the selection is deleted first; with `p` the
  deleted text swaps into the unnamed register *with the selection's
  kind*, with `P` the register is left unchanged (both Vim behavior).
  To keep the §8.1 front-entry invariant intact, VISUAL `P`'s implicit
  deletion bypasses `record_kill` entirely — it writes **neither** the
  unnamed register nor the kill ring.
  Same-kind cases: charwise-over-charwise splices in place;
  linewise-over-linewise replaces the lines. Cross-kind cases: a linewise
  register pasted over a charwise selection is inserted as complete
  line(s) at the deletion point, splitting the surrounding line; a
  charwise register pasted over a linewise selection becomes its own new
  line. Exact cursor placement in the cross-kind cases is oracle-resolved
  and test-encoded.

## 9. `.` Repeat

- Unchanged machinery. In vim flavor, VISUAL-mode changes are **not**
  recorded: `.` after a VISUAL change repeats the last recorded
  non-VISUAL change (deviation from Vim, §12; future work in TODO.md).
- New change commands that become recordable in vim flavor: operator +
  text object (e.g. `diw`, `ci"` including the inserted text).

## 10. `Ctrl-X Ctrl-E` (edit command line in editor)

- Vim-mode Command-mode binding (two-key chord; `Ctrl-X` becomes a pending
  prefix). Rationale: `v` is taken by VISUAL; `Ctrl-X Ctrl-E` is the
  established bash convention. It shadows Vim's Normal-mode `Ctrl-X`
  (number decrement), which is out of scope anyway (§12).
- Reuses the temp-file / raw-mode-suspend machinery of `vi_edit_in_editor`
  with two differences, which require refactoring rather than reuse of the
  existing function as-is (it is hard-wired to POSIX-`v` semantics):
  1. **New `KeyAction::ViEditBuffer` variant.** The existing
     `KeyAction::ViEditInEditor` is handled by the production read loop as
     edit-then-submit. The new variant replaces the buffer with the edit
     result, commits an undo unit (§7.2), redraws, and continues the read
     loop — the user reviews and presses Enter (zsh `edit-command-line`
     behavior; deliberate deviation from bash/POSIX-`v` immediate
     execution).
  2. **Editor resolution** uses the command string snapshotted from
     `ShellEnv` into the `LineEditor` before the read began (§1.3). The
     POSIX-vi `v` path keeps its current process-env resolution unchanged
     (its ShellEnv-resolution gap remains TODO residual (b)).
- No count support (operates on the current buffer only; the POSIX-`v`
  history-entry form stays vi-mode-only).
- Like `ViEditInEditor` and `@letter` macros today, the new action is only
  honored by the production `read_line_with_completion` loop (existing
  precedent, TODO residual (e)); the test-only plain loop bells.
- POSIX vi mode is unchanged: `v` still edits and submits immediately.

## 11. Rendering Summary

- Selection: reverse video overlay with re-assertion across style
  transitions and explicit boundary cleanup (§4.3). All other rendering
  (syntax highlight, autosuggestion, PS2 continuation prompts, viewport
  clamping) is unchanged.
- Cursor styles: Insert Bar / Command Block / Visual Block.
- Vim flavor introduces no new prompt-area UI.

## 12. Deliberate Deviations from Vim

Recorded here so "always match Vim" has an explicit exception list:

| Area | Vim | yosh vim mode | Why |
|---|---|---|---|
| `/` `?` `n` `N` | Buffer search | History search (glob) | Shell semantics boundary |
| `j` `k` at buffer edge, `G` | Buffer motion / go to line | History recall / history goto | Shell semantics boundary |
| `Enter` | Move to next line | Submit command | Shell |
| `#` `=` `\` `*` `_` `@x` etc. | Various/none | POSIX-vi shell helpers kept | Shell semantics boundary |
| `[count]v`, `[count]%` | Sized selection / percent-of-file | Count ignored | Low value at line-editor scale |
| `U` | Undo line (Vim `U`) | Restore-to-recalled-state | Existing approximation kept |
| Undo cursor position | Start of changed text | Snapshot cursor position | Snapshot-based implementation |
| Undo commit criterion | Per executed change command | Buffer byte-difference (same-result change preserves redo) | Snapshot-based implementation |
| `.` after VISUAL change | Repeats on same-size region | Not recorded | Deferred (TODO.md) |
| `Ctrl-R` in Insert | Insert register | Fuzzy history search | Shell; redo is Normal-mode only |
| `Ctrl-X` in Normal | Decrement number under cursor | `Ctrl-X Ctrl-E` prefix | Number inc/dec out of scope; bash convention reused |
| VISUAL `u` / `U` | Lower/uppercase selection | Absent (bell) | v1 scope |
| `~` on multi-scalar case maps | e.g. `ß` → `ẞ` | First scalar only (`ß` → `S`) | Existing limitation, TODO.md residual (i) |
| Linewise selection of empty line | Highlighted | Not visibly highlighted | Renderer has no cell for `'\n'`; v1 |
| Emacs yank-pop rotation | n/a | Does not update unnamed register | Register mirrors kill writes only |
| Insert-mode keys | Vim insert bindings | vi-insert (emacs Ctrl bindings inherited) | Existing behavior kept |
| Registers | Full register file | Single typed unnamed register | v1 scope |
| Blockwise VISUAL, `gv`, marks, `:`, `Ctrl-A` | Present | Absent | v1 scope |

## 13. Implementation Phases

Ordered so that every phase is independently shippable; the typed register
precedes VISUAL because VISUAL's linewise operators and `p` depend on it.

1. **Phase 1 — Option + flavor plumbing.** `ShellOptions.vim`,
   `set_by_name`, `all_entries`, `set.toml`, `EditMode::Vim`, the
   `is_vi_family()` audit of every `EditMode::Vi` gate, `ViFlavor` on
   `ViEngine`, REPL three-way sync + editor-command snapshot, cursor
   styles. At the end of Phase 1 vim mode behaves identically to vi mode
   (including `v`, `u`, `Ctrl-R`) — the flavor exists but changes nothing.
   Tests: "vim == vi" regression sweep, PTY smoke for `set -o` display.
2. **Phase 2 — Typed register + linewise Normal-mode ops.**
   `UnnamedRegister`, `record_kill` front-entry mirroring, §8.2
   delete/change ranges for `[count]dd`/`cc`/`yy`, linewise `p`/`P` with
   counts (§8.3), `Y` = `yy`.
3. **Phase 3 — VISUAL mode.** `ViMode::Visual`, the §4.1 keymap **minus**
   `%`/`ge`/text-object prefixes (which arrive with Phase 4), selection
   rendering (`set_reverse` plumbing with style-transition re-assertion
   and boundary cleanup, `[REV]`/`[/REV]` state-tracked markers,
   partial-repaint bypass), `Ctrl-X Ctrl-E` (`KeyAction::ViEditBuffer`).
4. **Phase 4 — Text objects + motions.** `vim.rs` boundary math, pending
   grammar (`Op`+`i`/`a`, `g` prefix, Visual prefixes), `%` with quote
   heuristic, `ge`/`gE`, `.`-recordability of the new commands.
5. **Phase 5 — Undo/redo.** `UndoManager` redo stack, compare-and-commit
   wrapper with its exclusion/re-entrancy/Insert-transfer rules (§7.2),
   history-recall undo reset, `[count]u`/`[count]Ctrl-R`.

Until Phase 5 lands, vim-mode `u`/`Ctrl-R` keep the Phase 1 (vi) behavior
— acceptable, since each phase only *adds* Vim behaviors.

TODO.md gains a "vim mode residuals" section listing the §12 deferred
items that are not permanent policy (VISUAL `.`-repeat, VISUAL case ops,
insert-mode `Ctrl-O`, named registers, blockwise, empty-line selection
highlight, `Ctrl-A`/`Ctrl-X` numbers, undo commit criterion).

## 14. Test Strategy

- **Unit tests** (in-module `#[test]`): text-object boundary functions,
  `%` matching (incl. quote heuristic), `ge`/`gE`, VISUAL range
  normalization (char/line, clamping, empty buffer), linewise
  delete/change range algorithm (§8.2, all separator cases), pending
  grammar transitions (`resolve_command_key` with flavor `Vim`),
  `UndoManager` undo/redo stack behavior, register kind rules. All pure
  functions over `(&[char], pos)` — same style as the existing tests in
  `vi.rs`.
- **Integration tests**: new `tests/vim_mode.rs`, mirroring
  `tests/vi_mode.rs` (`MockTerminal`, `seq![]`, `chars()` helpers, plus a
  `vim_read()` variant that sets `EditMode::Vim` and a `vim_read_full()`
  variant mirroring `vi_read_full` for paths that need the production
  loop). Coverage targets: every table row in §3/§4.1/§6.1, selection
  rendering via `[REV]`/`[/REV]` markers (including a multi-style
  selection proving re-assertion, and a selection ending at the buffer's
  last character proving boundary cleanup), undo/redo sequences including
  the no-op-insert redo-preservation case, the `c`-family single-unit
  case, and `.`-repeat single-unit case, linewise put with counts on
  single-line and multiline buffers, history/search keys proving the
  shell-semantics boundary, and full-suite regression: the existing
  `tests/vi_mode.rs` and emacs suites must pass unchanged (vi/emacs are
  observably untouched).
- **`Ctrl-X Ctrl-E`**: covered at the integration level via
  `vim_read_full()` — the production loop runs under `MockTerminal`
  (raw-mode transitions are no-ops there), with the editor command
  pointed at a small script that rewrites the temp file and exits,
  asserting the buffer is replaced and **not** submitted. A PTY variant
  repeats the flow end-to-end; it must `unset VISUAL` in the spawned
  shell and set `EDITOR` as a (non-exported) shell variable, proving the
  ShellEnv snapshot path rather than process-env inheritance.
- **PTY tests** (`tests/pty_interactive.rs`): `set -o vim` toggling and
  display (`vim on`, `vi off`, `emacs off`), a basic VISUAL edit, a
  `u`/`Ctrl-R` round-trip, and the `Ctrl-X Ctrl-E` scripted-editor test.
  Generous timeouts per existing PTY conventions.
- **E2E (`e2e/`)**: out of scope — the script-based harness does not cover
  interactive editing (same as vi mode).
- **Fidelity checks during development**: behaviors marked "Vim rule" /
  "oracle-verified" are checked against `/usr/bin/vim --clean` before the
  corresponding test is written; the test then encodes the observed
  behavior.
