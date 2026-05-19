# POSIX Subshell Trap Reset — Design

**Date:** 2026-05-19
**Status:** Design
**Type:** Bug fix / POSIX conformance
**Closes XFAIL:** `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`
**Closes TODO entry:** `TODO.md` §Future: POSIX Conformance Bugs — "Subshell trap reset for uncaught signals not implemented"

## 1. Background

The 2026-05-13 E2E Ch4+Ch8 expansion surfaced one XFAIL test that SP7
(2026-05-17) recorded as a known POSIX deviation:

```sh
# e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh
trap 'echo parent' USR1
out=$( (trap) )
case "$out" in
    *USR1*) echo unexpected ;;
    *) echo ok ;;
esac
```

POSIX §2.11 specifies that "when a subshell is entered, traps that are
not being ignored shall be set to the default actions". yosh already
implements the trap-table reset via `TrapStore::reset_non_ignored`
(`src/env/traps.rs:113`) — Command-action traps are dropped, Ignore-action
traps are preserved.

However, POSIX §2.14 trap RATIONALE separately requires that
`traps=$(trap)` show the parent's traps (so scripts can save/restore
trap settings). yosh implements this via `TrapStore::saved_traps`: when
entering a command substitution (`reset_for_command_sub`), the parent's
trap table is snapshotted into `saved_traps`, and `display_all` prefers
that snapshot when present.

The bug is that **`saved_traps` propagates through nested literal
subshells inside a command substitution**. In `$( (trap) )`:

1. Outer `$(...)`: child fork. `reset_for_command_sub` populates
   `saved_traps` with parent's USR1 trap, then resets `signal_traps`.
2. Inner `( ... )`: another fork inside the command-sub child.
   `exec_subshell` (`src/exec/compound.rs:103`) calls `reset_non_ignored`,
   which clears `signal_traps` (already empty) **but does not touch
   `saved_traps`**.
3. Inner child runs `trap` (no args). `display_all` sees
   `saved_traps.is_some()` and prints the parent's USR1 trap.

Result: `$out` contains `USR1`, the test fails.

The fix is to clear `saved_traps` at every non-command-sub subshell entry
so that the `trap` builtin in such subshells reflects the subshell's own
(reset) state, while preserving the `$(trap)` save/restore mechanism for
single-level command substitution.

## 2. Scope

**In scope:**

- New method `TrapStore::reset_for_subshell` in `src/env/traps.rs`
- Replace `reset_non_ignored` with `reset_for_subshell` at three
  fork-time call sites:
  - `src/exec/compound.rs:103` (`exec_subshell` — literal `(...)`)
  - `src/exec/pipeline.rs:79` (pipeline child)
  - `src/exec/control.rs:128` (`exec_async` — background `&`)
- Three new unit tests in `src/env/traps.rs::tests`
- Two new integration tests in `tests/subshell.rs`
- Strip the `# XFAIL:` line from
  `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`
- Remove the corresponding entry from `TODO.md` §Future: POSIX
  Conformance Bugs

**Out of scope:**

- The `$(...)` command-sub `saved_traps` mechanism — preserved verbatim
  per POSIX §2.14 RATIONALE (`traps=$(trap)` save/restore pattern).
- Semantics of `reset_non_ignored` itself — left unchanged so existing
  unit tests (`test_trap_store_reset_non_ignored` and the four other
  `set_trap_with` / `remove_trap_with` tests) continue to PASS untouched.
- Subshell EXIT-trap behavior — the existing rule "Command-action EXIT
  trap is cleared in subshells" stays in place via `reset_non_ignored`
  and is not affected by this change.
- Locale support — the second item under §Future: POSIX Conformance
  Bugs is a separate concern with vastly larger scope and a separate
  spec.

## 3. Design

### 3.1 New method `TrapStore::reset_for_subshell`

Added to `impl TrapStore` immediately after `reset_non_ignored`:

```rust
/// Reset traps for an immediate subshell (literal `(...)`, pipeline child,
/// background `&`). Clears `saved_traps` so a nested `trap` builtin reflects
/// the post-reset state of the subshell rather than an enclosing
/// command-substitution's parent snapshot.
///
/// Distinct from [`Self::reset_for_command_sub`]: command substitution
/// (`$(...)`) preserves parent traps in `saved_traps` for the POSIX
/// `traps=$(trap)` save/restore pattern (POSIX §2.14 RATIONALE), but
/// non-command-sub subshells must show their own (reset) state.
pub fn reset_for_subshell(&mut self) {
    self.reset_non_ignored();
    self.saved_traps = None;
}
```

Semantics:

- `reset_non_ignored()` keeps the current behavior: Command-action traps
  are dropped from `signal_traps`, Ignore-action traps are preserved, and
  Command-action `exit_trap` is cleared.
- `self.saved_traps = None` discards any parent command-sub snapshot so
  `display_all`'s `saved_traps.is_some()` branch takes the "no snapshot —
  show current trap table" path.

