# Empty `compound_list` Syntax Error Design

**Date**: 2026-04-19
**Sub-project**: 3 of 4 (XFAIL E2E test remediation — XCU Chapter 2 gaps)
**Target XFAIL**: `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`

## Context

POSIX §2.10 Shell Grammar defines `compound_list` via the BNF:

```
compound_list : term
              | newline_list term
              | term separator
              | newline_list term separator

term          : term separator and_or
              | and_or
```

`term` is right-recursive and requires at least one `and_or`, so
`compound_list` is non-empty by construction. yosh's
`parse_compound_list` (`src/parser/mod.rs:413`) skips leading newlines
and loops until a terminator, silently returning an empty `Vec` when
no `and_or` is produced. That allows constructs like `if true; then fi`
to parse successfully and return exit 0, violating POSIX §2.10.

`compound_list` appears in the following POSIX productions (all
require non-empty bodies):

- `if compound_list then compound_list [elif ... then ...]* [else compound_list] fi`
- `while compound_list do compound_list done`
- `until compound_list do compound_list done`
- `for name [in words]; do compound_list done`
- `{ compound_list }`
- `( compound_list )`

`case` items are NOT affected — the POSIX BNF explicitly allows
`pattern ')' linebreak DSEMI` (empty body). yosh's case parser uses
an inline body loop (not `parse_compound_list`) so it is already
compliant and needs no change.

## Goals

1. Flip the XFAIL at
   `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`.
2. Emit a POSIX-style syntax error (exit 2, `stderr` contains `syntax`)
   for every construct where an empty `compound_list` is encountered.
3. Keep `case` item bodies permissively empty (POSIX-compliant).
4. Produce context-aware error messages identifying which clause was
   empty (e.g. `'then' body`, `'while' condition`).

## Non-goals

- Changing the lexer, expander, or executor.
- Touching AST definitions.
- Improving unrelated error messages.
- Runtime `set -n` style toggle (POSIX `-n` already aborts on any
  parse error; no special-casing needed).

## Architecture

Single-file change: `src/parser/mod.rs`. The modification is
isolated to `parse_compound_list` and its ten call sites.

```
parse_compound_list(&mut self, context: &str) -> Result<Vec<CompleteCommand>>
  ├─ existing skip_newlines + loop
  └─ if commands.is_empty() at end:
       Err(ShellError::parse(UnexpectedToken, line, col,
           format!("syntax error: empty compound list in {context}")))
```

Callers pass a fixed context string. The ten sites:

| File location | Context string |
|---|---|
| `parse_if_clause` — if condition | `"'if' condition"` |
| `parse_if_clause` — then body | `"'then' body"` |
| `parse_if_clause` — elif condition | `"'elif' condition"` |
| `parse_if_clause` — elif body | `"'elif' body"` |
| `parse_if_clause` — else body | `"'else' body"` |
| `parse_do_group` — do body | `"'do' body"` |
| `parse_while_clause` — while condition | `"'while' condition"` |
| `parse_until_clause` — until condition | `"'until' condition"` |
| `parse_brace_group` — brace group | `"brace group"` |
| `parse_subshell` — subshell | `"subshell"` |

## Error handling

- Error kind: `ShellError::parse(ParseErrorKind::UnexpectedToken,
  line, col, msg)`.
- Exit code: `2`, per the existing `ShellErrorKind::Parse(_) => 2`
  mapping at `src/error.rs:103`.
- `line`/`col` come from `self.current_span()` at the moment the
  empty-list detection fires — this points at the terminating
  keyword (`fi`, `done`, `}`, `)`, etc.), which is the most useful
  location for the user.
- Message format: `syntax error: empty compound list in {context}`
  (always contains the literal `syntax`, satisfying the XFAIL test's
  `EXPECT_STDERR: syntax` substring requirement).

## Edge cases

