# Parser Module Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `src/parser/mod.rs` (2054 lines, 96 tests) into six focused files (`mod.rs`, `simple.rs`, `compound.rs`, `function.rs`, `redirect.rs`, `word.rs`) without changing any function body, signature, or visibility.

**Architecture:** Mechanical move only. The `Parser` struct stays in `mod.rs`; each new submodule contributes additional `impl Parser { ... }` blocks. Free functions (`is_valid_name`, `split_tildes_in_literal`, `is_name_safe`) move to `word.rs` with their existing `pub(crate)` / private visibility preserved. Tests are co-located with the production code they exercise via `#[cfg(test)] mod tests` in each submodule. After each split step, `cargo build && cargo test --lib parser::` must equal the Step 0 baseline; the step is then committed.

**Tech Stack:** Rust 2024 edition, `cargo`, the existing `crate::error`, `crate::lexer`, and `super::ast` types.

**Spec:** `docs/superpowers/specs/2026-05-04-parser-mod-split-design.md`

---

## File Structure

After this plan completes, `src/parser/` will contain:

| File          | Responsibility                                                                                                  |
|---------------|-----------------------------------------------------------------------------------------------------------------|
| `ast.rs`      | Unchanged (AST type definitions).                                                                               |
| `mod.rs`      | `Parser` struct, constructors (`new`, `new_with_aliases`, `new_with_aliases_at_line`), token utilities (`advance`, `eat`, `expect_reserved`, `skip_newlines`, `current_token`, `current_span`, `consumed_bytes`, `is_at_end`, `is_reserved`), driver methods (`parse_program`, `parse_complete_command`, `parse_separator_op`, `parse_and_or`, `parse_pipeline`, `parse_command`), predicate methods used by the driver (`is_complete_command_end`, `is_compound_command_start`), and 10 driver-level tests including the shared `parse` test helper. |
| `simple.rs`   | `parse_simple_command`, `try_parse_assignment`, plus 16 simple/assignment/RHS-tilde/simple-line tests.          |
| `compound.rs` | All `parse_*_clause` (`if`, `for`, `while`, `until`, `case`), `parse_brace_group`, `parse_subshell`, `parse_compound_list`, `parse_do_group`, `parse_compound_command`, plus 39 compound/empty-body/line-number/reserved-word tests. |
| `function.rs` | `try_parse_function_def` plus 2 function tests.                                                                 |
| `redirect.rs` | `try_parse_redirect`, `parse_redirect_list`, `extract_heredoc_delimiter`, `fill_heredoc_bodies`, plus 13 redirect/heredoc tests. |
| `word.rs`     | `expect_word`, free functions `is_valid_name`, `split_tildes_in_literal`, `is_name_safe`, plus 16 tilde-split tests. |

---

## Task 0: Capture Baseline

**Files:**
- No file changes.
- Outputs: `/tmp/parser-baseline.txt`, `/tmp/parser-grep.txt`, `/tmp/parser-bench-before.txt` (optional).

- [ ] **Step 1: Capture parser test count baseline**

```bash
cargo test --lib parser:: 2>&1 | tee /tmp/parser-baseline.txt
```

Expected: a line like `test result: ok. 96 passed; 0 failed; 0 ignored; 0 measured; ...`. Record the `96 passed` figure — every later task must reproduce it.

- [ ] **Step 2: Snapshot visibility / cfg-test markers in `parser/mod.rs`**

```bash
grep -nE '#\[cfg\(test\)\]|pub\(super\)|pub\(crate\)|#\[allow\(' \
    src/parser/mod.rs | tee /tmp/parser-grep.txt
```

Expected baseline content (verify these match):
- Line 74: `#[allow(dead_code)]` on `current_token`
- Line 1017: `pub(crate) fn split_tildes_in_literal`
- Line 1069: `mod tests {` with `#[cfg(test)]` directive (look at line 1068)

- [ ] **Step 3 (optional): Capture parser bench baseline**

```bash
cargo bench --bench parser_bench 2>&1 | tee /tmp/parser-bench-before.txt
```

Expected: bench run completes; record the times for later comparison. If the bench is too slow or fails to compile in your environment, skip — Task 7 will recheck once the split is done.

- [ ] **Step 4: Record current `parser/mod.rs` line count**

