# Exec Module Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `src/exec/mod.rs` (1460 lines) into a slim `mod.rs` plus two new submodules — `control.rs` (execution control flow) and `job_control.rs` (wait/jobs/fg/bg built-ins and helpers) — without changing any function body, signature, or externally visible API.

**Architecture:** Mechanical move only. The `Executor` struct stays in `mod.rs`; each new submodule contributes additional `impl Executor { ... }` blocks. Cross-submodule reachability is preserved by minimal `pub(super)` promotions on moved methods, the moved `ForegroundWaitResult` struct + fields, and the shared `preview_command` helper. Tests are co-located with the production code they exercise via `#[cfg(test)] mod tests` in each submodule. After each split task, `cargo build && cargo test --lib exec::` must equal the Task 0 baseline; the task is then committed.

**Tech Stack:** Rust 2024 edition, `cargo`, `nix` 0.31, `libc` 0.2, the existing `crate::env`, `crate::error`, `crate::parser::ast`, `crate::plugin`, `crate::signal` modules.

**Spec:** `docs/superpowers/specs/2026-05-05-exec-mod-split-design.md`

---

## File Structure

After this plan completes, `src/exec/` will contain:

| File | Responsibility |
|------|----------------|
| `mod.rs` | `Executor` struct, constructors (`new`, `from_env`, `load_plugins`), eval/source (`eval_string`, `source_file`, `verbose_print`), errexit policy (`with_errexit_suppressed`, `should_errexit`, `check_errexit`, `execute_exit_trap`), signal handling (`process_pending_signals`, `handle_default_signal`), shared free helpers (`exit_child` `pub(crate)`, `preview_command` `pub(super)`, `plugin_config_path` private), and 14 driver/setup-level tests. |
| `control.rs` | NEW. Execution control flow: `exec_command`, `exec_and_or`, `exec_async` (private), `exec_complete_command`, `exec_program`, `reap_zombies`, plus 17 tests covering builtins-via-dispatch, pipeline/and-or/program flow, exit_requested propagation, and LINENO updates. |
| `job_control.rs` | NEW. Job-control built-ins: `builtin_wait`, `builtin_jobs`, `builtin_fg`, `builtin_bg`, helpers (`wait_for_foreground_job`, `record_stopped_state`, `restore_shell_termios_if_interactive`, `display_job_notifications`), `ForegroundWaitResult` struct, free helper `strip_job_spec_prefix`, plus 3 `record_stopped_state` tests. |
| `command.rs` | Unchanged. |
| `compound.rs` | Unchanged. |
| `function.rs` | Unchanged. |
| `pipeline.rs` | Unchanged. |
| `redirect.rs` | Unchanged. |
| `simple.rs` | Unchanged. |
| `terminal_state.rs` | Unchanged. |

---

## Task 0: Capture Baseline

**Files:**
- No file changes.
- Outputs: `/tmp/exec-baseline.txt`, `/tmp/exec-grep.txt`, `/tmp/exec-wc-before.txt`.

- [ ] **Step 1: Capture lib-test baseline for the `exec` module**

```bash
cargo test --lib exec:: 2>&1 | tee /tmp/exec-baseline.txt
```

Expected: a line like `test result: ok. N passed; 0 failed; 0 ignored; ...`. Record the exact `passed` count — every later task must reproduce it. (Counting the `#[test]` markers in the source: 14 in `exec::tests` plus tests in sibling submodules; the **delta we care about for this plan** is "no decrease in `exec::tests` count after each move".)

- [ ] **Step 2: Capture full lib-test count baseline**

```bash
cargo test --lib 2>&1 | tail -5 | tee -a /tmp/exec-baseline.txt
```

Expected: a final summary line such as `test result: ok. NNN passed; 0 failed; 0 ignored; ...`. Record this number — Task 3's DoD requires the same total post-split.

- [ ] **Step 3: Snapshot key markers in `src/exec/mod.rs`**

```bash
grep -nE '#\[cfg\(test\)\]|pub\(super\)|pub\(crate\)|^fn |^pub fn |^impl Executor' \
    src/exec/mod.rs | tee /tmp/exec-grep.txt
```

Expected baseline content includes (line numbers approximate):
- Line 28: `pub(crate) fn exit_child`
- Line 49: `fn strip_job_spec_prefix`
- Line 62: `fn preview_command`
- Line 96: `impl Executor {`
- Line 222: `pub(crate) fn handle_default_signal`
- Line 321: `pub(crate) fn reap_zombies`
- Line 354: `fn exec_async`
- Line 445: `fn builtin_wait`
- Line 591: `fn builtin_jobs`
- Line 622: `fn builtin_fg`
- Line 724: `fn builtin_bg`
- Line 799: `fn restore_shell_termios_if_interactive`
- Line 820: `fn record_stopped_state`
- Line 849: `fn wait_for_foreground_job`
- Line 951: `fn plugin_config_path`
- Line 959–960: `#[cfg(test)] mod tests {`

