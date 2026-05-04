# Parser Visibility Tightening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Demote 26 `pub fn` methods on `Parser` to `pub(super) fn`, demote one `pub(crate) fn` to `pub(super) fn`, and remove a stale `#[allow(dead_code)]` — all in a single commit, after a callsite audit confirmed no external callers will break.

**Architecture:** Mechanical 28-line edit across 6 files in `src/parser/`. Each edit replaces a single visibility token (`pub` → `pub(super)` or `pub(crate)` → `pub(super)`) without touching `fn` body, signature, parameter list, or return type. The `Parser` struct itself and 10 methods that are reached from binaries, benches, or other yosh-internal crates remain `pub`. After all edits land in the working tree and verification commands pass, the engineer creates a single commit.

**Tech Stack:** Rust 2024 edition, `cargo`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-05-parser-visibility-tightening-design.md`

---

## File Structure

Six existing files in `src/parser/` are edited; nothing is created or deleted.

| File                          | Edits                                                                |
|-------------------------------|----------------------------------------------------------------------|
| `src/parser/mod.rs`           | 11 `pub fn` → `pub(super) fn` + 1 `#[allow(dead_code)]` removal      |
| `src/parser/word.rs`          | 1 `pub fn` → `pub(super) fn` + 1 `pub(crate) fn` → `pub(super) fn`   |
| `src/parser/function.rs`      | 1 `pub fn` → `pub(super) fn`                                         |
| `src/parser/redirect.rs`      | 2 `pub fn` → `pub(super) fn`                                         |
| `src/parser/simple.rs`        | 1 `pub fn` → `pub(super) fn`                                         |
| `src/parser/compound.rs`      | 10 `pub fn` → `pub(super) fn`                                        |

Total: **28 edits**.

---

## Task 0: Capture Baseline

**Files:** No file changes.

- [ ] **Step 1: Capture parser test count**

Run:
```bash
cargo test --lib parser:: 2>&1 | grep "test result" | head -1
```

Expected output (the first non-empty `test result` line):
```
test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 679 filtered out; finished in ...
```

Record: **99 passed**.

- [ ] **Step 2: Capture full lib test count**

Run:
```bash
cargo test --lib 2>&1 | grep -E "^test result" | head -1
```

Expected: `99 passed` for the parser-related portion; the full lib total is around 903 across the workspace's lib targets.

Record: **903 passed total** (sum across `yosh`, `yosh_plugin_api`, `yosh_plugin_manager`, `yosh_plugin_sdk`).

- [ ] **Step 3: Confirm pre-edit visibility count**

Run:
```bash
grep -c '^    pub fn ' src/parser/mod.rs src/parser/word.rs src/parser/function.rs src/parser/redirect.rs src/parser/simple.rs src/parser/compound.rs
```

Expected per-file counts (totals 36):
- `src/parser/mod.rs:20`
- `src/parser/word.rs:1`
- `src/parser/function.rs:1`
- `src/parser/redirect.rs:2`
- `src/parser/simple.rs:2`
- `src/parser/compound.rs:10`

This task does **not** produce a commit.

---

## Task 1: Edit `mod.rs` (11 demotions + 1 attribute removal)

**Files:**
- Modify: `src/parser/mod.rs`

This file gets the most edits. Each method is identified by its full signature line so the `Edit` operations are unambiguous.

- [ ] **Step 1: Demote `current_span`**

Edit `src/parser/mod.rs`. Replace exactly one occurrence:

old:
```rust
    pub fn current_span(&self) -> Span {
```

new:
```rust
    pub(super) fn current_span(&self) -> Span {
```

- [ ] **Step 2: Remove stale `#[allow(dead_code)]` on `current_token`**

Edit `src/parser/mod.rs`. Replace exactly one occurrence:

old:
```rust

    #[allow(dead_code)]
    pub fn current_token(&self) -> &Token {
```

new:
```rust

    pub fn current_token(&self) -> &Token {
```