```bash
wc -l src/parser/mod.rs
```

Expected: `2054 src/parser/mod.rs`. Used as the Definition-of-Done starting point.

This task does **not** produce a commit.

---

## Task 1: Split `word.rs`

**Files:**
- Create: `src/parser/word.rs`
- Modify: `src/parser/mod.rs` (delete moved code; add `mod word;` declaration)

**What moves:**
- Production `expect_word` method (lines 970–985)
- Free function `is_valid_name` (lines 987–1015)
- Free function `split_tildes_in_literal` (lines 1017–1067, `pub(crate)` preserved)
- Free function `is_name_safe` (lines 1023 region — listed in spec; verify exact lines via Read)
- Tests `lit` helper and 16 tilde-split tests (lines 1489–1632)

- [ ] **Step 1: Read the production lines being moved**

Use `Read src/parser/mod.rs offset=970 limit=16` (for `expect_word`) and `Read src/parser/mod.rs offset=987 limit=85` (for the three free functions). Confirm the exact content before editing.

- [ ] **Step 2: Read the test lines being moved**

Use `Read src/parser/mod.rs offset=1485 limit=148` (covers `lit` helper through `split_returns_ends_with_colon_flag`). Note: a section comment header may sit at line 1489 (`// ── split_tildes_in_literal ──`); preserve it inside the new tests module.

- [ ] **Step 3: Create `src/parser/word.rs`**

Write `src/parser/word.rs` with the following structure. Replace the `// COPY ... ` markers with the verbatim text from Steps 1–2.

```rust
use crate::error;
use crate::lexer::token::Token;
use super::Parser;
use super::ast::{Word, WordPart};

impl Parser {
    // COPY VERBATIM from mod.rs lines 970–985:
    //   pub fn expect_word(&mut self, context: &str) -> error::Result<Word> { ... }
}

// COPY VERBATIM from mod.rs lines 987–1015:
//   fn is_valid_name(s: &str) -> bool { ... }

// COPY VERBATIM from mod.rs lines 1017–1067:
//   pub(crate) fn split_tildes_in_literal(...) -> (Vec<WordPart>, bool) { ... }
//   fn is_name_safe(ch: char) -> bool { ... }

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast::WordPart;

    // COPY VERBATIM from mod.rs lines 1491–1495:
    //   fn lit(s: &str) -> WordPart { ... }

    // COPY VERBATIM from mod.rs lines 1496–1632 (16 #[test] functions
    // beginning with split_no_tilde_returns_single_literal and ending
    // with split_returns_ends_with_colon_flag).
}
```

Note on `use` statements: the new `word.rs` only needs the imports actually referenced by the moved code. After copying, run Step 6 below; if `cargo build` reports unused imports, remove them.

- [ ] **Step 4: Remove moved code from `src/parser/mod.rs`**

Delete the following ranges from `src/parser/mod.rs`:
- Lines 970–985 (`expect_word`)
- Lines 987–1067 (the three free functions)
- Lines 1489–1632 (the tilde-split test section)

Confirm by re-reading the surrounding context after each Edit; there should be no stray closing brace mismatches.

- [ ] **Step 5: Add `mod word;` to `src/parser/mod.rs`**

Edit the top of `src/parser/mod.rs` so the module declarations now read:

```rust
pub mod ast;
mod word;
```

Place `mod word;` directly after `pub mod ast;` so submodule declarations stay grouped.

- [ ] **Step 6: Build and run parser tests**

```bash
cargo build -p yosh
cargo test --lib parser::
```

Expected: build succeeds; test count equals the Task 0 baseline (96 passed).

If `cargo build` fails with "unresolved import `super::Parser`" inside `word.rs`, double-check the new file's `use` block.

If `cargo test` shows fewer tests, re-Read the new `word.rs::tests` module and confirm all 16 `#[test]` functions are present.

- [ ] **Step 7: Commit**

