# SP6 — PTY Harness Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate 10 SP6 XFAIL tests (fc / FCEDIT / PS1 / exec-redirect) from the non-interactive `e2e/run_tests.sh` harness to a new Rust PTY test file `tests/pty_posix.rs`, add `# MIGRATED_TO:` directive support to the e2e runner, and initialize `PS1` to its POSIX default at interactive shell startup.

**Architecture:** Seven sequential tasks. T1 adds runner support for the migration directive. T2 extracts PTY helpers to a shared module. T3 makes the single required yosh source change (PS1 init). T4-T6 migrate the 10 tests in three batches (fc / FCEDIT / PS1+exec-redirect). T7 closes the sub-project (TODO/memory updates, final XFail verification).

**Tech Stack:** Rust 2024 edition, `expectrl` 0.8 (PTY harness), `libc::tcgetattr` (raw-mode detection), `libc::getuid` (PS1 default choice). E2E runner is POSIX `/bin/sh`. POSIX semantics per IEEE Std 1003.1-2017.

**Spec:** [`docs/superpowers/specs/2026-05-16-e2e-xfail-sp6-pty-harness-design.md`](../specs/2026-05-16-e2e-xfail-sp6-pty-harness-design.md)

---

## File Surface (overview)

| File | Tasks touching it | Responsibility |
|------|-------------------|----------------|
| `e2e/run_tests.sh` | T1 | Add `# MIGRATED_TO:` parsing, `[MIGRATED]` reporting, `migrated` counter, summary line |
| `tests/helpers/pty.rs` | T2 (create) | Shared PTY primitives: `spawn_yosh`, `wait_for_prompt`, `wait_for_ps2`, `wait_for_raw_mode`, `read_until_prompt`, `TempDir` re-export |
| `tests/helpers/mod.rs` | T2 (modify) | Register `pub mod pty;` |
| `tests/pty_interactive.rs` | T2 (modify) | Strip inlined helpers in favor of `use helpers::pty::*;` |
| `src/interactive/mod.rs` | T3 | `Repl::new` — set PS1 default after history init |
| `tests/pty_posix.rs` | T4-T6 (create / extend) | The 10 new PTY tests |
| `e2e/posix_spec/4_required_builtin/fc_*.sh` | T4 | Replace `# XFAIL:` with `# MIGRATED_TO:` (×6) |
| `e2e/posix_spec/8_env_vars/FCEDIT_*.sh` | T5 | Replace `# XFAIL:` with `# MIGRATED_TO:` (×2) |
| `e2e/posix_spec/8_env_vars/PS1_default_value.sh` | T6 | Replace `# XFAIL:` with `# MIGRATED_TO:` |
| `e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh` | T6 | Replace `# XFAIL:` with `# MIGRATED_TO:` |
| `TODO.md` | T7 | Delete SP6 line; add `### SP6 follow-ups` if any surface |
| `MEMORY.md` + `project_e2e_xfail_roadmap.md` | T7 | Mark SP6 complete |

---

## Task 1 — G1: E2E runner `# MIGRATED_TO:` support (1 commit)

**Files:**
- Modify: `e2e/run_tests.sh:160-166` — counters block (add `migrated`)
- Modify: `e2e/run_tests.sh:172-244` — `parse_metadata` (add `meta_migrated`)
- Modify: `e2e/run_tests.sh:267-275` — early-skip branch after `parse_metadata`
- Modify: `e2e/run_tests.sh:429-436` — summary line (add `Migrated: N`)
- Test: a manually-created tmp test file with `# MIGRATED_TO:`, plus running the full suite to confirm no regression

### Steps

- [ ] **Step 1: Add `migrated` counter**

Edit `e2e/run_tests.sh` around line 166 (counters section). Replace:

```sh
# ── Counters ─────────────────────────────────────────────────────────
total=0
passed=0
failed=0
xfailed=0
xpassed=0
timedout=0
```

with:

```sh
# ── Counters ─────────────────────────────────────────────────────────
total=0
passed=0
failed=0
xfailed=0
xpassed=0
timedout=0
migrated=0
```

- [ ] **Step 2: Parse `# MIGRATED_TO:` in metadata loop**

Edit `e2e/run_tests.sh` `parse_metadata` (around L172). Find the line:

```sh
    meta_xfail=""
```

and add the next line directly below it:

```sh
    meta_migrated=""
```

Then in the `case "$_line"` switch (around L209-237), add a new arm before the closing `esac`:

```sh
            "# MIGRATED_TO: "*)
                meta_migrated="${_line#"# MIGRATED_TO: "}"
                ;;
```

The full updated arm sequence should be (with the new clause appended at the end):

```sh
        case "$_line" in
            "# POSIX_REF: "*)
                meta_posix_ref="${_line#"# POSIX_REF: "}"
                ;;
            "# DESCRIPTION: "*)
                meta_description="${_line#"# DESCRIPTION: "}"
                ;;
            "# EXPECT_OUTPUT<<"*)
                # Multi-line heredoc style: # EXPECT_OUTPUT<<DELIM
                _heredoc_delim="${_line#"# EXPECT_OUTPUT<<"}"
                _in_heredoc=1
                _heredoc_buf=""
                _heredoc_first=1
                ;;
            "# EXPECT_OUTPUT:"|"# EXPECT_OUTPUT: "*)
                meta_expect_output="${_line#"# EXPECT_OUTPUT:"}"
                meta_expect_output="${meta_expect_output# }"
                meta_has_expect_output=1
                ;;
            "# EXPECT_EXIT: "*)
                meta_expect_exit="${_line#"# EXPECT_EXIT: "}"
                ;;
            "# EXPECT_STDERR: "*)
                meta_expect_stderr="${_line#"# EXPECT_STDERR: "}"
                ;;
            "# XFAIL: "*)
                meta_xfail="${_line#"# XFAIL: "}"
                ;;
            "# MIGRATED_TO: "*)
                meta_migrated="${_line#"# MIGRATED_TO: "}"
                ;;
        esac
```

- [ ] **Step 3: Short-circuit migrated tests in the main loop**

Edit `e2e/run_tests.sh` around line 266-275 (after `parse_metadata "$test_file"` and before the `TEST_TMPDIR=$(mktemp ...)` line). Insert this block:

```sh
    # Migrated tests: short-circuit (no execution, no temp dir).
    if [ -n "$meta_migrated" ]; then
        if [ -n "$meta_xfail" ]; then
            printf "${YELLOW}[WARN]${RESET}  %s has both MIGRATED_TO and XFAIL — MIGRATED_TO wins; remove the stale XFAIL\n" "$rel_path"
        fi
        migrated=$((migrated + 1))
        printf "${CYAN}[MIGRATED]${RESET} %s (%s)\n" "$rel_path" "$meta_migrated"
        continue
    fi
```

Place this immediately after the `total=$((total + 1))` line and the `parse_metadata "$test_file"` call, before the `TEST_TMPDIR=$(mktemp ...)` block. After insertion, the relevant region should read:

```sh
    total=$((total + 1))

    # Parse metadata
    parse_metadata "$test_file"

    # Migrated tests: short-circuit (no execution, no temp dir).
    if [ -n "$meta_migrated" ]; then
        if [ -n "$meta_xfail" ]; then
            printf "${YELLOW}[WARN]${RESET}  %s has both MIGRATED_TO and XFAIL — MIGRATED_TO wins; remove the stale XFAIL\n" "$rel_path"
        fi
        migrated=$((migrated + 1))
        printf "${CYAN}[MIGRATED]${RESET} %s (%s)\n" "$rel_path" "$meta_migrated"
        continue
    fi

    # Create per-test temp directory
    TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/yosh_e2e.XXXXXX")
    export TEST_TMPDIR
```

- [ ] **Step 4: Add `Migrated: N` to the summary line**

Edit `e2e/run_tests.sh` around line 429-436 (summary block). Replace:

```sh
printf "Total: %d  " "$total"
printf "${GREEN}Passed: %d${RESET}  " "$passed"
printf "${RED}Failed: %d${RESET}  " "$failed"
printf "${YELLOW}Timedout: %d${RESET}  " "$timedout"
printf "${CYAN}XFail: %d${RESET}  " "$xfailed"
printf "${YELLOW}XPass: %d${RESET}\n" "$xpassed"
```

with:

```sh
printf "Total: %d  " "$total"
printf "${GREEN}Passed: %d${RESET}  " "$passed"
printf "${RED}Failed: %d${RESET}  " "$failed"
printf "${YELLOW}Timedout: %d${RESET}  " "$timedout"
printf "${CYAN}XFail: %d${RESET}  " "$xfailed"
printf "${CYAN}Migrated: %d${RESET}  " "$migrated"
printf "${YELLOW}XPass: %d${RESET}\n" "$xpassed"
```

- [ ] **Step 5: Manually test the new directive**

Create a tmp test file to verify the path:

```bash
mkdir -p /tmp/sp6-runner-check
cat >/tmp/sp6-runner-check/migrated_demo.sh <<'EOF'
#!/bin/sh
# POSIX_REF: demo
# DESCRIPTION: demo
# MIGRATED_TO: tests/pty_posix.rs::demo
# EXPECT_EXIT: 0
echo "this body must not execute"
EOF
chmod 644 /tmp/sp6-runner-check/migrated_demo.sh
```

Then run the runner against just that one file:

```bash
E2E_DIR=/tmp/sp6-runner-check ./e2e/run_tests.sh 2>&1 | tail -10
```

Expected output contains `[MIGRATED] migrated_demo.sh (tests/pty_posix.rs::demo)` and a summary line with `Migrated: 1`. The body `echo "this body must not execute"` MUST NOT appear in the output.

Cleanup:

```bash
rm -rf /tmp/sp6-runner-check
```

- [ ] **Step 6: Run the full e2e suite — verify no regression**

```bash
cargo build && ./e2e/run_tests.sh 2>&1 | tail -5
```

Expected: same summary as before this task (zero `Migrated`), no failures introduced. Note the final line ends with `Migrated: 0  XPass: 0` showing the new counter is wired but no test has been migrated yet.

- [ ] **Step 7: Commit**

```bash
git add e2e/run_tests.sh
git commit -m "$(cat <<'EOF'
feat(e2e/runner): add # MIGRATED_TO: directive for PTY-migrated tests

When a test header carries `# MIGRATED_TO: <pointer>`, the runner skips
execution and reports `[MIGRATED]` instead. A new `Migrated: N` counter
appears in the summary line. If both `# MIGRATED_TO:` and `# XFAIL:`
are present a `[WARN]` is emitted so stale XFAIL comments do not
silently survive a migration.

Prep for SP6 (PTY harness migration) which routes 10 XFAIL tests to
tests/pty_posix.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — G2: Extract PTY helpers to `tests/helpers/pty.rs` (1 commit)

**Files:**
- Create: `tests/helpers/pty.rs`
- Modify: `tests/helpers/mod.rs:5` — add `pub mod pty;`
- Modify: `tests/pty_interactive.rs:1-105` — remove inlined helpers, import from `helpers::pty`

### Steps

- [ ] **Step 1: Read existing helpers in `tests/pty_interactive.rs`**

The functions to extract live at:
- `TIMEOUT` / `RAW_MODE_WAIT_TIMEOUT` constants (top of file)
- `TempDir` (inlined struct + impl) — supersede with `helpers::TempDir` (already exists at `tests/helpers/mod.rs`)
- `spawn_yosh()` (around L46-58)
- `wait_for_prompt()` (around L60-63)
- `wait_for_ps2()` (around L65-68)
- `wait_for_raw_mode()` (around L83-105)

The `expect_output`, `exit_shell`, `TimeoutGuard`, `drain_pty_buffer`, and `suspend_fg_job` helpers stay in `tests/pty_interactive.rs` because they are only used by interactive REPL editing tests, not by SP6 POSIX-spec tests.

- [ ] **Step 2: Create `tests/helpers/pty.rs`**

