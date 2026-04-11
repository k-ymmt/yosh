# Phase 8 Known Limitations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve four Phase 8 known limitations: implement `umask` builtin with symbolic mode, fix `exec` redirect persistence, add `return`-outside-function error, and enable previously ignored tests.

**Architecture:** Each item is independent. `umask` is added as a regular builtin in `src/builtin/mod.rs`. `exec` redirect persistence is a small branch in `src/exec/mod.rs`. `return` validation requires a `scope_depth()` accessor on `VarStore` and an `in_dot_script` flag on `ShellEnv`. All changes follow the existing builtin/executor patterns.

**Tech Stack:** Rust, libc (for `umask` syscall), nix crate, cargo test

---

### Task 1: `umask` Builtin — Octal Mode

**Files:**
- Modify: `src/builtin/mod.rs:19-56` (classify_builtin + exec_regular_builtin + new function)
- Test: `tests/subshell.rs:304-310` (enable test_umask_inheritance)

- [ ] **Step 1: Write E2E test for umask display**

Add to the end of `tests/subshell.rs`, before the closing brace (after `test_background_command_trap_reset`):

```rust
#[test]
fn test_umask_octal_display() {
    let out = kish_exec("umask 027; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0027");
}

#[test]
fn test_umask_set_octal() {
    let out = kish_exec("umask 077; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0077");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test subshell test_umask_octal_display test_umask_set_octal -- --nocapture 2>&1 | tail -5`
Expected: FAIL — `umask` is not a recognized builtin.

- [ ] **Step 3: Add `umask` to builtin classification**

In `src/builtin/mod.rs`, add `"umask"` to the `Regular` arm of `classify_builtin` (line 28):

```rust
        "cd" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" => BuiltinKind::Regular,
```

- [ ] **Step 4: Implement `builtin_umask` (octal mode) and wire it up**

In `src/builtin/mod.rs`, add `"umask"` to `exec_regular_builtin` (after the `"echo"` arm):

```rust
        "umask" => builtin_umask(args),
```

Then add the function before the `#[cfg(test)]` section at the bottom of `src/builtin/mod.rs`:

```rust
fn builtin_umask(args: &[String]) -> i32 {
    if args.is_empty() {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        println!("{:04o}", current);
        return 0;
    }

    if args[0] == "-S" {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        println!("{}", umask_to_symbolic(current));
        return 0;
    }

    // Try octal parse first
    if args[0].chars().all(|c| c.is_ascii_digit()) {
        return umask_set_octal(&args[0]);
    }

    // Try symbolic parse
    umask_set_symbolic(&args[0])
}

fn umask_to_symbolic(mask: libc::mode_t) -> String {
    let perms = 0o777 & !mask;
    let fmt = |bits: libc::mode_t| -> String {
        let mut s = String::new();
        if bits & 4 != 0 { s.push('r'); }
        if bits & 2 != 0 { s.push('w'); }
        if bits & 1 != 0 { s.push('x'); }
        s
    };
    format!(
        "u={},g={},o={}",
        fmt((perms >> 6) & 7),
        fmt((perms >> 3) & 7),
        fmt(perms & 7),
    )
}

fn umask_set_octal(s: &str) -> i32 {
    for c in s.chars() {
        if !('0'..='7').contains(&c) {
            eprintln!("kish: umask: {}: invalid octal number", s);
            return 1;
        }
    }
    match libc::mode_t::from_str_radix(s, 8) {
        Ok(mode) => {
            unsafe { libc::umask(mode) };
            0
        }
        Err(_) => {
            eprintln!("kish: umask: {}: invalid octal number", s);
            1
        }
    }
}

fn umask_set_symbolic(s: &str) -> i32 {
    let current = unsafe { libc::umask(0) };
    unsafe { libc::umask(current) };

    let mut mask = current;

    for clause in s.split(',') {
        let bytes = clause.as_bytes();
        if bytes.is_empty() {
            eprintln!("kish: umask: {}: invalid symbolic mode", s);
            return 1;
        }

        let mut i = 0;
        let mut who_mask: libc::mode_t = 0;

        // Parse who (u/g/o/a)
        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            match bytes[i] {
                b'u' => who_mask |= 0o700,
                b'g' => who_mask |= 0o070,
                b'o' => who_mask |= 0o007,
                b'a' => who_mask |= 0o777,
                _ => unreachable!(),
            }
            i += 1;
        }

        // Default to 'a' if no who specified
        if who_mask == 0 {
            who_mask = 0o777;
        }

        // Parse operator (=, +, -)
        if i >= bytes.len() || !matches!(bytes[i], b'=' | b'+' | b'-') {
            eprintln!("kish: umask: {}: invalid symbolic mode", s);
            return 1;
        }
        let op = bytes[i] as char;
        i += 1;

        // Parse permissions (r/w/x)
        let mut perm_bits: libc::mode_t = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'r' => perm_bits |= 0o444,
                b'w' => perm_bits |= 0o222,
                b'x' => perm_bits |= 0o111,
                _ => {
                    eprintln!("kish: umask: {}: invalid symbolic mode", s);
                    return 1;
                }
            }
            i += 1;
        }

        // Apply within the who mask
        let effective_perms = perm_bits & who_mask;

        match op {
            '=' => {
                // Clear who bits, then set umask to deny everything NOT in perm
                mask = (mask & !who_mask) | (who_mask & !effective_perms);
            }
            '+' => {
                // Adding permissions = clearing umask bits
                mask &= !effective_perms;
            }
            '-' => {
                // Removing permissions = setting umask bits
                mask |= effective_perms;
            }
            _ => unreachable!(),
        }
    }

    unsafe { libc::umask(mask) };
    0
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test subshell test_umask_octal_display test_umask_set_octal -- --nocapture 2>&1 | tail -5`
Expected: 2 tests PASS.

