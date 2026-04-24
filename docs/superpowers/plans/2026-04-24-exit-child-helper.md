# exit_child Helper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every post-fork `std::process::exit` (and bare `libc::_exit`) in yosh with a safe `exit_child` helper that flushes stdio then invokes `libc::_exit`, fixing the multithreaded-test-harness deadlock documented in TODO.md.

**Architecture:** Add a single `pub(crate) fn exit_child(status: i32) -> !` in `src/exec/mod.rs`. Replace 8 post-fork exit call sites across 5 files with mechanical rewrites. Guard against future naive `_exit` regressions with one subshell+pipeline regression test. Shell-parent exits stay on `std::process::exit`.

**Tech Stack:** Rust, `libc::_exit`, nix `fork`, yosh integration tests via `yosh_exec` helper.

**Spec:** `docs/superpowers/specs/2026-04-24-exit-child-helper-design.md`

---

### Task 1: Add regression test (baseline)

Establish a guard test BEFORE modifying any code. With current `std::process::exit` the test passes (Rust runtime cleanup flushes stdio). After later tasks swap to `libc::_exit` without flush, this test fails, driving us to keep the flush in the helper.

**Files:**
- Modify: `tests/subshell.rs` (append at end of file)

- [ ] **Step 1: Append the test**

Add at the bottom of `tests/subshell.rs`:

```rust
#[test]
fn test_subshell_pipeline_preserves_unflushed_output() {
    // Regression test for the exit_child helper (TODO.md Known Bug fix).
    //
    // Naive `libc::_exit(0)` without stdout flush would regress
    // `( echo -n hi ) | cat` to empty output, because `echo -n` does
    // not append a newline and stdout's LineWriter would not auto-flush
    // before _exit. Confirmed empirically 2026-04-24.
    //
    // This test exercises BOTH the subshell exit path (compound.rs)
    // AND the pipeline member exit path (pipeline.rs) via the pipe.
    let out = yosh_exec("( echo -n hi ) | cat");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi");
}
```

- [ ] **Step 2: Run test to verify it passes on current code**

```bash
cargo test --test subshell test_subshell_pipeline_preserves_unflushed_output
```

Expected: PASS (baseline — current `std::process::exit` does flush stdio via Rust cleanup).

- [ ] **Step 3: Commit**

```bash
git add tests/subshell.rs
git commit -m "$(cat <<'EOF'
test(subshell): add regression guard for exit_child flush behavior

Baseline test for the upcoming exit_child helper — pins the behavior
that `( echo -n hi ) | cat` produces "hi", so any future change that
replaces std::process::exit with naive libc::_exit (no flush) in the
subshell or pipeline-member child will fail visibly instead of
silently swallowing un-newlined stdout.

Prompt context: TODO.md Known Bug / Code Quality line 89 — exit_child
helper for post-fork deadlock fix.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Add `exit_child` helper (no call-site changes)

Introduce the helper alone so the next task is a pure mechanical rewrite. `#[allow(dead_code)]` is intentionally **not** added — the helper compiles fine unused as `pub(crate)` inside a crate-internal module. If clippy warns, it's a signal to tighten scope in the next task.

**Files:**
- Modify: `src/exec/mod.rs` (add near top after existing `use` imports)

- [ ] **Step 1: Read the top of `src/exec/mod.rs` to find the right insertion point**

```bash
head -40 src/exec/mod.rs
```

Goal: identify the last `use` line at module scope. Insert the helper right after it (before any other item).

- [ ] **Step 2: Add the helper**

Insert in `src/exec/mod.rs` after the last top-level `use` line:

```rust
/// Exit a post-fork child process safely.
///
/// Uses `libc::_exit` to skip Rust runtime cleanup, which can deadlock
/// on std-internal mutexes inherited locked from a multithreaded parent
/// (e.g. `std::sys::pal::unix::stack_overflow::thread_info::LOCK`).
/// Flushes stdout/stderr first so buffered output is not lost.
///
/// Use ONLY after `fork()` in the child branch, never in the shell parent.
pub(crate) fn exit_child(status: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(status) }
}
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build
```

