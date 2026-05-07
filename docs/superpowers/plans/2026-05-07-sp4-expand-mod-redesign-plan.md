# SP4 — `src/expand/mod.rs` Responsibility Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `src/expand/mod.rs` (1230 lines) into a thin facade plus four responsibility-focused submodules (`scan.rs`, `tilde.rs`, `heredoc.rs`, `pipeline.rs`), and decompose `expand_part_to_fields`'s 10-arm `match` into 7 per-variant helpers — preserving every public API consumed by `exec/`, `interactive/`, `expand/arith.rs`, and `benches/expand_bench.rs`. Verify expand throughput holds within ±5% of the pre-PR-C baseline via `cargo bench --bench expand_bench`.

**Architecture:** `mod.rs` keeps the canonical `ExpandedField` type and the three public entry points (`expand_word` / `expand_words` / `expand_word_to_string`). All other concerns move out: lexical balanced-paren scanners → `scan.rs`; tilde resolution → `tilde.rs`; heredoc body expansion → `heredoc.rs`; the field-producing pipeline (`expand_word_to_fields`, `expand_part_to_fields`, `expand_param_to_fields`, `ifs_first_char`) → `pipeline.rs`. External callers see no signature change because `mod.rs` re-exports `skip_balanced_*`, `expand_tilde_*`, and renames `heredoc::expand_body` back to `expand_heredoc_body`.

**Tech Stack:** Rust 2024 edition, criterion (`benches/expand_bench.rs`). No new dependencies.

**Reference Documents:**
- Spec: `docs/superpowers/specs/2026-05-06-sp4-expand-mod-redesign-design.md` (revised commit `fcc7eff`)
- Umbrella: `docs/superpowers/specs/2026-05-06-large-file-redesign-umbrella-design.md`
- Predecessor plans: SP1 `2026-05-06-sp1-plugin-host-redesign-plan.md`, SP2 `2026-05-07-sp2-env-jobs-redesign-plan.md`, SP3 `2026-05-07-sp3-highlight-scanner-redesign-plan.md`

**Line-count Target:** Per umbrella DoD #6, each production file ≤ 400 lines. Tests are exempt (per-SP convention from SP3).

**Definition of Done (per umbrella + SP4):**

1. `cargo test` PASS (unit + integration).
2. `./e2e/run_tests.sh` PASS (full run, no filter).
3. `cargo bench --no-run` PASS (compile only).
4. `cargo clippy --all-targets -- -D warnings` — only the two pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99` remain.
5. `cargo fmt --check` PASS.
6. Each production file in `src/expand/` ≤ 400 lines.
7. README/CLAUDE.md/TODO.md references to `src/expand/mod.rs` still resolve via grep.
8. Public API names + signatures preserved (zero diff in callers from `grep -rn "crate::expand::" src/ tests/ benches/`).
9. `tests/pty_interactive.rs` PASS — non-flaky over 3 consecutive runs.
10. **Performance:** `cargo bench --bench expand_bench` shows expand throughput within ±5% of the PR-B baseline. Captured in Task A5; verified in Task C3.
11. TODO.md entry "`skip_balanced_*` unterminated input tests" (line 71) is removed (resolved by Task A3).

---

## File Structure

After all tasks complete, `src/expand/` looks like:

```
src/expand/
  mod.rs               — module declarations + re-exports + ExpandedField type
                         + 3 public entry points (expand_word/expand_words/expand_word_to_string).
                         (~150 production lines, plus reduced tests after relocation.)
  pipeline.rs          — expand_word_to_fields (pub(super)), expand_part_to_fields (private),
                         the 7 per-variant helpers, expand_param_to_fields (private),
                         ifs_first_char (private). Pipeline tests.
  heredoc.rs           — expand_body (pub(super)), expand_string (private), expand_part (private).
                         Heredoc tests.
  scan.rs              — skip_balanced_parens, skip_balanced_braces, skip_balanced_double_parens
                         (all pub(super)). Existing scan tests + 3 new unterminated-input tests.
  tilde.rs             — expand_tilde_prefix, expand_tilde_user (both pub(super)).
                         Tilde tests.
  pattern.rs           — (existing, untouched)
  param.rs             — (existing, untouched)
  command_sub.rs       — (existing, untouched)
  pathname.rs          — (existing, untouched)
  field_split.rs       — (existing, untouched)
  arith.rs             — (existing, untouched)
```

**Re-exports in `mod.rs`** (after all PRs):

```rust
pub(crate) use scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
pub(crate) use tilde::{expand_tilde_prefix, expand_tilde_user};
pub use heredoc::expand_body as expand_heredoc_body;
```

These re-exports preserve the call sites in `expand/arith.rs` (uses `crate::expand::skip_balanced_parens`), `interactive/mod.rs` (uses `crate::expand::expand_tilde_prefix`), and `exec/redirect.rs` (uses `crate::expand::expand_heredoc_body`) — zero diff in any caller.

**Visibility:**
- `ExpandedField` and the 3 public entry points: `pub`.
- `expand_word_to_fields`: `pub(super)` (called from `expand_word` / `expand_word_to_string` in `mod.rs`).
- All other moved functions: private to their submodule, except where re-exported by `mod.rs`.

**Pipeline helper signatures (after PR-C):**

```rust
// All field-mutating helpers receive `&mut Vec<ExpandedField>` and the in_double_quote flag
// where their behavior depends on it.
fn expand_part_literal(s: &str, fields: &mut Vec<ExpandedField>, in_double_quote: bool);
fn expand_part_quoted_literal(s: &str, fields: &mut Vec<ExpandedField>);
fn expand_part_double_quoted(env: &mut ShellEnv, parts: &[WordPart], fields: &mut Vec<ExpandedField>) -> crate::error::Result<()>;
fn expand_part_tilde(env: &mut ShellEnv, user: Option<&str>, fields: &mut Vec<ExpandedField>);
fn expand_part_parameter(env: &mut ShellEnv, param: &ParamExpr, fields: &mut Vec<ExpandedField>, in_double_quote: bool) -> crate::error::Result<()>;
fn expand_part_command_sub(env: &mut ShellEnv, program: &Program, fields: &mut Vec<ExpandedField>, in_double_quote: bool);
fn expand_part_arith_sub(env: &mut ShellEnv, expr: &str, fields: &mut Vec<ExpandedField>, in_double_quote: bool) -> crate::error::Result<()>;
```

Three of the variants (`EscapedLiteral`, `SingleQuoted`, `DollarSingleQuoted`) all funnel into one helper because their current bodies are identical: `fields.last_mut().unwrap().push_quoted(s);`.

---

## Task 0: Pre-flight Verification

Confirm a clean baseline before any move. This is non-negotiable: SP1/SP2/SP3 each surfaced regressions because the pre-flight skipped one of the gates.

**Files:**
- Read-only: `src/expand/mod.rs`, `TODO.md`, `Cargo.toml`

- [ ] **Step 1: Confirm baseline `cargo test` is green**

Run: `cargo test 2>&1 | tail -20`
Expected: every test PASS, no compilation errors. If any test fails, STOP — do not start the refactor on a red baseline.

- [ ] **Step 2: Confirm baseline E2E is green**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: `All tests passed` or per-suite green totals. If any test fails, STOP and report.

- [ ] **Step 3: Confirm baseline clippy state**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -30`
Expected: exactly two errors at `src/plugin/mod.rs:98-99` (`doc_lazy_continuation`). These are pre-existing and out of scope per the umbrella spec. Any other warnings or errors must be addressed before starting.