```bash
git add src/parser/mod.rs src/parser/word.rs
git commit -m "$(cat <<'EOF'
refactor(parser): split word and tilde helpers into src/parser/word.rs

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Split `function.rs`

**Files:**
- Create: `src/parser/function.rs`
- Modify: `src/parser/mod.rs`

**What moves:**
- Production `try_parse_function_def` (lines 767–826 of the **post-Task-1** `mod.rs` — line numbers will have shifted; use Read to find the method by name, not by absolute line)
- Tests `test_function_def` and `test_function_def_with_redirect` (find via grep on `fn test_function_def`)

- [ ] **Step 1: Locate the current line range of `try_parse_function_def`**

```bash
grep -n 'try_parse_function_def\|fn test_function_def' src/parser/mod.rs
```

Expected: one production declaration plus two test definitions. Note their start lines.

- [ ] **Step 2: Read the production method**

Use `Read src/parser/mod.rs offset=<production-start> limit=70` to capture `try_parse_function_def` and confirm its end is `Ok(Some(FunctionDef { ... }))` followed by `}`.

- [ ] **Step 3: Read the two test functions**

Use `Read src/parser/mod.rs offset=<first-test-start> limit=30` to capture `test_function_def` and `test_function_def_with_redirect` together.

- [ ] **Step 4: Create `src/parser/function.rs`**

```rust
use crate::error;
use crate::lexer::token::Token;
use super::Parser;
use super::ast::FunctionDef;

impl Parser {
    // COPY VERBATIM the try_parse_function_def method captured in Step 2.
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast;
    use super::super::tests::parse;

    // COPY VERBATIM test_function_def and test_function_def_with_redirect
    // captured in Step 3.
}
```

If the test bodies use additional types (`AndOrOp`, `SeparatorOp`, `Command`, etc.) beyond `ast::*`, add them to the `use super::super::ast::{...}` line. Verify by reading the test bodies.

- [ ] **Step 5: Remove moved code from `src/parser/mod.rs`**

Delete the production method range and the two test function ranges captured in Steps 2 and 3.

- [ ] **Step 6: Add `mod function;` to `src/parser/mod.rs`**

Update the module declarations:

```rust
pub mod ast;
mod word;
mod function;
```

- [ ] **Step 7: Build and run parser tests**

```bash
cargo build -p yosh
cargo test --lib parser::
```

Expected: 96 passed (same as baseline).

If a test fails with "cannot find function `parse` in this scope", the test in `function.rs::tests` is calling the shared `parse` helper from `mod.rs::tests`. Confirm `use super::super::tests::parse;` is present in `function.rs::tests`.

- [ ] **Step 8: Commit**

```bash
git add src/parser/mod.rs src/parser/function.rs
git commit -m "$(cat <<'EOF'
refactor(parser): split try_parse_function_def into src/parser/function.rs

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Split `redirect.rs`

**Files:**
- Create: `src/parser/redirect.rs`
- Modify: `src/parser/mod.rs`

**What moves:**
- Production `try_parse_redirect`, `parse_redirect_list`, `extract_heredoc_delimiter`, `fill_heredoc_bodies` (find via grep)
- Tests: 9 redirect tests (`test_output_redirect` through `test_multiple_redirects`) and 4 heredoc tests (`test_heredoc_body`, `test_heredoc_strip_tabs`, `test_heredoc_quoted_delimiter`, `test_heredoc_with_command_after`)

- [ ] **Step 1: Locate the production methods**

```bash
grep -n 'fn try_parse_redirect\|fn parse_redirect_list\|fn extract_heredoc_delimiter\|fn fill_heredoc_bodies' \
    src/parser/mod.rs
```

Note start line of each.

- [ ] **Step 2: Locate the test functions**

```bash
grep -n 'fn test_output_redirect\|fn test_input_redirect\|fn test_append_redirect\|fn test_fd_redirect\|fn test_dup_output\|fn test_heredoc_redirect\|fn test_clobber_redirect\|fn test_read_write_redirect\|fn test_multiple_redirects\|fn test_heredoc_body\|fn test_heredoc_strip_tabs\|fn test_heredoc_quoted_delimiter\|fn test_heredoc_with_command_after' \
    src/parser/mod.rs
```

Expected: 13 hits.

- [ ] **Step 3: Read the production block**

Use `Read` to capture each method individually (the four methods are contiguous in the original file but post-Task-2 line numbers may shift). Confirm completeness by checking each method's closing `}` is followed by either the next method or a free function / module boundary.

- [ ] **Step 4: Read the test bodies**

The 9 redirect tests are contiguous; the 4 heredoc tests are a separate contiguous block. Read each block separately.