```rust
//! Shared PTY helpers for integration tests that drive yosh through a
//! pseudo-terminal (`tests/pty_interactive.rs`, `tests/pty_posix.rs`).
//!
//! The constants and helpers here are the minimal surface that both
//! consumers need: spawning yosh under expectrl, synchronizing on the
//! prompt + raw-mode transition, and reading delimited output. Test-
//! specific helpers (UI-edit-action drivers, suspend/resume sequences,
//! ANSI strippers for syntax-highlight tests) stay in their owning
//! file.

use std::os::fd::AsRawFd;
use std::process::Command;
use std::time::{Duration, Instant};

use expectrl::{Expect, Regex, Session, session::OsSession};

use super::TempDir;

pub const TIMEOUT: Duration = Duration::from_secs(15);
pub const RAW_MODE_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Spawn yosh under a pseudo-terminal with `TERM=dumb` and `HOME` pointing
/// at a fresh per-test temp directory. Returns the expectrl session plus
/// the temp dir (which must outlive the session — drop frees `HOME`).
pub fn spawn_yosh() -> (OsSession, TempDir) {
    let bin = env!("CARGO_BIN_EXE_yosh");
    let tmpdir = TempDir::new();

    let mut cmd = Command::new(bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());

    let mut session = Session::spawn(cmd).expect("failed to spawn yosh");
    session.set_expect_timeout(Some(TIMEOUT));
    (session, tmpdir)
}

/// Variant of [`spawn_yosh`] that allows the caller to override or remove
/// environment variables before exec. Used by tests that need to start with
/// `PS1` absent from the environment, an explicit `FCEDIT` value, etc.
pub fn spawn_yosh_with_env(overrides: &[(&str, Option<&str>)]) -> (OsSession, TempDir) {
    let bin = env!("CARGO_BIN_EXE_yosh");
    let tmpdir = TempDir::new();

    let mut cmd = Command::new(bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());
    for (k, v) in overrides {
        match v {
            Some(value) => {
                cmd.env(k, value);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }

    let mut session = Session::spawn(cmd).expect("failed to spawn yosh");
    session.set_expect_timeout(Some(TIMEOUT));
    (session, tmpdir)
}

/// Wait until yosh prints `$ ` and has switched the slave PTY to raw mode.
pub fn wait_for_prompt(session: &mut OsSession) {
    session.expect("$ ").expect("prompt not found");
    wait_for_raw_mode(session);
}

/// Wait for the PS2 (`> `) continuation prompt.
pub fn wait_for_ps2(session: &mut OsSession) {
    session.expect("> ").expect("PS2 prompt not found");
    wait_for_raw_mode(session);
}

/// Block until the slave PTY has `ICANON` cleared in its termios — i.e.
/// yosh's LineEditor has finished switching to raw mode. Both ends of a
/// PTY share one termios struct, so `tcgetattr` on the master fd works.
pub fn wait_for_raw_mode(session: &OsSession) {
    let fd = session.as_raw_fd();
    let deadline = Instant::now() + RAW_MODE_WAIT_TIMEOUT;
    loop {
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::tcgetattr(fd, &mut termios) };
        if rc == 0 && (termios.c_lflag & (libc::ICANON as libc::tcflag_t)) == 0 {
            return;
        }
        if Instant::now() >= deadline {
            let errno = if rc != 0 {
                std::io::Error::last_os_error().to_string()
            } else {
                "ok".to_string()
            };
            panic!(
                "wait_for_raw_mode timed out: tcgetattr rc={} ({}), c_lflag=0x{:x}",
                rc, errno, termios.c_lflag,
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Read output that arrives between the previously-sent command and the
/// next `$ ` prompt, returning the captured text with the trailing prompt
/// stripped.
///
/// Use this after `session.send_line("...")` to capture the command's
/// stdout. Caller is responsible for asserting on the returned string —
/// note that the captured output includes the command echo at the start
/// (e.g. `"echo foo\r\nfoo\r\n"`), and assertions should typically be
/// substring (`out.contains("foo")`) rather than equality.
pub fn read_until_prompt(session: &mut OsSession) -> String {
    let captured = session
        .expect(Regex(r"\$ "))
        .expect("next prompt not found");
    String::from_utf8_lossy(captured.before()).into_owned()
}
```

- [ ] **Step 3: Register the new module in `tests/helpers/mod.rs`**

Edit `tests/helpers/mod.rs:5`. Replace:

```rust
pub mod mock_terminal;
```

with:

```rust
pub mod mock_terminal;
pub mod pty;
```

- [ ] **Step 4: Refactor `tests/pty_interactive.rs` to use the shared helpers**

Replace the top of `tests/pty_interactive.rs` (lines 1-105) with this header. The remaining file (from `expect_output` at L107 onwards) stays unchanged.

```rust
mod helpers;

use std::time::Duration;

use expectrl::{Eof, Expect, Regex, session::OsSession};

use helpers::pty::{
    RAW_MODE_WAIT_TIMEOUT, TIMEOUT, spawn_yosh, wait_for_prompt, wait_for_ps2,
    wait_for_raw_mode,
};
use helpers::TempDir;

// (existing `expect_output`, `exit_shell`, `TimeoutGuard`, `drain_pty_buffer`,
//  `suspend_fg_job`, and all #[test] functions follow unchanged starting at
//  the original L107.)
```

Specifically: remove the inlined `TempDir` (original L11-42), `spawn_yosh` (L46-58), `wait_for_prompt` (L60-63), `wait_for_ps2` (L65-68), `wait_for_raw_mode` (L83-105), `TIMEOUT` / `RAW_MODE_WAIT_TIMEOUT` constants (L7-8), and the `TEMP_DIR_COUNTER` static (L13-14). Replace with the `mod helpers; use ...;` header above.

The `std::os::fd::AsRawFd` / `std::path::PathBuf` / `std::process::Command` / `AtomicU64` imports at the top are no longer needed (they served only the inlined helpers); strip them.

- [ ] **Step 5: Build the test binaries to verify compilation**

```bash
cargo test --no-run --test pty_interactive 2>&1 | tail -20
```

Expected: builds cleanly. If a previously-inlined helper turns out to be referenced by an unmoved function, add it back to `tests/helpers/pty.rs` and re-export.

- [ ] **Step 6: Run the existing PTY interactive tests — verify no regression**

```bash
cargo test --test pty_interactive 2>&1 | tail -10
```

