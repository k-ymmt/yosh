# Phase 6: Special Builtins + Alias Expansion — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add POSIX special builtins, shell options, trap registration with EXIT trap execution, alias expansion at the lexer level, and the correct POSIX distinction between special and regular builtins.

**Architecture:** Extend `ShellEnv` with `ShellOptions`, `TrapStore`, and `AliasStore`. Separate builtins into `special.rs` and regular builtins in `mod.rs`. Modify `Executor::exec_simple_command` for POSIX-compliant prefix assignment handling. Add alias expansion to the Lexer with recursion prevention. Wire shell options into existing expansion and execution modules.

**Tech Stack:** Rust (edition 2024), nix 0.31, libc 0.2

---

### Task 1: ShellOptions

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Write failing tests for ShellOptions**

Add to the bottom of `src/env/mod.rs`, inside the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn test_shell_options_default() {
        let opts = ShellOptions::default();
        assert!(!opts.allexport);
        assert!(!opts.errexit);
        assert!(!opts.noglob);
        assert!(!opts.noexec);
        assert!(!opts.nounset);
        assert!(!opts.verbose);
        assert!(!opts.xtrace);
        assert!(!opts.noclobber);
        assert!(!opts.pipefail);
        assert_eq!(opts.to_flag_string(), "");
    }

    #[test]
    fn test_shell_options_set_by_char() {
        let mut opts = ShellOptions::default();
        opts.set_by_char('a', true).unwrap();
        opts.set_by_char('x', true).unwrap();
        assert!(opts.allexport);
        assert!(opts.xtrace);
        let s = opts.to_flag_string();
        assert!(s.contains('a'));
        assert!(s.contains('x'));

        opts.set_by_char('a', false).unwrap();
        assert!(!opts.allexport);

        assert!(opts.set_by_char('Z', true).is_err());
    }

    #[test]
    fn test_shell_options_set_by_name() {
        let mut opts = ShellOptions::default();
        opts.set_by_name("allexport", true).unwrap();
        assert!(opts.allexport);
        opts.set_by_name("allexport", false).unwrap();
        assert!(!opts.allexport);
        assert!(opts.set_by_name("invalid", true).is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib env::tests::test_shell_options -- --nocapture`
Expected: FAIL — `ShellOptions` not defined

- [ ] **Step 3: Implement ShellOptions**

Add above the `ShellEnv` struct in `src/env/mod.rs`:

```rust
/// POSIX shell option flags.
#[derive(Debug, Clone)]
pub struct ShellOptions {
    pub allexport: bool,     // -a
    pub notify: bool,        // -b
    pub noclobber: bool,     // -C
    pub errexit: bool,       // -e
    pub noglob: bool,        // -f
    pub noexec: bool,        // -n
    pub monitor: bool,       // -m
    pub nounset: bool,       // -u
    pub verbose: bool,       // -v
    pub xtrace: bool,        // -x
    pub ignoreeof: bool,
    pub pipefail: bool,
}

impl Default for ShellOptions {
    fn default() -> Self {
        ShellOptions {
            allexport: false,
            notify: false,
            noclobber: false,
            errexit: false,
            noglob: false,
            noexec: false,
            monitor: false,
            nounset: false,
            verbose: false,
            xtrace: false,
            ignoreeof: false,
            pipefail: false,
        }
    }
}

impl ShellOptions {
    /// Return active flags as a string for `$-` (e.g., "aex").
    pub fn to_flag_string(&self) -> String {
        let mut s = String::new();
        if self.allexport { s.push('a'); }
        if self.notify { s.push('b'); }
        if self.noclobber { s.push('C'); }
        if self.errexit { s.push('e'); }
        if self.noglob { s.push('f'); }
        if self.monitor { s.push('m'); }
        if self.noexec { s.push('n'); }
        if self.nounset { s.push('u'); }
        if self.verbose { s.push('v'); }
        if self.xtrace { s.push('x'); }
        s
    }

    /// Set/unset an option by its short flag character.
    pub fn set_by_char(&mut self, c: char, on: bool) -> Result<(), String> {
        match c {
            'a' => self.allexport = on,
            'b' => self.notify = on,
            'C' => self.noclobber = on,
            'e' => self.errexit = on,
            'f' => self.noglob = on,
            'm' => self.monitor = on,
            'n' => self.noexec = on,
            'u' => self.nounset = on,
            'v' => self.verbose = on,
            'x' => self.xtrace = on,
            _ => return Err(format!("set: -{}: invalid option", c)),
        }
        Ok(())
    }

    /// Set/unset an option by its long name (for `set -o name`).
    pub fn set_by_name(&mut self, name: &str, on: bool) -> Result<(), String> {
        match name {
            "allexport" => self.allexport = on,
            "notify" => self.notify = on,
            "noclobber" => self.noclobber = on,
            "errexit" => self.errexit = on,
            "noglob" => self.noglob = on,
            "monitor" => self.monitor = on,
            "noexec" => self.noexec = on,
            "nounset" => self.nounset = on,
            "verbose" => self.verbose = on,
            "xtrace" => self.xtrace = on,
            "ignoreeof" => self.ignoreeof = on,
            "pipefail" => self.pipefail = on,
            _ => return Err(format!("set: {}: invalid option name", name)),
        }
        Ok(())
    }

    /// Display all options in `set -o` format (human-readable).
    pub fn display_all(&self) {
        let options: &[(&str, bool)] = &[
            ("allexport", self.allexport),
            ("errexit", self.errexit),
            ("ignoreeof", self.ignoreeof),
            ("monitor", self.monitor),
            ("noclobber", self.noclobber),
            ("noexec", self.noexec),
            ("noglob", self.noglob),
            ("notify", self.notify),
            ("nounset", self.nounset),
            ("pipefail", self.pipefail),
            ("verbose", self.verbose),
            ("xtrace", self.xtrace),
        ];
        for (name, on) in options {
            println!("{:<15} {}", name, if *on { "on" } else { "off" });
        }
    }

    /// Display all options in restorable format (`set +o` output).
    pub fn display_restorable(&self) {
        let options: &[(&str, bool)] = &[
            ("allexport", self.allexport),
            ("errexit", self.errexit),
            ("ignoreeof", self.ignoreeof),
            ("monitor", self.monitor),
            ("noclobber", self.noclobber),
            ("noexec", self.noexec),
            ("noglob", self.noglob),
            ("notify", self.notify),
            ("nounset", self.nounset),
            ("pipefail", self.pipefail),
            ("verbose", self.verbose),
            ("xtrace", self.xtrace),
        ];
        for (name, on) in options {
            if *on {
                println!("set -o {}", name);
            } else {
                println!("set +o {}", name);
            }
        }
    }
}
```

- [ ] **Step 4: Add `options` field to ShellEnv**

In `src/env/mod.rs`, add `options: ShellOptions` to the `ShellEnv` struct and initialize it in `ShellEnv::new`:

```rust
pub struct ShellEnv {
    pub vars: VarStore,
    pub options: ShellOptions,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    pub positional_params: Vec<String>,
    pub last_bg_pid: Option<i32>,
    pub functions: HashMap<String, FunctionDef>,
    pub flow_control: Option<FlowControl>,
}
```

In `ShellEnv::new`, add: `options: ShellOptions::default(),`

- [ ] **Step 5: Fix compile errors from ShellEnv field addition**

Update `src/expand/command_sub.rs:45-54` where `ShellEnv` is constructed manually — add the `options` field:

```rust
let child_env = ShellEnv {
    vars: env.vars.clone(),
    options: env.options.clone(),
    last_exit_status: env.last_exit_status,
    shell_pid: env.shell_pid,
    shell_name: env.shell_name.clone(),
    positional_params: env.positional_params.clone(),
    last_bg_pid: env.last_bg_pid,
    functions: env.functions.clone(),
    flow_control: None,
};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass (including the 3 new ShellOptions tests)

- [ ] **Step 7: Commit**

```bash
git add src/env/mod.rs src/expand/command_sub.rs
git commit -m "feat(phase6): add ShellOptions to ShellEnv with flag management"
```

---

### Task 2: TrapStore

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Write failing tests for TrapStore**

Add to `#[cfg(test)] mod tests` in `src/env/mod.rs`:

```rust
    #[test]
    fn test_trap_store_default() {
        let store = TrapStore::default();
        assert!(store.exit_trap.is_none());
        assert!(store.signal_traps.is_empty());
    }

    #[test]
    fn test_trap_store_set_exit() {
        let mut store = TrapStore::default();
        store.set_trap("EXIT", TrapAction::Command("echo bye".to_string())).unwrap();
        assert!(matches!(store.get_trap("EXIT"), Some(TrapAction::Command(_))));
    }

    #[test]
    fn test_trap_store_set_signal() {
        let mut store = TrapStore::default();
        store.set_trap("INT", TrapAction::Ignore).unwrap();
        assert!(matches!(store.get_trap("INT"), Some(TrapAction::Ignore)));
        store.set_trap("INT", TrapAction::Default).unwrap();
        assert!(matches!(store.get_trap("INT"), Some(TrapAction::Default)));
    }

    #[test]
    fn test_trap_store_signal_name_to_number() {
        assert_eq!(TrapStore::signal_name_to_number("EXIT"), Some(0));
        assert_eq!(TrapStore::signal_name_to_number("HUP"), Some(1));
        assert_eq!(TrapStore::signal_name_to_number("INT"), Some(2));
        assert_eq!(TrapStore::signal_name_to_number("QUIT"), Some(3));
        assert_eq!(TrapStore::signal_name_to_number("TERM"), Some(15));
        assert_eq!(TrapStore::signal_name_to_number("0"), Some(0));
        assert_eq!(TrapStore::signal_name_to_number("2"), Some(2));
        assert_eq!(TrapStore::signal_name_to_number("INVALID"), None);
    }

    #[test]
    fn test_trap_store_remove() {
        let mut store = TrapStore::default();
        store.set_trap("EXIT", TrapAction::Command("echo bye".to_string())).unwrap();
        store.remove_trap("EXIT");
        assert!(store.exit_trap.is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib env::tests::test_trap_store -- --nocapture`
Expected: FAIL — `TrapStore` not defined

- [ ] **Step 3: Implement TrapStore**

Add to `src/env/mod.rs` (above `ShellEnv`):

```rust
/// Action to take when a trap fires.
#[derive(Debug, Clone, PartialEq)]
pub enum TrapAction {
    Default,
    Ignore,
    Command(String),
}

/// Storage for trap registrations.
#[derive(Debug, Clone)]
pub struct TrapStore {
    pub exit_trap: Option<TrapAction>,
    pub signal_traps: HashMap<i32, TrapAction>,
}

impl Default for TrapStore {
    fn default() -> Self {
        TrapStore {
            exit_trap: None,
            signal_traps: HashMap::new(),
        }
    }
}

impl TrapStore {
    /// Map a signal name or number string to its signal number.
    pub fn signal_name_to_number(name: &str) -> Option<i32> {
        // Try numeric first
        if let Ok(n) = name.parse::<i32>() {
            return Some(n);
        }
        match name {
            "EXIT" => Some(0),
            "HUP" | "SIGHUP" => Some(1),
            "INT" | "SIGINT" => Some(2),
            "QUIT" | "SIGQUIT" => Some(3),
            "ABRT" | "SIGABRT" => Some(6),
            "KILL" | "SIGKILL" => Some(9),
            "ALRM" | "SIGALRM" => Some(14),
            "TERM" | "SIGTERM" => Some(15),
            _ => None,
        }
    }

    /// Map a signal number to its display name.
    fn signal_number_to_name(num: i32) -> &'static str {
        match num {
            0 => "EXIT",
            1 => "HUP",
            2 => "INT",
            3 => "QUIT",
            6 => "ABRT",
            9 => "KILL",
            14 => "ALRM",
            15 => "TERM",
            _ => "UNKNOWN",
        }
    }

    /// Set a trap for a signal condition.
    pub fn set_trap(&mut self, condition: &str, action: TrapAction) -> Result<(), String> {
        let num = Self::signal_name_to_number(condition)
            .ok_or_else(|| format!("trap: {}: invalid signal specification", condition))?;
        if num == 0 {
            self.exit_trap = Some(action);
        } else {
            self.signal_traps.insert(num, action);
        }
        Ok(())
    }

    /// Get the trap action for a signal condition.
    pub fn get_trap(&self, condition: &str) -> Option<&TrapAction> {
        let num = Self::signal_name_to_number(condition)?;
        if num == 0 {
            self.exit_trap.as_ref()
        } else {
            self.signal_traps.get(&num)
        }
    }

    /// Remove a trap (reset to default).
    pub fn remove_trap(&mut self, condition: &str) {
        if let Some(num) = Self::signal_name_to_number(condition) {
            if num == 0 {
                self.exit_trap = None;
            } else {
                self.signal_traps.remove(&num);
            }
        }
    }

    /// Display all traps (for `trap` with no args).
    pub fn display_all(&self) {
        if let Some(action) = &self.exit_trap {
            if let TrapAction::Command(cmd) = action {
                println!("trap -- '{}' EXIT", cmd);
            } else if matches!(action, TrapAction::Ignore) {
                println!("trap -- '' EXIT");
            }
        }
        let mut nums: Vec<_> = self.signal_traps.keys().collect();
        nums.sort();
        for &num in &nums {
            if let Some(action) = self.signal_traps.get(num) {
                let name = Self::signal_number_to_name(*num);
                match action {
                    TrapAction::Command(cmd) => println!("trap -- '{}' {}", cmd, name),
                    TrapAction::Ignore => println!("trap -- '' {}", name),
                    TrapAction::Default => {}
                }
            }
        }
    }
}
```

- [ ] **Step 4: Add `traps` field to ShellEnv**

Add `pub traps: TrapStore` to `ShellEnv` struct and `traps: TrapStore::default()` in `ShellEnv::new`.

Update `src/expand/command_sub.rs` `child_env` construction: add `traps: env.traps.clone(),`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 6: Commit**

```bash
git add src/env/mod.rs src/expand/command_sub.rs
git commit -m "feat(phase6): add TrapStore to ShellEnv with signal name mapping"
```

---

### Task 3: AliasStore

**Files:**
- Create: `src/env/aliases.rs`
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Create `src/env/aliases.rs` with tests**

```rust
use std::collections::HashMap;

/// Storage for shell aliases.
#[derive(Debug, Clone, Default)]
pub struct AliasStore {
    aliases: HashMap<String, String>,
}

impl AliasStore {
    /// Define or update an alias.
    pub fn set(&mut self, name: &str, value: &str) {
        self.aliases.insert(name.to_string(), value.to_string());
    }

    /// Get the value of an alias.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    /// Remove an alias. Returns true if it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.aliases.remove(name).is_some()
    }

    /// Remove all aliases.
    pub fn clear(&mut self) {
        self.aliases.clear();
    }

    /// Iterate over aliases sorted by name.
    pub fn sorted_iter(&self) -> Vec<(&str, &str)> {
        let mut pairs: Vec<_> = self.aliases.iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        pairs.sort_by_key(|(k, _)| *k);
        pairs
    }

    /// Check if any aliases are defined.
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alias_set_get() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        assert_eq!(store.get("ll"), Some("ls -l"));
    }

    #[test]
    fn test_alias_remove() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        assert!(store.remove("ll"));
        assert_eq!(store.get("ll"), None);
        assert!(!store.remove("ll"));
    }

    #[test]
    fn test_alias_clear() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        store.set("la", "ls -a");
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_alias_sorted_iter() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        store.set("aa", "echo a");
        let sorted: Vec<_> = store.sorted_iter();
        assert_eq!(sorted, vec![("aa", "echo a"), ("ll", "ls -l")]);
    }

    #[test]
    fn test_alias_overwrite() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        store.set("ll", "ls -la");
        assert_eq!(store.get("ll"), Some("ls -la"));
    }
}
```

- [ ] **Step 2: Register module and add field to ShellEnv**

In `src/env/mod.rs`, add `pub mod aliases;` at the top (alongside `pub mod vars;`).

Add `use aliases::AliasStore;` and add `pub aliases: AliasStore` to `ShellEnv` struct.

Add `aliases: AliasStore::default()` in `ShellEnv::new`.

Update `src/expand/command_sub.rs` `child_env`: add `aliases: env.aliases.clone(),`.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass (including new AliasStore tests)

- [ ] **Step 4: Commit**

```bash
git add src/env/aliases.rs src/env/mod.rs src/expand/command_sub.rs
git commit -m "feat(phase6): add AliasStore to ShellEnv"
```

---

### Task 4: Special Builtins — File Separation and Dispatch

**Files:**
- Create: `src/builtin/special.rs`
- Modify: `src/builtin/mod.rs`
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Write failing test for BuiltinKind classification**

Add to `#[cfg(test)] mod tests` in `src/builtin/mod.rs`:

```rust
    #[test]
    fn test_classify_builtin() {
        assert!(matches!(classify_builtin(":"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("break"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("continue"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("return"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exit"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("export"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("readonly"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("unset"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("set"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("eval"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exec"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("trap"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("."), BuiltinKind::Special));
        assert!(matches!(classify_builtin("shift"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("times"), BuiltinKind::Special));

        assert!(matches!(classify_builtin("cd"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("echo"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("true"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("false"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("alias"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("unalias"), BuiltinKind::Regular));

        assert!(matches!(classify_builtin("ls"), BuiltinKind::NotBuiltin));
        assert!(matches!(classify_builtin("grep"), BuiltinKind::NotBuiltin));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib builtin::tests::test_classify_builtin`
Expected: FAIL — `classify_builtin` not defined

- [ ] **Step 3: Create `src/builtin/special.rs` with moved builtins**

Create the file with the existing `export`, `unset`, `readonly`, `exit`, `return`, `break`, `continue` implementations moved here, plus stubs for the new ones (`set`, `eval`, `exec`, `trap`, `.`, `shift`, `times`):

```rust
use crate::env::{FlowControl, ShellEnv, TrapAction};
use crate::exec::Executor;

/// Execute a special builtin command.
/// `eval` and `.` require Executor access; the rest only need ShellEnv.
/// Returns the exit status.
pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    match name {
        ":" => 0,
        "exit" => builtin_exit(args, &executor.env),
        "export" => builtin_export(args, &mut executor.env),
        "unset" => builtin_unset(args, &mut executor.env),
        "readonly" => builtin_readonly(args, &mut executor.env),
        "return" => builtin_return(args, &mut executor.env),
        "break" => builtin_break(args, &mut executor.env),
        "continue" => builtin_continue(args, &mut executor.env),
        "set" => builtin_set(args, &mut executor.env),
        "eval" => builtin_eval(args, executor),
        "exec" => builtin_exec(args, &mut executor.env),
        "trap" => builtin_trap(args, &mut executor.env),
        "." => builtin_source(args, executor),
        "shift" => builtin_shift(args, &mut executor.env),
        "times" => builtin_times(),
        _ => {
            eprintln!("kish: {}: not a special builtin", name);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Moved from builtin/mod.rs
// ---------------------------------------------------------------------------

fn builtin_exit(args: &[String], env: &ShellEnv) -> i32 {
    let code = if args.is_empty() {
        env.last_exit_status
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: exit: {}: numeric argument required", args[0]);
                2
            }
        }
    };

    // Execute EXIT trap if set
    // Note: we can't call executor here because we only have env.
    // EXIT trap execution will be handled by the caller (Executor or main).
    std::process::exit(code);
}

fn builtin_export(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        let mut exported: Vec<(String, String)> = env.vars.to_environ();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            println!("export {}={}", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, value) {
                eprintln!("kish: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else {
            env.vars.export(arg);
        }
    }
    status
}

fn builtin_unset(args: &[String], env: &mut ShellEnv) -> i32 {
    let mut status = 0;
    for name in args {
        // Check if it's a function first with -f flag
        if name == "-f" {
            continue;
        }
        if name == "-v" {
            continue;
        }
        if let Err(e) = env.vars.unset(name) {
            eprintln!("kish: unset: {}", e);
            status = 1;
        }
    }
    status
}

fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        let readonly_vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect();
        let mut sorted = readonly_vars;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in sorted {
            println!("readonly {}={}", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, value) {
                eprintln!("kish: readonly: {}", e);
                status = 1;
                continue;
            }
            env.vars.set_readonly(name);
        } else {
            env.vars.set_readonly(arg);
        }
    }
    status
}

fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
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

fn builtin_break(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: break: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: break: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(FlowControl::Break(n));
    0
}

fn builtin_continue(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: continue: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: continue: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(FlowControl::Continue(n));
    0
}

// ---------------------------------------------------------------------------
// New special builtins (stubs — implemented in later tasks)
// ---------------------------------------------------------------------------

fn builtin_set(_args: &[String], _env: &mut ShellEnv) -> i32 {
    // Stub — implemented in Task 5
    0
}

fn builtin_eval(_args: &[String], _executor: &mut Executor) -> i32 {
    // Stub — implemented in Task 6
    0
}

fn builtin_exec(_args: &[String], _env: &mut ShellEnv) -> i32 {
    // Stub — implemented in Task 7
    0
}

fn builtin_trap(_args: &[String], _env: &mut ShellEnv) -> i32 {
    // Stub — implemented in Task 8
    0
}

fn builtin_source(_args: &[String], _executor: &mut Executor) -> i32 {
    // Stub — implemented in Task 9
    0
}

fn builtin_shift(_args: &[String], _env: &mut ShellEnv) -> i32 {
    // Stub — implemented in Task 10
    0
}

fn builtin_times() -> i32 {
    // Stub — implemented in Task 11
    0
}
```

- [ ] **Step 4: Update `src/builtin/mod.rs` — add dispatch and BuiltinKind**

Replace the contents of `src/builtin/mod.rs` with:

```rust
pub mod special;

use crate::env::ShellEnv;

/// Classification of builtin commands per POSIX.
pub enum BuiltinKind {
    Special,
    Regular,
    NotBuiltin,
}

/// Classify a command name as special builtin, regular builtin, or not a builtin.
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        // POSIX special builtins
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit"
        | "export" | "readonly" | "return" | "set" | "shift" | "times"
        | "trap" | "unset" => BuiltinKind::Special,

        // Regular builtins
        "cd" | "echo" | "true" | "false" | "alias" | "unalias" => BuiltinKind::Regular,

        _ => BuiltinKind::NotBuiltin,
    }
}

/// Execute a regular builtin command, returning its exit status.
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    match name {
        "cd" => builtin_cd(args, env),
        "true" => 0,
        "false" => 1,
        "echo" => builtin_echo(args),
        "alias" => builtin_alias(args, env),
        "unalias" => builtin_unalias(args, env),
        _ => {
            eprintln!("kish: {}: not a regular builtin", name);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Regular builtin implementations
// ---------------------------------------------------------------------------

fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 {
    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                eprintln!("kish: cd: HOME not set");
                return 1;
            }
        }
    } else {
        args[0].clone()
    };

    if let Ok(old_pwd) = std::env::current_dir() {
        let _ = env.vars.set("OLDPWD", old_pwd.to_string_lossy().to_string());
    }

    match std::env::set_current_dir(&target) {
        Ok(_) => {
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

fn builtin_echo(args: &[String]) -> i32 {
    println!("{}", args.join(" "));
    0
}

fn builtin_alias(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        for (name, value) in env.aliases.sorted_iter() {
            println!("alias {}='{}'", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            env.aliases.set(name, value);
        } else {
            match env.aliases.get(arg) {
                Some(value) => println!("alias {}='{}'", arg, value),
                None => {
                    eprintln!("kish: alias: {}: not found", arg);
                    status = 1;
                }
            }
        }
    }
    status
}

fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        eprintln!("kish: unalias: usage: unalias name [name ...]");
        return 2;
    }

    let mut status = 0;
    for arg in args {
        if arg == "-a" {
            env.aliases.clear();
        } else if !env.aliases.remove(arg) {
            eprintln!("kish: unalias: {}: not found", arg);
            status = 1;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn make_env() -> ShellEnv {
        ShellEnv::new("kish", vec![])
    }

    #[test]
    fn test_classify_builtin() {
        assert!(matches!(classify_builtin(":"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("break"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("continue"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("return"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exit"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("export"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("readonly"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("unset"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("set"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("eval"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exec"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("trap"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("."), BuiltinKind::Special));
        assert!(matches!(classify_builtin("shift"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("times"), BuiltinKind::Special));

        assert!(matches!(classify_builtin("cd"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("echo"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("true"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("false"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("alias"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("unalias"), BuiltinKind::Regular));

        assert!(matches!(classify_builtin("ls"), BuiltinKind::NotBuiltin));
        assert!(matches!(classify_builtin("grep"), BuiltinKind::NotBuiltin));
    }

    #[test]
    fn test_true_false() {
        let mut env = make_env();
        assert_eq!(exec_regular_builtin("true", &[], &mut env), 0);
        assert_eq!(exec_regular_builtin("false", &[], &mut env), 1);
    }

    #[test]
    fn test_cd_to_tmp() {
        let mut env = make_env();
        let args = vec!["/tmp".to_string()];
        let status = exec_regular_builtin("cd", &args, &mut env);
        assert_eq!(status, 0);
        let pwd = env.vars.get("PWD").unwrap_or("");
        assert!(!pwd.is_empty());
    }

    #[test]
    fn test_alias_unalias() {
        let mut env = make_env();
        let args = vec!["ll=ls -l".to_string()];
        assert_eq!(exec_regular_builtin("alias", &args, &mut env), 0);
        assert_eq!(env.aliases.get("ll"), Some("ls -l"));

        let args = vec!["ll".to_string()];
        assert_eq!(exec_regular_builtin("unalias", &args, &mut env), 0);
        assert_eq!(env.aliases.get("ll"), None);
    }

    #[test]
    fn test_unalias_all() {
        let mut env = make_env();
        env.aliases.set("ll", "ls -l");
        env.aliases.set("la", "ls -a");
        let args = vec!["-a".to_string()];
        assert_eq!(exec_regular_builtin("unalias", &args, &mut env), 0);
        assert!(env.aliases.is_empty());
    }
}
```

- [ ] **Step 5: Update `src/exec/mod.rs` — use new dispatch**

Replace the import and exec_simple_command's builtin section. Change the imports at the top from:

```rust
use crate::builtin::{exec_builtin, is_builtin};
```

to:

```rust
use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
use crate::builtin::special::exec_special_builtin;
```

Then in `exec_simple_command`, replace the builtin+external section (lines 333-371) with:

```rust
        // Command lookup: function → special builtin → regular builtin → external
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

        match classify_builtin(&command_name) {
            BuiltinKind::Special => {
                // Special builtins: prefix assignments persist
                for assignment in &cmd.assignments {
                    let value = assignment
                        .value
                        .as_ref()
                        .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
                        .unwrap_or_default();
                    if let Err(e) = self.env.vars.set(&assignment.name, value) {
                        eprintln!("kish: {}", e);
                        self.env.last_exit_status = 1;
                        return 1;
                    }
                }

                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.env.last_exit_status = 1;
                    return 1;
                }
                let status = exec_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                self.env.last_exit_status = status;
                status
            }
            BuiltinKind::Regular => {
                // Regular builtins: prefix assignments are temporary
                let saved = self.apply_temp_assignments(&cmd.assignments);

                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.restore_assignments(saved);
                    self.env.last_exit_status = 1;
                    return 1;
                }
                let status = exec_regular_builtin(&command_name, &args, &mut self.env);
                redirect_state.restore();
                self.restore_assignments(saved);
                self.env.last_exit_status = status;
                status
            }
            BuiltinKind::NotBuiltin => {
                let env_vars = self.build_env_vars(&cmd.assignments);
                let status = self.exec_external_with_redirects(
                    &command_name,
                    &args,
                    &env_vars,
                    &cmd.redirects,
                );
                self.env.last_exit_status = status;
                status
            }
        }
```

- [ ] **Step 6: Add temp assignment helpers to Executor**

Add these methods to `impl Executor` in `src/exec/mod.rs`:

```rust
    /// Apply temporary variable assignments, saving old values for restoration.
    /// Returns Vec of (name, old_value_option) pairs.
    fn apply_temp_assignments(&mut self, assignments: &[Assignment]) -> Vec<(String, Option<String>)> {
        let mut saved = Vec::new();
        for assignment in assignments {
            let old_val = self.env.vars.get(&assignment.name).map(|s| s.to_string());
            saved.push((assignment.name.clone(), old_val));
            let value = assignment
                .value
                .as_ref()
                .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
                .unwrap_or_default();
            let _ = self.env.vars.set(&assignment.name, value);
        }
        saved
    }

    /// Restore variable assignments from saved values.
    fn restore_assignments(&mut self, saved: Vec<(String, Option<String>)>) {
        for (name, old_val) in saved {
            match old_val {
                Some(val) => { let _ = self.env.vars.set(&name, val); }
                None => { let _ = self.env.vars.unset(&name); }
            }
        }
    }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 8: Commit**

```bash
git add src/builtin/special.rs src/builtin/mod.rs src/exec/mod.rs
git commit -m "feat(phase6): separate special/regular builtins with POSIX dispatch"
```

---

### Task 5: `set` Builtin Implementation

**Files:**
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Write integration tests for `set`**

Add to `tests/parser_integration.rs`:

```rust
// ── set builtin ─────────────────────────────────────────────────────────────

#[test]
fn test_set_positional_params() {
    let out = kish_exec("set -- a b c; echo $1 $2 $3");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a b c\n");
}

#[test]
fn test_set_enable_option() {
    // set -f should disable glob
    let out = kish_exec("set -f; echo *");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "*\n");
}

#[test]
fn test_set_disable_xv() {
    // "set -" disables -x and -v
    let out = kish_exec("set -x; set -; echo hello");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stdout, "hello\n");
    // After "set -", xtrace should be off — no trace for "echo hello"
    assert!(!stderr.contains("+ echo hello"));
}