### 3.2 Call-site replacement

Three single-line changes:

| File | Line | Before | After |
|---|---|---|---|
| `src/exec/compound.rs` | 103 | `self.env.traps.reset_non_ignored();` | `self.env.traps.reset_for_subshell();` |
| `src/exec/pipeline.rs` | 79 | `self.env.traps.reset_non_ignored();` | `self.env.traps.reset_for_subshell();` |
| `src/exec/control.rs` | 128 | `self.env.traps.reset_non_ignored();` | `self.env.traps.reset_for_subshell();` |

`reset_for_command_sub` at `src/expand/command_sub.rs:70` is unchanged
— `$(...)` continues to populate `saved_traps`.

### 3.3 Display flow after fix

| Context | `saved_traps` state on `display_all` entry | Output |
|---|---|---|
| top-level shell | `None` | current `signal_traps` + ignored-on-entry set |
| inside `$(trap)` | `Some(parent_snapshot)` | parent's snapshot (POSIX rationale path) |
| inside `(trap)` | `None` (cleared by `reset_for_subshell`) | current (reset) `signal_traps` |
| inside `$( (trap) )` | `None` (cleared in inner fork) | current (reset) `signal_traps` |
| inside `pipeline \| (trap)` | `None` (cleared in pipeline child) | current (reset) `signal_traps` |

### 3.4 Invariants preserved

- `TrapStore::saved_traps` remains private (no visibility bump required).
- `TrapStore::reset_non_ignored` signature and behavior are unchanged
  (no ripple to its five existing unit tests).
- `TrapStore::reset_for_command_sub` is unchanged (no ripple to
  `test_cmdsub_trap_isolation` in `tests/subshell.rs:236`).
- `display_all`'s saved-traps branch is unchanged.
- The ignored-on-entry display path (`ignored_on_entry_set_opt`) is
  unchanged.

## 4. Test plan

### 4.1 Existing tests (must remain PASS)

The following tests exercise adjacent behavior. All of them must
continue to PASS without modification:

| Test | What it asserts |
|---|---|
| `tests/subshell.rs::test_subshell_trap_command_reset` | `(trap)` does not show parent's Command-action INT trap |
| `tests/subshell.rs::test_subshell_trap_ignore_inherited` | `(trap)` shows parent's Ignore-action INT trap |
| `tests/subshell.rs::test_cmdsub_trap_isolation` | `$(trap)` shows parent's Command-action INT trap |
| `tests/signals.rs::test_subshell_trap_reset` (l.102) | `(trap)` does not show parent's Command trap |
| `tests/signals.rs` (l.111) | `(trap -p INT)` shows Ignore-inherited INT |
| `tests/ignored_on_entry.rs` (l.90) | `( trap )` shows SIGTERM when ignored-on-entry |
| `e2e/signal_and_trap/trap_in_subshell_reset.sh` | `(output=$(trap); ...)` USR1 not shown |
| `e2e/signal_and_trap/trap_display.sh` | `$(trap)` shows EXIT trap |
| `e2e/posix_spec/2_11/.../trap_list_shows_handlers.sh` | `$(trap)` shows INT trap |
| `e2e/posix_spec/4_special_builtin/trap_list_no_args.sh` | `$(trap)` shows EXIT trap |
| `e2e/posix_spec/4_special_builtin/trap_subshell_does_not_leak.sh` | subshell EXIT trap fires on subshell exit, does not affect parent |
| `e2e/subshell/subshell_trap_inherit_ignore.sh` | `''`-trap (Ignore) is inherited into subshells |

### 4.2 New unit tests (`src/env/traps.rs::tests`)

Three tests cover the new method. Because `saved_traps` is a private
field, the tests use a `#[cfg(test)] fn saved_traps_is_some(&self) -> bool`
helper (added inside the same `impl TrapStore` block) to inspect state
without bumping field visibility.

```rust
#[test]
fn test_reset_for_subshell_clears_saved_traps() {
    let mut store = TrapStore::default();
    store.set_trap("INT", TrapAction::Command("echo parent".into())).unwrap();
    store.reset_for_command_sub();
    assert!(store.saved_traps_is_some(), "precondition: saved_traps set");
    store.reset_for_subshell();
    assert!(!store.saved_traps_is_some(), "saved_traps must be cleared");
    assert!(store.signal_traps.is_empty(), "signal_traps must be reset");
}

#[test]
fn test_reset_for_subshell_preserves_ignored() {
    let mut store = TrapStore::default();
    store.set_trap("HUP", TrapAction::Ignore).unwrap();
    store.set_trap("INT", TrapAction::Command("x".into())).unwrap();
    store.reset_for_subshell();
    assert_eq!(store.signal_traps.get(&1), Some(&TrapAction::Ignore));
    assert!(!store.signal_traps.contains_key(&2));
}

#[test]
fn test_reset_for_subshell_with_no_saved_traps_is_safe() {
    let mut store = TrapStore::default();
    store.set_trap("INT", TrapAction::Command("x".into())).unwrap();
    store.reset_for_subshell();
    assert!(!store.saved_traps_is_some());
    assert!(store.signal_traps.is_empty());
}
```