(Removes one `#[allow(dead_code)]` line. `current_token` itself stays `pub` because `interactive/parse_status.rs:61` calls it.)

- [ ] **Step 3: Demote `eat`**

Edit `src/parser/mod.rs`. Replace exactly one occurrence:

old:
```rust
    pub fn eat(&mut self, expected: &Token) -> error::Result<bool> {
```

new:
```rust
    pub(super) fn eat(&mut self, expected: &Token) -> error::Result<bool> {
```

- [ ] **Step 4: Demote `expect_reserved`**

old:
```rust
    pub fn expect_reserved(&mut self, keyword: &str) -> error::Result<()> {
```

new:
```rust
    pub(super) fn expect_reserved(&mut self, keyword: &str) -> error::Result<()> {
```

- [ ] **Step 5: Demote `skip_newlines`**

old:
```rust
    pub fn skip_newlines(&mut self) -> error::Result<()> {
```

new:
```rust
    pub(super) fn skip_newlines(&mut self) -> error::Result<()> {
```

- [ ] **Step 6: Demote `is_reserved`**

old:
```rust
    pub fn is_reserved(&self, keyword: &str) -> bool {
```

new:
```rust
    pub(super) fn is_reserved(&self, keyword: &str) -> bool {
```

- [ ] **Step 7: Demote `parse_separator_op`**

old:
```rust
    pub fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
```

new:
```rust
    pub(super) fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
```

- [ ] **Step 8: Demote `parse_and_or`**

old:
```rust
    pub fn parse_and_or(&mut self) -> error::Result<AndOrList> {
```

new:
```rust
    pub(super) fn parse_and_or(&mut self) -> error::Result<AndOrList> {
```

- [ ] **Step 9: Demote `parse_pipeline`**

old:
```rust
    pub fn parse_pipeline(&mut self) -> error::Result<Pipeline> {
```

new:
```rust
    pub(super) fn parse_pipeline(&mut self) -> error::Result<Pipeline> {
```

- [ ] **Step 10: Demote `parse_command`**

old:
```rust
    pub fn parse_command(&mut self) -> error::Result<Command> {
```

new:
```rust
    pub(super) fn parse_command(&mut self) -> error::Result<Command> {
```

- [ ] **Step 11: Demote `is_complete_command_end`**

old:
```rust
    pub fn is_complete_command_end(&self) -> bool {
```

new:
```rust
    pub(super) fn is_complete_command_end(&self) -> bool {
```

- [ ] **Step 12: Demote `is_compound_command_start`**

old:
```rust
    pub fn is_compound_command_start(&self) -> bool {
```

new:
```rust
    pub(super) fn is_compound_command_start(&self) -> bool {
```

- [ ] **Step 13: Verify build**

Run:
```bash
cargo build -p yosh
```

Expected: success, no errors. If `error[E0624]: ... is private` appears, an external caller was missed in the audit; restore that specific method to `pub` and report.

This task does **not** produce a commit.

---

## Task 2: Edit `word.rs` (1 method + 1 free fn)

**Files:**
- Modify: `src/parser/word.rs`

- [ ] **Step 1: Demote `expect_word`**

Edit `src/parser/word.rs`. Replace exactly one occurrence:

old:
```rust
    pub fn expect_word(&mut self, context: &str) -> error::Result<Word> {
```

new:
```rust
    pub(super) fn expect_word(&mut self, context: &str) -> error::Result<Word> {
```

- [ ] **Step 2: Demote `split_tildes_in_literal`**

Edit `src/parser/word.rs`. Replace exactly one occurrence:

old:
```rust
pub(crate) fn split_tildes_in_literal(
```

new:
```rust
pub(super) fn split_tildes_in_literal(
```

(Note: `split_tildes_in_literal` is a free function at module scope, not an `impl Parser` method. Indentation differs from the inherent-method examples above.)

- [ ] **Step 3: Verify build**