- [ ] **Step 4: Confirm baseline fmt is clean**

Run: `cargo fmt --check 2>&1`
Expected: no output (exit 0).

- [ ] **Step 5: Snapshot the public-API caller list**

Run: `grep -rn "crate::expand::\|yosh::expand::" src/ tests/ benches/ 2>/dev/null | grep -v "//" | sort -u > /tmp/sp4-callers-before.txt && wc -l /tmp/sp4-callers-before.txt`
Expected: ≥10 lines. Save the snapshot for Task D1's zero-diff verification.

- [ ] **Step 6: Confirm TODO.md target line exists**

Run: `grep -n "skip_balanced_\* unterminated input tests" TODO.md`
Expected: a hit on line ~71. If absent, the umbrella's TODO cleanup expectation is stale — flag it in Task D1 instead of failing here.

---

## Task A1: Create `scan.rs` and Move `skip_balanced_*` Functions

Move the three byte-level balanced-bracket scanners into a dedicated module. They are the cleanest extraction (no cross-dependencies, used by `expand/arith.rs` and `expand/mod.rs::expand_heredoc_string`).

**Files:**
- Create: `src/expand/scan.rs`
- Modify: `src/expand/mod.rs` (remove the three functions, add `mod scan;` and `pub(crate) use scan::{...};`)

- [ ] **Step 1: Create `src/expand/scan.rs` with the three functions**

Cut these functions verbatim from `src/expand/mod.rs:445-612` (the entire `skip_balanced_parens`, `skip_balanced_braces`, `skip_balanced_double_parens` block, including their doc comments). Paste into `src/expand/scan.rs`. Change `pub(crate)` to `pub(super)` on each function — they are visible only within `expand/`, and re-exported by `mod.rs` for cross-crate use. Add this header:

```rust
//! Lexical balanced-bracket scanners with quote/escape awareness.
//!
//! Used by `expand::heredoc` (after PR-B) and `expand::arith` for parenthesis-,
//! brace-, and double-paren depth tracking inside string bodies.
```

The three functions are otherwise unchanged.

- [ ] **Step 2: Update `src/expand/mod.rs` to declare and re-export `scan`**

In the top of `mod.rs`, after the existing `pub mod arith; ...` declarations, add:

```rust
mod scan;

pub(crate) use scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
```

Delete the three `pub(crate) fn skip_balanced_*` definitions and their preceding doc comments from `mod.rs:445-612`. The internal callers in `expand_heredoc_string` (currently calling `skip_balanced_*` as bare names) keep working because of the `pub(crate) use` brings the names back into module scope.

- [ ] **Step 3: Run `cargo build` to confirm compile**

Run: `cargo build 2>&1 | tail -20`
Expected: clean build. If `expand::arith` complains about `crate::expand::skip_balanced_parens` (line 89), nothing should change — the path still resolves through the `pub(crate) use` re-export.

- [ ] **Step 4: Run scan tests in their current location**

Tests still live in `mod.rs::tests` at this stage. Run: `cargo test expand::tests::test_skip_balanced 2>&1 | tail -15`
Expected: 9 tests PASS (`test_skip_balanced_parens_simple`, `_nested`, `_single_quoted`, `_double_quoted`, `_backslash_escape`, `_double_parens_simple`, `_double_parens_nested`, `_braces_simple`, `_braces_nested`, `_braces_single_quoted`, `_braces_double_quoted`, `_braces_backslash_escape`).

- [ ] **Step 5: Commit**

```bash
git add src/expand/scan.rs src/expand/mod.rs
git commit -m "$(cat <<'EOF'
refactor(expand): extract skip_balanced_* scanners to scan.rs

Move skip_balanced_parens, skip_balanced_braces, and
skip_balanced_double_parens from src/expand/mod.rs (lines 445-612) into
a new src/expand/scan.rs module. Public-API surface is preserved via
pub(crate) use re-exports — crate::expand::skip_balanced_parens still
resolves for the lone external caller in expand/arith.rs.

Tests stay in mod.rs::tests for now; they relocate in Task A2.

Part of SP4 PR-A.
EOF
)"
```

---

## Task A2: Move Scanner Tests to `scan.rs` and Add Unterminated-Input Tests

Relocate the existing 11 `test_skip_balanced_*` tests from `mod.rs::tests` into a new `#[cfg(test)] mod tests` block inside `scan.rs`. While there, add the three unterminated-input tests called out in TODO.md.

**Files:**
- Modify: `src/expand/scan.rs` (append tests block)
- Modify: `src/expand/mod.rs` (delete the relocated tests)

- [ ] **Step 1: Write the failing unterminated-input tests in `scan.rs`**

Append to `src/expand/scan.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ── existing: balanced-parens ─────────────────────────────────────
    #[test]
    fn test_skip_balanced_parens_simple() {
        let input = b"echo hello)";
        assert_eq!(skip_balanced_parens(input, 0), 10);
    }

    #[test]
    fn test_skip_balanced_parens_nested() {
        let input = b"(inner) outer)";
        assert_eq!(skip_balanced_parens(input, 0), 13);
    }

    #[test]
    fn test_skip_balanced_parens_single_quoted() {
        let input = b"')' real)";
        assert_eq!(skip_balanced_parens(input, 0), 8);
    }

    #[test]
    fn test_skip_balanced_parens_double_quoted() {
        let input = b"\")(\" real)";
        assert_eq!(skip_balanced_parens(input, 0), 9);
    }

    #[test]
    fn test_skip_balanced_parens_backslash_escape() {
        let input = b"\\) real)";
        assert_eq!(skip_balanced_parens(input, 0), 7);
    }

    // ── new: unterminated-input contract ─────────────────────────────
    #[test]
    fn test_skip_balanced_parens_unterminated_returns_len() {
        let input = b"echo hello";
        assert_eq!(skip_balanced_parens(input, 0), input.len());
    }

    // ── existing: balanced-double-parens ─────────────────────────────
    #[test]
    fn test_skip_balanced_double_parens_simple() {
        let input = b"1 + 2))";
        assert_eq!(skip_balanced_double_parens(input, 0), 5);
    }

    #[test]
    fn test_skip_balanced_double_parens_nested() {
        let input = b"(1 + 2) * 3))";
        assert_eq!(skip_balanced_double_parens(input, 0), 11);
    }

    // ── new: unterminated-input contract ─────────────────────────────
    #[test]
    fn test_skip_balanced_double_parens_unterminated_returns_len() {
        let input = b"1 + 2";
        assert_eq!(skip_balanced_double_parens(input, 0), input.len());
    }

    // ── existing: balanced-braces ────────────────────────────────────
    #[test]
    fn test_skip_balanced_braces_simple() {
        let input = b"var}";
        assert_eq!(skip_balanced_braces(input, 0), 3);
    }

    #[test]
    fn test_skip_balanced_braces_nested() {
        let input = b"{inner} outer}";
        assert_eq!(skip_balanced_braces(input, 0), 13);
    }

    #[test]
    fn test_skip_balanced_braces_single_quoted() {
        let input = b"var:-'}'}";
        assert_eq!(skip_balanced_braces(input, 0), 8);
    }

    #[test]
    fn test_skip_balanced_braces_double_quoted() {
        let input = b"var:-\"}{\"}";
        assert_eq!(skip_balanced_braces(input, 0), 9);
    }

    #[test]
    fn test_skip_balanced_braces_backslash_escape() {
        let input = b"var:-\\} real}";
        assert_eq!(skip_balanced_braces(input, 0), 12);
    }

    // ── new: unterminated-input contract ─────────────────────────────
    #[test]
    fn test_skip_balanced_braces_unterminated_returns_len() {
        let input = b"var:-default";
        assert_eq!(skip_balanced_braces(input, 0), input.len());
    }
}
```

