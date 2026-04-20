# POSIX Chapter 2 Sub-project 5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the final two POSIX Chapter 2 conformance gap items — §2.11 ignored-on-entry signal inheritance, and §2.10.2 Rule 5 (reserved word as `for` NAME).

**Architecture:** Two independent fixes under one sub-project. §2.11 is a three-layer change (signal subsystem captures the inherited `SIG_IGN` set at startup, TrapStore silent-ignores operations on those signals via dependency injection, `reset_child_signals` unions the set for subshells/execs). §2.10.2 Rule 5 is a one-layer parser fix in `parse_for_clause` delegating to the canonical `lexer::reserved::is_posix_reserved_word`. Rust integration tests in `tests/ignored_on_entry.rs` drive ignored-on-entry state via `pre_exec` + `sigaction` to exercise §2.11 end-to-end.

**Tech Stack:** Rust 2024 edition, `nix` + `libc` crates for signal primitives, existing yosh parser/TrapStore, `cargo test` + `./e2e/run_tests.sh` harnesses.

**Spec:** `docs/superpowers/specs/2026-04-20-posix-ch2-gaps-subproject5-design.md`

---

## Prerequisites (before Task 1)

- [ ] **Step 0.1: Build**

```bash
cargo build
```
Expected: clean build (pre-existing warnings may remain).

- [ ] **Step 0.2: Record baseline**

```bash
cargo test --lib 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected:
- Lib: `test result: ok. 630 passed`
- E2E: `Total: 372  Passed: 371  Failed: 0  XFail: 1`

If counts differ, stop and reconcile.

---

## Task 1 (Commit ①): Signal subsystem — capture + skip + union

**Files:**
- Modify: `src/signal.rs` (add static, helper functions; modify `init_signal_handling` and `reset_child_signals`; add tests)

### Step 1.1: Add the `HashSet` import and static

In `src/signal.rs`, update the top-of-file imports to include `HashSet`:

Currently (lines 1–6):
```rust
use std::os::unix::io::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
```

- [ ] Change to:

```rust
use std::collections::HashSet;
use std::os::unix::io::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
```

### Step 1.2: Add the `IGNORED_ON_ENTRY` static

Below the existing `SELF_PIPE` declaration (around line 79), add:

- [ ] Insert:

```rust
/// Signals inherited with SIG_IGN disposition at shell entry.
/// Per POSIX §2.11, these signals cannot be trapped or reset by the shell.
/// Captured once at startup before any yosh handler is installed; never mutated
/// afterward, so a stale `get()` from a fork/exec child reflects the correct
/// entry state (because the global is inherited as a copy of the parent's set).
static IGNORED_ON_ENTRY: OnceLock<HashSet<i32>> = OnceLock::new();
```

### Step 1.3: Add `capture_ignored_on_entry` helper

- [ ] Below the `SELF_PIPE` static (after the `IGNORED_ON_ENTRY` declaration from Step 1.2), add:

```rust
/// Query each trappable POSIX signal's current disposition via `sigaction(_, NULL, &mut old)`
/// and return the set of signals currently set to SIG_IGN.
/// Must be called before any yosh handler is installed to correctly observe
/// what was inherited from the parent process.
fn capture_ignored_on_entry() -> HashSet<i32> {
    let mut set = HashSet::new();
    for &(num, _) in SIGNAL_TABLE {
        if num == libc::SIGKILL || num == libc::SIGSTOP {
            // SIGKILL/SIGSTOP cannot be caught or ignored; skip them.
            continue;
        }
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(num, std::ptr::null(), &mut old) };
        if rc != 0 {
            continue;
        }
        if old.sa_sigaction == libc::SIG_IGN {
            set.insert(num);
        }
    }
    set
}

/// Returns `true` if `sig` was inherited with SIG_IGN disposition at shell startup.
/// Returns `false` if [`init_signal_handling`] has not been called yet.
pub fn is_ignored_on_entry(sig: i32) -> bool {
    IGNORED_ON_ENTRY
        .get()
        .map_or(false, |set| set.contains(&sig))
}

/// Returns a reference to the set of ignored-on-entry signals.
///
/// # Panics
///
/// Panics if [`init_signal_handling`] has not been called.
pub fn ignored_on_entry_set() -> &'static HashSet<i32> {
    IGNORED_ON_ENTRY
        .get()
        .expect("init_signal_handling() must be called first")
}
```

### Step 1.4: Write the failing test for `is_ignored_on_entry` default-false behavior

Append to the `mod tests` block at the bottom of `src/signal.rs` (before the closing `}` on line 428):

- [ ] Add:

```rust
    // -----------------------------------------------------------------------
    // Sub-project 5 — Task 1: Ignored-on-entry capture tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_ignored_on_entry_false_for_unlikely_signal() {
        // After init (possibly already called by other tests), a benign signal
        // that is extremely unlikely to be inherited as SIG_IGN in a `cargo test`
        // run should report `false`. SIGUSR2 is a safe choice — no library
        // installs SIG_IGN for it by default.
        init_signal_handling();
        assert!(
            !is_ignored_on_entry(libc::SIGUSR2),
            "SIGUSR2 should not be ignored-on-entry in a normal test environment"
        );
    }