- [ ] **Step 4: Record current `src/exec/mod.rs` line count**

```bash
wc -l src/exec/mod.rs | tee /tmp/exec-wc-before.txt
```

Expected: `1460 src/exec/mod.rs`. Used as the Definition-of-Done starting point.

- [ ] **Step 5: Verify the e2e baseline (job-control coverage)**

```bash
./e2e/run_tests.sh --filter=job 2>&1 | tail -10
```

Expected: all job-control e2e tests pass. Record the pass count — Task 3 must equal it. If your environment cannot run the full e2e harness, at minimum confirm `cargo test --lib exec::` and `cargo test --features test-helpers` are clean before starting Task 1.

This task does **not** produce a commit.

---

## Task 1: Split `job_control.rs`

**Files:**
- Create: `src/exec/job_control.rs`
- Modify: `src/exec/mod.rs` (delete moved code; add `mod job_control;` declaration; promote `preview_command` to `pub(super)`)

**What moves:**
- Production methods: `builtin_wait` (lines 444–589), `builtin_jobs` (591–620), `builtin_fg` (622–722), `builtin_bg` (724–793), `restore_shell_termios_if_interactive` (795–806), `record_stopped_state` (808–833), `wait_for_foreground_job` (835–936), `display_job_notifications` (938–948).
- Production struct: `ForegroundWaitResult` (lines 35–43).
- Free helper: `strip_job_spec_prefix` (lines 45–54).
- Tests: `record_stopped_state_clears_stale_saved_tmodes_on_none_capture`, `record_stopped_state_stores_some_capture`, `record_stopped_state_no_op_on_unknown_job` (lines 1328–1426).

**What changes visibility in `mod.rs`:**
- Nothing. `preview_command` stays private and is NOT imported by `job_control.rs` — none of the moved items reference it (only `exec_async` uses it, and `exec_async` stays in `mod.rs` until Task 2).

**Visibility plan for moved symbols:**

| Symbol | Old visibility | New visibility |
|--------|----------------|----------------|
| `ForegroundWaitResult` (struct + 3 fields) | `pub(crate)` struct, `pub` fields | `pub(super)` struct, `pub(super)` fields |
| `strip_job_spec_prefix` (free fn) | private (`fn`) | private (`fn`) — local to `job_control.rs` |
| `Executor::builtin_wait` | private (`fn`) | `pub(super)` |
| `Executor::builtin_jobs` | private (`fn`) | `pub(super)` |
| `Executor::builtin_fg` | private (`fn`) | `pub(super)` |
| `Executor::builtin_bg` | private (`fn`) | `pub(super)` |
| `Executor::restore_shell_termios_if_interactive` | private (`fn`) | `pub(super)` |
| `Executor::record_stopped_state` | private (`fn`) | private (`fn`) — internal to `job_control.rs` |
| `Executor::wait_for_foreground_job` | private (`fn`) | `pub(super)` |
| `Executor::display_job_notifications` | `pub` | `pub` (unchanged) |

- [ ] **Step 1: Read the production lines being moved**

Read these ranges from `src/exec/mod.rs`, copying their exact content for use in Step 4:

- `Read src/exec/mod.rs offset=35 limit=10` — `ForegroundWaitResult` struct.
- `Read src/exec/mod.rs offset=45 limit=10` — `strip_job_spec_prefix` free function.
- `Read src/exec/mod.rs offset=444 limit=146` — `builtin_wait`.
- `Read src/exec/mod.rs offset=591 limit=30` — `builtin_jobs`.
- `Read src/exec/mod.rs offset=622 limit=101` — `builtin_fg`.
- `Read src/exec/mod.rs offset=724 limit=70` — `builtin_bg`.
- `Read src/exec/mod.rs offset=795 limit=12` — `restore_shell_termios_if_interactive`.
- `Read src/exec/mod.rs offset=808 limit=26` — `record_stopped_state`.
- `Read src/exec/mod.rs offset=835 limit=102` — `wait_for_foreground_job`.
- `Read src/exec/mod.rs offset=938 limit=11` — `display_job_notifications`.

Confirm exact content before editing. The line ranges above are derived from the Task 0 grep snapshot and may shift by ±1 if a future patch changes earlier lines — re-grep `^\(pub \)\?fn builtin_wait\|fn record_stopped_state\|^pub fn display_job_notifications` if needed.

- [ ] **Step 2: Read the test lines being moved**

Read `src/exec/mod.rs offset=1328 limit=99` — covers all three `record_stopped_state_*` tests through line 1426 inclusive.

- [ ] **Step 3: Inspect existing `pub(crate)` callers of moved methods (information only)**

