# SP3 — `src/interactive/highlight_scanner.rs` Responsibility Redesign

Part of the [Large-File Responsibility Redesign Umbrella](2026-05-06-large-file-redesign-umbrella-design.md).

## Current State

`src/interactive/highlight_scanner.rs` is 1594 lines (~1100 production + ~500 tests) — the project's largest file. All scan-mode logic is implemented as methods on a single `HighlightScanner` struct that holds the cache, state, span accumulator, and public API. Mode dispatch happens in `scan_from` via `match state.current_mode()`.

Production breakdown:

| Region | Lines | Range |
|---|---|---|
| `ScanMode` enum, `ScannerState` + impl | ~70 | 10–72 |
| `is_keyword` / `is_operator_char` / `is_redirect_start` / `is_valid_name` / `is_word_break` | ~45 | 74–114 |
| `HighlightCache` + impl | ~50 | 115–160 |
| `HighlightScanner` struct, `new`, `scan` (public), `scan_from` (dispatcher) | ~165 | 162–330 |
| `scan_normal` | ~195 | 330–524 |
| `scan_dollar` | ~90 | 524–614 |
| `scan_word` | ~120 | 614–733 |
| `scan_single_quote` | ~28 | 733–761 |
| `scan_double_quote` | ~185 | 761–946 |
| `scan_dollar_single_quote` | ~36 | 946–982 |
| `scan_parameter` | ~30 | 982–1011 |
| `scan_arith_sub` | ~28 | 1011–1039 |
| `scan_comment` | ~22 | 1039–1060 |
| `mark_unclosed_errors` | ~45 | 1060–1100 |

Tests cover both `HighlightScanner::scan` API behavior (`test_scan_*`) and `CommandChecker` (`test_checker_*`) — the latter is misplaced; it tests `command_checker.rs` types from this file's `tests` module for historical reasons.

## Proposed Structure

```
src/interactive/highlight_scanner/
  mod.rs          — HighlightScanner struct, scan() public API, scan_from dispatcher  (~200 lines)
  state.rs        — ScannerState, ScanMode + impl, mark_unclosed_errors                (~120 lines)
  cache.rs        — HighlightCache + impl                                              (~80 lines)
  helpers.rs      — is_keyword/operator_char/redirect_start/valid_name/word_break      (~70 lines)
  ctx.rs          — ScanCtx<'a> shared mutable state for scanner functions             (~50 lines)
  normal.rs       — scan_normal                                                        (~210 lines)
  word.rs         — scan_word                                                          (~135 lines)
  quotes.rs       — scan_single_quote, scan_double_quote, scan_dollar_single_quote     (~270 lines)
  expansion.rs    — scan_dollar, scan_parameter, scan_arith_sub                        (~165 lines)
  comment.rs      — scan_comment                                                       (~30 lines)
```

Ten files, all under 270 lines.

## Responsibility Redesign — `ScanCtx` + Free-Function Scanners

Each scan-mode function currently takes `&mut self` (i.e., `&mut HighlightScanner`). Most touch only state and spans (passed as parameters); two pieces of `HighlightScanner`'s internals leak in:

- **`self.checker` (CommandChecker)** — `scan_word` calls `self.checker.check(&word, checker_env)` to determine if a word names an existing command. This is the only real `self.*` access in the scanner bodies.
- **`self.cache`** — used by `scan_from` itself for checkpoint bookkeeping. Not used inside any per-mode scanner.

Forcing scanners to be `impl HighlightScanner` methods couples them to the wrapper struct unnecessarily. Introduce a small context struct that bundles the mutable state every scanner needs (state, spans) plus the one auxiliary the word scanner needs (checker):

```rust
// ctx.rs
pub(super) struct ScanCtx<'a> {
    pub input: &'a [char],
    pub state: &'a mut ScannerState,
    pub spans: &'a mut Vec<ColorSpan>,
    pub checker: &'a mut CommandChecker,
}
```

`checker` is included even though only `scan_word` reads it. The alternative — threading `&mut CommandChecker` as a separate parameter through the dispatcher and through `scan_normal` (which calls `scan_word`) — would clutter every callsite. Keeping `checker` in `ScanCtx` makes the per-scanner signature uniform: `(ctx: &mut ScanCtx, env: &CheckerEnv, pos: usize, [payload...])`.