- [ ] **Step 6: Enable `test_umask_inheritance`**

In `tests/subshell.rs`, remove the `#[ignore]` attribute from `test_umask_inheritance` (line 305):

Remove this line:
```rust
#[ignore = "umask builtin not yet implemented in kish"]
```

- [ ] **Step 7: Run full umask tests**

Run: `cargo test --test subshell test_umask -- --nocapture 2>&1 | tail -10`
Expected: `test_umask_inheritance`, `test_umask_isolation`, `test_umask_octal_display`, `test_umask_set_octal` all PASS.

- [ ] **Step 8: Commit**

```bash
git add src/builtin/mod.rs tests/subshell.rs
git commit -m "feat(umask): implement umask builtin with octal mode"
```

---

### Task 2: `umask` Builtin — Symbolic Mode

**Files:**
- Modify: `src/builtin/mod.rs` (already has `umask_to_symbolic` and `umask_set_symbolic` from Task 1)
- Test: `tests/subshell.rs`

- [ ] **Step 1: Write E2E tests for symbolic mode**

Add to the end of `tests/subshell.rs`:

```rust
#[test]
fn test_umask_symbolic_display() {
    let out = kish_exec("umask 027; umask -S");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "u=rwx,g=rx,o=");
}

#[test]
fn test_umask_set_symbolic() {
    let out = kish_exec("umask u=rwx,g=rx,o=rx; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0022");
}

#[test]
fn test_umask_symbolic_add_remove() {
    // Start with 077, add group read => 037
    let out = kish_exec("umask 077; umask g+r; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0037");
}

#[test]
fn test_umask_symbolic_minus() {
    // Start with 022, remove user write permission => 0222 ... no.
    // umask 022 means deny g=w,o=w. Remove user read => add u-r to umask => 0422
    let out = kish_exec("umask 022; umask u-r; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0422");
}

#[test]
fn test_umask_invalid_octal() {
    let out = kish_exec("umask 089; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "1");
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test subshell test_umask_symbolic test_umask_invalid -- --nocapture 2>&1 | tail -15`
Expected: All 5 tests PASS (the implementation was already added in Task 1).

- [ ] **Step 3: Commit**

```bash
git add tests/subshell.rs
git commit -m "test(umask): add symbolic mode and error handling tests"
```

---

### Task 3: `exec` Redirect Persistence

**Files:**
- Modify: `src/exec/mod.rs:498-522` (special builtin branch)
- Test: `tests/subshell.rs:319-328` (enable test_fd_inheritance)

- [ ] **Step 1: Write E2E test for exec redirect persistence**

Add to the end of `tests/subshell.rs`:

```rust
#[test]
fn test_exec_redirect_persistence() {
    // exec 3>file should persist fd 3 for subsequent commands
    let out = kish_exec("exec 3>/tmp/kish-exec-persist-$$; echo hello >&3; exec 3>&-; cat /tmp/kish-exec-persist-$$; rm -f /tmp/kish-exec-persist-$$");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test subshell test_exec_redirect_persistence -- --nocapture 2>&1 | tail -5`
Expected: FAIL — redirect is restored, so fd 3 is not available after `exec`.

- [ ] **Step 3: Implement exec redirect persistence**

In `src/exec/mod.rs`, inside the `BuiltinKind::Special` arm (after the assignment loop ending at line 512, before `let mut redirect_state`), add the exec-no-args early return:

```rust
                // exec with no args: redirects persist (don't save/restore)
                if command_name == "exec" && args.is_empty() {
                    let mut redirect_state = RedirectState::new();
                    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, false) {
                        eprintln!("kish: {}", e);
                        self.env.last_exit_status = 1;
                        return 1;
                    }
                    self.env.last_exit_status = 0;
                    return 0;
                }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test subshell test_exec_redirect_persistence -- --nocapture 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Enable `test_fd_inheritance`**

In `tests/subshell.rs`, remove the `#[ignore]` attribute from `test_fd_inheritance` (line 320):

Remove this line:
```rust
#[ignore = "exec N>file fd persistence not yet implemented in kish"]
```

- [ ] **Step 6: Run both fd tests**

Run: `cargo test --test subshell test_fd_inheritance test_exec_redirect -- --nocapture 2>&1 | tail -5`
Expected: Both PASS.

- [ ] **Step 7: Commit**

```bash
git add src/exec/mod.rs tests/subshell.rs
git commit -m "fix(exec): persist redirects when exec has no command arguments"
```

