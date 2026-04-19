# Empty `compound_list` Syntax Error Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make yosh reject empty `compound_list` productions (e.g. `if true; then fi`) as POSIX §2.10 syntax errors (exit 2, stderr containing `syntax`).

**Architecture:** Add a `context: &str` parameter to `parse_compound_list`, insert an `if commands.is_empty()` guard that returns `ShellError::parse`, and update all ten call sites to pass a fixed context label (e.g. `"'then' body"`). `case` item bodies are parsed by a separate inline loop and remain unaffected. No lexer, expander, or executor changes.

**Tech Stack:** Rust 2024 edition. Existing parser infrastructure in `src/parser/mod.rs`; existing `ShellError::parse(ParseErrorKind::UnexpectedToken, line, col, msg)` → exit 2 mapping at `src/error.rs:103`.

**Spec:** `docs/superpowers/specs/2026-04-19-empty-compound-list-syntax-error-design.md`

---

## File Structure

**Modify:**

- `src/parser/mod.rs` — change `parse_compound_list` signature + body; update ten call sites (in `parse_if_clause`, `parse_do_group`, `parse_while_clause`, `parse_until_clause`, `parse_brace_group`, `parse_subshell`); append unit tests inside the existing `#[cfg(test)] mod tests { ... }` block.
- `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh` — remove the `# XFAIL:` line.
- `TODO.md` — delete the `§2.10 Shell Grammar` entry.

**Create:** 10 new E2E test files under `e2e/posix_spec/2_10_shell_grammar/`:

- `empty_if_condition_is_error.sh`
- `empty_elif_body_is_error.sh`
- `empty_else_body_is_error.sh`
- `empty_while_condition_is_error.sh`
- `empty_while_body_is_error.sh`
- `empty_until_condition_is_error.sh`
- `empty_for_body_is_error.sh`
- `empty_brace_group_is_error.sh`
- `empty_subshell_is_error.sh`
- `case_empty_body_is_ok.sh` (regression guard — case with empty body must still parse)

---

## Task 0: Verify baseline

- [ ] **Step 1: Confirm baseline tests pass**

Run: `cargo test --lib 2>&1 | tail -3`
Expected: 594 passed.

- [ ] **Step 2: Confirm the XFAIL is present**

Run: `grep -n XFAIL e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`
Expected: a line matching `# XFAIL: parser accepts empty compound_list;`.

- [ ] **Step 3: Confirm the current call sites**

Run: `grep -n 'parse_compound_list()' src/parser/mod.rs`
Expected output (10 call sites):

```
427:        let condition = self.parse_compound_list()?;
429:        let then_part = self.parse_compound_list()?;
437:                let elif_cond = self.parse_compound_list()?;
439:                let elif_body = self.parse_compound_list()?;
443:                else_part = Some(self.parse_compound_list()?);
543:        let body = self.parse_compound_list()?;
551:        let condition = self.parse_compound_list()?;
559:        let condition = self.parse_compound_list()?;
640:        let body = self.parse_compound_list()?;
648:        let body = self.parse_compound_list()?;
```

If the line numbers have drifted, adapt the edits below accordingly by searching for the surrounding function name (`parse_if_clause`, `parse_do_group`, etc.).

---

## Task 1: Add `context` parameter and non-empty guard

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Append failing unit tests**

Locate the existing `#[cfg(test)] mod tests { ... }` block (near the bottom of `src/parser/mod.rs`). Append the following inside that block, before the closing `}`:

```rust
    // ── empty compound_list rejection (POSIX §2.10) ─────────────

    fn parse_err(source: &str) -> ShellError {
        Parser::new(source).parse_program().unwrap_err()
    }

    fn parse_ok(source: &str) {
        Parser::new(source)
            .parse_program()
            .unwrap_or_else(|e| panic!("expected OK, got: {e}"));
    }

    #[test]
    fn empty_if_then_errors() {
        let err = parse_err("if true; then fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("syntax"), "message: {s}");
        assert!(s.contains("'then' body"), "message: {s}");
    }

    #[test]
    fn empty_if_condition_errors() {
        let err = parse_err("if then true; fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("syntax"), "message: {s}");
        assert!(s.contains("'if' condition"), "message: {s}");
    }

    #[test]
    fn empty_elif_condition_errors() {
        let err = parse_err("if true; then :; elif then :; fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'elif' condition"), "message: {s}");
    }

    #[test]
    fn empty_elif_body_errors() {
        let err = parse_err("if true; then :; elif true; then fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'elif' body"), "message: {s}");
    }

    #[test]
    fn empty_else_body_errors() {
        let err = parse_err("if true; then :; else fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'else' body"), "message: {s}");
    }

    #[test]
    fn empty_while_condition_errors() {
        let err = parse_err("while do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'while' condition"), "message: {s}");
    }

    #[test]
    fn empty_while_body_errors() {
        let err = parse_err("while true; do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'do' body"), "message: {s}");
    }

    #[test]
    fn empty_until_condition_errors() {
        let err = parse_err("until do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'until' condition"), "message: {s}");
    }

    #[test]
    fn empty_until_body_errors() {
        let err = parse_err("until false; do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'do' body"), "message: {s}");
    }

    #[test]
    fn empty_for_body_errors() {
        let err = parse_err("for i in a b; do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'do' body"), "message: {s}");
    }

    #[test]
    fn empty_brace_group_errors() {
        let err = parse_err("{ }\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("brace group"), "message: {s}");
    }

    #[test]
    fn empty_subshell_errors() {
        let err = parse_err("( )\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("subshell"), "message: {s}");
    }

    #[test]
    fn nonempty_if_parses_ok() {
        parse_ok("if true; then :; fi\n");
    }

    #[test]
    fn case_empty_body_still_parses_ok() {
        parse_ok("case x in pat) ;; esac\n");
    }

    #[test]
    fn comment_only_body_errors_per_posix() {
        let err = parse_err("if true; then\n#only comment\nfi\n");
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("'then' body"));
    }
```

`ShellError` should already be in scope from the top-of-file `use`; if the test module cannot see it, add `use crate::error::ShellError;` at the top of the test module.

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test --lib parser::tests::empty 2>&1 | tail -30`
Expected: tests panic with messages like `called unwrap_err() on an Ok value` because the current parser accepts empty compound lists.

`cargo test --lib parser::tests::nonempty_if_parses_ok` and `parser::tests::case_empty_body_still_parses_ok` should already PASS in the red phase — they exercise today's permissive behavior for valid inputs.

`parser::tests::comment_only_body_errors_per_posix` will also fail (panic on `unwrap_err`).

Include the actual tail of output.

- [ ] **Step 3: Modify `parse_compound_list` signature and add the guard**

Locate `parse_compound_list` (currently at line 413 of `src/parser/mod.rs`). Replace the whole function with:

```rust
    /// Parse a compound_list: skip newlines, then parse complete_commands until at_end or is_complete_command_end.
    ///
    /// POSIX §2.10 requires at least one `and_or`. If the list would be
    /// empty, returns a parse error of the form
    /// `syntax error: empty compound list in {context}` so callers can
    /// surface context-aware diagnostics.
    pub fn parse_compound_list(
        &mut self,
        context: &str,
    ) -> error::Result<Vec<CompleteCommand>> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() && !self.is_complete_command_end() {
            let cmd = self.parse_complete_command()?;
            commands.push(cmd);
            self.skip_newlines()?;
        }
        if commands.is_empty() {
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("syntax error: empty compound list in {context}"),
            ));
        }
        Ok(commands)
    }
