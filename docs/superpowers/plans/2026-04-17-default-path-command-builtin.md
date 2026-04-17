# Default PATH and `command` Builtin — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add POSIX-compliant default PATH handling (`libc::confstr(_CS_PATH)` at startup when PATH is unset) and implement the full POSIX `command` builtin (`-p`/`-v`/`-V` plus function-skip execution).

**Architecture:** Fish-style runtime `confstr(_CS_PATH)` cached in `ShellEnv::default_path_cache: OnceLock<String>`. A new `command` builtin lives in `src/builtin/command.rs`, handled as a special case in `src/exec/simple.rs` (like `wait`/`fg`/`bg`) so it has Executor access for execution paths. A shared `resolve_command_kind()` helper in `src/builtin/resolve.rs` is reusable for future `type`/`which` builtins.

**Tech Stack:** Rust 2024 edition, `libc` 0.2 (already a dependency), `std::sync::OnceLock`, `nix` 0.31 (for `execvp`), existing yosh builtin/exec architecture.

**Spec:** `docs/superpowers/specs/2026-04-17-default-path-command-builtin-design.md`

---

## File Structure

### New files
- `src/env/default_path.rs` — `default_path()`, `call_confstr()`, `fallback_default_path()`
- `src/builtin/command.rs` — `cmd_command()` implementation (flag parse + dispatch)
- `src/builtin/resolve.rs` — `CommandKind` enum + `resolve_command_kind()`
- `e2e/builtin_command/command_v_finds_external.sh` — E2E test
- `e2e/builtin_command/command_v_builtin.sh` — E2E test
- `e2e/builtin_command/command_v_alias.sh` — E2E test
- `e2e/builtin_command/command_V_external.sh` — E2E test
- `e2e/builtin_command/command_V_not_found.sh` — E2E test
- `e2e/builtin_command/command_p_when_path_unset.sh` — E2E test
- `e2e/builtin_command/command_skips_function.sh` — E2E test

### Modified files
- `src/env/mod.rs` — add `default_path_cache` field + `pub mod default_path`
- `src/main.rs` — add `ensure_default_path()` to `run_string()`, `run_file()` (indirect via `run_string`), and `Repl::new()`
- `src/interactive/mod.rs` — call `ensure_default_path()` after `Executor::new()` in `Repl::new()`
- `src/builtin/mod.rs` — register `command` in `BUILTIN_NAMES` + `classify_builtin()`; special-case dispatch stays in `exec/simple.rs`
- `src/exec/simple.rs` — special-case `command` like `wait`/`fg`/`bg` so it gets `&mut Executor`; add `skip_function_lookup` transient flag for no-flag form
- `src/exec/mod.rs` — add `skip_function_lookup: bool` field to `Executor` (consumed on next `exec_simple_command`)
- `src/exec/command.rs` — remove `#[allow(dead_code)]` from `find_in_path`

---

## Task 1: Add `fallback_default_path()` pure function

**Files:**
- Create: `src/env/default_path.rs`
- Modify: `src/env/mod.rs:1-6` (add `pub mod default_path;`)

- [ ] **Step 1.1: Create module file with failing test**

Create `src/env/default_path.rs`:

```rust
//! POSIX default PATH discovery via `confstr(_CS_PATH)`.

/// Hardcoded fallback PATH used when `confstr(_CS_PATH)` is unavailable or fails.
///
/// Chosen to be minimal and work on any POSIX-like system without depending
/// on `/usr/local/bin` (absent on many minimal Linux containers) or `.`
/// (classic security foot-gun).
pub fn fallback_default_path() -> String {
    "/bin:/usr/bin".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_bin_usr_bin() {
        assert_eq!(fallback_default_path(), "/bin:/usr/bin");
    }

    #[test]
    fn fallback_does_not_contain_cwd_or_empty() {
        let p = fallback_default_path();
        assert!(!p.split(':').any(|d| d == "." || d.is_empty()));
    }
}
```

- [ ] **Step 1.2: Wire into the `env` module**

Edit `src/env/mod.rs`. Change lines 1-6 from:

```rust
pub mod aliases;
pub mod exec_state;
pub mod jobs;
pub mod shell_mode;
pub mod traps;
pub mod vars;
```

to:

```rust
pub mod aliases;
pub mod default_path;
pub mod exec_state;
pub mod jobs;
pub mod shell_mode;
pub mod traps;
pub mod vars;
```

- [ ] **Step 1.3: Run the unit tests**

Run: `cargo test --lib env::default_path -- --nocapture`
Expected: both tests pass (2 passed).

- [ ] **Step 1.4: Commit**

```bash
git add src/env/default_path.rs src/env/mod.rs
git commit -m "$(cat <<'EOF'
feat(env): add fallback_default_path helper

First slice of POSIX default-PATH support: a pure, test-friendly fallback
used when libc::confstr(_CS_PATH) is unavailable. Subsequent commits will
add the confstr wrapper and the ShellEnv cache.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `call_confstr()` unsafe wrapper

**Files:**
- Modify: `src/env/default_path.rs` (append)

- [ ] **Step 2.1: Add the wrapper with a structural test**

Append to `src/env/default_path.rs` (keep the existing `fallback_default_path` and its tests; add to the file):

```rust
use std::ptr;

