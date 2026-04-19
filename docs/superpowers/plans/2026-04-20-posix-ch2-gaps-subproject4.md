# POSIX §2.6.1 Escape Metadata Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve backslash-escape semantics for tilde expansion by introducing `WordPart::EscapedLiteral`, making `\<newline>` line-continuation transparent in the lexer, routing `export`/`readonly` args through `try_parse_assignment` pre-expansion, and removing the `prev_was_literal` heuristic + `expand_tilde_in_assignment_value` helper.

**Architecture:** Four-layer, bottom-up: (1) AST variant + match-site network, (2) lexer emits EscapedLiteral / skips line-continuation, (3) parser removes prev_was_literal, (4) executor routes export/readonly through assignment parser and retires the string-based tilde-expansion helper.

**Tech Stack:** Rust 2024 edition, yosh parser/lexer/executor, existing `e2e/run_tests.sh` harness.

**Spec:** `docs/superpowers/specs/2026-04-20-posix-ch2-gaps-subproject4-design.md`

---

## Prerequisites (before Task 1)

- [ ] **Step 0.1: Build**

```bash
cargo build
```
Expected: clean build (7 pre-existing warnings).

- [ ] **Step 0.2: Record baseline**

```bash
cargo test --lib 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected:
- Lib: `test result: ok. 627 passed`
- E2E: `Total: 368  Passed: 367  Failed: 0  XFail: 1`

If counts differ, stop and reconcile.

---

## Task 1 (Commit ①): Add `WordPart::EscapedLiteral` + match-site network

**Files:**
- Modify: `src/parser/ast.rs` (add variant)
- Modify: `src/expand/mod.rs` (add arms in 2 match statements)
- Modify: `src/parser/mod.rs` (add arm in `try_parse_assignment` walker)
- Modify: `src/exec/mod.rs` (add arm in word-to-string match)

### Step 1.1: Add the variant

In `/Users/kazukiyamamoto/Projects/rust/kish/src/parser/ast.rs` around line 144–154, change the `WordPart` enum:

Currently:
```rust
pub enum WordPart {
    Literal(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<WordPart>),
    DollarSingleQuoted(String),
    Parameter(ParamExpr),
    CommandSub(Program),
    ArithSub(String),
    Tilde(Option<String>),
}
```

- [ ] Insert `EscapedLiteral(String),` as the SECOND variant (after `Literal`):

```rust
pub enum WordPart {
    Literal(String),
    /// A sequence of characters that came through a `\<char>` unquoted escape
    /// (or a `\$`/`\\`/`\"`/`` \` `` escape inside double quotes). Expands
    /// identically to `Literal` in the output but is excluded from tilde-prefix
    /// recognition in assignment values — the backslash that produced it
    /// explicitly suppresses tilde expansion per POSIX §2.6.1.
    EscapedLiteral(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<WordPart>),
    DollarSingleQuoted(String),
    Parameter(ParamExpr),
    CommandSub(Program),
    ArithSub(String),
    Tilde(Option<String>),
}
```

### Step 1.2: Add expander arm in `expand_part_to_fields`

In `src/expand/mod.rs` around line 358–435, find the match on `WordPart` inside `expand_part_to_fields`. The existing `WordPart::Literal(s)` arm at line ~366 pushes to `fields.last_mut().unwrap().push_quoted(s)` / `push_unquoted(s)`.

- [ ] Add an `EscapedLiteral` arm immediately after `Literal` with identical body:

```rust
        WordPart::Literal(s) => {
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(s);
            } else {
                fields.last_mut().unwrap().push_unquoted(s);
            }
        }
        WordPart::EscapedLiteral(s) => {
            // Expand identically to Literal — the escape served its purpose
            // at parse time by suppressing tilde recognition. The escape also
            // removes the "was splittable on IFS" property, so escaped text
            // must be treated as quoted content for field splitting.
            fields.last_mut().unwrap().push_quoted(s);
        }
```

### Step 1.3: Add expander arm in the simpler match (no-split path)

`src/expand/mod.rs` has another smaller match at line ~309 (`expand_part_to_string` or similar — look for the match where `WordPart::Literal(s) => out.push_str(s)`). This path is used for double-quoted contexts and similar.

- [ ] Add `EscapedLiteral` arm next to `Literal`:

```rust
        WordPart::Literal(s) => out.push_str(s),
        WordPart::EscapedLiteral(s) => out.push_str(s),
```

### Step 1.4: Add arm in `src/exec/mod.rs`

Around line 57–58 of `src/exec/mod.rs` there is a match like:
```rust
        WordPart::Literal(lit) => s.push_str(lit),
        WordPart::SingleQuoted(lit) => { ... }
```

- [ ] Add EscapedLiteral arm:

```rust
        WordPart::Literal(lit) => s.push_str(lit),
        WordPart::EscapedLiteral(lit) => s.push_str(lit),
        WordPart::SingleQuoted(lit) => { ... }