```bash
grep -n "wait_for_foreground_job\|restore_shell_termios_if_interactive\|builtin_wait\|builtin_jobs\|builtin_fg\|builtin_bg\|display_job_notifications\|ForegroundWaitResult" \
    src/exec/*.rs src/interactive/*.rs src/main.rs src/builtin/*.rs 2>/dev/null \
    | grep -v "src/exec/mod.rs"
```

Expected callers (these all reach the methods via `self.method(...)` or `executor.method(...)` and therefore work transparently as long as visibility is `pub(super)` for sibling submodules and `pub` for cross-crate-module callers):

- `src/exec/pipeline.rs` lines 130, 133 → `wait_for_foreground_job`, `restore_shell_termios_if_interactive`
- `src/exec/simple.rs` lines 225, 251–253, 547, 555 → `builtin_wait`, `builtin_fg`, `builtin_bg`, `builtin_jobs`, `wait_for_foreground_job`, `restore_shell_termios_if_interactive`
- `src/interactive/mod.rs` line 133 → `display_job_notifications`

No external file imports `ForegroundWaitResult` by name — only field access (`result.last_status`, `result.process_statuses`, `result.stopped`), so `pub(super)` on the struct + fields is sufficient.

- [ ] **Step 4: Create `src/exec/job_control.rs`**

Write `src/exec/job_control.rs` with the structure shown below. Replace each `// COPY VERBATIM ... ` marker with the exact text captured in Steps 1–2. Apply the visibility changes (`fn` → `pub(super) fn` for the methods listed in the visibility plan, `pub` → `pub(super)` for the struct and its fields).

```rust
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use super::Executor;
use crate::env::jobs::{self, JobSpecError, JobStatus};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::signal;

/// Result of waiting for a foreground job.
pub(super) struct ForegroundWaitResult {
    /// Exit status of the last process to report.
    pub(super) last_status: i32,
    /// Per-process exit statuses (pid, exit_code) in reporting order — used by pipefail.
    pub(super) process_statuses: Vec<(nix::unistd::Pid, i32)>,
    /// Whether the job was stopped (e.g., Ctrl+Z) rather than exiting.
    pub(super) stopped: bool,
}

/// Strip the leading `%` (and optional `?`) from a job spec string for
/// inclusion in error messages. Matches bash: `wait %sleep` with ambiguous
/// match reports `wait: sleep: ambiguous job spec`, not `%sleep:`.
/// Inputs that don't start with `%` are returned unchanged.
fn strip_job_spec_prefix(spec: &str) -> &str {
    match spec.strip_prefix('%') {
        Some(rest) => rest.strip_prefix('?').unwrap_or(rest),
        None => spec,
    }
}

impl Executor {
    /// POSIX wait builtin: wait for background jobs.
    pub(super) fn builtin_wait(&mut self, args: &[String]) -> Result<i32, ShellError> {
        // COPY VERBATIM body from mod.rs lines 446–589
        // (drop the `use crate::env::jobs::JobStatus; use nix::sys::wait::{...};
        //  use nix::unistd::Pid;` block at the top — already imported at module scope)
    }

    pub(super) fn builtin_jobs(&mut self, args: &[String]) -> Result<i32, ShellError> {
        // COPY VERBATIM body from mod.rs lines 591–620
    }

    pub(super) fn builtin_fg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        // COPY VERBATIM body from mod.rs lines 622–722
        // (drop the `use crate::env::jobs::{self, JobStatus};` line — already imported)
    }

    pub(super) fn builtin_bg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        // COPY VERBATIM body from mod.rs lines 724–793
        // (drop the `use crate::env::jobs::JobStatus;` line — already imported)
    }

    /// Apply the shell's captured termios snapshot when in interactive
    /// + monitor mode. Best-effort; silent on failure or when the
    ///   snapshot is not set (non-interactive, non-monitor, or capture
    ///   failed at REPL startup).
    pub(super) fn restore_shell_termios_if_interactive(&self) {
        // COPY VERBATIM body from mod.rs lines 799–806
    }

    /// Apply the per-job state transition for `WaitStatus::Stopped`.
    ///
    /// (full doc comment from mod.rs lines 808–819 verbatim)
    fn record_stopped_state(
        &mut self,
        job_id: crate::env::jobs::JobId,
        sig: i32,
        captured: Option<nix::sys::termios::Termios>,
    ) {
        // COPY VERBATIM body from mod.rs lines 826–832
    }

    /// Wait for a foreground job to complete or stop.
    ///
    /// (full doc comment from mod.rs lines 836–848 verbatim)
    pub(super) fn wait_for_foreground_job(
        &mut self,
        job_id: crate::env::jobs::JobId,
    ) -> ForegroundWaitResult {
        // COPY VERBATIM body from mod.rs lines 850–935
    }

    /// Display pending job notifications and clean up completed jobs.
    pub fn display_job_notifications(&mut self) {
        // COPY VERBATIM body from mod.rs lines 939–948
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_stopped_state_clears_stale_saved_tmodes_on_none_capture() {
        // COPY VERBATIM body from mod.rs lines 1329–1376
    }

    #[test]
    fn record_stopped_state_stores_some_capture() {
        // COPY VERBATIM body from mod.rs lines 1379–1415
    }

    #[test]
    fn record_stopped_state_no_op_on_unknown_job() {
        // COPY VERBATIM body from mod.rs lines 1418–1426
    }
}
```

**Notes on imports:**
- `JobStatus`, `JobSpecError`, `jobs::give_terminal`, `jobs::take_terminal` are imported at module scope so the per-fn `use crate::env::jobs::...` lines inside the moved bodies become redundant — strip them while copying.
- The body of `builtin_wait` uses `Pid::from_raw`, `WaitPidFlag`, `WaitStatus`, `waitpid` — all imported at module scope.
- `nix::sys::signal::killpg`, `nix::sys::signal::Signal::SIGCONT`, `crate::exec::terminal_state::*` are referenced fully qualified inside the moved bodies; do not add fresh `use` lines for them.

- [ ] **Step 5: Update `src/exec/mod.rs` — remove moved code**

Use `Edit` (multiple invocations as needed) to delete the following ranges from `src/exec/mod.rs`:

1. The `ForegroundWaitResult` struct (lines 35–43 inclusive — the `///` doc, `pub(crate) struct`, three `pub` fields, and closing `}`).
2. The `strip_job_spec_prefix` free fn (lines 45–54 inclusive — the doc comment block plus the function body).
3. The `builtin_wait` method (lines 444–589 inclusive — the `/// POSIX wait builtin...` doc comment plus the entire fn body).
4. The `builtin_jobs` method (lines 591–620 inclusive).
5. The `builtin_fg` method (lines 622–722 inclusive).
6. The `builtin_bg` method (lines 724–793 inclusive).
7. The `restore_shell_termios_if_interactive` method (lines 795–806 inclusive — the doc comment plus body).
8. The `record_stopped_state` method (lines 808–833 inclusive).
9. The `wait_for_foreground_job` method (lines 835–936 inclusive).
10. The `display_job_notifications` method (lines 938–948 inclusive).
11. The three `record_stopped_state_*` tests in `mod.rs::tests` (lines 1328–1426 inclusive).