/// Call `libc::confstr(_CS_PATH, ...)` to retrieve the POSIX-recommended
/// default PATH. Returns `None` if `confstr` is unsupported on this OS,
/// returns 0, or produces invalid UTF-8.
///
/// This is a thin unsafe FFI wrapper — the unsafety is limited to the two
/// libc calls. The returned String is safe to pass around freely.
pub fn call_confstr() -> Option<String> {
    // Step 1: query required buffer size (NUL included).
    // Safety: passing null_mut + 0 is explicitly allowed by POSIX for size
    // queries. No memory is written.
    let needed = unsafe { libc::confstr(libc::_CS_PATH, ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }

    // Step 2: allocate and fill the buffer.
    let mut buf = vec![0u8; needed];
    // Safety: buf is exactly `needed` bytes long, matching the size confstr
    // asked for on the previous call. confstr writes up to `needed` bytes
    // including NUL.
    let written = unsafe { libc::confstr(libc::_CS_PATH, buf.as_mut_ptr().cast(), needed) };
    if written == 0 || written > needed {
        return None;
    }

    // Drop the trailing NUL.
    buf.truncate(written.saturating_sub(1));
    String::from_utf8(buf).ok()
}
```

Append these tests inside the existing `#[cfg(test)] mod tests { ... }` block (right before the closing `}`):

```rust
    #[test]
    fn call_confstr_returns_something_usable() {
        // macOS and Linux both implement _CS_PATH; failure here would mean
        // the OS is genuinely non-POSIX (CI sanity check).
        let p = call_confstr().expect("confstr(_CS_PATH) should succeed on POSIX systems");
        assert!(!p.is_empty());
        // Must contain at least one of /bin or /usr/bin: true on both macOS
        // and Linux default values, without asserting the exact string.
        assert!(
            p.split(':').any(|d| d == "/bin" || d == "/usr/bin"),
            "expected /bin or /usr/bin in confstr PATH, got: {p}"
        );
    }

    #[test]
    fn call_confstr_has_no_cwd_or_empty_entries() {
        // POSIX _CS_PATH never includes "." or empty segments.
        let p = call_confstr().expect("confstr should succeed");
        assert!(!p.split(':').any(|d| d == "." || d.is_empty()));
    }
```

- [ ] **Step 2.2: Run the tests**

Run: `cargo test --lib env::default_path`
Expected: 4 tests pass.

- [ ] **Step 2.3: Commit**

```bash
git add src/env/default_path.rs
git commit -m "$(cat <<'EOF'
feat(env): add call_confstr wrapper for POSIX _CS_PATH

Wraps libc::confstr(_CS_PATH, ...) behind a safe API that returns
Option<String>. Tests assert structural properties (non-empty, contains
/bin or /usr/bin, no CWD/empty entries) rather than the exact OS-specific
value, so they pass on both macOS and Linux.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add `default_path_cache` field to `ShellEnv`

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 3.1: Add `OnceLock` field + initializer**

Edit `src/env/mod.rs`. Change the import block (lines 8-19) from:

```rust
use std::collections::HashMap;

use nix::unistd::{Pid, getpid};
use jobs::JobTable;
use aliases::AliasStore;
use vars::VarStore;
pub use exec_state::{ExecState, FlowControl};
pub use shell_mode::{ShellMode, ShellOptions};
pub use traps::{TrapAction, TrapStore};

use crate::interactive::history::History;
use crate::parser::ast::FunctionDef;
```

to:

```rust
use std::collections::HashMap;
use std::sync::OnceLock;

use nix::unistd::{Pid, getpid};
use jobs::JobTable;
use aliases::AliasStore;
use vars::VarStore;
pub use exec_state::{ExecState, FlowControl};
pub use shell_mode::{ShellMode, ShellOptions};
pub use traps::{TrapAction, TrapStore};

use crate::interactive::history::History;
use crate::parser::ast::FunctionDef;
```

Change the `ShellEnv` struct (lines 30-41) from:

```rust
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub exec: ExecState,
    pub process: ProcessState,
    pub mode: ShellMode,
    pub functions: HashMap<String, FunctionDef>,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub history: History,
    pub shell_name: String,
}
```

to:

```rust
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub exec: ExecState,
    pub process: ProcessState,
    pub mode: ShellMode,
    pub functions: HashMap<String, FunctionDef>,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub history: History,
    pub shell_name: String,
    /// Cache of the POSIX default PATH (`confstr(_CS_PATH)`), computed
    /// lazily on first use. See `env::default_path::default_path()`.
    pub default_path_cache: OnceLock<String>,
}
```

Change the `ShellEnv::new` body (lines 50-72) to add the new field after `history`. Replace:

```rust
            shell_name: shell_name.into(),
            functions: HashMap::new(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            history: History::new(),
        }
    }
}
```

with:

```rust
            shell_name: shell_name.into(),
            functions: HashMap::new(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            history: History::new(),
            default_path_cache: OnceLock::new(),
        }
    }
}
```

- [ ] **Step 3.2: Run existing ShellEnv tests to ensure nothing broke**

Run: `cargo test --lib env::tests`
Expected: 3 tests pass (construction, jobs_table, shell_pgid).

- [ ] **Step 3.3: Run the full env module tests**

Run: `cargo test --lib env::`
Expected: all pass (previous 4 + the env:: suite).

- [ ] **Step 3.4: Commit**

```bash
git add src/env/mod.rs
git commit -m "$(cat <<'EOF'
feat(env): add default_path_cache field to ShellEnv

OnceLock<String> gives lazy, once-only initialization of the POSIX default
PATH. Next commits add the default_path() accessor and the startup hook.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add `default_path(env)` accessor

**Files:**
- Modify: `src/env/default_path.rs`

- [ ] **Step 4.1: Add the accessor + tests**

Append to `src/env/default_path.rs` (after the existing `call_confstr` function, before the `#[cfg(test)]` block):

```rust
use crate::env::ShellEnv;

/// Return the POSIX default PATH, cached per `ShellEnv`.
///
/// Computed once via `call_confstr()`; falls back to `fallback_default_path()`
/// if `confstr` fails. Never panics.
pub fn default_path(env: &ShellEnv) -> &str {
    env.default_path_cache
        .get_or_init(|| call_confstr().unwrap_or_else(fallback_default_path))
        .as_str()
}
```

Append tests inside the existing `#[cfg(test)] mod tests` block (before the closing `}`):

```rust
    use crate::env::ShellEnv;

    #[test]
    fn default_path_is_non_empty() {
        let env = ShellEnv::new("yosh", vec![]);
        assert!(!default_path(&env).is_empty());
    }

    #[test]
    fn default_path_contains_bin_or_usr_bin() {
        let env = ShellEnv::new("yosh", vec![]);
        let dp = default_path(&env);
        assert!(
            dp.split(':').any(|d| d == "/bin" || d == "/usr/bin"),
            "expected /bin or /usr/bin in default path, got: {dp}"
        );
    }

    #[test]
    fn default_path_finds_sh() {
        // /bin/sh is POSIX-mandatory on every conforming system (macOS + Linux).
        use crate::exec::command::find_in_path;
        let env = ShellEnv::new("yosh", vec![]);
        let dp = default_path(&env);
        assert!(find_in_path("sh", dp).is_some(), "expected to find sh in: {dp}");
    }

    #[test]
    fn default_path_is_cached() {
        // Two calls return the same slice — proves OnceLock caches.
        let env = ShellEnv::new("yosh", vec![]);
        let a = default_path(&env).as_ptr();
        let b = default_path(&env).as_ptr();
        assert_eq!(a, b, "default_path should return the same cached string");
    }
```

- [ ] **Step 4.2: Run the default_path tests**

Run: `cargo test --lib env::default_path`
Expected: 8 tests pass (4 existing + 4 new).

> Note: `find_in_path` is currently `#[allow(dead_code)]` but still accessible via the `pub` fn. We remove the `allow` attribute in Task 11.

- [ ] **Step 4.3: Commit**

```bash
git add src/env/default_path.rs
git commit -m "$(cat <<'EOF'
feat(env): add default_path(env) with OnceLock caching

One call to confstr per ShellEnv; subsequent lookups are a single atomic
load. Tests assert caching via pointer identity, plus structural properties
that hold on both macOS and Linux.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Hook `ensure_default_path()` into startup

**Files:**
- Create (helper function): in `src/env/default_path.rs`
- Modify: `src/main.rs:181` (add call in `run_string`)
- Modify: `src/interactive/mod.rs:42` (add call in `Repl::new`)

- [ ] **Step 5.1: Add `ensure_default_path()` helper**

Append to `src/env/default_path.rs` (right before the `#[cfg(test)]` block):

```rust
/// If `PATH` is not set on the environment, populate it with the POSIX
/// default (from `confstr(_CS_PATH)`) and mark it exported so children
/// inherit it. Called once at shell startup.
///
/// When `PATH` is already set (the common case), this is a single HashMap
/// lookup — the `confstr` call is skipped entirely.
pub fn ensure_default_path(env: &mut ShellEnv) {
    if env.vars.get("PATH").is_some() {
        return;
    }
    let dp = default_path(env).to_string();
    // set() never fails here because PATH is not readonly in a fresh env.
    let _ = env.vars.set("PATH", dp);
    env.vars.export("PATH");
}
```

