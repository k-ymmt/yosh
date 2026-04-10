# Phase 1: Known Limitations Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix two Phase 1 known limitations — nested command substitution edge cases in `read_balanced_parens`, and `pending_heredocs` field encapsulation.

**Architecture:** Two independent changes. (1) Add `$(`-detection with recursive self-call to `read_balanced_parens` so nested command substitutions with inner quoting are handled correctly. (2) Make `pending_heredocs` private, add `has_pending_heredocs()` accessor, update parser call sites.

**Tech Stack:** Rust, POSIX shell semantics

---

### Task 1: Add failing test for nested command substitution with quoted paren

**Files:**
- Modify: `tests/parser_integration.rs` (after line 176, in the command substitution tests section)

- [ ] **Step 1: Write the failing integration test**

Add after the existing `test_command_sub_in_assignment` test:

```rust
#[test]
fn test_nested_command_sub_with_quoted_paren() {
    let out = kish_exec("echo $(echo $(echo ')'))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), ")\n");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_nested_command_sub_with_quoted_paren -- --nocapture`
Expected: FAIL — the lexer misparses the nested command substitution due to the unbalanced paren counting.

- [ ] **Step 3: Commit failing test**

```bash
git add tests/parser_integration.rs
git commit -m "test: add failing test for nested command sub with quoted paren"
```

---

### Task 2: Add failing tests for additional nested command substitution cases

**Files:**
- Modify: `tests/parser_integration.rs` (after the test added in Task 1)

- [ ] **Step 1: Write failing tests for basic nesting and arithmetic inside command sub**

```rust
#[test]
fn test_nested_command_sub_basic() {
    let out = kish_exec("echo $(echo $(echo hello))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_command_sub_with_arith_inside() {
    let out = kish_exec("echo $(echo $((1+2)))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}
```

- [ ] **Step 2: Run tests to check which pass and which fail**

Run: `cargo test test_nested_command_sub_basic test_command_sub_with_arith_inside -- --nocapture`
Expected: `test_nested_command_sub_basic` likely passes (no quoting edge case). `test_command_sub_with_arith_inside` may pass or fail depending on current depth handling. Note the results — these serve as regression guards.

- [ ] **Step 3: Commit**

```bash
git add tests/parser_integration.rs
git commit -m "test: add nested command sub and arith-in-cmdsub tests"
```

---

### Task 3: Fix `read_balanced_parens` to handle nested `$(...)` recursively

**Files:**
- Modify: `src/lexer/mod.rs:1112-1203` (the `read_balanced_parens` function)

- [ ] **Step 1: Add `$(` detection with recursive self-call in the default match arm**

Replace the default match arm (lines 1195-1198) of `read_balanced_parens`:

```rust
                // current code:
                _ => {
                    content.push(ch as char);
                    self.advance();
                }
```

with:

```rust
                b'$' => {
                    self.advance();
                    if !self.at_end() && self.current_byte() == b'(' {
                        // Nested $(...) — recurse to handle inner quoting context
                        let inner = self.read_balanced_parens(span.clone())?;
                        content.push('$');
                        content.push('(');
                        content.push_str(&inner);
                        content.push(')');
                    } else {
                        content.push('$');
                    }
                }
                _ => {
                    content.push(ch as char);
                    self.advance();
                }
```

Note: Check whether `Span` implements `Clone`. If not, the span can be reconstructed via `self.current_span()` before the recursive call, or `Span` can be made `Clone`.

- [ ] **Step 2: Verify `Span` is `Clone`-able, fix if needed**

Check `src/lexer/token.rs` for the `Span` struct. If it does not derive `Clone`, add `Clone` to its derive list:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    // ...
}
```

Alternatively, pass `self.current_span()` to the recursive call instead of cloning:

```rust
                b'$' => {
                    self.advance();
                    if !self.at_end() && self.current_byte() == b'(' {
                        let inner_span = self.current_span();
                        let inner = self.read_balanced_parens(inner_span)?;
                        content.push('$');
                        content.push('(');
                        content.push_str(&inner);
                        content.push(')');
                    } else {
                        content.push('$');
                    }
                }
```

This approach is preferred if `Span` is not already `Clone`.

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass, including `test_nested_command_sub_with_quoted_paren` from Task 1.

- [ ] **Step 4: Commit**

```bash
git add src/lexer/mod.rs
git commit -m "fix(lexer): handle nested command substitution in read_balanced_parens

Detect \$( inside read_balanced_parens and recursively call self to
correctly handle inner quoting context. Fixes edge cases like
\$(echo \$(echo ')'))."
```

---

### Task 4: Add E2E tests for nested command substitution

**Files:**
- Create: `e2e/command_substitution/nested_quoted_paren.sh`
- Create: `e2e/command_substitution/nested_with_arith.sh`

- [ ] **Step 1: Create E2E test for quoted paren in nested command sub**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution with quoted closing paren
# EXPECT_OUTPUT: )
echo $(echo $(echo ')'))
```

- [ ] **Step 2: Create E2E test for arithmetic inside command sub**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Arithmetic expansion inside command substitution
# EXPECT_OUTPUT: 3
echo $(echo $((1+2)))
```

- [ ] **Step 3: Run E2E tests to verify they pass**

Run: `./e2e/run_tests.sh --filter=command_substitution`
Expected: All command substitution tests pass, including the two new ones.

- [ ] **Step 4: Commit**

```bash
git add e2e/command_substitution/nested_quoted_paren.sh e2e/command_substitution/nested_with_arith.sh
git commit -m "test(e2e): add nested command sub with quoted paren and arith tests"
```

---

### Task 5: Encapsulate `pending_heredocs` — add accessor and make field private

**Files:**
- Modify: `src/lexer/mod.rs:30` (field declaration)
- Modify: `src/lexer/mod.rs` (add accessor after existing accessor methods, around line 157)
- Modify: `src/parser/mod.rs:90,164,273` (replace direct field access)

- [ ] **Step 1: Add `has_pending_heredocs` accessor method**

Add after `process_pending_heredocs` (around line 158) in `src/lexer/mod.rs`:

```rust
    pub fn has_pending_heredocs(&self) -> bool {
        !self.pending_heredocs.is_empty()
    }
```

- [ ] **Step 2: Replace direct field access in parser**

In `src/parser/mod.rs`, replace all three occurrences of:

```rust
!self.lexer.pending_heredocs.is_empty()
```

with:

```rust
self.lexer.has_pending_heredocs()
```

Locations:
- Line 90: inside `skip_newlines`
- Line 164: inside separator parsing
- Line 273: inside simple command parsing

- [ ] **Step 3: Make `pending_heredocs` field private**

In `src/lexer/mod.rs` line 30, change:

```rust
    pub pending_heredocs: Vec<PendingHereDoc>,
```

to:

```rust
    pending_heredocs: Vec<PendingHereDoc>,
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All tests pass — no behavioral change, only encapsulation improvement.

- [ ] **Step 5: Run E2E tests**

Run: `./e2e/run_tests.sh`
Expected: All E2E tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lexer/mod.rs src/parser/mod.rs
git commit -m "refactor(lexer): encapsulate pending_heredocs field

Add has_pending_heredocs() accessor method and make the field private.
Replace three direct field accesses in parser with accessor call."
```

---

### Task 6: Update TODO.md — remove completed Phase 1 items

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the Phase 1 section from TODO.md**

Delete the entire `## Phase 1: Known Limitations` section (lines 3-6) and the blank line after it. The two items are now resolved.

- [ ] **Step 2: Run full test suite to confirm everything is clean**

Run: `cargo test && ./e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed Phase 1 known limitations from TODO.md"
```