After deletion, ensure the surrounding `impl Executor { ... }` brace structure remains balanced. The closing `}` of `impl Executor` originally lives at line 949 — keep it.

- [ ] **Step 6: Update `src/exec/mod.rs` — declare new submodule**

Add `mod job_control;` to the existing `mod` declaration block at the top of `src/exec/mod.rs`. Insert it in alphabetical position so the block reads:

```rust
pub mod command;
mod compound;
mod function;
mod job_control;
pub mod pipeline;
pub mod redirect;
mod simple;
pub(crate) mod terminal_state;
```

- [ ] **Step 7: Drop now-unused imports in `src/exec/mod.rs`**

After removing the job-control bodies, some imports at the top of `mod.rs` may become unused. Specifically, check whether `crate::env::jobs::JobSpecError` is still referenced after the move — it is used only by `builtin_wait`/`fg`/`bg`. Run `cargo check` after Step 6 to surface unused-import warnings, then remove them.

```bash
cargo check 2>&1 | grep "unused import"
```

Expected: zero unused-import warnings after pruning. If the only flagged import is `JobSpecError`, delete `use crate::env::jobs::JobSpecError;` from `src/exec/mod.rs`.

- [ ] **Step 8: Build**

```bash
cargo build 2>&1 | tail -30
```

Expected: build succeeds with zero errors and zero warnings. If a `pub(super)` visibility error appears against the moved struct fields, double-check Step 4 applied `pub(super)` consistently to `last_status`, `process_statuses`, and `stopped`. If a "private function" error appears against `preview_command` from `job_control.rs::builtin_jobs`, double-check that `use super::preview_command;` is present in the import block at the top of `job_control.rs`.

- [ ] **Step 9: Run unit tests**

```bash
cargo test --lib exec:: 2>&1 | tail -10
```

Expected: passed-count matches Task 0 Step 1 baseline.

- [ ] **Step 10: Run full lib tests**

```bash
cargo test --lib 2>&1 | tail -5
```

Expected: passed-count matches Task 0 Step 2 baseline.

- [ ] **Step 11: Run plugin integration tests**

```bash
cargo test --features test-helpers 2>&1 | tail -10
```

Expected: full integration suite green.

- [ ] **Step 12: Run job-control e2e**

```bash
./e2e/run_tests.sh --filter=job 2>&1 | tail -10
```

Expected: pass count matches Task 0 Step 5 baseline. If this filter returns no tests in your environment, run `./e2e/run_tests.sh --filter=2_14_2_set` (special builtins) and `./e2e/run_tests.sh` for the full suite.

- [ ] **Step 13: Format check**

```bash
cargo fmt --all -- --check
```

