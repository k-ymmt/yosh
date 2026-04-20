# Design: Fix `test_classify_incomplete_if` / `_while` Hang

**Date:** 2026-04-20
**Status:** Proposed
**Scope:** Parser + interactive `classify_parse` probe

## Problem

`tests/interactive.rs::test_classify_incomplete_if` and `test_classify_incomplete_while` hang indefinitely (>60s, SIGKILL by cargo test runner). Memory grows unboundedly (observed 1.6 GB in ~5 seconds) rather than staying in a tight infinite loop.

Tests expect:

```rust
classify_parse("if true; then\n", &aliases) == ParseStatus::Incomplete
classify_parse("while true; do\n", &aliases) == ParseStatus::Incomplete
```

## Root Cause (two interacting defects, both surfaced by commit `fe7c31c`)

Commit `fe7c31c` (2026-04-19) changed `parse_compound_list` to reject an empty list with a new `UnexpectedToken` error. This interacted with two pre-existing latent defects:

### Defect A — `parse_simple_command` accepts zero-progress

`src/parser/mod.rs::parse_simple_command` loops over Word / redirect / pending-heredoc newline and otherwise `break`s. If the current token is neither a Word nor a redirect (e.g. `DSemi`, `Pipe`, `RBrace`), it exits the loop **without advancing** and returns `Ok(SimpleCommand { assignments: [], words: [], redirects: [] })`.

POSIX §2.9.1 grammar forbids this: every `simple_command` derivation requires at least one of `cmd_prefix`, `cmd_name`, or `cmd_word`.

Because `parse_separator_op` also returns `None` for unhandled operators, `parse_complete_command` happily wraps the empty simple command in a `CompleteCommand` and returns `Ok`, again without advancing.

Inside `parse_compound_list`, the loop condition `while !is_at_end && !is_complete_command_end` remains true, and each iteration appends another empty `CompleteCommand` to `commands: Vec`. The result is an unbounded memory-eating non-loop (one token, infinite AST).

### Defect B — `classify_parse::is_completable` probes produce empty bodies

`is_completable` appends each `CLOSING_KEYWORDS` suffix (`\nfi\n`, `\ndone\n`, `\n;;\nesac\n`, …) and retries `parse_program`. Under the new non-empty rule, every probe for `"if true; then\n"` yields an empty `'then' body` (because the suffix starts with a closer, not a body), so no probe succeeds and `classify_parse` returns `Error` instead of `Incomplete` — which would be a test **failure**, not a hang, but the test never reaches that failure because Defect A hangs the `\n;;\nesac\n` probe first.

### Observed trace

Confirmed via instrumented `eprintln`. On the 6th probe `"if true; then\n\n;;\nesac\n"`, the parser enters `parse_compound_list("'then' body")` with current token `DSemi` and loops forever building empty `CompleteCommand`s. Memory usage: 324 MB → 663 MB → 1.2 GB → 1.6 GB across four seconds.

## Fix

Two coordinated changes:

### Fix A — `parse_simple_command` rejects empty results

In `src/parser/mod.rs::parse_simple_command`, after the main loop:

```rust
if assignments.is_empty() && words.is_empty() && redirects.is_empty() {
    let span = self.current_span();
    return Err(ShellError::parse(
        ParseErrorKind::UnexpectedToken,
        span.line,
        span.column,
        format!(
            "syntax error: unexpected token at start of command"
        ),
    ));
}
```

This is the root-cause fix. It makes any "no progress" outcome from `parse_simple_command` an explicit error, defending every caller (direct and transitive) from infinite-loop failure modes that involve an unhandled operator token.

**POSIX correctness:** Every simple command must have at least one `cmd_prefix`, `cmd_name`, or `cmd_word` element.

**Risk:** Other tests or code paths may currently rely on the permissive empty-return behavior. Verification plan includes running the full unit + integration + E2E suites to catch any regression.

