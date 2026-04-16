# ENV Tilde Expansion Design

**Date:** 2026-04-16
**Status:** Approved
**TODO ref:** `ENV` tilde expansion (`src/interactive/mod.rs`)

## Problem

When `ENV=~/foo` is set, the `~` is not expanded because the ENV value is wrapped in double quotes (`format!("\"{}\"", env_val)`) before lexing. In double-quote context, the lexer does not recognize tilde prefixes. POSIX 2.6.1 specifies that tilde expansion occurs before parameter expansion, but the current code only performs parameter expansion.

## Solution: Pre-process Tilde Before Double-Quote Wrapping

Apply tilde expansion to the ENV value **before** wrapping it in double quotes for parameter expansion. This follows POSIX expansion ordering (tilde → parameter → command substitution → arithmetic).

### Scope

- **File:** `src/interactive/mod.rs` (ENV processing in `Repl::new`, lines 67-90)
- **File:** `src/expand/mod.rs` (make `expand_tilde_user` `pub(crate)`)

### Algorithm

1. Get ENV value as string
2. If value starts with `~`:
   a. Find the end of the tilde prefix (first `/` or end of string)
   b. Extract username (empty for `~`, `"bob"` for `~bob`)
   c. If username is empty: replace `~` with `$HOME` from ShellEnv
   d. If username is non-empty: call `expand_tilde_user(username)` (getpwnam lookup)
   e. If expansion fails (no HOME, unknown user): keep original value unchanged
3. Proceed with existing double-quote wrapping and parameter expansion

### Examples

| ENV value | After tilde expansion | After parameter expansion |
|---|---|---|
| `~/foo` | `/home/user/foo` | `/home/user/foo` |
| `~bob/foo` | `/home/bob/foo` | `/home/bob/foo` |
| `$HOME/foo` | `$HOME/foo` (no tilde) | `/home/user/foo` |
| `~/$DIR/init` | `/home/user/$DIR/init` | `/home/user/bin/init` |
| `~` | `/home/user` | `/home/user` |
| `~nonexistent/foo` | `~nonexistent/foo` (unchanged) | `~nonexistent/foo` |
| `/absolute/path` | `/absolute/path` (no tilde) | `/absolute/path` |

### Visibility Change

`expand_tilde_user` in `src/expand/mod.rs:529` is currently private. Change to `pub(crate)` to allow reuse from `src/interactive/mod.rs`. The `Tilde(None)` case (bare `~`) is handled inline using ShellEnv's `$HOME`, consistent with how `expand_part_to_fields` handles `WordPart::Tilde(None)`.

### Testing

- Existing E2E tests (`source_env_expansion.sh`, `source_env.sh`) verify parameter expansion continues to work
- `expand_tilde_user` already has unit tests (`test_tilde_root_starts_with_slash`)
- Tilde expansion in ENV is an interactive-startup-only feature; direct E2E testing requires PTY tests (out of scope for this change)

### Not In Scope

- Changing the expansion pipeline in `src/expand/mod.rs`
- Adding a generic `expand_env_value` function (YAGNI)
- Tilde expansion in other contexts (already handled by the lexer/expander)