Expected: clean. If rustfmt rewraps any of the moved bodies (long match arms in `builtin_wait`/`fg`/`wait_for_foreground_job` are likely candidates), run `cargo fmt --all` and re-verify Steps 8–12.

- [ ] **Step 14: Clippy**

```bash
cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: clean. Common transient findings after a split: `unused_imports` in `mod.rs` (handled in Step 7), or `module_inception` if a submodule was misnamed — neither should fire here.

- [ ] **Step 15: Commit**

```bash
git add src/exec/mod.rs src/exec/job_control.rs
git commit -m "$(cat <<'EOF'
refactor(exec): split job-control built-ins into src/exec/job_control.rs

Moves builtin_wait/jobs/fg/bg, wait_for_foreground_job,
record_stopped_state, restore_shell_termios_if_interactive,
display_job_notifications, the ForegroundWaitResult struct, and the
strip_job_spec_prefix helper out of src/exec/mod.rs into a new
src/exec/job_control.rs. Also relocates the three record_stopped_state
unit tests into the new submodule.

preview_command stays private in mod.rs; builtin_jobs reaches it via
use super::preview_command; because Rust private items are visible to
descendant modules, and crate::exec::job_control is a child of
crate::exec.

No public API surface changes. No behavior changes. Mirrors the parser
mod-split pattern from 2026-05-04.

Spec: docs/superpowers/specs/2026-05-05-exec-mod-split-design.md
Plan: docs/superpowers/plans/2026-05-05-exec-mod-split.md (Task 1)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Split `control.rs`

**Files:**
- Create: `src/exec/control.rs`
- Modify: `src/exec/mod.rs` (delete moved code; add `mod control;` declaration)

**What moves:**
- Production methods: `exec_command` (lines 247–279), `exec_and_or` (281–318), `reap_zombies` (320–351), `exec_async` (353–391), `exec_complete_command` (393–429), `exec_program` (431–442).
- Tests: 17 control-flow tests from the current `mod.rs::tests` (`exec_builtin_*`, `exec_external_*`, `assignment_only_sets_var`, `exit_status_tracked`, `test_single_command_pipeline`, `test_negated_pipeline`, `test_and_list_*` (4 tests), `test_or_list_first_*` (2 tests), `test_exec_program_sequential`, `exec_and_or_stops_after_*` (2 tests), `exec_simple_command_sets_lineno`, `exec_compound_command_sets_lineno`, `exec_compound_subshell_sets_lineno_on_entry`).
- Test helpers: `make_simple_cmd` (lines 967–974), `make_pipeline` (lines 1061–1071) — both used only by tests now in `control.rs`.

**Visibility plan for moved symbols:**

| Symbol | Old visibility | New visibility |
|--------|----------------|----------------|
| `Executor::exec_command` | `pub` | `pub` (unchanged — `pipeline.rs` calls `self.exec_command`) |
| `Executor::exec_and_or` | `pub` | `pub` (unchanged) |
| `Executor::exec_async` | private (`fn`) | private (`fn`) — only `exec_complete_command` (also in `control.rs`) calls it |
| `Executor::exec_complete_command` | `pub` | `pub` (unchanged — `compound.rs`, `interactive/mod.rs`, `main.rs` all call it) |
| `Executor::exec_program` | `pub` | `pub` (unchanged) |
| `Executor::reap_zombies` | `pub(crate)` | `pub(crate)` (unchanged — `interactive/mod.rs` calls it) |

- [ ] **Step 1: Locate and read the production lines being moved**

The exact line numbers depend on Task 1's deletions. Re-grep first:

```bash
grep -nE '^\s*pub fn exec_command|^\s*pub fn exec_and_or|^\s*pub\(crate\) fn reap_zombies|^\s*fn exec_async|^\s*pub fn exec_complete_command|^\s*pub fn exec_program' src/exec/mod.rs
```

Use the resulting line numbers as the `offset` for `Read src/exec/mod.rs offset=N limit=M`. Approximate body sizes (use as the `limit` value):

- `exec_command`: ~33 lines (doc + body).
- `exec_and_or`: ~38 lines.
- `reap_zombies`: ~32 lines.
- `exec_async`: ~39 lines.
- `exec_complete_command`: ~37 lines.
- `exec_program`: ~12 lines.

For each, read from the doc comment line (one or two lines above the `fn` signature reported by grep) through the closing `}`. Confirm exact content before editing.

- [ ] **Step 2: Locate and read the tests being moved**

After Task 1's deletions the test module starts well below line 959 (Task 1 removed ~616 lines total, including the three `record_stopped_state_*` tests). Re-grep the test starting points:

```bash
grep -nE '^\s*fn make_simple_cmd|^\s*fn make_pipeline|fn exec_builtin_true_returns_0|fn test_single_command_pipeline|fn test_and_list_all_succeed|fn test_exec_program_sequential|fn exec_and_or_stops_after_first_pipeline|fn exec_simple_command_sets_lineno|fn exec_compound_command_sets_lineno|fn exec_compound_subshell_sets_lineno_on_entry' src/exec/mod.rs
```