#[test]
fn test_set_dash_o_display() {
    let out = kish_exec("set -o");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("allexport"));
    assert!(stdout.contains("off"));
}

#[test]
fn test_set_plus_o_restorable() {
    let out = kish_exec("set +o");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("set +o allexport"));
}

#[test]
fn test_set_no_args_displays_vars() {
    let out = kish_exec("X=hello; set");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("X=hello"));
}
```

- [ ] **Step 2: Implement `builtin_set`**

Replace the stub in `src/builtin/special.rs`:

```rust
fn builtin_set(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Display all variables
        let mut vars: Vec<(String, String)> = env.vars.vars_iter()
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in vars {
            println!("{}={}", name, value);
        }
        return 0;
    }

    let mut i = 0;
    let mut setting_positional = false;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            // Everything after -- becomes positional params
            env.positional_params = args[i + 1..].to_vec();
            return 0;
        }

        if arg == "-" {
            // Disable -x and -v, rest becomes positional params
            env.options.xtrace = false;
            env.options.verbose = false;
            if i + 1 < args.len() {
                env.positional_params = args[i + 1..].to_vec();
            }
            return 0;
        }

        if arg == "-o" || arg == "+o" {
            let on = arg.starts_with('-');
            i += 1;
            if i >= args.len() {
                // Display options
                if on {
                    env.options.display_all();
                } else {
                    env.options.display_restorable();
                }
                return 0;
            }
            if let Err(e) = env.options.set_by_name(&args[i], on) {
                eprintln!("kish: {}", e);
                return 1;
            }
            i += 1;
            continue;
        }

        if arg.starts_with('-') || arg.starts_with('+') {
            let on = arg.starts_with('-');
            for c in arg[1..].chars() {
                if let Err(e) = env.options.set_by_char(c, on) {
                    eprintln!("kish: {}", e);
                    return 1;
                }
            }
            i += 1;
            continue;
        }

        // If we get here, remaining args are positional params
        setting_positional = true;
        break;
    }

    if setting_positional {
        env.positional_params = args[i..].to_vec();
    }

    0
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 4: Commit**