- [ ] **Step 5: Create `src/parser/redirect.rs`**

```rust
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::{Span, Token};
use super::Parser;
use super::ast::{HereDoc, Redirect, RedirectKind, Word};

impl Parser {
    // COPY VERBATIM try_parse_redirect, parse_redirect_list,
    // extract_heredoc_delimiter, fill_heredoc_bodies (in that order).
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast::{self, RedirectKind, WordPart};
    use super::super::tests::parse;

    // COPY VERBATIM the 9 redirect tests, then the 4 heredoc tests.
}
```

After copying, prune any unused imports flagged by `cargo build`.

- [ ] **Step 6: Remove moved code from `src/parser/mod.rs`**

Delete the four production methods and the two test blocks. Order of deletion within `mod.rs` does not matter as long as each Edit's `old_string` is unique; the easiest sequence is largest-block first.

- [ ] **Step 7: Add `mod redirect;` to `src/parser/mod.rs`**

```rust
pub mod ast;
mod word;
mod function;
mod redirect;
```

- [ ] **Step 8: Build and run parser tests**

```bash
cargo build -p yosh
cargo test --lib parser::
```

Expected: 96 passed.

- [ ] **Step 9: Commit**

```bash
git add src/parser/mod.rs src/parser/redirect.rs
git commit -m "$(cat <<'EOF'
refactor(parser): split redirect and heredoc parsing into src/parser/redirect.rs

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Split `simple.rs`

**Files:**
- Create: `src/parser/simple.rs`
- Modify: `src/parser/mod.rs`

**What moves:**
- Production `parse_simple_command`, `try_parse_assignment`
- Tests: `test_simple_command`, `test_assignment_only`, `test_assignment_with_command`, `test_assignment_empty_value`, the 10 `assignment_rhs_*` tests, plus `parse_simple_command_captures_line` and `parse_simple_command_on_third_line`. Also the helpers `parse_first_simple` and `parse_first_assignment`.

**Important:** `try_parse_assignment` calls `split_tildes_in_literal`, which now lives in `word.rs`. Add `use super::word::split_tildes_in_literal;` to the new file.

- [ ] **Step 1: Locate the production methods**

```bash
grep -n 'fn parse_simple_command\|fn try_parse_assignment' src/parser/mod.rs
```

- [ ] **Step 2: Locate the test functions and helpers**

```bash
grep -n 'fn parse_first_simple\|fn parse_first_assignment\|fn test_simple_command\|fn test_assignment_only\|fn test_assignment_with_command\|fn test_assignment_empty_value\|fn assignment_rhs_\|fn parse_simple_command_captures_line\|fn parse_simple_command_on_third_line' \
    src/parser/mod.rs
```

Expected: 1 + 1 + 4 + 10 + 2 = 18 hits, plus 2 helper hits = 20 lines.

- [ ] **Step 3: Read each block**

Capture: production methods (2 contiguous), `parse_first_simple` helper (10 lines), the 4 simple/assignment tests (contiguous near the top of `mod tests`), the 10 `assignment_rhs_*` tests (contiguous), `parse_first_assignment` helper, and the 2 simple-command line-number tests.

- [ ] **Step 4: Create `src/parser/simple.rs`**

```rust
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::{Span, Token};
use super::Parser;
use super::ast::{Assignment, SimpleCommand, Word, WordPart};
use super::word::split_tildes_in_literal;

impl Parser {
    // COPY VERBATIM parse_simple_command, then try_parse_assignment.
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast::{self, AndOrOp, Command, WordPart};
    use super::super::tests::parse;

    // COPY VERBATIM parse_first_simple helper.

    // COPY VERBATIM test_simple_command, test_assignment_only,
    // test_assignment_with_command, test_assignment_empty_value.

    // COPY VERBATIM the 10 assignment_rhs_* tests.

    // COPY VERBATIM parse_first_assignment helper.

