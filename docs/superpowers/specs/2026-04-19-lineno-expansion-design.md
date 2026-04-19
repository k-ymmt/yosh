# `$LINENO` Expansion Design

**Date**: 2026-04-19
**Sub-project**: 4 of 4 (XFAIL E2E test remediation — XCU Chapter 2 gaps)
**Target XFAIL**: `e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh`

## Context

POSIX §2.5.3 requires `LINENO` to expand to the script's current line
number at the point of evaluation. yosh currently does nothing with
`LINENO` — the variable is never set, so `$LINENO` expands to the empty
string. The XFAIL test asserts that `echo $LINENO` prints the script
line of that `echo` command.

The expander already handles `ParamExpr::Simple("LINENO")` via
`env.vars.get("LINENO")` (`src/expand/param.rs:9`). The lexer already
tracks per-token `Span { line, column }` (`src/lexer/token.rs:4`). What
is missing is (a) persisting the per-command source line into the AST
and (b) updating the `LINENO` shell variable from that line right
before each command runs.

## Goals

1. Flip the XFAIL at `e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh`.
2. Support `$LINENO` in script-level simple and compound commands.
3. Work transparently inside function bodies (the line number of the
   command inside the body).
4. No cost to existing call sites that don't care about line numbers.

## Non-goals

- bash-style function-local LINENO reset (POSIX marks this as
  implementation-defined; yosh will use the body's source line).
- Column-level position tracking.
- A dedicated non-allocating fast path (current `env.vars.set` is fine;
  optimization deferred unless benchmarks show a problem).
- Updating `LINENO` export status (it remains shell-local, not
  exported to child processes — matching bash).

## Architecture

Three small changes across three files:

```
src/parser/ast.rs
  SimpleCommand  { ..., line: usize }
  CompoundCommand { kind, line: usize }

src/parser/mod.rs
  parse_simple_command()   — capture self.current.span.line at entry
  parse_compound_command() — capture self.current.span.line at entry

src/exec/*
  exec_simple_command(cmd)   — first line: env.vars.set("LINENO", cmd.line)
  exec_compound_command(cmd) — first line: env.vars.set("LINENO", cmd.line)
```

Data flow:

```
source text
  ↓  (lexer: Span { line, column } on every SpannedToken)
  ↓
parse_simple_command / parse_compound_command
  ↓  (captures self.current.span.line at entry)
  ↓
AST with per-command line numbers
  ↓
exec_simple_command / exec_compound_command
  ↓  (env.vars.set("LINENO", cmd.line))
  ↓
expand ParamExpr::Simple("LINENO")
  ↓  (env.vars.get("LINENO"))
  ↓
stdout: "6"
```

## AST line attribution rules

### `SimpleCommand.line`

Line of the command's first token (whether that's an assignment, a
word, or a redirection operator).

```rust
pub fn parse_simple_command(&mut self) -> Result<SimpleCommand> {
    let line = self.current.span.line;
    // ... existing logic ...
    Ok(SimpleCommand { assignments, words, redirects, line })
}
```

### `CompoundCommand.line`

Line of the opening reserved word (`if`, `while`, `until`, `for`,
`case`) or opening bracket (`{`, `(`). Captured centrally in
`parse_compound_command`, so per-clause parsers (`parse_if_clause`,
etc.) are unchanged:

```rust
pub fn parse_compound_command(&mut self) -> Result<CompoundCommand> {
    let line = self.current.span.line;
    let kind = match ... { ... };
    Ok(CompoundCommand { kind, line })
}
```

### Not annotated

`Program`, `CompleteCommand`, `AndOrList`, `Pipeline`, `Assignment`,
`Redirect`, `Word`, `FunctionDef`, `CaseItem` — each `Command`
inside them carries its own line, which is sufficient for POSIX intent.

### Examples

| Input | line values |
|---|---|
| `echo hi` (line 1) | `SimpleCommand.line = 1` |
| `\n\necho hi` (line 3) | `SimpleCommand.line = 3` |
| `{ echo a; echo b; }` | `CompoundCommand.line = 1`, inner `SimpleCommand`s `line = 1` |
| multi-line `if` | `CompoundCommand.line = N` (the `if`), inner commands per their own lines |
| `cmd \\\n arg` (line continuation) | `SimpleCommand.line = N` (line of `cmd`; the `\<newline>` is collapsed by the lexer) |

## Executor integration

```rust
pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
    let _ = self.env.vars.set("LINENO", cmd.line.to_string());
    // ... existing body ...
}

pub(crate) fn exec_compound_command(&mut self, cmd: &CompoundCommand, redirects: &[Redirect]) -> Result<i32, ShellError> {
    let _ = self.env.vars.set("LINENO", cmd.line.to_string());
    // ... existing body ...
}
```