- [ ] **Step 2: Delete the same tests from `mod.rs::tests`**

Remove the 11 relocated `test_skip_balanced_*` tests from `src/expand/mod.rs::tests` (currently around lines 1159-1228). The block to delete starts at the first `#[test] fn test_skip_balanced_parens_simple` and ends at the closing `}` of `test_skip_balanced_braces_backslash_escape`. Leave the rest of `mod tests` intact.

- [ ] **Step 3: Run `cargo test` to confirm all 14 scan tests pass**

Run: `cargo test expand::scan 2>&1 | tail -25`
Expected: 14 tests PASS (11 relocated + 3 new). If any unterminated-input test fails, the documented contract (return `bytes.len()`) is broken — investigate before proceeding.

- [ ] **Step 4: Run full `cargo test` to confirm nothing else regressed**

Run: `cargo test 2>&1 | tail -5`
Expected: full test count unchanged (only the 3 new tests added; 11 relocated). No FAILs.

- [ ] **Step 5: Delete the TODO.md entry**

Open `TODO.md` and delete the line:

```
- [ ] `skip_balanced_*` unterminated input tests — `skip_balanced_parens`, `skip_balanced_braces`, `skip_balanced_double_parens` all return `bytes.len()` on unterminated input but none have tests for this behavior (`src/expand/mod.rs`)
```

Per CLAUDE.md project convention, completed items are deleted (not marked `[x]`).

- [ ] **Step 6: Commit**

```bash
git add src/expand/scan.rs src/expand/mod.rs TODO.md
git commit -m "$(cat <<'EOF'
test(expand): relocate scan tests to scan.rs and cover unterminated input

Move 11 test_skip_balanced_* tests from src/expand/mod.rs::tests into
src/expand/scan.rs::tests. Add three unterminated-input tests verifying
the documented contract that skip_balanced_parens, skip_balanced_braces,
and skip_balanced_double_parens all return bytes.len() when the closing
delimiter is missing.

Resolves the TODO.md entry "skip_balanced_* unterminated input tests".

Part of SP4 PR-A.
EOF
)"
```

---

## Task A3: Create `tilde.rs` and Move `expand_tilde_*` Functions

Move the two tilde resolution functions into their own module. Both are `pub(crate)`-visible (referenced from `interactive/mod.rs` for ENV preprocessing).

**Files:**
- Create: `src/expand/tilde.rs`
- Modify: `src/expand/mod.rs` (remove the two functions, add `mod tilde;` and `pub(crate) use tilde::{...};`)

- [ ] **Step 1: Create `src/expand/tilde.rs` with the two functions**

Cut `expand_tilde_prefix` (currently `mod.rs:617-641`) and `expand_tilde_user` (currently `mod.rs:644-657`) verbatim, including doc comments. Paste into `src/expand/tilde.rs`. Change visibility from `pub(crate)` to `pub(super)`. Add this header:

```rust
//! POSIX tilde expansion: `~` (HOME) and `~user` (getpwnam).
//!
//! Used by `expand::pipeline` for inline word-part tilde, by `expand::heredoc`
//! for unquoted heredoc bodies, and re-exported from `expand` for use by
//! `interactive::mod` during ENV preprocessing.
```

`expand_tilde_user` uses `libc::getpwnam` and `std::ffi::{CString, CStr}` — keep the existing `use` statements local to the function bodies (they are already inline as `use ...;` inside `expand_tilde_user`).

- [ ] **Step 2: Update `src/expand/mod.rs` to declare and re-export `tilde`**

After the `mod scan;` line, add:

```rust
mod tilde;

pub(crate) use tilde::{expand_tilde_prefix, expand_tilde_user};
```

Delete the two `pub(crate) fn expand_tilde_*` definitions from `mod.rs`. The remaining call site inside `mod.rs::expand_heredoc_part` (`out.push_str(&expand_tilde_user(user));`, line 338) and inside `mod.rs::expand_part_to_fields` (line 406) keep working because the names are brought back into scope by the `pub(crate) use`.

- [ ] **Step 3: Run `cargo build` to confirm compile**

Run: `cargo build 2>&1 | tail -10`
Expected: clean build. The external caller `interactive/mod.rs:94` (`crate::expand::expand_tilde_prefix`) still resolves through the re-export.

- [ ] **Step 4: Move tilde tests to `tilde.rs`**

Append to `src/expand/tilde.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_prefix_home() {
        assert_eq!(
            expand_tilde_prefix(Some("/home/user"), "~/docs"),
            "/home/user/docs"
        );
    }

    #[test]
    fn test_expand_tilde_prefix_home_only() {
        assert_eq!(expand_tilde_prefix(Some("/home/user"), "~"), "/home/user");
    }

    #[test]
    fn test_expand_tilde_prefix_no_home() {
        assert_eq!(expand_tilde_prefix(None, "~/docs"), "~/docs");
    }

    #[test]
    fn test_expand_tilde_prefix_no_tilde() {
        assert_eq!(
            expand_tilde_prefix(Some("/home/user"), "/abs/path"),
            "/abs/path"
        );
    }

    #[test]
    fn test_expand_tilde_prefix_empty_home() {
        assert_eq!(expand_tilde_prefix(Some(""), "~/docs"), "~/docs");
    }
}
```

Delete the same five tests from `mod.rs::tests` (currently around lines 1129-1157).

The two tilde tests that depend on the full `expand_word` / `expand_word_to_string` pipeline (`test_tilde_root_starts_with_slash`, `test_tilde_none`, `test_tilde_none_no_home`) **stay in `mod.rs::tests`** — they are integration tests for the public API, not unit tests for `expand_tilde_*`.

- [ ] **Step 5: Run `cargo test` to confirm all tilde tests pass**

Run: `cargo test expand::tilde 2>&1 | tail -15`
Expected: 5 tests PASS in `expand::tilde::tests`.