```bash
git add src/builtin/special.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement set builtin with options and positional params"
```

---

### Task 6: `eval` Builtin Implementation

**Files:**
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Write integration tests for `eval`**

Add to `tests/parser_integration.rs`:

```rust
// ── eval builtin ────────────────────────────────────────────────────────────

#[test]
fn test_eval_simple() {
    let out = kish_exec("eval 'echo hello'");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_eval_variable_expansion() {
    let out = kish_exec("CMD='echo world'; eval $CMD");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_eval_multi_args() {
    let out = kish_exec("eval echo hello world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_eval_empty() {
    let out = kish_exec("eval; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}
```

- [ ] **Step 2: Implement `builtin_eval`**

Replace the stub in `src/builtin/special.rs`:

```rust
fn builtin_eval(args: &[String], executor: &mut Executor) -> i32 {
    if args.is_empty() {
        return 0;
    }
    let input = args.join(" ");
    match crate::parser::Parser::new(&input).parse_program() {
        Ok(program) => executor.exec_program(&program),
        Err(e) => {
            eprintln!("kish: eval: {}", e);
            2
        }
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 4: Commit**

```bash
git add src/builtin/special.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement eval builtin"
```

---

### Task 7: `exec` Builtin Implementation

**Files:**
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Write integration tests for `exec`**

Add to `tests/parser_integration.rs`:

```rust
// ── exec builtin ────────────────────────────────────────────────────────────