No changes to `exec_pipeline`, `exec_and_or`, or
`exec_complete_command` — each inner `Command` will set its own
LINENO when the executor reaches it.

`set -n` (noexec) short-circuits before these calls, so LINENO is not
updated during syntax-only runs. That matches POSIX semantics.

## Edge cases

| Scenario | Behavior |
|---|---|
| No command executed yet | `LINENO` unset; `$LINENO` → `""` (POSIX: initial value implementation-defined) |
| First simple command | `LINENO = <that line>` |
| Function call | Body runs; LINENO updates per command inside body |
| Subshell `( cmd )` | Env clones; child executor's `exec_compound_command` sets LINENO correctly |
| Command substitution `` `cmd` `` / `$(cmd)` | Sub-program parses fresh (line starts at 1), LINENO reflects sub-program line |
| Interactive REPL | Each input is a fresh parse (line=1), matching bash behavior |
| `readonly LINENO` then next command runs | `env.vars.set` returns `Err(String)`, dropped with `let _ =`. LINENO keeps its last successful value. Silent — matches the existing pattern for other shell vars |
| `line: 0` default in test-constructed AST | LINENO gets set to `"0"` — harmless, just an unusual value |

## Existing code impact

Adding named fields to `SimpleCommand` and `CompoundCommand` requires
updating every struct-literal site. Expected touch points:

- `src/parser/mod.rs` — the canonical construction paths (add `line`
  from captured span).
- `src/parser/mod.rs` tests — add `line: 0` (or a plausible line) to
  any hand-built AST literals.
- `src/exec/mod.rs` tests — same.
- `tests/parser_integration.rs` and similar — same.
- Any `assert_eq!` comparing whole `SimpleCommand` / `CompoundCommand`
  values (less common) — update expected values.

The implementation step begins with `grep -rn 'SimpleCommand\s*{' src/
tests/` (and same for `CompoundCommand`) to enumerate all sites.
Missing updates fail compile, making detection immediate.

## Testing

### A. Parser unit tests (`src/parser/mod.rs` `#[cfg(test)] mod tests`)

- `parse_simple_command_captures_line`
- `parse_simple_command_on_third_line`
- `parse_compound_if_captures_line`
- `parse_compound_if_on_second_line`
- `parse_brace_group_captures_line`
- `parse_subshell_captures_line`
- `parse_nested_if_then_captures_body_line`
- `parse_while_captures_line`

### B. Executor unit tests

- `exec_simple_command_sets_lineno` — hand-built
  `SimpleCommand { line: 5, ... }` → `env.vars.get("LINENO") ==
  Some("5")`
- `exec_compound_command_sets_lineno` — analogous

### C. XFAIL flip

Rewrite `e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh` to
remove the `# XFAIL:` line AND the `# Note:` line so `echo $LINENO`
lands on line 6, matching the existing `EXPECT_OUTPUT: 6`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO expands to the current script line number
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
echo $LINENO
```

### D. New E2E tests (under `e2e/posix_spec/2_05_03_shell_variables/`)

| File | Purpose |
|---|---|
| `lineno_after_blank_lines.sh` | Leading blank lines shift the reported line |
| `lineno_multiple_commands.sh` | Three successive `echo $LINENO` print increasing numbers |
| `lineno_inside_if.sh` | `if ... then echo $LINENO ...` reports the `echo`'s line |
| `lineno_inside_for.sh` | `for` body re-emits the same body-line each iteration |
| `lineno_inside_function.sh` | Function body's command reports its own body line |
| `lineno_inside_subshell.sh` | Subshell preserves line reporting |
| `lineno_after_heredoc.sh` | LINENO advances past the heredoc content |
| `lineno_unset_acts_like_posix.sh` | `unset LINENO`; next command re-sets it |

Tests where the absolute line is layout-dependent use `case`-style
verification (e.g., `case $x in [0-9]*) exit 0 ;; *) exit 1 ;; esac`)
or record-and-diff pairs of outputs to confirm structural properties
(e.g., "second echo's line > first echo's line").

All files: `POSIX_REF: 2.5.3 Shell Variables`, `644` permissions.

### E. Regression

- `cargo test --lib` all green (every previously-passing parser /
  executor test continues after the `line` field is added).
- Existing E2E suite: no new FAIL / XPASS.

## Completion criteria

1. `cargo test --lib` all green; `cargo clippy --lib` no new warnings
   in `src/parser/**` or `src/exec/**`; `cargo fmt --check` clean
   (using `rustfmt --edition 2024 --check` where needed — see the
   known-fmt-bug TODO).
2. `./e2e/run_tests.sh` summary: **`XFail: 0, XPass: 0, Failed: 0,
   Timedout: 0`** — the final XFAIL of the four-sub-project remediation
   is closed.
3. `TODO.md` entry `§2.5.3 LINENO — ...` removed per project
   convention.
