# Phase 2 Known Limitations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all four Phase 2 known limitations: `echo -n`, `cd -`, `VarStore` scope chain, and `TempDir` ID collision.

**Architecture:** Four independent fixes. Tasks 1-2 are simple builtin enhancements. Task 3 refactors `VarStore` from a flat `HashMap` to a scope chain with positional parameter support, then migrates `ShellEnv` to delegate positional parameters to `VarStore`. Task 4 adds an atomic counter to `TempDir`.

**Tech Stack:** Rust, `std::sync::atomic`, `std::collections::HashMap`

---

### Task 1: `echo -n` Flag Support

**Files:**
- Modify: `src/builtin/mod.rs:94-97` (builtin_echo function)
- Modify: `src/builtin/mod.rs:236-297` (tests module)
- Create: `e2e/builtin/echo_dash_n.sh`

- [ ] **Step 1: Write the failing unit test**

Add to the `#[cfg(test)] mod tests` block in `src/builtin/mod.rs`:

```rust
#[test]
fn test_echo_dash_n() {
    // -n flag should suppress trailing newline.
    // We can't easily capture stdout in unit tests, so verify
    // the function returns 0 (behavior tested via E2E).
    let args = vec!["-n".to_string(), "hello".to_string()];
    assert_eq!(builtin_echo(&args), 0);
}
```

- [ ] **Step 2: Run test to verify it passes (baseline)**

Run: `cargo test --lib builtin::tests::test_echo_dash_n`
Expected: PASS (the function already returns 0; the real behavior change is in output)

- [ ] **Step 3: Implement `echo -n`**

Replace the `builtin_echo` function in `src/builtin/mod.rs:94-97`:

```rust
fn builtin_echo(args: &[String]) -> i32 {
    if args.first().map(|a| a.as_str()) == Some("-n") {
        print!("{}", args[1..].join(" "));
    } else {
        println!("{}", args.join(" "));
    }
    0
}
```

- [ ] **Step 4: Run all tests to verify no regressions**

Run: `cargo test --lib`
Expected: All tests PASS

- [ ] **Step 5: Create E2E test**

Create `e2e/builtin/echo_dash_n.sh`:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo -n suppresses trailing newline
# EXPECT_OUTPUT: helloworld
echo -n hello
echo world
```

- [ ] **Step 6: Run E2E test**

Run: `cargo build && ./e2e/run_tests.sh --filter=echo_dash_n`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/builtin/mod.rs e2e/builtin/echo_dash_n.sh
git commit -m "feat(builtin): support echo -n flag to suppress trailing newline"
```

---

### Task 2: `cd -` (OLDPWD Navigation)

**Files:**
- Modify: `src/builtin/mod.rs:55-92` (builtin_cd function)
- Modify: `e2e/builtin/cd_dash_oldpwd.sh` (remove XFAIL marker)

- [ ] **Step 1: Write E2E test (remove XFAIL from existing test)**

Edit `e2e/builtin/cd_dash_oldpwd.sh` — remove line 4 (`# XFAIL: Phase 2 limitation — cd - not implemented`).

The test already covers the expected behavior:
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - changes to OLDPWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/dir1" "$TEST_TMPDIR/dir2"
cd "$TEST_TMPDIR/dir1"
cd "$TEST_TMPDIR/dir2"
cd -
pwd_result=$(pwd)
case "$pwd_result" in
  *dir1) exit 0 ;;
  *) exit 1 ;;
