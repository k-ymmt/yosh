# Phase 2: Basic Execution Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute simple commands, pipelines, and lists (`;`, `&&`, `||`, `&`) so kish can run real shell scripts.

**Architecture:** An `Executor` walks the AST produced by Phase 1's parser. It uses a `ShellEnv` to track variables and state, a minimal word expander to convert `Word` AST nodes to strings, and `nix` for POSIX process management (fork/exec/wait/pipe/dup2). Builtins run in-process; external commands fork+exec. Pipelines connect commands via pipes with each command in a child process.

**Tech Stack:** Rust 2024, `nix` 0.31 (process, fs, signal features), `libc` 0.2

**Scope note:** This is Phase 2 of 8. Word expansion is minimal (literals, quotes, simple `$VAR`, `$?`). Full expansion (tilde, command substitution, arithmetic, field splitting, pathname) is Phase 3. Full redirection is Phase 4. Control structures (if/for/while/case) are Phase 5.

---

## File Structure

**Create:**
- `src/env/mod.rs` — ShellEnv: execution environment holding variables, exit status, positional params
- `src/env/vars.rs` — VarStore: scoped variable storage with export/readonly attributes
- `src/expand/mod.rs` — Minimal word expansion (literal + quote removal + basic `$VAR`)
- `src/builtin/mod.rs` — Builtin command dispatch and implementations
- `src/exec/mod.rs` — Executor: AST walker, simple command dispatch
- `src/exec/command.rs` — External command execution (fork/exec/wait, PATH lookup)
- `src/exec/pipeline.rs` — Pipeline execution (pipe + fork + dup2)
- `src/exec/redirect.rs` — Redirection handling (fd save/restore)

**Modify:**
- `Cargo.toml` — Add nix and libc dependencies
- `src/main.rs` — Wire up executor, update CLI to execute instead of just parse

**Reference:**
- `docs/superpowers/specs/2026-04-08-posix-shell-design.md` — Section 6
- `src/parser/ast.rs` — AST types the executor operates on

---

### Task 1: Dependencies and shell environment

**Files:**
- Modify: `Cargo.toml`
- Create: `src/env/mod.rs`
- Create: `src/env/vars.rs`

- [ ] **Step 1: Add dependencies to Cargo.toml**

```toml
[package]
name = "kish"
version = "0.1.0"
edition = "2024"

[dependencies]
nix = { version = "0.31", features = ["signal", "process", "fs"] }
libc = "0.2"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles (may download crates).

- [ ] **Step 3: Create VarStore**

Create `src/env/vars.rs`:

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Variable {
    pub value: String,
    pub exported: bool,
    pub readonly: bool,
}

#[derive(Debug)]
pub struct VarStore {
    vars: HashMap<String, Variable>,
}

impl VarStore {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    /// Initialize from the process environment.
    pub fn from_environ() -> Self {
        let mut store = Self::new();
        for (key, value) in std::env::vars() {
            store.vars.insert(key, Variable {
                value,
                exported: true,
                readonly: false,
            });
        }
        store
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|v| v.value.as_str())
    }

    pub fn get_var(&self, name: &str) -> Option<&Variable> {
        self.vars.get(name)
    }

    pub fn set(&mut self, name: &str, value: String) -> Result<(), String> {
        if let Some(v) = self.vars.get(name) {
            if v.readonly {
                return Err(format!("{}: readonly variable", name));
            }
        }
        let exported = self.vars.get(name).map_or(false, |v| v.exported);
        self.vars.insert(name.to_string(), Variable {
            value,
            exported,
            readonly: false,
        });
        Ok(())
    }

    pub fn unset(&mut self, name: &str) -> Result<(), String> {
        if let Some(v) = self.vars.get(name) {
            if v.readonly {
                return Err(format!("{}: readonly variable", name));
            }
        }
        self.vars.remove(name);
        Ok(())
    }

    pub fn export(&mut self, name: &str) {
        if let Some(v) = self.vars.get_mut(name) {
            v.exported = true;
        } else {
            self.vars.insert(name.to_string(), Variable {
                value: String::new(),
                exported: true,
                readonly: false,
            });
        }
    }

    pub fn set_readonly(&mut self, name: &str) {
        if let Some(v) = self.vars.get_mut(name) {
            v.readonly = true;
        }
    }

    /// Collect exported variables as (name, value) pairs for execve.
    pub fn to_environ(&self) -> Vec<(String, String)> {
        self.vars.iter()
            .filter(|(_, v)| v.exported)
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set() {
        let mut store = VarStore::new();
        assert_eq!(store.get("FOO"), None);
        store.set("FOO", "bar".to_string()).unwrap();
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_unset() {
        let mut store = VarStore::new();
        store.set("FOO", "bar".to_string()).unwrap();
        store.unset("FOO").unwrap();
        assert_eq!(store.get("FOO"), None);
    }

    #[test]
    fn test_readonly_prevents_set() {
        let mut store = VarStore::new();
        store.set("FOO", "bar".to_string()).unwrap();
        store.set_readonly("FOO");
        assert!(store.set("FOO", "baz".to_string()).is_err());
    }

    #[test]
    fn test_readonly_prevents_unset() {
        let mut store = VarStore::new();
        store.set("FOO", "bar".to_string()).unwrap();
        store.set_readonly("FOO");
        assert!(store.unset("FOO").is_err());
    }

    #[test]
    fn test_export() {
        let mut store = VarStore::new();
        store.set("FOO", "bar".to_string()).unwrap();
        assert!(!store.get_var("FOO").unwrap().exported);
        store.export("FOO");
        assert!(store.get_var("FOO").unwrap().exported);
        let env = store.to_environ();
        assert!(env.iter().any(|(k, v)| k == "FOO" && v == "bar"));
    }

    #[test]
    fn test_to_environ_excludes_unexported() {
        let mut store = VarStore::new();
        store.set("FOO", "bar".to_string()).unwrap();
        store.set("BAZ", "qux".to_string()).unwrap();
        store.export("FOO");
        let env = store.to_environ();
        assert!(env.iter().any(|(k, _)| k == "FOO"));
        assert!(!env.iter().any(|(k, _)| k == "BAZ"));
    }

    #[test]
    fn test_from_environ() {
        let store = VarStore::from_environ();
        // PATH should exist in most environments
        assert!(store.get("PATH").is_some());
        assert!(store.get_var("PATH").unwrap().exported);
    }
}
```

- [ ] **Step 4: Create ShellEnv**

