# SP3 — `src/interactive/highlight_scanner.rs` Responsibility Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `src/interactive/highlight_scanner.rs` (1594 lines, the largest file in the project) into a `src/interactive/highlight_scanner/` module of ten responsibility-focused submodules; convert the per-mode `scan_*` methods from `&mut self` methods on `HighlightScanner` to free functions taking `&mut ScanCtx<'_>`; preserve every public API used by `interactive/highlight.rs`, `interactive/mod.rs`, and `interactive/line_editor.rs`; verify the redesign holds scan-throughput within ±5% of baseline via `cargo bench --bench interactive_smoke`.

**Architecture:** `src/interactive/highlight_scanner/mod.rs` becomes a thin facade defining `HighlightScanner` (which still holds the cache and the public `scan` API) and a dispatcher `scan_from` that builds `ScanCtx` once per scan call and matches on `ScanMode` to delegate to the appropriate per-mode free function. Submodules host scanner bodies as `pub(super) fn` free functions with a uniform signature: `(ctx: &mut ScanCtx, env: &CheckerEnv, pos: usize, [payload...]) -> usize`.

**Tech Stack:** Rust 2024 edition, criterion (`benches/interactive_smoke.rs`), `expectrl` (PTY tests). The `ScanCtx` struct uses borrowed lifetimes — no allocations introduced by the redesign.

**Reference Documents:**
- Spec: `docs/superpowers/specs/2026-05-06-sp3-highlight-scanner-redesign-design.md` (revised commits `0d7514b`, `5502e54`)
- Umbrella: `docs/superpowers/specs/2026-05-06-large-file-redesign-umbrella-design.md`
- Predecessor plans: SP1 `2026-05-06-sp1-plugin-host-redesign-plan.md`, SP2 `2026-05-07-sp2-env-jobs-redesign-plan.md`

**Line-count Target:** Per umbrella DoD #6, each production file ≤ 400 lines. Spec's stricter ≤ 270 is aspirational.

**Definition of Done (per umbrella + SP3):**

1. `cargo test --lib --features test-helpers` PASS.
2. `./e2e/run_tests.sh` PASS (full).
3. `cargo bench --no-run` PASS (compile only).
4. `cargo clippy --all-targets -- -D warnings` — only the two pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99` remain.
5. `cargo fmt --check` PASS.
6. Each production file in `src/interactive/highlight_scanner/` ≤ 400 lines.
7. README/CLAUDE.md/TODO.md references to `src/interactive/highlight_scanner.rs` still resolve via grep.
8. Public API names + signatures preserved: `HighlightScanner::new`, `HighlightScanner::scan`. Re-export at `src/interactive/highlight.rs:11` continues to work.
9. `tests/pty_interactive.rs` syntax-highlighting tests PASS — non-flaky over 3 consecutive runs.
10. **Performance:** `cargo bench --bench interactive_smoke` shows scan throughput within ±5% of the PR-A baseline. (Captured in Task A5; verified in Task B6.)
11. `command_checker.rs` gains the 6 relocated `test_checker_*` tests; `highlight_scanner/mod.rs::tests` no longer references `CommandChecker` / `CheckerEnv` for testing.

---

## File Structure

After all tasks complete, `src/interactive/highlight_scanner/` looks like:

```
src/interactive/highlight_scanner/
  mod.rs          — HighlightScanner struct, public scan() API, scan_from dispatcher.
                    The dispatcher builds ScanCtx, matches on ScanMode (9 variants),
                    delegates to per-mode free functions in submodules. Storage tests stay.
  state.rs        — ScanMode enum, ScannerState struct + impl, mark_unclosed_errors as
                    pub(super) free fn. State-related tests.
  cache.rs        — HighlightCache struct + impl (checkpoint logic).
  helpers.rs      — KEYWORDS / COMMAND_POSITION_KEYWORDS tables, is_keyword / is_operator_char
                    / is_redirect_start / is_valid_name / is_word_break free fns.
  ctx.rs          — ScanCtx<'a> struct (input, state, spans, checker — the four pieces
                    of mutable state shared across all scanners).
  normal.rs       — scan_normal as pub(super) fn. Calls expansion::scan_dollar and
                    word::scan_word for embedded $-expansion and unquoted words.
  word.rs         — scan_word as pub(super) fn (the only one that uses ctx.checker).
  quotes.rs       — scan_single_quote, scan_double_quote, scan_dollar_single_quote.
                    scan_double_quote calls expansion::scan_dollar inline.
  expansion.rs    — scan_dollar, scan_parameter, scan_arith_sub.
  comment.rs      — scan_comment.
```

**Re-exports in `mod.rs`:** None at module level — `HighlightScanner` is defined in `mod.rs` itself, so `src/interactive/highlight.rs:11`'s `pub use super::highlight_scanner::HighlightScanner;` continues to resolve through `highlight_scanner/mod.rs`.

**Visibility:**
- `HighlightScanner` and its `pub` methods (`new`, `scan`) — `pub`.
- `ScanCtx`, `ScanMode`, `ScannerState`, `HighlightCache`, all per-mode scanner functions — `pub(super)` (visible within `highlight_scanner/`).
- Helpers (`is_keyword` etc.) and `mark_unclosed_errors` — `pub(super)`.
- `KEYWORDS` / `COMMAND_POSITION_KEYWORDS` — `pub(super) const`.

**Scanner signatures (after PR-B):**

```rust
// All scanners take `(ctx: &mut ScanCtx, env: &CheckerEnv, pos: usize, [payload])`.
pub(super) fn scan_normal(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize) -> usize;
pub(super) fn scan_word(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize) -> usize;
pub(super) fn scan_single_quote(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize, start: usize) -> usize;
pub(super) fn scan_double_quote(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize, start: usize) -> usize;
pub(super) fn scan_dollar_single_quote(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize, start: usize) -> usize;
pub(super) fn scan_dollar(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize) -> usize;
pub(super) fn scan_parameter(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize, start: usize, braced: bool) -> usize;
pub(super) fn scan_arith_sub(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize, start: usize) -> usize;
pub(super) fn scan_comment(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize, start: usize) -> usize;
```

The `env` parameter is `unused` in scanners that don't need it (single_quote, dollar_single_quote, parameter, arith_sub, comment). Mark with `_env` or `#[allow(unused_variables)]` per Rust convention. Uniformity outweighs minor noise.

**Dispatcher in `mod.rs::scan_from`** (after PR-B):

```rust
fn scan_from(&mut self, chars: &[char], start_pos: usize, state: &mut ScannerState, env: &CheckerEnv) -> Vec<ColorSpan> {
    let mut spans = Vec::new();
    let mut pos = start_pos;
    self.cache.checkpoints.retain(|(cp, _)| *cp < start_pos);
    if start_pos == 0 {
        self.cache.checkpoints.push((0, state.clone()));
    }
    while pos < chars.len() {
        if pos > 0
            && pos.is_multiple_of(self.cache.checkpoint_interval)
            && !self.cache.checkpoints.iter().any(|(cp, _)| *cp == pos)
        {
            self.cache.checkpoints.push((pos, state.clone()));
        }
        let mut ctx = ScanCtx {
            input: chars,
            state,
            spans: &mut spans,
            checker: &mut self.checker,
        };
        pos = match ctx.state.current_mode().clone() {
            ScanMode::Normal              => normal::scan_normal(&mut ctx, env, pos),
            ScanMode::SingleQuote { start }       => quotes::scan_single_quote(&mut ctx, env, pos, start),
            ScanMode::DoubleQuote { start }       => quotes::scan_double_quote(&mut ctx, env, pos, start),
            ScanMode::DollarSingleQuote { start } => quotes::scan_dollar_single_quote(&mut ctx, env, pos, start),
            ScanMode::Parameter { start, braced } => expansion::scan_parameter(&mut ctx, env, pos, start, braced),
            ScanMode::ArithSub { start }          => expansion::scan_arith_sub(&mut ctx, env, pos, start),
            ScanMode::Comment { start }           => comment::scan_comment(&mut ctx, env, pos, start),
            ScanMode::CommandSub { .. }           => { ctx.state.pop_mode(); pos }
            ScanMode::Backtick { .. }             => { ctx.state.pop_mode(); pos }
        };
    }
    spans
}
```

The `state: &mut ScannerState` parameter on `scan_from` is reborrowed into `ctx.state` each loop iteration. The `spans: Vec<ColorSpan>` is built inside `scan_from` and returned. The `mut ctx` is constructed fresh inside the loop because the borrow on `self.checker` and on the local `spans` vec must be reborrowed each iteration (otherwise the `self.cache.checkpoints.push` above would conflict with the outstanding `&mut self.checker` borrow).

---

## Task 0: Pre-flight Verification

**Goal:** Confirm baseline build/test state, capture file statistics, verify external callers and the bench file before starting.

**Files:** Read-only inspection.

- [ ] **Step 1: Verify clean working tree**

```bash
git status
```
Expected: clean tree on `main`.

- [ ] **Step 2: Capture file size baseline**

