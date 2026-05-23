# `fc` Builtin Stack-Overflow on Self-Reference — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix bare `fc` and `fc -e <editor>` stack-overflow by hoisting `Repl::run`'s `history.add` call past `exec_complete_command`, then drop the `echo` operand workaround from four PTY tests and add one regression test.

**Architecture:** Single 4-line statement reordering inside `Repl::run` (`src/interactive/mod.rs`). The current command's history-add happens **after** execution, so the fc builtin observes the pre-fc history when it resolves "previous command". `exit` is still recorded because the for-loop's `break` falls through to the new add site.

**Tech Stack:** Rust 2024 edition, `expectrl` PTY test crate, `cargo test --test pty_posix`.

**Spec:** `docs/superpowers/specs/2026-05-23-fc-history-add-recursion-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/interactive/mod.rs` | Modify (4-line reorder) | Move `history.add` call after the inner exec loop |
| `tests/pty_posix.rs` | Modify (4 tests) + add (1 test) | Drop `echo` operand workaround; add bare-`fc` regression |
| `TODO.md` | Modify (delete entry) | Remove the resolved SP6 follow-up bullet |

No new files. No new public API. No new dependencies.

---

## Task 1: Add the regression test (RED phase)

**Files:**
- Modify: `tests/pty_posix.rs` (insert new test in `mod fc { ... }`, near line 158 just before the closing `}` of `mod fc`)

- [ ] **Step 1.1: Insert the regression test**

Open `tests/pty_posix.rs` and add the following `#[test]` inside `mod fc`, after the `no_args_uses_editor` test and before the closing `}` of `mod fc` (currently at line 159):

```rust
    #[test]
    fn bare_fc_does_not_recurse() {
        // Regression: bare `fc` with FCEDIT=cat used to stack-overflow
        // yosh because Repl::run added the fc command to history BEFORE
        // running it, so fc's "previous command" resolved to itself and
        // re-entered fc indefinitely. Hoisting history.add to after
        // exec_complete_command fixes this — fc now sees the pre-fc
        // history and resolves to the user's actual prior command.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
```

- [ ] **Step 1.2: Build the test binary**

Run: `cargo test --test pty_posix --no-run`

Expected: builds successfully (warnings OK, no errors). Build takes 1–3 min on a cold cache.

- [ ] **Step 1.3: Run the regression test to verify it FAILS**

Run: `cargo test --test pty_posix fc::bare_fc_does_not_recurse -- --nocapture`

Expected: **FAIL.** The yosh subprocess stack-overflows (SIGSEGV) when it tries to recurse on itself via the bare `fc` invocation. The PTY capture hits the 15 s `TIMEOUT` (`tests/helpers/pty.rs:19`) or returns early with empty output. Either way, the assertion `out.contains("RC=0")` is false.

If the test passes here, the bug is not reproduced — STOP and investigate before proceeding to Task 2.

---

## Task 2: Apply the fix (GREEN phase)

**Files:**
- Modify: `src/interactive/mod.rs:251-294` (move `history.add` call from before to after the inner `for` loop)

- [ ] **Step 2.1: Read the current code block to confirm line locations**

Run: `sed -n '250,295p' src/interactive/mod.rs` (or open the file in an editor)

Confirm the structure matches:

```rust
            match classify_parse(&input_buffer, &self.executor.env.aliases) {
                ParseStatus::Complete(commands) => {
                    // Add to history before executing
                    let histsize: usize = self
                        .executor
                        .env
                        .vars
                        .get("HISTSIZE")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(500);
                    let histcontrol = self
                        .executor
                        .env
                        .vars
                        .get("HISTCONTROL")
                        .unwrap_or("ignoreboth")
                        .to_string();
                    let cmd_text = input_buffer.trim_end().to_string();
                    self.executor
                        .env
                        .history
                        .add(&cmd_text, histsize, &histcontrol);

                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.exec.last_exit_status = status;
                        if self.executor.exit_requested.is_some() {
                            break;
                        }
                    }
                    input_buffer.clear();
                }
```

- [ ] **Step 2.2: Apply the edit — remove the early `history.add`, replace with a comment leader; add the new `history.add` after the for-loop**

Replace the entire `ParseStatus::Complete(commands) => { ... }` block with:

```rust
                ParseStatus::Complete(commands) => {
                    let histsize: usize = self
                        .executor
                        .env
                        .vars
                        .get("HISTSIZE")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(500);
                    let histcontrol = self
                        .executor
                        .env
                        .vars
                        .get("HISTCONTROL")
                        .unwrap_or("ignoreboth")
                        .to_string();
                    let cmd_text = input_buffer.trim_end().to_string();

                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.exec.last_exit_status = status;
                        if self.executor.exit_requested.is_some() {
                            break;
                        }
                    }

                    // Record AFTER execution so `fc` resolving "previous
                    // command" sees the user's prior input, not the fc
                    // command itself. `exit` is still captured: the
                    // break above falls through to this add call.
                    self.executor
                        .env
                        .history
                        .add(&cmd_text, histsize, &histcontrol);

                    input_buffer.clear();
                }
```

