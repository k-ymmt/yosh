# `getopts` OPTIND Reset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `OPTIND=1` restart POSIX `getopts` parsing even when the visible value was already `1`, especially while parsing stacked options such as `-ab`.

**Architecture:** Store successful `OPTIND` write generations in `src/env/vars.rs` next to the existing scope-local `getopts_subindex`. `builtin_getopts` checks for an unobserved `OPTIND` write before parsing, clears `getopts_subindex` when one exists, then marks the resulting generation as observed after its own state update.

**Tech Stack:** Rust 2024, existing `#[cfg(test)]` unit tests, POSIX e2e harness `./e2e/run_tests.sh`.

**Spec:** `docs/superpowers/specs/2026-05-29-getopts-optind-reset-design.md`

---

## File Structure

- **Modify:** `src/env/vars.rs`
  - Add per-scope `OPTIND` write generation tracking.
  - Advance the generation on successful `VarStore::set("OPTIND", ...)`.
  - Keep internal scope restore from creating a caller-visible reset.
  - Add private-field API for `getopts` reset detection and observation.
  - Add unit tests for same-value `OPTIND` writes and scope isolation.
- **Modify:** `src/builtin/getopts.rs`
  - Reset `getopts_subindex` before parsing when `OPTIND` has been written since the last observed generation.
  - Mark the current generation observed after successful state mutation.
  - Add unit tests for stacked reset and no-reset normal stacked parsing.
- **Create:** `e2e/posix_spec/4_required_builtin/getopts_optind_reset_stacked.sh`
  - Shell-visible POSIX regression test.
- **Modify:** `TODO.md`
  - Delete the completed SP4 follow-up about user-reset semantics for `OPTIND`.

---

## Task 1: Add `OPTIND` Write Tracking To `VarStore`

**Files:**
- Modify: `src/env/vars.rs`

- [ ] **Step 1: Write failing unit tests for write detection**

Add tests in `src/env/vars.rs` proving:

```rust
#[test]
fn optind_write_since_getopts_detects_same_value_assignment() {
    let mut store = VarStore::new();
    store.set("OPTIND", "1").unwrap();
    store.mark_getopts_observed_optind();
    assert!(!store.optind_written_since_getopts());

    store.set("OPTIND", "1").unwrap();
    assert!(store.optind_written_since_getopts());
}

#[test]
fn optind_write_generation_is_scope_local() {
    let mut store = VarStore::new();
    store.set("OPTIND", "1").unwrap();
    store.mark_getopts_observed_optind();

    store.push_scope(vec![]);
    assert!(!store.optind_written_since_getopts());
    store.set("OPTIND", "1").unwrap();
    assert!(store.optind_written_since_getopts());

    store.pop_scope();
    assert!(!store.optind_written_since_getopts());
}

#[test]
fn pop_scope_optind_restore_does_not_trigger_caller_reset() {
    let mut store = VarStore::new();
    store.set("OPTIND", "1").unwrap();
    store.set_getopts_subindex(2);
    store.mark_getopts_observed_optind();

    store.push_scope(vec![]);
    store.set("OPTIND", "1").unwrap();

    store.pop_scope();
    assert_eq!(store.getopts_subindex(), 2);
    assert!(!store.optind_written_since_getopts());
}
```

Expected initial result: compile failure because the new methods do not exist.

- [ ] **Step 2: Extend `Scope`**

Add two `u64` fields to `Scope`:

```rust
/// Successful writes to visible `OPTIND` in this scope.
optind_write_generation: u64,
/// Last `OPTIND` write generation observed by `getopts`.
getopts_observed_optind_generation: u64,
```

Initialize both to `0` in `VarStore::new`, `VarStore::from_environ`, and `push_scope`.

- [ ] **Step 3: Advance generation on successful `OPTIND` writes**

In `VarStore::set`, after the write has passed readonly checks and landed in the correct variable slot, increment `self.scopes.last_mut().unwrap().optind_write_generation` when `name == "OPTIND"`.

Important: do not increment on failed readonly writes. Keep `environ_cache` invalidation behaviour unchanged.

For internal `pop_scope` restoration, either restore `OPTIND` through a helper that bypasses the user-write generation or immediately mark the caller's current `OPTIND` generation as observed after restoring. The result must be that returning from a function does not reset the caller's pending `getopts_subindex`.

- [ ] **Step 4: Add intent-level API**

Add methods near the existing `getopts_subindex` accessors:

```rust
pub fn optind_written_since_getopts(&self) -> bool
pub fn mark_getopts_observed_optind(&mut self)
pub fn reset_getopts_subindex(&mut self)
```

`mark_getopts_observed_optind` copies the current scope's write generation into `getopts_observed_optind_generation`.

- [ ] **Step 5: Run focused tests**

```bash
cargo test --lib env::vars::tests::optind_write
cargo test --lib env::vars::tests::pop_scope_optind_restore_does_not_trigger_caller_reset
```