    // COPY VERBATIM parse_simple_command_captures_line and
    // parse_simple_command_on_third_line.
}
```

The `assignment_rhs_*` tests use `parse_first_assignment` extensively, so make sure that helper sits **above** them in the file.

- [ ] **Step 5: Remove moved code from `src/parser/mod.rs`**

Delete the two production methods and all moved test/helper blocks.

- [ ] **Step 6: Add `mod simple;` to `src/parser/mod.rs`**

```rust
pub mod ast;
mod word;
mod function;
mod redirect;
mod simple;
```

- [ ] **Step 7: Build and run parser tests**

```bash
cargo build -p yosh
cargo test --lib parser::
```

Expected: 96 passed.

If `cargo build` reports `error[E0425]: cannot find function 'split_tildes_in_literal'` inside `simple.rs`, confirm `use super::word::split_tildes_in_literal;` is present.

- [ ] **Step 8: Commit**

```bash
git add src/parser/mod.rs src/parser/simple.rs
git commit -m "$(cat <<'EOF'
refactor(parser): split simple-command parsing into src/parser/simple.rs

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Split `compound.rs`

**Files:**
- Create: `src/parser/compound.rs`
- Modify: `src/parser/mod.rs`

**What moves:**
- Production: `parse_compound_command`, `parse_compound_list`, `parse_if_clause`, `parse_for_clause`, `parse_do_group`, `parse_while_clause`, `parse_until_clause`, `parse_case_clause`, `parse_brace_group`, `parse_subshell` (10 methods)
- Test helpers: `parse_first_compound`, `parse_err`, `parse_ok`, `first_compound_cmd`
- Tests (39 total):
  - 14 compound tests: `test_if_then_fi`, `test_if_else`, `test_if_elif`, `test_for_loop_with_words`, `test_for_loop_without_in`, `test_for_loop_with_do_on_newline`, `test_while_loop`, `test_until_loop`, `test_case_basic`, `test_case_fallthrough`, `test_case_multiple_patterns`, `test_case_empty`, `test_brace_group`, `test_subshell`
  - 15 empty-body / non-empty / case-empty / comment-only tests
  - 6 compound line-number tests (compound contexts only — the simple-command line-number tests went to `simple.rs`)
  - 4 `parse_for_*` reserved-word/valid-name/time-word tests

- [ ] **Step 1: Locate the production methods**

```bash
grep -n 'fn parse_compound_command\|fn parse_compound_list\|fn parse_if_clause\|fn parse_for_clause\|fn parse_do_group\|fn parse_while_clause\|fn parse_until_clause\|fn parse_case_clause\|fn parse_brace_group\|fn parse_subshell' \
    src/parser/mod.rs
```

Expected: 10 hits.

- [ ] **Step 2: Locate the test helpers**

```bash
grep -n 'fn parse_first_compound\|fn parse_err\|fn parse_ok\|fn first_compound_cmd' \
    src/parser/mod.rs
```

Expected: 4 hits.

- [ ] **Step 3: Locate the 39 test functions**

```bash
grep -n 'fn test_if_\|fn test_for_\|fn test_while_loop\|fn test_until_loop\|fn test_case_\|fn test_brace_group\|fn test_subshell\|fn empty_\|fn nonempty_\|fn case_empty_\|fn comment_only_\|fn parse_compound_if_\|fn parse_brace_group_\|fn parse_subshell_\|fn parse_while_captures_line\|fn parse_nested_if_then_\|fn parse_for_reserved_word_\|fn parse_for_valid_name_\|fn parse_for_time_word_' \
    src/parser/mod.rs
```

Expected: 39 hits (14 + 15 + 6 + 4).

- [ ] **Step 4: Read the production block**

The 10 methods are contiguous (the original layout has compound methods between `is_compound_command_start` and `try_parse_function_def`). Read in chunks of ~150 lines.

- [ ] **Step 5: Read the test helpers**

Read each helper (`parse_first_compound` ~10 lines, `parse_err` and `parse_ok` ~5 lines each, `first_compound_cmd` ~26 lines).

- [ ] **Step 6: Read the 39 test functions**

The compound tests cluster near `test_if_then_fi`; the empty-body cluster sits later; line-number compound tests follow; reserved-word `for` tests are at the very end of the file. Read each cluster.

- [ ] **Step 7: Create `src/parser/compound.rs`**