```

- [ ] **Step 4: Update all ten call sites**

Apply the following substitutions. After each, confirm the file compiles cleanly before the next.

In `parse_if_clause` (around line 425–448):

```rust
// BEFORE                                        // AFTER
let condition = self.parse_compound_list()?;     let condition = self.parse_compound_list("'if' condition")?;
let then_part = self.parse_compound_list()?;     let then_part = self.parse_compound_list("'then' body")?;
let elif_cond = self.parse_compound_list()?;     let elif_cond = self.parse_compound_list("'elif' condition")?;
let elif_body = self.parse_compound_list()?;     let elif_body = self.parse_compound_list("'elif' body")?;
else_part = Some(self.parse_compound_list()?);   else_part = Some(self.parse_compound_list("'else' body")?);
```

In `parse_do_group` (around line 541–546):

```rust
let body = self.parse_compound_list("'do' body")?;
```

In `parse_while_clause` (around line 549–554):

```rust
let condition = self.parse_compound_list("'while' condition")?;
// body stays via parse_do_group (which now passes "'do' body")
```

In `parse_until_clause` (around line 557–562):

```rust
let condition = self.parse_compound_list("'until' condition")?;
```

In `parse_brace_group` (around line 638–643):

```rust
let body = self.parse_compound_list("brace group")?;
```

In `parse_subshell` (around line 646–655):

```rust
let body = self.parse_compound_list("subshell")?;
```

Do NOT modify `parse_case_clause` (case bodies use a dedicated inline loop and POSIX permits empty case item bodies).

- [ ] **Step 5: Run tests to confirm they pass**

```bash
cargo build 2>&1 | tail -3
cargo test --lib parser::tests 2>&1 | tail -10
cargo test --lib 2>&1 | tail -5
```

Expected:
- Clean build.
- `parser::tests` all pass (15 new + existing).
- Full suite: 594 baseline + 15 new = 609 passed. If any pre-existing test depended on empty-compound-list permissiveness, it will fail here. Investigate and fix (or report to the coordinator) before committing.

- [ ] **Step 6: Commit**

```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
feat(parser): reject empty compound_list per POSIX §2.10

parse_compound_list now takes a context string and returns a parse
error when the resulting list is empty. Ten call sites (in
parse_if_clause, parse_do_group, parse_while_clause,
parse_until_clause, parse_brace_group, parse_subshell) now pass
context labels like "'then' body" or "'while' condition", producing
messages of the form:

    yosh: line 1: syntax error: empty compound list in 'then' body

case item bodies remain permissively empty (POSIX BNF allows them).

Task 1/3 of the empty-compound_list XFAIL remediation. See
docs/superpowers/specs/2026-04-19-empty-compound-list-syntax-error-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Flip XFAIL and add E2E tests

**Files:**
- Modify: `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`
- Create: 10 files under `e2e/posix_spec/2_10_shell_grammar/`

- [ ] **Step 1: Remove the XFAIL line**

Edit `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`. Delete this exact line:

```
# XFAIL: parser accepts empty compound_list; should be a syntax error per §2.10 BNF (term : term separator and_or | and_or)
```

Leave everything else intact.

- [ ] **Step 2: Verify the flip**

```bash
cargo build 2>&1 | tail -3
./e2e/run_tests.sh --filter=empty_compound_list 2>&1 | tail -5
```

Expected: `[PASS]  posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`. If FAIL, stop and report.

- [ ] **Step 3: Create `empty_if_condition_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_if_condition_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'if' condition (before 'then') is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if then true; fi
```

- [ ] **Step 4: Create `empty_elif_body_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_elif_body_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'elif' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then :; elif true; then fi
```

- [ ] **Step 5: Create `empty_else_body_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_else_body_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'else' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then :; else fi
```

- [ ] **Step 6: Create `empty_while_condition_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_while_condition_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'while' condition is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
while do done
```

- [ ] **Step 7: Create `empty_while_body_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_while_body_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'while' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
while true; do done
```

- [ ] **Step 8: Create `empty_until_condition_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_until_condition_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'until' condition is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
until do done
```

- [ ] **Step 9: Create `empty_for_body_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_for_body_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'for' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
for i in a; do done
```

- [ ] **Step 10: Create `empty_brace_group_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_brace_group_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty brace group is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
{ }
```

- [ ] **Step 11: Create `empty_subshell_is_error.sh`**

Path: `e2e/posix_spec/2_10_shell_grammar/empty_subshell_is_error.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty subshell is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
( )
```

- [ ] **Step 12: Create `case_empty_body_is_ok.sh` (regression guard)**

Path: `e2e/posix_spec/2_10_shell_grammar/case_empty_body_is_ok.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: case item bodies can be empty per POSIX BNF
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case x in
    pat) ;;
esac
echo ok
```

- [ ] **Step 13: Set permissions on all new files to 644**