Expected: PASS.

---

## Task 2: Make `builtin_getopts` Consume The Reset Signal

**Files:**
- Modify: `src/builtin/getopts.rs`

- [ ] **Step 1: Write the failing reset unit test**

Add this test near `builtin_stacked_two_calls`:

```rust
#[test]
fn builtin_optind_reset_to_one_restarts_stacked_option() {
    let mut env = make_env();
    env.vars.set_positional_params(vec!["-ab".into()]);
    let args = s(&["ab", "opt"]);

    let rc1 = super::builtin_getopts(&args, &mut env).unwrap();
    assert_eq!(rc1, 0);
    assert_eq!(env.vars.get("opt"), Some("a"));
    assert_eq!(env.vars.get("OPTIND"), Some("1"));

    env.vars.set("OPTIND", "1").unwrap();

    let rc2 = super::builtin_getopts(&args, &mut env).unwrap();
    assert_eq!(rc2, 0);
    assert_eq!(env.vars.get("opt"), Some("a"));
    assert_eq!(env.vars.get("OPTIND"), Some("1"));
}
```

Expected initial result: FAIL because the second call currently returns `b`.

- [ ] **Step 2: Preserve the no-user-write stacked regression guard**

Keep `builtin_stacked_two_calls` as the guard that `getopts`' own `OPTIND` write does not force a restart. If implementation changes make that test ambiguous, add a new explicit test with the same `-ab` flow and no intervening user assignment.

- [ ] **Step 3: Add the pre-parse reset check**

At the start of `builtin_getopts`, after operands are resolved and before reading `OPTIND`, add:

```rust
if env.vars.optind_written_since_getopts() {
    env.vars.reset_getopts_subindex();
}
```

Then read `OPTIND` and `getopts_subindex` as today.

- [ ] **Step 4: Mark observation after state mutation**

After `env.assign_var("OPTIND", step.optind.to_string())` and `env.vars.set_getopts_subindex(step.subindex)`, call:

```rust
env.vars.mark_getopts_observed_optind();
```

Keep this after all readonly pre-checks and successful assignments so failed paths do not partially update observation state.

- [ ] **Step 5: Run focused getopts tests**

```bash
cargo test --lib builtin::getopts::tests
```

Expected: PASS.

---

## Task 3: Add Shell-Visible E2E Coverage

**Files:**
- Create: `e2e/posix_spec/4_required_builtin/getopts_optind_reset_stacked.sh`

- [ ] **Step 1: Create the e2e test**

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: OPTIND=1 restarts getopts parsing inside a stacked option
# EXPECT_OUTPUT: aa
# EXPECT_EXIT: 0
set -- -ab
getopts ab opt
printf '%s' "$opt"
OPTIND=1
getopts ab opt
printf '%s\n' "$opt"
```

- [ ] **Step 2: Ensure file mode is 644**

```bash
chmod 644 e2e/posix_spec/4_required_builtin/getopts_optind_reset_stacked.sh
```

- [ ] **Step 3: Run the focused e2e**

```bash
cargo build
./e2e/run_tests.sh --filter=getopts_optind_reset_stacked
```

Expected: PASS.

---

## Task 4: Close The TODO And Run Regression Checks

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Delete the completed TODO item**

Remove the SP4 follow-up bullet beginning:

```text
User-reset semantics for `OPTIND`
```

Per project convention, delete completed TODO entries rather than marking them `[x]`.

- [ ] **Step 2: Run relevant regression tests**

```bash
cargo test --lib env::vars::tests
cargo test --lib builtin::getopts::tests
cargo test
cargo build
./e2e/run_tests.sh --filter=getopts
./e2e/run_tests.sh --filter=OPTIND
```

Expected: all commands PASS. If the e2e filters overlap or include unrelated failures, rerun the focused failing test standalone and record the unrelated failure clearly before finalizing.

- [ ] **Step 3: Review diff**

```bash
git diff -- src/env/vars.rs src/builtin/getopts.rs e2e/posix_spec/4_required_builtin/getopts_optind_reset_stacked.sh TODO.md
```

Confirm:

- `OPTIND` write tracking is scope-local.
- Same-value `OPTIND=1` writes are detected.
- `getopts`' own `OPTIND` update is marked observed.
- Readonly pre-check failure paths do not mark observation state.
- The new e2e file has mode `100644`.

- [ ] **Step 4: Commit**

```bash
git add src/env/vars.rs src/builtin/getopts.rs e2e/posix_spec/4_required_builtin/getopts_optind_reset_stacked.sh TODO.md
git commit -m "fix(getopts): honor OPTIND reset during stacked parsing"
```

---

## Verification Summary

Required before completion:

```bash
cargo test
cargo build
./e2e/run_tests.sh --filter=getopts
./e2e/run_tests.sh --filter=OPTIND
```
