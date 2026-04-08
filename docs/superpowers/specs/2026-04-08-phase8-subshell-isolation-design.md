# Phase 8: Subshell Environment Isolation

## Overview

Phase 8 fixes remaining gaps in subshell environment isolation and adds a comprehensive test suite to verify POSIX compliance (XCU §2.12).

The fork()-based architecture already provides most of the required isolation. This phase addresses the few remaining gaps and proves correctness through ~36 integration tests.

## Approach

Gap fixes first, then comprehensive test suite (Approach 1: fix-then-verify).

## Code Modifications

### 1. Pipeline trap reset (primary fix)

**File:** `src/exec/pipeline.rs`
**Location:** Line 67, inside `ForkResult::Child` branch of `exec_multi_pipeline`
**Change:** Add `self.env.traps.reset_non_ignored()` before `signal::reset_child_signals()`

Currently, pipeline child processes reset OS-level signal handlers but do not reset the shell-level `TrapStore`. POSIX §2.12 requires that command traps are reset to default in subshell environments, which includes each command in a multi-command pipeline.

### 2. exec_subshell consistency (minor)

**File:** `src/exec/mod.rs`
**Location:** `exec_subshell` method (~line 193)
**Change:** Ensure trap reset and signal reset ordering is consistent with `command_sub.rs`

The current implementation works correctly due to fork() semantics, but the explicit ordering should match the pattern used in command substitution for maintainability.

### 3. No structural changes

- No changes to `ShellEnv`, `VarStore`, `AliasStore`, or `TrapStore` definitions
- No changes to lexer or parser
- No new modules or types

## Test Suite Design

New file: `tests/subshell.rs` (~36 integration tests)

All tests use the existing `kish_exec` helper (external process execution).

### Category 1: `( ... )` Subshell (~12 tests)

| Test | POSIX Requirement | Description |
|------|-------------------|-------------|
| variable_isolation | §2.12 variables | Variable changes in subshell don't affect parent |
| function_isolation | §2.12 functions | Function def/redef in subshell don't affect parent |
| alias_isolation | §2.12 aliases | Alias changes in subshell don't affect parent |
| trap_command_reset | §2.12 traps | Command traps are reset in subshell |
| trap_ignore_inherited | §2.12 traps | Ignore traps are preserved in subshell |
| option_isolation | §2.12 options | `set -x` in subshell doesn't affect parent |
| dollar_dollar_is_parent_pid | §2.5.2 | `$$` returns parent shell PID in subshell |
| exit_status_propagation | §2.12 | Subshell exit status propagates to parent |
| readonly_inherited | §2.12 variables | Readonly variables are inherited by subshell |
| positional_params_isolation | §2.12 | `$1`, `$#`, `$@` inherited, changes don't affect parent |
| cwd_inheritance | §2.12 working dir | Subshell inherits parent's working directory |
| cwd_isolation | §2.12 working dir | `cd` in subshell doesn't affect parent |

### Category 2: Pipeline (~7 tests)

| Test | POSIX Requirement | Description |
|------|-------------------|-------------|
| pipeline_variable_isolation | §2.12 | Variable changes in pipeline don't affect parent |
| pipeline_trap_reset | §2.12 traps | Command traps are reset in pipeline commands |
| pipeline_function_isolation | §2.12 | Function def in pipeline doesn't affect parent |
| pipeline_cwd_isolation | §2.12 working dir | `cd` in pipeline doesn't affect parent |
| pipeline_option_isolation | §2.12 options | Option changes in pipeline don't affect parent |
| pipeline_pipefail | pipefail | Pipefail interacts correctly with subshell isolation |
| pipeline_exit_status | §2.9.2 | Last command's exit status is pipeline's status |

### Category 3: Command Substitution `$(...)` (~7 tests)

| Test | POSIX Requirement | Description |
|------|-------------------|-------------|
| cmdsub_variable_isolation | §2.12 | Variable changes in `$(...)` don't affect parent |
| cmdsub_exit_status | §2.6.3 | `$?` reflects command substitution's exit status |
| cmdsub_nested_isolation | §2.12 | Nested `$($(..))` maintains isolation |
| cmdsub_trap_isolation | §2.12 traps | Traps in command substitution are reset |
| cmdsub_function_isolation | §2.12 | Function def in `$(...)` doesn't affect parent |
| cmdsub_positional_params | §2.12 | Positional params inherited, changes don't affect parent |
| cmdsub_cwd_isolation | §2.12 working dir | `cd` in `$(...)` doesn't affect parent |

### Category 4: Edge Cases (~10 tests)

| Test | POSIX Requirement | Description |
|------|-------------------|-------------|
| nested_subshell | §2.12 | `( ( ... ) )` maintains isolation at each level |
| subshell_exit_no_parent | §2.12 | `exit` in subshell doesn't terminate parent |
| subshell_errexit | §2.12 + errexit | `set -e` interacts correctly with subshell |
| umask_inheritance | §2.12 file creation mask | Subshell inherits parent's umask |
| umask_isolation | §2.12 file creation mask | umask changes in subshell don't affect parent |
| fd_inheritance | §2.12 open files | Open file descriptors are inherited by subshell |
| export_and_non_export | §2.12 variables | Both exported and non-exported vars available in subshell |
| last_bg_pid_inheritance | §2.12 | `$!` is inherited by subshell |
| subshell_return_error | §2.12 | `return` outside function in subshell is an error |
| deeply_nested_isolation | §2.12 | 3+ levels of nesting maintain isolation |

## Files Changed

| File | Change |
|------|--------|
| `src/exec/pipeline.rs` | Add trap reset in pipeline child (1 line) |
| `src/exec/mod.rs` | Minor consistency improvement in `exec_subshell` |
| `tests/subshell.rs` | New file: ~36 integration tests |
| `tests/helpers/mod.rs` | Add helpers if needed (e.g., umask operations) |
| `TODO.md` | Record Phase 8 completion |

## Risk Assessment

- **Low risk:** Modifications are minimal (1 primary fix)
- **No breaking changes:** Fork-based isolation already works; we're adding a missing trap reset
- **No architectural changes:** All changes within existing patterns