---

### Task 4: `return` Outside Function Error

**Files:**
- Modify: `src/env/vars.rs:91` (add `scope_depth` method after `pop_scope`)
- Modify: `src/env/mod.rs:281-324` (add `in_dot_script` field to `ShellEnv`)
- Modify: `src/builtin/special.rs:129-143` (update `builtin_return`)
- Modify: `src/builtin/special.rs:328-362` (update `builtin_source`)
- Test: `tests/subshell.rs`

- [ ] **Step 1: Write E2E test for return outside function**

Add to the end of `tests/subshell.rs`:

```rust
#[test]
fn test_return_outside_function_error() {
    let out = kish_exec("return 0 2>/dev/null; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "1");
}

#[test]
fn test_return_outside_function_in_subshell() {
    let out = kish_exec("(return 0 2>/dev/null; echo $?)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "1");
}

#[test]
fn test_return_in_function_still_works() {
    let out = kish_exec("f() { return 42; }; f; echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn test_return_in_dot_script() {
    let out = kish_exec("echo 'return 0; echo unreachable' > /tmp/kish-return-test-$$.sh; . /tmp/kish-return-test-$$.sh; echo $?; rm -f /tmp/kish-return-test-$$.sh");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "0");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test subshell test_return_outside_function_error test_return_outside_function_in_subshell -- --nocapture 2>&1 | tail -5`
Expected: FAIL — `return` currently sets FlowControl unconditionally.

- [ ] **Step 3: Add `scope_depth()` to `VarStore`**

In `src/env/vars.rs`, after the `pop_scope` method (after line 91), add:

```rust
    /// Return the current scope depth. 1 = global scope only.
    pub fn scope_depth(&self) -> usize {
        self.scopes.len()
    }
```

- [ ] **Step 4: Add `in_dot_script` field to `ShellEnv`**

In `src/env/mod.rs`, add the field to the `ShellEnv` struct (after `is_interactive: bool` at line 299):

```rust
    /// True when executing inside a dot script (`. file` / `source file`).
    pub in_dot_script: bool,
```

And initialize it in `ShellEnv::new()` (after `is_interactive: false` at line 322):

```rust
            in_dot_script: false,
```

- [ ] **Step 5: Update `builtin_return` to check scope**

In `src/builtin/special.rs`, replace the `builtin_return` function:

```rust
fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
    if env.vars.scope_depth() <= 1 && !env.in_dot_script {
        eprintln!("kish: return: can only return from a function or sourced script");
        return 1;
    }
    let code = if args.is_empty() {
        env.last_exit_status & 0xFF
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: return: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    env.flow_control = Some(FlowControl::Return(code));
    code
}
```

- [ ] **Step 6: Update `builtin_source` to set `in_dot_script` flag**

In `src/builtin/special.rs`, replace the `builtin_source` function:

```rust
fn builtin_source(args: &[String], executor: &mut Executor) -> i32 {
    if args.is_empty() {
        eprintln!("kish: .: filename argument required");
        return 2;
    }
    let filename = &args[0];
    let path = if filename.contains('/') {
        std::path::PathBuf::from(filename)
    } else {
        if let Some(path_var) = executor.env.vars.get("PATH") {
            let mut found = None;
            for dir in path_var.split(':') {
                let candidate = std::path::PathBuf::from(dir).join(filename);
                if candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => { eprintln!("kish: .: {}: not found", filename); return 1; }
            }
        } else {
            std::path::PathBuf::from(filename)
        }
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("kish: .: {}: {}", path.display(), e); return 1; }
    };
    let prev_dot_script = executor.env.in_dot_script;
    executor.env.in_dot_script = true;
    let status = match crate::parser::Parser::new_with_aliases(&content, &executor.env.aliases).parse_program() {
        Ok(program) => executor.exec_program(&program),
        Err(e) => { eprintln!("kish: .: {}", e); 2 }
    };
    executor.env.in_dot_script = prev_dot_script;
    status
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --test subshell test_return_outside_function test_return_in_function test_return_in_dot_script -- --nocapture 2>&1 | tail -10`
Expected: All 4 tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src/env/vars.rs src/env/mod.rs src/builtin/special.rs tests/subshell.rs
git commit -m "fix(return): error when return used outside function or dot script"
```

---

### Task 5: Update TODO.md and Final Verification

**Files:**
- Modify: `TODO.md:1-9`

- [ ] **Step 1: Remove resolved Phase 8 items from TODO.md**

Delete the entire `## Phase 8: Known Limitations` section from `TODO.md` (lines 3-8), including the header and all 4 items. The file should begin with the next section (`## Job Control: Known Limitations`).

The resulting top of `TODO.md` should be:

```markdown
# TODO

## Job Control: Known Limitations
```

- [ ] **Step 2: Run the full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass, no ignored tests related to Phase 8.

- [ ] **Step 3: Verify no Phase 8 ignored tests remain**

Run: `cargo test --test subshell -- --ignored 2>&1 | tail -5`
Expected: `0 passed; 0 failed` (no remaining ignored tests in subshell.rs).

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "docs: remove resolved Phase 8 known limitations from TODO.md"
```