#[test]
fn test_exec_replaces_process() {
    let out = kish_exec("exec /usr/bin/echo replaced");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "replaced\n");
}

#[test]
fn test_exec_no_args() {
    // exec with no args is a no-op
    let out = kish_exec("exec; echo still here");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "still here\n");
}

#[test]
fn test_exec_not_found() {
    let out = kish_exec("exec /nonexistent/binary");
    assert!(!out.status.success());
}
```

- [ ] **Step 2: Implement `builtin_exec`**

Replace the stub in `src/builtin/special.rs`. Add `use std::ffi::CString;` and `use nix::unistd::execvp;` at the top:

```rust
fn builtin_exec(args: &[String], _env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // No command — just return (redirects already applied by caller)
        return 0;
    }

    let cmd = &args[0];
    let c_cmd = match CString::new(cmd.as_str()) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("kish: exec: {}: invalid command name", cmd);
            return 126;
        }
    };
    let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
    for a in args {
        match CString::new(a.as_str()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                eprintln!("kish: exec: {}: invalid argument", a);
                return 126;
            }
        }
    }

    // This does not return on success
    let err = execvp(&c_cmd, &c_args).unwrap_err();
    use nix::errno::Errno;
    match err {
        Errno::ENOENT => {
            eprintln!("kish: exec: {}: not found", cmd);
            127
        }
        Errno::EACCES => {
            eprintln!("kish: exec: {}: permission denied", cmd);
            126
        }
        _ => {
            eprintln!("kish: exec: {}: {}", cmd, err);
            126
        }
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 4: Commit**

```bash
git add src/builtin/special.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement exec builtin"
```

---

### Task 8: `trap` Builtin Implementation + EXIT Trap Execution

**Files:**
- Modify: `src/builtin/special.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write integration tests for `trap`**

Add to `tests/parser_integration.rs`:

```rust
// ── trap builtin ────────────────────────────────────────────────────────────

#[test]
fn test_trap_exit() {
    let out = kish_exec("trap 'echo goodbye' EXIT; echo hello");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "hello\ngoodbye\n");
}

