# classify_parse Hang Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop `test_classify_incomplete_if` / `test_classify_incomplete_while` from hanging the test runner, and make them report `ParseStatus::Incomplete` as the tests assert.

**Architecture:** Two coordinated fixes in two files. (1) `parse_simple_command` returns `Err(UnexpectedToken)` when it would otherwise produce a zero-progress empty command — closing the POSIX §2.9.1 grammar hole that lets `parse_compound_list` loop forever on tokens like `DSemi`. (2) `CLOSING_KEYWORDS` in `classify_parse::is_completable` appends a `:` null-builtin body before each closing keyword, so probes satisfy the non-empty compound_list rule introduced by commit `fe7c31c`.

**Tech Stack:** Rust 2024 edition, `cargo test`, Criterion (unaffected), project-local POSIX E2E harness at `e2e/run_tests.sh`.

**Spec:** `docs/superpowers/specs/2026-04-20-classify-parse-hang-fix-design.md`

---

## File Structure

- **Modify:** `src/parser/mod.rs` — add empty-result guard to `parse_simple_command`; add parser-unit tests.
- **Modify:** `src/interactive/parse_status.rs` — replace `CLOSING_KEYWORDS` with body-padded variants; add `classify_parse` regression tests (if existing test file allows inline `#[test]`, else add them to `tests/interactive.rs`).
- **Verify (no change):** `tests/interactive.rs::test_classify_incomplete_if` and `_while` go from hanging to passing.
- **Modify:** `TODO.md` — delete the completed hang entry (line 69 pattern) per project convention (never use `[x]` markers).

---

## Task 1: Fix A — parse_simple_command rejects empty result

**Files:**
- Modify: `src/parser/mod.rs:272-319` (`parse_simple_command`)
- Modify: `src/parser/mod.rs:1046` onwards (`mod tests`)

### - [ ] Step 1.1: Write the failing test

Append to `src/parser/mod.rs` inside `mod tests { ... }`:

```rust
#[test]
fn parse_program_on_leading_dsemi_errs_not_hangs() {
    // Regression guard: DSemi at start of a simple command used to cause
    // parse_simple_command to return Ok with zero progress, which made
    // parse_compound_list loop forever. See
    // docs/superpowers/specs/2026-04-20-classify-parse-hang-fix-design.md.
    let mut p = Parser::new(";;");
    let err = p
        .parse_program()
        .expect_err("';;' must not parse as a program");
    assert!(
        err.message.contains("unexpected token")
            || err.message.contains("syntax error"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn parse_program_on_leading_pipe_errs() {
    let mut p = Parser::new("|");
    assert!(p.parse_program().is_err());
}

#[test]
fn parse_program_on_dsemi_in_then_body_errs_not_hangs() {
    // The exact input that the original hang reproduced on — the 6th
    // is_completable probe candidate for "if true; then\n".
    let mut p = Parser::new("if true; then\n\n;;\nesac\n");
    assert!(p.parse_program().is_err());
}
```

### - [ ] Step 1.2: Run the non-hanging failing test to confirm pre-fix behavior

Pre-fix behavior of the three new tests:

| Test | Pre-fix behavior | Post-fix |
|------|-----------------|----------|
| `parse_program_on_leading_pipe_errs` | `parse_program("|")` returns `Ok` with a malformed pipeline (no hang). Test's `is_err()` assertion fails with a panic. | PASS |
| `parse_program_on_leading_dsemi_errs_not_hangs` | `parse_program(";;")` **hangs** — `parse_complete_command` returns `Ok` with zero progress, and `parse_program`'s outer `while !is_at_end` loop reinvokes it forever. | PASS |
| `parse_program_on_dsemi_in_then_body_errs_not_hangs` | **Hangs** — same mechanism inside `parse_compound_list('then' body)`. | PASS |

Only the non-hanging test can be run safely pre-fix. Verify it fails in the expected way:

```bash
cargo test --lib parse_program_on_leading_pipe_errs -- --nocapture
```

Expected (pre-fix): test FAILS with panic like `assertion failed: p.parse_program().is_err()`.

**Do NOT** run the other two tests pre-fix — they will hang the test runner and consume unbounded memory (observed ~1.6 GB in 5 seconds during diagnosis). Steps 1.4 post-fix verifies all three.

### - [ ] Step 1.3: Implement the empty-result guard

Edit `src/parser/mod.rs:272-319`. Replace the final `Ok(SimpleCommand { ... })` block with:

```rust
    pub fn parse_simple_command(&mut self) -> error::Result<SimpleCommand> {
        let line = self.current.span.line;
        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            // Try redirect first
            if let Some(redirect) = self.try_parse_redirect()? {
                redirects.push(redirect);
                continue;
            }

            // Check for word token
            if let Token::Word(word) = &self.current.token.clone() {
                let word = word.clone();

                // Only try assignments before any command words have been seen
                if words.is_empty()
                    && let Some(assignment) = Self::try_parse_assignment(&word)
                {
                    self.advance()?;
                    assignments.push(assignment);
                    continue;
                }

                // It's a regular word
                self.advance()?;
                words.push(word);
                continue;
            }

            // If we hit a newline and have pending heredocs, process them now
            if self.current.token == Token::Newline && self.lexer.has_pending_heredocs() {
                self.lexer.process_pending_heredocs()?;
            }

            // End of simple command
            break;
        }

        // POSIX §2.9.1: a simple_command derives from at least one of
        // cmd_prefix (assignment/redirect), cmd_name (word), or cmd_word
        // (word). A zero-progress return would let callers like
        // parse_compound_list loop forever on unhandled operator tokens
        // (e.g. DSemi, Pipe in unexpected positions). Reject explicitly.
        if assignments.is_empty() && words.is_empty() && redirects.is_empty() {
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "syntax error: unexpected token at start of command",
            ));
        }

        Ok(SimpleCommand {
            assignments,
            words,
            redirects,
            line,
        })
    }
```

Key change: add the `if assignments.is_empty() && words.is_empty() && redirects.is_empty()` guard immediately before the final `Ok(SimpleCommand { .. })`.

### - [ ] Step 1.4: Run the three new tests — they must pass

```bash
cargo test --lib parse_program_on_leading_dsemi_errs_not_hangs \
                 parse_program_on_leading_pipe_errs \
                 parse_program_on_dsemi_in_then_body_errs_not_hangs
```

Expected: all three PASS within seconds. The `_in_then_body_` case is the critical one — it must complete in <5 seconds (previously unbounded).

### - [ ] Step 1.5: Run the full parser test module — verify no existing tests broke

```bash
cargo test --lib parser::tests
```

Expected: all existing parser tests PASS. If any test regresses, the failure is the most valuable signal the plan produces — investigate before proceeding. A test that passed on empty simple commands would rely on latent bug behavior.

### - [ ] Step 1.6: Commit

```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
fix(parser): reject zero-progress empty simple_command

parse_simple_command previously returned Ok(SimpleCommand {
assignments: [], words: [], redirects: [] }) when the current token
was neither a Word nor a redirect (e.g. DSemi, Pipe). Combined with
parse_separator_op returning None for the same tokens,
parse_complete_command advanced zero bytes yet returned Ok, letting
parse_compound_list loop forever while growing commands: Vec.

Root cause of test_classify_incomplete_if / _while hanging >60s after
commit fe7c31c. POSIX §2.9.1 requires every simple_command derivation
to have at least one of cmd_prefix, cmd_name, or cmd_word.

See docs/superpowers/specs/2026-04-20-classify-parse-hang-fix-design.md.

Task: "TODO.md の中から優先度が高いものを対応してください" (A:
test_classify_incomplete_if/_while hang fix).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Fix B — CLOSING_KEYWORDS probes include `:` body

**Files:**
- Modify: `src/interactive/parse_status.rs:17-24` (`CLOSING_KEYWORDS`)

### - [ ] Step 2.1: Verify current state of the failing integration tests

With Task 1 applied, the original hanging tests should no longer hang — they should fail with "expected Incomplete, got Error". Run them:

```bash
cargo test --test interactive test_classify_incomplete_if \
                              test_classify_incomplete_while -- --nocapture
```

Expected: both tests **fail** (no longer hang), with panic messages like `expected Incomplete, got Error("syntax error: empty compound list in 'then' body")`.

If they still hang, Task 1 is incomplete — return to Task 1 before proceeding.

### - [ ] Step 2.2: Update `CLOSING_KEYWORDS`

Edit `src/interactive/parse_status.rs:17-24`. Replace the constant with body-padded variants:

```rust
/// Closing-keyword probes for `is_completable`: each suffix wraps a
/// single `:` null builtin (POSIX-defined, always valid) before the
/// closer, so the probe satisfies the non-empty `compound_list` rule
/// introduced in commit `fe7c31c` (2026-04-19). Without the `:` body,
/// every probe would produce an empty `then`/`do`/`else` body and fail
/// with `syntax error: empty compound list in <ctx>`, making genuinely
/// incomplete input indistinguishable from genuinely invalid input.
const CLOSING_KEYWORDS: &[&str] = &[
    "\n:\nfi\n",
    "\n:\ndone\n",
    "\n:\nesac\n",
    "\n:\n}\n",
    "\n:\n)\n",
    "\n:\n;;\nesac\n",
];
```

### - [ ] Step 2.3: Re-run the two originally-failing tests

```bash
cargo test --test interactive test_classify_incomplete_if \
                              test_classify_incomplete_while -- --nocapture
```

Expected: **both PASS** within seconds.

### - [ ] Step 2.4: Commit

```bash
git add src/interactive/parse_status.rs
git commit -m "$(cat <<'EOF'
fix(interactive): add `:` body to classify_parse closing-keyword probes