```bash
wc -l src/interactive/highlight_scanner.rs src/interactive/command_checker.rs
```
Expected: `1594 src/interactive/highlight_scanner.rs`, `122 src/interactive/command_checker.rs`.

- [ ] **Step 3: Confirm bench file exists**

```bash
ls -la benches/interactive_smoke.rs
```
Expected: file exists.

- [ ] **Step 4: List external callers (read-only sanity)**

```bash
rg -n "HighlightScanner|use crate::interactive::highlight_scanner" --type rust src/ | grep -v "src/interactive/highlight_scanner"
```
Expected: 5 hits — `interactive/highlight.rs:11` (re-export), `interactive/mod.rs:25 / 35 / 121`, `interactive/line_editor.rs:12 / 938 / 968`. **No hit references the internal `ScanMode`, `ScannerState`, or per-mode scanners.**

- [ ] **Step 5: Run lib + integration test baseline**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok. NNN passed; 0 failed`. Record the count.

- [ ] **Step 6: Run e2e baseline**

```bash
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected: `Passed: 393 / 393` (or current count, 0 failures). Run in background — takes 1–3 minutes.

- [ ] **Step 7: Bench compile baseline**

```bash
cargo bench --no-run 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 8: No commit**

Pre-flight is informational. Proceed to Task A1.

---

## Task A1: Convert `highlight_scanner.rs` to `highlight_scanner/mod.rs` (Pure Rename)

**Goal:** Move file into a module directory without changing any content. Mechanical rename; verify nothing broke.

**Files:**
- Create: `src/interactive/highlight_scanner/` (directory)
- Move: `src/interactive/highlight_scanner.rs` → `src/interactive/highlight_scanner/mod.rs`

- [ ] **Step 1: Create directory and move file**

```bash
mkdir -p src/interactive/highlight_scanner && git mv src/interactive/highlight_scanner.rs src/interactive/highlight_scanner/mod.rs
```
Expected: success, no output.

- [ ] **Step 2: Verify build**

```bash
cargo build 2>&1 | tail -3
```
Expected: `Finished`. Rust resolves `pub mod highlight_scanner;` (declared at `src/interactive/mod.rs`) to either the file or the dir's `mod.rs` — equivalent.

- [ ] **Step 3: Verify tests still pass**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok`. Same total count as Task 0 baseline.

- [ ] **Step 4: Confirm rename (no duplicate)**

```bash
wc -l src/interactive/highlight_scanner/mod.rs && ls src/interactive/ | grep highlight_scanner
```
Expected: `1594 src/interactive/highlight_scanner/mod.rs`. `ls` shows directory only.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): convert highlight_scanner.rs to mod.rs

Pure rename — first step of SP3. No content changes. The file
becomes the module file for `pub mod highlight_scanner;` declared
in src/interactive/mod.rs, enabling future per-responsibility
submodules.

Original SP3 prompt: split src/interactive/highlight_scanner.rs
into state / cache / helpers / ctx / per-mode-scanner submodules
per docs/superpowers/specs/2026-05-06-sp3-highlight-scanner-redesign-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A2: Extract `helpers.rs`

**Goal:** Move character classification free functions and keyword tables to a `helpers.rs` submodule.

**Files:**
- Create: `src/interactive/highlight_scanner/helpers.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (delete moved items, add `mod helpers;` and `use helpers::*;`)

- [ ] **Step 1: Create `helpers.rs` with this exact content:**

```rust
//! Character classification helpers and keyword tables for the highlight scanner.
//!
//! These are pure functions that read no state. They identify shell-syntax
//! categories: keywords, operators, redirect starts, valid name characters,
//! and word boundaries.

pub(super) const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "do", "done", "while", "until", "case", "esac",
    "in", "!", "{", "}",
];

/// Keywords after which the *next* word is also in command position.
pub(super) const COMMAND_POSITION_KEYWORDS: &[&str] = &["then", "else", "elif", "do", "!", "time"];

pub(super) fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

pub(super) fn is_operator_char(ch: char) -> bool {
    matches!(ch, '|' | '&' | ';')
}

pub(super) fn is_redirect_start(ch: char) -> bool {
    matches!(ch, '<' | '>')
}

pub(super) fn is_valid_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True for characters that cannot appear inside an unquoted word.
pub(super) fn is_word_break(ch: char) -> bool {
    ch.is_ascii_whitespace()
        || is_operator_char(ch)
        || is_redirect_start(ch)
        || matches!(ch, '(' | ')' | '\'' | '"' | '`' | '$' | '#')
}
```

- [ ] **Step 2: Remove the moved items from `mod.rs`**

In `src/interactive/highlight_scanner/mod.rs`, delete:

1. The two `const` arrays `KEYWORDS` and `COMMAND_POSITION_KEYWORDS` (lines ~66–72).
2. The five free functions: `is_keyword`, `is_operator_char`, `is_redirect_start`, `is_valid_name`, `is_word_break` (lines ~74–108).
3. Their preceding section header comments (`// Keyword tables`, `// Character classification helpers`).

- [ ] **Step 3: Add `mod helpers;` and a glob import to `mod.rs`**

In `src/interactive/highlight_scanner/mod.rs`, just below the existing `use` statements at the top of the file, add:

```rust

mod helpers;

use helpers::*;
```

(Glob import is appropriate here because every helper is `pub(super)` and consumed extensively throughout `mod.rs`.)

- [ ] **Step 4: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok`. Same totals as A1.

```bash
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: zero new warnings, zero errors.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): extract char classification helpers to helpers.rs

SP3 PR-A step 2. Move KEYWORDS / COMMAND_POSITION_KEYWORDS tables
and is_keyword / is_operator_char / is_redirect_start /
is_valid_name / is_word_break to a dedicated submodule. Pure
relocation — no code change. Visibility changed from private to
pub(super) so the rest of mod.rs and future submodules can use them.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A3: Extract `state.rs`

**Goal:** Move `ScanMode` enum, `ScannerState` struct + impl, and `mark_unclosed_errors` (currently a method on `HighlightScanner`) to `state.rs`. The latter becomes a `pub(super) fn` free function so it can be called from the dispatcher in `mod.rs`.

**Files:**
- Create: `src/interactive/highlight_scanner/state.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (delete moved items, add `mod state;` and re-imports, update `mark_unclosed_errors` callsite)

- [ ] **Step 1: Locate `mark_unclosed_errors` and inspect its current form**

```bash
grep -n "fn mark_unclosed_errors\|self.mark_unclosed_errors" src/interactive/highlight_scanner/mod.rs
```
Note the line numbers and the call site for use in Step 3.

- [ ] **Step 2: Create `src/interactive/highlight_scanner/state.rs` with this content:**

```rust
//! Scanner state machine — modes and the mutable state carried through scan.

use super::highlight::ColorSpan;

/// Each mode represents a different parsing context inside the input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScanMode {
    Normal,
    SingleQuote { start: usize },
    DoubleQuote { start: usize },
    DollarSingleQuote { start: usize },
    Parameter { start: usize, braced: bool },
    CommandSub { start: usize },
    Backtick { start: usize },
    ArithSub { start: usize },
    Comment { start: usize },
}

/// Mutable state carried through the scan.
#[derive(Debug, Clone)]
pub(super) struct ScannerState {
    pub mode_stack: Vec<ScanMode>,
    /// True when the next non-whitespace character starts a new token at
    /// the beginning of a word (used for `#` comment detection and `~`).
    pub word_start: bool,
    /// True when the next word is in command position (first word of a
    /// simple command, or immediately after `|`, `&&`, `||`, `;`, etc.).
    pub command_position: bool,
}

impl ScannerState {
    pub(super) fn new() -> Self {
        Self {
            mode_stack: vec![ScanMode::Normal],
            word_start: true,
            command_position: true,
        }
    }

    pub(super) fn current_mode(&self) -> &ScanMode {
        self.mode_stack.last().unwrap_or(&ScanMode::Normal)
    }

    pub(super) fn push_mode(&mut self, mode: ScanMode) {
        self.mode_stack.push(mode);
    }

    pub(super) fn pop_mode(&mut self) {
        if self.mode_stack.len() > 1 {
            self.mode_stack.pop();
        }
    }
}
```

**Note:** The `super::highlight::ColorSpan` import is for the next step (the `mark_unclosed_errors` function). Fields `mode_stack`, `word_start`, `command_position` change from private to `pub` (within the submodule) so callsites in mod.rs (`state.word_start = true`, etc.) keep working — `pub(super) struct` requires `pub` fields to be readable from the parent. You can also use `pub(super)` on each field for tighter visibility; either works.

- [ ] **Step 3: Move `mark_unclosed_errors` into state.rs as a free function**

The current method (in `mod.rs`) reads `&ScannerState` (specifically the mode stack) and writes to `&mut Vec<ColorSpan>`. It does not use `&self` for anything else. Convert to:

```rust
// Append to state.rs