`HighlightScanner.cache` and `HighlightScanner.accumulated_state` stay private to `mod.rs` — `scan_from` is the only consumer, and it remains an `impl HighlightScanner` method.

Each scan-mode function becomes a free function taking `&mut ScanCtx<'_>` plus the read-only `CheckerEnv` and the current position:

```rust
// normal.rs
pub(super) fn scan_normal(
    ctx: &mut ScanCtx<'_>,
    env: &CheckerEnv<'_>,
    pos: usize,
) -> usize {
    // ... same body as today, accessing ctx.input / ctx.state / ctx.spans
}
```

`HighlightScanner::scan_from` becomes the dispatcher that builds `ScanCtx` once and loops. The actual `ScanMode` enum has nine variants (`Normal`, `SingleQuote { start }`, `DoubleQuote { start }`, `DollarSingleQuote { start }`, `Parameter { start, braced }`, `CommandSub { start }`, `Backtick { start }`, `ArithSub { start }`, `Comment { start }`); struct-variant payloads must be destructured and passed to scanners that need them. `CommandSub` and `Backtick` are degenerate: they have no scanner of their own — `scan_normal` handles entry/exit when it sees `$( ... )` or `` ` ... ` ``, so the dispatcher just pops the mode if it ever lands there:

```rust
fn scan_from(&mut self, /* ... */, env: &CheckerEnv<'_>) {
    let mut ctx = ScanCtx {
        input: /* slice */,
        state: &mut self.state,
        spans: &mut self.spans,
    };
    let mut pos = /* ... */;
    while pos < ctx.input.len() {
        pos = match ctx.state.current_mode().clone() {
            ScanMode::Normal              => normal::scan_normal(&mut ctx, env, pos),
            ScanMode::SingleQuote { start }
                                          => quotes::scan_single_quote(&mut ctx, env, pos, start),
            ScanMode::DoubleQuote { start }
                                          => quotes::scan_double_quote(&mut ctx, env, pos, start),
            ScanMode::DollarSingleQuote { start }
                                          => quotes::scan_dollar_single_quote(&mut ctx, env, pos, start),
            ScanMode::Parameter { start, braced }
                                          => expansion::scan_parameter(&mut ctx, env, pos, start, braced),
            ScanMode::ArithSub { start }  => expansion::scan_arith_sub(&mut ctx, env, pos, start),
            ScanMode::Comment { start }   => comment::scan_comment(&mut ctx, env, pos, start),
            ScanMode::CommandSub { .. }   => { ctx.state.pop_mode(); pos }
            ScanMode::Backtick { .. }     => { ctx.state.pop_mode(); pos }
        };
    }
}
```

`scan_dollar` and `scan_word` are **not** dispatcher entries — they are helpers called from `scan_normal` (and `scan_double_quote`, which calls `scan_dollar` for inline `$` expansion). After the split they live in `expansion.rs` and `word.rs` as `pub(super) fn`s, called directly across sibling submodules.

### Effect

1. Scanner functions are decoupled from `HighlightScanner`'s internals. Cache logic can change in `cache.rs` without touching scanners.
2. Each scanner is unit-testable in isolation: build a `ScanCtx`, call the function, assert on `ctx.spans`.
3. The dispatcher in `mod.rs` becomes a small, readable table mapping `ScanMode` to its scanner — exactly the kind of code that should live in `mod.rs`.

## Why Not a `ScanMode` Trait

A trait with one method per mode (`fn scan(&mut self, ...) -> usize`) would replace the `match` block with dynamic dispatch. We deliberately do not do this:

- Scan modes form a closed state machine driven by POSIX §2.4 lexical rules. The variant set is fixed; there is no extension story for plugin-defined modes.
- `match` on an enum is the most direct expression of "this state runs that scanner." Trait dispatch adds a layer (vtable or generics) that obscures the mapping.
- Trait objects would erase the inlining that the current direct-call form preserves.

The B3 redesign here is the `ScanCtx`-driven decoupling — not a trait abstraction.

## Test Reorganization