```

### Step 1.5: Run the test — should fail because `is_ignored_on_entry` exists but `IGNORED_ON_ENTRY` is still empty

- [ ] Run:

```bash
cargo test --lib test_is_ignored_on_entry_false_for_unlikely_signal 2>&1 | tail -10
```
Expected: the test **passes** (because `get()` returns `None` → `map_or(false, ...)` returns `false`). This confirms the "not yet captured" defensive path. We'll add a positive-path test below.

### Step 1.6: Integrate `capture_ignored_on_entry` into `init_signal_handling`

In `src/signal.rs`, modify `init_signal_handling` (lines 104–163). Inside the `SELF_PIPE.get_or_init(|| { ... })` closure, add the capture **as the first statement** and skip handler registration for captured signals.

Currently (excerpted):
```rust
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
        let mut fds: [libc::c_int; 2] = [0; 2];
        // ... pipe creation ...
        for &(num, _) in HANDLED_SIGNALS {
            let sig = Signal::try_from(num).expect("invalid signal number in HANDLED_SIGNALS");
            let sa = if num == libc::SIGHUP || num == libc::SIGTERM {
                &sa_no_restart
            } else {
                &sa_restart
            };
            unsafe {
                sigaction(sig, sa).expect("sigaction failed");
            }
        }
        (read_fd, write_fd)
    });
}
```

- [ ] Change the opening of the closure to capture first:

```rust
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
        // POSIX §2.11: capture the set of signals inherited as SIG_IGN before we
        // install any yosh handler. Skip registration for those signals so they
        // remain ignored for the shell's lifetime.
        IGNORED_ON_ENTRY.get_or_init(capture_ignored_on_entry);

        let mut fds: [libc::c_int; 2] = [0; 2];
        // ... (rest of pipe creation unchanged) ...
```

- [ ] Change the `for &(num, _) in HANDLED_SIGNALS` loop to skip ignored-on-entry signals:

```rust
        for &(num, _) in HANDLED_SIGNALS {
            // POSIX §2.11: leave inherited SIG_IGN in place.
            if IGNORED_ON_ENTRY
                .get()
                .expect("IGNORED_ON_ENTRY must be initialized above")
                .contains(&num)
            {
                continue;
            }

            let sig = Signal::try_from(num).expect("invalid signal number in HANDLED_SIGNALS");
            let sa = if num == libc::SIGHUP || num == libc::SIGTERM {
                &sa_no_restart
            } else {
                &sa_restart
            };
            unsafe {
                sigaction(sig, sa).expect("sigaction failed");
            }
        }
