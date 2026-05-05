# Exec Module Split Design

**Date**: 2026-05-05
**Status**: Design proposed
**Scope**: Mechanical split of `src/exec/mod.rs` (1460 lines) into focused
submodules.

## Overview

`src/exec/mod.rs` has grown to 1460 lines: roughly 950 lines of production
code plus 500 lines of tests in a single `#[cfg(test)] mod tests`. Although
many execution categories already live in dedicated sibling files
(`simple.rs`, `compound.rs`, `pipeline.rs`, `function.rs`, `command.rs`,
`redirect.rs`, `terminal_state.rs`), `mod.rs` still hosts two large
clusters that have no responsibility-specific home:

- **Execution control flow** — the `Executor` driver methods that route
  parsed AST nodes to the right execution path (`exec_program`,
  `exec_complete_command`, `exec_and_or`, `exec_async`, `exec_command`,
  `reap_zombies`).
- **Job-control built-ins** — `wait`/`jobs`/`fg`/`bg` plus their helpers
  (`wait_for_foreground_job`, `record_stopped_state`,
  `restore_shell_termios_if_interactive`, `display_job_notifications`,
  `ForegroundWaitResult`, `strip_job_spec_prefix`).

This spec splits these two clusters out, mirroring the already-completed
`src/parser/mod.rs` split (2026-05-04). The split is purely mechanical:
no `fn` body or signature is modified, and the only visibility changes are
the minimum `pub(super)` annotations required so the moved methods remain
reachable from sibling submodules.

## Goals

1. Reduce `src/exec/mod.rs` to ~430 lines (production + tests) holding the
   `Executor` struct, constructors, signal/errexit/eval methods, and shared
   free helpers (`exit_child`, `preview_command`, `plugin_config_path`).
2. Co-locate execution-control logic and its tests in a new
   `src/exec/control.rs`.
3. Co-locate all job-control built-in logic and its tests in a new
   `src/exec/job_control.rs`.
4. Preserve every externally referenced symbol's visibility, name, and
   signature so that no caller outside `src/exec/` needs to change.
5. Each step of the split is an independent commit that builds and passes
   tests on its own — no transient broken state across commits.

## Non-Goals

- **API visibility tightening**: broad reduction of `pub fn` to
  `pub(crate)` / `pub(super)` is deferred to a follow-up spec, mirroring
  the parser visibility-tightening pattern. Only the minimum `pub(super)`
  annotations needed for cross-submodule reachability are added here.
- **Method body changes**: no logic edits, no helper extraction, no
  optimization.
- **Behavioral changes**: job-control bug fixes, errexit refinements,
  signal-handling tweaks are out of scope.
- **TODO follow-ups**: `find_in_path` vs `lookup_in_path` consolidation,
  `exec_regular_builtin` dispatch-table refactor, and other TODO.md items
  remain unaddressed.
- **Documentation rewrites**: existing doc comments are preserved verbatim;
  no new documentation is written as part of the split.
- **Sibling-file edits**: `simple.rs`, `compound.rs`, `pipeline.rs`,
  `function.rs`, `command.rs`, `redirect.rs`, `terminal_state.rs` are not
  modified beyond what is required if a moving free helper changes path
  (none expected — see §3 helper analysis).

## Target Module Layout

```
src/exec/
├── mod.rs              Executor struct + constructors + signal handling +
│                       errexit policy + eval/source + shared helpers
│                       (exit_child, preview_command, plugin_config_path).
├── control.rs          NEW. Execution control flow: exec_program,
│                       exec_complete_command, exec_and_or, exec_async,
│                       exec_command, reap_zombies.
├── job_control.rs      NEW. Job-control built-ins: builtin_wait,
│                       builtin_jobs, builtin_fg, builtin_bg, plus helpers
│                       (wait_for_foreground_job, record_stopped_state,
│                       restore_shell_termios_if_interactive,
│                       display_job_notifications, ForegroundWaitResult,
│                       strip_job_spec_prefix).
├── command.rs          Unchanged.
├── compound.rs         Unchanged.
├── function.rs         Unchanged.
├── pipeline.rs         Unchanged.
├── redirect.rs         Unchanged.
├── simple.rs           Unchanged.
└── terminal_state.rs   Unchanged.
```