| Existing Tests | New Location |
|---|---|
| `test_scan_*` (15+ tests covering `HighlightScanner::scan` behavior) | `mod.rs` |
| Helpers (`test_scanner`, `test_env`, `scan_input`, `assert_span`, `make_aliases`, `checker_env`) | `mod.rs` `#[cfg(test)] mod tests` |
| `test_checker_*` (6 tests of `CommandChecker` / `CheckerEnv`: `builtin_special`, `alias`, `path_search`, `path_cache_invalidation`, `direct_path`, `path_with_tempfile`) | **`src/interactive/command_checker.rs`** — they belong with the type they test |

The `test_checker_*` move is a side cleanup taken in PR-C.

## Out of Scope

- TODO entry "`highlight_scanner.rs` `KEYWORDS` duplicates POSIX §2.4 list" — consolidating with `crate::lexer::reserved::RESERVED_WORDS` requires reconciling `COMMAND_POSITION_KEYWORDS` (which includes `"time"`) and command-position restoration logic. That work is a separate, focused spec.
- TODO entries for syntax-highlighting features (color palette customization, mode-stack approach for nested expansion, ANSI optimization) — out of scope; they are feature work, not refactoring.

## PR Breakdown

1. **PR-A — Scaffolding.** Convert `highlight_scanner.rs` to `highlight_scanner/mod.rs`. Extract `state.rs`, `cache.rs`, `helpers.rs`, `ctx.rs`. Each is a pure move of types/free functions. `HighlightScanner` and all `scan_*` methods stay in `mod.rs` for now. Tests unchanged.
2. **PR-B — Mode scanners.** Extract `normal.rs`, `quotes.rs`, `expansion.rs`, `word.rs`, `comment.rs`. Convert each `scan_*` from `&mut self` method to free function taking `&mut ScanCtx + &CheckerEnv + pos`. Update `scan_from` to build `ScanCtx` and dispatch.
3. **PR-C — Test relocation.** Move `test_checker_*` (6 tests) to `command_checker.rs`. Pure move.

PR-A is mechanical. PR-B is the redesign body. PR-C is unrelated cleanup that becomes natural at this point.

## Performance

`highlight_scanner` runs on every keystroke in the interactive REPL. The redesign must not regress scan throughput.

- Free-function calls do not introduce vtable indirection. Rust's inlining decisions are based on body size and inlining hints, not on whether a function is a method or a free function.
- `ScanCtx` is a small struct of references; passing `&mut ScanCtx` is equivalent in cost to passing `&mut self` plus access through extra field indirection.
- Verify with `cargo bench --bench interactive_smoke` before and after PR-B. Threshold: scan throughput within ±5% of the PR-A baseline. A regression beyond 5% triggers redesign review.

## Risks

- `mark_unclosed_errors` reads `&ScannerState` and writes to `&mut Vec<ColorSpan>`. Place it in `state.rs` as a free function `pub(super) fn mark_unclosed_errors(state: &ScannerState, input_len: usize, spans: &mut Vec<ColorSpan>)`. Callers in `mod.rs` invoke it after the scan loop.
- `HighlightScanner.spans` and `HighlightScanner.state` cannot be both mutably borrowed at the same time as `&mut self` in safe Rust. The current method form works because everything goes through `&mut self`. The `ScanCtx` form requires reborrowing the two fields explicitly — the standard pattern is `let ctx = ScanCtx { state: &mut self.state, spans: &mut self.spans, input };`. The compiler accepts this because the fields are disjoint.
- Public API: `HighlightScanner::new()`, `HighlightScanner::scan()`, and `apply_style()` from the parent `highlight.rs` module are unchanged. `line_editor.rs` is not touched.
- Cache invalidation logic in `HighlightCache::diff_pos` / `nearest_checkpoint` runs against `ScannerState` snapshots; moving cache to its own file does not change any invariants here.

## Definition of Done

- `cargo test` PASS.
- `cargo bench --no-run` PASS, and `interactive_smoke` shows scan throughput within ±5% of PR-A baseline.
- `tests/pty_interactive.rs` syntax-highlighting tests PASS.
- Each production file ≤ 270 lines.
- All TODO entries about syntax highlighting are preserved (out of scope here).
- `command_checker.rs` gains the 6 relocated tests; `highlight_scanner/mod.rs` `tests` module no longer references `CommandChecker` / `CheckerEnv` for testing.
