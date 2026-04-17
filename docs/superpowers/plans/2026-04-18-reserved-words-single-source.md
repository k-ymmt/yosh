# POSIX Reserved Words: Single Source of Truth — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Consolidate the POSIX §2.4 reserved-word list into a single canonical module and rename the misleading `Token::is_reserved_word` method.

**Architecture:** Create a new `src/lexer/reserved.rs` holding the 16-word POSIX list and an `is_posix_reserved_word` predicate. Update `src/builtin/resolve.rs` to consume from this module instead of holding its own copy. Rename `Token::is_reserved_word` → `Token::matches_keyword` to match what it actually does (unquoted-literal-equality check, not list lookup). Parser-layer API names (`expect_reserved`, `is_reserved`) stay unchanged.

**Tech Stack:** Rust 2021, `cargo test`, project-local E2E runner (`./e2e/run_tests.sh`).

**Spec:** `docs/superpowers/specs/2026-04-18-reserved-words-single-source-design.md`

---

## File Structure

| File | Action | Responsibility |
| --- | --- | --- |
| `src/lexer/reserved.rs` | Create | Canonical POSIX §2.4 reserved-word list and `is_posix_reserved_word` predicate; unit tests |
| `src/lexer/mod.rs` | Modify (line 1-5 area) | Register the new `reserved` submodule as `pub` |
| `src/lexer/token.rs` | Modify (lines 41-49, tests 56-74) | Rename `is_reserved_word` → `matches_keyword`; rename tests |
| `src/parser/mod.rs` | Modify (line 73, 103) | Update two call sites to use the new method name |
| `src/builtin/resolve.rs` | Modify (lines 27-35, 49) | Delete local list/fn; import from `crate::lexer::reserved` |
| `TODO.md` | Modify (line 61) | Delete the completed entry |

Each task below produces a self-contained, compilable, test-passing change suitable for a single commit.

---

## Task 1: Create the canonical reserved-words module

**Files:**
- Create: `src/lexer/reserved.rs`
- Modify: `src/lexer/mod.rs:1-5`

- [ ] **Step 1: Create `src/lexer/reserved.rs` with failing tests first**

Write the file with the test module but a stub implementation that returns `false`, so tests fail meaningfully.

```rust
//! POSIX reserved words per IEEE Std 1003.1-2017 §2.4.

pub const RESERVED_WORDS: &[&str] = &[];

pub fn is_posix_reserved_word(_name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_posix_reserved_words_are_recognized() {
        for kw in [
            "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi",
            "for", "if", "in", "then", "until", "while",
        ] {
            assert!(is_posix_reserved_word(kw), "{kw} should be reserved");
        }
    }

    #[test]
    fn non_reserved_words_return_false() {
        for s in ["echo", "foo", "", "IF", "If"] {
            assert!(!is_posix_reserved_word(s), "{s} should not be reserved");
        }
    }

    #[test]
    fn list_length_is_sixteen() {
        assert_eq!(RESERVED_WORDS.len(), 16);
    }
}
```

- [ ] **Step 2: Register the module in `src/lexer/mod.rs`**

Add `pub mod reserved;` after the existing `pub mod token;` line.

Resulting `src/lexer/mod.rs:1-6`:

```rust
pub mod token;
pub mod reserved;
mod alias;
mod heredoc;
mod scanner;
mod word;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib lexer::reserved::tests`

Expected: All three tests FAIL — `all_posix_reserved_words_are_recognized` fails on the first keyword (returns false), `list_length_is_sixteen` fails (`0 != 16`). The `non_reserved_words_return_false` test passes by coincidence (stub returns false for everything), which is fine.

- [ ] **Step 4: Replace the stub with the real implementation**

Edit `src/lexer/reserved.rs` — replace the empty const and stub function:

```rust
pub const RESERVED_WORDS: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi",
    "for", "if", "in", "then", "until", "while",
];

pub fn is_posix_reserved_word(name: &str) -> bool {
    RESERVED_WORDS.contains(&name)
}
```