Append this test inside the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn ensure_default_path_populates_when_unset() {
        let mut env = ShellEnv::new("yosh", vec![]);
        // Simulate env -i startup: remove any inherited PATH.
        let _ = env.vars.unset("PATH");
        assert!(env.vars.get("PATH").is_none());
        ensure_default_path(&mut env);
        let pv = env.vars.get("PATH").expect("PATH should be set now");
        assert!(!pv.is_empty());
        let v = env.vars.get_var("PATH").expect("variable exists");
        assert!(v.exported, "PATH should be exported so children inherit it");
    }

    #[test]
    fn ensure_default_path_preserves_existing() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", "/custom/path");
        ensure_default_path(&mut env);
        assert_eq!(env.vars.get("PATH"), Some("/custom/path"));
    }
```

- [ ] **Step 5.2: Run the new tests**

Run: `cargo test --lib env::default_path::tests::ensure_default_path`
Expected: 2 tests pass.

- [ ] **Step 5.3: Hook into `run_string`**

Edit `src/main.rs:181-186`. Change:

```rust
fn run_string(input: &str, shell_name: String, positional: Vec<String>, cmd_string: bool) -> i32 {
    signal::init_signal_handling();
    let mut executor = Executor::new(shell_name, positional);
    executor.load_plugins();
    executor.env.mode.options.cmd_string = cmd_string;
    executor.verbose_print(input);
```

to:

```rust
fn run_string(input: &str, shell_name: String, positional: Vec<String>, cmd_string: bool) -> i32 {
    signal::init_signal_handling();
    let mut executor = Executor::new(shell_name, positional);
    env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();
    executor.env.mode.options.cmd_string = cmd_string;
    executor.verbose_print(input);
```

- [ ] **Step 5.4: Hook into `Repl::new`**

Edit `src/interactive/mod.rs:40-48`. Change:

```rust
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        executor.env.mode.is_interactive = true;
        executor.env.mode.options.monitor = true;
        signal::init_job_control_signals();
        // Ensure shell has terminal
        crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();
```

to:

```rust
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        crate::env::default_path::ensure_default_path(&mut executor.env);
        executor.env.mode.is_interactive = true;
        executor.env.mode.options.monitor = true;
        signal::init_job_control_signals();
        // Ensure shell has terminal
        crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();
```

- [ ] **Step 5.5: Build and run full unit test suite**

Run: `cargo build && cargo test --lib`
Expected: clean build, all tests pass.

- [ ] **Step 5.6: Manual smoke test on the startup hook**

Run: `env -i cargo run --quiet -- -c 'echo $PATH'`
Expected: prints a non-empty PATH value that contains `/bin` or `/usr/bin`.

- [ ] **Step 5.7: Commit**

```bash
git add src/env/default_path.rs src/main.rs src/interactive/mod.rs
git commit -m "$(cat <<'EOF'
feat(main): populate PATH at startup when unset

If the environment lacks PATH, set it from confstr(_CS_PATH) and export it
so children inherit it. Matches bash/zsh/fish behavior. Common path (PATH
already set) pays a single HashMap lookup.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Create `CommandKind` enum and `resolve_command_kind()`

**Files:**
- Create: `src/builtin/resolve.rs`
- Modify: `src/builtin/mod.rs:1-3` (add `pub mod resolve;`)

- [ ] **Step 6.1: Create the module with failing tests**

Create `src/builtin/resolve.rs`:

```rust
//! Shared "what is this name?" resolver used by the `command -v` / `-V`
//! builtin and (in the future) `type`.

use std::path::PathBuf;

use crate::builtin::{classify_builtin, BuiltinKind};
use crate::env::ShellEnv;
use crate::exec::command::find_in_path;

/// Classification of a command name against the current shell state.
#[derive(Debug, PartialEq, Eq)]
pub enum CommandKind {
    /// The name is an alias; payload is the alias value (the right-hand side).
    Alias(String),
    /// The name is a POSIX reserved word (e.g. `if`, `while`, `for`).
    Keyword,
    /// The name is a shell function defined in this session.
    Function,
    /// The name is a builtin command; payload distinguishes special vs regular.
    Builtin(BuiltinKind),
    /// The name resolves to an executable file on `PATH`.
    External(PathBuf),
    /// Nothing found.
    NotFound,
}

/// POSIX reserved words per IEEE Std 1003.1-2017 §2.4.
const RESERVED_WORDS: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi",
    "for", "if", "in", "then", "until", "while",
];

fn is_reserved_word(name: &str) -> bool {
    RESERVED_WORDS.contains(&name)
}

/// Walk yosh's name-resolution order and report what `name` would bind to.
///
/// Order (matches bash `command -V` reporting order):
///   1. alias
///   2. reserved word (keyword)
///   3. function
///   4. builtin (Special or Regular)
///   5. PATH search
pub fn resolve_command_kind(env: &ShellEnv, name: &str) -> CommandKind {
    if let Some(val) = env.aliases.get(name) {
        return CommandKind::Alias(val.to_string());
    }
    if is_reserved_word(name) {
        return CommandKind::Keyword;
    }
    if env.functions.contains_key(name) {
        return CommandKind::Function;
    }
    match classify_builtin(name) {
        BuiltinKind::NotBuiltin => {}
        kind => return CommandKind::Builtin(kind),
    }
    // External: search $PATH.
    if let Some(path_var) = env.vars.get("PATH") {
        if let Some(p) = find_in_path(name, path_var) {
            return CommandKind::External(p);
        }
    }
    CommandKind::NotFound
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn alias_wins_over_everything() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ls", "ls -G");
        // Even though "ls" also exists in PATH, alias takes precedence.
        assert_eq!(
            resolve_command_kind(&env, "ls"),
            CommandKind::Alias("ls -G".to_string())
        );
    }

    #[test]
    fn keyword_detected() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(resolve_command_kind(&env, "if"), CommandKind::Keyword);
        assert_eq!(resolve_command_kind(&env, "for"), CommandKind::Keyword);
        assert_eq!(resolve_command_kind(&env, "done"), CommandKind::Keyword);
    }

    #[test]
    fn function_wins_over_builtin() {
        // FunctionDef fields: { name: String, body: Rc<CompoundCommand>, redirects: Vec<Redirect> }
        // CompoundCommand is a struct wrapping CompoundCommandKind.
        // BraceGroup with an empty body is the minimal valid construction.
        use std::rc::Rc;
        use crate::parser::ast::{FunctionDef, CompoundCommand, CompoundCommandKind};
        let mut env = env_with_path("/bin:/usr/bin");
        env.functions.insert(
            "echo".to_string(),
            FunctionDef {
                name: "echo".to_string(),
                body: Rc::new(CompoundCommand {
                    kind: CompoundCommandKind::BraceGroup { body: Vec::new() },
                }),
                redirects: Vec::new(),
            },
        );
        assert_eq!(resolve_command_kind(&env, "echo"), CommandKind::Function);
    }

    #[test]
    fn special_builtin_detected() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&env, "export"),
            CommandKind::Builtin(BuiltinKind::Special)
        );
    }

    #[test]
    fn regular_builtin_detected() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&env, "cd"),
            CommandKind::Builtin(BuiltinKind::Regular)
        );
    }

    #[test]
    fn external_detected() {
        // /bin/sh is POSIX-mandatory on macOS + Linux.
        let env = env_with_path("/bin:/usr/bin");
        match resolve_command_kind(&env, "sh") {
            CommandKind::External(p) => {
                assert!(
                    p.ends_with("sh"),
                    "expected path ending in 'sh', got: {}",
                    p.display()
                );
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    #[test]
    fn not_found_for_unknown_name() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&env, "definitely_not_a_real_cmd_xyz"),
            CommandKind::NotFound
        );
    }
}
```

- [ ] **Step 6.2: Wire into `builtin` module**

Edit `src/builtin/mod.rs:1-2`. Change:

```rust
pub mod regular;
pub mod special;
```

to:

```rust
pub mod regular;
pub mod resolve;
pub mod special;
```

- [ ] **Step 6.3: Check that `FunctionDef` fields match**

Run: `grep -A5 'struct FunctionDef' src/parser/ast.rs | head -10` to verify field names used in the test (`name`, `body`).

If the struct uses different field names or types than the test assumes, adjust the test's `FunctionDef { ... }` literal accordingly. (If `body` is not `Box<Command>`, inspect the field and use the right value to construct a minimal valid instance.)

- [ ] **Step 6.4: Run the resolver tests**

Run: `cargo test --lib builtin::resolve`
Expected: 7 tests pass.

- [ ] **Step 6.5: Commit**

```bash
git add src/builtin/resolve.rs src/builtin/mod.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add resolve_command_kind helper

Single resolver used by 'command -v' / '-V' (and future 'type'/'which').
Reports alias → keyword → function → builtin → external in bash's reporting
order. Covered by unit tests; relies on /bin/sh being POSIX-mandatory for
the External case (holds on macOS + Linux).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Create `command` builtin skeleton with flag parsing

**Files:**
- Create: `src/builtin/command.rs`
- Modify: `src/builtin/mod.rs` (add `pub mod command;` + update `BUILTIN_NAMES`, `classify_builtin`)

- [ ] **Step 7.1: Create the skeleton**

Create `src/builtin/command.rs`:

```rust
//! POSIX `command` builtin.
//!
//! `command [-p] [-v|-V] command_name [argument...]`
//!
//! - `-p`  use the POSIX default PATH for lookup (from `confstr(_CS_PATH)`)
//! - `-v`  concise description of `command_name`
//! - `-V`  verbose description of `command_name`
//! - no flags: execute `command_name`, bypassing shell functions
//!
//! This file holds only the flag parser + description output paths. The
//! actual execution (for `-p` and no-flag forms) is dispatched from
//! `exec/simple.rs` so the `command` invocation has access to the
//! `Executor` for redirects/assignments.

/// Parsed form of a `command [...]` invocation.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandFlags {
    pub use_default_path: bool,
    pub verbose: Verbosity,
    pub name: String,
    pub rest: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verbosity {
    /// No `-v` / `-V` flag: this is an execute invocation.
    Execute,
    /// `-v`: concise description.
    Brief,
    /// `-V`: verbose description.
    Verbose,
}

/// Parse the argument list for `command`. Returns `Err(message)` on invalid
/// flags or missing command name. Messages are already formatted for stderr
/// (e.g., `"command: -x: invalid option"`).
pub fn parse_flags(args: &[String]) -> Result<CommandFlags, String> {
    let mut use_default_path = false;
    let mut verbose = Verbosity::Execute;
    let mut idx = 0;

    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        // Parse clustered flags: "-pv" = -p -v, "-Vp" = -V -p.
        for ch in a[1..].chars() {
            match ch {
                'p' => use_default_path = true,
                'v' => verbose = Verbosity::Brief,
                'V' => verbose = Verbosity::Verbose,
                other => return Err(format!("command: -{}: invalid option", other)),
            }
        }
        idx += 1;
    }

    if idx >= args.len() {
        return Err("command: missing command name".to_string());
    }

    let name = args[idx].clone();
    let rest = args[idx + 1..].to_vec();
    Ok(CommandFlags { use_default_path, verbose, name, rest })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn no_flags_execute() {
        let p = parse_flags(&v(&["ls", "-l"])).unwrap();
        assert!(!p.use_default_path);
        assert_eq!(p.verbose, Verbosity::Execute);
        assert_eq!(p.name, "ls");
        assert_eq!(p.rest, v(&["-l"]));
    }

    #[test]
    fn p_flag() {
        let p = parse_flags(&v(&["-p", "ls"])).unwrap();
        assert!(p.use_default_path);
        assert_eq!(p.name, "ls");
    }

    #[test]
    fn v_flag() {
        let p = parse_flags(&v(&["-v", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn big_v_flag() {
        let p = parse_flags(&v(&["-V", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Verbose);
    }

    #[test]
    fn combined_flags() {
        let p = parse_flags(&v(&["-pv", "ls"])).unwrap();
        assert!(p.use_default_path);
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn double_dash_stops_parsing() {
        let p = parse_flags(&v(&["--", "-v", "arg"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Execute);
        assert_eq!(p.name, "-v");
        assert_eq!(p.rest, v(&["arg"]));
    }

    #[test]
    fn single_dash_is_a_name() {
        let p = parse_flags(&v(&["-"])).unwrap();
        assert_eq!(p.name, "-");
    }

    #[test]
    fn invalid_option_errors() {
        let err = parse_flags(&v(&["-x", "ls"])).unwrap_err();
        assert!(err.contains("-x"));
    }

    #[test]
    fn missing_name_errors() {
        let err = parse_flags(&v(&[])).unwrap_err();
        assert!(err.to_lowercase().contains("missing"));

        let err = parse_flags(&v(&["-v"])).unwrap_err();
        assert!(err.to_lowercase().contains("missing"));
    }
}
```

- [ ] **Step 7.2: Wire the module and registry**

Edit `src/builtin/mod.rs`. Change the module declarations (lines 1-3) from:

```rust
pub mod regular;
pub mod resolve;
pub mod special;
```

to:

```rust
pub mod command;
pub mod regular;
pub mod resolve;
pub mod special;
```

Change `BUILTIN_NAMES` (lines 7-14) from:

```rust
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export",
    "readonly", "return", "set", "shift", "times", "trap", "unset", "fc",
    // Regular builtins
    "cd", "echo", "true", "false", "alias", "unalias", "kill", "wait",
    "fg", "bg", "jobs", "umask",
];
```

to:

```rust
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export",
    "readonly", "return", "set", "shift", "times", "trap", "unset", "fc",
    // Regular builtins
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill",
    "wait", "fg", "bg", "jobs", "umask",
];
```

Change `classify_builtin` (lines 29-40) from:

```rust
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export"
        | "readonly" | "return" | "set" | "shift" | "times" | "trap" | "unset"
        | "fc" => {
            BuiltinKind::Special
        }
        "cd" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}
```

to:

```rust
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export"
        | "readonly" | "return" | "set" | "shift" | "times" | "trap" | "unset"
        | "fc" => {
            BuiltinKind::Special
        }
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias"
        | "kill" | "wait" | "fg" | "bg" | "jobs" | "umask" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}
```

- [ ] **Step 7.3: Run the new tests**

Run: `cargo test --lib builtin::command`
Expected: 9 tests pass.

- [ ] **Step 7.4: Run the full builtin module tests**

Run: `cargo test --lib builtin::`
Expected: all pass, including `test_builtin_names_consistent_with_classify` (which walks `BUILTIN_NAMES`).

- [ ] **Step 7.5: Commit**

```bash
git add src/builtin/command.rs src/builtin/mod.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add command builtin skeleton + flag parser

Parses -p / -v / -V (including clustered forms like -pv) and --. Registers
'command' in BUILTIN_NAMES and classify_builtin as a regular builtin.
Execution paths are wired up in subsequent commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Implement `command -v` output

**Files:**
- Modify: `src/builtin/command.rs` (append)

- [ ] **Step 8.1: Add the `-v` formatter + tests**

Append to `src/builtin/command.rs` after `parse_flags`:

```rust
use crate::builtin::resolve::{resolve_command_kind, CommandKind};
use crate::builtin::BuiltinKind;
use crate::env::ShellEnv;

/// Render `-v` concise output. Returns `(stdout, exit_status)`.
/// When `name` is unknown, stdout is empty and exit is 1.
pub fn render_brief(env: &ShellEnv, name: &str) -> (String, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => (format!("alias {}='{}'", name, val), 0),
        CommandKind::Keyword => (name.to_string(), 0),
        CommandKind::Function => (name.to_string(), 0),
        CommandKind::Builtin(_) => (name.to_string(), 0),
        CommandKind::External(p) => (p.to_string_lossy().into_owned(), 0),
        CommandKind::NotFound => (String::new(), 1),
    }
}
```

Append tests inside the existing `#[cfg(test)] mod tests`:

```rust
    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn brief_alias() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, code) = render_brief(&env, "ll");
        assert_eq!(out, "alias ll='ls -l'");
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_keyword() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(render_brief(&env, "if"), ("if".to_string(), 0));
    }

    #[test]
    fn brief_builtin() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(render_brief(&env, "cd"), ("cd".to_string(), 0));
        assert_eq!(render_brief(&env, "export"), ("export".to_string(), 0));
    }

    #[test]
    fn brief_external() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, code) = render_brief(&env, "sh");
        assert!(out.ends_with("/sh"), "expected path ending in /sh, got: {out}");
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_not_found() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, code) = render_brief(&env, "definitely_not_a_real_cmd_xyz");
        assert_eq!(out, "");
        assert_eq!(code, 1);
    }
```

- [ ] **Step 8.2: Run the tests**

Run: `cargo test --lib builtin::command::tests::brief`
Expected: 5 tests pass.

- [ ] **Step 8.3: Commit**

```bash
git add src/builtin/command.rs
git commit -m "$(cat <<'EOF'
feat(builtin): implement command -v output

Formats one-line descriptions using resolve_command_kind, following the
bash/POSIX output convention: "alias X='Y'" for aliases, bare name for
keywords/functions/builtins, absolute path for externals, empty output
with exit 1 for unknown names.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Implement `command -V` output

**Files:**
- Modify: `src/builtin/command.rs` (append)

- [ ] **Step 9.1: Add the `-V` formatter + tests**

Append to `src/builtin/command.rs` (after `render_brief`):

```rust
/// Render `-V` verbose output. Returns `(stdout_or_empty, stderr_or_empty, exit_status)`.
/// For NotFound, stdout is empty and stderr holds the "not found" message.
pub fn render_verbose(env: &ShellEnv, name: &str) -> (String, String, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => (format!("{} is aliased to '{}'", name, val), String::new(), 0),
        CommandKind::Keyword => (format!("{} is a shell keyword", name), String::new(), 0),
        CommandKind::Function => (format!("{} is a function", name), String::new(), 0),
        CommandKind::Builtin(BuiltinKind::Special) => (
            format!("{} is a special shell builtin", name),
            String::new(),
            0,
        ),
        CommandKind::Builtin(BuiltinKind::Regular) => (
            format!("{} is a shell builtin", name),
            String::new(),
            0,
        ),
        CommandKind::Builtin(BuiltinKind::NotBuiltin) => {
            // Cannot happen — resolve_command_kind never returns this.
            (String::new(), format!("yosh: command: {}: not found", name), 1)
        }
        CommandKind::External(p) => (
            format!("{} is {}", name, p.to_string_lossy()),
            String::new(),
            0,
        ),
        CommandKind::NotFound => (
            String::new(),
            format!("yosh: command: {}: not found", name),
            1,
        ),
    }
}
```

Append tests:

```rust
    #[test]
    fn verbose_alias() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, err, code) = render_verbose(&env, "ll");
        assert_eq!(out, "ll is aliased to 'ls -l'");
        assert_eq!(err, "");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_keyword() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "if");
        assert_eq!(out, "if is a shell keyword");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_special_builtin() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "export");
        assert_eq!(out, "export is a special shell builtin");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_regular_builtin() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "cd");
        assert_eq!(out, "cd is a shell builtin");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_external() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "sh");
        assert!(out.starts_with("sh is "), "got: {out}");
        assert!(out.contains("/sh"), "got: {out}");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_not_found() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, err, code) = render_verbose(&env, "definitely_not_a_real_cmd_xyz");
        assert_eq!(out, "");
        assert!(err.contains("not found"), "got stderr: {err}");
        assert_eq!(code, 1);
    }