/// Append unclosed-quote / unclosed-expansion error spans to `spans`.
/// Called from `scan_from` after the main scan loop completes; if any
/// non-Normal mode is still on the stack, the corresponding `start`
/// position gets an Error span.
pub(super) fn mark_unclosed_errors(state: &ScannerState, input_len: usize, spans: &mut Vec<ColorSpan>) {
    // [PASTE THE EXACT BODY of HighlightScanner::mark_unclosed_errors HERE,
    //  replacing all `self.<field>` references with the corresponding parameter
    //  (state, input_len, spans). The body uses state.mode_stack to walk
    //  outstanding modes and pushes ColorSpan::Error for each unclosed range.]
}
```

To get the exact body, run:

```bash
awk '/fn mark_unclosed_errors/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -60
```

and paste the body, mechanically rewriting:
- `self.` → (drop it; field accesses now go through the `state` parameter)
- references to outer `chars.len()` → `input_len`

Specifically, any `state.mode_stack.iter()` etc. stay as-is. The `&mut spans` parameter replaces any internal access through `self.spans`.

- [ ] **Step 4: Remove moved items from `mod.rs`**

In `src/interactive/highlight_scanner/mod.rs`, delete:

1. The `ScanMode` enum (lines ~10–20).
2. The `ScannerState` struct + `impl ScannerState` block (lines ~28–60).
3. The `mark_unclosed_errors` method on `HighlightScanner` (its `&mut self` form). Locate by name.
4. Their preceding section header comments.

- [ ] **Step 5: Update `mark_unclosed_errors` callsite in `mod.rs`**

The call site currently looks like:

```rust
self.mark_unclosed_errors(chars, &state, &mut spans);  // or similar
```

Replace with:

```rust
state::mark_unclosed_errors(&state, chars.len(), &mut spans);
```

- [ ] **Step 6: Add `mod state;` and reimports**

In `src/interactive/highlight_scanner/mod.rs`, add to the module-declaration block (alphabetized):

```rust
mod helpers;
mod state;

use helpers::*;
use state::{ScanMode, ScannerState};
```

(`mark_unclosed_errors` is called via the explicit path `state::mark_unclosed_errors` — no need to re-import.)

- [ ] **Step 7: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok`. Same totals.

```bash
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: no new warnings or errors. Possible warning: `unused import: ScanMode` if `mod.rs` no longer pattern-matches on it directly; if so, remove `ScanMode` from the use line.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): extract ScanMode/ScannerState to state.rs

SP3 PR-A step 3. Move ScanMode enum, ScannerState struct + impl,
and mark_unclosed_errors (now a pub(super) free fn taking explicit
&ScannerState + input_len + &mut spans) to a dedicated submodule.
Visibility tightened to pub(super).

The method-form mark_unclosed_errors becomes a free function
because its body never reads self.* — it only walks state.mode_stack
and pushes spans. Callsite in scan_from updated to invoke the
explicit path state::mark_unclosed_errors.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A4: Extract `cache.rs`

**Goal:** Move `HighlightCache` struct + impl to `cache.rs`. The cache is used by `HighlightScanner.cache` field and accessed in `scan_from` for checkpoint bookkeeping.

**Files:**
- Create: `src/interactive/highlight_scanner/cache.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (delete moved items, add `mod cache;` and `use cache::HighlightCache;`)

- [ ] **Step 1: Create `cache.rs` with the moved content:**

```rust
//! Incremental rescan cache for the highlight scanner.
//!
//! Stores the previous input + spans and a series of checkpoints
//! (state snapshots at fixed positions) so that an unchanged prefix
//! can be reused across keystrokes.

use super::highlight::ColorSpan;
use super::state::ScannerState;

pub(super) struct HighlightCache {
    pub prev_input: Vec<char>,
    pub prev_spans: Vec<ColorSpan>,
    pub checkpoints: Vec<(usize, ScannerState)>,
    pub checkpoint_interval: usize,
}

impl HighlightCache {
    // [PASTE THE EXACT impl HighlightCache BLOCK from mod.rs here.
    //  Inside the block, change every `pub fn` or private `fn` to `pub(super) fn`.
    //  Field accesses don't change because all fields are `pub` within submodule.]
}
```

To get the exact `impl` body:

```bash
awk '/^impl HighlightCache/,/^\}/' src/interactive/highlight_scanner/mod.rs | head -50
```

Apply visibility tightening: every method becomes `pub(super)`.

- [ ] **Step 2: Remove moved items from `mod.rs`**

In `src/interactive/highlight_scanner/mod.rs`, delete:

1. The `HighlightCache` struct definition.
2. The `impl HighlightCache { ... }` block.
3. Their preceding section header (`// HighlightCache`).

- [ ] **Step 3: Add `mod cache;` to `mod.rs`**

```rust
mod cache;
mod helpers;
mod state;

use cache::HighlightCache;
use helpers::*;
use state::{ScanMode, ScannerState};
```

- [ ] **Step 4: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: tests pass, no new warnings/errors.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): extract HighlightCache to cache.rs

SP3 PR-A step 4. Move HighlightCache struct + impl to a dedicated
submodule. Pure relocation; visibility tightened to pub(super).

scan_from in mod.rs continues to access self.cache.checkpoints
directly — fields are pub within the submodule hierarchy.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task A5: Capture Bench Baseline + Add `ctx.rs`

**Goal:** Run `cargo bench --bench interactive_smoke` once to capture pre-redesign throughput numbers. Then add `ctx.rs` defining the `ScanCtx` struct (no consumers yet — it's referenced only when scanners are converted in PR-B).

**Files:**
- Create: `src/interactive/highlight_scanner/ctx.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (add `mod ctx;`)
- Capture: bench output stored in working tree as `benches/data/sp3-baseline.txt` (gitignored or committed depending on project convention).

- [ ] **Step 1: Run bench baseline**

```bash
cargo bench --bench interactive_smoke 2>&1 | tee benches/data/sp3-baseline.txt | tail -20
```

This is slow (compiling release + running criterion benchmarks; 3–5 minutes). Run in background.

Expected: criterion output with throughput values like `time:   [X.XX µs Y.YY µs Z.ZZ µs]`. Capture the median for the main scan benchmark.

If `benches/data/` doesn't exist, `mkdir -p benches/data/` first.

- [ ] **Step 2: Read off the baseline numbers**

```bash
grep -E "scan_smoke|time:" benches/data/sp3-baseline.txt | head -10
```

Record the median throughput value (e.g., `time: [12.345 µs 12.456 µs 12.567 µs]`). The middle number is the median you'll compare against in Task B6.

- [ ] **Step 3: Create `ctx.rs` with the ScanCtx struct**

```rust
//! Per-scan context — the bundle of mutable state every scanner needs.
//!
//! Built fresh by `scan_from` each loop iteration and passed to the
//! per-mode scanner functions. Holds:
//! - `input` — the input character slice (read-only).
//! - `state` — the mode stack and word/command-position flags.
//! - `spans` — the output span vector under construction.
//! - `checker` — `CommandChecker`. Only `scan_word` reads it (to
//!   determine if a word is a known command), but it's bundled here
//!   to keep every scanner's signature uniform.

use super::cache::HighlightCache;
use super::command_checker::CommandChecker;
use super::highlight::ColorSpan;
use super::state::ScannerState;

#[allow(dead_code)] // HighlightCache reference held via mod.rs::scan_from, not via ScanCtx
pub(super) struct ScanCtx<'a> {
    pub input: &'a [char],
    pub state: &'a mut ScannerState,
    pub spans: &'a mut Vec<ColorSpan>,
    pub checker: &'a mut CommandChecker,
    _phantom: std::marker::PhantomData<&'a HighlightCache>,
}
```

**Wait — that's wrong.** ScanCtx doesn't hold a cache reference. Use the simpler form:

```rust
//! Per-scan context — the bundle of mutable state every scanner needs.
//!
//! Built fresh by `scan_from` each loop iteration and passed to the
//! per-mode scanner functions. Holds:
//! - `input` — the input character slice (read-only).
//! - `state` — the mode stack and word/command-position flags.
//! - `spans` — the output span vector under construction.
//! - `checker` — `CommandChecker`. Only `scan_word` reads it (to
//!   determine if a word is a known command), but it's bundled here
//!   to keep every scanner's signature uniform.

use super::command_checker::CommandChecker;
use super::highlight::ColorSpan;
use super::state::ScannerState;

pub(super) struct ScanCtx<'a> {
    pub input: &'a [char],
    pub state: &'a mut ScannerState,
    pub spans: &'a mut Vec<ColorSpan>,
    pub checker: &'a mut CommandChecker,
}
```

(The `command_checker` module is at `src/interactive/command_checker.rs` — a sibling of `highlight_scanner.rs`/`highlight_scanner/`. Path: `super::command_checker::CommandChecker` since `mod.rs` is `src/interactive/highlight_scanner/mod.rs`, so `super` is `src/interactive/`.)

- [ ] **Step 4: Add `mod ctx;` to `mod.rs`**

```rust
mod cache;
mod ctx;
mod helpers;
mod state;

