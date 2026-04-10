# Phase 2: Known Limitations — Design Spec

## Overview

Address all four Phase 2 known limitations in kish:

1. `echo -n` flag handling
2. `cd -` (OLDPWD navigation)
3. `VarStore` scope mechanism for function execution
4. `TempDir` ID collision prevention

## 1. `echo -n` Flag Handling

**File:** `src/builtin/mod.rs`

Currently `builtin_echo` outputs all arguments joined by space with a trailing newline. POSIX does not mandate `-n`, but all practical shells (bash, dash, zsh) support it.

**Design:**

- If the first argument is `-n`, suppress the trailing newline and output the remaining arguments joined by space using `print!`.
- Otherwise, use `println!` as before.
- No other flags are handled (POSIX `echo` explicitly avoids option processing beyond XSI `-n`).

## 2. `cd -` (OLDPWD Navigation)

**File:** `src/builtin/mod.rs`

The current `builtin_cd` already saves `OLDPWD` before changing directories but does not handle the `-` argument.

**Design:**

- When the argument is `-`, retrieve `$OLDPWD` and use it as the target directory.
- Per POSIX, print the resolved directory path to stdout when `cd -` is used.
- If `OLDPWD` is not set, print an error and return exit code 1.
- The rest of the cd logic (saving OLDPWD, updating PWD) applies as normal after resolving the target.

## 3. `VarStore` Scope Chain

**File:** `src/env/vars.rs`

The current `VarStore` is a flat `HashMap<String, Variable>`. This needs a scope mechanism for Phase 5 function execution, where positional parameters must be local to each function invocation while regular variables remain shared with the caller (POSIX semantics).

### Data Structure

```rust
struct Scope {
    vars: HashMap<String, Variable>,
    positional_params: Vec<String>,
}

pub struct VarStore {
    scopes: Vec<Scope>,  // scopes[0] = global, scopes.last() = current
}
```

### Operations

| Operation | Behavior |
|---|---|
| `get(name)` | Walk scopes from top to bottom, return first match |
| `set(name, val)` | Find the scope containing the variable and update in-place. If not found, create in global scope (POSIX: function assignments affect caller) |
| `push_scope(params)` | Push a new `Scope` with the given positional parameters and an empty vars map |
| `pop_scope()` | Pop the top scope, restoring the previous scope's positional parameters |
| `positional_params()` | Return the current (topmost) scope's positional parameters |
| `export(name)` | Walk scopes to find the variable, mark as exported |
| `readonly(name)` | Walk scopes to find the variable, mark as readonly |
| `unset(name)` | Walk scopes to find and remove the variable |
| `to_environ()` | Flatten all scopes (bottom-up, later scopes shadow earlier), collect exported variables |
| `clone()` | For subshells: flatten all scopes into a single-scope VarStore |

### Design Rationale

- **POSIX compliance:** Functions share the caller's variable namespace. Only positional parameters are scoped per function invocation. This is achieved by having `set` write to the scope where the variable already exists (or global if new).
- **Extensibility:** Adding `local` (bash extension) in the future requires only a `set_local` variant that inserts into the current scope instead of searching the chain.
- **Performance:** Shell function nesting is typically shallow (< 10 levels), so O(depth) chain traversal is negligible.

### Backward Compatibility

- `VarStore::new()` and `from_environ()` initialize with a single global scope.
- All existing `get`/`set`/`unset`/`export`/`readonly`/`to_environ` calls continue to work identically with a single-scope VarStore.
- Positional parameters are currently managed separately in `ShellEnv`; this design moves them into `VarStore` scopes. Migration requires updating `ShellEnv` to delegate positional parameter access to `VarStore`.

## 4. `TempDir` ID Collision Prevention

**File:** `tests/helpers/mod.rs`

The current implementation uses only a nanosecond timestamp, which can collide when multiple tests create `TempDir` instances in rapid succession (Rust's `cargo test` runs tests in parallel threads within a single process).

**Design:**

- Add a process-global `AtomicU64` counter.
- Generate IDs as `kish-test-{nanosecond_timestamp}-{sequential_counter}`.
- The atomic counter guarantees uniqueness within the same process, while the timestamp differentiates across process invocations.
- No external dependencies needed (`std::sync::atomic`).

## Testing Strategy

- **`echo -n`:** Unit test verifying output without trailing newline; E2E test with `echo -n hello`.
- **`cd -`:** Unit test verifying directory change and stdout output; E2E test for `cd /tmp && cd - && pwd`.
- **`VarStore` scope:** Unit tests for push/pop scope, variable resolution across scopes, positional parameter isolation, and readonly/export behavior across scopes.
- **`TempDir`:** Existing tests exercise `TempDir::new()` in parallel; the fix is preventive — verify no test failures under `cargo test`.
