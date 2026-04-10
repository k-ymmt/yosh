# Phase 3, 5, 6 Known Limitations Design

## Overview

Address remaining known limitations from Phase 3 (expansion), Phase 5 (arithmetic/functions), and Phase 6 (alias). Phase 6 alias edge cases were verified to already match dash/bash behavior and require no changes. Phase 6 `-m`/`-b`/`ignoreeof` items remain as known limitations.

## Scope

4 implementation items + 1 TODO cleanup:

1. Unquoted `$@` field splitting (Phase 3)
2. Deeply nested command substitution test coverage (Phase 3)
3. Positional parameters in arithmetic expansion (Phase 5)
4. Function-scoped prefix assignments `VAR=val func` (Phase 5)
5. Remove alias expansion edge case line from TODO.md (already done)

## Item 1: Unquoted `$@` Field Splitting

### Problem

`src/expand/mod.rs` line 462: unquoted `$@` falls through to the default `_` branch, which calls `param::expand()`. This joins all positional parameters with a space into a single string. POSIX requires unquoted `$@` to produce separate fields per positional parameter (each subject to further field splitting and pathname expansion).

### Design

Add a dedicated match arm in `expand_param_to_fields()` for unquoted `$@`, before the catch-all `_` arm:

```rust
ParamExpr::Special(SpecialParam::At) if !in_double_quote => {
    let params = env.vars.positional_params().to_vec();
    if params.is_empty() {
        return;
    }
    for (i, p) in params.iter().enumerate() {
        if i == 0 {
            fields.last_mut().unwrap().push_unquoted(p);
        } else {
            fields.push(ExpandedField::new());
            fields.last_mut().unwrap().push_unquoted(p);
        }
    }
}
```

Key difference from `"$@"`: uses `push_unquoted()` so each field remains subject to IFS field splitting and glob expansion.

### Tests

- E2E: `set -- "a b" c d; for x in $@; do echo "$x"; done` -> `a`, `b`, `c`, `d` (4 lines, IFS splits "a b")
- E2E: `set -- x y z; echo $@` -> `x y z` (simple expansion)
- E2E: empty positional params produce no fields
- Unit: verify field count matches positional param count

## Item 2: Deeply Nested Command Substitution Tests

### Problem

No tests exist for deeply nested command substitutions. The implementation may work correctly but edge cases are unverified.

### Design

Add `e2e/command_substitution/nested_deep.sh` with these test cases:

1. 3-level nesting: `echo $(echo $(echo $(echo deep)))` -> `deep`
2. Nested with mixed quoting: `echo "$(echo "$(echo 'inner')")"` -> `inner`
3. Nested with arithmetic: `echo $(( $(echo 1) + $(echo 2) ))` -> `3`
4. Nested with variable expansion: `x=hello; echo $(echo $(echo $x))` -> `hello`
5. Nested with pipeline: `echo $(echo foo | cat | cat)` -> `foo`

If any test fails, fix the underlying issue in `src/expand/command_sub.rs` or related modules.

## Item 3: Positional Parameters in Arithmetic Expansion

### Problem

`src/expand/arith.rs` `expand_vars()` (lines 28-71) only handles `$VAR` (alphabetic start) and `${VAR}` (braced). It does not recognize:
- `$N` where N is a digit (positional parameters)
- `$#`, `$?`, `$-`, `$!`, `$$` (special parameters)
- `${N}` where N is numeric (braced positional parameters, including multi-digit like `${10}`)

### Design

Extend `expand_vars()` with two new branches after the existing `${ }` and `$VAR` checks:

**Branch 1 - `$N` (single digit positional parameter):**
```rust
} else if bytes[i + 1].is_ascii_digit() {
    i += 1;
    let n = (bytes[i] - b'0') as usize;
    let val = if n == 0 {
        env.shell_name.clone()
    } else {
        env.vars.positional_params().get(n - 1).cloned().unwrap_or_default()
    };
    result.push_str(if val.is_empty() { "0" } else { &val });
    i += 1;
}
```

**Branch 2 - Special parameters (`$#`, `$?`, `$-`, `$!`, `$$`):**
```rust
} else if b"#?-!$".contains(&bytes[i + 1]) {
    i += 1;
    let val = match bytes[i] {
        b'#' => env.vars.positional_params().len().to_string(),
        b'?' => env.last_exit_status.to_string(),
        b'-' => env.options.to_flag_string(),
        b'!' => env.last_bg_pid.map(|p| p.to_string()).unwrap_or("0".to_string()),
        b'$' => env.shell_pid.as_raw().to_string(),
        _ => unreachable!(),
    };
    result.push_str(if val.is_empty() { "0" } else { &val });
    i += 1;
}
```

**Braced form `${N}` fix:** The existing `${VAR}` branch calls `env.vars.get(name)` which doesn't resolve numeric names to positional params. Add a helper function `arith_var_lookup()` that checks if the name is all-digit (positional param), a single special character, or a regular variable name, and dispatches accordingly. Use this helper in both the braced and unbraced branches.

### Tests

- E2E: `set -- 10 20; echo $(($1 + $2))` -> `30`
- E2E: `set -- 5; echo $(($1 * $1))` -> `25`
- E2E: `set -- a b c; echo $(($# + 1))` -> `4`
- E2E: `set -- 100; echo $((${1} + 1))` -> `101`
- Unit: verify expand_vars produces correct substitutions for `$1`, `$#`, `$?`

## Item 4: Function-Scoped Prefix Assignments

### Problem

`src/exec/mod.rs` lines 442-454: when a function is called with prefix assignments (`VAR=val func`), the assignments in `cmd.assignments` are completely ignored. POSIX requires prefix assignments to be visible within the function but restored after the function returns.

### Design

Apply the same save/restore pattern used for regular builtins (lines 499-514). Modify the function call branch:

```rust
if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
    let saved = self.apply_temp_assignments(&cmd.assignments);  // NEW
    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
        eprintln!("kish: {}", e);
        self.restore_assignments(saved);  // NEW
        self.env.last_exit_status = 1;
        return 1;
    }
    let status = self.exec_function_call(&func_def, &args);
    redirect_state.restore();
    self.restore_assignments(saved);  // NEW
    self.env.last_exit_status = status;
    return status;
}
```

The existing `apply_temp_assignments()` / `restore_assignments()` mechanism handles save and restore correctly, including the case where the variable didn't exist before (restore removes it).

Additionally, remove the `XFAIL` marker from `e2e/function/function_prefix_assignment.sh` since this test should now pass.

### Tests

- Existing XFAIL test: `e2e/function/function_prefix_assignment.sh` (remove XFAIL)
- E2E: multiple prefix assignments: `A=1 B=2 func; echo "$A $B"` -> values restored
- E2E: prefix assignment with unset variable: variable should not persist after function

## Error Handling

- Arithmetic expansion with unset positional params defaults to `0` (consistent with existing behavior for unset variables)
- `apply_temp_assignments` failure on readonly variables returns exit status 1 (existing behavior)

## Testing Strategy

- Unit tests in respective modules for core logic
- E2E tests for POSIX compliance verification against expected output
- Existing test suite must continue to pass with no regressions