Use the reported line numbers as `offset` values. The 17 control-flow tests cluster contiguously, broken only by the 4 errexit tests + 5 misc tests that stay in `mod.rs`. Read each contiguous control-flow block plus the two helpers (`make_simple_cmd`, `make_pipeline`).

- [ ] **Step 3: Inspect external callers (information only)**

```bash
grep -n "exec_command\|exec_and_or\|exec_async\|exec_complete_command\|exec_program\|reap_zombies" \
    src/exec/*.rs src/interactive/*.rs src/main.rs 2>/dev/null \
    | grep -v "src/exec/mod.rs\|src/exec/control.rs"
```

Expected callers (preserved by leaving the methods at their existing visibility):

- `src/exec/compound.rs` line 66 → `self.exec_complete_command`
- `src/exec/pipeline.rs` lines 16, 105 → `self.exec_command`
- `src/interactive/mod.rs` lines 132, 262 → `self.executor.reap_zombies`, `self.executor.exec_complete_command`
- `src/main.rs` line 262 → `executor.exec_complete_command`

`exec_async` and `exec_and_or` are only invoked through the `control.rs`-internal callgraph (`exec_complete_command` → `exec_async`/`exec_and_or`); no external caller invokes them directly outside of their own tests.

- [ ] **Step 4: Create `src/exec/control.rs`**

Write `src/exec/control.rs` with the structure shown below. Replace each `// COPY VERBATIM ...` marker with the exact text captured in Steps 1–2. Visibility annotations match the visibility plan above (no changes to existing `pub` / `pub(crate)`).

```rust
use nix::unistd::{ForkResult, fork};

use super::{Executor, exit_child, preview_command};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::{
    AndOrList, AndOrOp, Command, CompleteCommand, Program, SeparatorOp,
};
use crate::signal;

impl Executor {
    /// Dispatch a `Command` to the appropriate execution path.
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        // COPY VERBATIM body from mod.rs lines 249–278
    }

    /// Execute an AND-OR list.
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        // COPY VERBATIM body from mod.rs lines 282–317
    }

    /// Reap any zombie background children without blocking.
    pub(crate) fn reap_zombies(&mut self) {
        // COPY VERBATIM body from mod.rs lines 322–351
    }

    /// Execute a command asynchronously (background with &).
    fn exec_async(&mut self, and_or: &AndOrList) -> Result<i32, ShellError> {
        // COPY VERBATIM body from mod.rs lines 354–390
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // COPY VERBATIM body from mod.rs lines 394–428
    }

    /// Execute a program (sequence of complete commands).
    pub fn exec_program(&mut self, program: &Program) -> i32 {
        // COPY VERBATIM body from mod.rs lines 432–441
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{
        AndOrList, AndOrOp, Command, CompleteCommand, CompoundCommand, CompoundCommandKind,
        Pipeline, Program, SeparatorOp, SimpleCommand, Word,
    };

    fn make_simple_cmd(words: &[&str]) -> SimpleCommand {
        // COPY VERBATIM body from mod.rs lines 967–974
    }

    fn make_pipeline(word: &str) -> Pipeline {
        // COPY VERBATIM body from mod.rs lines 1061–1071
    }

    #[test]
    fn exec_builtin_true_returns_0() {
        // COPY VERBATIM body from mod.rs (test of same name)
    }

    #[test]
    fn exec_builtin_false_returns_1() {
        // COPY VERBATIM body
    }

    #[test]
    fn exec_external_true_returns_0() {
        // COPY VERBATIM body
    }

    #[test]
    fn assignment_only_sets_var() {
        // COPY VERBATIM body
    }

    #[test]
    fn exit_status_tracked() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_single_command_pipeline() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_negated_pipeline() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_and_list_all_succeed() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_and_list_first_fails() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_or_list_first_fails() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_or_list_first_succeeds() {
        // COPY VERBATIM body
    }

    #[test]
    fn test_exec_program_sequential() {
        // COPY VERBATIM body
    }

    #[test]
    fn exec_and_or_stops_after_first_pipeline_when_exit_requested() {
        // COPY VERBATIM body
    }

    #[test]
    fn exec_and_or_stops_after_rest_pipeline_when_exit_requested() {
        // COPY VERBATIM body
    }

    #[test]
    fn exec_simple_command_sets_lineno() {
        // COPY VERBATIM body
    }

    #[test]
    fn exec_compound_command_sets_lineno() {
        // COPY VERBATIM body
    }

    #[test]
    fn exec_compound_subshell_sets_lineno_on_entry() {
        // COPY VERBATIM body
    }
}
```