```

- [ ] **Step 9.2: Run the tests**

Run: `cargo test --lib builtin::command::tests::verbose`
Expected: 6 tests pass.

- [ ] **Step 9.3: Commit**

```bash
git add src/builtin/command.rs
git commit -m "$(cat <<'EOF'
feat(builtin): implement command -V output

Verbose description: "X is aliased to '...'", "X is a shell keyword",
"X is a function", "X is a special shell builtin", "X is a shell builtin",
or "X is /path". Not-found prints "yosh: command: X: not found" on stderr
with exit 1.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Wire up non-executing dispatch (`-v` / `-V`) in `exec/simple.rs`

**Files:**
- Modify: `src/exec/simple.rs` (special-case `command` like `wait`/`fg`/`bg`)

- [ ] **Step 10.1: Add the special case for `-v` / `-V`**

This step handles only the non-executing (`-v` / `-V`) dispatches. The executing forms (`-p`, no-flag) come in the next tasks.

Edit `src/exec/simple.rs`. Below the existing `fg`/`bg`/`jobs` special case (ends around line 143), add a new special case block. Find:

```rust
        // fg/bg/jobs need Executor access for job table + terminal control
        if command_name == "fg" || command_name == "bg" || command_name == "jobs" {
            let saved = self.apply_temp_assignments(&cmd.assignments).map_err(|e| {
                self.env.exec.last_exit_status = 1;
                e
            })?;
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
            let status = match command_name.as_str() {
                "fg" => self.builtin_fg(&args),
                "bg" => self.builtin_bg(&args),
                "jobs" => self.builtin_jobs(&args),
                _ => unreachable!(),
            }.unwrap_or_else(|e| { eprintln!("{}", e); e.exit_code() });
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }
```

