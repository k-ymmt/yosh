# FD_TEST_LOCK Poisoning Cascade Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent `src/exec/redirect.rs` fd tests from cascade-failing with `PoisonError` when any one fd test panics under `FD_TEST_LOCK`.

**Architecture:** Test-only code change. Replace three `FD_TEST_LOCK.lock().unwrap()` call sites with `FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())`. `PoisonError::into_inner()` returns the `MutexGuard` unchanged, so mutual exclusion is preserved; only the poison state is bypassed. No new tests; spec explicitly scopes tests out because `FD_TEST_LOCK` protects temporal fd exclusion only — it guards no shared data whose corruption would affect other tests.

**Tech Stack:** Rust 2024 edition, `std::sync::Mutex`, cargo test harness.

---

## File Structure

- Modify: `src/exec/redirect.rs` — three call sites inside `#[cfg(test)] mod tests` (lines 262, 297, 329).
- Modify: `TODO.md` — remove completed entry (line 79).

No files created. No files deleted. Scope is surgical.

---

## Task 1: Switch FD_TEST_LOCK acquisitions to poison-tolerant pattern

**Files:**
- Modify: `src/exec/redirect.rs:262,297,329`

- [ ] **Step 1: Baseline — run current fd tests and confirm they pass**

Run:
```bash
cargo test --lib -- exec::redirect::tests
```

Expected: 3 tests pass (`test_redirect_output_and_restore`, `test_redirect_input`, `test_apply_rolls_back_on_second_redirect_failure`). If any fail, stop and investigate — the baseline must be clean before applying the fix.

- [ ] **Step 2: Edit line 262 (`test_redirect_output_and_restore`)**

In `src/exec/redirect.rs`, inside `test_redirect_output_and_restore`, change the first line of the test body:

From:
```rust
        let _guard = FD_TEST_LOCK.lock().unwrap();
```

To:
```rust
        let _guard = FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
```

- [ ] **Step 3: Edit line 297 (`test_redirect_input`)**

In `src/exec/redirect.rs`, inside `test_redirect_input`, change the first line of the test body:

From:
```rust
        let _guard = FD_TEST_LOCK.lock().unwrap();
```

To:
```rust
        let _guard = FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
```

- [ ] **Step 4: Edit line 329 (`test_apply_rolls_back_on_second_redirect_failure`)**

In `src/exec/redirect.rs`, inside `test_apply_rolls_back_on_second_redirect_failure`, change the first line of the test body:

From:
```rust
        let _guard = FD_TEST_LOCK.lock().unwrap();
```

To:
```rust
        let _guard = FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
```

- [ ] **Step 5: Verify fd tests still pass**

Run:
```bash
cargo test --lib -- exec::redirect::tests
```

Expected: same 3 tests pass. Behavior under the happy path is unchanged; the new pattern only diverges from the old when the mutex is poisoned, which baseline tests do not trigger.

- [ ] **Step 6: Verify the full library test suite still passes**

Run:
```bash
cargo test --lib
```

Expected: entire `--lib` suite passes with no new failures. This is a precaution — the change is confined to `#[cfg(test)]` code, so no production test should regress, but running the broader suite confirms nothing adjacent was accidentally affected.

- [ ] **Step 7: Verify rustfmt clean**

Run:
```bash
cargo fmt --check
```

Expected: exit 0, no output. (Note: per TODO.md, `cargo fmt --check -- <path>` has an edition-parsing bug on rustfmt 1.8.0. Using the project-wide form without an explicit path avoids that bug.)

- [ ] **Step 8: Verify clippy clean for the changed file**

Run:
```bash
cargo clippy --lib -- -D warnings
```

Expected: exit 0. Should surface no new lints. `unwrap_or_else(|e| e.into_inner())` is a well-known idiom; clippy has no lint that flags it.

---

## Task 2: Remove completed entry from TODO.md

**Files:**
- Modify: `TODO.md:79`

CLAUDE.md directive: "Delete completed items rather than marking them with `[x]`." This task enforces that rule.

- [ ] **Step 1: Remove the FD_TEST_LOCK line from TODO.md**

Delete this exact line (line 79) from `TODO.md`:

```
- [ ] `FD_TEST_LOCK.lock().unwrap()` lock-poisoning cascade — if a fd-test panics while holding the lock, subsequent fd tests cascade-fail with `PoisonError`. Switch to `.lock().unwrap_or_else(|e| e.into_inner())` to keep failures local to the original panicking test (`src/exec/redirect.rs:258,262,297,329`). Code-review follow-up from 2026-04-20 redirect self-heal.
```

Leave the surrounding `## Future: Code Quality Improvements` section intact; only this one line goes.

- [ ] **Step 2: Confirm the entry is gone**

Run:
```bash
grep -n "FD_TEST_LOCK" TODO.md
```

Expected: no output (exit 1). The symbol should not appear anywhere in TODO.md.

- [ ] **Step 3: Commit both changes together**

Run:
```bash
git add src/exec/redirect.rs TODO.md
git commit -m "$(cat <<'EOF'
test(redirect): recover FD_TEST_LOCK from poisoning to prevent cascade

A panic inside any FD_TEST_LOCK-protected test left the mutex poisoned,
causing subsequent fd tests to fail with PoisonError. The lock guards
only temporal fd exclusion (no shared data), so recovering via
unwrap_or_else(|e| e.into_inner()) is safe and isolates the original
panic to the offending test.

Original prompt: TODO.md の中から優先度が高そうなものを1つ対応してください
Spec: docs/superpowers/specs/2026-04-21-fd-test-lock-poisoning-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds. Pre-commit hooks (if any) pass.

- [ ] **Step 4: Confirm tree is clean**

Run:
```bash
git status
```

Expected: "nothing to commit, working tree clean".

---

## Self-Review Notes

**Spec coverage:** The spec has four substantive sections — Problem, Fix, Testing, Scope (Out), Risk. Task 1 implements Fix and satisfies Testing (existing tests still pass; no new tests per spec). Scope-out items (`tests/signals.rs`, poison logging, broader audit) are not touched, as required. Task 2 addresses the CLAUDE.md bookkeeping obligation. Risk is addressed by the verification steps (1, 5, 6, 7, 8).

**Placeholder scan:** No TBDs, TODOs, "similar to above", or uninstantiated references. Every code block is complete and copy-pasteable.

**Type consistency:** Single symbol involved (`FD_TEST_LOCK`, a `Mutex<()>`); replacement idiom is identical across the three call sites. No drift possible.