Create `src/env/mod.rs`:

```rust
pub mod vars;

use nix::unistd::{getpid, Pid};
use vars::VarStore;

#[derive(Debug)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    pub positional_params: Vec<String>,
}

impl ShellEnv {
    pub fn new(shell_name: String, args: Vec<String>) -> Self {
        Self {
            vars: VarStore::from_environ(),
            last_exit_status: 0,
            shell_pid: getpid(),
            shell_name,
            positional_params: args,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_env_new() {
        let env = ShellEnv::new("kish".to_string(), vec![]);
        assert_eq!(env.last_exit_status, 0);
        assert_eq!(env.shell_name, "kish");
        assert!(env.positional_params.is_empty());
        assert!(env.vars.get("PATH").is_some());
    }
}
```

- [ ] **Step 5: Add module declarations to main.rs**

Add to the top of `src/main.rs`, after existing mod declarations:

```rust
mod env;
```

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: All pass (Phase 1 tests + new env tests).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/env/ src/main.rs
git commit -m "feat(phase2): add nix/libc deps, ShellEnv, and VarStore"
```

---

### Task 2: Minimal word expansion

**Files:**
- Create: `src/expand/mod.rs`
- Modify: `src/main.rs` (add mod declaration)

- [ ] **Step 1: Write tests**

Create `src/expand/mod.rs`:

```rust
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

/// Expand a Word to a single string (no field splitting in Phase 2).
pub fn expand_word_to_string(env: &ShellEnv, word: &Word) -> String {
    todo!()
}

/// Expand a slice of Words to a Vec of strings.
pub fn expand_words(env: &ShellEnv, words: &[Word]) -> Vec<String> {
    words.iter().map(|w| expand_word_to_string(env, w)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env() -> ShellEnv {
        let mut env = ShellEnv::new("kish".to_string(), vec!["arg1".to_string(), "arg2".to_string()]);
        env.vars.set("FOO", "hello".to_string()).unwrap();
        env.vars.set("BAR", "world".to_string()).unwrap();
        env.last_exit_status = 42;
        env
    }

    #[test]
    fn test_literal() {
        let env = test_env();
        let w = Word::literal("hello");
        assert_eq!(expand_word_to_string(&env, &w), "hello");
    }

    #[test]
    fn test_single_quoted() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::SingleQuoted("hello world".to_string())] };
        assert_eq!(expand_word_to_string(&env, &w), "hello world");
    }

    #[test]
    fn test_double_quoted_literal() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::DoubleQuoted(vec![
            WordPart::Literal("hello world".to_string()),
        ])] };
        assert_eq!(expand_word_to_string(&env, &w), "hello world");
    }

    #[test]
    fn test_dollar_single_quoted() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::DollarSingleQuoted("hello\n".to_string())] };
        assert_eq!(expand_word_to_string(&env, &w), "hello\n");
    }

    #[test]
    fn test_simple_param() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Simple("FOO".to_string()))] };
        assert_eq!(expand_word_to_string(&env, &w), "hello");
    }

    #[test]
    fn test_unset_param() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Simple("NOTSET".to_string()))] };
        assert_eq!(expand_word_to_string(&env, &w), "");
    }

    #[test]
    fn test_special_question() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Question))] };
        assert_eq!(expand_word_to_string(&env, &w), "42");
    }

    #[test]
    fn test_special_dollar() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Dollar))] };
        let result = expand_word_to_string(&env, &w);
        assert!(!result.is_empty()); // PID as string
    }

    #[test]
    fn test_special_zero() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Zero))] };
        assert_eq!(expand_word_to_string(&env, &w), "kish");
    }

    #[test]
    fn test_positional_param() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Positional(1))] };
        assert_eq!(expand_word_to_string(&env, &w), "arg1");
    }

    #[test]
    fn test_positional_param_out_of_range() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Positional(99))] };
        assert_eq!(expand_word_to_string(&env, &w), "");
    }

    #[test]
    fn test_special_hash() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash))] };
        assert_eq!(expand_word_to_string(&env, &w), "2");
    }

    #[test]
    fn test_tilde_none() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::Tilde(None)] };
        let result = expand_word_to_string(&env, &w);
        assert!(!result.is_empty()); // HOME directory
    }

    #[test]
    fn test_mixed_parts() {
        let env = test_env();
        let w = Word { parts: vec![
            WordPart::Literal("hi ".to_string()),
            WordPart::Parameter(ParamExpr::Simple("FOO".to_string())),
            WordPart::Literal("!".to_string()),
        ] };
        assert_eq!(expand_word_to_string(&env, &w), "hi hello!");
    }

    #[test]
    fn test_dollar_in_double_quote() {
        let env = test_env();
        let w = Word { parts: vec![WordPart::DoubleQuoted(vec![
            WordPart::Literal("val=".to_string()),
            WordPart::Parameter(ParamExpr::Simple("FOO".to_string())),
        ])] };
        assert_eq!(expand_word_to_string(&env, &w), "val=hello");
    }

    #[test]
    fn test_expand_words() {
        let env = test_env();
        let words = vec![Word::literal("echo"), Word::literal("hello")];
        let result = expand_words(&env, &words);
        assert_eq!(result, vec!["echo", "hello"]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test expand::tests`
Expected: FAIL (not yet implemented).

- [ ] **Step 3: Implement word expansion**

Replace the `todo!()` in `expand_word_to_string`:

```rust
/// Expand a Word to a single string (no field splitting in Phase 2).
pub fn expand_word_to_string(env: &ShellEnv, word: &Word) -> String {
    let mut result = String::new();
    for part in &word.parts {
        expand_part(env, part, &mut result);
    }
    result
}

fn expand_part(env: &ShellEnv, part: &WordPart, out: &mut String) {
    match part {
        WordPart::Literal(s) => out.push_str(s),
        WordPart::SingleQuoted(s) => out.push_str(s),
        WordPart::DollarSingleQuoted(s) => out.push_str(s),
        WordPart::DoubleQuoted(parts) => {
            for p in parts {
                expand_part(env, p, out);
            }
        }
        WordPart::Tilde(None) => {
            if let Some(home) = env.vars.get("HOME") {
                out.push_str(home);
            } else {
                out.push('~');
            }
        }
        WordPart::Tilde(Some(user)) => {
            // ~user expansion not implemented in Phase 2
            out.push('~');
            out.push_str(user);
        }
        WordPart::Parameter(param) => {
            expand_param(env, param, out);
        }
        WordPart::CommandSub(_) => {
            // Deferred to Phase 3
        }
        WordPart::ArithSub(_) => {
            // Deferred to Phase 3
        }
    }
}

fn expand_param(env: &ShellEnv, param: &ParamExpr, out: &mut String) {
    match param {
        ParamExpr::Simple(name) => {
            if let Some(val) = env.vars.get(name) {
                out.push_str(val);
            }
        }
        ParamExpr::Positional(n) => {
            if *n > 0 && *n <= env.positional_params.len() {
                out.push_str(&env.positional_params[*n - 1]);
            }
        }
        ParamExpr::Special(sp) => {
            match sp {
                SpecialParam::Question => {
                    out.push_str(&env.last_exit_status.to_string());
                }
                SpecialParam::Dollar => {
                    out.push_str(&env.shell_pid.as_raw().to_string());
                }
                SpecialParam::Zero => {
                    out.push_str(&env.shell_name);
                }
                SpecialParam::Hash => {
                    out.push_str(&env.positional_params.len().to_string());
                }
                SpecialParam::At | SpecialParam::Star => {
                    // In Phase 2: join all positional params with space
                    out.push_str(&env.positional_params.join(" "));
                }
                SpecialParam::Bang => {
                    // $! — last background PID, not tracked in Phase 2
                }
                SpecialParam::Dash => {
                    // $- — current option flags, not implemented in Phase 2
                }
            }
        }
        ParamExpr::Length(name) => {
            if let Some(val) = env.vars.get(name) {
                out.push_str(&val.len().to_string());
            } else {
                out.push('0');
            }
        }
        ParamExpr::Default { name, word, null_check } => {
            let val = env.vars.get(name);
            let use_default = match val {
                None => true,
                Some(v) if *null_check && v.is_empty() => true,
                _ => false,
            };
            if use_default {
                if let Some(w) = word {
                    out.push_str(&expand_word_to_string(env, w));
                }
            } else if let Some(v) = val {
                out.push_str(v);
            }
        }
        // Other ParamExpr forms deferred to Phase 3
        _ => {}
    }
}
```

- [ ] **Step 4: Add mod declaration to main.rs**

Add `mod expand;` to `src/main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/expand/ src/main.rs
git commit -m "feat(phase2): minimal word expansion (literals, quotes, basic \$VAR)"
```

---

### Task 3: Builtin commands

**Files:**
- Create: `src/builtin/mod.rs`
- Modify: `src/main.rs` (add mod declaration)

- [ ] **Step 1: Write tests and implement builtins**

Create `src/builtin/mod.rs`:

```rust
use crate::env::ShellEnv;
use std::io::Write;

/// Check if a command name is a builtin. Returns true if it is.
pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "exit" | "cd" | "export" | "unset" | "readonly" |
        "true" | "false" | ":" | "echo" | "return"
    )
}