```

### Step 1.7: Write the test that exercises the positive-path capture via an externally-installed SIG_IGN

Append to `mod tests`:

- [ ] Add:

```rust
    #[test]
    fn test_capture_ignored_on_entry_detects_sig_ign() {
        // This test is standalone — it does NOT call init_signal_handling.
        // It exercises `capture_ignored_on_entry` directly to verify the
        // sigaction query logic. We use SIGUSR2 (a benign realtime-like
        // signal nobody else in tests touches) and restore the original
        // disposition afterward to avoid polluting sibling tests.
        let sig_num = libc::SIGUSR2;

        // Save the current disposition.
        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(sig_num, std::ptr::null(), &mut original) };
        assert_eq!(rc, 0);

        // Install SIG_IGN.
        let ign_sa = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
        let sig = Signal::try_from(sig_num).unwrap();
        unsafe { sigaction(sig, &ign_sa).unwrap(); }

        // Run the capture helper and assert SIGUSR2 is in the set.
        let captured = capture_ignored_on_entry();
        assert!(
            captured.contains(&sig_num),
            "capture_ignored_on_entry should detect SIGUSR2 SIG_IGN, got {:?}",
            captured
        );

        // Restore original disposition.
        let rc = unsafe { libc::sigaction(sig_num, &original, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_capture_ignored_on_entry_excludes_default() {
        // SIGUSR1 at SIG_DFL should NOT appear in the captured set.
        let sig_num = libc::SIGUSR1;

        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(sig_num, std::ptr::null(), &mut original) };
        assert_eq!(rc, 0);

        let dfl_sa = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
        let sig = Signal::try_from(sig_num).unwrap();
        unsafe { sigaction(sig, &dfl_sa).unwrap(); }

        let captured = capture_ignored_on_entry();
        assert!(
            !captured.contains(&sig_num),
            "capture_ignored_on_entry should not include SIG_DFL signals, got {:?}",
            captured
        );

        // Restore.
        let rc = unsafe { libc::sigaction(sig_num, &original, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }
```

### Step 1.8: Run the new tests

- [ ] Run:

```bash
cargo test --lib test_capture_ignored_on_entry 2>&1 | tail -10
```
Expected: 2 tests pass.

Also run the previously added test:

```bash
cargo test --lib test_is_ignored_on_entry_false_for_unlikely_signal 2>&1 | tail -5
```
Expected: 1 test passes.

### Step 1.9: Modify `reset_child_signals` to union `IGNORED_ON_ENTRY`

Locate `reset_child_signals` in `src/signal.rs` (lines 226–242).

Currently:
```rust
pub fn reset_child_signals(ignored: &[i32]) {
    for &(num, _) in HANDLED_SIGNALS {
        if ignored.contains(&num) {
            ignore_signal(num);
        } else {
            default_signal(num);
        }
    }

    // Close self-pipe fds if they exist.
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}
```

- [ ] Change to:

```rust
pub fn reset_child_signals(ignored: &[i32]) {
    let entry_set = IGNORED_ON_ENTRY.get();
    for &(num, _) in HANDLED_SIGNALS {
        let keep_ignored = ignored.contains(&num)
            || entry_set.map_or(false, |s| s.contains(&num));
        if keep_ignored {
            ignore_signal(num);
        } else {
            default_signal(num);
        }
    }

    // Close self-pipe fds if they exist.
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}
```

### Step 1.10: Run the full library test suite

- [ ] Run:

```bash
cargo test --lib 2>&1 | tail -5
```
Expected: `test result: ok. 633 passed` (630 baseline + 3 new). Zero failures.

### Step 1.11: Run the full E2E suite

- [ ] Run:

```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: `Total: 372  Passed: 371  Failed: 0  Timedout: 0  XFail: 1  XPass: 0` (unchanged from baseline — TrapStore and parser changes haven't landed yet).

### Step 1.12: Commit

- [ ] Run:

```bash
git add src/signal.rs
git commit -m "$(cat <<'EOF'
feat(signal): capture ignored-on-entry dispositions per POSIX §2.11

Add IGNORED_ON_ENTRY static populated by capture_ignored_on_entry() at
the start of init_signal_handling(). Skip handler registration for
signals present in the set so inherited SIG_IGN survives shell startup.
Union the set inside reset_child_signals() so subshells and exec'd
children also preserve the disposition.

Part of sub-project 5 (POSIX Chapter 2 conformance gaps, final).
Spec: docs/superpowers/specs/2026-04-20-posix-ch2-gaps-subproject5-design.md

Original prompt: 前回までの記録から、 #5 を対応してください。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 (Commit ②): TrapStore silent-ignore + display union

**Files:**
- Modify: `src/env/traps.rs` (add `set_trap_with` / `remove_trap_with`, delegate from public methods, update `display_all`, add 4 tests)

### Step 2.1: Update imports and sort-key container

In `src/env/traps.rs`, the current file uses `HashMap<i32, TrapAction>`. The `display_all` method materializes keys into a `Vec<i32>` and sorts — we will switch to `BTreeSet<i32>` for the union with ignored-on-entry. The signal_traps map itself stays `HashMap`.

- [ ] At the top of `src/env/traps.rs`, change:

```rust
use std::collections::HashMap;
```

to:

```rust
use std::collections::{BTreeSet, HashMap};
```

### Step 2.2: Write the failing test for `set_trap_with` silent-ignore

Append to the `mod tests` block at the bottom of `src/env/traps.rs`:

- [ ] Add:

```rust
    // -----------------------------------------------------------------------
    // Sub-project 5 — Task 2: Silent-ignore via dependency-injected predicate
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_trap_with_ignored_predicate_is_silent() {
        // Simulate SIGINT (2) as ignored-on-entry via an injected predicate.
        let mut store = TrapStore::default();
        let is_ignored = |sig: i32| sig == 2;
        let result = store.set_trap_with(
            "INT",
            TrapAction::Command("echo caught".to_string()),
            &is_ignored,
        );
        assert!(result.is_ok(), "silent-ignore must return Ok(()), got {:?}", result);
        assert!(
            store.signal_traps.is_empty(),
            "signal_traps should remain empty when set on ignored-on-entry; got {:?}",
            store.signal_traps
        );
    }
```

### Step 2.3: Run the test — should fail to compile

- [ ] Run:

```bash
cargo test --lib test_set_trap_with_ignored_predicate_is_silent 2>&1 | tail -10
```
Expected: compile error like `no method named 'set_trap_with' found for struct TrapStore`.

### Step 2.4: Implement `set_trap_with` and make `set_trap` delegate

In `src/env/traps.rs`, find `set_trap` (lines 47–56). Replace with the following two methods:

- [ ] Change the `set_trap` method block to:

```rust
    /// Set a trap for the given condition (signal name or number).
    /// Delegates to [`Self::set_trap_with`] using [`crate::signal::is_ignored_on_entry`]
    /// as the ignored-on-entry predicate.
    pub fn set_trap(&mut self, condition: &str, action: TrapAction) -> Result<(), String> {
        self.set_trap_with(condition, action, &|sig| {
            crate::signal::is_ignored_on_entry(sig)
        })
    }

    /// Set a trap for the given condition, using `is_ignored` to decide whether
    /// to silently no-op (POSIX §2.11: ignored-on-entry signals cannot be trapped
    /// or reset). Exposed for unit testing so tests can inject a synthetic
    /// predicate without mutating process signal state.
    pub(crate) fn set_trap_with(
        &mut self,
        condition: &str,
        action: TrapAction,
        is_ignored: &dyn Fn(i32) -> bool,
    ) -> Result<(), String> {
        let num = Self::signal_name_to_number(condition)
            .ok_or_else(|| format!("invalid signal name: {}", condition))?;
        if num == 0 {
            // EXIT pseudo-signal is always settable.
            self.exit_trap = Some(action);
            return Ok(());
        }
        if is_ignored(num) {
            // POSIX §2.11: silent no-op.
            return Ok(());
        }
        self.signal_traps.insert(num, action);
        Ok(())
    }
```

### Step 2.5: Run the test — should pass

- [ ] Run:

```bash
cargo test --lib test_set_trap_with_ignored_predicate_is_silent 2>&1 | tail -5
```
Expected: PASS.

### Step 2.6: Write the failing test for `remove_trap_with` silent-ignore

- [ ] Add to `mod tests`:

```rust
    #[test]
    fn test_remove_trap_with_ignored_predicate_is_silent() {
        // Pre-populate signal_traps with SIGINT=Ignore, then attempt to remove
        // with SIGINT marked ignored-on-entry — the entry must remain.
        let mut store = TrapStore::default();
        store.signal_traps.insert(2, TrapAction::Ignore);
        let is_ignored = |sig: i32| sig == 2;
        store.remove_trap_with("INT", &is_ignored);
        assert_eq!(
            store.signal_traps.get(&2),
            Some(&TrapAction::Ignore),
            "remove_trap on ignored-on-entry signal must be silent no-op"
        );
    }
```

### Step 2.7: Run the test — should fail to compile

- [ ] Run:

```bash
cargo test --lib test_remove_trap_with_ignored_predicate_is_silent 2>&1 | tail -5
```
Expected: compile error.

### Step 2.8: Implement `remove_trap_with` and make `remove_trap` delegate

Find `remove_trap` (lines 70–78). Replace with:

- [ ] Change the `remove_trap` method block to:

```rust
    /// Remove/reset the trap for the given condition.
    /// Delegates to [`Self::remove_trap_with`].
    pub fn remove_trap(&mut self, condition: &str) {
        self.remove_trap_with(condition, &|sig| {
            crate::signal::is_ignored_on_entry(sig)
        })
    }

    /// Remove/reset a trap with an injected ignored-on-entry predicate.
    /// Silent no-op for ignored-on-entry signals per POSIX §2.11.
    pub(crate) fn remove_trap_with(
        &mut self,
        condition: &str,
        is_ignored: &dyn Fn(i32) -> bool,
    ) {
        let Some(num) = Self::signal_name_to_number(condition) else { return; };
        if num == 0 {
            self.exit_trap = None;
            return;
        }
        if is_ignored(num) {
            return;
        }
        self.signal_traps.remove(&num);
    }
```

### Step 2.9: Run the test — should pass

- [ ] Run:

```bash
cargo test --lib test_remove_trap_with_ignored_predicate_is_silent 2>&1 | tail -5
```
Expected: PASS.

### Step 2.10: Add regression tests for non-ignored and EXIT paths

- [ ] Add to `mod tests`:

```rust
    #[test]
    fn test_set_trap_with_non_ignored_predicate_inserts_normally() {
        // Regression: when predicate returns false, behaviour matches the
        // original set_trap — insertion into signal_traps.
        let mut store = TrapStore::default();
        let never_ignored = |_sig: i32| false;
        let result = store.set_trap_with(
            "INT",
            TrapAction::Command("echo x".to_string()),
            &never_ignored,
        );
        assert!(result.is_ok());
        assert!(matches!(
            store.signal_traps.get(&2),
            Some(TrapAction::Command(_))
        ));
    }

    #[test]
    fn test_set_trap_exit_signal_bypasses_ignored_check() {
        // EXIT (signal 0) is always settable regardless of the predicate.
        let mut store = TrapStore::default();
        let always_ignored = |_sig: i32| true;
        let result = store.set_trap_with(
            "EXIT",
            TrapAction::Command("echo bye".to_string()),
            &always_ignored,
        );
        assert!(result.is_ok());
        assert!(matches!(store.exit_trap, Some(TrapAction::Command(_))));
    }
```

### Step 2.11: Run the new tests

- [ ] Run:

```bash
cargo test --lib test_set_trap_with 2>&1 | tail -10
cargo test --lib test_set_trap_exit_signal_bypasses 2>&1 | tail -5
```
Expected: 3 tests pass (two from Step 2.10, plus the earlier `set_trap_with_ignored`).

### Step 2.12: Add `ignored_on_entry_set_opt` helper to `src/signal.rs`

`display_all` must not panic if called before `init_signal_handling` (e.g. in
unit tests that don't initialise the signal subsystem), so we need a
non-panicking `Option`-returning accessor. Add it to `src/signal.rs`, just
below the existing `pub fn ignored_on_entry_set` (added in Task 1 Step 1.3):

- [ ] Insert:

```rust
/// Like [`ignored_on_entry_set`] but returns `None` if the capture has not
/// happened yet (useful for callers that must not panic, e.g. `display_all`).
pub fn ignored_on_entry_set_opt() -> Option<&'static HashSet<i32>> {
    IGNORED_ON_ENTRY.get()
}
```

### Step 2.13: Update `display_all` to union ignored-on-entry

Find `display_all` in `src/env/traps.rs` (lines 118–146). It currently
collects keys into a `Vec<i32>` and sorts.

Currently:
```rust
    pub fn display_all(&self) {
        let (exit_trap, signal_traps) = if let Some(saved) = &self.saved_traps {
            (&saved.0, &saved.1)
        } else {
            (&self.exit_trap, &self.signal_traps)
        };

        // Exit trap first
        if let Some(action) = exit_trap {
            match action {
                TrapAction::Command(cmd) => println!("trap -- '{}' EXIT", cmd),
                TrapAction::Ignore => println!("trap -- '' EXIT"),
                TrapAction::Default => {}
            }
        }
        // Signal traps sorted by number
        let mut keys: Vec<i32> = signal_traps.keys().copied().collect();
        keys.sort();
        for num in keys {
            if let Some(action) = signal_traps.get(&num) {
                let name = Self::signal_number_to_name(num);
                match action {
                    TrapAction::Command(cmd) => println!("trap -- '{}' SIG{}", cmd, name),
                    TrapAction::Ignore => println!("trap -- '' SIG{}", name),
                    TrapAction::Default => {}
                }
            }
        }
    }
```

- [ ] Replace with:

```rust
    pub fn display_all(&self) {
        let (exit_trap, signal_traps) = if let Some(saved) = &self.saved_traps {
            (&saved.0, &saved.1)
        } else {
            (&self.exit_trap, &self.signal_traps)
        };

        // Exit trap first
        if let Some(action) = exit_trap {
            match action {
                TrapAction::Command(cmd) => println!("trap -- '{}' EXIT", cmd),
                TrapAction::Ignore => println!("trap -- '' EXIT"),
                TrapAction::Default => {}
            }
        }

        // Union signal_traps keys with ignored-on-entry signals (POSIX §2.11:
        // these must appear in `trap` output even though we didn't install a
        // TrapAction for them). BTreeSet gives deterministic sort-by-number.
        let mut keys: BTreeSet<i32> = signal_traps.keys().copied().collect();
        if let Some(entry_set) = crate::signal::ignored_on_entry_set_opt() {
            for &sig in entry_set {
                keys.insert(sig);
            }
        }
        for num in keys {
            let name = Self::signal_number_to_name(num);
            match signal_traps.get(&num) {
                Some(TrapAction::Command(cmd)) => println!("trap -- '{}' SIG{}", cmd, name),
                Some(TrapAction::Ignore) => println!("trap -- '' SIG{}", name),
                Some(TrapAction::Default) => {}
                None => {
                    // Ignored-on-entry with no explicit trap — display as ''.
                    println!("trap -- '' SIG{}", name);
                }
            }
        }
    }
```

### Step 2.14: Build to check compilation

- [ ] Run:

```bash
cargo build 2>&1 | tail -5
```
Expected: clean build, no errors.

### Step 2.15: Run the full library test suite

- [ ] Run:

```bash
cargo test --lib 2>&1 | tail -5
```
Expected: `test result: ok. 637 passed` (633 from Task 1 + 4 new). Zero failures.

### Step 2.16: Run the trap-specific E2E tests

- [ ] Run:

```bash
./e2e/run_tests.sh --filter=trap 2>&1 | tail -5
```
Expected: all trap-related tests pass (display output unchanged because no signals are ignored-on-entry in the E2E subprocess).

### Step 2.17: Run the full E2E suite

- [ ] Run:

```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: `Total: 372  Passed: 371  Failed: 0  XFail: 1` (unchanged).

### Step 2.18: Commit

- [ ] Run:

```bash
git add src/env/traps.rs src/signal.rs
git commit -m "$(cat <<'EOF'
feat(trap): silent-ignore trap ops on ignored-on-entry signals

Add set_trap_with / remove_trap_with dependency-injection variants on
TrapStore; public set_trap / remove_trap delegate using
signal::is_ignored_on_entry. Per POSIX §2.11, operations targeting
ignored-on-entry signals succeed silently with no state change.

display_all now unions ignored-on-entry signals into its sorted key
set (via new signal::ignored_on_entry_set_opt accessor) and renders
missing entries as `trap -- '' SIG<NAME>`, matching bash output.
BTreeSet replaces the Vec+sort pattern for deterministic union.

Part of sub-project 5 (POSIX Chapter 2 conformance gaps, final).

Original prompt: 前回までの記録から、 #5 を対応してください。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 (Commit ③): Rust integration tests

**Files:**
- Create: `tests/ignored_on_entry.rs`

### Step 3.1: Create the integration test file

- [ ] Create `tests/ignored_on_entry.rs` with the following content:

```rust
//! Integration tests for POSIX §2.11 ignored-on-entry signal inheritance.
//!
//! Each test spawns the yosh binary in a subprocess with specific signals
//! pre-set to SIG_IGN via `pre_exec`, then asserts yosh's observable
//! behaviour (stdout, stderr, exit code). This verifies the end-to-end
//! flow from `capture_ignored_on_entry` through `TrapStore::set_trap`
//! and `reset_child_signals`.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// Spawn yosh with the given signal numbers pre-ignored (SIG_IGN) in the child,
/// feeding the `script` to `yosh -c`. Returns (stdout, stderr, exit_code).
fn spawn_yosh_with_ignored(signals: &[i32], script: &str) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yosh"));
    cmd.arg("-c").arg(script);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let sigs = signals.to_vec();
    unsafe {
        cmd.pre_exec(move || {
            let sa = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
            for &num in &sigs {
                let sig = Signal::try_from(num)
                    .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
                unsafe {
                    sigaction(sig, &sa)
                        .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                }
            }
            Ok(())
        });
    }

    let out = cmd.output().expect("yosh binary should be buildable and runnable");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn trap_set_on_ignored_on_entry_sigint_is_silent() {
    // Parent sets SIGINT=SIG_IGN, then yosh runs `trap 'echo caught' INT; echo $?`.
    // POSIX §2.11: the trap set must silently no-op and $? must be 0.
    let (stdout, stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGINT],
        "trap 'echo caught' INT; echo $?",
    );
    assert_eq!(code, 0, "exit code; stderr={}", stderr);
    assert_eq!(stdout.trim(), "0", "stdout should be just '0'; got {:?}", stdout);
    assert!(stderr.is_empty(), "no stderr expected, got {:?}", stderr);
}

#[test]
fn trap_reset_on_ignored_on_entry_sigint_is_silent() {
    // `trap - INT` on an ignored-on-entry signal must also silent no-op.
    let (stdout, stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGINT],
        "trap - INT; echo $?",
    );
    assert_eq!(code, 0, "exit code; stderr={}", stderr);
    assert_eq!(stdout.trim(), "0", "stdout={:?}", stdout);
}

#[test]
fn trap_display_shows_ignored_on_entry() {
    // `trap` with no args should list SIGTERM as `trap -- '' SIGTERM`.
    let (stdout, _stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGTERM],
        "trap",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("trap -- '' SIGTERM"),
        "expected 'trap -- \\'\\' SIGTERM' in stdout; got {:?}",
        stdout
    );
}