Run:
```bash
cargo build -p yosh
```

Expected: success.

This task does **not** produce a commit.

---

## Task 3: Edit `function.rs` (1 demotion)

**Files:**
- Modify: `src/parser/function.rs`

- [ ] **Step 1: Demote `try_parse_function_def`**

Edit `src/parser/function.rs`. Replace exactly one occurrence:

old:
```rust
    pub fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
```

new:
```rust
    pub(super) fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
```

- [ ] **Step 2: Verify build**

Run:
```bash
cargo build -p yosh
```

Expected: success.

This task does **not** produce a commit.

---

## Task 4: Edit `redirect.rs` (2 demotions)

**Files:**
- Modify: `src/parser/redirect.rs`

- [ ] **Step 1: Demote `try_parse_redirect`**

Edit `src/parser/redirect.rs`. Replace exactly one occurrence:

old:
```rust
    pub fn try_parse_redirect(&mut self) -> error::Result<Option<Redirect>> {
```

new:
```rust
    pub(super) fn try_parse_redirect(&mut self) -> error::Result<Option<Redirect>> {
```

- [ ] **Step 2: Demote `parse_redirect_list`**

old:
```rust
    pub fn parse_redirect_list(&mut self) -> error::Result<Vec<Redirect>> {
```

new:
```rust
    pub(super) fn parse_redirect_list(&mut self) -> error::Result<Vec<Redirect>> {
```

- [ ] **Step 3: Verify build**

Run:
```bash
cargo build -p yosh
```

Expected: success.

This task does **not** produce a commit.

---

## Task 5: Edit `simple.rs` (1 demotion)

**Files:**
- Modify: `src/parser/simple.rs`

Note: `simple.rs` contains two `pub fn` items: `parse_simple_command` (demoted) and `try_parse_assignment` (stays `pub` because `src/exec/simple.rs:33` calls it). Do **not** demote `try_parse_assignment`.

- [ ] **Step 1: Demote `parse_simple_command`**

Edit `src/parser/simple.rs`. Replace exactly one occurrence:

old:
```rust
    pub fn parse_simple_command(&mut self) -> error::Result<SimpleCommand> {
```

new:
```rust
    pub(super) fn parse_simple_command(&mut self) -> error::Result<SimpleCommand> {
```

- [ ] **Step 2: Verify build**

Run:
```bash
cargo build -p yosh
```

Expected: success.

This task does **not** produce a commit.

---

## Task 6: Edit `compound.rs` (10 demotions)

**Files:**
- Modify: `src/parser/compound.rs`

- [ ] **Step 1: Demote `parse_compound_command`**

old:
```rust
    pub fn parse_compound_command(&mut self) -> error::Result<CompoundCommand> {
```

new:
```rust
    pub(super) fn parse_compound_command(&mut self) -> error::Result<CompoundCommand> {
```

- [ ] **Step 2: Demote `parse_compound_list`**

old:
```rust
    pub fn parse_compound_list(&mut self, context: &str) -> error::Result<Vec<CompleteCommand>> {
```

new:
```rust
    pub(super) fn parse_compound_list(&mut self, context: &str) -> error::Result<Vec<CompleteCommand>> {
```

- [ ] **Step 3: Demote `parse_if_clause`**

old:
```rust
    pub fn parse_if_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_if_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 4: Demote `parse_for_clause`**

old:
```rust
    pub fn parse_for_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_for_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 5: Demote `parse_do_group`**

old:
```rust
    pub fn parse_do_group(&mut self) -> error::Result<Vec<CompleteCommand>> {
```

new:
```rust
    pub(super) fn parse_do_group(&mut self) -> error::Result<Vec<CompleteCommand>> {
```

- [ ] **Step 6: Demote `parse_while_clause`**

old:
```rust
    pub fn parse_while_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_while_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 7: Demote `parse_until_clause`**

old:
```rust
    pub fn parse_until_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_until_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 8: Demote `parse_case_clause`**