Run: `cargo test expand::tests::test_tilde 2>&1 | tail -10`
Expected: 3 tests PASS (`test_tilde_root_starts_with_slash`, `test_tilde_none`, `test_tilde_none_no_home`) staying in `mod.rs::tests`.

- [ ] **Step 6: Commit**

```bash
git add src/expand/tilde.rs src/expand/mod.rs
git commit -m "$(cat <<'EOF'
refactor(expand): extract expand_tilde_prefix/expand_tilde_user to tilde.rs

Move the two tilde expansion functions from src/expand/mod.rs into a
dedicated src/expand/tilde.rs module. Public surface preserved via
pub(crate) use re-export — crate::expand::expand_tilde_prefix still
resolves for the external caller in interactive/mod.rs.

Five unit tests follow the move; three integration-style tilde tests
remain in mod.rs::tests because they exercise the full expand_word
pipeline.

Part of SP4 PR-A.
EOF
)"
```

---

## Task A4: PR-A Verification Gate

Confirm PR-A is fully green before starting PR-B. PR-B builds on `scan` and `tilde`; if either is broken, defects compound.

**Files:**
- Read-only

- [ ] **Step 1: Run full `cargo test`**

Run: `cargo test 2>&1 | tail -10`
Expected: full test count = baseline + 3 (the new unterminated-input tests). Zero FAILs.

- [ ] **Step 2: Run E2E suite**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All tests passed.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -30`
Expected: only the two pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99`. No new warnings.

- [ ] **Step 4: Run fmt check**

Run: `cargo fmt --check 2>&1`
Expected: no output. If anything is reported, run `cargo fmt` and amend the most recent commit.

- [ ] **Step 5: Verify production line counts so far**

Run: `wc -l src/expand/scan.rs src/expand/tilde.rs src/expand/mod.rs`
Expected:
- `scan.rs` ≤ 250 lines (~165 production + ~80 tests)
- `tilde.rs` ≤ 110 lines (~45 production + ~50 tests)
- `mod.rs` ≤ 1080 lines (down from 1230 — three skip_balanced functions + two tilde functions + their tests removed)

- [ ] **Step 6: Snapshot caller list (interim)**

Run: `grep -rn "crate::expand::\|yosh::expand::" src/ tests/ benches/ 2>/dev/null | grep -v "//" | sort -u > /tmp/sp4-callers-after-A.txt && diff /tmp/sp4-callers-before.txt /tmp/sp4-callers-after-A.txt`
Expected: zero diff. If non-empty, an external caller signature changed — investigate before continuing.

---

## Task B1: Create `heredoc.rs` and Move `expand_heredoc_*` Functions

Move the 200-line heredoc family into its own module. Internally rename `expand_heredoc_body` → `expand_body` because it is no longer the `expand_*` symmetric API (per spec). External name preserved via `pub use heredoc::expand_body as expand_heredoc_body` in `mod.rs`.

**Files:**
- Create: `src/expand/heredoc.rs`
- Modify: `src/expand/mod.rs` (remove the three functions, add `mod heredoc;` and the `pub use ... as ...` re-export)

- [ ] **Step 1: Create `src/expand/heredoc.rs` with the three functions, renamed**

Cut `expand_heredoc_body` (currently `mod.rs:148-170`), `expand_heredoc_string` (`mod.rs:174-305`), and `expand_heredoc_part` (`mod.rs:307-341`) verbatim into `src/expand/heredoc.rs`. Apply these renames:
- `expand_heredoc_body` → `expand_body` (the public-facing name within this module)
- `expand_heredoc_string` → `expand_string` (private)
- `expand_heredoc_part` → `expand_part` (private)

Update internal callers within `expand_body`: `expand_heredoc_part(...)` → `expand_part(...)`, `expand_heredoc_string(...)` → `expand_string(...)`.

Add this header at the top of `heredoc.rs`:

```rust
//! POSIX §2.7.4 here-document body expansion.
//!
//! Heredoc expansion is a distinct pipeline from word expansion: there is no
//! field splitting, no pathname expansion, and no tilde expansion. Quoted
//! heredocs (`<<'EOF'`) suppress all expansion; unquoted heredocs perform
//! parameter, arithmetic, and command substitution only.

use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, WordPart};
use super::{arith, command_sub, param};
use super::scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
use super::tilde::expand_tilde_user;
```

Set the function visibility:
- `pub(super) fn expand_body(...)` — called from the `pub use` re-export in `mod.rs`
- `fn expand_string(...)` — private
- `fn expand_part(...)` — private

- [ ] **Step 2: Update `src/expand/mod.rs` to declare and re-export `heredoc`**

After the `mod tilde;` line, add:

```rust
mod heredoc;

pub use heredoc::expand_body as expand_heredoc_body;
```

Delete the three `expand_heredoc_*` definitions from `mod.rs:144-341` (they are now in `heredoc.rs`). The external callers (`exec/redirect.rs:184` uses `crate::expand::expand_heredoc_body`) keep working because the re-export brings back the original public name.

- [ ] **Step 3: Run `cargo build` to confirm compile**

Run: `cargo build 2>&1 | tail -15`
Expected: clean build.

If `heredoc.rs` reports unresolved imports for `arith` / `command_sub` / `param`: those are `pub mod arith;` / `pub mod command_sub;` / `pub mod param;` declarations at the top of `mod.rs`. The `use super::{arith, command_sub, param};` line in `heredoc.rs` resolves them.

If unresolved for `skip_balanced_*` / `expand_tilde_user`: those went through `mod scan;` / `mod tilde;` (private), so submodules access them via `super::scan::*` / `super::tilde::*` directly (not via the `pub(crate) use` re-export, which is for cross-crate use). The `use super::scan::*;` / `use super::tilde::expand_tilde_user;` in the header handles this.

- [ ] **Step 4: Move heredoc tests to `heredoc.rs`**

Append to `src/expand/heredoc.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, WordPart};

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn test_expand_heredoc_body_literal() {
        let mut env = make_env();
        let parts = vec![WordPart::Literal("hello world\n".to_string())];
        assert_eq!(expand_body(&mut env, &parts, true), "hello world\n");
    }

    #[test]
    fn test_expand_heredoc_body_quoted_no_expansion() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let parts = vec![WordPart::Literal("value is $FOO\n".to_string())];
        assert_eq!(expand_body(&mut env, &parts, true), "value is $FOO\n");
    }

    #[test]
    fn test_expand_heredoc_body_unquoted_expands() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let parts = vec![
            WordPart::Literal("value is ".to_string()),
            WordPart::Parameter(ParamExpr::Simple("FOO".to_string())),
            WordPart::Literal("\n".to_string()),
        ];
        assert_eq!(expand_body(&mut env, &parts, false), "value is bar\n");
    }
}
```

Note the function name in tests is `expand_body` (not `expand_heredoc_body`) — this is the internal name within `heredoc.rs`. The original tests reference `expand_heredoc_body`; rename to `expand_body` during the move.

Delete the three corresponding tests from `mod.rs::tests` (currently around lines 1095-1126).

- [ ] **Step 5: Run heredoc tests**