#[test]
fn subshell_inherits_ignored_on_entry() {
    // In a subshell `( trap )`, SIGTERM should still appear as ignored.
    let (stdout, _stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGTERM],
        "( trap )",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("SIGTERM"),
        "subshell should inherit ignored-on-entry; stdout={:?}",
        stdout
    );
}

#[test]
fn external_cmd_inherits_ignored_on_entry() {
    // When yosh execs an external `sh -c 'trap'`, SIG_IGN must be preserved
    // across exec (POSIX guarantee, reinforced by reset_child_signals union).
    // We grep for INT in the external sh's trap output.
    let (stdout, _stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGINT],
        "sh -c 'trap' | grep -i int",
    );
    // grep returns 0 if a match was found.
    assert_eq!(code, 0, "external sh should inherit SIGINT ignore; stdout={:?}", stdout);
}

#[test]
fn non_ignored_signal_trap_still_works() {
    // Sanity: a signal NOT ignored-on-entry still accepts trap actions.
    // We trap SIGUSR1 then send it to the shell's PID and check output.
    let (stdout, stderr, code) = spawn_yosh_with_ignored(
        &[], // no signals pre-ignored
        "trap 'echo caught' USR1; kill -USR1 $$; sleep 0.1; echo done",
    );
    assert_eq!(code, 0, "stderr={}", stderr);
    assert!(
        stdout.contains("caught"),
        "USR1 trap should fire; stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
    assert!(stdout.contains("done"), "shell should continue; stdout={:?}", stdout);
}
```

### Step 3.2: Run the integration tests

- [ ] Run:

```bash
cargo build 2>&1 | tail -3
cargo test --test ignored_on_entry 2>&1 | tail -15
```
Expected: 6 tests pass.

If `external_cmd_inherits_ignored_on_entry` fails because the system lacks `sh` or `grep`, or if `non_ignored_signal_trap_still_works` races on the `sleep 0.1`, document the flaky test in the commit body and consider widening the sleep or using `wait`. But first try the written test.

### Step 3.3: Run the full library suite to check for regressions

- [ ] Run:

```bash
cargo test --lib 2>&1 | tail -5
```
Expected: `test result: ok. 637 passed`, unchanged.

### Step 3.4: Commit

- [ ] Run:

```bash
git add tests/ignored_on_entry.rs
git commit -m "$(cat <<'EOF'
test(signal): integration tests for ignored-on-entry inheritance

Add tests/ignored_on_entry.rs with 6 cases exercising the POSIX §2.11
ignored-on-entry contract end-to-end via fork/pre_exec/sigaction+exec:

- trap set on ignored signal is silent (exit 0, no state change)
- trap reset on ignored signal is silent
- `trap` (no args) displays ignored-on-entry signal as `trap -- '' SIG<NAME>`
- subshells inherit the ignore disposition
- external commands inherit via exec (POSIX guarantee + union in
  reset_child_signals)
- traps on non-ignored signals still work

Resolves the "no in-harness test yet (nested sh -c escapes yosh)"
blocker from TODO.md by moving coverage into Rust integration tests
that drive the SIG_IGN setup directly via pre_exec.

Part of sub-project 5 (POSIX Chapter 2 conformance gaps, final).

Original prompt: 前回までの記録から、 #5 を対応してください。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 (Commit ④): §2.10.2 Rule 5 parser rejection + E2E

**Files:**
- Modify: `src/parser/mod.rs` (add reserved-word check in `parse_for_clause`, add 4 unit tests)
- Modify: `e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh` (remove `# XFAIL:` header)
- Create: `e2e/posix_spec/2_10_shell_grammar/rule05_for_in_word_rejected.sh`
- Create: `e2e/posix_spec/2_10_shell_grammar/rule05_for_while_word_rejected.sh`
- Create: `e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh`

### Step 4.1: Write the failing test for reserved-word rejection

In `src/parser/mod.rs`, find the existing `mod tests` block (search for `#[cfg(test)]`). Add a new test at the bottom of that module:

- [ ] Add:

```rust
    #[test]
    fn parse_for_reserved_word_if_rejected() {
        // POSIX §2.10.2 Rule 5: NAME in `for` must not be a reserved word.
        let src = "for if in a; do :; done\n";
        let err = Parser::new(src).parse_program().unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("reserved word") || msg.contains("not a valid"),
            "expected reserved-word error, got: {}",
            msg
        );
    }
```

### Step 4.2: Run the test — should fail

- [ ] Run:

```bash
cargo test --lib parse_for_reserved_word_if_rejected 2>&1 | tail -10
```
Expected: FAIL — parser currently accepts `for if in ...`.

### Step 4.3: Add the Rule 5 check in `parse_for_clause`

In `src/parser/mod.rs` `parse_for_clause`, locate the existing `is_valid_name` check (approximately lines 544–552):

```rust
                if !is_valid_name(name) {
                    let span = self.current_span();
                    return Err(ShellError::parse(
                        ParseErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        format!("'{}' is not a valid variable name", name),
                    ));
                }
```

- [ ] Insert a new check **immediately after** this block (and before `let name = name.to_string();`):

```rust
                if crate::lexer::reserved::is_posix_reserved_word(name) {
                    let span = self.current_span();
                    return Err(ShellError::parse(
                        ParseErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        format!(
                            "'{}' is a reserved word and cannot be used as a for-loop variable name",
                            name
                        ),
                    ));
                }
```

### Step 4.4: Run the failing test — should now pass

- [ ] Run:

```bash
cargo test --lib parse_for_reserved_word_if_rejected 2>&1 | tail -5
```
Expected: PASS.

### Step 4.5: Add the remaining parser unit tests

- [ ] Add to the same `mod tests` block:

```rust
    #[test]
    fn parse_for_reserved_word_in_rejected() {
        let src = "for in in a; do :; done\n";
        let err = Parser::new(src).parse_program().unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("reserved word") || msg.contains("not a valid"),
            "expected reserved-word error, got: {}",
            msg
        );
    }

    #[test]
    fn parse_for_valid_name_ok() {
        // Regression: a plain identifier NAME continues to parse cleanly.
        let src = "for i in a b c; do echo $i; done\n";
        assert!(
            Parser::new(src).parse_program().is_ok(),
            "valid for-loop should parse"
        );
    }

    #[test]
    fn parse_for_time_word_ok() {
        // POSIX §2.4 RESERVED_WORDS does NOT include `time` (that is a bash
        // extension from pipeline-prefix context). `for time in ...` must
        // therefore still parse in yosh.
        let src = "for time in a; do :; done\n";
        assert!(
            Parser::new(src).parse_program().is_ok(),
            "'for time' should parse because `time` is not in RESERVED_WORDS"
        );
    }
```

### Step 4.6: Run the new tests

- [ ] Run:

```bash
cargo test --lib parse_for_reserved_word 2>&1 | tail -10
cargo test --lib parse_for_valid_name_ok 2>&1 | tail -5
cargo test --lib parse_for_time_word_ok 2>&1 | tail -5
```
Expected: 4 tests pass.

### Step 4.7: Run the full library suite

- [ ] Run:

```bash
cargo test --lib 2>&1 | tail -5
```
Expected: `test result: ok. 641 passed` (637 + 4 new).

### Step 4.8: Remove the `# XFAIL:` line from the existing E2E test

In `e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh`, the current content is:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A reserved word is not a valid NAME
# XFAIL: yosh accepts reserved words as for-loop NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for if in a; do
    :
done
```

- [ ] Delete the `# XFAIL: yosh accepts reserved words as for-loop NAME` line (entire line, including newline). Final content:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A reserved word is not a valid NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for if in a; do
    :
done
```

### Step 4.9: Create `rule05_for_in_word_rejected.sh`

- [ ] Write the file at `e2e/posix_spec/2_10_shell_grammar/rule05_for_in_word_rejected.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: `in` is a reserved word and must be rejected as NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for in in a; do
    :
done
```

### Step 4.10: Create `rule05_for_while_word_rejected.sh`

- [ ] Write the file at `e2e/posix_spec/2_10_shell_grammar/rule05_for_while_word_rejected.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: `while` is a reserved word and must be rejected as NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for while in a; do
    :
done
```

### Step 4.11: Create `rule05_for_valid_name_ok.sh`

- [ ] Write the file at `e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A plain identifier NAME is accepted
# EXPECT_OUTPUT: a
# b
# c
# EXPECT_EXIT: 0
for i in a b c; do
    echo $i
done
```

### Step 4.12: Set file permissions to 644 for the new E2E tests

- [ ] Run:

```bash
chmod 644 e2e/posix_spec/2_10_shell_grammar/rule05_for_in_word_rejected.sh \
          e2e/posix_spec/2_10_shell_grammar/rule05_for_while_word_rejected.sh \
          e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh
```

### Step 4.13: Run the filtered E2E

- [ ] Run:

```bash
./e2e/run_tests.sh --filter=rule05 2>&1 | tail -15
```
Expected: 4 tests pass (the 3 new ones + the previously XFAIL test, now promoted to PASS).

### Step 4.14: Run the full E2E suite

- [ ] Run:

```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: `Total: 375  Passed: 375  Failed: 0  XFail: 0  XPass: 0` (3 new tests + XFAIL → PASS, so XFail goes 1→0 and total goes 372→375).

### Step 4.15: Commit

- [ ] Run:

```bash
git add src/parser/mod.rs \
        e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh \
        e2e/posix_spec/2_10_shell_grammar/rule05_for_in_word_rejected.sh \
        e2e/posix_spec/2_10_shell_grammar/rule05_for_while_word_rejected.sh \
        e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh
git commit -m "$(cat <<'EOF'
fix(parser): reject reserved words as for-loop NAME per POSIX §2.10.2 Rule 5

parse_for_clause now rejects reserved words as the for-loop NAME by
delegating to lexer::reserved::is_posix_reserved_word. The check is
inserted after the existing is_valid_name gate so numeric/symbol
tokens still fail with their existing message.

Promotes rule05_for_reserved_word_rejected.sh from XFAIL to PASS and
adds three companion E2E tests (rejection for `in`, rejection for
`while`, and a valid-name regression guard).

`time` is NOT rejected: POSIX §2.4 RESERVED_WORDS does not include
`time`, so `for time in ...` continues to parse in yosh. This matches
spec scope; bash-style rejection of `time` would require extending
the canonical reserved-words list and is out of scope.

Part of sub-project 5 (POSIX Chapter 2 conformance gaps, final).

Original prompt: 前回までの記録から、 #5 を対応してください。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 (Commit ⑤): TODO.md cleanup

**Files:**
- Modify: `TODO.md` (remove the "Future: POSIX Conformance Gaps (Chapter 2)" section)

### Step 5.1: Locate and delete the section

In `TODO.md`, find the block:

```markdown
## Future: POSIX Conformance Gaps (Chapter 2)

- [ ] §2.11 ignored-on-entry signal inheritance — no in-harness test yet (nested `sh -c` escapes yosh); revisit after a yosh-aware subshell helper lands
- [ ] §2.10.2 Rule 5 — yosh accepts reserved words as `for` NAME (`e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh` XFAIL). POSIX requires NAME to be a valid name, not a reserved word.

```

- [ ] Delete the entire block (the heading, the two items, and the blank line that follows).

### Step 5.2: Verify the section is gone

- [ ] Run:

```bash
grep -n 'POSIX Conformance Gaps' TODO.md
```
Expected: no output (exit code 1 from grep is fine — means no match).

### Step 5.3: Run the full suite one final time

- [ ] Run:

```bash
cargo test --lib 2>&1 | tail -3
cargo test --test ignored_on_entry 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected:
- Lib: `test result: ok. 641 passed` (or higher if other tests were added)
- Integration: `test result: ok. 6 passed`
- E2E: `Total: 375  Passed: 375  Failed: 0  XFail: 0`

### Step 5.4: Commit

- [ ] Run:

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): close POSIX Chapter 2 conformance gap section