Keep the existing `#[cfg(test)] mod tests { ... }` block unchanged.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib lexer::reserved::tests`

Expected: All three tests PASS.

- [ ] **Step 6: Run full unit test suite to verify no regressions**

Run: `cargo test --lib`

Expected: All tests PASS (this task should not affect any other module yet, since nothing imports the new module).

- [ ] **Step 7: Commit**

```bash
git add src/lexer/reserved.rs src/lexer/mod.rs
git commit -m "feat(lexer): add canonical POSIX reserved-words module"
```

---

## Task 2: Rename `Token::is_reserved_word` → `Token::matches_keyword`

This is a pure rename across `src/lexer/token.rs` and `src/parser/mod.rs`. It must be atomic — splitting it would leave the tree non-compiling. No behavior change.

**Files:**
- Modify: `src/lexer/token.rs:41-49` (method) and `src/lexer/token.rs:56-74` (tests)
- Modify: `src/parser/mod.rs:73` and `src/parser/mod.rs:103`

- [ ] **Step 1: Rename the method in `src/lexer/token.rs`**

Replace lines 41-49 (the existing `impl Token` block):

```rust
impl Token {
    /// True if this token is an unquoted literal word equal to `keyword`.
    pub fn matches_keyword(&self, keyword: &str) -> bool {
        if let Token::Word(w) = self {
            w.as_literal() == Some(keyword)
        } else {
            false
        }
    }
}
```

- [ ] **Step 2: Rename the test functions in `src/lexer/token.rs:56-74`**

Replace the entire `#[cfg(test)] mod tests { ... }` block:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Word, WordPart};

    #[test]
    fn test_matches_keyword_literal() {
        let tok = Token::Word(Word::literal("if"));
        assert!(tok.matches_keyword("if"));
        assert!(!tok.matches_keyword("then"));
    }

    #[test]
    fn test_matches_keyword_quoted_not_matched() {
        let tok = Token::Word(Word {
            parts: vec![WordPart::SingleQuoted("if".to_string())],
        });
        assert!(!tok.matches_keyword("if"));
    }

    #[test]
    fn test_matches_keyword_non_word_token() {
        assert!(!Token::Pipe.matches_keyword("if"));
    }
}
```

- [ ] **Step 3: Update parser call site in `src/parser/mod.rs:73`**

Inside `expect_reserved`, change `is_reserved_word` to `matches_keyword`:

```rust
    pub fn expect_reserved(&mut self, keyword: &str) -> error::Result<()> {
        if self.current.token.matches_keyword(keyword) {
            self.advance()?;
            Ok(())
        } else {
```

- [ ] **Step 4: Update parser call site in `src/parser/mod.rs:103`**

Inside `is_reserved`, change `is_reserved_word` to `matches_keyword`:

```rust
    pub fn is_reserved(&self, keyword: &str) -> bool {
        self.current.token.matches_keyword(keyword)
    }
```

- [ ] **Step 5: Verify nothing else references the old name**

Run: `cargo build 2>&1 | grep -i "is_reserved_word" || echo "no remaining references"`

Expected: `no remaining references`

If anything appears, it is an undocumented call site. Open it, rename, and re-run this step before continuing.

- [ ] **Step 6: Run full unit + integration tests**

Run: `cargo test`

Expected: All tests PASS, including the renamed `test_matches_keyword_*` cases and every parser test that exercises `expect_reserved` (if/then/else, while, until, for, case, brace group).

- [ ] **Step 7: Commit**

```bash
git add src/lexer/token.rs src/parser/mod.rs
git commit -m "refactor(lexer): rename Token::is_reserved_word to matches_keyword

The method never consulted any reserved-word list; it only checked
unquoted literal equality. The new name describes what it does."
```

---

## Task 3: Switch `resolve.rs` to consume the canonical module

**Files:**
- Modify: `src/builtin/resolve.rs:27-35` (delete duplicates) and `src/builtin/resolve.rs:49` (call site)

- [ ] **Step 1: Delete the local `RESERVED_WORDS` and `is_reserved_word`**

In `src/builtin/resolve.rs`, remove lines 27-35 (the doc comment + const + private function):

```rust
/// POSIX reserved words per IEEE Std 1003.1-2017 §2.4.
const RESERVED_WORDS: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi",
    "for", "if", "in", "then", "until", "while",
];

fn is_reserved_word(name: &str) -> bool {
    RESERVED_WORDS.contains(&name)
}
```

The doc comment (`/// Walk yosh's name-resolution order ...`) on the next item should remain; only delete the const + fn block above it.

- [ ] **Step 2: Add the import**

Add this `use` line near the top of `src/builtin/resolve.rs`, alongside the other `use crate::...` lines (after `use crate::exec::command::find_in_path;`):

```rust
use crate::lexer::reserved::is_posix_reserved_word;
```

- [ ] **Step 3: Update the call site in `resolve_command_kind`**

In the body of `resolve_command_kind` (formerly line 49), change:

```rust
    if is_reserved_word(name) {
```

to:

```rust
    if is_posix_reserved_word(name) {
```

- [ ] **Step 4: Run resolve.rs tests to verify the regression check passes**

Run: `cargo test --lib builtin::resolve::tests::keyword_detected`

Expected: PASS. This test asserts `resolve_command_kind(&env, "if")`, `"for"`, `"done"` all return `CommandKind::Keyword`, which exercises the new lookup path.

- [ ] **Step 5: Run the full unit + integration test suite**

Run: `cargo test`

Expected: All tests PASS.

- [ ] **Step 6: Run E2E tests**

Run: `./e2e/run_tests.sh`

Expected: All tests PASS. Existing coverage of `command -v if`, control-flow constructs (if/while/until/for/case), and brace groups exercises both consumers.

If any test fails, do not proceed — investigate and fix the regression.

- [ ] **Step 7: Commit**

```bash
git add src/builtin/resolve.rs
git commit -m "refactor(builtin): consume canonical reserved-words module

Removes the duplicate POSIX §2.4 list previously held in resolve.rs.
The single source of truth now lives in src/lexer/reserved.rs."
```

---

## Task 4: Remove the completed item from `TODO.md`

**Files:**
- Modify: `TODO.md:61`

Per project convention (CLAUDE.md): **delete completed items, do not mark them with `[x]`**.

- [ ] **Step 1: Delete the completed bullet**

Remove this line from `TODO.md` (currently line 61):

```
- [ ] POSIX reserved-word list duplicated — `RESERVED_WORDS` in `src/builtin/resolve.rs` and `Token::is_reserved_word` in `src/lexer/token.rs` each maintain their own POSIX §2.4 keyword list; consolidate into a single source of truth
```

- [ ] **Step 2: Verify the file is still well-formed**

Run: `cargo build` (sanity — TODO.md isn't part of the build but ensures nothing else broke).

Open `TODO.md` and confirm the surrounding `## Future: Code Quality Improvements` section still lists the remaining items in order with no orphaned blank lines.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed reserved-words consolidation item"
```

---

## Final Verification

After all four tasks are committed, run the complete verification suite once more from a clean state:

- [ ] **Step 1: Confirm working tree is clean**

Run: `git status`

Expected: `nothing to commit, working tree clean`.

- [ ] **Step 2: Run all unit + integration tests**

Run: `cargo test`

Expected: All tests PASS, no warnings introduced.

- [ ] **Step 3: Run E2E suite**

Run: `./e2e/run_tests.sh`

Expected: All tests PASS.

- [ ] **Step 4: Confirm no stale references remain**

Run: `cargo build 2>&1 | grep -E "(is_reserved_word|RESERVED_WORDS)" || echo "clean"`

Expected: `clean` (the only remaining mentions of `RESERVED_WORDS` should be inside `src/lexer/reserved.rs`, which the grep on build output won't surface anyway).

- [ ] **Step 5: Spot-check the canonical list is the only one**

Run a project-wide search for the literal list signature.

```bash
rg --no-heading -n '"if"\s*,\s*"in"\s*,\s*"then"' src/
```

Expected: zero matches outside `src/lexer/reserved.rs`. If any other file contains a similar literal sequence of POSIX keywords, investigate whether it represents another silent duplication and bring it to the user before closing this work.