Run: `cargo test expand::heredoc 2>&1 | tail -15`
Expected: 3 tests PASS in `expand::heredoc::tests`.

- [ ] **Step 6: Run full `cargo test` and E2E**

Run: `cargo test 2>&1 | tail -5`
Expected: full count, zero FAILs.

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All tests passed. **Heredoc is on the runtime path — any heredoc-related E2E regression must be investigated before commit.**

- [ ] **Step 7: Commit**

```bash
git add src/expand/heredoc.rs src/expand/mod.rs
git commit -m "$(cat <<'EOF'
refactor(expand): extract expand_heredoc_* to heredoc.rs

Move the three heredoc body expansion functions from src/expand/mod.rs
(lines 144-341) into a dedicated src/expand/heredoc.rs module. Apply
internal renames per the SP4 spec: expand_heredoc_body → expand_body,
expand_heredoc_string → expand_string, expand_heredoc_part → expand_part.
External public name preserved via pub use heredoc::expand_body as
expand_heredoc_body in mod.rs.

Heredoc tests follow the move and use the internal expand_body name.
Cross-module dependencies: super::scan for skip_balanced_*, super::tilde
for expand_tilde_user.

Part of SP4 PR-B.
EOF
)"
```

---

## Task B2: PR-B Verification Gate + Bench Baseline

Confirm PR-B is green and capture the bench baseline that PR-C must hit within ±5%.

**Files:**
- Read-only + bench output

- [ ] **Step 1: Run full `cargo test`**

Run: `cargo test 2>&1 | tail -10`
Expected: full count, zero FAILs.