Expected: clean build. A `dead_code` warning is acceptable at this point (consumed in Task 3).

- [ ] **Step 4: Verify existing tests still pass**

```bash
cargo test --test subshell
```

Expected: all subshell tests (including the new regression test from Task 1) pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs
git commit -m "$(cat <<'EOF'
feat(exec): add exit_child helper for post-fork safe exit

Introduces pub(crate) fn exit_child(status: i32) -> ! in src/exec/mod.rs.
Flushes stdout/stderr then calls libc::_exit, skipping Rust runtime
cleanup which can deadlock on std-internal mutexes inherited locked
from a multithreaded parent after fork().

No call sites yet — those are swapped in the next commit so this
definition stands alone for review.

Prompt context: TODO.md Known Bug / Code Quality line 89.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Replace all 8 post-fork exit sites

Mechanical rewrite across 5 files. Grouped into one commit so the codebase never lives in a half-swapped state.

**Files:**
- Modify: `src/exec/compound.rs` (1 site)
- Modify: `src/exec/simple.rs` (2 sites)
- Modify: `src/exec/pipeline.rs` (3 sites, including 2 bare `libc::_exit`)
- Modify: `src/exec/mod.rs` (1 site, bg job)
- Modify: `src/expand/command_sub.rs` (1 site)

- [ ] **Step 1: Replace `src/exec/compound.rs:99`**

In the `Ok(ForkResult::Child)` arm of `exec_subshell`:

```rust
// before:
                let status = self.exec_body(body);
                std::process::exit(status);
// after:
                let status = self.exec_body(body);
                super::exit_child(status);
```

- [ ] **Step 2: Replace `src/exec/simple.rs:474`**

In the redirect-apply error branch of the external-command child:

```rust
// before:
                if let Err(e) = redir_state.apply(redirects, &mut self.env, false) {
                    eprintln!("yosh: {}", e);
                    std::process::exit(1);
                }
// after:
                if let Err(e) = redir_state.apply(redirects, &mut self.env, false) {
                    eprintln!("yosh: {}", e);
                    super::exit_child(1);
                }
```

- [ ] **Step 3: Replace `src/exec/simple.rs:508`**

At the end of the `ForkResult::Child` arm (after `execvp` failure match):

```rust
// before:
                };
                std::process::exit(exit_code);
            }
            Ok(ForkResult::Parent { child }) => {
// after:
                };
                super::exit_child(exit_code);
            }
            Ok(ForkResult::Parent { child }) => {
```

- [ ] **Step 4: Replace `src/exec/pipeline.rs:91`**

stdin dup2 failure in the pipeline child:

```rust
// before:
                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }
// after:
                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            super::exit_child(1);
                        }
                    }
```

- [ ] **Step 5: Replace `src/exec/pipeline.rs:99`**

stdout dup2 failure in the pipeline child:

```rust
// before:
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }
// after:
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            super::exit_child(1);
                        }
                    }
```

- [ ] **Step 6: Replace `src/exec/pipeline.rs:106`**

Pipeline member normal exit:

```rust
// before:
                    close_all_pipes(&pipes);

                    let status = self.exec_command(cmd);
                    std::process::exit(status);
                }
// after:
                    close_all_pipes(&pipes);

                    let status = self.exec_command(cmd);
                    super::exit_child(status);
                }
```

- [ ] **Step 7: Replace `src/exec/mod.rs:361`**

Background-job child normal exit (inside the `Ok(ForkResult::Child)` arm of the bg-job spawn path). Note: no `super::` since we are in `src/exec/mod.rs` itself.

```rust
// before:
                let status = self.exec_and_or(and_or);
                std::process::exit(status);
            }
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
// after:
                let status = self.exec_and_or(and_or);
                exit_child(status);
            }
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
```

- [ ] **Step 8: Replace `src/expand/command_sub.rs:72`**

Command substitution child normal exit (cross-module call):

```rust
// before:
            let status = executor.exec_program(program);
            std::process::exit(status);
        }
        Ok(ForkResult::Parent { child }) => {
// after:
            let status = executor.exec_program(program);
            crate::exec::exit_child(status);
        }
        Ok(ForkResult::Parent { child }) => {
```

