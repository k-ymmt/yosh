# Phase 1: Known Limitations Fix — Design Spec

## Overview

Address two known limitations from Phase 1:

1. **Nested command substitution edge cases** — `$(echo $(echo ')'))` fails due to the balanced-paren depth-counting approach in `read_balanced_parens`
2. **`Lexer.pending_heredocs` encapsulation** — field is `pub` but should use accessor methods

## Item 1: Nested Command Substitution in `read_balanced_parens`

### Problem

`read_balanced_parens` (`src/lexer/mod.rs:1112`) uses a depth counter to track `(` and `)`. It does not recognize `$(` as introducing a nested command substitution. When a quoted `)` appears inside an inner command substitution (e.g., `$(echo $(echo ')'))`), the depth counter is decremented prematurely, producing malformed output.

### Root Cause

The function treats all `(` and `)` characters uniformly. It correctly handles single-quoted and double-quoted strings, but does not account for `$(` introducing a recursive command substitution context with its own quoting rules.

### Solution: Recursive Self-Call on `$(` Detection

In the main loop of `read_balanced_parens`, when `$` is read:

1. Peek at the next character
2. If the next character is `(`:
   - Add `$` to the content buffer
   - Recursively call `read_balanced_parens()`
   - Append `(` + returned content + `)` to the buffer
3. Otherwise, add `$` to the content buffer as-is

This ensures the inner command substitution's quoting context is handled independently.

### Trace: `$(echo $(echo ')'))`

1. Outer call: consumes `(`, depth=1, reads `echo `
2. Reads `$`, peeks `(` — recursive call
3. Inner call: consumes `(`, depth=1, reads `echo `
4. Reads `'` — enters single-quote mode, reads `)` as literal, reads closing `'`
5. Reads `)` — depth=0, returns `"echo ')'"`
6. Outer appends `$(echo ')')` to buffer
7. Reads `)` — depth=0, returns `"echo $(echo ')')"`

### Compatibility: `$((expr))`

Arithmetic expansion `$((1+2))` inside `$(...)` is handled naturally. The recursive call processes `(1+2)` as the inner content (the leading `(` increments depth, the first `)` decrements it, the second `)` closes the recursive call). The outer function reconstructs `$((1+2))`.

## Item 2: `Lexer.pending_heredocs` Encapsulation

### Problem

`pending_heredocs` is declared as `pub` on the `Lexer` struct (`src/lexer/mod.rs:30`). The parser accesses it directly in three locations to check `.is_empty()`. This bypasses the existing accessor methods (`register_heredoc`, `process_pending_heredocs`, `take_heredoc_body`).

### Solution

1. Remove `pub` from `pending_heredocs` field declaration
2. Add accessor method:
   ```rust
   pub fn has_pending_heredocs(&self) -> bool {
       !self.pending_heredocs.is_empty()
   }
   ```
3. Replace three direct accesses in `src/parser/mod.rs` (lines 90, 164, 273) with `self.lexer.has_pending_heredocs()`

### Impact

No behavioral change. The compiler enforces that no external code directly accesses the field after making it private.

## Testing

### Item 1: Nested Command Substitution

- **Unit/integration tests** in existing test infrastructure:
  - `$(echo $(echo ')'))` — quoted `)` inside nested command sub
  - `$(echo $(echo hello))` — basic nesting
  - `$(echo $((1+2)))` — arithmetic expansion inside command sub
- **E2E tests** to verify correct shell output for each case

### Item 2: Encapsulation

- No new tests required — existing tests pass unchanged
- Compilation success guarantees no external direct access remains

## Files Changed

- `src/lexer/mod.rs` — `read_balanced_parens` recursive fix + `has_pending_heredocs` accessor + field visibility
- `src/parser/mod.rs` — replace 3 direct field accesses with accessor call
- Test files — new test cases for nested command substitution
- E2E test files — new shell scripts for nested command substitution