```rust
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::{Span, Token};
use super::Parser;
use super::ast::{
    CaseItem, CaseTerminator, CompleteCommand, CompoundCommand, CompoundCommandKind, Word,
};

impl Parser {
    // COPY VERBATIM the 10 compound methods in this order:
    //   parse_compound_command
    //   parse_compound_list
    //   parse_if_clause
    //   parse_for_clause
    //   parse_do_group
    //   parse_while_clause
    //   parse_until_clause
    //   parse_case_clause
    //   parse_brace_group
    //   parse_subshell
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ast::{self, CompoundCommandKind, Command};
    use super::super::tests::parse;
    use crate::error::ShellError;

    // COPY VERBATIM parse_first_compound helper.
    // COPY VERBATIM parse_err and parse_ok helpers.
    // COPY VERBATIM first_compound_cmd helper.

    // COPY VERBATIM the 14 compound tests.
    // COPY VERBATIM the 15 empty-body / nonempty / case-empty / comment-only tests.
    // COPY VERBATIM the 6 compound line-number tests:
    //   parse_compound_if_captures_line
    //   parse_compound_if_on_second_line
    //   parse_brace_group_captures_line
    //   parse_subshell_captures_line
    //   parse_while_captures_line
    //   parse_nested_if_then_captures_body_line
    // COPY VERBATIM the 4 parse_for_* reserved-word / valid-name / time-word tests.
}
```

If unused imports are reported, prune them per `cargo build` output.

- [ ] **Step 8: Remove moved code from `src/parser/mod.rs`**

Delete the 10 production methods, 4 test helpers, and 39 test functions. Note: the order of Edit calls doesn't matter, but capturing whole contiguous blocks per Edit reduces the number of operations needed.

- [ ] **Step 9: Add `mod compound;` to `src/parser/mod.rs`**

```rust
pub mod ast;
mod word;
mod function;
mod redirect;
mod simple;
mod compound;
```

- [ ] **Step 10: Build and run parser tests**

```bash
cargo build -p yosh
cargo test --lib parser::
```

Expected: 96 passed.

- [ ] **Step 11: Commit**

```bash
git add src/parser/mod.rs src/parser/compound.rs
git commit -m "$(cat <<'EOF'
refactor(parser): split compound-command parsing into src/parser/compound.rs

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Settle Shared Test Helpers

By this point `mod.rs` should contain only the `Parser` struct, constructors, token utilities, the driver methods (`parse_program`, `parse_complete_command`, `parse_separator_op`, `parse_and_or`, `parse_pipeline`, `parse_command`, `is_complete_command_end`, `is_compound_command_start`), free helpers if any remain, and the driver-level `#[cfg(test)] mod tests` (10 tests + the `parse` helper).

The submodule test files reference `super::super::tests::parse;` — verify each one resolves correctly.

**Files:**
- Modify: `src/parser/mod.rs` (only if `parse` helper needs visibility adjustment)
- Possibly modify each submodule's `mod tests` use clause

- [ ] **Step 1: Confirm the `parse` helper is the only shared test helper**

```bash
grep -n 'fn parse(' src/parser/mod.rs
grep -rn 'super::super::tests::' src/parser/
```

Expected:
- `mod.rs` has exactly one `fn parse(` inside `mod tests`.
- Submodules have `use super::super::tests::parse;` (or `use super::super::tests::*;`) wherever they call it.

- [ ] **Step 2: Confirm helper visibility**

The `parse` helper is `fn parse(...)` (no `pub`). Submodule tests reach it through `super::super::tests::parse`, which works because `mod tests` is a sibling module. If `cargo build` reports a privacy error, change the helper to `pub(super) fn parse(...)`.

- [ ] **Step 3: Decide whether to lift to `test_helpers.rs`**

Per spec §3.3 / §5.1, lift only if 3 or more shared helpers exist. After Tasks 1–5 the only cross-submodule helper should be `parse`. If true, **skip the lift** — keep `parse` in `mod.rs::tests`. If you find 3 or more shared helpers, follow the alternative path below; otherwise proceed to Step 4.

Alternative (only if ≥ 3 shared helpers):

1. Create `src/parser/test_helpers.rs` with `#[cfg(test)] pub(crate) mod test_helpers;` content.
2. Add `#[cfg(test)] mod test_helpers;` to `src/parser/mod.rs`.
3. Move the shared helpers from `mod.rs::tests` into `test_helpers.rs` (changing `fn` → `pub(crate) fn`).
4. Update each submodule's `use` clause to `use super::test_helpers::*;`.