use cache::HighlightCache;
use helpers::*;
use state::{ScanMode, ScannerState};
```

(Don't import `ScanCtx` in `mod.rs` yet — it isn't used until PR-B. But the module declaration is needed for the type to exist.)

- [ ] **Step 5: Verify**

```bash
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: 1 warning about `ScanCtx` being unused (`pub(super) struct ScanCtx<'_> ... never constructed`) — that's expected since PR-B hasn't started. The test suite still passes:

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok`.

If the unused-struct warning is bothersome, add `#[allow(dead_code)]` above the struct definition. The warning will go away in B1 when scanners start using it.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): introduce ScanCtx in ctx.rs (PR-A finale)

SP3 PR-A step 5 (final). Add ScanCtx<'a> struct with four fields:
input slice, &mut ScannerState, &mut Vec<ColorSpan>, &mut
CommandChecker. No consumers yet — every per-mode scanner gets
converted to take &mut ScanCtx in PR-B.

Bench baseline captured: benches/data/sp3-baseline.txt

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**PR-A is now complete.** Currently the file structure is:

```
src/interactive/highlight_scanner/
  mod.rs       — HighlightScanner, scan(), scan_from() + 11 scan_* methods + tests
  cache.rs     — HighlightCache
  ctx.rs       — ScanCtx (unused yet)
  helpers.rs   — KEYWORDS, is_keyword/is_*, etc.
  state.rs     — ScanMode, ScannerState, mark_unclosed_errors
```

`mod.rs` should be roughly 1100–1200 lines (down from 1594 by ~400 lines moved out). PR-B converts the 11 `scan_*` methods to free functions in their target submodules.

---

## Task B1: Extract Simple Scanners (5 functions to 4 files) — Part 1: `quotes.rs` and `comment.rs`

**Goal:** Move the 4 simplest scanners (already `Self::` associated functions, no `&mut self`) to their target files as free functions taking explicit (chars/state/spans/pos/payload) parameters. **ScanCtx is NOT yet used** — that conversion happens in B5. This task converts to free-function form, which is a strict subset of the final form.

The 4 scanners moved in this task:
- `scan_single_quote(chars, pos, start, state, spans) -> usize` → `quotes.rs`
- `scan_dollar_single_quote(chars, pos, start, state, spans) -> usize` → `quotes.rs`
- `scan_comment(chars, pos, start, state, spans) -> usize` → `comment.rs`

(`scan_double_quote` is `&mut self` because it … actually let me check. It might not be. Verify in the file before assuming. If it's also `Self::`, include it here. Otherwise defer to B3.)

For this task, scope is limited to scanners that take **no auxiliary** beyond chars/state/spans/pos/payload. `scan_parameter` (takes `braced`) and `scan_arith_sub` move in B2 alongside `scan_dollar` (which is `&mut self` and needs more care).

**Files:**
- Create: `src/interactive/highlight_scanner/quotes.rs`
- Create: `src/interactive/highlight_scanner/comment.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (delete moved methods, add `mod quotes; mod comment;`, update dispatcher)

- [ ] **Step 1: Verify `scan_double_quote` is `&mut self` (and excluded here)**

```bash
grep -n "fn scan_double_quote\|fn scan_single_quote\|fn scan_dollar_single_quote\|fn scan_comment\|fn scan_parameter\|fn scan_arith_sub" src/interactive/highlight_scanner/mod.rs
```

Note which take `&mut self` vs which are `Self::` (associated). Per Task 0 grep output:
- `Self::scan_single_quote(chars, pos, start, state, &mut spans)` — associated
- `self.scan_double_quote(chars, pos, start, state, &mut spans, checker_env)` — `&mut self` (excluded from B1)
- `Self::scan_dollar_single_quote(chars, pos, start, state, &mut spans)` — associated
- `Self::scan_parameter(chars, pos, start, braced, state, &mut spans)` — associated (but in B2 with scan_dollar)
- `Self::scan_arith_sub(chars, pos, start, state, &mut spans)` — associated (in B2)
- `Self::scan_comment(chars, pos, start, state, &mut spans)` — associated (in B1)

So B1 covers: scan_single_quote, scan_dollar_single_quote, scan_comment.

- [ ] **Step 2: Create `quotes.rs` with the moved scanners**

```rust
//! Quoted-string scanners: single-quote, double-quote, dollar-single-quote.
//!
//! Each scanner is a free function. Takes the input slice, current pos,
//! the variant payload (`start` of the opening quote), shared state,
//! and the span accumulator.

use super::highlight::{ColorSpan, HighlightStyle};
use super::state::{ScanMode, ScannerState};

// [PASTE THE EXACT body of HighlightScanner::scan_single_quote here, transformed:
//  - Replace `fn scan_single_quote(chars: &[char], pos: usize, start: usize, state: &mut ScannerState, spans: &mut Vec<ColorSpan>) -> usize`
//    (it was already an associated function, so no `self`).
//  - Add `pub(super)` visibility.]

// scan_double_quote will land here in Task B3.

// [PASTE the body of scan_dollar_single_quote similarly with pub(super).]
```

To get the exact bodies:

```bash
awk '/fn scan_single_quote/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -40
awk '/fn scan_dollar_single_quote/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -45
```

Copy into `quotes.rs`. Apply visibility (`pub(super)`) and remove the leading 4-space indentation that came from being inside `impl HighlightScanner`.

The resulting `quotes.rs` (without `scan_double_quote` yet) should be ~70 lines.

- [ ] **Step 3: Create `comment.rs` similarly**

```rust
//! Comment scanner — handles a `#`-comment that starts at the beginning
//! of a word and runs to the end of the input.

use super::highlight::{ColorSpan, HighlightStyle};
use super::state::ScannerState;

// [PASTE the body of HighlightScanner::scan_comment with pub(super) visibility,
//  removing the leading impl-block indentation.]
```

To get the body:

```bash
awk '/fn scan_comment/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -30
```

Result: ~25–30 lines.

- [ ] **Step 4: Remove the moved methods from `mod.rs`**

In `src/interactive/highlight_scanner/mod.rs`, delete:

1. `fn scan_single_quote(...)` (the entire associated function, ~30 lines).
2. `fn scan_dollar_single_quote(...)` (~40 lines).
3. `fn scan_comment(...)` (~25 lines).

- [ ] **Step 5: Update the dispatcher in `scan_from`**

In `mod.rs`, find the `match state.current_mode().clone()` block in `scan_from`. Replace these three arms:

```rust
ScanMode::SingleQuote { start } => {
    pos = Self::scan_single_quote(chars, pos, start, state, &mut spans);
}
ScanMode::DollarSingleQuote { start } => {
    pos = Self::scan_dollar_single_quote(chars, pos, start, state, &mut spans);
}
ScanMode::Comment { start } => {
    pos = Self::scan_comment(chars, pos, start, state, &mut spans);
}
```

with:

```rust
ScanMode::SingleQuote { start } => {
    pos = quotes::scan_single_quote(chars, pos, start, state, &mut spans);
}
ScanMode::DollarSingleQuote { start } => {
    pos = quotes::scan_dollar_single_quote(chars, pos, start, state, &mut spans);
}
ScanMode::Comment { start } => {
    pos = comment::scan_comment(chars, pos, start, state, &mut spans);
}
```

Other arms remain unchanged for now.

- [ ] **Step 6: Add `mod quotes;` and `mod comment;` to mod.rs**

```rust
mod cache;
mod comment;
mod ctx;
mod helpers;
mod quotes;
mod state;

use cache::HighlightCache;
use helpers::*;
use state::{ScanMode, ScannerState};
```

- [ ] **Step 7: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: tests still pass; only the pre-existing `ScanCtx unused` warning if not yet silenced.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): extract simple scanners (single_quote, dollar_single_quote, comment)

SP3 PR-B step 1. Move three associated scan_* functions to their
target submodules as pub(super) free functions. Dispatcher in
scan_from updated to call them via the explicit module path.

scan_double_quote stays in mod.rs for now — it takes &mut self
and will move in B3 once scan_dollar (which it calls) is also
free.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task B2: Extract `scan_parameter`, `scan_arith_sub` to `expansion.rs` (without `scan_dollar` yet)

**Goal:** Move two more associated `Self::` scanners to `expansion.rs`. `scan_dollar` (the third expansion-related function) is `&mut self` because it calls back into `self.scan_normal` recursively in some shells — verify the call pattern, but treat as separate task (B3).

**Files:**
- Create: `src/interactive/highlight_scanner/expansion.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (delete moved methods, add `mod expansion;`, update dispatcher)

- [ ] **Step 1: Create `expansion.rs` with two scanners**

```rust
//! Dollar-expansion scanners: $variable, ${braced}, $((arith)).
//!
//! Each scanner is a free function. scan_dollar (top-level $-detector
//! that branches into the others) lands here in Task B3.

use super::highlight::{ColorSpan, HighlightStyle};
use super::helpers::is_valid_name;
use super::state::{ScanMode, ScannerState};

// [PASTE scan_parameter body with pub(super) visibility and dropped indent.]

// scan_dollar will land here in Task B3.

// [PASTE scan_arith_sub body with pub(super) visibility and dropped indent.]
```

Bodies:

```bash
awk '/fn scan_parameter/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -35
awk '/fn scan_arith_sub/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -32
```

Result: `expansion.rs` ~70–80 lines (without `scan_dollar`).

- [ ] **Step 2: Remove the two methods from `mod.rs`**

Delete:
1. `fn scan_parameter(chars, pos, start, braced, state, spans)`.
2. `fn scan_arith_sub(chars, pos, start, state, spans)`.

- [ ] **Step 3: Update dispatcher arms in `scan_from`**

Replace:

```rust
ScanMode::Parameter { start, braced } => {
    pos = Self::scan_parameter(chars, pos, start, braced, state, &mut spans);
}
ScanMode::ArithSub { start } => {
    pos = Self::scan_arith_sub(chars, pos, start, state, &mut spans);
}
```

with:

```rust
ScanMode::Parameter { start, braced } => {
    pos = expansion::scan_parameter(chars, pos, start, braced, state, &mut spans);
}
ScanMode::ArithSub { start } => {
    pos = expansion::scan_arith_sub(chars, pos, start, state, &mut spans);
}
```

- [ ] **Step 4: Add `mod expansion;` to mod.rs**

```rust
mod cache;
mod comment;
mod ctx;
mod expansion;
mod helpers;
mod quotes;
mod state;
```

- [ ] **Step 5: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): extract scan_parameter and scan_arith_sub to expansion.rs

