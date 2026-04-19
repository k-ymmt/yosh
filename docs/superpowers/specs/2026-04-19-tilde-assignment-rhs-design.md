# Tilde Expansion on Assignment RHS Design

**Date**: 2026-04-19
**Sub-project**: 2 of 4 (XFAIL E2E test remediation — XCU Chapter 2 gaps)
**Target XFAIL**: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`

## Context

POSIX §2.6.1 mandates tilde expansion in assignment words at two
positions: immediately after the `=` that opens the value, and after
each unquoted `:` inside the value. `x=~/bin` and `PATH=~/a:~/b` are
canonical examples. yosh currently performs tilde expansion only at
word start (the lexer at `src/lexer/word.rs:112` emits `WordPart::Tilde`
only when `parts.is_empty() && literal.is_empty()`), so any tilde
embedded in an assignment value stays as a literal character and is
never expanded.

Investigation:

- `WordPart::Tilde(Option<String>)` already exists in the AST
  (`src/parser/ast.rs:151`).
- The expander already handles it correctly
  (`src/expand/mod.rs:332,336`).
- The parser's `try_parse_assignment` (`src/parser/mod.rs:301-347`)
  splits an assignment word at `=`, but the resulting value is
  `[Literal(after_eq), ...]` — with the tilde still buried inside the
  first literal.

The fix therefore belongs at the parser layer: after splitting, walk
the first literal and break out tildes that appear at position 0 or
immediately after a `:`.

## Goals

1. Flip the XFAIL at
   `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`.
2. Support `x=~/bin`, `x=~user/bin`, and colon-separated variants
   (`PATH=~/a:~/b:~/c`) per POSIX §2.6.1.
3. Preserve existing semantics for quoted tildes (`'~'`, `"~"`) — they
   already stay in quoted parts and are not literal-embedded.
4. Work uniformly for `export`, `readonly`, and command-prefix
   assignments, since all route through the same parser helper.

## Non-goals

- Tildes that cross `WordPart` boundaries (e.g., `x=$HOME:~/bin` where
  the tilde follows a parameter expansion, not a raw colon in a
  literal). Tracked as a separate TODO.
- Tilde expansion for regular command words beyond position 0 (not
  POSIX-required).
- `~+`, `~-`, `~N` bash extensions (non-POSIX).
- Runtime `set -o` style toggles for tilde behavior (not POSIX).

## Architecture

Single-file change: `src/parser/mod.rs`. No lexer or expander changes.

```
try_parse_assignment(Word)
  ├─ extract `name` before `=`
  ├─ build initial value_parts = [Literal(after_eq), ...remaining_parts]
  └─ value_parts = split_tildes_in_assignment_value(value_parts)
```

`split_tildes_in_assignment_value(parts: Vec<WordPart>) -> Vec<WordPart>`
— new private pure function. Handles only the first `Literal` part;
other parts pass through untouched. Unit-testable without any
filesystem or environment dependencies.

## Tilde split algorithm

Operates on the first `Literal` of the input parts:

```
1. If parts is empty, or parts[0] is not Literal, return parts unchanged.
2. Let s = the first Literal's content.
3. Split s on ':' into segments (preserving empty segments between
   consecutive colons and at ends).
4. For each segment, in order:
     - If this is not the first segment, emit a Literal(":") separator.
     - If segment starts with '~':
         - Let user = chars until first '/' (or end of segment).
         - If user contains only name-safe chars (letters, digits,
           '_', '.', '-') or is empty:
             - Emit Tilde(None) if user empty, else Tilde(Some(user)).
             - Emit Literal(rest) if rest (from '/' onwards) non-empty.
         - Else:
             - Emit Literal(segment) unchanged.
     - Else: emit Literal(segment) unchanged.