| Input | Behavior |
|---|---|
| `if true; then fi` | `then` body empty → Err |
| `if ; then :; fi` | `if` condition empty → Err |
| `while do done` | `while` condition empty → Err first (short-circuits) |
| `for i in a; do done` | `do` body empty → Err |
| `{ }` | brace group empty → Err |
| `( )` | subshell empty → Err |
| `if true; then\n\ntrue\nfi` | consecutive newlines fine → parses OK (existing behavior preserved) |
| `if true; then\n#comment\nfi` | comments stripped at lex; body empty → Err (POSIX-correct) |
| `if true; then :; fi` | `:` is a valid and_or → parses OK |
| `case x in pat) ;; esac` | case bodies can be empty → parses OK (separate parse path) |
| `a() { }`, `a() ( )` | function body is a compound_command → inner brace/subshell triggers Err |
| Nested: `if true; then if a; then fi; fi` | inner `then fi` errors first |

## Testing

### A. Unit tests (`src/parser/mod.rs` `#[cfg(test)] mod tests`)

Each test calls `Parser::new(source).parse_program()` and asserts the
resulting `Err` has exit code 2 and contains both `"syntax"` and the
relevant context substring.

Empty-case tests:

- `empty_if_then_errors`
- `empty_if_condition_errors`
- `empty_elif_condition_errors`
- `empty_elif_body_errors`
- `empty_else_body_errors`
- `empty_while_condition_errors`
- `empty_while_body_errors`
- `empty_until_condition_errors`
- `empty_until_body_errors`
- `empty_for_body_errors`
- `empty_brace_group_errors`
- `empty_subshell_errors`

Regression tests (non-empty stays valid):

- `nonempty_if_parses_ok` — `if true; then :; fi`
- `case_empty_body_still_parses_ok` — `case x in pat) ;; esac`
- `comment_only_body_errors_per_posix` — `if true; then\n#c\nfi`

### B. XFAIL flip

Remove the `XFAIL:` line from
`e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`.

### C. New E2E tests (under `e2e/posix_spec/2_10_shell_grammar/`)

| File | Input | Expected |
|---|---|---|
| `empty_if_condition_is_error.sh` | `if then true; fi` | exit 2, stderr contains `syntax` |
| `empty_elif_body_is_error.sh` | `if true; then :; elif true; then fi` | exit 2, stderr `syntax` |
| `empty_else_body_is_error.sh` | `if true; then :; else fi` | exit 2, stderr `syntax` |
| `empty_while_condition_is_error.sh` | `while do done` | exit 2, stderr `syntax` |
| `empty_while_body_is_error.sh` | `while true; do done` | exit 2, stderr `syntax` |
| `empty_until_condition_is_error.sh` | `until do done` | exit 2, stderr `syntax` |
| `empty_for_body_is_error.sh` | `for i in a; do done` | exit 2, stderr `syntax` |
| `empty_brace_group_is_error.sh` | `{ }` | exit 2, stderr `syntax` |
| `empty_subshell_is_error.sh` | `( )` | exit 2, stderr `syntax` |
| `case_empty_body_is_ok.sh` | `case x in pat) ;; esac; echo ok` | exit 0, stdout `ok` (regression guard) |

All files: `POSIX_REF: 2.10 Shell Grammar`, permissions `644`.

### D. Regression

- `cargo test --lib`: all green (existing suite continues to pass).
- Existing if/while/for/case E2E tests (`e2e/control_flow/`): unchanged.

## Completion criteria

1. `cargo test --lib` all green, `cargo clippy` no new warnings,
   `cargo fmt --check` clean.
2. `./e2e/run_tests.sh` summary: `XFail: 1, XPass: 0, Failed: 0,
   Timedout: 0` (remaining XFAIL is §2.5.3 LINENO, handled by
   sub-project 4).
3. `TODO.md` entry for `§2.10 Shell Grammar — parser accepts an empty
   compound_list ...` is removed per project convention.