After that block (and before `match classify_builtin(&command_name) {`), insert:

```rust
        // `command` needs Executor access for -p / no-flag execution paths.
        if command_name == "command" {
            let saved = self.apply_temp_assignments(&cmd.assignments).map_err(|e| {
                self.env.exec.last_exit_status = 1;
                e
            })?;
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
            let status = self.builtin_command(&args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }
```

- [ ] **Step 10.2: Add `builtin_command` method that handles `-v` / `-V`**

Create a new file or add to an existing file that provides methods on `Executor`. Edit `src/exec/simple.rs` by appending, at the end of the file, a new `impl Executor` block:

```rust
impl Executor {
    /// POSIX `command` builtin. Dispatches by verbosity:
    /// - Brief (`-v`) / Verbose (`-V`) → print and return exit status
    /// - Execute (`-p` or no flag) → handled in later tasks (returns 1 for now)
    pub(crate) fn builtin_command(&mut self, args: &[String]) -> i32 {
        use crate::builtin::command::{parse_flags, render_brief, render_verbose, Verbosity};

        let parsed = match parse_flags(args) {
            Ok(p) => p,
            Err(msg) => {
                eprintln!("yosh: {}", msg);
                return 2;
            }
        };

        match parsed.verbose {
            Verbosity::Brief => {
                let (out, code) = render_brief(&self.env, &parsed.name);
                if !out.is_empty() {
                    println!("{}", out);
                }
                code
            }
            Verbosity::Verbose => {
                let (out, err, code) = render_verbose(&self.env, &parsed.name);
                if !out.is_empty() {
                    println!("{}", out);
                }
                if !err.is_empty() {
                    eprintln!("{}", err);
                }
                code
            }
            Verbosity::Execute => {
                // TODO (next tasks): -p path and no-flag path.
                eprintln!("yosh: command: execution path not yet implemented");
                1
            }
        }
    }
}
```