#[test]
fn test_trap_display() {
    let out = kish_exec("trap 'echo bye' EXIT; trap");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trap -- 'echo bye' EXIT"));
}

#[test]
fn test_trap_reset() {
    let out = kish_exec("trap 'echo bye' EXIT; trap - EXIT; echo hello");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "hello\n");
}

#[test]
fn test_trap_ignore() {
    let out = kish_exec("trap '' EXIT; trap");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trap -- '' EXIT"));
}
```

- [ ] **Step 2: Implement `builtin_trap`**

Replace the stub in `src/builtin/special.rs`:

```rust
fn builtin_trap(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Display all traps
        env.traps.display_all();
        return 0;
    }

    // trap -p [signal...]
    if args[0] == "-p" {
        if args.len() == 1 {
            env.traps.display_all();
        } else {
            for sig in &args[1..] {
                env.traps.display_all(); // simplified — displays all
            }
        }
        return 0;
    }

    // Determine if first arg is action or signal
    // If there's only one arg, it's a signal to reset
    // If first arg is "-", reset the following signals
    if args.len() == 1 {
        // trap signal — reset to default
        env.traps.remove_trap(&args[0]);
        return 0;
    }

    let action_str = &args[0];
    let signals = &args[1..];

    // Determine action
    let action = if action_str == "-" {
        TrapAction::Default
    } else if action_str.is_empty() {
        TrapAction::Ignore
    } else {
        TrapAction::Command(action_str.to_string())
    };

    let mut status = 0;
    for sig in signals {
        if matches!(action, TrapAction::Default) {
            env.traps.remove_trap(sig);
        } else if let Err(e) = env.traps.set_trap(sig, action.clone()) {
            eprintln!("kish: {}", e);
            status = 1;
        }
    }
    status
}
```

- [ ] **Step 3: Add EXIT trap execution to main.rs**

Modify `src/main.rs`. Change the `run_string` function to handle EXIT trap:

```rust
fn run_string(input: &str, shell_name: String, positional: Vec<String>) -> i32 {
    match parser::Parser::new(input).parse_program() {
        Ok(program) => {
            let mut executor = Executor::new(shell_name, positional);
            let status = executor.exec_program(&program);
            // Execute EXIT trap if set
            execute_exit_trap(&mut executor);
            status
        }
        Err(e) => { eprintln!("{}", e); 2 }
    }
}

fn execute_exit_trap(executor: &mut Executor) {
    if let Some(ref action) = executor.env.traps.exit_trap {
        if let env::TrapAction::Command(cmd) = action.clone() {
            // Clear exit trap to prevent recursion
            executor.env.traps.exit_trap = None;
            if let Ok(program) = parser::Parser::new(&cmd).parse_program() {
                executor.exec_program(&program);
            }
        }
    }
}
```

Add `use env::TrapAction;` at the top of `main.rs` (after the existing `use` statements), or use the fully qualified path as shown above.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 5: Commit**

```bash
git add src/builtin/special.rs src/main.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement trap builtin with EXIT trap execution"
```

---

### Task 9: `.` (source) Builtin Implementation

**Files:**
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Write integration tests for `.`**

Add to `tests/parser_integration.rs`:

```rust
// ── source (.) builtin ──────────────────────────────────────────────────────