Each new submodule contributes additional `impl Executor { ... }` blocks;
the `Executor` struct itself is defined only in `mod.rs`. `mod.rs` declares
the new submodules with `mod control; mod job_control;` — both private
because their public surface (the `pub` methods on `Executor`) reaches
external callers transparently through the struct.

## 1. Symbol Migration

### 1.1 Stays in `mod.rs`

| Symbol | Visibility | Rationale |
| --- | --- | --- |
| `Executor` struct | `pub` | Type definition must have a single home; sibling submodules add `impl` blocks. |
| `Executor::new`, `from_env`, `load_plugins` | `pub` | Constructors / lifecycle. |
| `Executor::source_file`, `eval_string`, `verbose_print` | `pub` | Eval/source entry points; called from `main.rs`, `interactive/mod.rs`. |
| `Executor::with_errexit_suppressed`, `should_errexit`, `check_errexit` | `pub` | Errexit policy, used by `compound.rs`, `main.rs`. |
| `Executor::process_pending_signals`, `handle_default_signal`, `execute_exit_trap` | `pub` / `pub(crate)` | Signal handling; many external call sites. |
| `exit_child` (free fn) | `pub(crate)` | Post-fork helper used by `simple.rs`, `compound.rs`, `pipeline.rs`, and the new `control.rs` (`exec_async`). |
| `preview_command` (free fn) | `fn` (private, unchanged) | Used by both `control.rs::exec_async` and `job_control.rs::builtin_jobs`. Stays private: Rust private items are visible to descendant modules, and both new submodules are direct children of `crate::exec` (where `preview_command` is defined). No promotion is required. |
| `plugin_config_path` (free fn) | `fn` (private) | Used only by `Executor::load_plugins`. |

### 1.2 Moves to `control.rs`

| Symbol | New visibility | External callers |
| --- | --- | --- |
| `Executor::exec_command` | `pub` | Called as `self.exec_command(...)` by `pipeline.rs`. |
| `Executor::exec_and_or` | `pub` | Called as `self.exec_and_or(...)` internally. |
| `Executor::exec_async` | `fn` (private) | Used only inside `control.rs::exec_complete_command`. |
| `Executor::exec_complete_command` | `pub` | Called by `compound.rs`, `interactive/mod.rs`, `main.rs`. |
| `Executor::exec_program` | `pub` | Currently `pub`; preserved verbatim per non-goal §"API visibility tightening". |
| `Executor::reap_zombies` | `pub(crate)` | Called by `interactive/mod.rs`. |

### 1.3 Moves to `job_control.rs`

| Symbol | New visibility | External callers |
| --- | --- | --- |
| `Executor::builtin_wait` | `pub(super)` | Called as `self.builtin_wait(...)` by `simple.rs`. |
| `Executor::builtin_jobs` | `pub(super)` | Called as `self.builtin_jobs(...)` by `simple.rs`. |
| `Executor::builtin_fg` | `pub(super)` | Called as `self.builtin_fg(...)` by `simple.rs`. |
| `Executor::builtin_bg` | `pub(super)` | Called as `self.builtin_bg(...)` by `simple.rs`. |
| `Executor::wait_for_foreground_job` | `pub(super)` | Called as `self.wait_for_foreground_job(...)` by `pipeline.rs`, `simple.rs`. |
| `Executor::restore_shell_termios_if_interactive` | `pub(super)` | Called as `self.restore_shell_termios_if_interactive()` by `pipeline.rs`, `simple.rs`. |
| `Executor::record_stopped_state` | `fn` (private) | Used only by `wait_for_foreground_job`. |
| `Executor::display_job_notifications` | `pub` | Called by `interactive/mod.rs` and by `Executor::exec_complete_command` (now in `control.rs`). |
| `ForegroundWaitResult` (struct) | `pub(super)` (struct + fields) | Returned by `wait_for_foreground_job`; accessed via field reads in `pipeline.rs` and `simple.rs`. The type name itself is not imported by callers, so `pub(super)` on the type plus `pub(super)` on `last_status` / `process_statuses` / `stopped` is sufficient. |
| `strip_job_spec_prefix` (free fn) | `fn` (private) | Used only by `builtin_wait` / `builtin_fg` / `builtin_bg`. |