- [ ] **Step 2: Run E2E suite**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All tests passed.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -30`
Expected: only the two pre-existing `doc_lazy_continuation` errors. No new warnings.

- [ ] **Step 4: Run fmt check**

Run: `cargo fmt --check 2>&1`
Expected: no output.

- [ ] **Step 5: Capture `expand_bench` baseline**

Run: `cargo bench --bench expand_bench 2>&1 | tee /tmp/sp4-bench-pr-b.txt | tail -50`
Expected: criterion produces per-benchmark `time:   [...]` lines. Save the file — Task C3 compares against it.

If the bench fails to compile (`cargo bench --no-run` step), STOP and investigate — likely a test API change leaked into the bench module.

- [ ] **Step 6: Verify production line counts**

Run: `wc -l src/expand/*.rs`
Expected:
- `mod.rs` ≤ 800 lines (down from 1230 — heredoc 200 lines + heredoc tests ~30 lines + earlier removals)
- `heredoc.rs` ≤ 280 lines (~210 production + ~50 tests)
- `scan.rs` ≤ 250 lines
- `tilde.rs` ≤ 110 lines

---

## Task C1: Create `pipeline.rs` and Move Field-Producing Functions (Without Decomposition Yet)

Move `expand_word_to_fields`, `expand_part_to_fields`, `expand_param_to_fields`, and `ifs_first_char` to `pipeline.rs` as a single relocation step. Do **not** decompose `expand_part_to_fields` yet — that's Task C2. Two separate commits make bisection easier if a regression appears.

**Files:**
- Create: `src/expand/pipeline.rs`
- Modify: `src/expand/mod.rs` (remove the four functions, add `mod pipeline;`)

- [ ] **Step 1: Create `src/expand/pipeline.rs` with all four functions**

Cut these regions verbatim from `src/expand/mod.rs`:
- `expand_word_to_fields` (currently `mod.rs:346-355`) — change `fn` to `pub(super) fn` (called from `mod.rs::expand_word` and `mod.rs::expand_word_to_string`).
- `expand_part_to_fields` (currently `mod.rs:359-443`) — keep `fn` (private).
- `expand_param_to_fields` (currently `mod.rs:660-723`) — keep `fn` (private).
- `ifs_first_char` (currently `mod.rs:726-731`) — keep `fn` (private).

Paste into `src/expand/pipeline.rs` in this order. Add this header:

```rust
//! Field-producing core of the POSIX expansion pipeline.
//!
//! `expand_word_to_fields` is the entry point called by `expand::expand_word`
//! and `expand::expand_word_to_string`. It walks a `Word`'s parts, dispatching
//! each to a per-variant helper, and accumulates `ExpandedField` values that
//! the public API then runs through field-splitting, pathname expansion, and
//! quote removal.

use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};
use super::{arith, command_sub, param};
use super::ExpandedField;
```

- [ ] **Step 2: Update `src/expand/mod.rs` to declare `pipeline`**

After the `mod heredoc;` line, add:

```rust
mod pipeline;

use pipeline::expand_word_to_fields;
```

The `use pipeline::expand_word_to_fields;` brings the relocated function back into `mod.rs`'s scope so the existing call sites in `expand_word` (line 107) and `expand_word_to_string` (line 134) keep working unchanged.

Delete the four function definitions from `mod.rs`. The four functions span:
- `expand_word_to_fields`: `mod.rs:346-355`
- `expand_part_to_fields`: `mod.rs:359-443`
- `expand_param_to_fields`: `mod.rs:660-723`
- `ifs_first_char`: `mod.rs:726-731`

Plus the section divider comments (e.g., `// ─── Stage 1: expand to ExpandedField list ──`).

- [ ] **Step 3: Run `cargo build` to confirm compile**

Run: `cargo build 2>&1 | tail -20`
Expected: clean build.

Common compile errors at this stage:
- `ExpandedField is private` — make sure `mod.rs` declares `pub struct ExpandedField` (it does already) and `pipeline.rs` uses `super::ExpandedField`.
- `expand_word_to_fields is private` — confirm the visibility is `pub(super) fn` in `pipeline.rs`.
- `cannot find ParamExpr / SpecialParam in scope` — confirm the `use crate::parser::ast::{...};` in `pipeline.rs` includes both.

- [ ] **Step 4: Move pipeline tests to `pipeline.rs`**

The pipeline tests in `mod.rs::tests` are the ones that exercise `expand_word_to_fields` directly (not `expand_word`/`expand_word_to_string`):
- `test_unquoted_dollar_at_splits_per_param` (lines ~801-820)
- `test_unquoted_dollar_at_empty_produces_nothing` (lines ~822-834)

Append to `src/expand/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

    #[test]
    fn test_unquoted_dollar_at_splits_per_param() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 3, "expected 3 fields, got {:?}", fields);
        assert_eq!(fields[0].value, "a");
        assert_eq!(fields[1].value, "b");
        assert_eq!(fields[2].value, "c");
        assert!((0..fields[0].value.len()).all(|i| !fields[0].is_quoted(i)));
        assert!((0..fields[1].value.len()).all(|i| !fields[1].is_quoted(i)));
        assert!((0..fields[2].value.len()).all(|i| !fields[2].is_quoted(i)));
    }

    #[test]
    fn test_unquoted_dollar_at_empty_produces_nothing() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert!(fields.len() <= 1, "expected 0 or 1 fields, got {:?}", fields);
    }
}
```

Delete the same two tests from `mod.rs::tests`. The remaining `expand_word`-level tests (`test_dollar_at_in_double_quotes_splits`, `test_dollar_at_empty_params_produces_nothing`, `test_dollar_star_in_double_quotes_joins`, etc.) **stay in `mod.rs::tests`** because they exercise the public API.

- [ ] **Step 5: Run pipeline tests + full suite**

Run: `cargo test expand::pipeline 2>&1 | tail -10`
Expected: 2 tests PASS.

Run: `cargo test 2>&1 | tail -5`
Expected: full count, zero FAILs.

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All tests passed.

- [ ] **Step 6: Commit**

```bash
git add src/expand/pipeline.rs src/expand/mod.rs
git commit -m "$(cat <<'EOF'
refactor(expand): extract pipeline functions to pipeline.rs

Move expand_word_to_fields (now pub(super)), expand_part_to_fields,
expand_param_to_fields, and ifs_first_char from src/expand/mod.rs into
a new src/expand/pipeline.rs module. Decomposition of expand_part_to_fields
into per-variant helpers is deferred to the next commit (Task C2) to keep
relocation and refactor in separate, bisectable changes.

Two pipeline-level tests follow the move; integration-level tests
exercising the public expand_word/expand_word_to_string API remain in
mod.rs::tests.

Part of SP4 PR-C (relocation half).
EOF
)"
```

---

## Task C2: Decompose `expand_part_to_fields` into 7 Per-Variant Helpers

This is the spec's central redesign: the 10-arm `match` collapses into a thin dispatcher delegating to 7 helpers. Three of the variants (`EscapedLiteral`, `SingleQuoted`, `DollarSingleQuoted`) all share one helper since their bodies are byte-identical.

**Files:**
- Modify: `src/expand/pipeline.rs`

- [ ] **Step 1: Replace `expand_part_to_fields` body with the dispatcher and helpers**

In `src/expand/pipeline.rs`, locate the current `fn expand_part_to_fields(env, part, fields, in_double_quote) -> Result<()>`. Replace its 10-arm `match` body and add the 7 helper functions immediately after. The result:

```rust
/// Expand one `WordPart`, appending into `fields`.
/// `in_double_quote` is true when we are inside `DoubleQuoted(...)`.
fn expand_part_to_fields(
    env: &mut ShellEnv,
    part: &WordPart,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match part {
        WordPart::Literal(s) => expand_part_literal(s, fields, in_double_quote),
        WordPart::EscapedLiteral(s)
        | WordPart::SingleQuoted(s)
        | WordPart::DollarSingleQuoted(s) => expand_part_quoted_literal(s, fields),
        WordPart::DoubleQuoted(parts) => expand_part_double_quoted(env, parts, fields)?,
        WordPart::Tilde(user) => expand_part_tilde(env, user.as_deref(), fields),
        WordPart::Parameter(p) => expand_part_parameter(env, p, fields, in_double_quote)?,
        WordPart::CommandSub(program) => expand_part_command_sub(env, program, fields, in_double_quote),
        WordPart::ArithSub(expr) => expand_part_arith_sub(env, expr, fields, in_double_quote)?,
    }
    Ok(())
}

fn expand_part_literal(s: &str, fields: &mut Vec<ExpandedField>, in_double_quote: bool) {
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(s);
    } else {
        fields.last_mut().unwrap().push_unquoted(s);
    }
}

/// `EscapedLiteral`, `SingleQuoted`, and `DollarSingleQuoted` all push their
/// text as quoted (protected from field splitting and pathname expansion).
/// They differ only in their parser-level meaning, not their expansion behavior.
fn expand_part_quoted_literal(s: &str, fields: &mut Vec<ExpandedField>) {
    fields.last_mut().unwrap().push_quoted(s);
}

fn expand_part_double_quoted(
    env: &mut ShellEnv,
    parts: &[WordPart],
    fields: &mut Vec<ExpandedField>,
) -> crate::error::Result<()> {
    fields.last_mut().unwrap().was_quoted = true;
    for inner in parts {
        expand_part_to_fields(env, inner, fields, true)?;
    }
    Ok(())
}

fn expand_part_tilde(env: &mut ShellEnv, user: Option<&str>, fields: &mut Vec<ExpandedField>) {
    let result = match user {
        None => env
            .vars
            .get("HOME")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "~".to_string()),
        Some(name) => super::tilde::expand_tilde_user(name),
    };
    fields.last_mut().unwrap().push_quoted(&result);
}

fn expand_part_parameter(
    env: &mut ShellEnv,
    param: &ParamExpr,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    expand_param_to_fields(env, param, fields, in_double_quote)
}

fn expand_part_command_sub(
    env: &mut ShellEnv,
    program: &crate::parser::ast::Program,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) {
    let output = command_sub::execute(env, program);
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(&output);
    } else {
        fields.last_mut().unwrap().push_unquoted(&output);
    }
}

fn expand_part_arith_sub(
    env: &mut ShellEnv,
    expr: &str,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match arith::evaluate(env, expr) {
        Ok(result) => {
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&result);
            } else {
                fields.last_mut().unwrap().push_unquoted(&result);
            }
            Ok(())
        }
        Err(msg) => Err(crate::error::ShellError::expansion(
            crate::error::ExpansionErrorKind::InvalidArithmetic,
            msg,
        )),
    }
}
```

The `expand_part_parameter` helper is a thin wrapper around the existing private `expand_param_to_fields`. It exists for symmetry with the other helpers (so the dispatcher reads uniformly).

The `Program` type used by `expand_part_command_sub` lives at `crate::parser::ast::Program`. If it is not already in `pipeline.rs`'s `use` line, the inline `crate::parser::ast::Program` path keeps it explicit without needing to extend the import.

- [ ] **Step 2: Run `cargo build` to verify the dispatch compiles**

Run: `cargo build 2>&1 | tail -15`
Expected: clean build.

Common errors:
- `expected enum WordPart::Tilde, found ...` — confirm `Tilde(user)` matches the actual variant `Tilde(Option<String>)` (verify via `grep -n "Tilde" src/parser/ast.rs`). The `user.as_deref()` converts `&Option<String>` → `Option<&str>`.
- `mismatched types: expected Result<(), ShellError>, found ()` — `expand_part_command_sub` and `expand_part_literal` / `expand_part_quoted_literal` / `expand_part_tilde` return `()`. The dispatcher must use `?` only on the helpers that return `Result`, and bare-call the ones that return `()`. Re-check the dispatcher arms.

- [ ] **Step 3: Run unit tests + integration tests**

Run: `cargo test 2>&1 | tail -10`
Expected: full count, zero FAILs. **The decomposition is bit-for-bit equivalent** — every existing test that exercised `expand_part_to_fields` indirectly (via `expand_word`, `expand_word_to_string`, `expand_word_to_fields`) must still pass.

If any test fails, the decomposition has a behavioral diff. Check each helper against the original `match` arm in commit `<previous>`'s `mod.rs:359-443`. The most likely culprits:
- `expand_part_double_quoted` forgetting `was_quoted = true` on the empty-parts case.
- `expand_part_tilde` using `unwrap_or_else(|| "~".to_string())` vs the original branching on `home_dir`.
- `expand_part_arith_sub` losing the `Err` branch's error wrapping.

- [ ] **Step 4: Run E2E**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All tests passed. **Word expansion is on every command's runtime path** — any E2E regression here is severe.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -30`
Expected: only the two pre-existing `doc_lazy_continuation` errors. New warnings about unused parameters or shadowed bindings must be fixed (typically by reading the actual offending helper).

- [ ] **Step 6: Run fmt**

Run: `cargo fmt --check 2>&1`
Expected: no output. If anything, `cargo fmt` and amend.

- [ ] **Step 7: Commit**

```bash
git add src/expand/pipeline.rs
git commit -m "$(cat <<'EOF'
refactor(expand): decompose expand_part_to_fields into 7 per-variant helpers

Replace the 85-line, 10-arm match in expand_part_to_fields with a 25-line
dispatcher delegating to seven private helpers in pipeline.rs:
  - expand_part_literal (in_double_quote-aware)
  - expand_part_quoted_literal (EscapedLiteral / SingleQuoted /
    DollarSingleQuoted — all push_quoted)
  - expand_part_double_quoted (sets was_quoted, recurses)
  - expand_part_tilde (Option<&str> for ~ vs ~user)
  - expand_part_parameter (delegates to expand_param_to_fields)
  - expand_part_command_sub
  - expand_part_arith_sub

Each helper has a single responsibility and its current behavior is
byte-for-byte equivalent to the original match arm. Integration tests
in mod.rs::tests provide the regression safety net.

Part of SP4 PR-C (decomposition half).
EOF
)"
```

---

## Task C3: Bench Verification (±5% Threshold) and Final mod.rs Cleanup

Confirm the decomposition didn't regress expand throughput and clean up any leftovers in `mod.rs`.

**Files:**
- Read-only bench output
- Modify: `src/expand/mod.rs` (final tidying — remove orphan section dividers, ensure `pub use`/`mod` ordering matches the spec's facade shape)

- [ ] **Step 1: Run `cargo bench --bench expand_bench` post-decomposition**

Run: `cargo bench --bench expand_bench 2>&1 | tee /tmp/sp4-bench-pr-c.txt | tail -50`
Expected: criterion produces per-benchmark `time:   [...]` lines. Save the file.

- [ ] **Step 2: Compare against PR-B baseline**

Diff the two outputs. For each benchmark, confirm the post-decomposition mean is within ±5% of the PR-B mean. Criterion auto-reports as `change: [...% ...% ...%]` if both runs share the same target name.

If any benchmark exceeds +5%, the helper decomposition introduced an inlining or branch-prediction regression. Investigate before continuing — common causes:
- `expand_part_parameter` adding a function-call layer where the original code inlined into the match arm. (The helper compiles to the same code at `-O`, but check `cargo bench --release` — debug builds have layered call overhead.)
- `expand_part_tilde`'s `Option<&str>` matching producing different codegen than the inline `Tilde(None)` / `Tilde(Some)` arms.

If +5% is exceeded, revisit Task C2's helpers — collapse `expand_part_parameter` to inline-call `expand_param_to_fields` directly from the dispatcher rather than through a pass-through wrapper.

- [ ] **Step 3: Verify `mod.rs` final shape**

Run: `wc -l src/expand/mod.rs`
Expected: ≤ 350 lines (production + remaining tests). The umbrella DoD #6 caps production at 400.

Read `src/expand/mod.rs` end-to-end. It should contain (in order):
1. `pub mod` declarations for the 6 untouched submodules (`pattern`, `param`, `command_sub`, `pathname`, `field_split`, `arith`).
2. `mod` declarations for the 4 new submodules (`scan`, `tilde`, `heredoc`, `pipeline`).
3. `pub(crate) use` re-exports for `scan` and `tilde` items.
4. `pub use heredoc::expand_body as expand_heredoc_body;`.
5. `use pipeline::expand_word_to_fields;` (private import).
6. `use crate::env::ShellEnv;` and `use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};` — keep only the imports still needed by `mod.rs` itself (the public-API functions and remaining tests). Remove any now-orphan imports (`SpecialParam` may no longer be needed if no test uses it).
7. `pub struct ExpandedField` + `impl` blocks (unchanged).
8. `pub fn expand_word`, `pub fn expand_words`, `pub fn expand_word_to_string` (unchanged).
9. `#[cfg(test)] mod tests` with the integration-level tests that remain.

Remove any orphan section-divider comments left from earlier moves (e.g., `// ─── Stage 1: ...`, `// ─── Tests ───`).

- [ ] **Step 4: Run full `cargo test`**

Run: `cargo test 2>&1 | tail -10`
Expected: full count, zero FAILs.

- [ ] **Step 5: Run E2E**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All tests passed.

- [ ] **Step 6: Run clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -10`
Expected: only the two pre-existing `doc_lazy_continuation` errors.

Run: `cargo fmt --check 2>&1`
Expected: no output.

- [ ] **Step 7: Commit (only if mod.rs was modified in Step 3)**

```bash
git add src/expand/mod.rs
git commit -m "$(cat <<'EOF'
refactor(expand): clean up mod.rs facade after pipeline extraction

Remove orphan section dividers and tighten use statements so mod.rs
contains only the canonical ExpandedField type, three public entry
points (expand_word/expand_words/expand_word_to_string), and the
integration-level tests that exercise them. All implementation has
moved to scan/tilde/heredoc/pipeline submodules.

Part of SP4 PR-C (mod.rs facade cleanup).
EOF
)"
```

If `mod.rs` was already clean after Task C1+C2, skip this commit.

---

## Task D1: Final Verification + Documentation Cleanup

Confirm the umbrella DoD holds end-to-end and update any remaining documentation references.

**Files:**
- Read-only verification + targeted doc edits

- [ ] **Step 1: Final full `cargo test` (3 consecutive runs for non-flake)**

```bash
for i in 1 2 3; do
  echo "=== Run $i ===" >&2
  cargo test 2>&1 | tail -3
done
```

Expected: three consecutive PASS results. Any flake (e.g., a test sometimes failing) must be investigated — `tests/pty_interactive.rs` is the most likely culprit per CLAUDE.md.

- [ ] **Step 2: Final E2E (3 consecutive runs)**

```bash
for i in 1 2 3; do
  echo "=== E2E Run $i ===" >&2
  ./e2e/run_tests.sh 2>&1 | tail -3
done
```

Expected: three consecutive `All tests passed` results.

- [ ] **Step 3: `cargo bench --no-run`**

Run: `cargo bench --no-run 2>&1 | tail -5`
Expected: clean compile of all bench targets. (Full bench run already happened in Task C3.)

- [ ] **Step 4: `cargo clippy --all-targets -- -D warnings`**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -30`
Expected: only the two pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99`.

- [ ] **Step 5: `cargo fmt --check`**

Run: `cargo fmt --check 2>&1`
Expected: no output.

- [ ] **Step 6: Production line-count verification**

Run: `wc -l src/expand/*.rs`
Expected: every file ≤ 400 production lines (test lines exempt). Concretely:
- `mod.rs` ≤ 350 (mostly tests + facade)
- `pipeline.rs` ≤ 400 (decomposed dispatcher + 7 helpers + `expand_param_to_fields` + `ifs_first_char` + tests)
- `heredoc.rs` ≤ 280
- `scan.rs` ≤ 250
- `tilde.rs` ≤ 110

If `pipeline.rs` exceeds 400 production lines, split tests into a sibling `pipeline/tests.rs` or move integration tests up to `mod.rs::tests`. Document the choice in the commit message.

- [ ] **Step 7: Public-API caller diff**

Run: `grep -rn "crate::expand::\|yosh::expand::" src/ tests/ benches/ 2>/dev/null | grep -v "//" | sort -u > /tmp/sp4-callers-after-D.txt && diff /tmp/sp4-callers-before.txt /tmp/sp4-callers-after-D.txt`
Expected: zero diff. If non-empty, an external caller signature changed — this violates DoD #8 and must be reverted before claiming completion.

- [ ] **Step 8: Verify TODO.md / CLAUDE.md / README references**

Run: `grep -rn "src/expand/mod.rs" README.md CLAUDE.md TODO.md docs/ 2>/dev/null`
Expected: any remaining references either point to specific functions still present in `mod.rs` (e.g., `ExpandedField`, `expand_word`) or describe the file as the entry point. If a reference now points to relocated content (e.g., "see `expand/mod.rs:NNN` for `expand_heredoc_body`"), update it to the new path (e.g., `src/expand/heredoc.rs`).

The most likely stale references:
- `TODO.md` line ~71 — already removed in Task A2.
- Any `docs/superpowers/specs/*.md` mentioning `src/expand/mod.rs:NNN` — these are historical specs, leave unchanged.
- `CLAUDE.md` mentions `src/expand/` as a pipeline stage — this stays valid.

- [ ] **Step 9: PTY tests**

Run: `cargo test --test pty_interactive 2>&1 | tail -10`
Expected: PASS. Re-run twice more if flaky per CLAUDE.md guidance.

- [ ] **Step 10: Final commit (only if Step 8 produced documentation edits)**

```bash
git add README.md CLAUDE.md TODO.md docs/
git commit -m "$(cat <<'EOF'
docs(sp4): update file references after expand/ module split

Repoint stale src/expand/mod.rs:NNN references to their new homes:
- expand_heredoc_body → src/expand/heredoc.rs
- expand_tilde_* → src/expand/tilde.rs
- skip_balanced_* → src/expand/scan.rs
- expand_word_to_fields → src/expand/pipeline.rs

Part of SP4 PR-C (final cleanup).
EOF
)"
```

If Step 8 found nothing to update, skip this commit.

- [ ] **Step 11: Verify the SP4 commit chain**

Run: `git log --oneline | head -10`
Expected to see (in reverse-chron order, after the spec-correction commit `fcc7eff`):
- `refactor(expand): clean up mod.rs facade after pipeline extraction` (optional)
- `refactor(expand): decompose expand_part_to_fields into 7 per-variant helpers`
- `refactor(expand): extract pipeline functions to pipeline.rs`
- `refactor(expand): extract expand_heredoc_* to heredoc.rs`
- `refactor(expand): extract expand_tilde_prefix/expand_tilde_user to tilde.rs`
- `test(expand): relocate scan tests to scan.rs and cover unterminated input`
- `refactor(expand): extract skip_balanced_* scanners to scan.rs`
- `docs(sp4): correct per-variant helper decomposition in SP4 spec`

Confirm the chain matches. If a commit is missing, the corresponding task did not run — re-check the task list.

---

## Risk Mitigation Notes

**Performance regression in PR-C decomposition:** The 7-helper split introduces no allocations (every helper writes into the shared `&mut Vec<ExpandedField>`), but the dispatcher's match-then-call layout differs from the original inline match. Release-mode inlining should be identical; Task C3's bench gate catches any divergence.

**Bit-for-bit behavior preservation:** Each helper must produce the same `ExpandedField` sequence as the original match arm. The integration-level tests in `mod.rs::tests` (`test_dollar_at_in_double_quotes_splits`, `test_dollar_star_in_double_quotes_joins`, `test_double_quoted_literal`, etc.) are the safety net. If any of those fails after Task C2, the decomposition has a defect — read the failing test's input and trace it through the helper sequence.

**Cross-module imports after each PR:** PR-A leaves `mod.rs::expand_heredoc_string` calling bare `skip_balanced_*` names — this works because of the `pub(crate) use scan::{...};` re-export. PR-B replaces those calls (via the move) with `super::scan::skip_balanced_*` paths inside `heredoc.rs`. PR-C leaves the helpers in `pipeline.rs` calling `super::tilde::expand_tilde_user` etc. The cross-module path consistency is verified by `cargo build` at every commit.

**`expand_part_parameter` indirection cost:** The helper exists for dispatcher uniformity but adds a function-call layer over the existing private `expand_param_to_fields`. If Task C3's bench gate flags a regression, replace `WordPart::Parameter(p) => expand_part_parameter(env, p, fields, in_double_quote)?,` in the dispatcher with `WordPart::Parameter(p) => expand_param_to_fields(env, p, fields, in_double_quote)?,` directly, and delete the `expand_part_parameter` helper. Update the spec retrospectively (one-line edit).

**Spec re-export over-promise:** The spec's "Public API Compatibility" section lists `expand_tilde_user`, `skip_balanced_braces`, and `skip_balanced_double_parens` as preserved public-path symbols. None of them have any external (cross-crate or test-suite) caller today. The `pub(crate) use` re-exports preserve them defensively at zero cost — no need to add tests verifying the re-export path resolves.

---

## Plan Self-Review

**Spec coverage:**
- "Heredoc as an Independent Module" → Task B1.
- "Pipeline Module — Per-Variant Helpers" → Task C1 (relocation) + Task C2 (decomposition).
- "Scan Helpers Module + TODO Cleanup" → Task A1 (extraction) + Task A2 (tests + TODO removal).
- "Tilde Module" → Task A3.
- "`mod.rs` Final Shape" → Task C3 Step 3 + Task D1 Step 6.
- "Test Reorganization" table → distributed across Task A2/A3, B1, C1.
- "PR Breakdown" three PRs → Tasks A* (PR-A), Tasks B* (PR-B), Tasks C* (PR-C).
- DoD items #1-#11 → Task D1 covers each.

**Placeholder scan:** No `TBD`, `TODO`, "implement later", or generic "add error handling" instructions. Every code block is complete. Every command has expected output.

**Type consistency:** Helper signatures in Task C2 match the file-structure section's signatures verbatim. `ExpandedField` is referenced as `super::ExpandedField` from `pipeline.rs` and as bare `ExpandedField` inside `mod.rs` — both correct given `mod.rs`'s definition. `WordPart::Tilde(user)` destructures consistently as `Option<String>` with `.as_deref()` to get `Option<&str>` for the helper.