Key differences from the original:
- The `// Add to history before executing` comment is removed.
- The early `history.add` call (formerly lines 269-272) is deleted.
- A new `history.add` call is inserted **after** the `for cmd in &commands` loop, **before** `input_buffer.clear()`.
- A multi-line `// Record AFTER execution ...` comment documents the WHY.

- [ ] **Step 2.3: Verify the file compiles**

Run: `cargo check --lib`

Expected: success (warnings OK, no errors). Should be quick (incremental).

- [ ] **Step 2.4: Re-run the regression test to verify it PASSES**

Run: `cargo test --test pty_posix fc::bare_fc_does_not_recurse -- --nocapture`

Expected: **PASS.** With the fix, bare `fc` sees `echo seedline` as the previous history entry, `cat` passes the tempfile through unchanged, and `eval_string("echo seedline")` exits 0. The shell prints `RC=0`.

---

## Task 3: Drop the workaround from existing fc tests

**Files:**
- Modify: `tests/pty_posix.rs` (4 tests across `mod fc` and `mod fcedit`)

- [ ] **Step 3.1: Update `mod fc::editor_dash_e` (currently lines 103-132)**

Replace the entire body of `editor_dash_e` with:

```rust
    #[test]
    fn editor_dash_e() {
        // Bare `fc -e cat`: cat reads the tempfile (no edits), exits 0;
        // fc then re-executes the previous (seeded) history entry. We
        // mute the re-execution's stdio and check only the exit status.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc -e cat </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
```

Notable changes from the original:
- The 9-line workaround comment (about adding fc to history before execution causing recursion) is removed.
- The fc command line drops the `echo` prefix operand: `"fc -e cat echo </dev/null ..."` → `"fc -e cat </dev/null ..."`.

- [ ] **Step 3.2: Update `mod fc::no_args_uses_editor` (currently lines 134-158)**

Replace the entire body of `no_args_uses_editor` with:

```rust
    #[test]
    fn no_args_uses_editor() {
        // Bare `fc` with FCEDIT=cat: cat reads tempfile, exits 0; fc
        // re-executes the previous command. Check exit status only.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
```

Notable changes from the original:
- Workaround comment removed.
- `"fc echo </dev/null ..."` → `"fc </dev/null ..."` (drop `echo` operand).

- [ ] **Step 3.3: Update `mod fcedit::used_by_fc` (currently lines 164-188)**

Replace the entire body of `used_by_fc` with:

```rust
    #[test]
    fn used_by_fc() {
        // FCEDIT=cat → fc invokes cat as editor → cat reads tempfile,
        // exits 0 → fc re-executes the previous command.
        let (mut session, _tmpdir) = spawn_yosh();
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "export FCEDIT=cat");
        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
```

Notable changes:
- The 5-line workaround paragraph (referencing `src/interactive/mod.rs:268-272`) is removed.
- `"fc echo </dev/null ..."` → `"fc </dev/null ..."`.

- [ ] **Step 3.4: Update `mod fcedit::default_ed` (currently lines 190-212)**

Replace the entire body of `default_ed` with:

```rust
    #[test]
    fn default_ed() {
        // FCEDIT and EDITOR removed → fc falls back to /bin/ed. We
        // verify /bin/ed exits 0 when given an empty stdin (probed
        // platform-side; see SP6 design §6).
        let (mut session, _tmpdir) = spawn_yosh_with_env(&[("FCEDIT", None), ("EDITOR", None)]);
        wait_for_prompt(&mut session);

        run_and_drain(&mut session, "echo seedline");

        let out = capture_until_sentinel(
            &mut session,
            "fc </dev/null >/dev/null 2>&1; echo RC=$?",
        );

        assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

        session.send_line("exit").unwrap();
        let _ = session.expect(Eof);
    }
```

Notable changes:
- Workaround paragraph (about prefix-match) removed.
- `"fc echo </dev/null ..."` → `"fc </dev/null ..."`.

- [ ] **Step 3.5: Build and run all fc + fcedit tests to confirm they still pass**

Run: `cargo test --test pty_posix 'fc::' 'fcedit::' -- --nocapture`

Expected: **all PASS.** Tests should include at minimum:
- `fc::list_recent`
- `fc::list_no_numbers`
- `fc::list_reverse`
- `fc::substitute`
- `fc::editor_dash_e`
- `fc::no_args_uses_editor`
- `fc::bare_fc_does_not_recurse`
- `fcedit::used_by_fc`
- `fcedit::default_ed`

If any test fails, STOP and inspect the diff against the spec before continuing. PTY tests are sometimes flaky on first run after a build; if a failure looks timing-related, re-run once before treating it as a real regression.

---

## Task 4: Remove the resolved TODO entry

**Files:**
- Modify: `TODO.md` (delete lines 221-231)

- [ ] **Step 4.1: Delete the resolved bullet**

Open `TODO.md` and remove the entire bullet currently at lines 221-231 (under `### SP6 follow-ups (non-blocking)`):