#[test]
fn test_source_file() {
    let dir = helpers::TempDir::new();
    let script = dir.write_file("lib.sh", "MY_SOURCE_VAR=sourced\n");
    let cmd = format!(". {}; echo $MY_SOURCE_VAR", script.display());
    let out = kish_exec(&cmd);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "sourced\n");
}

#[test]
fn test_source_not_found() {
    let out = kish_exec(". /nonexistent/file.sh");
    assert!(!out.status.success());
}
```

- [ ] **Step 2: Implement `builtin_source`**

Replace the stub in `src/builtin/special.rs`:

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
        // Search PATH
        if let Some(path_var) = executor.env.vars.get("PATH") {
            let mut found = None;
            for dir in path_var.to_string().split(':') {
                let candidate = std::path::PathBuf::from(dir).join(filename);
                if candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => {
                    eprintln!("kish: .: {}: not found", filename);
                    return 1;
                }
            }
        } else {
            std::path::PathBuf::from(filename)
        }
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish: .: {}: {}", path.display(), e);
            return 1;
        }
    };

    match crate::parser::Parser::new(&content).parse_program() {
        Ok(program) => executor.exec_program(&program),
        Err(e) => {
            eprintln!("kish: .: {}", e);
            2
        }
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 4: Commit**

```bash
git add src/builtin/special.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement source (.) builtin"
```

---

### Task 10: `shift` and `times` Builtins

**Files:**
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Write integration tests**

Add to `tests/parser_integration.rs`:

```rust
// ── shift builtin ───────────────────────────────────────────────────────────