- [ ] **Step 10.3: Build and run the unit tests**

Run: `cargo build && cargo test --lib`
Expected: clean build, all tests pass.

- [ ] **Step 10.4: Smoke test `-v` and `-V` interactively**

Run:
```
cargo run --quiet -- -c 'command -v cd'
```
Expected: prints `cd`, exit 0.

Run:
```
cargo run --quiet -- -c 'command -V if'
```
Expected: prints `if is a shell keyword`, exit 0.

Run:
```
cargo run --quiet -- -c 'command -v definitely_not_real_xyz; echo exit=$?'
```
Expected: (no output from command -v), then `exit=1`.

- [ ] **Step 10.5: Commit**

```bash
git add src/exec/simple.rs
git commit -m "$(cat <<'EOF'
feat(exec): wire command -v / -V into simple command dispatch

Handles 'command' as a special case (like wait/fg/bg) so the builtin has
Executor access for redirects and later the -p / no-flag execution paths.
This commit wires up only the informational -v / -V paths; -p and the
no-flag execute path come in subsequent commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Implement `command -p` (execute with default PATH)

**Files:**
- Modify: `src/exec/command.rs` (remove `#[allow(dead_code)]`)
- Modify: `src/exec/simple.rs` (fill in `Verbosity::Execute` branch when `-p` set)

- [ ] **Step 11.1: Unlock `find_in_path` for production use**

Edit `src/exec/command.rs:8-11`. Change:

```rust
/// Search each directory in `path_var` for `cmd`.
/// Returns the full path if found and executable, otherwise None.
#[allow(dead_code)]
pub fn find_in_path(cmd: &str, path_var: &str) -> Option<PathBuf> {
```

to:

```rust
/// Search each directory in `path_var` for `cmd`.
/// Returns the full path if found and executable, otherwise None.
pub fn find_in_path(cmd: &str, path_var: &str) -> Option<PathBuf> {
```

- [ ] **Step 11.2: Implement the `-p` branch of `builtin_command`**

Edit `src/exec/simple.rs`. Find the current placeholder in `builtin_command`:

```rust
            Verbosity::Execute => {
                // TODO (next tasks): -p path and no-flag path.
                eprintln!("yosh: command: execution path not yet implemented");
                1
            }
```

Replace it with:

```rust
            Verbosity::Execute => {
                if parsed.use_default_path {
                    self.exec_command_with_default_path(&parsed.name, &parsed.rest)
                } else {
                    // No-flag path (function-skip): implemented in the next task.
                    self.exec_command_skip_functions(&parsed.name, &parsed.rest)
                }
            }
```

Add new helper methods inside the same `impl Executor` block (right after `builtin_command`):

```rust
    /// `command -p name args...`: look up `name` via the POSIX default PATH
    /// (ignoring $PATH entirely) and exec it. Builtins are still honored
    /// for the name: POSIX says `command -p` runs the named utility in
    /// preference over functions, but builtins are part of the utility set.
    pub(crate) fn exec_command_with_default_path(
        &mut self,
        name: &str,
        args: &[String],
    ) -> i32 {
        use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
        use crate::builtin::special::exec_special_builtin;
        use crate::env::default_path::default_path;

        // If `name` is a builtin, run the builtin (POSIX: command -p still
        // runs builtins; -p only affects external lookup).
        match classify_builtin(name) {
            BuiltinKind::Special => {
                let status = exec_special_builtin(name, args, self);
                return status;
            }
            BuiltinKind::Regular => {
                // Don't re-enter special-cased handlers (wait/fg/bg/jobs/command).
                // If we get here with one of those, fall through to external.
                if !matches!(name, "wait" | "fg" | "bg" | "jobs" | "command") {
                    return exec_regular_builtin(name, args, &mut self.env);
                }
            }
            BuiltinKind::NotBuiltin => {}
        }

        let dp = default_path(&self.env).to_string();
        let resolved = match crate::exec::command::find_in_path(name, &dp) {
            Some(p) => p,
            None => {
                eprintln!("yosh: command: {}: not found", name);
                return 127;
            }
        };
        exec_external_absolute(&resolved, name, args, &mut self.env)
    }

    /// `command name args...` (no flags): execute `name` bypassing shell
    /// functions. Implemented in the next task — this is a stub so the
    /// build stays green between tasks.
    pub(crate) fn exec_command_skip_functions(
        &mut self,
        _name: &str,
        _args: &[String],
    ) -> i32 {
        eprintln!("yosh: command: function-skip path not yet implemented");
        1
    }
```

Add the external-execution helper (below the `impl Executor` block, as a free function in the same file):