/// Execute a builtin command. Returns the exit status.
pub fn exec_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    match name {
        "exit" => builtin_exit(args, env),
        "cd" => builtin_cd(args, env),
        "export" => builtin_export(args, env),
        "unset" => builtin_unset(args, env),
        "readonly" => builtin_readonly(args, env),
        "true" | ":" => 0,
        "false" => 1,
        "echo" => builtin_echo(args),
        "return" => builtin_return(args, env),
        _ => {
            eprintln!("kish: {}: not a builtin", name);
            1
        }
    }
}

fn builtin_exit(args: &[String], env: &ShellEnv) -> i32 {
    let code = if let Some(arg) = args.first() {
        arg.parse::<i32>().unwrap_or_else(|_| {
            eprintln!("kish: exit: {}: numeric argument required", arg);
            2
        })
    } else {
        env.last_exit_status
    };
    std::process::exit(code & 0xFF);
}

fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 {
    let target = if let Some(dir) = args.first() {
        dir.to_string()
    } else if let Some(home) = env.vars.get("HOME") {
        home.to_string()
    } else {
        eprintln!("kish: cd: HOME not set");
        return 1;
    };

    match std::env::set_current_dir(&target) {
        Ok(()) => {
            if let Ok(pwd) = std::env::current_dir() {
                let _ = env.vars.set("PWD", pwd.to_string_lossy().to_string());
            }
            0
        }
        Err(e) => {
            eprintln!("kish: cd: {}: {}", target, e);
            1
        }
    }
}

fn builtin_export(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Print all exported variables
        for (name, value) in env.vars.to_environ() {
            println!("export {}=\"{}\"", name, value);
        }
        return 0;
    }

    for arg in args {
        if let Some(eq_pos) = arg.find('=') {
            let name = &arg[..eq_pos];
            let value = &arg[eq_pos + 1..];
            if let Err(e) = env.vars.set(name, value.to_string()) {
                eprintln!("kish: export: {}", e);
                return 1;
            }
            env.vars.export(name);
        } else {
            env.vars.export(arg);
        }
    }
    0
}

fn builtin_unset(args: &[String], env: &mut ShellEnv) -> i32 {
    for name in args {
        if let Err(e) = env.vars.unset(name) {
            eprintln!("kish: unset: {}", e);
            return 1;
        }
    }
    0
}

fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> i32 {
    for arg in args {
        if let Some(eq_pos) = arg.find('=') {
            let name = &arg[..eq_pos];
            let value = &arg[eq_pos + 1..];
            if let Err(e) = env.vars.set(name, value.to_string()) {
                eprintln!("kish: readonly: {}", e);
                return 1;
            }
            env.vars.set_readonly(name);
        } else {
            env.vars.set_readonly(arg);
        }
    }
    0
}

fn builtin_echo(args: &[String]) -> i32 {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut first = true;
    for arg in args {
        if !first {
            let _ = write!(out, " ");
        }
        let _ = write!(out, "{}", arg);
        first = false;
    }
    let _ = writeln!(out);
    0
}

fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
    // In Phase 2, return behaves like setting the exit status.
    // Full return-from-function behavior is Phase 5.
    if let Some(arg) = args.first() {
        arg.parse::<i32>().unwrap_or_else(|_| {
            eprintln!("kish: return: {}: numeric argument required", arg);
            2
        }) & 0xFF
    } else {
        env.last_exit_status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env() -> ShellEnv {
        ShellEnv::new("kish".to_string(), vec![])
    }

    #[test]
    fn test_is_builtin() {
        assert!(is_builtin("cd"));
        assert!(is_builtin("exit"));
        assert!(is_builtin("true"));
        assert!(is_builtin("false"));
        assert!(is_builtin(":"));
        assert!(is_builtin("echo"));
        assert!(is_builtin("export"));
        assert!(is_builtin("unset"));
        assert!(!is_builtin("ls"));
        assert!(!is_builtin("grep"));
    }

    #[test]
    fn test_true_false_colon() {
        let mut env = test_env();
        assert_eq!(exec_builtin("true", &[], &mut env), 0);
        assert_eq!(exec_builtin("false", &[], &mut env), 1);
        assert_eq!(exec_builtin(":", &[], &mut env), 0);
    }

    #[test]
    fn test_export_and_unset() {
        let mut env = test_env();
        exec_builtin("export", &["FOO=bar".to_string()], &mut env);
        assert_eq!(env.vars.get("FOO"), Some("bar"));
        assert!(env.vars.get_var("FOO").unwrap().exported);

        exec_builtin("unset", &["FOO".to_string()], &mut env);
        assert_eq!(env.vars.get("FOO"), None);
    }

    #[test]
    fn test_cd_to_tmp() {
        let mut env = test_env();
        let original = std::env::current_dir().unwrap();
        let status = exec_builtin("cd", &["/tmp".to_string()], &mut env);
        assert_eq!(status, 0);
        // Restore
        std::env::set_current_dir(original).unwrap();
    }

    #[test]
    fn test_cd_nonexistent() {
        let mut env = test_env();
        let status = exec_builtin("cd", &["/nonexistent_dir_12345".to_string()], &mut env);
        assert_ne!(status, 0);
    }
}
```

- [ ] **Step 2: Add mod declaration to main.rs**

Add `mod builtin;` to `src/main.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/builtin/ src/main.rs
git commit -m "feat(phase2): builtin commands (exit, cd, echo, export, unset, true, false)"
```

---

### Task 4: External command execution

**Files:**
- Create: `src/exec/mod.rs` (placeholder with module declarations)
- Create: `src/exec/command.rs`
- Modify: `src/main.rs` (add mod declaration)

- [ ] **Step 1: Create exec module placeholder**

Create `src/exec/mod.rs`:

```rust
pub mod command;
```

- [ ] **Step 2: Write tests and implement external command execution**

Create `src/exec/command.rs`:

```rust
use std::ffi::CString;
use std::path::{Path, PathBuf};

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{execvp, fork, ForkResult};

/// Search for a command in PATH. Returns the full path if found.
pub fn find_in_path(cmd: &str, path_var: &str) -> Option<PathBuf> {
    for dir in path_var.split(':') {
        let candidate = Path::new(dir).join(cmd);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Execute an external command via fork+exec. Returns exit status.
/// `env_vars` is a list of (name, value) pairs for the child's environment.
pub fn exec_external(
    cmd: &str,
    args: &[String],
    env_vars: &[(String, String)],
) -> i32 {
    // Build C strings for execve
    let c_args: Vec<CString> = args.iter()
        .map(|a| CString::new(a.as_str()).unwrap_or_else(|_| CString::new("").unwrap()))
        .collect();
    let c_arg_refs: Vec<&std::ffi::CStr> = c_args.iter().map(|a| a.as_c_str()).collect();

    let c_cmd = match CString::new(cmd) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("kish: {}: invalid command name", cmd);
            return 127;
        }
    };

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Set up environment
            for (key, value) in env_vars {
                std::env::set_var(key, value);
            }
            // exec replaces the child process
            let _ = execvp(&c_cmd, &c_arg_refs);
            // If execvp returns, it failed
            let err = std::io::Error::last_os_error();
            eprintln!("kish: {}: {}", cmd, err);
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                std::process::exit(126);
            } else {
                std::process::exit(127);
            }
        }
        Ok(ForkResult::Parent { child }) => {
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(_, code)) => code,
                Ok(WaitStatus::Signaled(_, sig, _)) => 128 + sig as i32,
                _ => 1,
            }
        }
        Err(e) => {
            eprintln!("kish: fork: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_in_path_exists() {
        let path = std::env::var("PATH").unwrap_or_default();
        let result = find_in_path("sh", &path);
        assert!(result.is_some(), "sh should be found in PATH");
    }

    #[test]
    fn test_find_in_path_not_found() {
        let result = find_in_path("nonexistent_cmd_12345", "/usr/bin:/bin");
        assert!(result.is_none());
    }

    #[test]
    fn test_exec_external_true() {
        let status = exec_external("true", &["true".to_string()], &[]);
        assert_eq!(status, 0);
    }

    #[test]
    fn test_exec_external_false() {
        let status = exec_external("false", &["false".to_string()], &[]);
        assert_eq!(status, 1);
    }

    #[test]
    fn test_exec_external_not_found() {
        let status = exec_external(
            "nonexistent_command_12345",
            &["nonexistent_command_12345".to_string()],
            &[],
        );
        assert_eq!(status, 127);
    }
}
```

- [ ] **Step 3: Add mod declaration to main.rs**

Add `mod exec;` to `src/main.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/ src/main.rs
git commit -m "feat(phase2): external command execution with fork/exec and PATH lookup"
```

---

### Task 5: Redirection handling

**Files:**
- Create: `src/exec/redirect.rs`
- Modify: `src/exec/mod.rs` (add module declaration)

- [ ] **Step 1: Implement redirection handling**

Create `src/exec/redirect.rs`:

```rust
use std::os::fd::RawFd;

use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::unistd::{close, dup, dup2};

use crate::env::ShellEnv;
use crate::expand::expand_word_to_string;
use crate::parser::ast::{Redirect, RedirectKind};

/// Tracks saved file descriptors for restoring after builtin execution.
pub struct RedirectState {
    saved_fds: Vec<(RawFd, RawFd)>, // (target_fd, saved_copy)
}

impl RedirectState {
    pub fn new() -> Self {
        Self { saved_fds: Vec::new() }
    }

    /// Apply redirections. For builtins, call restore() after.
    /// For external commands in child process, no restore needed.
    pub fn apply(&mut self, redirects: &[Redirect], env: &ShellEnv, save: bool) -> Result<(), String> {
        for redir in redirects {
            self.apply_one(redir, env, save)?;
        }
        Ok(())
    }