### 1.4 Public API surface — unchanged

Every existing `pub` method on `Executor` retains its `pub` visibility.
Public free fns and types either stay in `mod.rs` (`exit_child` is
`pub(crate)`, unchanged) or move with a non-tightened visibility. No
external caller (`main.rs`, `interactive/mod.rs`, `tests/`, sibling
`src/exec/*.rs` files) needs source changes for the move itself.

## 2. Tests Migration

The existing `mod.rs` `tests` module is split along the same boundaries as
the production code. Each new submodule grows its own `#[cfg(test)] mod
tests { use super::*; ... }` block.

### 2.1 Move to `control.rs::tests`

- `exec_builtin_true_returns_0`
- `exec_builtin_false_returns_1`
- `exec_external_true_returns_0`
- `assignment_only_sets_var`
- `exit_status_tracked`
- `test_single_command_pipeline`
- `test_negated_pipeline`
- `test_and_list_all_succeed`
- `test_and_list_first_fails`
- `test_or_list_first_fails`
- `test_or_list_first_succeeds`
- `test_exec_program_sequential`
- `exec_and_or_stops_after_first_pipeline_when_exit_requested`
- `exec_and_or_stops_after_rest_pipeline_when_exit_requested`
- `exec_simple_command_sets_lineno`
- `exec_compound_command_sets_lineno`
- `exec_compound_subshell_sets_lineno_on_entry`
- Test helpers used by these (`make_simple_cmd`, `make_pipeline`)
  follow the tests they support.

### 2.2 Move to `job_control.rs::tests`

- `record_stopped_state_clears_stale_saved_tmodes_on_none_capture`
- `record_stopped_state_stores_some_capture`
- `record_stopped_state_no_op_on_unknown_job`

### 2.3 Stay in `mod.rs::tests`

- `test_should_errexit_default_off`
- `test_should_errexit_enabled`
- `test_with_errexit_suppressed`
- `test_with_errexit_suppressed_nested`
- `plugin_config_path_points_to_lock_file`
- `exit_requested_defaults_to_none`
- `handle_default_signal_sets_exit_requested_in_interactive_mode`
- `check_errexit_sets_exit_requested_in_interactive_mode`
- `source_file_nonexistent_returns_none`
- `source_file_sets_variable`
- `source_file_parse_error_returns_some_2`

Test bodies are not modified; only their containing module changes. Each
submodule's `tests` mod uses `use super::*;` to reach the moved private
helpers.

## 3. Helper Analysis (cross-submodule reachability)

- `exit_child` (mod.rs) is called by four sibling files plus the new
  `control.rs::exec_async`. Visibility unchanged at `pub(crate)`.
- `preview_command` (mod.rs) is called by both `control.rs::exec_async`
  (parent process side, after the background fork) and
  `job_control.rs::builtin_jobs` (preview formatting in the `jobs`
  output). It stays private. Rust visibility rules grant descendant
  modules access to a parent's private items, and both new submodules
  are direct children of `crate::exec`, so a plain `use super::preview_command;`
  inside each submodule is sufficient.
- `strip_job_spec_prefix` is called only by job-control built-ins — moves
  with them as a private fn in `job_control.rs`.
- `ForegroundWaitResult` is returned by `wait_for_foreground_job` and
  consumed by field access in `pipeline.rs` and `simple.rs`. Neither
  sibling imports the type by name, so a `pub(super) struct
  ForegroundWaitResult` with `pub(super)` fields preserves all current
  call sites.