```rust
/// Spawn an absolute path with `args`, inheriting the shell's exported
/// environment. Used by `command -p` (after default-PATH lookup) and by
/// `command name` (after current-PATH lookup).
///
/// Uses `std::process::Command` rather than manual fork+execvp because
/// yosh's existing external-command pipeline is tightly coupled to job
/// control, redirects, and env-sync concerns that we don't need here
/// (command -p / no-flag forms always run in the foreground with the
/// simple-command redirects already applied by the special-case handler).
fn exec_external_absolute(
    resolved: &std::path::Path,
    display_name: &str,
    args: &[String],
    env: &mut crate::env::ShellEnv,
) -> i32 {
    use std::os::unix::process::CommandExt;
    use std::os::unix::process::ExitStatusExt;

    let env_pairs: Vec<(String, String)> = env.vars.environ().to_vec();

    let result = std::process::Command::new(resolved)
        .arg0(display_name)
        .args(args)
        .env_clear()
        .envs(env_pairs)
        .status();

    match result {
        Ok(s) => {
            if let Some(code) = s.code() {
                code
            } else if let Some(sig) = s.signal() {
                128 + sig
            } else {
                1
            }
        }
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!("yosh: command: {}: not found", display_name);
                127
            }
            std::io::ErrorKind::PermissionDenied => {
                eprintln!("yosh: command: {}: permission denied", display_name);
                126
            }
            _ => {
                eprintln!("yosh: command: {}: {}", display_name, e);
                1
            }
        },
    }
}
```

- [ ] **Step 11.3: Build**

Run: `cargo build`
Expected: clean build (no `dead_code` warning for `find_in_path`, no unused-import warning).

- [ ] **Step 11.4: Run unit tests**

Run: `cargo test --lib`
Expected: all pass.

- [ ] **Step 11.5: Smoke test**

Run:
```
cargo run --quiet -- -c 'PATH=/nonsense command -p printf "hello\n"'
```
Expected: prints `hello`, exit 0.

- [ ] **Step 11.6: Commit**

```bash
git add src/exec/command.rs src/exec/simple.rs
git commit -m "$(cat <<'EOF'
feat(builtin): implement command -p (default PATH execution)

Resolves the sub-command against confstr(_CS_PATH) instead of the current
$PATH, then exec's the absolute path directly (bypassing execvp's own
PATH search). Builtins with the given name still run as builtins per
POSIX; only external lookup uses the default PATH.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Implement no-flag `command` (skip functions)

**Files:**
- Modify: `src/exec/simple.rs` (fill in `exec_command_skip_functions`)

- [ ] **Step 12.1: Replace the stub with a real implementation**

Edit `src/exec/simple.rs`. Find:

```rust
    /// `command name args...` (no flags): execute `name` bypassing shell
    /// functions. Implemented in the next task — this is a stub so the
    /// build stays green between tasks.
    pub(crate) fn exec_command_skip_functions(
        &mut self,
        _name: &str,
        _args: &[String],
    ) -> i32 {
        eprintln!("yosh: command: function-skip path not yet implemented");
        1
    }
```

Replace with:

```rust
    /// `command name args...`: execute `name` using the current $PATH but
    /// bypassing shell functions. Aliases are already handled (they're
    /// expanded at parse time, so `command` arrived here only if the
    /// parser saw `command` itself, not the expanded alias).
    pub(crate) fn exec_command_skip_functions(
        &mut self,
        name: &str,
        args: &[String],
    ) -> i32 {
        use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
        use crate::builtin::special::exec_special_builtin;

        // Builtins take precedence over external; functions are deliberately
        // skipped.
        match classify_builtin(name) {
            BuiltinKind::Special => return exec_special_builtin(name, args, self),
            BuiltinKind::Regular => {
                if !matches!(name, "wait" | "fg" | "bg" | "jobs" | "command") {
                    return exec_regular_builtin(name, args, &mut self.env);
                }
                // For the special-cased regular builtins, fall through to
                // external lookup (running `command wait` via PATH would be
                // surprising, but this matches how yosh currently dispatches
                // those names only when invoked as direct simple commands).
            }
            BuiltinKind::NotBuiltin => {}
        }

        // External: resolve via $PATH (not the POSIX default path).
        let path_var = self.env.vars.get("PATH").map(|s| s.to_string());
        let resolved = match path_var.as_deref().and_then(|pv| crate::exec::command::find_in_path(name, pv)) {
            Some(p) => p,
            None => {
                eprintln!("yosh: command: {}: not found", name);
                return 127;
            }
        };
        exec_external_absolute(&resolved, name, args, &mut self.env)
    }
```

- [ ] **Step 12.2: Build and run unit tests**

Run: `cargo build && cargo test --lib`
Expected: clean build, all tests pass.

- [ ] **Step 12.3: Smoke test**

Run:
```
cargo run --quiet -- -c 'printf() { echo fake; }; command printf "real\n"'
```
Expected: prints `real` (the external printf, bypassing the user function), exit 0.

- [ ] **Step 12.4: Commit**

```bash
git add src/exec/simple.rs
git commit -m "$(cat <<'EOF'
feat(builtin): implement no-flag command (function bypass)

'command foo args' runs foo as if no shell function called 'foo' existed.
Aliases are already out-of-band (parse-time expansion). Matches bash/zsh
behavior.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: E2E tests for `command -v` / `-V`

**Files:**
- Create: `e2e/builtin_command/command_v_finds_external.sh`
- Create: `e2e/builtin_command/command_v_builtin.sh`
- Create: `e2e/builtin_command/command_v_alias.sh`
- Create: `e2e/builtin_command/command_V_external.sh`
- Create: `e2e/builtin_command/command_V_not_found.sh`

- [ ] **Step 13.1: Create the test directory and files**

Run:
```bash
mkdir -p e2e/builtin_command
```

Create `e2e/builtin_command/command_v_finds_external.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v reports absolute path for externals
# EXPECT_OUTPUT: /bin/sh
# EXPECT_EXIT: 0
PATH=/bin:/usr/bin command -v sh
```

Create `e2e/builtin_command/command_v_builtin.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v reports builtin name only
# EXPECT_OUTPUT: cd
# EXPECT_EXIT: 0
command -v cd
```

Create `e2e/builtin_command/command_v_alias.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v reports alias in POSIX form
# EXPECT_OUTPUT: alias ll='ls -l'
# EXPECT_EXIT: 0
alias ll='ls -l'
command -v ll
```

Create `e2e/builtin_command/command_V_external.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -V reports "X is /path" for externals
# EXPECT_OUTPUT: sh is /bin/sh
# EXPECT_EXIT: 0
PATH=/bin:/usr/bin command -V sh
```

Create `e2e/builtin_command/command_V_not_found.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -V on unknown name exits with nonzero status
# EXPECT_EXIT: 1
command -V definitely_not_a_real_cmd_xyz 2>/dev/null
```

- [ ] **Step 13.2: Set the expected permissions**

Run:
```bash
chmod 644 e2e/builtin_command/*.sh
```