```

### Step 1.5: Add arm in parser walker (`try_parse_assignment`)

In `src/parser/mod.rs` around line 399–414, the walker's match on `part` currently has `WordPart::Literal(s) => { ... }` and `other => { ... }`. The catch-all `other` arm handles all non-Literal variants including Parameter, CommandSub, etc.

With `EscapedLiteral` added, the catch-all `other` arm will transparently cover it — pushing the part as-is and resetting `at_boundary = false`, `prev_was_literal = false`. This matches our desired semantics for EscapedLiteral.

- [ ] **No code change needed in the walker at this step.** The catch-all naturally handles the new variant. Verify by running tests (Step 1.6).

If for clarity you want an explicit arm, add it AFTER `Literal` and BEFORE the catch-all, with identical body to the catch-all:
```rust
                WordPart::EscapedLiteral(_) => {
                    value_parts.push(part.clone());
                    at_boundary = false;
                    prev_was_literal = false;
                }
```

Either approach is correct for Task 1. Defer the explicit arm to Task 3 when `prev_was_literal` is deleted.

### Step 1.6: Verify compilation and full test suite

- [ ] Run:
```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -5
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected:
- Build: clean (no new warnings beyond the 7 existing).
- Lib: `test result: ok. 627 passed` (unchanged — lexer doesn't emit EscapedLiteral yet).
- E2E: `Total: 368  Passed: 367  Failed: 0  XFail: 1` (unchanged).

If the build fails with a non-exhaustive match error in a file not listed above, add an `EscapedLiteral(_) => { /* same as Literal */ }` arm there and note the location. Common suspects: `src/parser/mod.rs` other match statements, `src/builtin/*.rs`, test assertions.

### Step 1.7: Commit

```bash
git add src/parser/ast.rs src/expand/mod.rs src/exec/mod.rs src/parser/mod.rs
git commit -m "$(cat <<'EOF'
feat(ast): add WordPart::EscapedLiteral variant

New variant to distinguish characters that came through a `\<char>`
unquoted escape (or `\$`/`\\`/`\"`/`\\\`` escape inside double quotes)
from plain Literal content. Expander treats it identically to Literal
but emits `push_quoted` to match the escape's "not subject to field
splitting" property. Lexer does not yet emit this variant — Task 2
activates the emission.

Prerequisite plumbing for sub-project 4's fix of two bugs:
- `export NAME=\~/val` wrongly expands
- `x=foo:\<newline>~/bin` does not expand

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 (Commit ②): Lexer emits EscapedLiteral; line-continuation transparent

**Files:**
- Modify: `src/lexer/word.rs` (unquoted `\` handling, double-quoted `\` handling, caller block)
- Modify: `src/lexer/mod.rs` (existing `test_line_continuation` around line 258; add 2 new tests)

### Step 2.1: Update unquoted backslash handling

In `/Users/kazukiyamamoto/Projects/rust/kish/src/lexer/word.rs` lines 91–96 and the function `read_backslash` at lines 196–213.

Current (lines 91–96):
```rust
                    b'\\' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backslash()?;
                        parts.push(part);
                    }
```

- [ ] Replace with a version that checks for line-continuation BEFORE flushing:

```rust
                    b'\\' => {
                        // Peek at the next byte to differentiate \<newline> line-
                        // continuation (transparent — don't flush, don't emit) from
                        // \<char> escape (flush and emit EscapedLiteral).
                        if self.peek_next_byte() == Some(b'\n') {
                            self.advance(); // consume '\'
                            self.advance(); // consume '\n'
                            // loop continues with `literal` still accumulating
                        } else {
                            if !literal.is_empty() {
                                parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                            }
                            let part = self.read_escape_unquoted()?;
                            parts.push(part);
                        }
                    }
```

### Step 2.2: Replace `read_backslash` with `read_escape_unquoted`

- [ ] Rename the function at lines 196–213 and simplify it (no more line-continuation branch — it's handled in the caller):

Current body:
```rust
    /// Handles `\` outside double quotes.
    /// `\<newline>` is line continuation (returns empty Literal, filtered later).
    /// Otherwise returns literal of next char.
    fn read_backslash(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        if ch == b'\n' {
            // line continuation: consume newline, return empty literal (filtered later)
            self.advance();
            Ok(WordPart::Literal(String::new()))
        } else {
            self.advance();
            Ok(WordPart::Literal((ch as char).to_string()))
        }
    }
```

Replace with:
```rust
    /// Handles `\<char>` escape outside double quotes. The caller MUST have
    /// confirmed the next byte is not `\n` (line-continuation is handled inline
    /// in `read_word_parts`). Returns EscapedLiteral for the escaped character.
    fn read_escape_unquoted(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            // Trailing `\` with no following char: per POSIX, treat as a literal
            // backslash (the behavior of a backslash at EOF is implementation-
            // defined; yosh preserves it).
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        self.advance();
        Ok(WordPart::EscapedLiteral((ch as char).to_string()))
    }
```

### Step 2.3: Add or verify `peek_next_byte` helper

The caller in Step 2.1 uses `self.peek_next_byte()`. Verify whether this exists in the Lexer struct.

- [ ] Run:
```bash
grep -n 'fn peek_next_byte\|fn peek_byte\|fn peek\b' src/lexer/mod.rs src/lexer/scanner.rs
```

If a `peek_next_byte(&self) -> Option<u8>` method exists, use it directly. If the method name differs (e.g., `peek_ahead`), use that name in Step 2.1. If no such method exists, add this helper to `src/lexer/scanner.rs` (or wherever `current_byte` is defined):

```rust
    pub(crate) fn peek_next_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }
```

Exact field name (`bytes` / `pos`) may differ — inspect the existing `current_byte` implementation and mirror its style.

### Step 2.4: Update double-quoted backslash handling

In `src/lexer/word.rs` lines 216–238, `read_backslash_in_double_quote`.

Current:
```rust
    fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        match ch {
            b'$' | b'`' | b'"' | b'\\' => {
                self.advance();
                Ok(WordPart::Literal((ch as char).to_string()))
            }
            b'\n' => {
                // line continuation
                self.advance();
                Ok(WordPart::Literal(String::new()))
            }
            _ => {
                // backslash is kept literally
                self.advance();
                Ok(WordPart::Literal(format!("\\{}", ch as char)))
            }
        }
    }
```

- [ ] Replace with:

```rust
    /// Inside double quotes, `\` only escapes `$ ` " \ <newline>`.
    /// For the escaped forms, returns EscapedLiteral. For the line-continuation
    /// form (`\<newline>`), returns an empty Literal that the caller's post-
    /// loop filter removes.
    fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        match ch {
            b'$' | b'`' | b'"' | b'\\' => {
                self.advance();
                Ok(WordPart::EscapedLiteral((ch as char).to_string()))
            }
            b'\n' => {
                // Line continuation inside double quotes: consume the newline.
                // The empty Literal is stripped by the post-loop filter at the
                // outer `read_word_parts`.
                self.advance();
                Ok(WordPart::Literal(String::new()))
            }
            _ => {
                // Non-special escape in double quotes: preserve the backslash
                // as-is (per POSIX §2.2.3). This is plain Literal (not
                // EscapedLiteral) because no semantic escape occurred.
                self.advance();
                Ok(WordPart::Literal(format!("\\{}", ch as char)))
            }
        }
    }
```

Note: double-quoted line-continuation keeps the existing empty-Literal path because the outer filter still runs. Only unquoted line-continuation is made transparent at the caller level.

### Step 2.5: Inspect and update existing lexer line-continuation test

- [ ] Run:
```bash
grep -n 'test_line_continuation\|line_continuation' src/lexer/mod.rs
```
Inspect the test body. It likely asserts a specific token/word shape.

- [ ] If the test asserts the number or content of `WordPart::Literal` entries for a `\<newline>` input, update the assertion to match the new behavior: unquoted `foo\<newline>bar` now produces a single merged Literal `"foobar"` (no split). If the test was at the full-tokenize level and only checked the resulting string, it may continue to pass unchanged.

If you cannot run the test to see its current failure mode at this point, make a best-effort assertion update based on reading the body, then run `cargo test --lib test_line_continuation` to verify.

### Step 2.6: Add new lexer unit tests

Locate the lexer test module (likely in `src/lexer/mod.rs` near the bottom or in a `mod tests` block).

- [ ] Add these two tests at the end of the test module:

```rust
    #[test]
    fn lexer_backslash_escape_emits_escaped_literal() {
        use crate::parser::ast::WordPart;
        let mut lexer = Lexer::new("x=\\~/bin");
        let tok = lexer.next_token().expect("token");
        // The token should be Word with parts [Literal("x="), EscapedLiteral("~"), Literal("/bin")]
        let parts = match &tok.token {
            crate::lexer::token::Token::Word(w) => &w.parts,
            other => panic!("expected Word, got {:?}", other),
        };
        let has_escaped = parts.iter().any(|p| matches!(p, WordPart::EscapedLiteral(s) if s == "~"));
        assert!(
            has_escaped,
            "expected EscapedLiteral(~) in parts, got {:?}",
            parts
        );
    }

    #[test]
    fn lexer_line_continuation_merges_literals() {
        use crate::parser::ast::WordPart;
        let mut lexer = Lexer::new("x=foo\\\nbar");
        let tok = lexer.next_token().expect("token");
        let parts = match &tok.token {
            crate::lexer::token::Token::Word(w) => &w.parts,
            other => panic!("expected Word, got {:?}", other),
        };
        // Must be exactly one Literal("x=foobar") — line continuation is transparent
        assert_eq!(parts.len(), 1, "expected single merged Literal, got {:?}", parts);
        match &parts[0] {
            WordPart::Literal(s) => assert_eq!(s, "x=foobar"),
            other => panic!("expected Literal, got {:?}", other),
        }
    }