SP3 PR-B step 2. Move two associated dollar-expansion scanners to
expansion.rs as pub(super) free functions. scan_dollar (which is
&mut self because it bridges into the dispatcher) lands here in
the next task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task B3: Convert `scan_word`, `scan_dollar`, `scan_double_quote` to Free Functions

**Goal:** Convert the three `&mut self` scanners to free functions. These are linked by call relationships:
- `scan_normal` calls `self.scan_dollar` and `self.scan_word` — but `scan_normal` is still in `mod.rs` after this task.
- `scan_double_quote` may call `scan_dollar` inline — verify and update accordingly.

After this task, scan_normal still calls these via the new module paths (e.g., `expansion::scan_dollar(...)`). scan_normal itself moves in B4.

**Files:**
- Create: `src/interactive/highlight_scanner/word.rs`
- Modify: `src/interactive/highlight_scanner/quotes.rs` (add `scan_double_quote`)
- Modify: `src/interactive/highlight_scanner/expansion.rs` (add `scan_dollar`)
- Modify: `src/interactive/highlight_scanner/mod.rs` (remove three methods, update dispatcher and `scan_normal`'s callsites)

- [ ] **Step 1: Verify `self.*` usage in each function body**

```bash
awk 'NR>=524 && NR<=614 && /self\./' src/interactive/highlight_scanner/mod.rs   # scan_dollar
awk 'NR>=614 && NR<=733 && /self\./' src/interactive/highlight_scanner/mod.rs   # scan_word
awk 'NR>=761 && NR<=946 && /self\./' src/interactive/highlight_scanner/mod.rs   # scan_double_quote
```

Note ALL `self.*` accesses in each. Pre-known:
- `scan_word`: `self.checker.check(&word, checker_env)` — needs `&mut checker` parameter.
- `scan_dollar`: zero `self.*` accesses (verified in plan-prep). Convert without checker.
- `scan_double_quote`: needs verification. If it uses only state/spans/pos, no extra param needed. If it uses `self.checker` or other, add accordingly.

**If `scan_double_quote` calls `self.scan_dollar`**, the converted form will call `expansion::scan_dollar(...)` directly. (After this task, scan_dollar is a free function in expansion.rs.)

- [ ] **Step 2: Create `word.rs` with `scan_word` as a free function**

```rust
//! Unquoted-word scanner — collects a word from `pos` until a word break,
//! classifies it (keyword / command / argument / variable / etc.), and
//! emits a span. The only scanner that needs `CommandChecker` access
//! to determine command existence.

use super::command_checker::{CheckerEnv, CommandChecker};
use super::helpers::{
    COMMAND_POSITION_KEYWORDS, is_keyword, is_operator_char, is_redirect_start, is_valid_name,
    is_word_break,
};
use super::highlight::{ColorSpan, HighlightStyle};
use super::state::ScannerState;

pub(super) fn scan_word(
    chars: &[char],
    pos: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    checker: &mut CommandChecker,
    checker_env: &CheckerEnv,
) -> usize {
    // [PASTE THE EXACT body of HighlightScanner::scan_word here, replacing
    //  every `self.checker.check(&word, checker_env)` with
    //  `checker.check(&word, checker_env)`. Remove leading impl-block indent.]
}
```

To get the body:

```bash
awk '/fn scan_word/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -130
```

Mechanically rewrite `self.checker` → `checker` (the new parameter). No other `self.*` references should exist.

- [ ] **Step 3: Add `scan_dollar` to `expansion.rs`**

```rust
// Append to expansion.rs

pub(super) fn scan_dollar(
    chars: &[char],
    pos: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    _checker_env: &CheckerEnv,
) -> usize {
    // [PASTE the EXACT body of HighlightScanner::scan_dollar with pub(super)
    //  visibility, dropped indent. No `self.*` access exists, so no
    //  parameter rewrites needed. The `_checker_env` parameter is for
    //  uniform calling convention with other scanners; it's unused here.]
}
```

The `use super::command_checker::CheckerEnv;` import needs adding to `expansion.rs`'s top.

- [ ] **Step 4: Add `scan_double_quote` to `quotes.rs`**

This one's signature includes `checker_env`:

```rust
// Append to quotes.rs

use super::command_checker::CheckerEnv;
// (Add this use if not already present)

pub(super) fn scan_double_quote(
    chars: &[char],
    pos: usize,
    start: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    checker_env: &CheckerEnv,
) -> usize {
    // [PASTE the EXACT body of HighlightScanner::scan_double_quote with
    //  pub(super) visibility, dropped indent. If the body calls
    //  `self.scan_dollar(chars, pos, state, spans, checker_env)`, replace
    //  with `super::expansion::scan_dollar(chars, pos, state, spans,
    //  checker_env)` (the path to expansion::scan_dollar from quotes.rs).]
}
```

To get the body:

```bash
awk '/fn scan_double_quote/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -190
```

Verify and rewrite any internal `self.scan_dollar` calls.

- [ ] **Step 5: Remove the three methods from `mod.rs`**

In `src/interactive/highlight_scanner/mod.rs`, delete:
1. `fn scan_word(&mut self, ...)` method (~120 lines).
2. `fn scan_dollar(&mut self, ...)` method (~90 lines).
3. `fn scan_double_quote(&mut self, ...)` method (~185 lines).

Verify by searching:

```bash
grep -n "fn scan_word\|fn scan_dollar[^_]\|fn scan_double_quote" src/interactive/highlight_scanner/mod.rs
```

Expected: zero hits.

- [ ] **Step 6: Update `scan_normal`'s callsites in `mod.rs`**

In `mod.rs`, `scan_normal` (still a method at this point) calls:

```rust
return self.scan_dollar(chars, pos, state, spans, checker_env);
// ...
self.scan_word(chars, pos, state, spans, checker_env)
```

Replace with:

```rust
return expansion::scan_dollar(chars, pos, state, spans, checker_env);
// ...
word::scan_word(chars, pos, state, spans, &mut self.checker, checker_env)
```

Note that `scan_word`'s call site needs `&mut self.checker` as the new parameter.

- [ ] **Step 7: Update dispatcher arm for DoubleQuote**

In `mod.rs::scan_from`, replace:

```rust
ScanMode::DoubleQuote { start } => {
    pos = self.scan_double_quote(chars, pos, start, state, &mut spans, checker_env);
}
```

with:

```rust
ScanMode::DoubleQuote { start } => {
    pos = quotes::scan_double_quote(chars, pos, start, state, &mut spans, checker_env);
}
```

- [ ] **Step 8: Add `mod word;` to mod.rs**

```rust
mod cache;
mod comment;
mod ctx;
mod expansion;
mod helpers;
mod quotes;
mod state;
mod word;
```

- [ ] **Step 9: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: tests pass. Compile may surface borrow-checker issues from passing `&mut self.checker` while `self.cache.checkpoints` is also used elsewhere in `scan_from`. If so, **report DONE_WITH_CONCERNS** and let the controller diagnose.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): convert scan_word, scan_dollar, scan_double_quote to free fns

SP3 PR-B step 3. Three &mut self scanners become pub(super) free
functions in their target submodules. scan_word picks up
&mut CommandChecker as an explicit parameter (was self.checker
inside the impl-method form). scan_double_quote's internal
self.scan_dollar call rewires to super::expansion::scan_dollar.

scan_normal in mod.rs is updated to call the new free-function
forms. scan_normal itself stays in mod.rs for one more task and
moves in B4.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task B4: Convert `scan_normal` to Free Function in `normal.rs`

**Goal:** The last `&mut self` scanner moves out of `mod.rs`. After this task, all 11 scanner functions are free `pub(super) fn`s in their target files; `mod.rs` holds only `HighlightScanner`, `scan` (public), and `scan_from` (dispatcher).

**Files:**
- Create: `src/interactive/highlight_scanner/normal.rs`
- Modify: `src/interactive/highlight_scanner/mod.rs` (remove `scan_normal` method, update dispatcher)

- [ ] **Step 1: Verify `scan_normal` body's `self.*` accesses**

```bash
awk 'NR>=330 && NR<=524 && /self\./' src/interactive/highlight_scanner/mod.rs | head -20
```

Note all `self.*` calls. Per Task B3, `self.scan_dollar` and `self.scan_word` were already replaced by `expansion::scan_dollar` and `word::scan_word(..., &mut self.checker, ...)`. Other `self.*` access should be zero — but verify.

If the only `self.*` access is `self.checker` (passed to scan_word), the conversion is straightforward: take `checker: &mut CommandChecker` as a parameter. The dispatcher passes `&mut self.checker` from `scan_from`.

- [ ] **Step 2: Create `normal.rs`**

```rust
//! Top-level scanner for normal (unquoted, unstacked) shell-syntax mode.
//!
//! Handles whitespace, operators (`|`, `&&`, `||`, `;`, `&`), redirects
//! (`<`, `>`, `>>`, etc.), opening of quotes/expansions/comments
//! (delegates by pushing onto state.mode_stack), and falls through to
//! scan_word for unquoted words and scan_dollar for `$` expansions.

use super::command_checker::{CheckerEnv, CommandChecker};
use super::expansion;
use super::helpers::{is_operator_char, is_redirect_start};
use super::highlight::{ColorSpan, HighlightStyle};
use super::state::{ScanMode, ScannerState};
use super::word;

pub(super) fn scan_normal(
    chars: &[char],
    pos: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    checker: &mut CommandChecker,
    checker_env: &CheckerEnv,
) -> usize {
    // [PASTE the EXACT body of HighlightScanner::scan_normal here, with these
    //  rewrites:
    //  - `self.scan_dollar(chars, pos, state, spans, checker_env)` →
    //    `expansion::scan_dollar(chars, pos, state, spans, checker_env)`
    //  - `self.scan_word(chars, pos, state, spans, checker_env)` →
    //    `word::scan_word(chars, pos, state, spans, checker, checker_env)`
    //  - Drop leading impl-block indentation.]
}
```

To get the body:

```bash
awk '/fn scan_normal/,/^    \}$/' src/interactive/highlight_scanner/mod.rs | head -200
```

- [ ] **Step 3: Remove `scan_normal` from `mod.rs`**

Delete the `fn scan_normal(&mut self, ...)` method.

- [ ] **Step 4: Update dispatcher**

In `mod.rs::scan_from`, replace:

```rust
ScanMode::Normal => {
    pos = self.scan_normal(chars, pos, state, &mut spans, checker_env);
}
```

with:

```rust
ScanMode::Normal => {
    pos = normal::scan_normal(chars, pos, state, &mut spans, &mut self.checker, checker_env);
}
```

- [ ] **Step 5: Add `mod normal;` to mod.rs**

```rust
mod cache;
mod comment;
mod ctx;
mod expansion;
mod helpers;
mod normal;
mod quotes;
mod state;
mod word;
```

- [ ] **Step 6: Verify**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
cargo build 2>&1 | grep -E "warning|error" | head -5
```
Expected: tests pass. Possible borrow-checker error from `&mut self.checker` while iterating in `scan_from`. If so, look at the dispatch loop body — the borrow of `self.checker` is held only for the duration of the function call, so the loop body should release it on each iteration. Likely fine.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): convert scan_normal to free fn in normal.rs

