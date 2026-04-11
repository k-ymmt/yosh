# Phase 8: Known Limitations Design

## Overview

Resolve four known limitations from Phase 8 of TODO.md related to subshell behavior:

1. `umask` builtin not implemented
2. `exec N>file` fd persistence not implemented
3. `test_umask_isolation` passing incidentally (resolved by item 1)
4. `return` outside function in subshell error not implemented

## Item 1: `umask` Builtin

### Classification

Regular builtin (not special per POSIX).

### Interface

- `umask` — display current umask as 4-digit octal (e.g., `0022`)
- `umask -S` — display as symbolic (e.g., `u=rwx,g=rx,o=rx`)
- `umask 027` — set via octal number
- `umask u=rwx,g=rx,o=` — set via symbolic mode

### Implementation

**Location**: `src/builtin/mod.rs`

- Add `"umask"` to `classify_builtin` → `BuiltinKind::Regular`
- Add `"umask"` case to `exec_regular_builtin` → call `builtin_umask`
- Implement `builtin_umask(args: &[String]) -> i32`

**Reading the current umask**: `libc::umask()` has a side effect — it sets the new value and returns the old one. Use the pattern:
```rust
let current = unsafe { libc::umask(0) };
unsafe { libc::umask(current) };
```

**Octal display**: Format as `{:04o}` for 4-digit octal output.

**Symbolic display**: Convert umask (denial bits) to permission (allowed bits) by inverting against `0o777`:
- `u=` permissions from bits `(0o777 - umask) & 0o700`
- `g=` permissions from bits `(0o777 - umask) & 0o070`
- `o=` permissions from bits `(0o777 - umask) & 0o007`
- Map each group: bit 4→`r`, bit 2→`w`, bit 1→`x`

**Symbolic parse**: Parse `[ugoa]*[=+-][rwx]*` comma-separated entries:
- `who`: `u` (user), `g` (group), `o` (other), `a` (all = ugo); if omitted, defaults to `a`
- `op`: `=` (set exact), `+` (add permissions / clear umask bits), `-` (remove permissions / set umask bits)
- `perm`: `r` (read), `w` (write), `x` (execute)
- For `=`: set the umask bits for the specified `who` to the inverse of `perm`
- For `+`: clear the umask bits corresponding to `perm` for the specified `who`
- For `-`: set the umask bits corresponding to `perm` for the specified `who`

**Octal parse**: Validate digits are 0-7 and value fits in mode_t.

### Error handling

- Invalid octal digits → `"kish: umask: <value>: invalid octal number"`
- Invalid symbolic syntax → `"kish: umask: <value>: invalid symbolic mode"`

### Subshell behavior

umask is a process attribute inherited via `fork()`. No special handling needed — fork-based subshell isolation works correctly.

### Tests

- Remove `#[ignore]` from `test_umask_inheritance` in `tests/subshell.rs`
- `test_umask_isolation` already passes and will now pass for the correct reason

## Item 2: `exec` Redirect Persistence

### Problem

Currently, `exec` is handled like any other special builtin in `exec_simple_command`: redirects are applied with `save=true` and restored afterward. When `exec` has no command arguments (e.g., `exec 3>file`), the redirects should persist in the current shell environment.

### Implementation

**Location**: `src/exec/mod.rs`, in the `BuiltinKind::Special` branch of `exec_simple_command`

**Change**: Before the general special builtin redirect handling, detect the `exec`-with-no-args case:

```
if command_name == "exec" && args.is_empty() {
    // Apply redirects without saving (they persist)
    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, false) {
        eprintln!("kish: {}", e);
        self.env.last_exit_status = 1;
        return 1;
    }
    // Do NOT call restore — redirects are permanent
    self.env.last_exit_status = 0;
    return 0;
}
```

When `exec` has arguments, the existing behavior is correct — `execvp` replaces the current process, so the restore never runs and redirects are inherited by the new program.

### Tests

- Remove `#[ignore]` from `test_fd_inheritance` in `tests/subshell.rs`
- Test verifies: `exec 3>/tmp/file; (echo hello >&3); cat /tmp/file` produces `hello`

## Item 3: `test_umask_isolation`

No code changes needed. The test already passes due to fork isolation. With the `umask` builtin implemented (Item 1), it passes for the correct reason — umask changes in the child process do not affect the parent.

The TODO item's concern was that the test passed "incidentally." After Item 1, it passes correctly.

## Item 4: `return` Outside Function Error

### POSIX Requirement

`return` is only valid inside a function body or a dot script (`. file`). Using `return` outside these contexts is an error.

### Implementation

**`src/env/vars.rs`**: Add scope depth accessor:
```rust
pub fn scope_depth(&self) -> usize {
    self.scopes.len()
}
```

**`src/env/mod.rs`**: Add `in_dot_script: bool` field to `ShellEnv`, initialized to `false`.

**`src/builtin/special.rs`** — `builtin_return`:
```rust
fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
    // Check if we're in a function (scope_depth > 1) or dot script
    if env.vars.scope_depth() <= 1 && !env.in_dot_script {
        eprintln!("kish: return: can only return from a function or sourced script");
        return 1;
    }
    // ... existing code ...
}
```

**`src/builtin/special.rs`** — `builtin_source`: Wrap execution with `in_dot_script` flag:
```rust
fn builtin_source(args: &[String], executor: &mut Executor) -> i32 {
    // ... existing file reading code ...
    let prev = executor.env.in_dot_script;
    executor.env.in_dot_script = true;
    let status = /* parse and execute */;
    executor.env.in_dot_script = prev;
    status
}
```

### Tests

Add to `tests/subshell.rs`:
```rust
#[test]
fn test_return_outside_function_error() {
    let out = kish_exec("(return 0) 2>&1; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // return outside function should produce an error and non-zero exit
    assert!(stdout.trim().lines().last().unwrap() != "0");
}
```

## Files Changed

| File | Change |
|---|---|
| `src/builtin/mod.rs` | Add `umask` to classify + exec_regular_builtin |
| `src/builtin/special.rs` | `builtin_return` scope check, `builtin_source` dot script flag |
| `src/exec/mod.rs` | `exec` no-args redirect persistence |
| `src/env/mod.rs` | Add `in_dot_script: bool` to `ShellEnv` |
| `src/env/vars.rs` | Add `scope_depth()` method |
| `tests/subshell.rs` | Remove 2 `#[ignore]`, add return-outside-function test |
| `TODO.md` | Delete resolved Phase 8 items |

## Error Messages

- `kish: umask: <value>: invalid octal number`
- `kish: umask: <value>: invalid symbolic mode`
- `kish: return: can only return from a function or sourced script`