Remove the "Future: POSIX Conformance Gaps (Chapter 2)" section — both
remaining items (§2.11 ignored-on-entry and §2.10.2 Rule 5) are now
addressed by sub-project 5. The five-part Chapter 2 conformance gap
remediation series (sub-projects 1–5) is complete.

Original prompt: 前回までの記録から、 #5 を対応してください。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Post-implementation verification

- [ ] **Step P.1: Full regression check**

```bash
cargo build 2>&1 | tail -3
cargo test --lib 2>&1 | tail -3
cargo test --test ignored_on_entry 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -5
```
All expected green. No XFAIL / FAIL.

- [ ] **Step P.2: Symbol sanity-check**

```bash
grep -rn 'is_ignored_on_entry\|ignored_on_entry_set' src/
grep -rn 'set_trap_with\|remove_trap_with' src/
```
Expected:
- `is_ignored_on_entry` appears in `src/signal.rs` (definition + use sites) and `src/env/traps.rs` (use in delegation closures).
- `set_trap_with` / `remove_trap_with` appear only in `src/env/traps.rs`.

- [ ] **Step P.3: TODO.md is clean**

```bash
grep -n 'POSIX Conformance Gaps\|§2.11 ignored\|§2.10.2 Rule 5' TODO.md
```
Expected: no output.

---

## Notes for the implementer

- **Order matters**: Task 1 must land before Task 2 (`set_trap_with` delegation calls into `signal::is_ignored_on_entry`). Task 3 (integration tests) requires both Task 1 and Task 2 behaviours to be observable end-to-end — running it after Task 2 is correct. Task 4 is independent of §2.11 work but is placed last in the commit stream so Task 5's TODO cleanup can reference all items as closed.
- **`libc::sigaction` field name**: on both macOS and Linux the field storing the handler pointer is `sa_sigaction` in the `libc` crate's definition. If the build complains about field access, check `target_os` and use whichever field is available; the `SIG_IGN` constant comparison is the same in both cases.
- **`pre_exec` safety**: `sigaction` is async-signal-safe, which is the only thing that runs between fork and exec in the integration test. Do not add allocation or other non-safe operations inside the `pre_exec` closure.
- **Test isolation**: unit tests in `src/signal.rs` that modify `sigaction` state for SIGUSR1/SIGUSR2 save-and-restore the original disposition. Do not add such modifications for SIGINT/SIGTERM — those are used by the integration test subprocesses and cross-test pollution is easier to miss.
- **Never use `--no-verify`** when committing; if a hook fails, diagnose before proceeding.