SP3 PR-B step 4. The last &mut self scanner moves out of mod.rs.
After this commit, all 11 scanner functions are pub(super) free
functions distributed across normal/word/quotes/expansion/comment
submodules. mod.rs holds only HighlightScanner, scan() public API,
and scan_from() dispatcher.

scan_normal's internal self.scan_dollar / self.scan_word calls
rewire to expansion::scan_dollar / word::scan_word.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task B5: Refactor All Scanner Signatures to Use `ScanCtx`

**Goal:** Replace the `(chars, pos, state, spans, [checker, env])` parameter list on every scanner function with `(ctx: &mut ScanCtx, env: &CheckerEnv, pos, [payload])`. The `ScanCtx` is built once in `scan_from` and reborrowed each loop iteration. This is the redesign body — the actual `ScanCtx` introduction.

**Files:**
- Modify: every submodule (`normal.rs`, `word.rs`, `quotes.rs`, `expansion.rs`, `comment.rs`).
- Modify: `src/interactive/highlight_scanner/mod.rs::scan_from` (build and reborrow ScanCtx).

- [ ] **Step 1: Update each scanner signature**

For each scanner, change the parameter list from:

```rust
pub(super) fn scan_X(
    chars: &[char],
    pos: usize,
    [start: usize,] [braced: bool,]
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    [checker: &mut CommandChecker,]
    checker_env: &CheckerEnv,
) -> usize { /* body uses chars, state, spans, [checker], [env] */ }
```

to:

```rust
pub(super) fn scan_X(
    ctx: &mut ScanCtx<'_>,
    env: &CheckerEnv<'_>,
    pos: usize,
    [start: usize,] [braced: bool,]
) -> usize {
    // body — replace:
    //   chars   → ctx.input
    //   state   → ctx.state (read) / &mut *ctx.state (when a method call needs &mut)
    //   spans   → ctx.spans
    //   checker → ctx.checker  (only scan_word)
    //   checker_env → env
    // For sub-calls: e.g., expansion::scan_dollar(chars, pos, state, spans, env)
    //   becomes expansion::scan_dollar(ctx, env, pos)
}
```

The `&mut ctx.state` reborrow may surface if a method call needs `&mut ScannerState`. Use `&mut *ctx.state` to spell it out. Or use `let state = &mut *ctx.state; state.push_mode(...)` style for clarity.

- [ ] **Step 2: Apply to each file in turn**

For each of the 5 submodule files, perform the signature rewrite for every `pub(super) fn scan_*` it contains. Use `Edit` tool with exact `old_string`/`new_string` for each function header AND its body where it accesses `chars` / `state` / `spans` / `checker` / `checker_env`.

To save time, do all 5 files together as one edit pass before recompiling. Compile-error feedback comes from `cargo build` — fix any missed access pattern.

- [ ] **Step 3: Update `scan_from` to build `ScanCtx`**

In `mod.rs`, rewrite `scan_from` to build `ScanCtx` once per loop iteration:

```rust
fn scan_from(
    &mut self,
    chars: &[char],
    start_pos: usize,
    state: &mut ScannerState,
    checker_env: &CheckerEnv,
) -> Vec<ColorSpan> {
    let mut spans = Vec::new();
    let mut pos = start_pos;

    self.cache.checkpoints.retain(|(cp, _)| *cp < start_pos);
    if start_pos == 0 {
        self.cache.checkpoints.push((0, state.clone()));
    }

    while pos < chars.len() {
        if pos > 0
            && pos.is_multiple_of(self.cache.checkpoint_interval)
            && !self.cache.checkpoints.iter().any(|(cp, _)| *cp == pos)
        {
            self.cache.checkpoints.push((pos, state.clone()));
        }

        let mut ctx = ScanCtx {
            input: chars,
            state,
            spans: &mut spans,
            checker: &mut self.checker,
        };
        pos = match ctx.state.current_mode().clone() {
            ScanMode::Normal => normal::scan_normal(&mut ctx, checker_env, pos),
            ScanMode::SingleQuote { start } => quotes::scan_single_quote(&mut ctx, checker_env, pos, start),
            ScanMode::DoubleQuote { start } => quotes::scan_double_quote(&mut ctx, checker_env, pos, start),
            ScanMode::DollarSingleQuote { start } => quotes::scan_dollar_single_quote(&mut ctx, checker_env, pos, start),
            ScanMode::Parameter { start, braced } => expansion::scan_parameter(&mut ctx, checker_env, pos, start, braced),
            ScanMode::ArithSub { start } => expansion::scan_arith_sub(&mut ctx, checker_env, pos, start),
            ScanMode::Comment { start } => comment::scan_comment(&mut ctx, checker_env, pos, start),
            ScanMode::CommandSub { .. } => { ctx.state.pop_mode(); pos }
            ScanMode::Backtick { .. } => { ctx.state.pop_mode(); pos }
        };
    }

    state::mark_unclosed_errors(state, chars.len(), &mut spans);

    spans
}
```

**Borrow-checker note:** the `let mut ctx = ScanCtx { ... }` inside the loop reborrows `state` and `self.checker` and `spans` for one iteration. The `self.cache.checkpoints.push(...)` calls happen BEFORE the ctx is built each iteration, so there's no overlap. This pattern works because Rust's NLL drops the ctx borrow at the end of the match expression.