```bash
chmod 644 \
  e2e/posix_spec/2_10_shell_grammar/empty_if_condition_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_elif_body_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_else_body_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_while_condition_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_while_body_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_until_condition_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_for_body_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_brace_group_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/empty_subshell_is_error.sh \
  e2e/posix_spec/2_10_shell_grammar/case_empty_body_is_ok.sh
```

- [ ] **Step 14: Run the filter**

```bash
./e2e/run_tests.sh --filter=2_10_shell_grammar 2>&1 | tail -20
```

Expected: every new test PASS; no FAIL / XPASS. Paste the tail into your report.

If a test unexpectedly fails (e.g., `case_empty_body_is_ok.sh` FAILs because the case parser accidentally routes through the now-strict `parse_compound_list`), stop and report — that indicates a gap in Task 1 that must be addressed before continuing.

- [ ] **Step 15: Commit**

```bash
git add e2e/posix_spec/2_10_shell_grammar/
git commit -m "$(cat <<'EOF'
test(parser): flip empty compound_list XFAIL and add §2.10 coverage

- e2e/posix_spec/.../empty_compound_list_in_if_is_error.sh: XFAIL
  removed; now PASSes.
- 9 new error-expected tests covering empty if condition, elif body,
  else body, while/until condition, while/do body, for body, brace
  group, and subshell.
- case_empty_body_is_ok.sh: regression guard asserting case items can
  still have empty bodies per POSIX BNF.

Task 2/3 of the empty-compound_list XFAIL remediation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: TODO.md cleanup and final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the completed TODO entry**

Open `TODO.md`. Under the section **"Future: POSIX Conformance Gaps (Chapter 2)"**, delete this exact line (project convention: delete rather than `[x]`):

```
- [ ] §2.10 Shell Grammar — parser accepts an empty `compound_list` inside `if ... then fi` (exit 0) instead of rejecting it as a syntax error; POSIX BNF `term : term separator and_or | and_or` requires at least one `and_or` (see `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh` XFAIL)
```

Leave other entries intact.

- [ ] **Step 2: Run the full verification suite**

```bash
cargo test --lib 2>&1 | tail -5
cargo fmt --check -- src/parser/mod.rs 2>&1 | head -20
cargo clippy --lib 2>&1 | grep -E "parser/mod.rs" | head -10
cargo build 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -5
```

Expected:
- `cargo test`: 609 passed (594 baseline + 15 new).
- `cargo fmt --check`: clean for `src/parser/mod.rs`. (Project-wide `cargo fmt --check` without a path argument may still show unrelated drift from prior sub-projects — do NOT reformat those files here.)
- `cargo clippy`: no new warnings for `src/parser/mod.rs`.
- E2E summary: `Total: 318  Passed: 317  Failed: 0  Timedout: 0  XFail: 1  XPass: 0` (baseline was 308; now 308 + 10 new = 318; the remaining XFail is §2.5.3 LINENO, handled by sub-project 4).

If `cargo fmt --check` flags `src/parser/mod.rs`, run `cargo fmt -- src/parser/mod.rs` and include the reformatted file in this task's commit. If the `rustfmt` edition-detection bug tracked in `TODO.md` (Future: Code Quality Improvements) prevents the check from working, fall back to `rustfmt --edition 2024 --check src/parser/mod.rs`.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
# Include src/parser/mod.rs only if fmt touched it.
git commit -m "$(cat <<'EOF'
chore(parser): remove completed §2.10 empty-compound_list TODO

The empty-compound_list conformance gap is closed by tasks 1-2. Per
project convention, completed TODO entries are deleted rather than
marked [x].

Task 3/3 of the empty-compound_list XFAIL remediation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Completion Criteria (final check)

1. `cargo test --lib` — 609 passed.
2. `cargo clippy --lib` — no new warnings in `src/parser/mod.rs`.
3. `cargo fmt --check -- src/parser/mod.rs` — clean (or clean under `rustfmt --edition 2024`).
4. `./e2e/run_tests.sh` summary: `XFail: 1, XPass: 0, Failed: 0, Timedout: 0`.
5. Three focused commits (Tasks 1–3), each with its task number in the body.
6. `TODO.md` no longer lists the `§2.10 Shell Grammar` gap.