    fn apply_one(&mut self, redir: &Redirect, env: &ShellEnv, save: bool) -> Result<(), String> {
        match &redir.kind {
            RedirectKind::Input(word) => {
                let path = expand_word_to_string(env, word);
                let target_fd = redir.fd.unwrap_or(0);
                let fd = open(
                    path.as_str(),
                    OFlag::O_RDONLY,
                    Mode::empty(),
                ).map_err(|e| format!("{}: {}", path, e))?;
                if save { self.save_fd(target_fd)?; }
                dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                if fd != target_fd { close(fd).ok(); }
            }
            RedirectKind::Output(word) => {
                let path = expand_word_to_string(env, word);
                let target_fd = redir.fd.unwrap_or(1);
                let fd = open(
                    path.as_str(),
                    OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
                    Mode::from_bits_truncate(0o666),
                ).map_err(|e| format!("{}: {}", path, e))?;
                if save { self.save_fd(target_fd)?; }
                dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                if fd != target_fd { close(fd).ok(); }
            }
            RedirectKind::OutputClobber(word) => {
                // Same as Output (noclobber not implemented in Phase 2)
                let path = expand_word_to_string(env, word);
                let target_fd = redir.fd.unwrap_or(1);
                let fd = open(
                    path.as_str(),
                    OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
                    Mode::from_bits_truncate(0o666),
                ).map_err(|e| format!("{}: {}", path, e))?;
                if save { self.save_fd(target_fd)?; }
                dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                if fd != target_fd { close(fd).ok(); }
            }
            RedirectKind::Append(word) => {
                let path = expand_word_to_string(env, word);
                let target_fd = redir.fd.unwrap_or(1);
                let fd = open(
                    path.as_str(),
                    OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_APPEND,
                    Mode::from_bits_truncate(0o666),
                ).map_err(|e| format!("{}: {}", path, e))?;
                if save { self.save_fd(target_fd)?; }
                dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                if fd != target_fd { close(fd).ok(); }
            }
            RedirectKind::DupOutput(word) => {
                let target_fd = redir.fd.unwrap_or(1);
                let source = expand_word_to_string(env, word);
                if source == "-" {
                    if save { self.save_fd(target_fd)?; }
                    close(target_fd).ok();
                } else {
                    let source_fd: RawFd = source.parse()
                        .map_err(|_| format!("{}: bad file descriptor", source))?;
                    if save { self.save_fd(target_fd)?; }
                    dup2(source_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                }
            }
            RedirectKind::DupInput(word) => {
                let target_fd = redir.fd.unwrap_or(0);
                let source = expand_word_to_string(env, word);
                if source == "-" {
                    if save { self.save_fd(target_fd)?; }
                    close(target_fd).ok();
                } else {
                    let source_fd: RawFd = source.parse()
                        .map_err(|_| format!("{}: bad file descriptor", source))?;
                    if save { self.save_fd(target_fd)?; }
                    dup2(source_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                }
            }
            RedirectKind::ReadWrite(word) => {
                let path = expand_word_to_string(env, word);
                let target_fd = redir.fd.unwrap_or(0);
                let fd = open(
                    path.as_str(),
                    OFlag::O_RDWR | OFlag::O_CREAT,
                    Mode::from_bits_truncate(0o666),
                ).map_err(|e| format!("{}: {}", path, e))?;
                if save { self.save_fd(target_fd)?; }
                dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                if fd != target_fd { close(fd).ok(); }
            }
            RedirectKind::HereDoc(_) => {
                // Here-document I/O deferred to Phase 4
            }
        }
        Ok(())
    }

    fn save_fd(&mut self, fd: RawFd) -> Result<(), String> {
        let saved = dup(fd).map_err(|e| format!("dup: {}", e))?;
        self.saved_fds.push((fd, saved));
        Ok(())
    }

    /// Restore all saved file descriptors.
    pub fn restore(&mut self) {
        for (target_fd, saved_fd) in self.saved_fds.drain(..).rev() {
            let _ = dup2(saved_fd, target_fd);
            let _ = close(saved_fd);
        }
    }
}

impl Drop for RedirectState {
    fn drop(&mut self) {
        // Ensure saved fds are closed even if restore() wasn't called
        for (_, saved_fd) in &self.saved_fds {
            let _ = close(*saved_fd);
        }
    }
}
```

- [ ] **Step 2: Add module declaration**

Update `src/exec/mod.rs`:

```rust
pub mod command;
pub mod redirect;
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/exec/redirect.rs src/exec/mod.rs
git commit -m "feat(phase2): redirection handling with fd save/restore"
```

---

### Task 6: Executor — simple command dispatch

**Files:**
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Write tests**

Add to `src/exec/mod.rs`:

```rust
pub mod command;
pub mod redirect;

use crate::builtin;
use crate::env::ShellEnv;
use crate::expand::{expand_word_to_string, expand_words};
use crate::parser::ast::*;
use redirect::RedirectState;

pub struct Executor {
    pub env: ShellEnv,
}

impl Executor {
    pub fn new(shell_name: String, args: Vec<String>) -> Self {
        Self {
            env: ShellEnv::new(shell_name, args),
        }
    }

    pub fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32 {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple(words: &[&str]) -> SimpleCommand {
        SimpleCommand {
            assignments: vec![],
            words: words.iter().map(|w| Word::literal(w)).collect(),
            redirects: vec![],
        }
    }

    fn make_simple_with_assigns(assigns: &[(&str, &str)], words: &[&str]) -> SimpleCommand {
        SimpleCommand {
            assignments: assigns.iter().map(|(n, v)| Assignment {
                name: n.to_string(),
                value: Some(Word::literal(v)),
            }).collect(),
            words: words.iter().map(|w| Word::literal(w)).collect(),
            redirects: vec![],
        }
    }

    #[test]
    fn test_exec_builtin_true() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let cmd = make_simple(&["true"]);
        assert_eq!(exec.exec_simple_command(&cmd), 0);
    }

    #[test]
    fn test_exec_builtin_false() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let cmd = make_simple(&["false"]);
        assert_eq!(exec.exec_simple_command(&cmd), 1);
    }

    #[test]
    fn test_exec_external_true() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let cmd = make_simple(&["/usr/bin/true"]);
        assert_eq!(exec.exec_simple_command(&cmd), 0);
    }