- [ ] **Step 9: Verify nothing else remains**

Confirm the only remaining `std::process::exit` and `libc::_exit` sites are the intentionally-preserved shell-parent ones:

```bash
grep -n "std::process::exit\|libc::_exit" src/exec/ src/expand/command_sub.rs
```

Expected output (exactly these lines, nothing else):
- `src/exec/mod.rs:155:  std::process::exit(status);` — errexit in non-interactive shell (parent)
- `src/exec/mod.rs:211:  std::process::exit(128 + sig);` — signal handler (parent)

Any other `std::process::exit` or `libc::_exit` in `src/exec/` or `src/expand/command_sub.rs` means a replacement was missed — go back and fix.

- [ ] **Step 10: Build**

```bash
cargo build
```

Expected: clean build with no warnings (the `dead_code` from Task 2 is now resolved).

- [ ] **Step 11: Run subshell test suite**

```bash
cargo test --test subshell
```

Expected: all pass, including `test_subshell_pipeline_preserves_unflushed_output`. If that specific test fails with empty stdout, the `exit_child` helper is missing the flush — go check Task 2's code block.

- [ ] **Step 12: Commit**

```bash
git add src/exec/compound.rs src/exec/simple.rs src/exec/pipeline.rs src/exec/mod.rs src/expand/command_sub.rs
git commit -m "$(cat <<'EOF'
fix(exec): route post-fork child exits through exit_child helper

Replaces 8 call sites that previously used std::process::exit or bare
libc::_exit in post-fork children with super::exit_child (or
crate::exec::exit_child across modules). Fixes a rare cargo test
--workspace deadlock where a child inherits a locked
std::sys::pal::unix::stack_overflow::thread_info::LOCK from a
multithreaded parent, then blocks forever in
std::rt::cleanup -> stack_overflow::cleanup -> LOCK.lock().

Sites changed:
- src/exec/compound.rs:99  — subshell normal exit
- src/exec/simple.rs:474   — redirect-apply failure in external cmd child
- src/exec/simple.rs:508   — execvp failure
- src/exec/pipeline.rs:91  — stdin dup2 failure (also adds missing flush)
- src/exec/pipeline.rs:99  — stdout dup2 failure (also adds missing flush)
- src/exec/pipeline.rs:106 — pipeline member normal exit
- src/exec/mod.rs:361      — background-job child normal exit
- src/expand/command_sub.rs:72 — command substitution child normal exit

Shell-parent std::process::exit sites (errexit in non-interactive shell,
signal handler, exit builtin) are intentionally preserved so they still
run Rust runtime cleanup.

Prompt context: TODO.md Known Bug / Code Quality line 89 — systematic
fix for post-fork deadlock.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Full test-suite verification

Per CLAUDE.md, always run `cargo test` and E2E before declaring work complete. Long-running commands go to background per saved memory (cargo test --workspace ~6-7 min).

**Files:** None modified — verification only.

- [ ] **Step 1: Run workspace unit + integration tests in background**

```bash
cargo test --workspace
```

Run this via the Bash tool with `run_in_background: true`. Wait for the completion notification (do not poll).

Expected: all pass. Known-flaky: `exec_compound_subshell_sets_lineno_on_entry` was the deadlock target — it should now run cleanly. If the full suite hangs > 10 min, that indicates the fix did not land as intended; investigate by sampling the test process.

- [ ] **Step 2: Run E2E suite in background (after unit tests complete)**

```bash
./e2e/run_tests.sh
```

Run with `run_in_background: true`. Wait for completion notification.

Expected: 374/374 pass (current suite size — per recent commits). Transient 1–6 timeouts/failures are a known flake per TODO.md:114; if it flakes, re-run once. If consistent failures appear, investigate whether they correlate with post-fork child paths.

- [ ] **Step 3: Re-run `cargo test --workspace` once more to stress the deadlock path**

```bash
cargo test --workspace
```

Run in background. Two consecutive clean runs is strong evidence the race is resolved (the original bug required a 6-hour hang to reproduce, so a couple green runs already improves confidence materially).

Expected: all pass. No hang.

- [ ] **Step 4: No commit in this task**

Verification only — proceed to Task 5.

---

### Task 5: Update TODO.md (delete completed entries)

Per CLAUDE.md: delete completed items, do not use `[x]` markers.

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Delete the entire `## Known Bugs` section**