```

The exact Lexer API (`Lexer::new`, `next_token`) may differ — inspect `src/lexer/mod.rs` for the public interface used by existing tests and mirror their pattern. If the existing tests use `Lexer::new(source)` followed by token extraction helpers, reuse those helpers.

### Step 2.7: Run tests

- [ ] Run:
```bash
cargo test --lib -- lexer 2>&1 | tail -20
```
Expected: all lexer tests pass (including the 2 new). If `test_line_continuation` or similar fails, fix its assertion per Step 2.5.

- [ ] Run:
```bash
cargo test --lib 2>&1 | tail -5
./e2e/run_tests.sh --filter=tilde 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected lib count: 629 passed (627 baseline + 2 new lexer tests). **However**, the pre-existing test `assignment_rhs_line_continuation_tilde_known_regression` at `src/parser/mod.rs` (added in sub-project 3 fixup `7d74bab`) asserts `!has_tilde` for `x=foo:\<newline>~/bin`. After the Step 2 lexer change, the line-continuation is transparent, so the assignment value becomes a single Literal `"foo:~/bin"` and `split_tildes_in_literal` DOES produce a Tilde node. The test will FAIL.

### Step 2.8: Flip the line-continuation pinning test

- [ ] Locate `assignment_rhs_line_continuation_tilde_known_regression` in `src/parser/mod.rs` (around line 1699). The current body:

```rust
    #[test]
    fn assignment_rhs_line_continuation_tilde_known_regression() {
        // `x=foo:\<newline>~/bin` — POSIX §2.2.1 removes the backslash+newline
        // before tokenization, so the RHS is equivalent to `foo:~/bin` with the
        // tilde expanded. yosh currently produces `[Literal("foo:"), Literal("~/bin")]`
        // (tilde NOT expanded) because the lexer splits at the continuation,
        // making the two Literals adjacent and triggering the prev_was_literal
        // suppression. This is a known pre-existing regression (not caused by
        // this sub-project); sub-project 4 will fix it via escape metadata.
        let (_, parts) = parse_first_assignment("x=foo:\\\n~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(
            !has_tilde,
            "If this assertion flips to passing (tilde IS now in parts), the \
             line-continuation regression has been fixed and this test should \
             be updated to `assert!(has_tilde, ...)`. parts = {:?}",
            parts
        );
    }
```

- [ ] Replace the entire test with:

```rust
    #[test]
    fn assignment_rhs_line_continuation_tilde_expands() {
        // POSIX §2.2.1: `\<newline>` is removed before tokenization, so
        // `x=foo:\<newline>~/bin` is semantically identical to `x=foo:~/bin`
        // and the tilde MUST expand at the ':' boundary.
        let (_, parts) = parse_first_assignment("x=foo:\\\n~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(has_tilde, "parts = {:?}", parts);
    }
```

### Step 2.9: Re-run tests

- [ ] Run:
```bash
cargo test --lib 2>&1 | tail -5
./e2e/run_tests.sh --filter=tilde 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected:
- Lib: `test result: ok. 629 passed` (627 baseline + 2 new lexer + 0 net parser — the flip replaces one test with one test).
- Tilde E2E: all 18 pass.
- Full E2E: `Total: 368 Passed: 367 Failed: 0 XFail: 1` (unchanged — no new E2E files yet).

### Step 2.10: Commit

```bash
git add src/lexer/word.rs src/lexer/mod.rs src/parser/mod.rs src/lexer/scanner.rs
git commit -m "$(cat <<'EOF'
feat(lexer): emit EscapedLiteral; make \<newline> transparent

- `\<char>` unquoted escape now emits WordPart::EscapedLiteral(char)
  instead of Literal(char), preserving the escape information for
  downstream tilde-prefix recognition.
- `\<newline>` unquoted line-continuation is now consumed inline in
  the outer dispatch loop without flushing the accumulated literal,
  so forms like `foo\<newline>bar` produce a single merged Literal
  instead of adjacent Literals with an empty filtered entry between.
- Double-quoted `\$`, `\\`, `\"`, `\\\`` similarly emit EscapedLiteral.
- Flips the sub-project-3 pinning test `assignment_rhs_line_
  continuation_tilde_known_regression` → `_expands` as the line-
  continuation now expands tilde correctly (POSIX §2.2.1).

Task 3 removes the prev_was_literal heuristic that the adjacent-
Literal trigger was propping up; the fix now flows from precise
AST metadata rather than a lexer-split proxy.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

(Include `src/lexer/scanner.rs` in the `git add` only if Step 2.3 required adding `peek_next_byte` there.)

---

## Task 3 (Commit ③): Remove `prev_was_literal`; add EscapedLiteral parser test

**Files:**
- Modify: `src/parser/mod.rs` (walker block + doc comment + new unit test)

### Step 3.1: Remove `prev_was_literal` from the walker block

Locate the walker block in `try_parse_assignment` at approximately `src/parser/mod.rs:358–414`. The current body includes:
- A 28-line doc comment explaining `prev_was_literal` (lines 358–384)
- Additional inline comment at lines 387–392
- `let mut prev_was_literal = true;` at line 393
- Inside `match part { WordPart::Literal(s) => { ... let effective_boundary = ...; ... prev_was_literal = true; } ... other => { ... prev_was_literal = false; } }`