is_completable appends each CLOSING_KEYWORDS suffix to classify whether
an input is Incomplete vs. Error. After commit fe7c31c made
parse_compound_list reject empty lists, every probe for an
if/while/for/brace-group/subshell that terminates at its header (e.g.
`if true; then\n`) produced an empty body and failed parsing — so
classify_parse mis-classified incomplete input as Error.

Prepend a `:` null builtin before each closer. `:` is POSIX-defined and
always valid, so each probe now yields a well-formed compound_list with
a single no-op command.

Makes tests/interactive.rs::test_classify_incomplete_if and
test_classify_incomplete_while pass.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Additional classify_parse regression tests

**Files:**
- Modify: `tests/interactive.rs` (append to existing `classify_parse` test block near line 244)

### - [ ] Step 3.1: Add three new regression tests

Append after the existing `test_classify_incomplete_while` (currently ends around `tests/interactive.rs:260`):

```rust
#[test]
fn test_classify_incomplete_for() {
    // Verifies the `\n:\ndone\n` probe path via for-loop header-only input.
    let aliases = AliasStore::default();
    match classify_parse("for x in 1\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_brace_group() {
    // Verifies the `\n:\n}\n` probe path via open-brace-only input.
    let aliases = AliasStore::default();
    match classify_parse("{ true\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_does_not_hang_on_dsemi_garbage() {
    // Regression guard: the `\n:\n;;\nesac\n` probe candidate
    // "if true; then\n\n;;\nesac\n" used to cause parse_compound_list
    // to loop forever. With the parse_simple_command empty-result
    // guard, classify_parse must return in finite time.
    let aliases = AliasStore::default();
    let _ = classify_parse("if ;;\n", &aliases);
    // Test passes as long as it returns (no assertion on specific
    // classification; correctness of the specific variant is covered
    // by parser-level tests in Task 1).
}
```

### - [ ] Step 3.2: Run the new tests

```bash
cargo test --test interactive test_classify_incomplete_for \
                              test_classify_incomplete_brace_group \
                              test_classify_does_not_hang_on_dsemi_garbage
```

Expected: all three PASS within seconds.

### - [ ] Step 3.3: Commit

```bash
git add tests/interactive.rs
git commit -m "$(cat <<'EOF'
test(interactive): add classify_parse regression coverage

- test_classify_incomplete_for: exercises the `\n:\ndone\n` probe path
- test_classify_incomplete_brace_group: exercises the `\n:\n}\n` probe
- test_classify_does_not_hang_on_dsemi_garbage: wall-clock guard on
  the probe that originally triggered the unbounded loop

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Full suite verification + TODO cleanup

**Files:**
- Modify: `TODO.md` (delete the completed entry, do NOT mark with `[x]`)

### - [ ] Step 4.1: Run the full unit + integration test suite

```bash
cargo test
```

Expected: all tests PASS. Pay particular attention to any parser or interactive tests that might have depended on the old empty-simple-command behavior.

If any test fails, do **not** patch by loosening the Task 1 guard. Instead, investigate whether the failing test encoded behavior that is actually POSIX-invalid, and either update the test to match the new (correct) behavior or narrow the guard — but only after understanding the failure.

### - [ ] Step 4.2: Run the E2E POSIX compliance suite

```bash
./e2e/run_tests.sh
```

Expected: clean pass rate matching the pre-fix baseline. If new E2E failures appear, triage — POSIX conformance is the primary project goal.

### - [ ] Step 4.3: Delete the completed TODO entry

Edit `TODO.md`. Find and delete (do not mark with `[x]`; project convention is deletion) this block near line 69:

```markdown
- [ ] `tests/interactive.rs::test_classify_incomplete_if` and `test_classify_incomplete_while` hang indefinitely (>60s, SIGKILL by cargo test runner). The tests call `classify_parse("if true; then\n", &aliases)` / `classify_parse("while true; do\n", &aliases)` expecting `ParseStatus::Incomplete`. Likely regression from the recent LINENO/compound_list parser work (af663e1/5920517/fe7c31c) — `classify_parse` probably never returns `Incomplete` for `then\n`/`do\n` with an empty body. Bisect to confirm, then fix the parser classification path.
```

Leave the surrounding list formatting intact.

### - [ ] Step 4.4: Commit the cleanup

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): remove completed classify_parse hang entry

Fixed by parse_simple_command empty-result guard +
CLOSING_KEYWORDS body-padded probes. See
docs/superpowers/plans/2026-04-20-classify-parse-hang-fix.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Acceptance Criteria

- `cargo test --test interactive test_classify_incomplete_if test_classify_incomplete_while` completes in <10 seconds and both tests pass.
- `cargo test` full run is green.
- `./e2e/run_tests.sh` shows no new failures vs. pre-fix baseline.
- `TODO.md` no longer contains the hang entry.
- Four commits in total (one per Task 1/2/3/4) with bodies referencing the spec.