### Fix B — `classify_parse::CLOSING_KEYWORDS` includes a body placeholder

In `src/interactive/parse_status.rs`, replace `CLOSING_KEYWORDS` with suffixes that insert the `:` null builtin (POSIX-defined, always valid as a body) before the closer:

```rust
const CLOSING_KEYWORDS: &[&str] = &[
    "\n:\nfi\n",
    "\n:\ndone\n",
    "\n:\nesac\n",
    "\n:\n}\n",
    "\n:\n)\n",
    "\n:\n;;\nesac\n",
];
```

Rationale:
- `:` satisfies the new non-empty compound_list rule.
- Each probe still appends only well-formed closer material that the parser would normally expect.
- Probes remain O(1) per suffix; total is still 6 reparses.

### Scope / Non-goals

- Not expanding `CLOSING_KEYWORDS` to cover more nesting depths; the existing six cover the same constructs as before.
- Not changing `is_complete_command_end` to recognize `DSemi`; Fix A already prevents the hang, and adding `DSemi` would alter case-statement parsing in non-trivial ways.
- Not refactoring the probe strategy itself (spec-aware incomplete detection) — tracked as future work.

## Testing

### Regression tests (existing)

- `tests/interactive.rs::test_classify_incomplete_if` — must pass, no longer hangs.
- `tests/interactive.rs::test_classify_incomplete_while` — must pass, no longer hangs.

### New unit tests in `src/parser/mod.rs`

1. `parse_simple_command_rejects_leading_dsemi` — `";;"` must return `Err(UnexpectedToken)`, must not hang.
2. `parse_simple_command_rejects_leading_pipe` — `"|"` must return `Err`.
3. `parse_simple_command_rejects_leading_rbrace` — `"}"` in contexts where it's not already treated as a terminator.
4. `parse_program_on_dsemi_in_then_body_errs` — `"if true; then\n;;\nesac"` must return `Err`, must not hang.

### New unit tests in `src/interactive/parse_status.rs`

1. `classify_incomplete_for` — `"for x in 1\n"` returns `Incomplete` (verifies the `\n:\ndone\n` probe path).
2. `classify_incomplete_brace_group` — `"{ true\n"` returns `Incomplete`.
3. `classify_error_not_hang_on_unterminated_garbage` — e.g. `"if ;; fi"` returns `Error` in finite time.

### Suite verification

- `cargo test` — full unit + integration pass.
- `./e2e/run_tests.sh` — E2E POSIX compliance clean run.
- Manual REPL smoke test: `if true; then<ENTER>` continues prompt (`PS2`), accepts `echo ok<ENTER>fi<ENTER>` and executes.

## Components

```
src/parser/mod.rs
├── parse_simple_command         <-- Fix A: error on empty result
└── (new tests)                  <-- parse_simple_command_rejects_*

src/interactive/parse_status.rs
├── CLOSING_KEYWORDS             <-- Fix B: body-containing probes
└── (new tests)                  <-- classify_incomplete_for, _brace_group, _error_not_hang

tests/interactive.rs
└── (existing)                   <-- test_classify_incomplete_if/_while now pass
```

## Risks & Mitigations

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Fix A breaks code paths that treated empty simple_command as "no-op" | Low | Full test suite run; grep for `SimpleCommand { words: vec![] ... }` patterns |
| Fix B probe still fails for some nested-incomplete case | Low | New tests cover `for`, brace group, subshell; can add targeted probes if found |
| Other parser entry points have similar zero-progress bugs | Medium | Defense-in-depth: Fix A localizes to `parse_simple_command`; document in TODO for broader audit |

## Follow-ups (out of scope)

- Audit remaining parser entry points (`parse_command`, `parse_compound_command` dispatcher) for similar non-advancing paths.
- Consider replacing the suffix-probe strategy in `classify_parse` with a structured "open-constructs stack" tracked by the lexer.