```
- [ ] `fc` builtin with no operand (`fc`, `fc -e <editor>`) infinite-recurses
      and stack-overflows yosh. Root cause: `Repl::run` adds the running
      command to history via `executor.env.history.add(...)` BEFORE
      `exec_complete_command` (`src/interactive/mod.rs:268-272`), so bare
      `fc` resolves "previous command" to the fc command itself and
      `eval_string`s back into fc. POSIX explicitly says fc must not
      enter itself in history. Affects `tests/pty_posix.rs::fc::editor_dash_e`
      and `tests/pty_posix.rs::fc::no_args_uses_editor`, which currently
      work around it by passing an `echo` prefix operand. Fix: hoist the
      `history.add` call after `exec_complete_command`, or have `fc`
      itself filter its own command from the history slice it operates on.
```

Per project convention (`CLAUDE.md` §TODO.md), **delete completed items** rather than marking them with `[x]`.

- [ ] **Step 4.2: Verify the SP6 section header still has at least one entry**

Run: `grep -A 1 '### SP6 follow-ups' TODO.md`

Expected: the next two follow-up bullets (PTY regex mis-match, `exec >file` sentinel) remain. SP6 should still have 2 bullets after this deletion.

---

## Task 5: Run the broader regression sweep

- [ ] **Step 5.1: Run interactive + signals + subshell test groups**

These three groups touch `Repl::run` and history adjacent code paths, so they are the most likely places to surface unintended fallout. Run them together:

```sh
cargo test --test interactive --test signals --test subshell
```

Expected: all PASS. If any test fails, the change has a side effect outside the fc path — STOP and investigate.

- [ ] **Step 5.2: Run the full pty_posix suite**

Run: `cargo test --test pty_posix`

Expected: all PASS. The fc/fcedit tests are covered by Step 3.5 but other modules (ps1, etc.) exercise `Repl::run` end-to-end.

- [ ] **Step 5.3: Run the workspace unit tests for `interactive`**

Run: `cargo test --lib interactive::`

Expected: all PASS. The `Repl` struct is in `src/interactive/mod.rs` and has no inline tests for the modified block, but adjacent module tests should not regress.

---

## Task 6: Commit

- [ ] **Step 6.1: Stage all changed files**

Run:

```sh
git add src/interactive/mod.rs tests/pty_posix.rs TODO.md
```

- [ ] **Step 6.2: Verify the staged diff is exactly what was intended**

Run: `git diff --cached --stat`

Expected output (modulo line-count drift):

```
 TODO.md                |  12 ------
 src/interactive/mod.rs |  18 +++++-----
 tests/pty_posix.rs     |  60 ++++++++--------------
 3 files changed, ~30 insertions(+), ~60 deletions(-)
```

If any other file appears in the diff, unstage it and re-check Task 1–5.

- [ ] **Step 6.3: Commit with the prescribed message**

Run:

```sh
git commit -m "$(cat <<'EOF'
fix(interactive): hoist history.add past exec to unbreak bare fc

Original task: TODO.md の中から優先度が高そうなものを1つ選んで対応
してください。 Selected the SP6 follow-up where bare fc and
fc -e <editor> stack-overflowed because Repl::run added the fc
command to history before running it, so fc's "previous command"
resolved to itself and re-entered fc indefinitely.

Move the history.add call in src/interactive/mod.rs to after the
inner exec loop. exit is still recorded because the for-loop's
break falls through to the new add site.

Drop the `echo` prefix-operand workaround from the four PTY tests
(fc::editor_dash_e, fc::no_args_uses_editor, fcedit::used_by_fc,
fcedit::default_ed) and add fc::bare_fc_does_not_recurse as a
direct regression test for the recursion case. Remove the
resolved item from TODO.md.

Spec: docs/superpowers/specs/2026-05-23-fc-history-add-recursion-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 6.4: Verify the commit landed cleanly**

Run: `git status && git log -1 --stat`

Expected: working tree clean; the new commit lists the three files above with the expected insertion/deletion counts.

---

## Self-Review Notes

**Spec coverage:**
- §1 (background, root cause) → context, not implemented
- §2 In-scope item 1 (hoist `history.add`) → Task 2
- §2 In-scope item 2 (drop workaround in 4 tests) → Task 3
- §2 In-scope item 3 (add regression test) → Task 1
- §2 In-scope item 4 (remove TODO entry) → Task 4
- §6 Integration tests → Task 3 Step 3.5 + Task 5
- §6 Manual verification → not in plan (optional; covered by automated regression test)

**Type / signature consistency:**
- `history.add(&str, usize, &str)` signature used identically in both old and new call sites.
- All four updated tests use the same call shape: `capture_until_sentinel(&mut session, "fc[ -e cat] </dev/null >/dev/null 2>&1; echo RC=$?")`.

**No placeholders:**
- Every test body is shown in full; no "similar to ..." references.
- Every commit message is literal text.
- All file paths and line numbers come from the actual files at plan-write time.