esac
```

- [ ] **Step 2: Run E2E test to verify it fails**

Run: `cargo build && ./e2e/run_tests.sh --filter=cd_dash_oldpwd`
Expected: FAIL (cd - not yet implemented)

- [ ] **Step 3: Implement `cd -`**

In `src/builtin/mod.rs`, replace the target resolution block in `builtin_cd` (lines 56-66):

```rust
fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 {
    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                eprintln!("kish: cd: HOME not set");
                return 1;
            }
        }
    } else if args[0] == "-" {
        match env.vars.get("OLDPWD") {
            Some(old) => {
                let old = old.to_string();
                println!("{}", old);
                old
            }
            None => {
                eprintln!("kish: cd: OLDPWD not set");
                return 1;
            }
        }
    } else {
        args[0].clone()
    };

    // Save current directory as OLDPWD before changing
    if let Ok(old_pwd) = std::env::current_dir() {
        let _ = env.vars.set("OLDPWD", old_pwd.to_string_lossy().to_string());
    }

    match std::env::set_current_dir(&target) {
        Ok(_) => {
            // Update $PWD
            match std::env::current_dir() {
                Ok(cwd) => {
                    let cwd_str = cwd.to_string_lossy().into_owned();
                    let _ = env.vars.set("PWD", cwd_str);
                }
                Err(e) => {
                    eprintln!("kish: cd: could not determine new directory: {}", e);
                }
            }
            0
        }
        Err(e) => {
            eprintln!("kish: cd: {}: {}", target, e);
            1
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib && cargo build && ./e2e/run_tests.sh --filter=cd`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/builtin/mod.rs e2e/builtin/cd_dash_oldpwd.sh
git commit -m "feat(builtin): implement cd - to change to OLDPWD"
```

---

### Task 3: `VarStore` Scope Chain

This is the largest task. It refactors `VarStore` to use a scope chain and migrates positional parameters from `ShellEnv` into `VarStore` scopes.

**Files:**
- Modify: `src/env/vars.rs` (VarStore refactoring)
- Modify: `src/env/mod.rs:286-328` (ShellEnv struct and constructor)
- Modify: `src/expand/param.rs:24-30, 142-152` (positional param expansion)
- Modify: `src/expand/mod.rs:435, 457` (quoted $@ and $* expansion)
- Modify: `src/expand/command_sub.rs:45-59` (child env cloning)
- Modify: `src/exec/mod.rs:298, 370-390` (for loop default, function call)
- Modify: `src/builtin/special.rs:206-243, 376-380` (set and shift builtins)

#### Task 3a: Refactor `VarStore` Internal Structure

**Files:**
- Modify: `src/env/vars.rs`

- [ ] **Step 1: Write failing test for scope push/pop**

Add to `src/env/vars.rs` tests module:

```rust
#[test]
fn test_push_pop_scope_positional_params() {
    let mut store = VarStore::new();
    store.set_positional_params(vec!["a".to_string(), "b".to_string()]);
    assert_eq!(store.positional_params(), &["a", "b"]);

    store.push_scope(vec!["x".to_string(), "y".to_string(), "z".to_string()]);
    assert_eq!(store.positional_params(), &["x", "y", "z"]);

    store.pop_scope();
    assert_eq!(store.positional_params(), &["a", "b"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib env::vars::tests::test_push_pop_scope_positional_params`
Expected: FAIL — `push_scope`, `pop_scope`, `positional_params`, `set_positional_params` methods do not exist

- [ ] **Step 3: Refactor `VarStore` to scope chain**

Replace the entire `src/env/vars.rs` file with:

```rust
use std::collections::HashMap;

/// A shell variable with its value and attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub value: String,
    pub exported: bool,
    pub readonly: bool,
}

impl Variable {
    pub fn new(value: impl Into<String>) -> Self {
        Variable {
            value: value.into(),
            exported: false,
            readonly: false,
        }
    }

    pub fn new_exported(value: impl Into<String>) -> Self {
        Variable {
            value: value.into(),
            exported: true,
            readonly: false,
        }
    }
}

/// A single scope in the scope chain.
#[derive(Debug, Clone)]
struct Scope {
    vars: HashMap<String, Variable>,
    positional_params: Vec<String>,
}

/// Storage for shell variables with scope chain support.
///
/// Scopes are stacked: `scopes[0]` is global, `scopes.last()` is current.
/// Variable lookups walk from top to bottom. Writes go to the scope that
/// already contains the variable, or to the global scope if the variable
/// is new (POSIX: function assignments affect the caller).
///
/// Positional parameters (`$1`, `$2`, ...) are per-scope — each function
/// invocation gets its own set.
#[derive(Debug, Clone)]
pub struct VarStore {
    scopes: Vec<Scope>,
}

impl VarStore {
    /// Create an empty VarStore with a single global scope.
    pub fn new() -> Self {
        VarStore {
            scopes: vec![Scope {
                vars: HashMap::new(),
                positional_params: Vec::new(),
            }],
        }
    }

    /// Initialize from the current process environment.
    pub fn from_environ() -> Self {
        let mut vars = HashMap::new();
        for (key, value) in std::env::vars() {
            vars.insert(key, Variable::new_exported(value));
        }
        VarStore {
            scopes: vec![Scope {
                vars,
                positional_params: Vec::new(),
            }],
        }
    }

    // ── Scope management ────────────────────────────────────────────────

    /// Push a new scope with the given positional parameters.
    /// Used for function calls.
    pub fn push_scope(&mut self, positional_params: Vec<String>) {
        self.scopes.push(Scope {
            vars: HashMap::new(),
            positional_params,
        });
    }

    /// Pop the current scope, restoring the previous scope's positional
    /// parameters. Panics if only the global scope remains.
    pub fn pop_scope(&mut self) {
        assert!(self.scopes.len() > 1, "cannot pop the global scope");
        self.scopes.pop();
    }

    // ── Positional parameters ───────────────────────────────────────────

    /// Get the current scope's positional parameters.
    pub fn positional_params(&self) -> &[String] {
        &self.scopes.last().unwrap().positional_params
    }

    /// Set the current scope's positional parameters.
    pub fn set_positional_params(&mut self, params: Vec<String>) {
        self.scopes.last_mut().unwrap().positional_params = params;
    }

    // ── Variable access ─────────────────────────────────────────────────

    /// Get the string value of a variable, if set.
    /// Walks scopes from top to bottom.
    pub fn get(&self, name: &str) -> Option<&str> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return Some(var.value.as_str());
            }
        }
        None
    }

    /// Get the full Variable struct, if set.
    /// Walks scopes from top to bottom.
    #[allow(dead_code)]
    pub fn get_var(&self, name: &str) -> Option<&Variable> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return Some(var);
            }
        }
        None
    }

    /// Set a variable's value. Returns an error if the variable is readonly.
    ///
    /// If the variable already exists in some scope, it is updated in-place
    /// in that scope (POSIX: function assignments affect the caller).
    /// If the variable is new, it is created in the global scope.
    pub fn set(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
        let value = value.into();

        // Search for existing variable in any scope (top to bottom).
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                let exported = existing.exported;
                scope.vars.insert(
                    name.to_string(),
                    Variable {
                        value,
                        exported,
                        readonly: false,
                    },
                );
                return Ok(());
            }
        }

        // Not found — create in global scope.
        self.scopes[0].vars.insert(name.to_string(), Variable::new(value));
        Ok(())
    }

    /// Set a variable's value with allexport support.
    pub fn set_with_options(
        &mut self,
        name: &str,
        value: impl Into<String>,
        allexport: bool,
    ) -> Result<(), String> {
        let value = value.into();

        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                let exported = existing.exported || allexport;
                scope.vars.insert(
                    name.to_string(),
                    Variable {
                        value,
                        exported,
                        readonly: false,
                    },
                );
                return Ok(());
            }
        }

        let mut var = Variable::new(value);
        if allexport {
            var.exported = true;
        }
        self.scopes[0].vars.insert(name.to_string(), var);
        Ok(())
    }

    /// Unset a variable. Returns an error if the variable is readonly.
    /// Removes from whichever scope contains it.
    pub fn unset(&mut self, name: &str) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                scope.vars.remove(name);
                return Ok(());
            }
        }
        Ok(())
    }

    /// Mark a variable as exported. Walks scopes to find it; if not found,
    /// creates in global scope with empty value.
    pub fn export(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.vars.get_mut(name) {
                var.exported = true;
                return;
            }
        }
        self.scopes[0]
            .vars
            .insert(name.to_string(), Variable::new_exported(""));
    }

    /// Mark a variable as readonly. Walks scopes to find it; if not found,
    /// creates in global scope with empty value.
    pub fn set_readonly(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.vars.get_mut(name) {
                var.readonly = true;
                return;
            }
        }
        let mut var = Variable::new("");
        var.readonly = true;
        self.scopes[0].vars.insert(name.to_string(), var);
    }

    /// Return only exported variables as (name, value) pairs.
    /// Later scopes shadow earlier ones.
    pub fn to_environ(&self) -> Vec<(String, String)> {
        let mut merged: HashMap<String, &Variable> = HashMap::new();
        for scope in &self.scopes {
            for (name, var) in &scope.vars {
                merged.insert(name.clone(), var);
            }
        }
        merged
            .into_iter()
            .filter(|(_, v)| v.exported)
            .map(|(k, v)| (k, v.value.clone()))
            .collect()
    }

    /// Iterate over all variables as (name, &Variable) pairs.
    /// Later scopes shadow earlier ones.
    pub fn vars_iter(&self) -> impl Iterator<Item = (&String, &Variable)> {
        let mut seen: HashMap<&String, &Variable> = HashMap::new();
        for scope in &self.scopes {
            for (name, var) in &scope.vars {
                seen.insert(name, var);
            }
        }
        seen.into_iter()
    }
}

impl Default for VarStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set() {
        let mut store = VarStore::new();
        assert_eq!(store.get("FOO"), None);
        store.set("FOO", "bar").unwrap();
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_unset() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        assert_eq!(store.get("FOO"), Some("bar"));
        store.unset("FOO").unwrap();
        assert_eq!(store.get("FOO"), None);
    }

    #[test]
    fn test_readonly_prevents_set() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set_readonly("FOO");
        let result = store.set("FOO", "baz");
        assert!(result.is_err());
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_readonly_prevents_unset() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set_readonly("FOO");
        let result = store.unset("FOO");
        assert!(result.is_err());
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_export() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        assert!(!store.get_var("FOO").unwrap().exported);
        store.export("FOO");
        assert!(store.get_var("FOO").unwrap().exported);
    }

    #[test]
    fn test_to_environ_excludes_unexported() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set("BAZ", "qux").unwrap();
        store.export("FOO");
        let env = store.to_environ();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("FOO".to_string(), "bar".to_string()));
    }

    #[test]
    fn test_from_environ() {
        let store = VarStore::from_environ();
        // All variables should be marked as exported
        for (_, var) in store.scopes[0].vars.iter() {
            assert!(var.exported, "Variables from environ should be exported");
        }
    }

    #[test]
    fn test_push_pop_scope_positional_params() {
        let mut store = VarStore::new();
        store.set_positional_params(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(store.positional_params(), &["a", "b"]);

        store.push_scope(vec!["x".to_string(), "y".to_string(), "z".to_string()]);
        assert_eq!(store.positional_params(), &["x", "y", "z"]);

        store.pop_scope();
        assert_eq!(store.positional_params(), &["a", "b"]);
    }

    #[test]
    fn test_scope_variable_lookup_walks_chain() {
        let mut store = VarStore::new();
        store.set("FOO", "global").unwrap();

        store.push_scope(vec![]);
        // Variable from global scope is visible
        assert_eq!(store.get("FOO"), Some("global"));

        // Setting FOO in function scope updates the global scope (POSIX)
        store.set("FOO", "updated").unwrap();
        store.pop_scope();
        assert_eq!(store.get("FOO"), Some("updated"));
    }

    #[test]
    fn test_scope_new_variable_goes_to_global() {
        let mut store = VarStore::new();
        store.push_scope(vec![]);
        store.set("NEW_VAR", "value").unwrap();
        store.pop_scope();
        // Variable created inside function scope persists in global
        assert_eq!(store.get("NEW_VAR"), Some("value"));
    }

    #[test]
    fn test_scope_readonly_across_scopes() {
        let mut store = VarStore::new();
        store.set("RO", "immutable").unwrap();
        store.set_readonly("RO");

        store.push_scope(vec![]);
        let result = store.set("RO", "changed");
        assert!(result.is_err());
        assert_eq!(store.get("RO"), Some("immutable"));
        store.pop_scope();
    }

    #[test]
    fn test_scope_export_across_scopes() {
        let mut store = VarStore::new();
        store.set("EX", "value").unwrap();

        store.push_scope(vec![]);
        store.export("EX");
        store.pop_scope();

        assert!(store.get_var("EX").unwrap().exported);
    }

    #[test]
    fn test_scope_unset_across_scopes() {
        let mut store = VarStore::new();
        store.set("DEL", "value").unwrap();

        store.push_scope(vec![]);
        store.unset("DEL").unwrap();
        store.pop_scope();

        assert_eq!(store.get("DEL"), None);
    }
}
```

- [ ] **Step 4: Run VarStore tests**

Run: `cargo test --lib env::vars::tests`
Expected: All PASS

- [ ] **Step 5: Commit VarStore refactoring**

```bash
git add src/env/vars.rs
git commit -m "refactor(env): convert VarStore to scope chain with positional params"
```

#### Task 3b: Migrate `ShellEnv` to Use `VarStore` Positional Params

**Files:**
- Modify: `src/env/mod.rs:286-328`
- Modify: `src/expand/param.rs:24-30, 142-152`
- Modify: `src/expand/mod.rs:435, 457`
- Modify: `src/expand/command_sub.rs:45-59`
- Modify: `src/exec/mod.rs:298, 370-390`
- Modify: `src/builtin/special.rs:206-243, 376-380`

- [ ] **Step 1: Remove `positional_params` from `ShellEnv` and update constructor**

In `src/env/mod.rs`, remove the `positional_params` field from `ShellEnv` struct and update `new()`:

```rust
pub struct ShellEnv {
    pub vars: VarStore,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    /// PID of the most recently started background job ($!)
    pub last_bg_pid: Option<i32>,
    pub functions: HashMap<String, FunctionDef>,
    pub flow_control: Option<FlowControl>,
    pub options: ShellOptions,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub bg_jobs: Vec<BgJob>,
    pub expansion_error: bool,
}

impl ShellEnv {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        ShellEnv {
            vars,
            last_exit_status: 0,
            shell_pid: getpid(),
            shell_name: shell_name.into(),
            last_bg_pid: None,
            functions: HashMap::new(),
            flow_control: None,
            options: ShellOptions::default(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            bg_jobs: Vec::new(),
            expansion_error: false,
        }
    }
}
```

- [ ] **Step 2: Fix compilation errors — update all `env.positional_params` references**

Replace every `env.positional_params` with `env.vars.positional_params()` or `env.vars.set_positional_params(...)` across the codebase:

**`src/expand/param.rs:24-30`** — change `env.positional_params.get(...)` to `env.vars.positional_params().get(...)`:

```rust
ParamExpr::Positional(n) => {
    if *n > 0 {
        env.vars.positional_params().get(n - 1).cloned().unwrap_or_default()
    } else {
        String::new()
    }
}
```

**`src/expand/param.rs:147-148`** — change special param expansion:

```rust
SpecialParam::Hash => env.vars.positional_params().len().to_string(),
SpecialParam::At | SpecialParam::Star => env.vars.positional_params().join(" "),
```

**`src/expand/mod.rs:435`** — change quoted `$@` expansion:

```rust
let params = env.vars.positional_params().to_vec();
```

**`src/expand/mod.rs:457`** — change quoted `$*` expansion:

```rust
let joined = env.vars.positional_params().join(&sep.to_string());
```

**`src/exec/mod.rs:298`** — change for-loop default:

```rust
None => self.env.vars.positional_params().to_vec(),
```

**`src/exec/mod.rs:370-390`** — change function call to use `push_scope`/`pop_scope`:

```rust
fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
    self.env.vars.push_scope(args.to_vec());

    let status =
        self.exec_compound_command(&func_def.body, &func_def.redirects);

    // Handle return flow control
    let final_status = match self.env.flow_control.take() {
        Some(FlowControl::Return(s)) => s,
        Some(other) => {
            self.env.flow_control = Some(other);
            status
        }
        None => status,
    };

    self.env.vars.pop_scope();
    self.env.last_exit_status = final_status;
    final_status
}
```

**`src/builtin/special.rs:206, 213, 243`** — change `set` positional param assignment:

Replace `env.positional_params = ...` with `env.vars.set_positional_params(...)`:

```rust
// Line 206:
env.vars.set_positional_params(args[i + 1..].to_vec());

// Line 213:
env.vars.set_positional_params(args[i + 1..].to_vec());

// Line 243:
env.vars.set_positional_params(args[i..].to_vec());
```

**`src/builtin/special.rs:376-380`** — change `shift`:

```rust
if n > env.vars.positional_params().len() {
    eprintln!("kish: shift: shift count out of range");
    return 1;
}
env.vars.set_positional_params(env.vars.positional_params()[n..].to_vec());
```

**`src/expand/command_sub.rs:45-59`** — change child env cloning. Remove the explicit `positional_params` field:

```rust
let mut child_env = ShellEnv {
    vars: env.vars.clone(),
    last_exit_status: env.last_exit_status,
    shell_pid: env.shell_pid,
    shell_name: env.shell_name.clone(),
    last_bg_pid: env.last_bg_pid,
    functions: env.functions.clone(),
    flow_control: None,
    options: env.options.clone(),
    traps: env.traps.clone(),
    aliases: env.aliases.clone(),
    bg_jobs: Vec::new(),
    expansion_error: false,
};
```

- [ ] **Step 3: Fix test files that reference `env.positional_params`**

Update test assertions in `src/env/mod.rs` tests:

```rust
// Change: assert_eq!(env.positional_params, vec!["arg1", "arg2"]);
// To:
assert_eq!(env.vars.positional_params(), &["arg1", "arg2"]);
```

Update test constructors in `src/expand/mod.rs`, `src/expand/param.rs`, and other test files that construct `ShellEnv` — they use `ShellEnv::new(...)` which will work unchanged since the constructor handles the migration internally.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 5: Commit migration**

```bash
git add src/env/mod.rs src/expand/param.rs src/expand/mod.rs src/expand/command_sub.rs src/exec/mod.rs src/builtin/special.rs
git commit -m "refactor(env): migrate positional params from ShellEnv to VarStore scopes"
```

---

### Task 4: `TempDir` ID Collision Prevention

**Files:**
- Modify: `tests/helpers/mod.rs`

- [ ] **Step 1: Apply the fix**

Replace the entire `tests/helpers/mod.rs`:

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("kish-test-{}-{}", id, seq));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write_file(&self, name: &str, content: &str) -> PathBuf {
        let file_path = self.path.join(name);
        std::fs::write(&file_path, content).unwrap();
        file_path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
```

- [ ] **Step 2: Run all tests to verify no regressions**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add tests/helpers/mod.rs
git commit -m "fix(test): add atomic counter to TempDir to prevent ID collisions"
```

---

### Task 5: Update TODO.md and Final Verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove Phase 2 known limitations from TODO.md**

Delete the entire `## Phase 2: Known Limitations` section (lines 3-8) from `TODO.md`.

- [ ] **Step 2: Run full test suite**

Run: `cargo test && cargo build && ./e2e/run_tests.sh`
Expected: All tests PASS, no XFAIL for cd_dash_oldpwd

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed Phase 2 known limitations from TODO.md"
```