old:
```rust
    pub fn parse_case_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_case_clause(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 9: Demote `parse_brace_group`**

old:
```rust
    pub fn parse_brace_group(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_brace_group(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 10: Demote `parse_subshell`**

old:
```rust
    pub fn parse_subshell(&mut self) -> error::Result<CompoundCommandKind> {
```

new:
```rust
    pub(super) fn parse_subshell(&mut self) -> error::Result<CompoundCommandKind> {
```

- [ ] **Step 11: Verify build**

Run:
```bash
cargo build -p yosh
```

Expected: success.

This task does **not** produce a commit.

---

## Task 7: Final Verification & Single Commit

**Files:**
- Read-only verification, then commit all 6 modified parser files.

- [ ] **Step 1: Verify post-edit visibility count**

Run:
```bash
grep -c '^    pub fn ' src/parser/mod.rs src/parser/word.rs src/parser/function.rs src/parser/redirect.rs src/parser/simple.rs src/parser/compound.rs
```

Expected per-file counts (totals 10):
- `src/parser/mod.rs:9` (was 20, demoted 11)
- `src/parser/word.rs:0` (was 1, demoted 1)
- `src/parser/function.rs:0` (was 1, demoted 1)
- `src/parser/redirect.rs:0` (was 2, demoted 2)
- `src/parser/simple.rs:1` (was 2, demoted 1, `try_parse_assignment` stays `pub`)
- `src/parser/compound.rs:0` (was 10, demoted 10)

If counts don't match, an Edit was missed; re-run the matrix in the spec to find the gap.

- [ ] **Step 2: Verify `pub(super) fn` count grew by 27**

Run:
```bash
grep -c '^    pub(super) fn \|^pub(super) fn ' src/parser/mod.rs src/parser/word.rs src/parser/function.rs src/parser/redirect.rs src/parser/simple.rs src/parser/compound.rs
```

Expected combined total: 27 new `pub(super)` items beyond whatever was already there (Task 1 of the prior split added a few). The exact per-file count is less important than confirming all 27 demotions stuck.

- [ ] **Step 3: Verify `current_token`'s `#[allow(dead_code)]` is gone**

Run:
```bash
grep -B1 'pub fn current_token' src/parser/mod.rs
```

Expected: shows the line preceding `pub fn current_token(...)` is **not** `#[allow(dead_code)]` (typically a blank line or the closing brace of the previous fn). If `#[allow(dead_code)]` still appears, Task 1 Step 2 didn't apply; redo it.

- [ ] **Step 4: Full build (all targets including bins and benches)**

Run:
```bash
cargo build
```

Expected: success. This step is critical because `cargo build -p yosh` (used in tasks 1–6) builds only the `yosh` library, while plain `cargo build` also builds binaries (`main.rs`, `bin/yosh-dhat.rs`) and benchmarks (`benches/*.rs`). Binaries and benches consume the library through its public API, so any over-tightened method that a bin/bench uses will fail here even if the lib build passed.

If `cargo build` fails with `error[E0603]: function ... is private` or `error[E0624]: ... method ... is private`, the failing line points at the bin or bench callsite. Restore the offending method to `pub` and report.

- [ ] **Step 5: Run parser tests**

Run:
```bash
cargo test --lib parser::
```

Expected: `test result: ok. 99 passed; 0 failed` (matching the Task 0 baseline).

- [ ] **Step 6: Run full lib tests**

Run:
```bash
cargo test --lib
```

Expected: total 903 passed across all lib targets, matching the Task 0 baseline.

- [ ] **Step 7: Check rustfmt for parser/**

Run:
```bash
cargo fmt --check -- src/parser/mod.rs src/parser/word.rs src/parser/function.rs src/parser/redirect.rs src/parser/simple.rs src/parser/compound.rs
```

Expected: no output (fmt-clean).

If rustfmt reports drift, run `cargo fmt -- src/parser/*.rs` and add the formatting changes to the same commit.

- [ ] **Step 8: Check clippy on lib (parser-scoped)**

Run:
```bash
cargo clippy --lib -- -D warnings 2>&1 | grep -A2 'parser/' || echo "no parser/* lints"
```

Expected: `no parser/* lints`. The pre-existing `clippy::doc_lazy_continuation` warnings in `src/plugin/mod.rs` (not parser) are unrelated to this task.

- [ ] **Step 9: (optional) E2E suite**

Run:
```bash
./e2e/run_tests.sh
```

Expected: `393/393 passed`. May take ~6–7 minutes. Skip if time-pressed; the lib + clippy steps already cover compile-time visibility correctness, and E2E tests are robust against visibility-only changes.

- [ ] **Step 10: Commit**

Run:
```bash
git add src/parser/mod.rs src/parser/word.rs src/parser/function.rs src/parser/redirect.rs src/parser/simple.rs src/parser/compound.rs
git commit -m "$(cat <<'EOF'
refactor(parser): tighten visibility of internal-only methods

Drops 26 `pub fn` to `pub(super) fn` and the free fn
`split_tildes_in_literal` from `pub(crate)` to `pub(super)`,
based on a callsite audit showing no external callers (binary,
bench, or other-crate). Also removes a stale `#[allow(dead_code)]`
on `current_token`, which is reachable via parse_status.rs.

API surface reduction: 36 → 10 `pub fn` on Parser (72%).

Spec: docs/superpowers/specs/2026-05-05-parser-visibility-tightening-design.md
Original prompt: parser/mod.rs split の follow-up — Parser API の可視性整理。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 11: Verify commit landed**

Run:
```bash
git log --oneline -1
git diff HEAD~1 HEAD --stat
```

Expected:
- New commit on top of `88af469` (the spec commit).
- Diff stat: 6 files changed; insertions and deletions roughly balanced (each demotion is +1 line / -1 line; the `#[allow(dead_code)]` removal is -1 line; net `~ -1` across the commit).

---

## Rollback Conditions

`git revert <commit>` if any of the following happen *after* the commit:

- A user reports a binary or bench fails to build that wasn't in the original audit set.
- A subsequent commit needs `Parser::*` access from outside `parser/` and the demotion blocks it (in this case, restore the specific method to `pub` rather than reverting the whole commit).
- `cargo test --lib` count drops below 903.

If the demotion fails *during* Tasks 1–6 (`cargo build -p yosh` errors), the offending method's callsite is in another `parser/*` file — that's normal cross-submodule resolution and `pub(super)` should handle it; the failure indicates the demotion landed correctly but a textual edit went wrong (e.g., an `Edit` matched the wrong line). Re-read the file to confirm the exact pattern before re-applying.

If the demotion fails *during* Task 7 Step 4 (`cargo build` plain), it means a bin or bench (compiled as a separate crate) called the demoted method. Restore that one method to `pub` and re-build. Note the discrepancy in the spec's "Lessons Learned" section before continuing.

---

## Definition of Done

All Definition-of-Done items from the spec hold:

1. `pub fn` count on `Parser` reduced from 36 to 10 (verified by Task 7 Step 1).
2. `split_tildes_in_literal` is now `pub(super) fn` (verified by inspection of `src/parser/word.rs`).
3. `#[allow(dead_code)]` on `current_token` removed (verified by Task 7 Step 3).
4. `cargo build` (all targets) succeeds (Task 7 Step 4).
5. `cargo test --lib` total matches the pre-change baseline (Task 7 Step 6).
6. `cargo fmt --check src/parser/` clean (Task 7 Step 7).
7. `cargo clippy --lib -- -D warnings` produces no new lint inside `src/parser/*` (Task 7 Step 8).
8. E2E suite pass/fail profile unchanged (Task 7 Step 9, optional).
9. A single commit appears in `git log` with the spec-required message format (Task 7 Steps 10–11).