Expected: same pass count as before this task. Note the test count may be 30+ and execution takes ~30-60 seconds.

- [ ] **Step 7: Commit**

```bash
git add tests/helpers/mod.rs tests/helpers/pty.rs tests/pty_interactive.rs
git commit -m "$(cat <<'EOF'
refactor(tests): extract PTY helpers to tests/helpers/pty.rs

Moves spawn_yosh, wait_for_prompt, wait_for_ps2, wait_for_raw_mode,
and the PTY-side timeouts out of tests/pty_interactive.rs into a
shared module so the upcoming tests/pty_posix.rs (SP6) can reuse them
without copy-paste.

Adds spawn_yosh_with_env() and read_until_prompt() — needed by SP6 to
strip PS1 from the inherited environment and to capture command output
between two prompts.

No behavior change. tests/pty_interactive.rs is unchanged below the
helper section; it just imports from helpers::pty instead of defining
locally.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 — G3: PS1 default value at interactive startup (1 commit)

**Files:**
- Modify: `src/interactive/mod.rs:68-77` — `Repl::new` history-defaults block
- Test: integration test in `tests/pty_posix.rs` will cover this in T6; no unit test added here (no clean way to instantiate `Repl` from a unit test without a real PTY)

### Steps

- [ ] **Step 1: Add PS1 default initialization**

Edit `src/interactive/mod.rs` between L74 (`HISTCONTROL` set) and L77 (`executor.env.history.load(...)`). Insert this block:

```rust
        // POSIX XCU §2.5.3: PS1 has a default value for interactive shells.
        // Set it as a real variable so observers like `[ -n "${PS1+x}" ]`
        // see it. Defer to inherited / rc-set value if already present.
        if executor.env.vars.get("PS1").is_none() {
            // SAFETY: getuid() is always safe to call.
            let default = if unsafe { libc::getuid() } == 0 {
                "# "
            } else {
                "$ "
            };
            let _ = executor.env.vars.set("PS1", default);
        }
```

After insertion, the relevant region of `Repl::new` should read:

```rust
        // Set history variable defaults
        let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
        let histfile = format!("{}/.yosh_history", home);
        let _ = executor.env.vars.set("HISTFILE", &histfile);
        let _ = executor.env.vars.set("HISTSIZE", "500");
        let _ = executor.env.vars.set("HISTFILESIZE", "500");
        let _ = executor.env.vars.set("HISTCONTROL", "ignoreboth");

        // POSIX XCU §2.5.3: PS1 has a default value for interactive shells.
        // Set it as a real variable so observers like `[ -n "${PS1+x}" ]`
        // see it. Defer to inherited / rc-set value if already present.
        if executor.env.vars.get("PS1").is_none() {
            // SAFETY: getuid() is always safe to call.
            let default = if unsafe { libc::getuid() } == 0 {
                "# "
            } else {
                "$ "
            };
            let _ = executor.env.vars.set("PS1", default);
        }

        // Load history from file
        executor.env.history.load(std::path::Path::new(&histfile));
```

- [ ] **Step 2: Verify `libc` is already in scope**

Search the file:

```bash
grep -n "use libc\|libc::" src/interactive/mod.rs | head -5
```

If `libc` is not yet imported in `src/interactive/mod.rs`, the inline `libc::getuid()` call still works because `libc` is a workspace dependency referenced via fully-qualified path in the new code. If the build fails on `libc` unresolved, add `use libc;` near the top of the file. (`src/interactive/prompt.rs:13` uses the bare `libc::getuid` form successfully so the dependency is present.)

- [ ] **Step 3: Build**

```bash
cargo build 2>&1 | tail -5
```

Expected: clean build. Time: 30-90s on warm cache.

- [ ] **Step 4: Run the existing test suite to check for regressions**

```bash
cargo test 2>&1 | tail -15
```

Expected: same pass count as before this task. PS1 is set late enough that it doesn't disturb plugin tests, signal tests, etc.

- [ ] **Step 5: Run `tests/pty_interactive.rs` to confirm PTY-side regressions are zero**

```bash
cargo test --test pty_interactive 2>&1 | tail -10
```

Expected: same pass count as Task 2 step 6.

- [ ] **Step 6: Smoke-test manually under interactive shell**

```bash
echo '[ -n "${PS1+x}" ] && echo SET; printf "PS1=[%s]\n" "$PS1"; exit' | ./target/debug/yosh
```

Note: this is a `-c`-style invocation through stdin which is non-interactive; PS1 will still be unset. To actually verify the change, run yosh interactively:

```bash
./target/debug/yosh
# At the prompt, type:
# [ -n "${PS1+x}" ] && echo SET || echo UNSET
# printf "PS1=[%s]\n" "$PS1"
# exit
```

Expected lines in the captured terminal:
```
SET
PS1=[$ ]
```

(The default prompt `$ ` is what gets echoed for `$PS1`.)

- [ ] **Step 7: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "$(cat <<'EOF'
feat(interactive): initialize PS1 to POSIX default at startup

Sets PS1 to "$ " (or "# " for uid 0) in Repl::new when not inherited
from the environment, so `[ -n "${PS1+x}" ]` and similar introspection
returns true on a freshly-started interactive shell. Matches POSIX
XCU §2.5.3 which lists PS1 as a shell-initialized variable.

Previously yosh only synthesized "$ " at prompt-render time via
src/interactive/prompt.rs::default_prompt — the variable itself was
never set, so scripts that observed PS1 saw "unset".

Inherited PS1 / ~/.yoshrc-set PS1 still wins (is_none() guard, rc
sourced after this block).

Unblocks SP6 test #9 (PS1_default_value) under the PTY harness.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — G4: Migrate the 6 `fc` tests (1 commit)

**Files:**
- Create: `tests/pty_posix.rs` — scaffold + 6 fc tests
- Modify: 6 files under `e2e/posix_spec/4_required_builtin/fc_*.sh` — swap `# XFAIL:` for `# MIGRATED_TO:`

### Steps

- [ ] **Step 1: Scaffold `tests/pty_posix.rs`**

Create the file with this top-of-file boilerplate:

```rust
//! POSIX-spec PTY-driven tests migrated from e2e/posix_spec/*.
//!
//! Each test corresponds to one e2e/posix_spec/.../foo.sh file. The
//! original shell file is retained as a stub with the directive
//! `# MIGRATED_TO: tests/pty_posix.rs::<test_path>` so readers
//! arriving at the POSIX spec layout find the Rust test, and so the
//! e2e runner accounts for it under `Migrated: N`.
//!
//! Why PTY: these tests depend on interactive history, an editor
//! process, the default PS1, or /dev/tty — none of which is
//! available to the non-interactive e2e runner.

mod helpers;

use expectrl::{Eof, Expect};

use helpers::pty::{
    read_until_prompt, spawn_yosh, spawn_yosh_with_env, wait_for_prompt,
};
```

- [ ] **Step 2: Add the `fc` module with the 6 tests**

Append to `tests/pty_posix.rs`:

```rust
mod fc {
    use super::*;

    /// Seed three commands into history and return after the third prompt.
    fn seed_three(session: &mut expectrl::session::OsSession) {
        for cmd in ["echo aaa", "echo bbb", "echo ccc"] {
            session.send_line(cmd).unwrap();
            // Drain the command's own output before the next prompt.
            let _ = read_until_prompt(session);
        }
    }