- [ ] **Step 4: Build and run all parser tests**

```bash
cargo build -p yosh
cargo test --lib parser::
```

Expected: 96 passed.

- [ ] **Step 5: Commit only if changes were made**

If Step 2 changed visibility or Step 3 lifted helpers:

```bash
git add src/parser/
git commit -m "$(cat <<'EOF'
refactor(parser): settle shared test-helper visibility after split

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If no changes were needed, this task does **not** produce a commit; advance to Task 7.

---

## Task 7: Final Polish

**Files:**
- Modify: any file under `src/parser/` flagged by `cargo fmt` or `cargo clippy`.

- [ ] **Step 1: Verify final `parser/mod.rs` size**

```bash
wc -l src/parser/mod.rs
```

Expected: ≤ 300 lines (target ~280). If significantly above 300, re-check Task 4 / Task 5 for incomplete deletions.

- [ ] **Step 2: Run `cargo fmt`**

```bash
cargo fmt --all
```

Then check:

```bash
git status
git diff
```

If `src/parser/*.rs` got formatting changes, accept them and continue.

- [ ] **Step 3: Run `cargo clippy --lib`**

```bash
cargo clippy --lib -- -D warnings
```

Expected: no warnings escalated to errors. If clippy fires lints (e.g. `module_inception`, `needless_pub`, `unused_imports`):

- For `unused_imports`: prune the offending `use` line.
- For `module_inception`: not expected (the new files are not named `mod.rs` inside their own folder).
- For other lints: fix in source rather than `#[allow(...)]`.

- [ ] **Step 4: Full build and full test run**

```bash
cargo build
cargo test
```

Expected: build succeeds; total test count unchanged from pre-split; no failures.

- [ ] **Step 5: Run E2E POSIX suite**

```bash
./e2e/run_tests.sh
```

Expected: pass/fail profile identical to pre-split. If a single E2E test starts failing, treat it as a regression — see Rollback below.

- [ ] **Step 6 (optional): Re-run parser bench**

Only if Task 0 Step 3 captured a baseline:

```bash
cargo bench --bench parser_bench 2>&1 | tee /tmp/parser-bench-after.txt
```

Compare against `/tmp/parser-bench-before.txt`. Tolerance: ±5%. A larger regression suggests an inadvertent semantic change — investigate before declaring done.

- [ ] **Step 7: Update `TODO.md` references**

Some TODO.md items reference `src/parser/mod.rs:<line>` for callsites that have moved. Run:

```bash
grep -n 'src/parser/mod\.rs' TODO.md
```

For each hit, update the file path to the new home (e.g. `src/parser/simple.rs` for `try_parse_assignment` references on TODO.md L81). Do not change the surrounding text.

- [ ] **Step 8: Commit polish + TODO updates**

```bash
git add src/parser/ TODO.md
git commit -m "$(cat <<'EOF'
refactor(parser): finalize mod.rs split (fmt, clippy, TODO refs)

Final polish: fmt-clean, clippy-clean, TODO.md references retargeted
to the new submodule paths. Closes parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Rollback Conditions (apply during any task)

Per spec §4.3, immediately `git revert <last-commit>` if any of the following appear:

- `cargo test --lib parser::` count drops below the Task 0 baseline (96).
- `cargo build` fails for a reason other than missing `use` (e.g. trait duplication, orphan-rule violation, recursive module).
- `cargo clippy` produces a new error (warnings excluded) attributable to the split.

After a revert, append a "Lessons" section to the spec describing the failure mode before retrying the task.

---

## Definition of Done

All seven items from spec §4.4 must hold:

1. `src/parser/mod.rs` ≤ 300 lines.
2. `src/parser/{simple,compound,function,redirect,word}.rs` exist and `cargo build` succeeds.
3. `cargo test` total matches the pre-split count.
4. `cargo fmt --check` and `cargo clippy --lib -- -D warnings` pass.
5. `./e2e/run_tests.sh` pass/fail profile unchanged.
6. `git log --oneline` shows 7 ordered commits for Tasks 1–7 (Task 0 produces no commit; Task 6 may produce no commit if no helpers needed lifting).
7. `TODO.md` references to `src/parser/mod.rs` line numbers updated to new paths.