### 4.3 New integration tests (`tests/subshell.rs`)

```rust
#[test]
fn test_nested_subshell_inside_cmdsub_shows_reset_traps() {
    // $( (trap) ) — nested literal subshell inside command sub must clear
    // saved_traps and show the inner subshell's reset state.
    let out = yosh_exec(
        "trap 'echo parent' USR1; out=$( (trap) ); \
         case \"$out\" in *USR1*) echo bad ;; *) echo ok ;; esac"
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}

#[test]
fn test_pipeline_child_clears_saved_traps() {
    // Pipeline child should also clear saved_traps.
    let out = yosh_exec(
        "trap 'echo parent' USR1; \
         out=$(echo dummy | (trap)); \
         case \"$out\" in *USR1*) echo bad ;; *) echo ok ;; esac"
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}
```

### 4.4 XFAIL strip and TODO cleanup

`e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`:

```diff
 #!/bin/sh
 # POSIX_REF: 2.11 Signals and Error Handling
 # DESCRIPTION: Subshell starts with traps reset to default for signals not caught in parent
-# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
 # EXPECT_OUTPUT: ok
 # EXPECT_EXIT: 0
```

`TODO.md` §Future: POSIX Conformance Bugs — remove the trap-reset entry
(7 lines). The locale entry remains.

## 5. Acceptance criteria

1. `cargo build` green; `cargo clippy --all-targets -- -D warnings`
   produces no warnings.
2. `cargo test` (unit + integration) all PASS, including the three new
   unit tests and the two new integration tests.
3. `./e2e/run_tests.sh` reports `XFail: 2 Migrated: 10` (down from
   `XFail: 3`), with `trap_resets_in_subshell_when_unhandled` PASSing
   (not XPASSing).
4. No regression in the 12 existing trap-related tests listed in §4.1.
5. `TODO.md` §Future: POSIX Conformance Bugs no longer contains the
   "Subshell trap reset for uncaught signals" entry; the locale entry
   is unchanged.
6. This spec is committed under `docs/superpowers/specs/`.

## 6. Commit shape

Two commits:

```
1) docs(trap-reset): add design spec for POSIX subshell trap reset
   - docs/superpowers/specs/2026-05-19-subshell-trap-reset-design.md

2) fix(exec): clear saved_traps in literal subshell/pipeline/async forks
   - src/env/traps.rs:
       * add reset_for_subshell
       * add saved_traps_is_some test helper
       * add 3 unit tests
   - src/exec/compound.rs: reset_non_ignored → reset_for_subshell
   - src/exec/pipeline.rs: reset_non_ignored → reset_for_subshell
   - src/exec/control.rs: reset_non_ignored → reset_for_subshell
   - tests/subshell.rs: add 2 integration tests
   - e2e/posix_spec/2_11_signals_and_error_handling/
       trap_resets_in_subshell_when_unhandled.sh: strip # XFAIL: line
   - TODO.md: remove trap-reset entry from §Future: POSIX Conformance Bugs
```

The split mirrors the SP1–SP7 design-then-implement pattern: a
documentation-only commit lands the spec for review, then a single
implementation commit bundles the source change, tests, XFAIL strip,
and TODO cleanup.

## 7. Risks

- **fork-time `Box::drop`**: `saved_traps` is `Option<Box<SavedTraps>>`.
  Assigning `None` in the post-fork child invokes `Box::drop`, which
  releases heap memory but does not touch mutexes, file descriptors,
  or other resources that would be unsafe between `fork()` and the
  child's eventual `_exit`/`execve`. Equivalent to the existing
  `HashMap::retain` and `Option::take` operations that
  `reset_non_ignored` already performs at the same point.
- **Behavioral compatibility with `reset_for_command_sub`**: Unchanged
  by construction. `test_cmdsub_trap_isolation` and the three e2e
  `$(trap)` tests verify the command-sub path is untouched.
- **dash divergence**: dash shows parent traps in literal subshells
  (no reset). yosh moves from dash-like (current) to bash-like (POSIX
  rationale-conformant) behavior. This is a deliberate alignment with
  the POSIX-rationale interpretation; the SP7 deferral note in TODO.md
  ("interpretation varies across shells") is what this fix resolves.
- **EXIT-trap interaction**: `reset_non_ignored` already clears
  Command-action `exit_trap` on subshell entry. The new method does
  not change that — `execute_exit_trap` at the bottom of `exec_subshell`
  (SP5 T6, `src/exec/compound.rs:109`) continues to fire only if the
  subshell installed its own EXIT trap after entry.
