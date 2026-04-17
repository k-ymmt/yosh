# POSIX Reserved Words: Single Source of Truth

## Problem

The codebase has two functions named or related to "reserved words" with overlapping intent but inconsistent implementations:

1. `RESERVED_WORDS` and `is_reserved_word` in `src/builtin/resolve.rs` — an explicit list of POSIX §2.4 reserved words used by `command -v` / `command -V` to classify a name as `Keyword`.
2. `Token::is_reserved_word(&self, keyword: &str)` in `src/lexer/token.rs` — despite its name, this does **not** consult any reserved-word list. It only checks whether a token is an unquoted literal word equal to the supplied keyword. The parser calls it via `expect_reserved("if")` and similar with hard-coded keyword string literals.

This creates two issues:

- **No single source of truth.** The canonical POSIX §2.4 reserved-word list lives in `resolve.rs` only. Parser call sites encode the same keywords as scattered string literals (`"if"`, `"then"`, `"fi"`, …). A future addition or correction would require updating both, and there is no mechanism to detect drift.
- **Misleading function name.** `Token::is_reserved_word` suggests it validates against POSIX reserved words, but it accepts any string. A reader auditing where reserved-word semantics are enforced gets a false positive here.

This work is a pure refactor: no observable shell behavior changes.

## Goals

- Establish exactly one canonical list of POSIX §2.4 reserved words in the codebase.
- Make function names match what the code actually does.
- Keep changes minimal and reversible; do not redesign reserved-word handling at a deeper level (e.g., tokenizing reserved words into dedicated `Token` variants is out of scope).

## Non-Goals

- Refactoring parser call sites to use a typed `ReservedWord` enum (considered as alternative case Z, rejected for excess churn with no behavior or performance benefit).
- Changing how the lexer recognizes reserved words (POSIX §2.4 context-sensitive recognition is unchanged).
- Adding new shell features.

## Design

### New module: `src/lexer/reserved.rs`

Defines the canonical list and a single predicate.

```rust
//! POSIX reserved words per IEEE Std 1003.1-2017 §2.4.

pub const RESERVED_WORDS: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi",
    "for", "if", "in", "then", "until", "while",
];

pub fn is_posix_reserved_word(name: &str) -> bool {
    RESERVED_WORDS.contains(&name)
}
```

`src/lexer/mod.rs` adds `pub mod reserved;`.

**Rationale for placement in `lexer/`:** POSIX §2.4 is a lexical/syntactic specification, and the lexer module already houses `token.rs`. Placing it here keeps the dependency direction healthy (`builtin` depends on `lexer`, not vice versa).

### Rename `Token::is_reserved_word` → `Token::matches_keyword`

The body is unchanged. The new name accurately describes the operation: "this token is an unquoted literal word equal to the given keyword."

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

The parser-facing API names (`expect_reserved`, `is_reserved`) are **kept**. They live at the parser layer where the caller's intent ("the keyword I'm asking about IS a POSIX reserved word") is correct, even though the underlying token primitive is generic.

### Consumer updates

**`src/builtin/resolve.rs`:**
- Delete the local `const RESERVED_WORDS` and `fn is_reserved_word`.
- Add `use crate::lexer::reserved::is_posix_reserved_word;`.
- Replace `is_reserved_word(name)` (line 49) with `is_posix_reserved_word(name)`.

**`src/parser/mod.rs`:**
- In `expect_reserved` (line 73): `self.current.token.is_reserved_word(keyword)` → `self.current.token.matches_keyword(keyword)`.
- In `is_reserved` (line 103): same rename.
- The literal `expect_reserved("if")` etc. call sites stay as-is.

**`src/lexer/token.rs`:**
- Rename the method as above.
- Rename test functions (`test_is_reserved_word_*` → `test_matches_keyword_*`) and update their assertions accordingly.

### New tests in `src/lexer/reserved.rs`

```rust
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

The length-pinning test guards against an accidental edit to the canonical list.

## Behavior Verification

This is a refactor with no behavior change. Verification:

1. `cargo test` — all unit and integration tests pass.
2. `./e2e/run_tests.sh` — existing E2E coverage of `command -v if` / `command -V while` and the control-flow constructs (if/then/else, while, until, for, case, brace group) regression-checks the rename.

No new E2E tests are required.

## Alternatives Considered

- **Case Y (introduce a `ReservedWord` enum):** Adds typing but parser call sites would still pass `&'static str` (via `as_str()`) into the existing `matches_keyword`, providing no behavior or performance benefit. Rejected as ceremony without payoff.
- **Case Z (enum + parser refactor):** All `expect_reserved("if")` call sites become `expect_reserved(ReservedWord::If)`. Stronger compile-time guarantees but large change footprint, and POSIX §2.4 reserved words are a closed set fixed by the standard, so the typo-prevention benefit is marginal. Rejected.
- **Keep list in `resolve.rs` and `pub`-export it:** Cheaper move but leaves a POSIX-language-level definition inside a builtin module, against the natural module boundary. Rejected.

## Out of Scope

- Tokenizing reserved words into dedicated `Token::ReservedWord(ReservedWord)` variants (would be the only path to a real performance win on the parser hot path, but requires implementing POSIX §2.4 context-sensitive recognition in the lexer — far beyond this refactor's scope).
- Renaming parser-layer APIs `expect_reserved` / `is_reserved`.