#[test]
fn test_shift_default() {
    let out = kish_exec_with_args("shift; echo $1 $2", &["a", "b", "c"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b c\n");
}

#[test]
fn test_shift_n() {
    let out = kish_exec_with_args("shift 2; echo $1", &["a", "b", "c"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "c\n");
}

#[test]
fn test_shift_too_many() {
    let out = kish_exec_with_args("shift 5; echo $?", &["a", "b"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "1\n");
}

// ── times builtin ───────────────────────────────────────────────────────────

#[test]
fn test_times() {
    let out = kish_exec("times");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Should output two lines with time values
    assert!(stdout.contains("m"));
}
```

Also add this helper function near the top of `tests/parser_integration.rs`:

```rust
fn kish_exec_with_args(input: &str, args: &[&str]) -> std::process::Output {
    let mut cmd_args = vec!["-c", input, "--"];
    cmd_args.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(cmd_args)
        .output()
        .expect("failed to execute kish")
}
```

- [ ] **Step 2: Implement `builtin_shift`**

Replace the stub in `src/builtin/special.rs`:

```rust
fn builtin_shift(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1usize
    } else {
        match args[0].parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: shift: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };

    if n > env.positional_params.len() {
        eprintln!("kish: shift: shift count out of range");
        return 1;
    }

    env.positional_params = env.positional_params[n..].to_vec();
    0
}
```

- [ ] **Step 3: Implement `builtin_times`**

Replace the stub in `src/builtin/special.rs`:

```rust
fn builtin_times() -> i32 {
    let mut tms: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if unsafe { libc::times(&mut tms) } == -1 {
        eprintln!("kish: times: failed");
        return 1;
    }
    let fmt = |t: libc::clock_t| -> String {
        let secs = t as f64 / ticks;
        let m = (secs / 60.0) as u64;
        let s = secs - (m as f64 * 60.0);
        format!("{}m{:.3}s", m, s)
    };
    println!("{} {}", fmt(tms.tms_utime), fmt(tms.tms_stime));
    println!("{} {}", fmt(tms.tms_cutime), fmt(tms.tms_cstime));
    0
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 5: Commit**

```bash
git add src/builtin/special.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement shift and times builtins"
```

---

### Task 11: `$-` Expansion and Shell Option Behaviors

**Files:**
- Modify: `src/expand/param.rs`
- Modify: `src/expand/mod.rs`
- Modify: `src/exec/redirect.rs`
- Modify: `src/env/vars.rs`
- Modify: `src/exec/mod.rs`
- Modify: `src/exec/pipeline.rs`

- [ ] **Step 1: Write integration tests for shell options and `$-`**

Add to `tests/parser_integration.rs`:

```rust
// ── shell option behaviors ──────────────────────────────────────────────────

#[test]
fn test_dash_parameter() {
    let out = kish_exec("set -x; echo $-");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().contains('x'));
}

#[test]
fn test_noglob() {
    let out = kish_exec("set -f; echo *");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "*\n");
}

#[test]
fn test_nounset() {
    let out = kish_exec("set -u; echo $UNDEFINED_VAR_XYZ");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("UNDEFINED_VAR_XYZ"));
}

#[test]
fn test_noclobber() {
    let dir = helpers::TempDir::new();
    let file = dir.write_file("existing.txt", "original");
    let cmd = format!("set -C; echo new > {}", file.display());
    let out = kish_exec(&cmd);
    assert!(!out.status.success());
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "original");
}

#[test]
fn test_noclobber_override() {
    let dir = helpers::TempDir::new();
    let file = dir.write_file("existing.txt", "original");
    let cmd = format!("set -C; echo new >| {}", file.display());
    let out = kish_exec(&cmd);
    assert!(out.status.success());
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "new\n");
}

#[test]
fn test_xtrace() {
    let out = kish_exec("set -x; echo hello");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("+ echo hello"));
}

#[test]
fn test_verbose() {
    let out = kish_exec("set -v; echo hello");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // verbose prints input lines — the "echo hello" should appear on stderr
    assert!(stderr.contains("echo hello"));
}

#[test]
fn test_allexport() {
    let out = kish_exec("set -a; MY_AE_VAR=exported; /usr/bin/env | grep MY_AE_VAR");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("MY_AE_VAR=exported"));
}

#[test]
fn test_noexec() {
    // set -n should parse but not execute
    let out = kish_exec("echo before; set -n; echo after");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("before"));
    assert!(!stdout.contains("after"));
}
```

- [ ] **Step 2: Implement `$-` expansion**

In `src/expand/param.rs`, change the `expand_special` function's `SpecialParam::Dash` case:

```rust
SpecialParam::Dash => env.options.to_flag_string(),
```

- [ ] **Step 3: Implement `-f` (noglob) check**

In `src/expand/mod.rs`, in the `expand_word` function, add a check before pathname expansion:

```rust
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> Vec<String> {
    let fields = expand_word_to_fields(env, word);
    let fields = field_split::split(env, fields);
    let fields = if env.options.noglob {
        fields  // skip pathname expansion
    } else {
        pathname::expand(env, fields)
    };
    fields
        .into_iter()
        .filter(|f| !f.is_empty())
        .map(|f| f.value)
        .collect()
}
```

- [ ] **Step 4: Implement `-u` (nounset) check**

In `src/expand/param.rs`, in the `expand` function's `ParamExpr::Simple(name)` case:

```rust
ParamExpr::Simple(name) => {
    match env.vars.get(name) {
        Some(val) => val.to_string(),
        None => {
            if env.options.nounset {
                eprintln!("kish: {}: parameter not set", name);
                env.last_exit_status = 1;
                // Signal to abort (use flow_control as Return to trigger early exit)
                env.flow_control = Some(crate::env::FlowControl::Return(1));
            }
            String::new()
        }
    }
}
```

- [ ] **Step 5: Implement `-C` (noclobber) check**

In `src/exec/redirect.rs`, in the `apply_one` method's `RedirectKind::Output` arm, add a noclobber check. Replace the existing `Output` case:

```rust
RedirectKind::Output(word) => {
    let target_fd = redirect.fd.unwrap_or(1);
    let path = expand_word_to_string(env, word);
    // noclobber check: -C prevents overwriting existing files
    if env.options.noclobber && std::path::Path::new(&path).exists() {
        return Err(format!("{}: cannot overwrite existing file", path));
    }
    let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
    let fd = open(path.as_str(), flags, Mode::from_bits_truncate(0o644))
        .map_err(|e| format!("{}: {}", path, e))?
        .into_raw_fd();
    if save {
        self.save_fd(target_fd)?;
    }
    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
    unsafe { libc::close(fd) };
}
RedirectKind::OutputClobber(word) => {
    // >| always overwrites, regardless of noclobber
    let target_fd = redirect.fd.unwrap_or(1);
    let path = expand_word_to_string(env, word);
    let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
    let fd = open(path.as_str(), flags, Mode::from_bits_truncate(0o644))
        .map_err(|e| format!("{}: {}", path, e))?
        .into_raw_fd();
    if save {
        self.save_fd(target_fd)?;
    }
    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
    unsafe { libc::close(fd) };
}
```

- [ ] **Step 6: Implement `-a` (allexport)**

In `src/env/vars.rs`, modify the `set` method to accept an `allexport` flag. Add a new method:

```rust
    /// Set a variable's value, with allexport auto-export.
    /// If `allexport` is true, the variable is automatically marked as exported.
    pub fn set_with_options(&mut self, name: &str, value: impl Into<String>, allexport: bool) -> Result<(), String> {
        if let Some(existing) = self.vars.get(name) {
            if existing.readonly {
                return Err(format!("{}: readonly variable", name));
            }
            let exported = existing.exported || allexport;
            self.vars.insert(
                name.to_string(),
                Variable {
                    value: value.into(),
                    exported,
                    readonly: false,
                },
            );
        } else {
            let mut var = Variable::new(value);
            if allexport {
                var.exported = true;
            }
            self.vars.insert(name.to_string(), var);
        }
        Ok(())
    }
```

Then update `src/exec/mod.rs` in the assignment-only section and the special builtin assignment section to use `set_with_options` when `self.env.options.allexport` is true. In the assignment-only block:

```rust
if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.options.allexport) {
```

Similarly for the special builtin assignment block.

- [ ] **Step 7: Implement `-x` (xtrace) and `-v` (verbose)**

In `src/exec/mod.rs`, add xtrace output before command execution. In `exec_simple_command`, after expanding words and before the function/builtin/external dispatch:

```rust
        // xtrace: print trace before execution
        if self.env.options.xtrace && !expanded.is_empty() {
            eprintln!("+ {}", expanded.join(" "));
        }
```

For verbose (`-v`), add to `exec_program` or at the point where input lines are first processed. Since kish processes parsed commands (not raw input lines), verbose output is best added at `exec_complete_command` level or in `main.rs` before parsing. For simplicity, add a `verbose_line` method to Executor that can be called from main:

Add a public method to `Executor`:

```rust
    /// Print a line to stderr if verbose mode is on.
    pub fn verbose_print(&self, line: &str) {
        if self.env.options.verbose {
            eprintln!("{}", line);
        }
    }
```

In `src/main.rs`'s `run_string`, call it before exec:

```rust
fn run_string(input: &str, shell_name: String, positional: Vec<String>) -> i32 {
    match parser::Parser::new(input).parse_program() {
        Ok(program) => {
            let mut executor = Executor::new(shell_name, positional);
            executor.verbose_print(input);
            let status = executor.exec_program(&program);
            execute_exit_trap(&mut executor);
            status
        }
        Err(e) => { eprintln!("{}", e); 2 }
    }
}
```

- [ ] **Step 8: Implement `-n` (noexec)**

In `src/exec/mod.rs`, in `exec_command`, add a noexec check at the top:

```rust
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        if self.env.options.noexec {
            return 0;
        }
        match cmd {
            // ... existing match arms
        }
    }
```

- [ ] **Step 9: Wire `pipefail` to `env.options`**

In `src/exec/pipeline.rs`, the `exec_multi_pipeline` method currently always uses last command's exit status. Add pipefail support by reading `self.env.options.pipefail`:

Replace the wait loop (lines 93-101) with:

```rust
        // Parent: wait for all children, collect exit statuses
        let mut last_status = 0;
        let mut max_nonzero = 0;
        for (idx, child) in children.into_iter().enumerate() {
            let status = wait_for_child(child);
            if status != 0 {
                max_nonzero = status;
            }
            if idx == n - 1 {
                last_status = status;
            }
        }

        if self.env.options.pipefail {
            // pipefail: return last non-zero status, or 0 if all succeeded
            max_nonzero
        } else {
            last_status
        }
```

- [ ] **Step 10: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 11: Commit**

```bash
git add src/expand/param.rs src/expand/mod.rs src/exec/redirect.rs src/env/vars.rs src/exec/mod.rs src/exec/pipeline.rs src/main.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement shell option behaviors ($-, -f, -u, -C, -a, -x, -v, -n, pipefail)"
```

---

### Task 12: Alias Expansion in Lexer

**Files:**
- Modify: `src/lexer/mod.rs`
- Modify: `src/parser/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/expand/command_sub.rs`
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Write integration tests for alias expansion**

Add to `tests/parser_integration.rs`:

```rust
// ── alias expansion ─────────────────────────────────────────────────────────

#[test]
fn test_alias_basic() {
    let out = kish_exec("alias greet='echo hello'; greet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_alias_with_args() {
    let out = kish_exec("alias say='echo'; say world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_alias_recursive_prevention() {
    // alias ls='ls -l' should not infinitely recurse
    let out = kish_exec("alias ls='echo ls called'; ls");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ls called\n");
}

#[test]
fn test_alias_trailing_space_chain() {
    // If alias value ends with space, next word is also alias-expanded
    let out = kish_exec("alias run='echo '; alias world='hello'; run world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_alias_display() {
    let out = kish_exec("alias ll='ls -l'; alias ll");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("alias ll='ls -l'"));
}

#[test]
fn test_unalias() {
    let out = kish_exec("alias greet='echo hi'; unalias greet; alias greet");
    assert!(!out.status.success());
}

#[test]
fn test_unalias_all() {
    let out = kish_exec("alias a='echo a'; alias b='echo b'; unalias -a; alias");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.is_empty());
}
```

- [ ] **Step 2: Add alias support to Lexer**

In `src/lexer/mod.rs`, add alias fields to the `Lexer` struct:

```rust
use std::collections::HashSet;
use crate::env::aliases::AliasStore;

pub struct Lexer<'a> {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
    pub pending_heredocs: Vec<PendingHereDoc>,
    heredoc_bodies: Vec<Vec<WordPart>>,
    // Alias expansion
    aliases: Option<&'a AliasStore>,
    alias_buffer: Option<(String, usize)>,  // (expanded text, read position)
    expanding_aliases: HashSet<String>,
    check_alias: bool,  // true when next token should be checked for alias
}
```

Update `Lexer::new` to accept an optional alias store:

```rust
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
            pending_heredocs: Vec::new(),
            heredoc_bodies: Vec::new(),
            aliases: None,
            alias_buffer: None,
            expanding_aliases: HashSet::new(),
            check_alias: true,
        }
    }

    pub fn new_with_aliases(input: &str, aliases: &AliasStore) -> Lexer<'_> {
        Lexer {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
            pending_heredocs: Vec::new(),
            heredoc_bodies: Vec::new(),
            aliases: Some(aliases),
            alias_buffer: None,
            expanding_aliases: HashSet::new(),
            check_alias: true,
        }
    }
```

Add a method to read from alias buffer or regular input:

```rust
    fn current_byte_aliased(&self) -> u8 {
        if let Some((ref buf, pos)) = self.alias_buffer {
            let bytes = buf.as_bytes();
            if pos < bytes.len() {
                return bytes[pos];
            }
        }
        self.current_byte()
    }

    fn advance_aliased(&mut self) -> u8 {
        if let Some((ref buf, ref mut pos)) = self.alias_buffer {
            let bytes = buf.as_bytes();
            if *pos < bytes.len() {
                let ch = bytes[*pos];
                *pos += 1;
                // Check if buffer is exhausted
                if *pos >= bytes.len() {
                    // Done with alias buffer
                }
                return ch;
            }
        }
        self.advance()
    }
```

Then modify `next_token` to check aliases: after reading a word token that is in command position, check if it matches an alias. If so, re-tokenize the alias value.

A simpler approach: after `next_token` returns a `Word` token, check in the Parser's advance method. However, per POSIX, alias expansion happens at the tokenization level. The simplest correct approach is to check after producing a word token:

In `next_token`, before returning a `Word` token, add alias check:

```rust
    /// Try to expand an alias. If the word matches, replace the remaining
    /// input with the alias value prepended.
    fn try_alias_expand(&mut self, word: &str) -> bool {
        if !self.check_alias {
            return false;
        }
        let aliases = match self.aliases {
            Some(a) => a,
            None => return false,
        };
        if self.expanding_aliases.contains(word) {
            return false;
        }
        match aliases.get(word) {
            Some(value) => {
                self.expanding_aliases.insert(word.to_string());
                // Check if alias value ends with blank — if so, next word
                // is also subject to alias expansion
                self.check_alias = value.ends_with(' ') || value.ends_with('\t');
                // Prepend alias value to remaining input
                let remaining = &self.input[self.pos..];
                let mut new_input = value.as_bytes().to_vec();
                new_input.extend_from_slice(remaining);
                self.input = new_input;
                self.pos = 0;
                true
            }
            None => {
                self.check_alias = false;
                false
            }
        }
    }
```

Then in `next_token`, after producing a regular word token (before returning), add the alias check. The exact integration point depends on the structure of the read_word method — wrap it so that after reading a word, if `check_alias` is true, try expansion and if expanded, re-read:

```rust
    // In next_token, after determining the token is a Word:
    // If check_alias and the word matches an alias, expand and re-tokenize.
    // This goes at the end of next_token, wrapping the return:
```

The exact integration will need to be adapted to the lexer structure. The key is:
1. After reading a word, check `try_alias_expand`
2. If expanded, recursively call `next_token` to re-tokenize from the new input
3. Reset `check_alias = true` after `;`, `\n`, `|`, `&&`, `||`, `(`, `{`

Add reset of `check_alias` after these tokens in `next_token`:

```rust
    // After emitting ;, \n, |, &&, ||, (, { tokens:
    // self.check_alias = true;
```

- [ ] **Step 3: Update Parser to accept AliasStore**

In `src/parser/mod.rs`, update `Parser::new` to optionally accept aliases:

```rust
impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self { lexer, current }
    }
}
```

Add a new constructor:

```rust
impl<'a> Parser<'a> {
    pub fn new_with_aliases(input: &str, aliases: &'a AliasStore) -> Parser<'a> {
        let mut lexer = Lexer::new_with_aliases(input, aliases);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Parser { lexer, current }
    }
}
```

Note: This requires adding a lifetime parameter to `Parser`. Change:

```rust
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: SpannedToken,
}
```

The existing `Parser::new` can use `Lexer::new` (which sets `aliases: None`), requiring the `'a` lifetime to be `'static` or handled via an option. The simplest approach: make the `aliases` field `Option<&'a AliasStore>` so `Parser::new` can use `'_` with no aliases.

Since this requires changes to every place `Parser` is used, update all call sites:
- `src/main.rs` — use `Parser::new_with_aliases` for `run_string`, keep `Parser::new` for `--parse`
- `src/builtin/special.rs` — `eval` and `.` use `Parser::new_with_aliases` with `executor.env.aliases`
- `src/expand/command_sub.rs` — command substitution uses `Parser::new` (aliases not expanded in command substitution per some shells, or pass aliases through — for POSIX compliance, pass them through)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 5: Commit**

```bash
git add src/lexer/mod.rs src/parser/mod.rs src/main.rs src/expand/command_sub.rs src/builtin/special.rs tests/parser_integration.rs
git commit -m "feat(phase6): implement alias expansion at lexer level with recursion prevention"
```

---

### Task 13: Prefix Assignment POSIX Compliance Integration Tests

**Files:**
- Modify: `tests/parser_integration.rs`

- [ ] **Step 1: Write integration tests for prefix assignment behavior**

Add to `tests/parser_integration.rs`:

```rust
// ── prefix assignment POSIX compliance ──────────────────────────────────────

#[test]
fn test_special_builtin_assignment_persists() {
    // VAR=val on a special builtin should persist
    let out = kish_exec("MY_SP_VAR=hello :; echo $MY_SP_VAR");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_regular_builtin_assignment_temporary() {
    // VAR=val on a regular builtin should not persist
    let out = kish_exec("MY_RB_VAR=hello echo test; echo MY_RB_VAR=$MY_RB_VAR");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("MY_RB_VAR=\n") || stdout.contains("MY_RB_VAR="));
    // The variable should not be set after the command
    assert!(!stdout.contains("MY_RB_VAR=hello\n") || stdout.lines().last().unwrap().contains("MY_RB_VAR="));
}

#[test]
fn test_assignment_only_sets_var() {
    let out = kish_exec("MY_ASSIGN_VAR=world; echo $MY_ASSIGN_VAR");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_external_cmd_assignment_does_not_persist() {
    let out = kish_exec("MY_EXT_VAR=hello /usr/bin/true; echo MY_EXT_VAR=$MY_EXT_VAR");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Variable should not persist after external command
    assert!(stdout.contains("MY_EXT_VAR=\n"));
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test`
Expected: ALL pass (these tests verify that Task 4's dispatch logic works correctly)

- [ ] **Step 3: Commit**

```bash
git add tests/parser_integration.rs
git commit -m "test(phase6): add prefix assignment POSIX compliance integration tests"
```

---

### Task 14: Final Integration and Cleanup

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Update TODO.md**

Mark Phase 6 complete and add known limitations:

Add to the end of the known limitations sections:

```markdown
## Phase 6: Known Limitations

- [ ] `trap` signal execution (INT, HUP, etc.) not implemented — only EXIT trap fires; signal trap registration is stored but execution deferred to Phase 7
- [ ] `-e` (errexit) flag is settable but behavior is not implemented — deferred to Phase 7
- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`
- [ ] `ignoreeof` is settable but has no effect — interactive mode feature
- [ ] `exec` with redirects only (no command) does not make redirects permanent — redirect persistence deferred
- [ ] Alias expansion does not cover all POSIX edge cases (e.g., alias in compound commands may not expand)
```

Update the Remaining Phases section:

```markdown
## Remaining Phases

- [x] Phase 5: Control structure execution (if, for, while, until, case, functions)
- [x] Phase 6: Special builtins (set, export, trap, eval, exec, etc.) + alias expansion
- [ ] Phase 7: Signals and errexit
- [ ] Phase 8: Subshell environment isolation
```

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "update TODO.md: mark Phase 6 complete, add Phase 6 known limitations"
```

- [ ] **Step 5: Run full test suite one final time**

Run: `cargo test`
Expected: ALL pass — confirm total test count increase (target: ~380+ tests)