- [ ] Replace the entire block (from the opening comment at line 358 through the end of the `for part in remaining_parts` loop) with:

```rust
        // Build value word with boundary-aware tilde splitting across all parts.
        //
        // The segment boundary starts true immediately after `=` (we just
        // consumed it). Whenever a Literal part is scanned,
        // split_tildes_in_literal returns whether the last character was an
        // unquoted `:`, which we propagate as the incoming boundary for the
        // next part.
        //
        // A non-Literal part (Parameter, CommandSub, quoted content, Tilde,
        // EscapedLiteral) resets the boundary to false: such parts cannot
        // expose an unquoted trailing `:` to the next segment, and
        // EscapedLiteral specifically carries an explicit "this character
        // was escaped" signal from the lexer — tilde-prefix recognition must
        // not fire immediately after it.
        let mut value_parts = Vec::new();
        let mut at_boundary = true;
        if !after_eq.is_empty() {
            let (parts, ends_colon) = split_tildes_in_literal(after_eq, at_boundary);
            value_parts.extend(parts);
            at_boundary = ends_colon;
        }
        for part in remaining_parts {
            match part {
                WordPart::Literal(s) => {
                    let (parts, ends_colon) = split_tildes_in_literal(s, at_boundary);
                    value_parts.extend(parts);
                    at_boundary = ends_colon;
                }
                other => {
                    // Parameter, CommandSub, SingleQuoted, DoubleQuoted,
                    // DollarSingleQuoted, ArithSub, Tilde, and EscapedLiteral
                    // all hit this arm: emit as-is and close the boundary.
                    value_parts.push(other.clone());
                    at_boundary = false;
                }
            }
        }
```

### Step 3.2: Verify prev_was_literal is fully removed

- [ ] Run:
```bash
grep -n 'prev_was_literal' src/
```
Expected: no matches.

### Step 3.3: Add a new unit test for EscapedLiteral in assignment values

