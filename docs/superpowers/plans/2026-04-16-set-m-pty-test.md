# `set -m` PTY Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two PTY tests verifying signal-level behavior of `set +m` / `set -m` toggle in an interactive shell.

**Architecture:** Two independent `#[test]` functions in `tests/pty_interactive.rs` using the existing `expectrl`-based PTY framework (`spawn_kish`, `wait_for_prompt`, `expect_output`, `exit_shell`). Test 1 verifies `set +m` disables job control (indirect). Test 2 verifies `set -m` re-enables job control after toggle, including Ctrl+Z suspend (direct signal-level).

**Tech Stack:** Rust, `expectrl` crate, PTY-based integration tests

**Spec:** `docs/superpowers/specs/2026-04-16-set-m-pty-test-design.md`

---

### Task 1: `test_pty_set_plus_m_disables_job_control`

**Files:**
- Modify: `tests/pty_interactive.rs` (append test)

- [ ] **Step 1: Write the test**

Append to `tests/pty_interactive.rs`:

```rust
#[test]
fn test_pty_set_plus_m_disables_job_control() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Interactive shell starts with monitor=on; disable it
    s.send("set +m\r").unwrap();
    wait_for_prompt(&mut s);

    // fg should fail with "no job control"
    s.send("fg\r").unwrap();
    s.expect("no job control")
        .expect("fg should report 'no job control' after set +m");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test pty_interactive test_pty_set_plus_m_disables_job_control -- --nocapture`

Expected: PASS — `set +m` calls `reset_job_control_signals()` and the `fg` builtin checks the monitor flag.

- [ ] **Step 3: Commit**

```bash
git add tests/pty_interactive.rs
git commit -m "test(pty): add set +m disables job control PTY test

Verify that 'set +m' in an interactive shell disables job control:
fg reports 'no job control' after monitor mode is turned off.

Task: TODO.md 'set -m signal-level re-enable PTY test'"
```

### Task 2: `test_pty_set_minus_m_reenables_job_control`

**Files:**
- Modify: `tests/pty_interactive.rs` (append test)

- [ ] **Step 1: Write the test**

Append to `tests/pty_interactive.rs`:

```rust
#[test]
fn test_pty_set_minus_m_reenables_job_control() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Disable then re-enable monitor mode
    s.send("set +m\r").unwrap();
    wait_for_prompt(&mut s);
    s.send("set -m\r").unwrap();
    wait_for_prompt(&mut s);

    // Start a foreground job
    s.send("sleep 100\r").unwrap();
    // Brief pause to let sleep start
    std::thread::sleep(Duration::from_millis(200));

    // Ctrl+Z to suspend
    s.send("\x1a").unwrap();

    // Shell should regain control and show prompt
    wait_for_prompt(&mut s);

    // jobs should show the stopped job
    s.send("jobs\r").unwrap();
    s.expect("Stopped")
        .expect("jobs should show Stopped after Ctrl+Z suspend");
    wait_for_prompt(&mut s);

    // Cleanup: kill the stopped job
    s.send("kill %1\r").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test pty_interactive test_pty_set_minus_m_reenables_job_control -- --nocapture`

Expected: PASS — after `set +m; set -m`, `init_job_control_signals()` re-establishes SIGCHLD handling, SIGTSTP ignore (for shell), and process group management, allowing Ctrl+Z to suspend the foreground job.

- [ ] **Step 3: Commit**

```bash
git add tests/pty_interactive.rs
git commit -m "test(pty): add set -m re-enables job control PTY test

Verify that 'set +m; set -m' toggle correctly re-enables job control
at the signal level: Ctrl+Z suspends a foreground job and jobs shows
it as Stopped. This proves init_job_control_signals() is effective
after the toggle.

Task: TODO.md 'set -m signal-level re-enable PTY test'"
```

### Task 3: Remove TODO item

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the completed item from TODO.md**

Delete this line from `TODO.md`:

```
- [ ] `set -m` signal-level re-enable PTY test — verify `init_job_control_signals()` is effective after `set +m; set -m` toggle; requires interactive/PTY context (`tests/pty_interactive.rs`)
```

- [ ] **Step 2: Remove the test gap comment from `tests/parser_integration.rs`**

Delete these lines from `tests/parser_integration.rs` (lines 963-966):

```rust
// Note: Testing `set -m` signal-level restoration (init_job_control_signals)
// requires an interactive/PTY context with a controlling terminal.
// The flag toggle is verified in test_set_monitor_toggle_flag above;
// signal-level re-enable verification is deferred to PTY tests.
```

- [ ] **Step 3: Commit**

```bash
git add TODO.md tests/parser_integration.rs
git commit -m "docs(TODO): remove completed set -m PTY test item"
```

### Task 4: Final verification

- [ ] **Step 1: Run all PTY tests**

Run: `cargo test --test pty_interactive -- --nocapture`

Expected: All PTY tests pass (including the two new ones).

- [ ] **Step 2: Run full test suite**

Run: `cargo test`

Expected: All tests pass.