## 4. Risks and Mitigations

- **Risk**: A moved method silently changes visibility and a sibling
  submodule fails to compile.
  **Mitigation**: Each migration step is followed by `cargo build` and
  `cargo test`; the migration table in §1 lists every external call site
  so unexpected failures are easy to attribute.
- **Risk**: `ForegroundWaitResult` field visibility is too tight, breaking
  `pipeline.rs` / `simple.rs` field access.
  **Mitigation**: §1.3 specifies `pub(super)` on both struct and fields;
  parent module is `src/exec/`, so all sibling submodules retain access.
  `cargo build` on Step 1 catches any regression immediately.
- **Risk**: Test relocation breaks compilation because a test helper
  (`make_simple_cmd`, `make_pipeline`) referenced from multiple test mods
  is moved with only one of them.
  **Mitigation**: Each helper moves with the tests that use it; if a
  helper turns out to be shared, it becomes a small `pub(super)` item in
  `mod.rs::tests` (a `#[cfg(test)] pub(super) mod test_helpers` pattern,
  added on demand). Initial scan suggests `make_simple_cmd` is used only
  by control-flow tests and `make_pipeline` only by pipeline-related
  tests — no shared helper conflict expected.
- **Risk**: Importing the new submodules with `mod` declarations in
  `mod.rs` introduces ordering issues with the existing `pub mod
  command;` / `pub(crate) mod terminal_state;` lines.
  **Mitigation**: New `mod` declarations are added in the existing
  declaration block at the top of `mod.rs`; both new modules are private
  (`mod control;`, `mod job_control;`) since their public surface is
  reached through `Executor`.

## 5. Definition of Done

1. `cargo build` is clean.
2. `cargo clippy --all-targets -- -D warnings` is clean.
3. `cargo test` passes (full suite, including `bin/`-targeted tests).
4. `cargo test --features test-helpers` passes (plugin integration suite).
5. `./e2e/run_tests.sh` passes (POSIX compliance, including job-control
   coverage which is heavily exercised).
6. `cargo fmt --all -- --check` is clean.
7. `wc -l src/exec/mod.rs` is ≤ 450 lines (target ~430).
8. `wc -l src/exec/control.rs` and `wc -l src/exec/job_control.rs` are
   recorded at the bottom of this spec post-implementation, alongside the
   new `mod.rs` line count.

## 6. Implementation Order (high-level — detailed plan in writing-plans phase)

1. **Step 1 — Create `job_control.rs`**: move job-control built-ins,
   helpers, `ForegroundWaitResult`, and their tests. Update
   `mod job_control;` in `mod.rs`. Adjust visibility of cross-submodule
   touch points (`pub(super)` on moved methods, struct, and fields).
   `builtin_jobs` reaches `preview_command` (still in `mod.rs`) via
   `use super::preview_command;` — no promotion needed because
   `job_control` is a descendant of `crate::exec`. Verify `cargo build`
   + `cargo test` + `./e2e/run_tests.sh` clean. Commit.
2. **Step 2 — Create `control.rs`**: move execution-control methods
   (`exec_program`, `exec_complete_command`, `exec_and_or`, `exec_async`,
   `exec_command`, `reap_zombies`) and their tests. Update `mod control;`
   in `mod.rs`. `exec_async` reaches `preview_command` the same way
   `builtin_jobs` did in Step 1: via `use super::preview_command;`.
   Verify the same DoD checks. Commit.
3. **Step 3 — Cleanup pass**: confirm `mod.rs` declarations are tidy,
   remove any orphaned imports, run `cargo fmt --all`, re-run the full
   DoD checklist, record final line counts at the bottom of this spec
   document. Commit.

## 7. Final Line Counts (filled post-implementation)

- `src/exec/mod.rs`: TBD
- `src/exec/control.rs`: TBD
- `src/exec/job_control.rs`: TBD