The section contains only the resolved deadlock bug. Once removed, nothing else belongs under the heading. Delete the heading line, the blank line after it, the bullet, and the trailing blank line — collapsing directly into the next heading.

`old_string` for the Edit tool (verify against current `TODO.md` — line numbers may have shifted):

```
## Known Bugs

- [ ] `cargo test --workspace` can deadlock on `exec_compound_subshell_sets_lineno_on_entry` (race, rarely reproducible) — after `fork()` in `exec_subshell`, the child calls `std::process::exit` which runs `std::rt::cleanup` → `stack_overflow::cleanup` → `drop_handler` → `delete_current_info` → `LOCK.lock()` on the static `Mutex<()>` in `std::sys::pal::unix::stack_overflow::thread_info`. In the multithreaded test harness, another worker thread can be holding that `LOCK` at fork time; the child inherits the locked state with no owner thread and blocks in `__psynch_mutexwait` forever, which makes the parent's `wait4()` hang too. Does not manifest in production (interactive shell parent is single-threaded). Observed 2026-04-24 via 6-hour hang; grandchild sampled at `__psynch_mutexwait`. Systematic fix tracked under Code Quality Improvements (`exit_child` helper) (`src/exec/compound.rs:86-103`, `src/expand/command_sub.rs:72`, `src/exec/simple.rs:474,508`, `src/exec/pipeline.rs:106`, `src/exec/mod.rs:155,211,361`).

## Job Control: Known Limitations
```

`new_string`:

```
## Job Control: Known Limitations
```

After: the file starts with `# TODO`, a blank line, then `## Job Control: Known Limitations` directly.

- [ ] **Step 2: Delete the Code Quality Improvements entry for `exit_child`**

Remove the bullet starting "`- [ ] Introduce \`exit_child(status: i32) -> !\` helper + replace post-fork \`std::process::exit\` sites` — systematic fix for the deadlock bug tracked under Known Bugs…". This is line 89 as of 2026-04-24; verify current line before editing.

The immediately following bullet (`- [ ] fork + run-Rust-shell-code-in-child is fundamentally POSIX-UB in MT contexts…`) **stays** — that is the unresolved architectural concern.

- [ ] **Step 3: Verify the remaining architectural entry is intact**

```bash
grep -n "fundamentally POSIX-UB" TODO.md
```

Expected: exactly one match, still in the `## Future: Code Quality Improvements` section.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): remove resolved deadlock bug and exit_child fix entries

The Known Bug (cargo test --workspace deadlock on
exec_compound_subshell_sets_lineno_on_entry) and its systematic fix
(exit_child helper) are now implemented across src/exec/ and
src/expand/command_sub.rs. Deleting both entries per the repo
convention in CLAUDE.md (delete completed items, do not use [x]).

The related architectural concern — fork+run-Rust-code-in-child is
POSIX-UB in MT contexts — stays open as a long-term redesign item.

Prompt context: "TODO.md の中から優先度が高いものを1つ選び、対応してください"
— completed Known Bug fix via exit_child helper.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Final status check**

```bash
git log --oneline -5
git status
```

Expected: 4 new commits (Task 1 test, Task 2 helper, Task 3 replacements, Task 5 TODO.md), clean working tree.

---

## Done criteria

- [ ] `cargo test --workspace` passes twice back-to-back with no hang.
- [ ] `./e2e/run_tests.sh` passes (≤ 1 transient flake acceptable).
- [ ] `grep -n "std::process::exit\|libc::_exit" src/exec/ src/expand/command_sub.rs` returns only the 2 intentionally-preserved shell-parent sites in `src/exec/mod.rs`.
- [ ] `TODO.md` no longer contains the Known Bug or the `exit_child` helper Code Quality entry. The `fork + run-Rust-shell-code-in-child is fundamentally POSIX-UB` entry remains.