    #[test]
    fn test_assignment_only() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let cmd = SimpleCommand {
            assignments: vec![Assignment {
                name: "FOO".to_string(),
                value: Some(Word::literal("bar")),
            }],
            words: vec![],
            redirects: vec![],
        };
        assert_eq!(exec.exec_simple_command(&cmd), 0);
        assert_eq!(exec.env.vars.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_exit_status_tracked() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        exec.exec_simple_command(&make_simple(&["false"]));
        assert_eq!(exec.env.last_exit_status, 1);
        exec.exec_simple_command(&make_simple(&["true"]));
        assert_eq!(exec.env.last_exit_status, 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test exec::tests`
Expected: FAIL (not yet implemented).

- [ ] **Step 3: Implement exec_simple_command**

```rust
    pub fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32 {
        // Expand words
        let args = expand_words(&self.env, &cmd.words);

        if args.is_empty() {
            // No command name — apply assignments to current environment
            for assign in &cmd.assignments {
                let value = assign.value.as_ref()
                    .map(|w| expand_word_to_string(&self.env, w))
                    .unwrap_or_default();
                if let Err(e) = self.env.vars.set(&assign.name, value) {
                    eprintln!("kish: {}", e);
                    self.env.last_exit_status = 1;
                    return 1;
                }
            }
            // Exit status: 0 (no command substitution in Phase 2)
            self.env.last_exit_status = 0;
            return 0;
        }

        let cmd_name = &args[0];

        // Check if command is a builtin
        if builtin::is_builtin(cmd_name) {
            // Apply redirections with save (for restore after)
            let mut redir_state = RedirectState::new();
            if let Err(e) = redir_state.apply(&cmd.redirects, &self.env, true) {
                eprintln!("kish: {}", e);
                self.env.last_exit_status = 1;
                return 1;
            }

            let status = builtin::exec_builtin(cmd_name, &args[1..], &mut self.env);

            // Restore redirections
            redir_state.restore();

            self.env.last_exit_status = status;
            return status;
        }

        // External command
        let env_vars = self.build_env_vars(&cmd.assignments);

        let status = self.exec_external_with_redirects(cmd_name, &args, &env_vars, &cmd.redirects);
        self.env.last_exit_status = status;
        status
    }

    fn exec_external_with_redirects(
        &self,
        cmd_name: &str,
        args: &[String],
        env_vars: &[(String, String)],
        redirects: &[Redirect],
    ) -> i32 {
        use nix::unistd::{fork, ForkResult};
        use nix::sys::wait::{waitpid, WaitStatus};
        use std::ffi::CString;

        let c_cmd = match CString::new(cmd_name.as_bytes()) {
            Ok(c) => c,
            Err(_) => { eprintln!("kish: {}: invalid command", cmd_name); return 127; }
        };
        let c_args: Vec<CString> = args.iter()
            .map(|a| CString::new(a.as_str()).unwrap_or_else(|_| CString::new("").unwrap()))
            .collect();
        let c_arg_refs: Vec<&std::ffi::CStr> = c_args.iter().map(|a| a.as_c_str()).collect();

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                // Apply redirections (no save needed — child process)
                let mut redir_state = RedirectState::new();
                if let Err(e) = redir_state.apply(redirects, &self.env, false) {
                    eprintln!("kish: {}", e);
                    std::process::exit(1);
                }
                // Set environment
                for (key, value) in env_vars {
                    std::env::set_var(key, value);
                }
                // Exec
                let _ = nix::unistd::execvp(&c_cmd, &c_arg_refs);
                let err = std::io::Error::last_os_error();
                eprintln!("kish: {}: {}", cmd_name, err);
                std::process::exit(if err.kind() == std::io::ErrorKind::PermissionDenied { 126 } else { 127 });
            }
            Ok(ForkResult::Parent { child }) => {
                match waitpid(child, None) {
                    Ok(WaitStatus::Exited(_, code)) => code,
                    Ok(WaitStatus::Signaled(_, sig, _)) => 128 + sig as i32,
                    _ => 1,
                }
            }
            Err(e) => { eprintln!("kish: fork: {}", e); 1 }
        }
    }

    fn build_env_vars(&self, assignments: &[Assignment]) -> Vec<(String, String)> {
        let mut env_vars = self.env.vars.to_environ();
        for assign in assignments {
            let value = assign.value.as_ref()
                .map(|w| expand_word_to_string(&self.env, w))
                .unwrap_or_default();
            // Add or replace in the env vars list
            if let Some(existing) = env_vars.iter_mut().find(|(k, _)| k == &assign.name) {
                existing.1 = value;
            } else {
                env_vars.push((assign.name.clone(), value));
            }
        }
        env_vars
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs
git commit -m "feat(phase2): executor simple command dispatch (builtins + external)"
```

---

### Task 7: Pipeline execution

**Files:**
- Create: `src/exec/pipeline.rs`
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Write tests in exec/mod.rs**

Add to `exec::tests`:

```rust
    fn make_pipeline(commands: Vec<SimpleCommand>, negated: bool) -> Pipeline {
        Pipeline {
            negated,
            commands: commands.into_iter().map(Command::Simple).collect(),
        }
    }

    #[test]
    fn test_single_command_pipeline() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let pipeline = make_pipeline(vec![make_simple(&["true"])], false);
        assert_eq!(exec.exec_pipeline(&pipeline), 0);
    }

    #[test]
    fn test_negated_pipeline() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let pipeline = make_pipeline(vec![make_simple(&["true"])], true);
        assert_eq!(exec.exec_pipeline(&pipeline), 1);

        let pipeline = make_pipeline(vec![make_simple(&["false"])], true);
        assert_eq!(exec.exec_pipeline(&pipeline), 0);
    }
```

- [ ] **Step 2: Implement pipeline execution**

Create `src/exec/pipeline.rs`:

```rust
use std::os::fd::RawFd;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{close, dup2, fork, pipe, ForkResult, Pid};

use crate::env::ShellEnv;
use crate::parser::ast::*;

use super::Executor;

impl Executor {
    pub fn exec_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
        let status = if pipeline.commands.len() == 1 {
            self.exec_command(&pipeline.commands[0])
        } else {
            self.exec_multi_pipeline(&pipeline.commands)
        };

        let status = if pipeline.negated {
            if status == 0 { 1 } else { 0 }
        } else {
            status
        };