If the borrow checker complains about `state` being borrowed twice (once via `state` directly for `state.clone()` calls and once via `ctx.state`), restructure: either move the `clone` calls out of the loop body's preamble or store the clone in a local before constructing ctx. The exact restructuring depends on actual error messages — diagnose at compile time.

- [ ] **Step 4: Update `mod.rs` imports**

```rust
mod cache;
mod comment;
mod ctx;
mod expansion;
mod helpers;
mod normal;
mod quotes;
mod state;
mod word;

use cache::HighlightCache;
use ctx::ScanCtx;
use helpers::*;
use state::{ScanMode, ScannerState};
```

- [ ] **Step 5: Verify**

```bash
cargo build 2>&1 | tail -20
```
Expected: clean build. If borrow errors surface, address them iteratively.

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: tests pass with same totals.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(highlight_scanner): switch all scanners to &mut ScanCtx signature

SP3 PR-B step 5 (final extraction). Every scanner now takes
&mut ScanCtx<'_> + &CheckerEnv + pos [+payload]. The dispatcher
in scan_from builds ScanCtx fresh each loop iteration with
references to state, spans, checker, and the input slice.

This is the responsibility-redesign body — scanner bodies are now
decoupled from HighlightScanner internals. They access only the
state and outputs they actually need, through one uniform context.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**PR-B is now complete** at the structural level. Performance verification follows in Task B6.

---

## Task B6: Bench Verification (±5% Threshold)

**Goal:** Re-run `cargo bench --bench interactive_smoke` and compare against the baseline captured in Task A5. SP3 DoD #10 requires throughput within ±5% of pre-redesign.

**Files:**
- Capture: `benches/data/sp3-after-prb.txt` (post-redesign bench output).

- [ ] **Step 1: Run bench**

```bash
cargo bench --bench interactive_smoke 2>&1 | tee benches/data/sp3-after-prb.txt | tail -20
```

This is slow (3–5 minutes). Run in background.

- [ ] **Step 2: Compare medians**

```bash
diff benches/data/sp3-baseline.txt benches/data/sp3-after-prb.txt | head -30
echo "---baseline---"
grep -E "scan_smoke|time:" benches/data/sp3-baseline.txt | head -5
echo "---after PR-B---"
grep -E "scan_smoke|time:" benches/data/sp3-after-prb.txt | head -5
```

Compute the percent change manually:
- baseline median: T0 (e.g., 12.456 µs)
- after-PR-B median: T1
- delta = (T1 - T0) / T0 × 100

| delta | action |
|---|---|
| within ±5% | PASS — proceed to PR-C |
| > +5% (regression) | escalate; attempt to identify hot-path inefficiency |
| > -5% (improvement) | great; verify the bench still measures what it should (sanity check) |