5. Merge adjacent Literal parts.
6. Append the remaining parts (parts[1..]) unchanged.
```

Validity of the user portion: the algorithm accepts what `getpwnam`
would plausibly accept. If `getpwnam` fails at expansion time, the
existing `expand_tilde_prefix` returns the original `~user` string
(`src/expand/mod.rs:626-631`) — no error propagation needed.

### Examples

| Input Literal | Output parts (merged Literals shown) |
|---|---|
| `~/bin` | `[Tilde(None), Literal("/bin")]` |
| `~user/bin` | `[Tilde(Some("user")), Literal("/bin")]` |
| `~/a:~/b` | `[Tilde(None), Literal("/a:"), Tilde(None), Literal("/b")]` |
| `/usr:~/bin` | `[Literal("/usr:"), Tilde(None), Literal("/bin")]` |
| `:~/foo` | `[Literal(":"), Tilde(None), Literal("/foo")]` |
| `~:~/a` | `[Tilde(None), Literal(":"), Tilde(None), Literal("/a")]` |
| `foo~/bin` | `[Literal("foo~/bin")]` (no change) |
| `~~/bin` | `[Literal("~~/bin")]` (second `~` is not a name-safe char) |
| `~` | `[Tilde(None)]` |
| `""` | `[]` (empty) |

## Edge cases

| Case | Behavior |
|---|---|
| `x=` | `Assignment { value: None }` (existing behavior preserved) |
| `x=~` | `[Tilde(None)]` |
| `x=~/` | `[Tilde(None), Literal("/")]` |
| `x=::~/a` | `[Literal("::"), Tilde(None), Literal("/a")]` |
| `x=~/a:` | `[Tilde(None), Literal("/a:")]` |
| `x=~ foo` | Lexer splits at space; assignment value is `"~"`, `foo` is a separate command word → `[Tilde(None)]` |
| `x=~/bin$var` | First Literal `"~/bin"` splits to `[Tilde(None), Literal("/bin")]`; remaining `[Parameter(var)]` preserved → final: `[Tilde(None), Literal("/bin"), Parameter(var)]` |
| `x=$var:~/bin` | First part is Parameter, not Literal → pass through unchanged. Out of scope; tracked as TODO. |
| `x='~'/bin` | Lexer puts `~` in `SingleQuoted("~")`; first Literal is empty → pass through. Not expanded. |
| `x="~"/bin` | Lexer puts `~` in `DoubleQuoted`; first Literal is empty → pass through. Not expanded. |
| `x=\~/bin` | Backslash-escape — lexer behavior to verify at implementation time; expected: escaped `~` stays in a Literal at position 0, but our name-safe check excludes non-`~` first character, so we need to ensure `\~` is represented as `Literal("\\~/bin")` or `Literal("~/bin")` such that the tilde path is intentional. Implementation adds a test covering the lexer's actual output. |

## Error handling

None. The transformation is pure; no `Result` return. Malformed input
(invalid user names, empty segments) simply falls through as literal
text, which is the POSIX-correct behavior.

## Testing

### A. Unit tests (in `src/parser/mod.rs` `#[cfg(test)]`)

Table-driven tests for `split_tildes_in_assignment_value`:

- `split_empty_returns_empty`
- `split_no_tilde_returns_unchanged`
- `split_leading_tilde_only`
- `split_leading_tilde_slash`
- `split_leading_tilde_user`
- `split_colon_separated_tildes`
- `split_middle_segment_without_tilde`
- `split_trailing_colon`
- `split_leading_colon`
- `split_consecutive_colons`
- `split_invalid_tilde_position` (e.g. `foo~/bin`)
- `split_preserves_non_literal_trailing_parts`
- `split_empty_first_literal_no_op`
- `split_first_part_is_not_literal_no_op`

Integration tests exercising `try_parse_assignment`:

- `assignment_tilde_rhs_produces_tilde_part`
- `assignment_tilde_with_colon_segments`

### B. XFAIL flip

Remove the `XFAIL:` line from
`e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`.

### C. New E2E tests (under `e2e/posix_spec/2_06_01_tilde_expansion/`)

| File | Purpose |
|---|---|
| `tilde_rhs_user_form.sh` | `x=~root/bin` — verify `getpwnam` path or safe fallback |
| `tilde_rhs_colon_multiple.sh` | `PATH=~/a:~/b` → `/h/a:/h/b` |
| `tilde_rhs_middle_segment.sh` | `x=/usr:~/bin` → `/usr:/h/bin` |
| `tilde_rhs_quoted_not_expanded.sh` | `x='~'/bin` → `~/bin` |
| `tilde_rhs_double_quoted_not_expanded.sh` | `x="~"/bin` → `~/bin` |
| `tilde_rhs_export.sh` | `export PATH=~/bin` → `/h/bin` |
| `tilde_rhs_readonly.sh` | `readonly x=~/bin` → `/h/bin` |
| `tilde_rhs_command_prefix.sh` | `PATH=~/bin cmd` — prefix assignment expansion |
| `tilde_rhs_not_at_start.sh` | `x=foo~/bin` → `foo~/bin` (unchanged) |

All files: `POSIX_REF: 2.6.1 Tilde Expansion`, permissions `644`,
metadata headers per `e2e/README.md`.

### D. Regression

- Existing tilde tests in `e2e/variable_and_expansion/` and
  `e2e/quoting/` (if any) must continue to pass.
- Any syntax-highlighting tests that walk the AST for `Tilde` parts
  benefit from the richer AST automatically.

## Completion criteria

1. `cargo test --lib` all green; `cargo clippy` no warnings in
   `src/parser/mod.rs`; `cargo fmt --check` clean.
2. `./e2e/run_tests.sh` summary: `XFail: 2, XPass: 0, Failed: 0,
   Timedout: 0` (remaining XFAILs are §2.10 empty compound_list and
   §2.5.3 LINENO, handled by sub-projects 3–4).
3. `TODO.md` updates:
   - Delete the `§2.6.1 Tilde expansion on assignment RHS` entry
     (project convention: remove rather than `[x]`).
   - Add new entry for `§2.6.1 Tilde in mixed-part assignment values`
     covering the deferred `x=$HOME:~/bin` case.