        self.env.last_exit_status = status;
        status
    }

    fn exec_multi_pipeline(&mut self, commands: &[Command]) -> i32 {
        let n = commands.len();
        let mut pipes: Vec<(RawFd, RawFd)> = Vec::new();
        let mut children: Vec<Pid> = Vec::new();

        // Create n-1 pipes
        for _ in 0..n - 1 {
            let (read_fd, write_fd) = pipe().expect("pipe failed");
            pipes.push((read_fd.into(), write_fd.into()));
        }

        for (i, cmd) in commands.iter().enumerate() {
            match unsafe { fork() } {
                Ok(ForkResult::Child) => {
                    // Set up stdin from previous pipe
                    if i > 0 {
                        let (read_fd, _) = pipes[i - 1];
                        dup2(read_fd, 0).expect("dup2 stdin failed");
                    }
                    // Set up stdout to next pipe
                    if i < n - 1 {
                        let (_, write_fd) = pipes[i];
                        dup2(write_fd, 1).expect("dup2 stdout failed");
                    }
                    // Close all pipe fds in child
                    for &(r, w) in &pipes {
                        let _ = close(r);
                        let _ = close(w);
                    }
                    // Execute
                    let status = self.exec_command(cmd);
                    std::process::exit(status);
                }
                Ok(ForkResult::Parent { child }) => {
                    children.push(child);
                }
                Err(e) => {
                    eprintln!("kish: fork: {}", e);
                    return 1;
                }
            }
        }

        // Parent: close all pipe fds
        for (r, w) in pipes {
            let _ = close(r);
            let _ = close(w);
        }

        // Wait for all children
        let mut last_status = 0;
        for child in &children {
            match waitpid(*child, None) {
                Ok(WaitStatus::Exited(_, code)) => {
                    last_status = code;
                }
                Ok(WaitStatus::Signaled(_, sig, _)) => {
                    last_status = 128 + sig as i32;
                }
                _ => {}
            }
        }

        last_status
    }
}
```

Add `exec_command` method to `src/exec/mod.rs`:

```rust
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        match cmd {
            Command::Simple(sc) => self.exec_simple_command(sc),
            Command::Compound(cc, redirects) => {
                // Compound command execution deferred to Phase 5
                eprintln!("kish: compound commands not yet implemented in Phase 2");
                1
            }
            Command::FunctionDef(fd) => {
                // Function definition deferred to Phase 5
                eprintln!("kish: functions not yet implemented in Phase 2");
                1
            }
        }
    }
```

Update `src/exec/mod.rs` to add module:

```rust
pub mod command;
pub mod pipeline;
pub mod redirect;
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/exec/
git commit -m "feat(phase2): pipeline execution with pipe/fork/dup2"
```

---

### Task 8: AND-OR lists, complete commands, and program execution

**Files:**
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Write tests**

Add to `exec::tests`:

```rust
    fn make_and_or(pipelines: Vec<(Option<AndOrOp>, Pipeline)>) -> AndOrList {
        let first = pipelines[0].1.clone();
        let rest: Vec<(AndOrOp, Pipeline)> = pipelines[1..].iter()
            .map(|(op, p)| (op.unwrap(), p.clone()))
            .collect();
        AndOrList { first, rest }
    }

    #[test]
    fn test_and_list_all_succeed() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let aol = AndOrList {
            first: make_pipeline(vec![make_simple(&["true"])], false),
            rest: vec![(AndOrOp::And, make_pipeline(vec![make_simple(&["true"])], false))],
        };
        assert_eq!(exec.exec_and_or(&aol), 0);
    }

    #[test]
    fn test_and_list_first_fails() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let aol = AndOrList {
            first: make_pipeline(vec![make_simple(&["false"])], false),
            rest: vec![(AndOrOp::And, make_pipeline(vec![make_simple(&["true"])], false))],
        };
        assert_eq!(exec.exec_and_or(&aol), 1);
    }

    #[test]
    fn test_or_list_first_fails() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let aol = AndOrList {
            first: make_pipeline(vec![make_simple(&["false"])], false),
            rest: vec![(AndOrOp::Or, make_pipeline(vec![make_simple(&["true"])], false))],
        };
        assert_eq!(exec.exec_and_or(&aol), 0);
    }

    #[test]
    fn test_or_list_first_succeeds() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let aol = AndOrList {
            first: make_pipeline(vec![make_simple(&["true"])], false),
            rest: vec![(AndOrOp::Or, make_pipeline(vec![make_simple(&["false"])], false))],
        };
        assert_eq!(exec.exec_and_or(&aol), 0);
    }

    #[test]
    fn test_exec_program_sequential() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let prog = Program {
            commands: vec![
                CompleteCommand {
                    items: vec![
                        (AndOrList {
                            first: make_pipeline(vec![make_simple(&["true"])], false),
                            rest: vec![],
                        }, Some(SeparatorOp::Semi)),
                        (AndOrList {
                            first: make_pipeline(vec![make_simple(&["false"])], false),
                            rest: vec![],
                        }, None),
                    ],
                },
            ],
        };
        let status = exec.exec_program(&prog);
        assert_eq!(status, 1); // last command's status
    }
```

- [ ] **Step 2: Implement AND-OR and program execution**

Add to `Executor` impl in `src/exec/mod.rs`:

```rust
    pub fn exec_program(&mut self, program: &Program) -> i32 {
        let mut status = 0;
        for complete_cmd in &program.commands {
            status = self.exec_complete_command(complete_cmd);
        }
        status
    }

    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        let mut status = 0;
        for (and_or, sep) in &cmd.items {
            match sep {
                Some(SeparatorOp::Amp) => {
                    // Async execution: fork and continue
                    match unsafe { nix::unistd::fork() } {
                        Ok(ForkResult::Child) => {
                            let s = self.exec_and_or(and_or);
                            std::process::exit(s);
                        }
                        Ok(ForkResult::Parent { child }) => {
                            // TODO: track background jobs in Phase 7
                            status = 0; // async list exit status is 0
                            self.env.last_exit_status = 0;
                        }
                        Err(e) => {
                            eprintln!("kish: fork: {}", e);
                            status = 1;
                        }
                    }
                }
                _ => {
                    // Sequential execution (; or implicit)
                    status = self.exec_and_or(and_or);
                }
            }
        }
        status
    }

    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let mut status = self.exec_pipeline(&and_or.first);

        for (op, pipeline) in &and_or.rest {
            match op {
                AndOrOp::And => {
                    if status == 0 {
                        status = self.exec_pipeline(pipeline);
                    }
                }
                AndOrOp::Or => {
                    if status != 0 {
                        status = self.exec_pipeline(pipeline);
                    }
                }
            }
        }

        self.env.last_exit_status = status;
        status
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/exec/mod.rs
git commit -m "feat(phase2): AND-OR lists, sequential/async execution, program runner"
```

---

### Task 9: Update entry point and integration tests

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/parser_integration.rs` (add execution tests)

- [ ] **Step 1: Update main.rs to execute**

Replace `src/main.rs`:

```rust
mod builtin;
mod env;
mod error;
mod exec;
mod expand;
mod lexer;
mod parser;

use std::env as std_env;
use std::fs;
use std::io::{self, Read};
use std::process;

use exec::Executor;

fn main() {
    let args: Vec<String> = std_env::args().collect();
    let shell_name = args.first().map_or("kish".to_string(), |a| a.clone());

    match args.len() {
        1 => {
            eprintln!("kish: interactive mode not yet implemented");
            process::exit(1);
        }
        _ => {
            if args[1] == "-c" {
                if args.len() < 3 {
                    eprintln!("kish: -c requires an argument");
                    process::exit(2);
                }
                let positional: Vec<String> = if args.len() > 3 {
                    args[3..].to_vec()
                } else {
                    vec![]
                };
                let shell_name_for_c = if args.len() > 3 { args[3].clone() } else { shell_name };
                let status = run_string(&args[2], shell_name_for_c, positional);
                process::exit(status);
            } else if args[1] == "--parse" {
                if args.len() < 3 {
                    eprintln!("kish: --parse requires an argument");
                    process::exit(2);
                }
                let input = if args[2] == "-" {
                    let mut buf = String::new();
                    io::stdin().read_to_string(&mut buf).unwrap();
                    buf
                } else {
                    args[2].clone()
                };
                match parser::Parser::new(&input).parse_program() {
                    Ok(ast) => println!("{:#?}", ast),
                    Err(e) => {
                        eprintln!("{}", e);
                        process::exit(2);
                    }
                }
            } else {
                let positional: Vec<String> = args[2..].to_vec();
                let status = run_file(&args[1], shell_name, positional);
                process::exit(status);
            }
        }
    }
}

fn run_string(input: &str, shell_name: String, positional: Vec<String>) -> i32 {
    match parser::Parser::new(input).parse_program() {
        Ok(program) => {
            let mut executor = Executor::new(shell_name, positional);
            executor.exec_program(&program)
        }
        Err(e) => {
            eprintln!("{}", e);
            2
        }
    }
}

fn run_file(path: &str, shell_name: String, positional: Vec<String>) -> i32 {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish: {}: {}", path, e);
            return 127;
        }
    };
    run_string(&content, shell_name, positional)
}
```

- [ ] **Step 2: Add execution integration tests**

Add to `tests/parser_integration.rs` (keep existing parse tests, add execution tests):

```rust
fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

fn kish_stdout(input: &str) -> String {
    let out = kish_exec(input);
    String::from_utf8_lossy(&out.stdout).to_string()
}

// --- Execution tests ---

#[test]
fn test_exec_echo() {
    let out = kish_exec("echo hello world");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_exec_true_false() {
    let out = kish_exec("true");
    assert!(out.status.success());

    let out = kish_exec("false");
    assert!(!out.status.success());
}

#[test]
fn test_exec_exit_code() {
    let out = kish_exec("exit 42");
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn test_exec_pipeline() {
    let out = kish_exec("echo hello | tr h H");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello\n");
}

#[test]
fn test_exec_pipeline_exit_status() {
    let out = kish_exec("false | true");
    assert!(out.status.success());

    let out = kish_exec("true | false");
    assert!(!out.status.success());
}

#[test]
fn test_exec_and_list() {
    let out = kish_exec("true && echo yes");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");

    let out = kish_exec("false && echo yes");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_or_list() {
    let out = kish_exec("false || echo fallback");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "fallback\n");

    let out = kish_exec("true || echo fallback");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_semicolon_list() {
    let out = kish_exec("echo first; echo second");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "first\nsecond\n");
}

#[test]
fn test_exec_negated_pipeline() {
    let out = kish_exec("! false");
    assert!(out.status.success());

    let out = kish_exec("! true");
    assert!(!out.status.success());
}

#[test]
fn test_exec_variable_expansion() {
    let out = kish_exec("FOO=hello; echo $FOO");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_exec_exit_status_variable() {
    let out = kish_exec("false; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

#[test]
fn test_exec_assignment_with_command() {
    // Assignment before command: exported to command env, not current env
    let out = kish_exec("FOO=bar echo hello; echo $FOO");
    // FOO should NOT persist in current env (external command)
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n\n");
}

#[test]
fn test_exec_export() {
    let out = kish_exec("export FOO=bar; echo $FOO");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bar\n");
}

#[test]
fn test_exec_output_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    let cmd = format!("echo hello > {}", outfile.display());
    let out = kish_exec(&cmd);
    assert!(out.status.success());
    let content = std::fs::read_to_string(&outfile).unwrap();
    assert_eq!(content, "hello\n");
}

#[test]
fn test_exec_append_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    let cmd = format!("echo first > {}; echo second >> {}", outfile.display(), outfile.display());
    kish_exec(&cmd);
    let content = std::fs::read_to_string(&outfile).unwrap();
    assert_eq!(content, "first\nsecond\n");
}

#[test]
fn test_exec_input_redirect() {
    let tmp = helpers::TempDir::new();
    let infile = tmp.write_file("in.txt", "hello from file\n");
    let cmd = format!("cat < {}", infile.display());
    let out = kish_exec(&cmd);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from file\n");
}

#[test]
fn test_exec_fd_redirect() {
    let out = kish_exec("echo error >&2");
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "error");
}

#[test]
fn test_exec_command_not_found() {
    let out = kish_exec("nonexistent_cmd_12345");
    assert_eq!(out.status.code(), Some(127));
}

#[test]
fn test_exec_script_file() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh", "echo hello\necho world\n");
    let output = Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output()
        .expect("failed to execute kish");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\nworld\n");
}

#[test]
fn test_exec_complex_pipeline() {
    let out = kish_exec("echo 'hello world' | tr ' ' '\\n' | sort");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("world"));
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All pass (Phase 1 parse tests + Phase 2 unit + integration tests).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs tests/parser_integration.rs
git commit -m "feat(phase2): wire up executor in main.rs, add execution integration tests"
```

---

## Subsequent Phases

This plan covers **Phase 2 only** (basic execution engine). After this phase is complete:

- **Phase 3:** Full word expansion (tilde, parameter, command sub, arithmetic, field splitting, pathname, quote removal)
- **Phase 4:** Full redirection + here-document I/O
- **Phase 5:** Control structure execution (if, for, while, until, case, functions)
- **Phase 6:** Special builtins (set, export, trap, eval, exec, etc.) + alias expansion
- **Phase 7:** Signals and errexit
- **Phase 8:** Subshell environment isolation
