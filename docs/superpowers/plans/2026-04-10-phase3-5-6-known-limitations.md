# Phase 3, 5, 6 Known Limitations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 3 POSIX compliance bugs (unquoted `$@` splitting, positional params in arithmetic, function prefix assignments) and add nested command substitution test coverage.

**Architecture:** Each fix is isolated to a single file with no cross-dependencies. Unquoted `$@` is a new match arm in `expand_param_to_fields()`. Arithmetic `$N` support extends `expand_vars()` with digit/special-char branches and a shared lookup helper. Function prefix assignments reuse the existing `apply_temp_assignments()`/`restore_assignments()` pattern.

**Tech Stack:** Rust, cargo test, e2e shell test framework (`e2e/run_tests.sh`)

---

### Task 1: Unquoted `$@` Field Splitting

**Files:**
- Modify: `src/expand/mod.rs:432-470` (expand_param_to_fields match arms)
- Modify: `e2e/variable_and_expansion/at_vs_star_unquoted.sh` (remove XFAIL)
- Test: `src/expand/mod.rs` (unit tests at bottom of file)

- [ ] **Step 1: Write the failing unit test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/expand/mod.rs`, after the existing `test_dollar_at_empty_params_produces_nothing` test (around line 542):

```rust
    #[test]
    fn test_unquoted_dollar_at_splits_per_param() {
        let mut env = ShellEnv::new(
            "kish",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // Unquoted $@ — each positional param becomes its own field
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word);
        assert_eq!(fields.len(), 3, "expected 3 fields, got {:?}", fields);
        assert_eq!(fields[0].value, "a");
        assert_eq!(fields[1].value, "b");
        assert_eq!(fields[2].value, "c");
        // All bytes should be unquoted (subject to IFS splitting)
        assert!(fields[0].quoted_mask.iter().all(|&q| !q));
        assert!(fields[1].quoted_mask.iter().all(|&q| !q));
        assert!(fields[2].quoted_mask.iter().all(|&q| !q));
    }

    #[test]
    fn test_unquoted_dollar_at_empty_produces_nothing() {
        let mut env = ShellEnv::new("kish", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word);
        // With no positional params, unquoted $@ should produce one empty field
        // (the initial field from expand_word_to_fields), which gets filtered by expand_word
        assert!(
            fields.len() <= 1,
            "expected 0 or 1 fields, got {:?}",
            fields
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_unquoted_dollar_at_splits_per_param -- --nocapture 2>&1`
Expected: FAIL — currently produces 1 field "a b c" instead of 3 fields

- [ ] **Step 3: Implement unquoted `$@` field splitting**

In `src/expand/mod.rs`, in the `expand_param_to_fields()` function, add a new match arm **between** the `"$*" in double quotes` arm (line 458) and the catch-all `_` arm (line 462):

Replace this code:

```rust
        // Everything else: expand to a string, then push.
        _ => {
```

With:

```rust
        // Unquoted $@: each positional parameter becomes its own field,
        // with content unquoted (subject to IFS splitting and glob).
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

        // Everything else: expand to a string, then push.
        _ => {
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_unquoted_dollar_at -- --nocapture 2>&1`
Expected: both `test_unquoted_dollar_at_splits_per_param` and `test_unquoted_dollar_at_empty_produces_nothing` PASS

- [ ] **Step 5: Remove XFAIL from e2e test**

In `e2e/variable_and_expansion/at_vs_star_unquoted.sh`, remove the XFAIL line:

```
# XFAIL: unquoted $@ joins parameters with space instead of treating each independently
```

- [ ] **Step 6: Run e2e test to verify**

Run: `./e2e/run_tests.sh --filter=at_vs_star_unquoted 2>&1`
Expected: PASS (no longer XFAIL)

- [ ] **Step 7: Run full test suite for regressions**

Run: `cargo test 2>&1` and `./e2e/run_tests.sh 2>&1 | tail -20`
Expected: no new failures

- [ ] **Step 8: Commit**

```bash
git add src/expand/mod.rs e2e/variable_and_expansion/at_vs_star_unquoted.sh
git commit -m "fix(expand): unquoted \$@ produces separate fields per positional param

POSIX requires unquoted \$@ to split into one field per positional
parameter, each subject to IFS field splitting. Previously joined
all params with space into a single string.

Addresses Phase 3 known limitation."
```

---

### Task 2: Deeply Nested Command Substitution Tests

**Files:**
- Modify: `e2e/command_substitution/nested_deep.sh` (add more test cases)
- Create: `e2e/command_substitution/nested_cmdsub_in_arith.sh`
- Create: `e2e/command_substitution/nested_cmdsub_pipeline.sh`
- Create: `e2e/command_substitution/nested_cmdsub_variable.sh`

Note: `nested_deep.sh` already exists with a 3-level test. `nested_with_quotes.sh` and `nested_with_arith.sh` already exist. We add the missing cases.

- [ ] **Step 1: Create nested command substitution with arithmetic test**

Create `e2e/command_substitution/nested_cmdsub_in_arith.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution nested inside arithmetic expansion
# EXPECT_OUTPUT: 3
echo $(( $(echo 1) + $(echo 2) ))
```

Set permissions: `chmod 644 e2e/command_substitution/nested_cmdsub_in_arith.sh`

- [ ] **Step 2: Create nested command substitution with variable test**

Create `e2e/command_substitution/nested_cmdsub_variable.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution with variable expansion
# EXPECT_OUTPUT: hello
x=hello
echo $(echo $(echo $x))
```

Set permissions: `chmod 644 e2e/command_substitution/nested_cmdsub_variable.sh`

- [ ] **Step 3: Create nested command substitution with pipeline test**

Create `e2e/command_substitution/nested_cmdsub_pipeline.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution with nested pipeline
# EXPECT_OUTPUT: foo
echo $(echo foo | cat | cat)
```

Set permissions: `chmod 644 e2e/command_substitution/nested_cmdsub_pipeline.sh`

- [ ] **Step 4: Run the new tests**

Run: `./e2e/run_tests.sh --filter=nested_cmdsub 2>&1`
Expected: all 3 new tests PASS

- [ ] **Step 5: If any test fails, investigate and fix**

If a test fails, read the relevant source in `src/expand/command_sub.rs` and fix the issue. Then rerun:
`./e2e/run_tests.sh --filter=nested_cmdsub 2>&1`

- [ ] **Step 6: Commit**

```bash
git add e2e/command_substitution/nested_cmdsub_in_arith.sh \
        e2e/command_substitution/nested_cmdsub_variable.sh \
        e2e/command_substitution/nested_cmdsub_pipeline.sh
git commit -m "test(e2e): add deeply nested command substitution edge case tests

Add tests for command substitution nested inside arithmetic, with
variable expansion, and with pipelines. Verifies Phase 3 nested
command substitution coverage."
```

---

### Task 3: Positional Parameters in Arithmetic Expansion

**Files:**
- Modify: `src/expand/arith.rs:28-71` (expand_vars function)
- Modify: `e2e/arithmetic/positional_in_arith.sh` (remove XFAIL)
- Test: `src/expand/arith.rs` (unit tests at bottom of file)

- [ ] **Step 1: Write failing unit tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/expand/arith.rs`, after the existing `test_variable_assign` test (around line 668):

```rust
    #[test]
    fn test_positional_param_in_arith() {
        let mut e = ShellEnv::new(
            "kish",
            vec!["10".to_string(), "20".to_string()],
        );
        assert_eq!(evaluate(&mut e, "$1 + $2"), Ok("30".to_string()));
    }

    #[test]
    fn test_positional_param_zero() {
        let mut e = ShellEnv::new(
            "kish",
            vec!["5".to_string()],
        );
        // $0 is the shell name, not numeric — defaults to "0" in arithmetic
        assert_eq!(evaluate(&mut e, "$0"), Ok("0".to_string()));
    }

    #[test]
    fn test_special_param_hash_in_arith() {
        let mut e = ShellEnv::new(
            "kish",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(evaluate(&mut e, "$# + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_special_param_question_in_arith() {
        let mut e = env();
        e.last_exit_status = 42;
        assert_eq!(evaluate(&mut e, "$?"), Ok("42".to_string()));
    }

    #[test]
    fn test_braced_positional_param_in_arith() {
        let mut e = ShellEnv::new(
            "kish",
            vec!["100".to_string()],
        );
        assert_eq!(evaluate(&mut e, "${1} + 1"), Ok("101".to_string()));
    }

    #[test]
    fn test_unset_positional_param_defaults_to_zero() {
        let mut e = env();
        // No positional params set; $1 should default to 0
        assert_eq!(evaluate(&mut e, "$1 + 5"), Ok("5".to_string()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin kish test_positional_param_in_arith -- --nocapture 2>&1`
Expected: FAIL — `$1` is not recognized, left as literal `$1`

- [ ] **Step 3: Add `arith_var_lookup` helper function**

In `src/expand/arith.rs`, add this helper function just before the `expand_vars` function (before line 28):

```rust
/// Look up a variable name in the arithmetic context.
/// Handles positional parameters (all-digit names), special parameters
/// (single char: #, ?, -, !, $), and regular variable names.
/// Returns "0" for unset values (arithmetic context default).
fn arith_var_lookup(env: &ShellEnv, name: &str) -> String {
    // All-digit name → positional parameter (or $0 for shell name)
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
        let n: usize = name.parse().unwrap_or(0);
        let val = if n == 0 {
            env.shell_name.clone()
        } else {
            env.vars.positional_params().get(n - 1).cloned().unwrap_or_default()
        };
        return if val.is_empty() || val.parse::<i64>().is_err() {
            "0".to_string()
        } else {
            val
        };
    }

    // Single-char special parameters
    if name.len() == 1 {
        let val = match name.as_bytes()[0] {
            b'#' => return env.vars.positional_params().len().to_string(),
            b'?' => return env.last_exit_status.to_string(),
            b'-' => {
                let s = env.options.to_flag_string();
                return if s.is_empty() { "0".to_string() } else { s };
            }
            b'!' => return env.last_bg_pid.map(|p| p.to_string()).unwrap_or_else(|| "0".to_string()),
            b'$' => return env.shell_pid.as_raw().to_string(),
            _ => env.vars.get(name).unwrap_or("0"),
        };
        return val.to_string();
    }

    // Regular variable
    env.vars.get(name).unwrap_or("0").to_string()
}
```

- [ ] **Step 4: Update `expand_vars` to use the helper and handle `$N` and special params**

Replace the entire `expand_vars` function in `src/expand/arith.rs` (lines 28-71) with:

```rust
/// Replace `$VAR`, `${VAR}`, `$N`, and `$#/$?/$-/$!` in an arithmetic expression
/// with their values. Unset variables default to "0".
fn expand_vars(env: &ShellEnv, expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'{' {
                // ${VAR}, ${1}, ${10}, ${#}, etc.
                i += 2;
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let name = &expr[start..i];
                if i < bytes.len() {
                    i += 1; // consume '}'
                }
                result.push_str(&arith_var_lookup(env, name));
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                // $VAR
                i += 1;
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                let name = &expr[start..i];
                let val = env.vars.get(name).unwrap_or("0");
                result.push_str(val);
            } else if bytes[i + 1].is_ascii_digit() {
                // $0, $1, ..., $9
                i += 1;
                let n = (bytes[i] - b'0') as usize;
                let val = if n == 0 {
                    env.shell_name.clone()
                } else {
                    env.vars
                        .positional_params()
                        .get(n - 1)
                        .cloned()
                        .unwrap_or_default()
                };
                // Empty values default to "0" in arithmetic context
                let val = if val.is_empty() { "0".to_string() } else { val };
                result.push_str(&val);
                i += 1;
            } else if b"#?-!$".contains(&bytes[i + 1]) {
                // $#, $?, $-, $!, $$
                i += 1;
                let val = match bytes[i] {
                    b'#' => env.vars.positional_params().len().to_string(),
                    b'?' => env.last_exit_status.to_string(),
                    b'-' => {
                        let s = env.options.to_flag_string();
                        if s.is_empty() { "0".to_string() } else { s }
                    }
                    b'!' => env.last_bg_pid.map(|p| p.to_string()).unwrap_or_else(|| "0".to_string()),
                    b'$' => env.shell_pid.as_raw().to_string(),
                    _ => unreachable!(),
                };
                result.push_str(&val);
                i += 1;
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bin kish positional_param_in_arith -- --nocapture 2>&1`
Run: `cargo test --bin kish special_param -- --nocapture 2>&1`
Run: `cargo test --bin kish braced_positional -- --nocapture 2>&1`
Expected: all PASS

- [ ] **Step 6: Remove XFAIL from e2e test**

In `e2e/arithmetic/positional_in_arith.sh`, remove the XFAIL line:

```
# XFAIL: Phase 5 limitation — $N inside $((...)) not supported
```

- [ ] **Step 7: Run e2e test to verify**

Run: `./e2e/run_tests.sh --filter=positional_in_arith 2>&1`
Expected: PASS

- [ ] **Step 8: Run full test suite for regressions**

Run: `cargo test 2>&1` and `./e2e/run_tests.sh 2>&1 | tail -20`
Expected: no new failures

- [ ] **Step 9: Commit**

```bash
git add src/expand/arith.rs e2e/arithmetic/positional_in_arith.sh
git commit -m "feat(arith): support positional and special params in arithmetic expansion

Add \$N (positional), \$# \$? \$- \$! \$\$ (special), and \${N}
(braced positional) parameter support inside \$((...)) arithmetic
expressions. Unset or non-numeric values default to 0.

Addresses Phase 5 known limitation."
```

---

### Task 4: Function-Scoped Prefix Assignments

**Files:**
- Modify: `src/exec/mod.rs:442-454` (function call branch)
- Modify: `e2e/function/function_prefix_assignment.sh` (remove XFAIL)
- Create: `e2e/function/function_prefix_multi_assign.sh`

- [ ] **Step 1: Run existing XFAIL test to confirm current failure**

Run: `./e2e/run_tests.sh --filter=function_prefix_assignment 2>&1`
Expected: XFAIL (known failure)

- [ ] **Step 2: Implement prefix assignments for function calls**

In `src/exec/mod.rs`, replace the function call branch (lines 442-454):

```rust
        // Check for function call (before builtins, matching POSIX lookup order)
        if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.env.last_exit_status = status;
            return status;
        }
```

With:

```rust
        // Check for function call (before builtins, matching POSIX lookup order)
        if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
            let saved = self.apply_temp_assignments(&cmd.assignments);
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.restore_assignments(saved);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.env.last_exit_status = status;
            return status;
        }
```

- [ ] **Step 3: Remove XFAIL from existing e2e test**

In `e2e/function/function_prefix_assignment.sh`, remove the XFAIL line:

```
# XFAIL: Phase 5 limitation — function-scoped prefix assignments not implemented
```

- [ ] **Step 4: Run existing e2e test to verify it passes**

Run: `./e2e/run_tests.sh --filter=function_prefix_assignment 2>&1`
Expected: PASS

- [ ] **Step 5: Add multi-assignment e2e test**

Create `e2e/function/function_prefix_multi_assign.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Multiple prefix assignments scoped to function call
# EXPECT_OUTPUT<<END
# 1 2
# original_a original_b
# END
A=original_a
B=original_b
show() { echo "$A $B"; }
A=1 B=2 show
echo "$A $B"
```

Set permissions: `chmod 644 e2e/function/function_prefix_multi_assign.sh`

- [ ] **Step 6: Run new e2e test**

Run: `./e2e/run_tests.sh --filter=function_prefix_multi 2>&1`
Expected: PASS

- [ ] **Step 7: Run full test suite for regressions**

Run: `cargo test 2>&1` and `./e2e/run_tests.sh 2>&1 | tail -20`
Expected: no new failures

- [ ] **Step 8: Commit**

```bash
git add src/exec/mod.rs \
        e2e/function/function_prefix_assignment.sh \
        e2e/function/function_prefix_multi_assign.sh
git commit -m "feat(exec): implement function-scoped prefix assignments (VAR=val func)

Prefix assignments on function calls are now temporarily applied
before function execution and restored afterward, matching POSIX
behavior and dash/bash semantics.

Addresses Phase 5 known limitation."
```

---

### Task 5: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove completed items from TODO.md**

Remove these completed lines from `TODO.md`:

From Phase 3 section:
```
- [ ] Unquoted `$@` should produce separate fields per positional param, currently joins with space (`src/expand/mod.rs`)
- [ ] Deeply nested command substitution edge cases untested
```

From Phase 5 section:
```
- [ ] `$N` (positional params) inside `$((...))` arithmetic not supported — use temp variable workaround: `x=$1; echo $((x - 1))` (`src/expand/arith.rs`)
- [ ] Function-scoped assignments with prefix syntax (`VAR=val func`) not implemented — assignments only apply to external commands
```

If a Phase section becomes empty after removal, remove the entire section header too.

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed Phase 3 and 5 known limitations from TODO.md"
```