Locate the `assignment_rhs_*` test group in `src/parser/mod.rs` (around lines 1550–1700 after sub-project 3's additions).

- [ ] Add this test after `assignment_rhs_backslash_tilde_after_colon_stays_literal`:

```rust
    #[test]
    fn assignment_rhs_param_then_escaped_tilde_stays_literal() {
        // `x=$var:\~/bin` — the `\~` escape after the `:` prevents tilde
        // expansion at the colon boundary. The lexer emits
        // [Literal("x="), Parameter(var), Literal(":"), EscapedLiteral("~"), Literal("/bin")]
        // (or similar). The walker treats EscapedLiteral as a non-Literal
        // segment-boundary closer, so the following Literal does not re-open
        // tilde recognition.
        let (_, parts) = parse_first_assignment("x=$var:\\~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }
```

### Step 3.4: Run tests

- [ ] Run:
```bash
cargo test --lib 2>&1 | tail -5
./e2e/run_tests.sh --filter=tilde 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected:
- Lib: `test result: ok. 630 passed` (629 after Task 2 + 1 new).
- Tilde E2E: 18/18 pass.
- Full E2E: 368 / 367 pass + 1 XFail / 0 Failed.

### Step 3.5: Commit

```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
refactor(parser): remove prev_was_literal heuristic

With the Task-2 lexer change, adjacent WordPart::Literal entries no
longer arise from line-continuation (transparent) or from `\<char>`
escapes (now EscapedLiteral). The prev_was_literal flag — introduced
in sub-project 3 as a proxy for "was this Literal preceded by an
escape" — is therefore obsolete.

Replaces the heuristic with the precise check: the catch-all `other`
arm of the walker match naturally handles EscapedLiteral with the
same boundary-closing semantics it applies to Parameter/CommandSub/
quoted content. Adds a new assignment_rhs_param_then_escaped_tilde_
stays_literal test that pins the `x=$var:\~/bin` case using the new
metadata.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 (Commit ④): Executor routing + builtin cleanup + E2E + TODO

**Files:**
- Modify: `src/parser/mod.rs` (make `try_parse_assignment` callable without `&self`)
- Modify: `src/exec/simple.rs` (route export/readonly args)
- Modify: `src/builtin/special.rs` (remove `expand_tilde_in_assignment_value` call from export/readonly)
- Modify: `src/expand/mod.rs` (delete `expand_tilde_in_assignment_value` function + its tests)
- Create: 3 E2E test files
- Modify: `TODO.md` (remove 3 entries)

### Step 4.1: Make `try_parse_assignment` callable as a free-standing associated function

Currently `try_parse_assignment` is `pub fn try_parse_assignment(&self, word: &Word) -> Option<Assignment>` at `src/parser/mod.rs:323`. The body uses `self` only for — nothing, actually; the method does not reference `self`. Convert to an associated function so it can be called outside a Parser instance.

- [ ] Change the signature from:
```rust
    pub fn try_parse_assignment(&self, word: &Word) -> Option<Assignment> {
```
to:
```rust
    pub fn try_parse_assignment(word: &Word) -> Option<Assignment> {
```

- [ ] Update existing callers inside `src/parser/mod.rs` (one call site: `parse_simple_command` around line 291 invokes `self.try_parse_assignment(&word)`). Change to `Parser::try_parse_assignment(&word)` or (if inside `impl Parser`) `Self::try_parse_assignment(&word)`.

- [ ] Run:
```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -5
```
Expected: builds cleanly, 630 tests still pass.

### Step 4.2: Add the `exec_assignment_builtin_args` helper

In `src/exec/simple.rs`, before the `impl Executor { ... }` block (or as a free function inside the module), add:

```rust
/// For export/readonly, re-process each Word argument by trying to parse
/// it as an Assignment first. Words that successfully parse as `NAME=value`
/// get their value Word expanded in a tilde-aware way (EscapedLiteral
/// segments bypass tilde recognition in split_tildes_in_literal), avoiding
/// the lossy string-based tilde expansion the builtin used to perform.
///
/// Returns a Vec of "NAME=value" or "NAME" strings suitable for the
/// existing builtin_export / builtin_readonly signatures.
fn expand_assignment_builtin_args(
    env: &mut crate::env::ShellEnv,
    words: &[crate::parser::ast::Word],
) -> crate::error::Result<Vec<String>> {
    use crate::parser::Parser;
    use crate::parser::ast::Assignment;

    let mut out = Vec::with_capacity(words.len());
    for word in words {
        match Parser::try_parse_assignment(word) {
            Some(Assignment { name, value: Some(value_word) }) => {
                let value = crate::expand::expand_word_to_string(env, &value_word)?;
                out.push(format!("{}={}", name, value));
            }
            Some(Assignment { name, value: None }) => {
                out.push(format!("{}=", name));
            }
            None => {
                // Not an assignment (e.g. `export NAME` bare form or `export -p`).
                // Fall back to normal word expansion + first field.
                let expanded = crate::expand::expand_words(env, std::slice::from_ref(word))?;
                out.extend(expanded);
            }
        }
    }
    Ok(out)
}
```

Check the exact import path for `Assignment` and `expand_word_to_string` by searching:
- `grep -rn 'pub fn expand_word_to_string' src/`
- `grep -n 'pub struct Assignment' src/parser/ast.rs`

Adjust imports in the helper as needed.

### Step 4.3: Route export/readonly in `exec_simple_command`

In `src/exec/simple.rs` around line 236, the current call:
```rust
                let status = exec_special_builtin(&command_name, &args, self);
```

- [ ] Replace with a conditional that uses the helper for export/readonly:

```rust
                let status = if command_name == "export" || command_name == "readonly" {
                    // Re-expand args from the original Words via try_parse_assignment
                    // so EscapedLiteral / Tilde metadata is respected. cmd.words[0]
                    // is the command name; args start at [1..].
                    let original_args = &cmd.words[1..];
                    match expand_assignment_builtin_args(&mut self.env, original_args) {
                        Ok(reparsed_args) => exec_special_builtin(&command_name, &reparsed_args, self),
                        Err(e) => {
                            self.env.exec.last_exit_status = 1;
                            redirect_state.restore();
                            return Err(e);
                        }
                    }
                } else {
                    exec_special_builtin(&command_name, &args, self)
                };
```

### Step 4.4: Remove the string-based tilde expansion from builtins

In `src/builtin/special.rs`:

- [ ] At line 104 (builtin_export), change:
```rust
            let value = expand_tilde_in_assignment_value(home.as_deref(), raw_value);
            if let Err(e) = env.vars.set(name, &value) {
```
to:
```rust
            // Value already has tilde expansion applied at the executor level
            // via exec_assignment_builtin_args. No further expansion needed.
            if let Err(e) = env.vars.set(name, raw_value) {
```

- [ ] Same change at line 153 (builtin_readonly):
```rust
            let value = expand_tilde_in_assignment_value(home.as_deref(), raw_value);
            if let Err(e) = env.vars.set(name, &value) {
```
→
```rust
            if let Err(e) = env.vars.set(name, raw_value) {
```

- [ ] Remove the `let home = env.vars.get("HOME").map(|s| s.to_string());` lines at lines 98 and 147 — `home` is no longer used.

- [ ] Remove the import `use crate::expand::expand_tilde_in_assignment_value;` at the top of `src/builtin/special.rs` (around line 9).

### Step 4.5: Delete `expand_tilde_in_assignment_value`

In `src/expand/mod.rs`:

- [ ] Delete the function at lines 643–656:

```rust
pub(crate) fn expand_tilde_in_assignment_value(home_dir: Option<&str>, value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, seg) in value.split(':').enumerate() {
        if i > 0 {
            out.push(':');
        }
        if seg.starts_with('~') {
            out.push_str(&expand_tilde_prefix(home_dir, seg));
        } else {
            out.push_str(seg);
        }
    }
    out
}
```

- [ ] Search for any tests of this function (e.g., in a `#[cfg(test)] mod tests` block in `src/expand/mod.rs`):
```bash
grep -n 'expand_tilde_in_assignment_value' src/expand/mod.rs
```
Delete any matching test functions.

- [ ] Verify no remaining references:
```bash
grep -rn 'expand_tilde_in_assignment_value' src/
```
Expected: no matches.

### Step 4.6: Compile and run unit tests

- [ ] Run:
```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -5
```
Expected: clean build; all unit tests still pass. Count depends on whether `expand_tilde_in_assignment_value` had tests deleted — expect within a few of 630.

### Step 4.7: Create E2E test `tilde_mixed_line_continuation_expands.sh`

- [ ] Create `/Users/kazukiyamamoto/Projects/rust/kish/e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_line_continuation_expands.sh` with:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands when ':' and '~' are separated by line-continuation (POSIX §2.2.1 removes \<newline> before tokenization)
# EXPECT_OUTPUT: foo:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=foo:\
~/bin
echo "$x"
```

Chmod 644:
```bash
chmod 644 e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_line_continuation_expands.sh
```

### Step 4.8: Create E2E test `tilde_export_escape_preserved.sh`

- [ ] Create `/Users/kazukiyamamoto/Projects/rust/kish/e2e/posix_spec/2_06_01_tilde_expansion/tilde_export_escape_preserved.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: export NAME=\~/val preserves the backslash-escaped tilde as literal
# EXPECT_OUTPUT: ~/val
# EXPECT_EXIT: 0
HOME=/home/x
export NAME=\~/val
echo "$NAME"
```

Chmod 644.

### Step 4.9: Create E2E test `tilde_readonly_escape_preserved.sh`

- [ ] Create `/Users/kazukiyamamoto/Projects/rust/kish/e2e/posix_spec/2_06_01_tilde_expansion/tilde_readonly_escape_preserved.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: readonly NAME=\~/val preserves the backslash-escaped tilde as literal
# EXPECT_OUTPUT: ~/val
# EXPECT_EXIT: 0
HOME=/home/x
readonly NAME=\~/val
echo "$NAME"
```

Chmod 644.

### Step 4.10: Run the new E2E tests

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=tilde_mixed_line_continuation 2>&1 | tail -2
./e2e/run_tests.sh --filter=tilde_export_escape 2>&1 | tail -2
./e2e/run_tests.sh --filter=tilde_readonly_escape 2>&1 | tail -2
```
Expected: each 1/1 pass.

If a test fails:
- For `tilde_mixed_line_continuation_expands.sh`: the output should be `foo:/home/x/bin`. If it's `foo:~/bin`, the line-continuation is still causing tilde suppression — re-check Task 2 lexer change.
- For the export/readonly tests: if output is `/home/x/val` instead of `~/val`, the escape routing in Step 4.3 didn't take effect — re-check Task 4.1/4.2/4.3.

### Step 4.11: Full regression

- [ ] Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -5
cargo test --lib 2>&1 | tail -5
```
Expected:
- E2E: `Total: 371 Passed: 370 Failed: 0 XFail: 1` (368 baseline + 3 new). XFail from sub-project 2 persists.
- Lib: clean, count within ~630.

### Step 4.12: Update TODO.md

Open `/Users/kazukiyamamoto/Projects/rust/kish/TODO.md`. Under `## Future: POSIX Conformance Gaps (Chapter 2)`, currently there are (in this order):

```
- [ ] §2.6.1 Tilde escape info lost at export/readonly — `export NAME=\~/val` wrongly expands because word expansion drops the backslash before `expand_tilde_in_assignment_value` sees the argument; would require preserving escape metadata through word expansion or routing export/readonly args through the parser's assignment path
- [ ] §2.6.1 Line-continuation tilde after unquoted `:` — `x=foo:\<newline>~/bin` does not expand the tilde because the `\<newline>` line-continuation causes the lexer to split into adjacent `WordPart::Literal` entries, which the parser's `prev_was_literal` heuristic (introduced in sub-project 3) then suppresses. Pre-existing (pre-sub-project 3 also produced the same wrong output, via a different code path). Pinned by `assignment_rhs_line_continuation_tilde_known_regression` in `src/parser/mod.rs`. Sub-project 4's escape-metadata work should replace `prev_was_literal` with a precise escape check and fix this case.
- [ ] §2.11 ignored-on-entry signal inheritance — no in-harness test yet (nested `sh -c` escapes yosh); revisit after a yosh-aware subshell helper lands
- [ ] §2.10.2 Rule 5 — yosh accepts reserved words as `for` NAME (`e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh` XFAIL). POSIX requires NAME to be a valid name, not a reserved word.
- [ ] Sub-project 4 must REMOVE `prev_was_literal` (not leave as fallback) — when escape metadata lands on `WordPart`, the `prev_was_literal` heuristic in `try_parse_assignment` should be deleted in the same commit, replaced by a precise escape check. Leaving it as a "belt and suspenders" fallback would let a future lexer refactor (stops producing adjacent Literals for escapes) silently turn `x=\~/bin` into an expand case with no test coverage catching it (`src/parser/mod.rs`).
```

- [ ] Delete:
  - The `§2.6.1 Tilde escape info lost at export/readonly` line
  - The `§2.6.1 Line-continuation tilde after unquoted` line
  - The `Sub-project 4 must REMOVE prev_was_literal` line

- [ ] Leave intact:
  - `§2.11 ignored-on-entry signal inheritance`
  - `§2.10.2 Rule 5`

### Step 4.13: Final verification

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=2_06_01_tilde_expansion 2>&1 | tail -2
cargo test --lib 2>&1 | tail -5
grep -rn 'prev_was_literal\|expand_tilde_in_assignment_value' src/
git status
git log --oneline 803130e..HEAD
```
Expected:
- Tilde E2E: `Total: 21  Passed: 21  Failed: 0` (18 existing + 3 new).
- Lib: clean pass.
- `grep`: no matches (both symbols gone).
- `git status`: working tree clean after Step 4.14.
- `git log`: 4 commits on top of the plan SHA `803130e`.

### Step 4.14: Commit

```bash
git add src/parser/mod.rs src/exec/simple.rs src/builtin/special.rs src/expand/mod.rs \
        e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_line_continuation_expands.sh \
        e2e/posix_spec/2_06_01_tilde_expansion/tilde_export_escape_preserved.sh \
        e2e/posix_spec/2_06_01_tilde_expansion/tilde_readonly_escape_preserved.sh \
        TODO.md
git commit -m "$(cat <<'EOF'
feat(exec): route export/readonly through assignment parser; retire string-based tilde helper

Two closely linked changes close the remaining §2.6.1 Chapter 2 gaps:

1. exec_simple_command now detects export/readonly as the command name
   and re-processes each Word argument via Parser::try_parse_assignment,
   expanding the resulting value Word through the normal tilde-aware
   path (which respects EscapedLiteral suppression and multi-WordPart
   boundaries). This fixes `export NAME=\~/val` — the backslash now
   reaches the expansion as an EscapedLiteral so the tilde stays
   literal.

2. expand_tilde_in_assignment_value is deleted from src/expand/mod.rs
   and its two call sites in src/builtin/special.rs are removed. The
   builtins now trust that the value they receive was already tilde-
   expanded at the executor layer. This both removes technical debt
   and avoids the original escape-stripping bug.

Also closes `x=foo:\<newline>~/bin` via the Task-2 lexer change and
the Task-3 prev_was_literal removal — adds the E2E pin
`tilde_mixed_line_continuation_expands.sh` alongside the two export/
readonly E2E pins.

Removes three TODO.md entries (§2.6.1 escape-at-export, §2.6.1 line-
continuation, sub-project-4-prerequisite prev_was_literal removal).

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Success Criteria (restated from spec)

- `export NAME=\~/val; echo "$NAME"` → `~/val`
- `x=foo:\<newline>~/bin; echo "$x"` → `foo:/home/x/bin`
- `grep prev_was_literal src/` → no matches
- `grep expand_tilde_in_assignment_value src/` → no matches
- All 18 existing `2_06_01_tilde_expansion/*.sh` pass
- 3 new E2E pass
- Full E2E `Total: 371 Passed: 370 Failed: 0 XFail: 1`
- `cargo test --lib` no failures
- TODO.md: 3 sub-project-4 entries removed; §2.11 and §2.10.2 Rule 5 remain
- 4 commits on top of `803130e`

## Notes for the executor

- **Non-exhaustive match errors are expected in Task 1 Step 1.6.** If the build fails with a compile error pointing to a match statement you didn't modify, add an `EscapedLiteral(_) => { /* same as Literal */ }` arm there. The spec lists the 3–4 expected sites; others are acceptable to add.
- **`peek_next_byte` may require a helper** in Task 2 Step 2.3. Inspect the lexer's scanner layer first; it probably already exposes a peek method under a slightly different name.
- **Line-continuation inside double quotes stays as an empty `Literal`** in Task 2 (Step 2.4) — the outer filter still runs at `src/lexer/word.rs:128–132`. Only unquoted line-continuation is made fully transparent.
- **Sub-project 3's pinning test must flip at Task 2 (not Task 3).** The lexer change at Task 2 is what makes the line-continuation tilde expand; the test must be updated at the same commit. Task 3 is solely about `prev_was_literal` removal and the new EscapedLiteral pin.
- **Task 4's executor change is surgical** — a single conditional before the `exec_special_builtin` call. Do not refactor `exec_simple_command` structure beyond adding the conditional and the `expand_assignment_builtin_args` helper.
- **Do NOT modify E2E `tilde_rhs_export.sh` / `tilde_rhs_readonly.sh`** — those existing tests cover the happy path (`export PATH=~/bin`) and must still pass through the new executor routing.