Expected: all five files are now mode `644` (matches the project's E2E convention per CLAUDE.md).

- [ ] **Step 13.3: Build debug binary**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 13.4: Run the new E2E tests**

Run: `./e2e/run_tests.sh --filter=builtin_command`
Expected: 5 tests pass. If any fail, read the diff output and fix either the test expectation or the implementation.

- [ ] **Step 13.5: Commit**

```bash
git add e2e/builtin_command/
git commit -m "$(cat <<'EOF'
test(e2e): add command -v / -V E2E tests

Covers external, builtin, alias, and not-found cases. Uses /bin/sh as the
external target (POSIX-mandatory on both macOS and Linux) and sets PATH
explicitly so the search is deterministic.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: E2E tests for `command -p` and function-skip

**Files:**
- Create: `e2e/builtin_command/command_p_when_path_unset.sh`
- Create: `e2e/builtin_command/command_skips_function.sh`

- [ ] **Step 14.1: Create the tests**

Create `e2e/builtin_command/command_p_when_path_unset.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -p uses default PATH even when PATH is unset
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset PATH
command -p printf "hello"
```

Create `e2e/builtin_command/command_skips_function.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command bypasses user-defined functions
# EXPECT_OUTPUT: real
# EXPECT_EXIT: 0
printf() { echo "fake"; }
command printf "real"
```

- [ ] **Step 14.2: Set permissions**

Run:
```bash
chmod 644 e2e/builtin_command/command_p_when_path_unset.sh e2e/builtin_command/command_skips_function.sh
```

- [ ] **Step 14.3: Run the tests**

Run: `./e2e/run_tests.sh --filter=builtin_command`
Expected: 7 tests pass (5 from Task 13 + 2 new).

- [ ] **Step 14.4: Commit**

```bash
git add e2e/builtin_command/command_p_when_path_unset.sh e2e/builtin_command/command_skips_function.sh
git commit -m "$(cat <<'EOF'
test(e2e): add command -p and function-skip E2E tests

command -p exercises the confstr(_CS_PATH) lookup with PATH explicitly
unset. The function-skip test proves 'command printf' runs the external,
not the user-defined printf().

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: E2E test for startup-time PATH initialization

**Files:**
- Create: `e2e/builtin_command/path_initialized_at_startup.sh`

- [ ] **Step 15.1: Create the test**

Create `e2e/builtin_command/path_initialized_at_startup.sh`:

```sh
#!/bin/sh
# POSIX_REF: 8. Environment Variables (PATH)
# DESCRIPTION: yosh sets a non-empty default PATH at startup when environment has none
# EXPECT_EXIT: 0
# Run a yosh sub-invocation with env -i so PATH is truly absent from its
# inherited environment; yosh should populate a default so `sh` is findable.
# The outer (current) yosh sets PATH=/bin:/usr/bin so `env` and the yosh
# binary itself are resolvable.
PATH=/bin:/usr/bin
env -i ./target/debug/yosh -c '
  case "$PATH" in
    "" ) exit 1 ;;
  esac
  command -v sh >/dev/null 2>&1 || exit 1
  exit 0
'
```

> Note: this test relies on the debug binary at `./target/debug/yosh`, which is built by the preceding `cargo build` step. The E2E runner runs tests from the repo root, so the relative path resolves correctly.

- [ ] **Step 15.2: Set permissions**

Run:
```bash
chmod 644 e2e/builtin_command/path_initialized_at_startup.sh
```

- [ ] **Step 15.3: Ensure debug binary exists**

Run: `cargo build`
Expected: `./target/debug/yosh` exists.

- [ ] **Step 15.4: Run the test**

Run: `./e2e/run_tests.sh --filter=builtin_command`
Expected: 8 tests pass total.

- [ ] **Step 15.5: Commit**

```bash
git add e2e/builtin_command/path_initialized_at_startup.sh
git commit -m "$(cat <<'EOF'
test(e2e): verify PATH is initialized at startup when env has none

Runs 'env -i ./target/debug/yosh -c ...' so the child yosh starts without
any PATH in its environment. The inner script asserts $PATH is non-empty
and 'command -v sh' succeeds — proving confstr(_CS_PATH) filled the gap.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 16: Full test sweep and wrap-up

**Files:**
- None modified.

- [ ] **Step 16.1: Run all unit tests**

Run: `cargo test`
Expected: 100% pass.

- [ ] **Step 16.2: Run all E2E tests**

Run: `./e2e/run_tests.sh`
Expected: 100% pass (no regressions in pre-existing tests).

- [ ] **Step 16.3: Confirm the no-exports-from-scratch behavior**

Run on macOS:
```bash
env -i cargo run --quiet -- -c 'echo "[$PATH]"'
```
Expected: prints `[` followed by a PATH value that includes `/bin` or `/usr/bin`, then `]`, exit 0.

Run on Linux (if available via CI, Docker, or a Linux workstation) the same command. Expected: similar, different exact PATH string, still contains `/bin` or `/usr/bin`.

- [ ] **Step 16.4: Check clippy is clean**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no errors. If clippy complains about something added in this plan, fix it and re-commit (small, focused commit per issue).

- [ ] **Step 16.5: Final verification commit (only if there were fixes in 16.4)**

If clippy produced fixes, amend the history with a small commit message like:

```bash
git add -u
git commit -m "$(cat <<'EOF'
chore: clippy fixes for command builtin implementation

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 16.6: Confirm TODO.md has no new entries needed**

Read `TODO.md`. Confirm no follow-up items for this plan need to be added (the scope closed cleanly: `command` builtin complete; default PATH handled).

- [ ] **Step 16.7: Final message to the user**

Report:
- Tasks completed: 16
- Unit tests added: approx 30
- E2E tests added: 8
- Files created: 3 src files + 8 E2E files
- Files modified: 5 src files
- `command` builtin: POSIX-complete (`-p` / `-v` / `-V` plus function-skip)
- Startup PATH fallback: active when env lacks PATH

---

## Notes

### Why `command` is special-cased in `exec/simple.rs`

Following the existing pattern for `wait`, `fg`, `bg`, `jobs` — these builtins need access to the `Executor` (not just `ShellEnv`) for redirect handling, subprocess lifecycle, and (for `command`) the option to invoke builtin dispatch paths directly. Treating it as just a `Regular` builtin wouldn't give that access.

### Why `std::process::Command` instead of manual `fork`/`execv`

After `find_in_path` resolves the name, we already have an absolute path. The existing external-command pipeline in `exec/simple.rs` wraps `execvp` in a lot of machinery (monitor-mode process-group placement, fine-grained signal reset, manual `libc::setenv` for each exported var, redirect application in the child). `command -p` and no-flag `command` run in the foreground with the simple-command redirects already applied by the special-case handler, so none of that machinery applies. `std::process::Command` with `env_clear() + envs(env.vars.environ())` and unix's stable `arg0(display_name)` extension gives us correct POSIX semantics (argv[0] = name, child inherits the shell's exported env) without re-implementing the fork/exec dance.

### Why aliases are already safe for `command name`

POSIX aliases are expanded at parse time, not execution time. When the user writes `command ls`, the parser sees `command` in command-name position and looks up `command` in aliases (there is none), so the word list passed to the executor is `["command", "ls"]`. By the time `builtin_command` runs, "ls" is not subject to alias expansion — it's a plain argument. This is why Flow D doesn't need to do anything special for aliases.

### What's not in this plan (deliberately)

- `type` builtin (`resolve_command_kind` is ready for it, but left for a future task)
- `which` builtin (non-POSIX; future task)
- `hash` builtin (POSIX; requires adding a command lookup cache, future task)
- Unifying `PATH=v cmd` with `command -p` (future refactor, noted in spec)