If delta > +5%, common culprits:
- Extra heap allocations in ScanCtx construction (shouldn't happen — all four fields are references).
- Loss of inlining due to function-vs-method call site changes (Rust usually inlines fine; verify with `cargo bench` profile).
- Extra reborrow checks (negligible at runtime).

If escalation needed: report DONE_WITH_CONCERNS with the bench delta and the most-affected criterion benchmark.

- [ ] **Step 3: No commit (data files only)**

The bench output files in `benches/data/` are reference artifacts. They can be committed if the project's convention permits (some projects gitignore them; others commit). Default: commit, since they document the verification.

```bash
git add benches/data/sp3-baseline.txt benches/data/sp3-after-prb.txt
git commit -m "$(cat <<'EOF'
bench(sp3): record interactive_smoke before/after PR-B redesign

SP3 DoD #10. Capture criterion output for scan throughput pre and
post the ScanCtx-based scanner refactor. Verifies that converting
&mut self methods to free functions taking &mut ScanCtx does not
exceed the ±5% threshold.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If the project gitignores `benches/data/`, skip the commit and just keep the files locally for the verification report.

---

## Task C1: Move `test_checker_*` Tests to `command_checker.rs`

**Goal:** The 6 `test_checker_*` tests in `highlight_scanner/mod.rs::tests` exercise `CommandChecker` and `CheckerEnv` from `command_checker.rs` — they belong there, not in highlight_scanner. PR-C is unrelated cleanup that becomes natural once the scanner module is well-organized.

**Files:**
- Modify: `src/interactive/command_checker.rs` (add `#[cfg(test)] mod tests` with the 6 tests)
- Modify: `src/interactive/highlight_scanner/mod.rs` (delete the 6 tests + their helper imports if no longer needed)

- [ ] **Step 1: Locate the 6 tests in `mod.rs`**

```bash
grep -n "fn test_checker_" src/interactive/highlight_scanner/mod.rs
```

Expected: 6 hits — `test_checker_builtin_special`, `test_checker_alias`, `test_checker_path_search`, `test_checker_path_cache_invalidation`, `test_checker_direct_path`, `test_checker_path_with_tempfile`.

- [ ] **Step 2: Inspect each test for dependencies**

Use Read with offset/limit to view each test. Tests likely use:
- `CommandChecker::new()`, `CommandChecker::check(...)`
- `CheckerEnv` (constructed via a helper or directly)
- `make_aliases`, `checker_env` (test-side helper functions in mod.rs)
- `tempfile` crate for filesystem-related tests
- `crate::env::AliasStore` for alias tests

Note any helper functions (`make_aliases`, `checker_env`, etc.) that the tests reference. These helpers themselves should move to `command_checker.rs::tests` if they're not used by other tests. If they ARE used by other tests (e.g., `test_scan_*`), KEEP them in `mod.rs::tests` and DUPLICATE simpler equivalents in `command_checker.rs::tests`.

- [ ] **Step 3: Add `#[cfg(test)] mod tests` block to `command_checker.rs`**

Append to the end of `src/interactive/command_checker.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // [Add other imports as needed by the tests — likely `use crate::env::AliasStore;`,
    //  `tempfile::TempDir`, etc. — match what the tests reference.]

    // [PASTE all 6 test functions verbatim, plus any small helper functions
    //  they need (make_aliases, checker_env, etc.), with the same body.
    //  Drop the leading 4-space indent that came from being inside the
    //  highlight_scanner mod.rs::tests block.]
}
```

To get the test bodies:

```bash
awk '/fn test_checker_builtin_special/,/fn test_checker_path_with_tempfile/' src/interactive/highlight_scanner/mod.rs | head -200
```

Plus any helper function bodies referenced.

- [ ] **Step 4: Remove the 6 tests from `mod.rs::tests`**

In `src/interactive/highlight_scanner/mod.rs`, delete the 6 test functions. If `mod.rs::tests` had helpers used ONLY by these 6 tests (e.g., a CheckerEnv-specific builder), delete those too.

If a helper (e.g., `make_aliases`) is also used by `test_scan_*` tests (which stay in mod.rs), keep it and duplicate a minimal version into `command_checker.rs::tests`.

- [ ] **Step 5: Verify**

```bash
cargo test --lib command_checker 2>&1 | tail -10
```
Expected: 6 tests pass, all `command_checker::tests::test_checker_*`.

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok`. Total still equal to baseline (tests moved within lib binary, count preserved).

```bash
grep -n "test_checker_" src/interactive/highlight_scanner/mod.rs
```
Expected: zero hits.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(command_checker): move test_checker_* tests from highlight_scanner

SP3 PR-C. The 6 test_checker_* tests exercised CommandChecker and
CheckerEnv but lived in highlight_scanner/mod.rs::tests for
historical reasons. Move them to command_checker.rs::tests where
they actually belong.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**PR-C is now complete.**

---

## Task D1: Final Verification + TODO/CLAUDE Cleanup

**Goal:** Run every Definition-of-Done check, deal with any clippy issues, update documentation references, and produce the final commit-graph summary.

**Files:**
- Modify (possibly): `TODO.md` (if any entry references `src/interactive/highlight_scanner.rs` directly).
- Modify (possibly): files in `src/interactive/highlight_scanner/` if `cargo fmt --check` rewraps.

- [ ] **Step 1: File inventory and line counts**

```bash
ls -la src/interactive/highlight_scanner/
wc -l src/interactive/highlight_scanner/*.rs
```

Expected: 10 .rs files. Each ≤ 400 lines (umbrella DoD #6). Likely sizes (per spec):
- mod.rs ~150–200, normal.rs ~200, quotes.rs ~250, expansion.rs ~165, word.rs ~135, state.rs ~120, cache.rs ~80, helpers.rs ~70, ctx.rs ~30, comment.rs ~30.

If any file exceeds 400, deal with it (trim test-section headers as in SP2's spec.rs treatment, or document as exception).

- [ ] **Step 2: cargo fmt check**

```bash
cargo fmt --check 2>&1 | head -30
```

If diffs, run `cargo fmt` to apply, stage for cleanup commit.

- [ ] **Step 3: Run lib + integration tests**

```bash
cargo test --lib --features test-helpers 2>&1 | grep "test result"
```
Expected: 4 lines, all `ok`.

- [ ] **Step 4: Run plugin integration tests**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -3
```
Expected: 24/24 pass.

- [ ] **Step 5: Run e2e suite**

```bash
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected: 393/393 pass.

- [ ] **Step 6: Run pty interactive tests 3x**

```bash
for i in 1 2 3; do
    echo "=== Run $i ==="
    cargo test --test pty_interactive 2>&1 | tail -3
done
```
Expected: each run 0 failures. **Any single-run failure → escalate.**

- [ ] **Step 7: Bench compile**

```bash
cargo bench --no-run 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 8: Run clippy**

```bash
cargo clippy --all-targets -- -D warnings 2>&1 | tail -30
```
Expected: only pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99`.

If new clippy issues from `src/interactive/highlight_scanner/`, fix in a separate commit before D1 finalization.

- [ ] **Step 9: Verify TODO/CLAUDE/README references**

```bash
rg -n "highlight_scanner\.rs|interactive::highlight_scanner" TODO.md CLAUDE.md README.md 2>/dev/null
```

Update any reference to single-file `src/interactive/highlight_scanner.rs` → either drop the trailing `.rs` (the directory is the new module) or rewrite as `src/interactive/highlight_scanner/mod.rs`. The umbrella spec mentions `highlight_scanner.rs` — that's an OK reference to the original target since the file is now the module's `mod.rs` (interpretable equivalently).

If TODO.md has any entry specifically about splitting highlight_scanner.rs, remove it (per umbrella DoD #7).

- [ ] **Step 10: Verify external callers compile**

```bash
cargo build --bins 2>&1 | tail -3
rg -n "use crate::interactive::highlight_scanner|HighlightScanner" --type rust src/ | head -10
```
Expected: same import sites as Task 0 baseline.

- [ ] **Step 11: rustdoc check**

```bash
cargo doc --no-deps --document-private-items 2>&1 | tail -3
ls target/doc/yosh/interactive/highlight_scanner/
```

Expected: `Finished`. Listing should include `index.html` plus submodule pages for `cache`, `comment`, `ctx`, `expansion`, `helpers`, `normal`, `quotes`, `state`, `word`. The public type `HighlightScanner` should be documented.

- [ ] **Step 12: Commit fmt/clippy cleanup if any**

```bash
git status
```

If changes:

```bash
git add -A
git commit -m "$(cat <<'EOF'
style: apply rustfmt and clippy fixes after SP3 redesign

SP3 final cleanup. Applies any line-wrap differences cargo fmt
flagged after the responsibility split, plus any minor clippy
suggestions in newly-added code. No semantic change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If clean, skip.

- [ ] **Step 13: Final commit graph**

```bash
git log --oneline -20
```

Expected (most recent first, approximately):
- (optional) `style: apply rustfmt...` (D1 cleanup)
- `bench(sp3): record interactive_smoke before/after PR-B redesign` (B6, optional)
- `refactor(command_checker): move test_checker_* tests from highlight_scanner` (C1)
- `refactor(highlight_scanner): switch all scanners to &mut ScanCtx signature` (B5)
- `refactor(highlight_scanner): convert scan_normal to free fn in normal.rs` (B4)
- `refactor(highlight_scanner): convert scan_word, scan_dollar, scan_double_quote to free fns` (B3)
- `refactor(highlight_scanner): extract scan_parameter and scan_arith_sub to expansion.rs` (B2)
- `refactor(highlight_scanner): extract simple scanners (single_quote, dollar_single_quote, comment)` (B1)
- `refactor(highlight_scanner): introduce ScanCtx in ctx.rs (PR-A finale)` (A5)
- `refactor(highlight_scanner): extract HighlightCache to cache.rs` (A4)
- `refactor(highlight_scanner): extract ScanMode/ScannerState to state.rs` (A3)
- `refactor(highlight_scanner): extract char classification helpers to helpers.rs` (A2)
- `refactor(highlight_scanner): convert highlight_scanner.rs to mod.rs` (A1)

11–13 SP3 commits total. **All DoD criteria met. SP3 is complete.**

---

## Risk Mitigation Notes

These risks were called out in the SP3 spec; the plan's defenses are listed inline:

| Risk | Defense |
|---|---|
| `mark_unclosed_errors` accesses `&ScannerState` and `&mut Vec<ColorSpan>` | Task A3 converts to `pub(super) fn` taking explicit refs; callsite in `scan_from` invokes via `state::mark_unclosed_errors`. |
| `HighlightScanner.spans` and `.state` cannot both be `&mut` borrowed via `&mut self` | Task B5 builds `ScanCtx` per loop iteration; the four fields are disjoint pieces of state, so reborrowing is straightforward. NLL drops each iteration's ctx at the end of the match. |
| Public API breakage at `highlight.rs:11` re-export | Task A1 verifies via build. `HighlightScanner::new()` and `::scan()` are `pub` methods unchanged in signature throughout PR-A and PR-B. |
| Cache invalidation logic | Task A4 keeps `HighlightCache` field semantics unchanged; `scan_from` accesses `self.cache.checkpoints` exactly as before. |
| Performance regression | Task A5 captures baseline; Task B6 verifies post-PR-B within ±5%. |
| `scan_word` needs `CommandChecker` access | Task B3 makes `&mut CommandChecker` an explicit parameter; Task B5 packages it into `ScanCtx`. |
| `scan_normal` calls `scan_dollar` and `scan_word` | Task B3 converts the callees first; Task B4 converts `scan_normal` once they're free functions. |

---

## Plan Self-Review

**1. Spec coverage:**
- ✅ §"Proposed Structure" 10 files → covered by Tasks A1–A5 (5 files), B1–B4 (5 files).
- ✅ §"Responsibility Redesign" `ScanCtx` introduction → A5 (struct), B5 (signature switch).
- ✅ §"Why Not a `ScanMode` Trait" → no implementation impact, design decision.
- ✅ §"Test Reorganization" `test_checker_*` move → Task C1.
- ✅ §"Out of Scope" KEYWORDS / TODO entries → not implemented (spec says out of scope).
- ✅ §"PR Breakdown" 3 PRs → A (5 tasks), B (6 tasks incl. bench), C (1 task), final D1.
- ✅ §"Performance" bench ±5% → Tasks A5 + B6.
- ✅ §"Risks" → covered in Risk Mitigation Notes above.
- ✅ §"Definition of Done" → expanded into 11-item DoD list.

**2. Placeholder scan:** No "TBD", "TODO", "implement later", or vague language. Task bodies that say "PASTE the body" for large code blocks include the exact source location (`awk` command for body extraction), the rewrite rules, and the visibility/indent transformations. This is necessary because reproducing 200-line scanner bodies verbatim in the plan would bloat the document while gaining nothing — the source of truth is the existing file.

**3. Type consistency:**
- `ScanCtx<'a>` defined in A5 with 4 fields (input, state, spans, checker). Used in B5 and beyond. ✅
- Scanner signatures: `pub(super) fn scan_X(ctx: &mut ScanCtx, env: &CheckerEnv, pos, [payload]) -> usize` — uniform across normal/word/quotes/expansion/comment. ✅
- `mark_unclosed_errors`: `pub(super) fn mark_unclosed_errors(state: &ScannerState, input_len: usize, spans: &mut Vec<ColorSpan>)` — defined in A3, called from `mod.rs::scan_from`. ✅
- `state::ScanMode` enum 9 variants: Normal, SingleQuote { start }, DoubleQuote { start }, DollarSingleQuote { start }, Parameter { start, braced }, CommandSub { start }, Backtick { start }, ArithSub { start }, Comment { start }. Dispatcher in B5 destructures each. ✅
- `HighlightScanner` struct fields: `cache: HighlightCache`, `accumulated_state: Option<...>`, `checker: CommandChecker`. None move; all stay private to `mod.rs`. ✅

No issues found.

**4. Inter-task call sequence verification:**
- A2 helpers → required by all later tasks (is_keyword etc. used in scanners). ✅
- A3 state → required by A4 (cache holds ScannerState in checkpoints), A5 (ScanCtx holds &mut ScannerState), and all scanners. ✅
- A4 cache → required by mod.rs::scan_from for checkpoints (HighlightCache field). ✅
- A5 ctx → required by B5 (ScanCtx is the key signature element). ✅
- B1 simple scanners → no upstream dependency on other scanners. ✅
- B2 expansion (parameter, arith_sub) → no dependency on scan_dollar (separate scanner). ✅
- B3 word/dollar/double_quote → scan_double_quote calls scan_dollar (verify in Step 1; spec assumed yes), so they go in same task. ✅
- B4 normal → calls scan_dollar (in expansion, free) and scan_word (in word, free). Both already free after B3. ✅
- B5 ScanCtx switch → all scanners are free functions; just a signature transformation. ✅
- B6 bench → after B5 to measure final form. ✅
- C1 test relocation → independent of B work, can happen any time after PR-A. ✅
- D1 final → after everything. ✅

The dependency order is correct.