**Notes on imports:**
- `exit_child` and `preview_command` are imported from `super::` — `super` is `crate::exec`. `exit_child` is `pub(crate)` so the import is unconditionally allowed; `preview_command` is private in `mod.rs`, but Rust grants descendant modules access to a parent's private items, so the `use super::preview_command;` line in this child module is also valid (verified by the same mechanism `job_control.rs` relies on after Task 1).
- `signal` and `crate::error::*` are needed because `exec_async` constructs a `ShellError` and calls `signal::setup_background_child_signals` / `signal::reset_child_signals`.
- The body of `exec_async` references `nix::unistd::getpid` and `nix::unistd::setpgid` fully qualified — keep those usages as-is rather than adding fresh imports.
- Tests reference `Executor::new` via `super::Executor` (re-exported through `super::*`); the existing in-test `Executor::new("yosh", vec![])` calls do not need any rewrite.

- [ ] **Step 5: Update `src/exec/mod.rs` — remove moved code**

Use `Edit` to delete each of the following items (use the line numbers from Step 1's grep output, not the pre-Task-1 numbers from the spec):

1. `exec_command` method (doc + body).
2. `exec_and_or` method (doc + body).
3. `reap_zombies` method (doc + body).
4. `exec_async` method (doc + body).
5. `exec_complete_command` method (doc + body).
6. `exec_program` method (doc + body).
7. The 17 control-flow tests inside `mod tests` listed in Step 4 (do not delete the errexit / exit_requested / plugin_config_path / source_file / handle_default_signal / check_errexit tests — those stay in `mod.rs`).
8. The two test helpers `make_simple_cmd` and `make_pipeline`.

Keep the remaining tests in `mod tests` (errexit, exit_requested, plugin_config_path, source_file, handle_default_signal, check_errexit — these stay in `mod.rs`).

- [ ] **Step 6: Update `src/exec/mod.rs` — declare new submodule**

Add `mod control;` to the existing `mod` declaration block. The block now reads:

```rust
pub mod command;
mod compound;
mod control;
mod function;
mod job_control;
pub mod pipeline;
pub mod redirect;
mod simple;
pub(crate) mod terminal_state;
```

- [ ] **Step 7: Drop now-unused imports in `src/exec/mod.rs`**

After removing the control-flow bodies, the top-of-file imports may include items now used only by `control.rs`. Specifically:

- `nix::unistd::{ForkResult, fork}` — only used by `exec_async`. Remove.
- `crate::parser::ast::{AndOrList, AndOrOp, Command, CompleteCommand, Program, SeparatorOp, WordPart}` — `WordPart` is still used by `preview_command` (which stays). The other AST types were used by the moved methods. Trim to `use crate::parser::ast::WordPart;`.
- `crate::env::jobs::JobSpecError` — already removed in Task 1 Step 8 if applicable.
- `crate::signal` — still used by `process_pending_signals` (which stays). Keep.

Run `cargo check 2>&1 | grep "unused import"` after edits and remove any lines flagged.

- [ ] **Step 8: Build**

```bash
cargo build 2>&1 | tail -30
```

Expected: zero errors, zero warnings.

- [ ] **Step 9: Run unit tests**

```bash
cargo test --lib exec:: 2>&1 | tail -10
```

Expected: passed-count matches Task 0 Step 1 baseline.

- [ ] **Step 10: Run full lib + integration tests**

```bash
cargo test --lib 2>&1 | tail -5 && cargo test --features test-helpers 2>&1 | tail -5
```

Expected: passed-counts match Task 0 Step 2 baseline (plus the integration-test count). Zero failures.

- [ ] **Step 11: Run e2e**

```bash
./e2e/run_tests.sh 2>&1 | tail -10
```

Expected: full e2e suite green. The control-flow split touches the most heavily-exercised code path in the shell, so a regression here is likely to surface in dozens of e2e tests rather than just job-control ones.

- [ ] **Step 12: Format & clippy**

```bash
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: both clean. If fmt rewraps the moved bodies, run `cargo fmt --all` and re-verify Steps 8–11.

- [ ] **Step 13: Commit**

```bash
git add src/exec/mod.rs src/exec/control.rs
git commit -m "$(cat <<'EOF'
refactor(exec): split execution control flow into src/exec/control.rs

Moves exec_command, exec_and_or, exec_async, exec_complete_command,
exec_program, and reap_zombies from src/exec/mod.rs into a new
src/exec/control.rs, alongside the 17 control-flow unit tests and
their make_simple_cmd / make_pipeline helpers.

Visibility is preserved verbatim: exec_command / exec_and_or /
exec_complete_command / exec_program stay pub, reap_zombies stays
pub(crate), exec_async stays private. exec_async reaches
preview_command via use super::preview_command; — descendant modules
can see a parent's private items, so no visibility change to
preview_command was needed.

No public API surface changes. No behavior changes. Mirrors the parser
mod-split pattern from 2026-05-04.

Spec: docs/superpowers/specs/2026-05-05-exec-mod-split-design.md
Plan: docs/superpowers/plans/2026-05-05-exec-mod-split.md (Task 2)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Final Cleanup & DoD Verification

**Files:**
- Modify: `src/exec/mod.rs` (only if cleanup surfaces orphaned items)
- Modify: `docs/superpowers/specs/2026-05-05-exec-mod-split-design.md` (fill §7 Final Line Counts)

- [ ] **Step 1: Re-confirm `mod.rs` is the expected size and content**

```bash
wc -l src/exec/mod.rs src/exec/control.rs src/exec/job_control.rs
```

Expected (approximate):
- `src/exec/mod.rs`: ≤ 450 (target ~430).
- `src/exec/control.rs`: ~200 production + ~270 tests = ~470 total.
- `src/exec/job_control.rs`: ~700 production + ~100 tests = ~800 total.

If `mod.rs` exceeds 450, identify the surplus by:

```bash
grep -nE '^\s*(pub )?fn |^impl ' src/exec/mod.rs
```

and check whether any item should have moved in Tasks 1–2 (it should not — the spec inventory was complete).

- [ ] **Step 2: Confirm no stale `mod` declarations or `use` lines**

```bash
grep -nE '^mod |^pub mod |^pub\(crate\) mod ' src/exec/mod.rs
```

Expected output exactly:

```
pub mod command;
mod compound;
mod control;
mod function;
mod job_control;
pub mod pipeline;
pub mod redirect;
mod simple;
pub(crate) mod terminal_state;
```

```bash
cargo check 2>&1 | grep "unused"
```

Expected: empty.

- [ ] **Step 3: Run full DoD checklist**

```bash
cargo build && \
cargo clippy --all-targets -- -D warnings && \
cargo test && \
cargo test --features test-helpers && \
cargo fmt --all -- --check && \
./e2e/run_tests.sh
```

Expected: every command exits 0. If any fails, fix locally and re-run before proceeding.

- [ ] **Step 4: Update spec §7 with final line counts**

Read the bottom of `docs/superpowers/specs/2026-05-05-exec-mod-split-design.md` and fill in §7. Replace the placeholder block with the actual values from Step 1:

```markdown
## 7. Final Line Counts (filled post-implementation)

- `src/exec/mod.rs`: <actual> lines
- `src/exec/control.rs`: <actual> lines
- `src/exec/job_control.rs`: <actual> lines
```

- [ ] **Step 5: Commit cleanup**

If Step 1 needed an edit to `mod.rs`, or Step 4 updated the spec, commit:

```bash
git add docs/superpowers/specs/2026-05-05-exec-mod-split-design.md src/exec/mod.rs
git commit -m "$(cat <<'EOF'
refactor(exec): record final line counts from exec/mod.rs split

Updates §7 of the exec mod-split spec with the actual post-implementation
line counts for mod.rs, control.rs, and job_control.rs. No code changes
expected from this commit.

Plan: docs/superpowers/plans/2026-05-05-exec-mod-split.md (Task 3)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If Step 1 produced no edits (which is the expected case), skip the commit and document the final line counts in the PR description / wrap-up message instead.

- [ ] **Step 6: Run TODO follow-up scan**

This split is intentionally narrow. After the merge, audit whether any of the deferred items below should be filed in `TODO.md`:

- Visibility tightening of `exec_program` / `exec_command` / `exec_and_or` (currently `pub`) — the parser visibility-tightening spec from 2026-05-05 is the precedent. File a follow-up TODO if no spec exists yet.
- `display_job_notifications` is `pub` but called only from `interactive/mod.rs`. A `pub(crate)` tightening is reasonable — also a follow-up TODO.
- `make_simple_cmd` / `make_pipeline` are now duplicated between `control.rs::tests` and any sibling test that needs them in the future. If a similar helper appears in `simple.rs::tests` or `compound.rs::tests`, deduplicate at that point — not now (YAGNI).

Add any items as `- [ ]` lines under TODO.md's "Future: Code Quality Improvements" section. No commit required for the audit itself; only commit if a TODO entry is added.

---

## Definition of Done (whole plan)

- [ ] `src/exec/mod.rs` line count ≤ 450 (Task 3 Step 1).
- [ ] `src/exec/control.rs` and `src/exec/job_control.rs` exist with the scope described in the File Structure table.
- [ ] `cargo build` clean.
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `cargo test` passes (lib + integration, count matches baseline).
- [ ] `cargo test --features test-helpers` passes.
- [ ] `./e2e/run_tests.sh` passes.
- [ ] `cargo fmt --all -- --check` clean.
- [ ] Spec §7 Final Line Counts populated.
- [ ] Three commits landed: `refactor(exec): split job-control built-ins ...`, `refactor(exec): split execution control flow ...`, optionally `refactor(exec): record final line counts ...`.
- [ ] No public API change visible to callers outside `src/exec/`.