    #[test]
    fn list_recent() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);
        seed_three(&mut session);

        session.send_line("fc -l").unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("echo aaa"), "missing 'echo aaa' in: {:?}", out);
        assert!(out.contains("echo bbb"), "missing 'echo bbb' in: {:?}", out);
        assert!(out.contains("echo ccc"), "missing 'echo ccc' in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn list_no_numbers() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);
        seed_three(&mut session);

        session.send_line("fc -l -n").unwrap();
        let out = read_until_prompt(&mut session);

        // -n suppresses leading line numbers; output lines start with a tab
        // (per src/builtin/special.rs::fc_list).
        assert!(out.contains("echo aaa"), "missing 'echo aaa' in: {:?}", out);
        assert!(out.contains("echo bbb"), "missing 'echo bbb' in: {:?}", out);
        assert!(out.contains("echo ccc"), "missing 'echo ccc' in: {:?}", out);
        // Look for a tab-prefixed entry to confirm -n's no-number formatting.
        assert!(out.contains("\techo aaa"), "expected '\\techo aaa' in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn list_reverse() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);
        seed_three(&mut session);

        session.send_line("fc -l -r").unwrap();
        let out = read_until_prompt(&mut session);

        // Reverse order: ccc should appear before aaa.
        let i_aaa = out.find("echo aaa").expect("echo aaa not found");
        let i_ccc = out.find("echo ccc").expect("echo ccc not found");
        assert!(i_ccc < i_aaa, "expected ccc before aaa in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn substitute() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        session.send_line("echo onevar").unwrap();
        let _ = read_until_prompt(&mut session);

        session.send_line("fc -s one=two echo").unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("twovar"), "expected 'twovar' in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn editor_dash_e() {
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        session.send_line("echo seedline").unwrap();
        let _ = read_until_prompt(&mut session);

        // `cat` reads the tempfile (no edits), exits 0; fc then re-executes.
        // We use </dev/null and >/dev/null to mute re-execution side effects;
        // only the exit status matters.
        session
            .send_line("fc -e cat </dev/null >/dev/null 2>&1; echo RC=$?")
            .unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }

    #[test]
    fn no_args_uses_editor() {
        // Bare `fc` with FCEDIT=cat: cat reads tempfile, exits 0; fc
        // re-executes the previous command. We check exit status only.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        session.send_line("export FCEDIT=cat").unwrap();
        let _ = read_until_prompt(&mut session);
        session.send_line("echo seedline").unwrap();
        let _ = read_until_prompt(&mut session);

        session
            .send_line("fc </dev/null >/dev/null 2>&1; echo RC=$?")
            .unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}
```

- [ ] **Step 3: Build the new test binary**

```bash
cargo test --no-run --test pty_posix 2>&1 | tail -20
```

Expected: builds cleanly. Fix any import / method-name errors before continuing.

- [ ] **Step 4: Run the 6 fc tests**

```bash
cargo test --test pty_posix fc:: 2>&1 | tail -20
```

Expected: 6 tests pass. If any test fails:
- Look at the assertion message — it includes the captured PTY output.
- Likely failure modes:
  - History not seeded — the `seed_three` helper didn't wait long enough between commands. Check that `read_until_prompt` returns before the next `send_line`.
  - `fc -s` doesn't produce `twovar` — debug by sending `echo $?` after `fc -s` and checking exit status; verify the substitute logic in `src/builtin/special.rs::fc_substitute`.

- [ ] **Step 5: Migrate the 6 e2e shell test files**

For each of the 6 files below, perform the same edit pattern: replace the `# XFAIL: …` line with `# MIGRATED_TO: tests/pty_posix.rs::fc::<name>`.

`e2e/posix_spec/4_required_builtin/fc_l_lists_recent.sh`:
- Replace `# XFAIL: harness limitation (fc requires non-empty history; non-interactive harness has no history)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fc::list_recent`

`e2e/posix_spec/4_required_builtin/fc_l_n_no_numbers.sh`:
- Replace `# XFAIL: harness limitation (fc requires non-empty history; non-interactive harness has no history)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fc::list_no_numbers`

`e2e/posix_spec/4_required_builtin/fc_r_reverse.sh`:
- Replace `# XFAIL: harness limitation (fc requires non-empty history; non-interactive harness has no history)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fc::list_reverse`

`e2e/posix_spec/4_required_builtin/fc_s_substitute.sh`:
- Replace `# XFAIL: harness limitation (fc -s substitution may rely on interactive history capture)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fc::substitute`

`e2e/posix_spec/4_required_builtin/fc_e_editor.sh`:
- Replace `# XFAIL: harness limitation (fc -e relies on launching an editor)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fc::editor_dash_e`

`e2e/posix_spec/4_required_builtin/fc_no_command.sh`:
- Replace `# XFAIL: harness limitation (fc editor invocation needs an interactive context)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fc::no_args_uses_editor`

- [ ] **Step 6: Verify the e2e runner reports them as Migrated**

```bash
./e2e/run_tests.sh --filter=fc_ 2>&1 | tail -20
```

Expected: 6 `[MIGRATED]` lines, summary `Migrated: 6`, no `[FAIL]`.

- [ ] **Step 7: Commit**

```bash
git add tests/pty_posix.rs \
    e2e/posix_spec/4_required_builtin/fc_l_lists_recent.sh \
    e2e/posix_spec/4_required_builtin/fc_l_n_no_numbers.sh \
    e2e/posix_spec/4_required_builtin/fc_r_reverse.sh \
    e2e/posix_spec/4_required_builtin/fc_s_substitute.sh \
    e2e/posix_spec/4_required_builtin/fc_e_editor.sh \
    e2e/posix_spec/4_required_builtin/fc_no_command.sh
git commit -m "$(cat <<'EOF'
test(sp6): migrate 6 fc XFAIL tests to PTY harness

Adds tests/pty_posix.rs with the fc module covering: list_recent,
list_no_numbers, list_reverse, substitute, editor_dash_e (fc -e cat),
and no_args_uses_editor (FCEDIT=cat). Each test spawns an interactive
yosh under expectrl, seeds history with echo commands, runs the fc
variant, and verifies output / exit status.

Replaces # XFAIL lines in the 6 e2e shell files with # MIGRATED_TO
pointers so the non-interactive runner accounts for them under
Migrated: N and readers in the POSIX spec layout can find the Rust
test.

Closes 6 of 10 SP6 tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 — G5: Migrate the 2 FCEDIT tests (1 commit, possibly + SP7 demotion)

**Files:**
- Modify: `tests/pty_posix.rs` — add `fcedit` module with 2 tests
- Modify: `e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh`
- Modify: `e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh`

### Steps

- [ ] **Step 1: Add the `fcedit::used_by_fc` test**

Append to `tests/pty_posix.rs` after the `mod fc { ... }` block:

```rust
mod fcedit {
    use super::*;

    #[test]
    fn used_by_fc() {
        // FCEDIT=cat → bare `fc` invokes cat as editor → cat reads
        // tempfile, exits 0 → fc re-executes the previous command.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        session.send_line("FCEDIT=cat").unwrap();
        let _ = read_until_prompt(&mut session);
        session.send_line("echo seedline").unwrap();
        let _ = read_until_prompt(&mut session);

        session
            .send_line("fc </dev/null >/dev/null 2>&1; echo RC=$?")
            .unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}
```

- [ ] **Step 2: Build and run `used_by_fc`**

```bash
cargo test --test pty_posix fcedit::used_by_fc 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 3: Probe `/bin/ed </dev/null` behavior on this platform**

Before writing the `default_ed` test, verify what `/bin/ed` does when launched with the fc tempfile and stdin redirected from `/dev/null`:

```bash
T=$(mktemp); echo "echo from-fc" > "$T"; /bin/ed "$T" </dev/null; echo "ED_RC=$?"; rm "$T"
```

Three possible outcomes:
- **A.** ed exits 0 silently (the test as designed works).
- **B.** ed prints `?` and exits non-zero (ed didn't like empty stdin or the file format).
- **C.** ed hangs waiting for input (no EOF on /dev/null somehow — unlikely).

Record the outcome. The next step branches on it.

- [ ] **Step 4: Implement `default_ed` based on probe result**

**If outcome A (ed exits 0):** append to the `fcedit` module:

```rust
    #[test]
    fn default_ed() {
        // FCEDIT and EDITOR removed → fc falls back to /bin/ed.
        let (mut session, _tmpdir) = spawn_yosh_with_env(&[
            ("FCEDIT", None),
            ("EDITOR", None),
        ]);
        wait_for_prompt(&mut session);

        session.send_line("echo seedline").unwrap();
        let _ = read_until_prompt(&mut session);

        session
            .send_line("fc </dev/null >/dev/null 2>&1; echo RC=$?")
            .unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
```

**If outcome B (ed exits non-zero):** the test cannot be made green without changing `fc`'s default editor. Per the design spec §6 risk paragraph and roadmap §5.4 escape hatch, demote this test to SP7. In `tests/pty_posix.rs`, do NOT add `default_ed`. Instead, leave `e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh` as-is for now and rewrite its XFAIL line in step 5b below.

**If outcome C (ed hangs):** treat as outcome B; demote to SP7.

- [ ] **Step 5a: (Outcome A) Migrate both shell files**

`e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh`:
- Replace `# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fcedit::used_by_fc`

`e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh`:
- Replace `# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fcedit::default_ed`

Skip step 5b.

- [ ] **Step 5b: (Outcome B/C) Migrate one file, demote the other to SP7**

`e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh`:
- Replace `# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)`
- With `# MIGRATED_TO: tests/pty_posix.rs::fcedit::used_by_fc`

`e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh`:
- Replace `# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)`
- With `# XFAIL: deferred (/bin/ed </dev/null exit status varies across platforms — tracked in TODO.md)`

Also append to `TODO.md` (under the `## Future: POSIX Conformance Bugs` section, or create a new `### SP6 follow-ups (non-blocking)` section if more than one item surfaces during this SP):

```
- [ ] `fc` default editor fallback uses `/bin/ed`, whose exit status with
      empty stdin (`</dev/null`) varies across platforms. POSIX leaves the
      default editor implementation-defined; bash uses `${EDITOR:-${VISUAL:-vi}}`
      and most users override via FCEDIT. Either change yosh's fallback to
      a guaranteed-zero editor (`true` or `vi -c q`), or document the
      platform dependency. Currently demoted from SP6 to SP7
      (`e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh`).
```

- [ ] **Step 6: Run the affected tests**

```bash
cargo test --test pty_posix fcedit:: 2>&1 | tail -10
./e2e/run_tests.sh --filter=FCEDIT 2>&1 | tail -10
```

Expected outcome A: 2 PASS, 2 `[MIGRATED]`.
Expected outcome B/C: 1 PASS, 1 `[MIGRATED]`, 1 `[XFAIL]`.

- [ ] **Step 7: Commit**

```bash
git add tests/pty_posix.rs \
    e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh \
    e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh
# If outcome B/C also touched TODO.md:
# git add TODO.md
git commit -m "$(cat <<'EOF'
test(sp6): migrate FCEDIT tests to PTY harness

FCEDIT=cat → fc uses cat as the editor. Verified under the PTY harness
by spawning interactive yosh, exporting FCEDIT, seeding one history
entry, and running bare `fc` with redirected stdio.

[Outcome A note:] /bin/ed </dev/null exits 0 on this platform, so the
default-ed fallback test also migrates cleanly.

[Outcome B/C note:] /bin/ed </dev/null exits non-zero on this
platform; FCEDIT_default_ed demoted to SP7 per the roadmap escape
hatch, recorded in TODO.md.

Closes [1 or 2] of the remaining 4 SP6 tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Replace the bracketed outcome note with whichever applies; delete the other.

---

## Task 6 — G6: Migrate PS1 + exec-redirect tests (1 commit)

**Files:**
- Modify: `tests/pty_posix.rs` — add `ps1` and `exec_redirect` modules
- Modify: `e2e/posix_spec/8_env_vars/PS1_default_value.sh`
- Modify: `e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh`

### Steps

- [ ] **Step 1: Add the `ps1::default_value_set` test**

Append to `tests/pty_posix.rs` after the `mod fcedit` block:

```rust
mod ps1 {
    use super::*;

    #[test]
    fn default_value_set() {
        // Start with PS1 stripped from the inherited env so yosh's
        // Repl::new must be the one to set it. The is_none() guard in
        // src/interactive/mod.rs ensures the default value is used.
        let (mut session, _tmpdir) = spawn_yosh_with_env(&[("PS1", None)]);
        wait_for_prompt(&mut session);

        session
            .send_line(r#"[ -n "${PS1+x}" ] && echo SET || echo UNSET"#)
            .unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("SET"), "PS1 not set after startup; got: {:?}", out);
        assert!(!out.contains("UNSET"), "PS1 reported UNSET in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}
```

- [ ] **Step 2: Add the `exec_redirect::no_cmd_redirects` test**

Append to `tests/pty_posix.rs` after the `mod ps1` block:

```rust
mod exec_redirect {
    use super::*;

    #[test]
    fn no_cmd_redirects() {
        // POSIX 2.14.10: bare `exec` with redirections applies them to
        // the current shell. After `exec >/file`, subsequent stdout
        // lands in /file. Restoring with `exec >/dev/tty` requires
        // /dev/tty to be available — i.e., the shell must run under a
        // PTY.
        let (mut session, tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        // Export the tmpdir path so the test script can reference it
        // via $TEST_TMPDIR.
        let tmp = tmpdir.path().to_string_lossy().to_string();
        session
            .send_line(&format!("export TEST_TMPDIR={}", tmp))
            .unwrap();
        let _ = read_until_prompt(&mut session);

        session
            .send_line(r#"exec >"$TEST_TMPDIR/out""#)
            .unwrap();
        let _ = read_until_prompt(&mut session);
        session.send_line("echo persistent").unwrap();
        let _ = read_until_prompt(&mut session);
        session
            .send_line("exec >/dev/tty 2>/dev/null || exec >&-")
            .unwrap();
        let _ = read_until_prompt(&mut session);
        session.send_line(r#"cat "$TEST_TMPDIR/out""#).unwrap();
        let out = read_until_prompt(&mut session);

        assert!(out.contains("persistent"), "missing 'persistent' in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
}
```

- [ ] **Step 3: Build and run the new tests**

```bash
cargo test --test pty_posix ps1:: 2>&1 | tail -10
cargo test --test pty_posix exec_redirect:: 2>&1 | tail -10
```

Expected: 2 PASS.

If `ps1::default_value_set` fails with `UNSET`, T3 didn't land cleanly — double-check `src/interactive/mod.rs::Repl::new` has the PS1 block placed before any code that would call into the prompt.

If `exec_redirect::no_cmd_redirects` fails because `cat` doesn't print "persistent", debug by:
- Removing the `exec >/dev/tty` restore step and changing `cat` to send to stderr (e.g., `cat $TEST_TMPDIR/out >&2`), to isolate whether the file actually contains "persistent" or whether `/dev/tty` restoration is the problem.

- [ ] **Step 4: Migrate the 2 shell files**

`e2e/posix_spec/8_env_vars/PS1_default_value.sh`:
- Replace `# XFAIL: harness limitation (PS1 default value is not exposed when invoked via -c on non-interactive shell)`
- With `# MIGRATED_TO: tests/pty_posix.rs::ps1::default_value_set`

`e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh`:
- Replace `# XFAIL: harness limitation (/dev/tty unavailable in non-interactive test environment)`
- With `# MIGRATED_TO: tests/pty_posix.rs::exec_redirect::no_cmd_redirects`

- [ ] **Step 5: Verify under the e2e runner**

```bash
./e2e/run_tests.sh --filter=PS1_default 2>&1 | tail -5
./e2e/run_tests.sh --filter=exec_no_cmd 2>&1 | tail -5
```

Expected: each shows 1 `[MIGRATED]`.

- [ ] **Step 6: Commit**

```bash
git add tests/pty_posix.rs \
    e2e/posix_spec/8_env_vars/PS1_default_value.sh \
    e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh
git commit -m "$(cat <<'EOF'
test(sp6): migrate PS1 default + exec-redirect tests to PTY harness

ps1::default_value_set spawns yosh with PS1 stripped from the inherited
env and verifies that ${PS1+x} returns "x" — i.e., the variable was
set by Repl::new's POSIX-default initialization (added in the
preceding commit).

exec_redirect::no_cmd_redirects runs the POSIX 2.14.10 sequence
(exec >file; echo; exec >/dev/tty; cat file) under a real PTY so
/dev/tty resolves to the slave terminal.

Closes the final SP6 tests (2 of 4 remaining after fc + fcedit).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7 — G7: Closure (1 commit)

**Files:**
- Modify: `TODO.md` — remove SP6 line; add `### SP6 follow-ups (non-blocking)` section if any items surfaced
- Modify: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md` — mark SP6 complete
- (no source changes)

### Steps

- [ ] **Step 1: Verify final test state with a full run**

```bash
cargo build && ./e2e/run_tests.sh 2>&1 | tail -5
```

Expected summary line (assuming outcome A in T5 and no other demotions):
```
Total: NNN  Passed: PPP  Failed: 0  Timedout: 0  XFail: 3  Migrated: 10  XPass: 0
```

If outcome B/C in T5: `XFail: 4, Migrated: 9`.

```bash
cargo test 2>&1 | tail -5
```

Expected: all green.

```bash
cargo test --test pty_posix 2>&1 | tail -10
```

Expected: 9 or 10 PASS (matches the Migrated count).

- [ ] **Step 2: Remove the SP6 line from TODO.md**

Edit `TODO.md`. In the `## E2E XFAIL Roadmap` section, delete the line:

```
- [ ] SP6 — PTY harness migration (10 tests)
```

The roadmap section after deletion should retain only the SP7 line:

```
## E2E XFAIL Roadmap

Decomposition of 55 XFAIL tests into 7 sub-projects. See
`docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`.

- [ ] SP7 — Deferred / recorded as known deviation (3 tests)
```

- [ ] **Step 3: Add SP6 follow-ups section to TODO.md if applicable**

Review the work in T1-T6. If any non-blocking polish items surfaced (e.g., flaky test timing observations, code-review comments to chase later), add a section between `### SP5 follow-ups` and `## Job Control: Known Limitations`:

```markdown
### SP6 follow-ups (non-blocking)

- [ ] <item 1, one short paragraph including file path>
- [ ] <item 2, …>
```

If T5 demoted `FCEDIT_default_ed.sh`, the demotion entry already lives in `## Future: POSIX Conformance Bugs` (added in T5 step 5b); a duplicate is not needed under `### SP6 follow-ups`.

If absolutely nothing surfaced, skip this step. (SP5 example: 7 items. SP1: 6 items. Some items are normal; zero items is rare but acceptable.)

- [ ] **Step 4: Update the memory entry**

Edit `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`. Replace:

```
- **SP6 pending**: 10 tests — PTY harness migration (fc/FCEDIT/PS1/exec_no_cmd_redirects). Spec needed.
```

with:

```
- **SP6 COMPLETE** (2026-05-16): 10 tests — PTY harness migration (fc/FCEDIT/PS1/exec_no_cmd_redirects). Migrated to tests/pty_posix.rs under expectrl. Spec `2026-05-16-e2e-xfail-sp6-pty-harness-design.md`. Plan `2026-05-16-e2e-xfail-sp6-pty-harness.md`. 7 commits (1 runner + 1 helpers + 1 PS1 + 3 migration batches + 1 closure). Added `# MIGRATED_TO:` directive to e2e/run_tests.sh and initialized PS1 to POSIX default in Repl::new. [Outcome note if applicable: FCEDIT_default_ed demoted to SP7 — /bin/ed </dev/null exit varies across platforms.] Follow-ups under `### SP6 follow-ups (non-blocking)` in TODO.md.
```

Update the description frontmatter (line 3 of that file):
```
description: "55-XFAIL-test decomposition roadmap; SP1+SP2+SP3+SP4+SP5+SP6 complete (2026-05-16), 3 XFails remain across SP7"
```

(Adjust the XFails count to 4 if outcome B/C in T5.)

Update the closing status header inside the file:
```
**Status (as of 2026-05-16):**
```

And the count-formula line:
```
After SP1+SP2+SP3+SP4+SP5+SP6: 55 - 11 - 5 - 9 - 9 - 8 - 10 = 3 XFails remain (matches `./e2e/run_tests.sh` baseline output `XFail: 3`).
```

- [ ] **Step 5: Update `MEMORY.md` index line**

Edit `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/MEMORY.md`. Replace the existing E2E roadmap line:

```
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1+SP2+SP3+SP4+SP5 COMPLETE (2026-05-16, 13 XFails remain); SP6-SP7 pending
```

with:

```
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1-SP6 COMPLETE (2026-05-16, 3 XFails remain); SP7 pending
```

(Adjust to "4 XFails remain" if outcome B/C in T5.)

- [ ] **Step 6: Final cross-check**

```bash
cargo test 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
git status
```

Expected:
- `cargo test`: all green.
- `e2e/run_tests.sh`: summary matches expected XFail/Migrated counts.
- `git status`: only `TODO.md` is modified (memory files are outside the repo).

- [ ] **Step 7: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(sp6): close SP6 — remove roadmap entry, record follow-ups

10 tests migrated to PTY harness (tests/pty_posix.rs). E2E suite now
reports XFail: 3 (SP7 only), Migrated: 10.

[If outcome B/C: corrected to XFail: 4, Migrated: 9 — FCEDIT_default_ed
demoted to SP7 with rationale in TODO.md.]

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Note: the memory file edits in steps 4-5 are outside the repo and are not part of this commit.

---

## Self-Review Checklist

Run through this after T7 commits, before declaring SP6 complete:

- [ ] All 10 e2e shell files carry `# MIGRATED_TO:` (or, for any SP7 demotion, `# XFAIL: deferred (...)` with a TODO.md entry).
- [ ] `cargo test --test pty_posix` passes with 9 or 10 tests, matching the Migrated count.
- [ ] `./e2e/run_tests.sh` summary line reports the expected `XFail: N, Migrated: M`.
- [ ] `cargo test` overall is green — no `tests/pty_interactive.rs` regression from T2's helper extraction or T3's PS1 init.
- [ ] `src/interactive/mod.rs::Repl::new` has the PS1 default block with the `is_none()` guard.
- [ ] `tests/helpers/pty.rs` exists and is imported by both `tests/pty_interactive.rs` and `tests/pty_posix.rs` (no copy-paste of `spawn_yosh` / `wait_for_prompt`).
- [ ] `e2e/run_tests.sh` reports `[WARN]` when both `MIGRATED_TO` and `XFAIL` are present (manually verified once during T1 step 5).
- [ ] `TODO.md` SP6 line is gone; memory roadmap entry shows SP6 COMPLETE.
- [ ] No `Other:` columns / extraneous changes in `git log --oneline main..HEAD` — exactly 7 commits matching G1-G7.
