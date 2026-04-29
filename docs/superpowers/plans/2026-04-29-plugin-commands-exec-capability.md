# Plugin `commands:exec` Capability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a WASM plugin capability `commands:exec` that lets plugins run external commands gated by a per-plugin allowlist of glob-style argv patterns (e.g., `git status:*`), with a 1000ms hard timeout.

**Architecture:** New WIT interface `yosh:plugin/commands@0.1.0` with a single `exec(program, args) -> result<exec-output, error-code>` function. Capability bit `CAP_COMMANDS_EXEC = 0x400` toggles host wiring (real vs deny). A new `src/plugin/pattern.rs` module owns the pattern grammar (token prefix with optional trailing `:*`). Host pulls allowlist patterns from `plugins.toml` `allowed_commands` and matches each `exec` call against them before spawning. Timeout enforced via `mpsc::recv_timeout` + `nix::sys::signal::kill(SIGTERM)` followed by `Child::kill()` (SIGKILL).

**Tech Stack:** Rust 2024, wasmtime 27 component model, `nix` 0.31 for SIGTERM, `std::process::Command` for spawn, `std::sync::mpsc` for timeout, `serde` for TOML config.

**Spec:** `docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md`

---

## File Structure

| File | Responsibility | Status |
|------|----------------|--------|
| `crates/yosh-plugin-api/wit/yosh-plugin.wit` | WIT contract: error-code variants, `commands` interface, `import commands` in world | Modify |
| `crates/yosh-plugin-api/src/lib.rs` | Capability bit, enum variant, string parsing | Modify |
| `src/plugin/pattern.rs` | `CommandPattern` parser + matcher (new module) | Create |
| `src/plugin/config.rs` | `PluginEntry.allowed_commands` field, capability string | Modify |
| `src/plugin/host.rs` | `HostContext.allowed_commands`, `host_commands_exec`, `deny_commands_exec`, `spawn_with_timeout` | Modify |
| `src/plugin/linker.rs` | Wire `yosh:plugin/commands@0.1.0` real-vs-deny per `CAP_COMMANDS_EXEC` | Modify |
| `src/plugin/mod.rs` | Thread `allowed_commands` from config through `load_one` into `HostContext`; `mod pattern;` | Modify |
| `crates/yosh-plugin-sdk/src/lib.rs` | Re-export `ExecOutput`, add `exec(program, args)` SDK helper | Modify |
| `tests/plugins/test_plugin/src/lib.rs` | Add `run-echo` command + `Capability::CommandsExec` | Modify |
| `tests/plugin.rs` | Integration tests t16–t20 | Modify |
| `TODO.md` | Optionally drop completed items if they overlap | Modify |

---

## Task 1: Add `CAP_COMMANDS_EXEC` capability to the API crate

**Files:**
- Modify: `crates/yosh-plugin-api/src/lib.rs`

This task adds the capability constant, enum variant, and string parsing — but does NOT touch the WIT yet, so existing plugins still build and load unchanged.

- [ ] **Step 1: Write failing tests for the new capability**

Add to `crates/yosh-plugin-api/src/lib.rs` `mod tests`:

```rust
#[test]
fn parse_commands_exec_capability() {
    assert_eq!(parse_capability("commands:exec"), Some(Capability::CommandsExec));
}

#[test]
fn commands_exec_capability_round_trip() {
    assert_eq!(parse_capability(Capability::CommandsExec.as_str()), Some(Capability::CommandsExec));
    assert_eq!(Capability::CommandsExec.as_str(), "commands:exec");
    assert_eq!(Capability::CommandsExec.to_bitflag(), CAP_COMMANDS_EXEC);
}

#[test]
fn cap_all_includes_commands_exec_bit() {
    assert_eq!(CAP_ALL & CAP_COMMANDS_EXEC, CAP_COMMANDS_EXEC);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p yosh-plugin-api`
Expected: FAIL with "cannot find value `CAP_COMMANDS_EXEC`" / "no variant `CommandsExec`".

- [ ] **Step 3: Implement the constant, enum variant, and parser arms**

In `crates/yosh-plugin-api/src/lib.rs`:

After `pub const CAP_FILES_WRITE: u32 = 0x200;`, add:
```rust
pub const CAP_COMMANDS_EXEC:   u32 = 0x400;
```

In `pub const CAP_ALL: u32 = ...`, append `| CAP_COMMANDS_EXEC` to the OR chain.

In `pub enum Capability`, append `CommandsExec,` after `FilesWrite,`.

In `Capability::to_bitflag`, add the arm:
```rust
Capability::CommandsExec   => CAP_COMMANDS_EXEC,
```

In `Capability::as_str`, add:
```rust
Capability::CommandsExec   => "commands:exec",
```

In `parse_capability`, add the arm before the catch-all:
```rust
"commands:exec"    => Capability::CommandsExec,
```

In the existing `cap_all_covers_every_variant` test, append `Capability::CommandsExec` to the array.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p yosh-plugin-api`
Expected: PASS (existing tests + 3 new tests).

- [ ] **Step 5: Commit**

```bash
git add crates/yosh-plugin-api/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(plugin-api): add CAP_COMMANDS_EXEC capability bit

Reserves bit 0x400 for the upcoming `commands:exec` plugin capability
that will let plugins invoke external commands behind a per-plugin
glob-style allowlist. WIT changes and host wiring follow in subsequent
commits.

Spec: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Create `CommandPattern` module

**Files:**
- Create: `src/plugin/pattern.rs`
- Modify: `src/plugin/mod.rs` (add `mod pattern;`)

This is a pure-Rust parser+matcher with no WIT dependencies. Self-contained, easy to TDD.

- [ ] **Step 1: Write the failing tests**

Create `src/plugin/pattern.rs`:

```rust
//! Glob-style argv allowlist patterns for the `commands:exec` capability.
//!
//! See `docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md` §4.

/// A parsed allowlist pattern. Matches against an argv `&[String]` slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPattern {
    pub tokens: Vec<String>,
    pub has_glob_suffix: bool,
}

impl CommandPattern {
    /// Parse a single pattern string. Tokens are whitespace-separated.
    /// A trailing `:*` (no whitespace before it) marks the pattern as
    /// a prefix match; otherwise the pattern is exact-length.
    ///
    /// Errors:
    /// * empty / whitespace-only input
    /// * a lone `:*` with no preceding tokens
    pub fn parse(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("empty pattern".to_string());
        }

        let (body, has_glob_suffix) = if let Some(stripped) = trimmed.strip_suffix(":*") {
            (stripped.trim_end(), true)
        } else {
            (trimmed, false)
        };

        if body.is_empty() {
            return Err("pattern has `:*` but no tokens".to_string());
        }

        let tokens: Vec<String> = body
            .split_whitespace()
            .map(|t| t.to_string())
            .collect();

        if tokens.is_empty() {
            return Err("pattern has no tokens after splitting".to_string());
        }

        Ok(CommandPattern { tokens, has_glob_suffix })
    }

    /// Match this pattern against an argv slice (`[program, arg1, arg2, ...]`).
    pub fn matches(&self, argv: &[String]) -> bool {
        if self.has_glob_suffix {
            argv.len() >= self.tokens.len()
                && self.tokens.iter().zip(argv).all(|(p, a)| p == a)
        } else {
            argv.len() == self.tokens.len()
                && self.tokens.iter().zip(argv).all(|(p, a)| p == a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_glob_suffix_separates_tokens() {
        let p = CommandPattern::parse("git log:*").unwrap();
        assert_eq!(p.tokens, vec!["git".to_string(), "log".to_string()]);
        assert!(p.has_glob_suffix);
    }

    #[test]
    fn parse_no_suffix_is_exact() {
        let p = CommandPattern::parse("git log").unwrap();
        assert_eq!(p.tokens, vec!["git".to_string(), "log".to_string()]);
        assert!(!p.has_glob_suffix);
    }

    #[test]
    fn parse_empty_string_errors() {
        assert!(CommandPattern::parse("").is_err());
        assert!(CommandPattern::parse("   ").is_err());
    }

    #[test]
    fn parse_lone_glob_suffix_errors() {
        assert!(CommandPattern::parse(":*").is_err());
        assert!(CommandPattern::parse("  :*").is_err());
    }

    #[test]
    fn match_glob_suffix_zero_extra() {
        let p = CommandPattern::parse("git:*").unwrap();
        assert!(p.matches(&["git".to_string()]));
    }

    #[test]
    fn match_glob_suffix_many_extra() {
        let p = CommandPattern::parse("git:*").unwrap();
        assert!(p.matches(&[
            "git".to_string(),
            "log".to_string(),
            "-p".to_string(),
        ]));
    }

    #[test]
    fn match_exact_requires_equal_length() {
        let p = CommandPattern::parse("git status").unwrap();
        assert!(p.matches(&["git".to_string(), "status".to_string()]));
        assert!(!p.matches(&["git".to_string(), "status".to_string(), "--porcelain".to_string()]));
        assert!(!p.matches(&["git".to_string()]));
    }

    #[test]
    fn match_literal_compare() {
        let p = CommandPattern::parse("git:*").unwrap();
        assert!(!p.matches(&["/usr/bin/git".to_string(), "status".to_string()]));
    }

    #[test]
    fn match_glob_suffix_subcommand_lock() {
        let p = CommandPattern::parse("git status:*").unwrap();
        assert!(p.matches(&["git".to_string(), "status".to_string()]));
        assert!(p.matches(&["git".to_string(), "status".to_string(), "--porcelain".to_string()]));
        assert!(!p.matches(&["git".to_string(), "log".to_string()]));
    }
}
```

In `src/plugin/mod.rs`, add `pub mod pattern;` next to the existing `pub mod cache;` / `pub mod config;` block (around line 23).

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p yosh pattern::`
Expected: PASS (9 tests).

- [ ] **Step 3: Commit**

```bash
git add src/plugin/pattern.rs src/plugin/mod.rs
git commit -m "$(cat <<'EOF'
feat(plugin): add CommandPattern parser and matcher

Pure-Rust pattern grammar for the upcoming commands:exec allowlist.
Supports two forms: `tok1 tok2` (exact-length match) and
`tok1 tok2:*` (token-prefix match, any tail). Wired into
src/plugin/mod.rs but not yet consumed.

Spec §4: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Atomic WIT change + linker deny stub + HostContext field

**Files:**
- Modify: `crates/yosh-plugin-api/wit/yosh-plugin.wit`
- Modify: `src/plugin/host.rs`
- Modify: `src/plugin/linker.rs`

This is the atomic moment when `import commands;` enters the world. Existing plugins (test_plugin, trap_plugin) will rebuild against the new WIT and import commands; the linker must therefore wire at least a deny stub before this commit lands. We do NOT yet wire `host_commands_exec` (that's Task 4).

- [ ] **Step 1: Update the WIT file**

Edit `crates/yosh-plugin-api/wit/yosh-plugin.wit`. In `interface types`, **append** `timeout` and `pattern-not-allowed` to the end of the `error-code` enum (keep `other` in its current position — appending after it preserves all existing wire discriminants):

```wit
enum error-code {
    denied,
    invalid-argument,
    io-failed,
    not-found,
    other,
    timeout,
    pattern-not-allowed,
}
```

After the `interface files { ... }` block, add a new interface:

```wit
interface commands {
    use types.{error-code};

    record exec-output {
        exit-code: s32,
        stdout: list<u8>,
        stderr: list<u8>,
    }

    exec: func(program: string, args: list<string>) -> result<exec-output, error-code>;
}
```

In `world plugin-world { ... }`, add `import commands;` after `import io;`:

```wit
world plugin-world {
    import variables;
    import filesystem;
    import files;
    import io;
    import commands;
    export plugin;
    export hooks;
}
```

- [ ] **Step 2: Add `allowed_commands` field to `HostContext` and a deny stub**

Edit `src/plugin/host.rs`.

Near the top of the file, add:
```rust
use super::generated::yosh::plugin::commands::ExecOutput;
use super::pattern::CommandPattern;
```

In `pub struct HostContext { ... }`, add the field (last):
```rust
pub(super) allowed_commands: Vec<CommandPattern>,
```

In `HostContext::new_for_plugin`, initialize the new field after `resource_table: ResourceTable::new()`:
```rust
allowed_commands: Vec::new(),
```

After the existing `// ── yosh:plugin/files host imports ───` block (after the `deny_files_remove_dir` function around line 455), add:

```rust
// ── yosh:plugin/commands host imports ───────────────────────────────

pub(super) fn deny_commands_exec(
    _ctx: &mut HostContext,
    _program: String,
    _args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Note: `host_commands_exec` is added in Task 4. Adding only the deny stub here keeps the WIT-change commit atomic and small.

- [ ] **Step 3: Wire the linker with a deny-only path**

Edit `src/plugin/linker.rs`.

Update the `use super::host::{...}` block to include `deny_commands_exec`:
```rust
use super::host::{
    HostContext,
    deny_commands_exec,
    deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    /* … unchanged … */
};
```

Note: the `CAP_COMMANDS_EXEC` import is added in Task 4 along with the real `if/else`. Adding it now would warn `unused_imports` because this transient commit binds the deny stub unconditionally.

After the `// ── yosh:plugin/files ──` block (after the `if/else` for the write group), add a single deny binding (no `if/else` yet — it lands in Task 4 once the real impl exists, avoiding a transient `clippy::if_same_then_else` warning):

```rust
// ── yosh:plugin/commands ───────────────────────────────────────────
//
// host_commands_exec wiring lands in the next commit (Task 4). For
// now the deny stub is bound unconditionally so existing plugins
// still instantiate cleanly.
let mut commands = linker.instance("yosh:plugin/commands@0.1.0")?;
commands.func_wrap(
    "exec",
    |mut store, (program, args): (String, Vec<String>)| {
        Ok((deny_commands_exec(store.data_mut(), program, args),))
    },
)?;
```

- [ ] **Step 4: Run the host crate build to confirm WIT bindgen succeeds**

Run: `cargo build -p yosh`
Expected: PASS. This shells out to `wasmtime::component::bindgen!` and confirms the new WIT parses.

- [ ] **Step 5: Run host tests to confirm nothing regressed**

Run: `cargo test -p yosh plugin::`
Expected: PASS. Existing metadata-contract / linker_construction_smoke tests cover the deny-only path automatically.

- [ ] **Step 6: Rebuild the wasm test plugins to confirm the WIT change compiles guest-side**

Run:
```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
```
Expected: PASS. The plugins don't yet use `commands::exec`, but rebuilding confirms the WIT is internally consistent.

- [ ] **Step 7: Commit**

```bash
git add crates/yosh-plugin-api/wit/yosh-plugin.wit src/plugin/host.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
feat(plugin): add commands:exec WIT interface with deny-only wiring

Adds the `commands` WIT interface with the single `exec(program, args)`
host import, plus the `timeout` and `pattern-not-allowed` error-code
variants (appended to preserve existing wire discriminants).
Linker wires both capability arms to the deny stub for now;
host_commands_exec lands in the next commit. HostContext gains an
empty `allowed_commands` field that the loader populates later.

Existing plugins continue to instantiate cleanly because the deny stub
returns Err(Denied) and no plugin yet calls commands::exec.

Spec §2, §3, §6: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Implement `host_commands_exec` with timeout

**Files:**
- Modify: `src/plugin/host.rs`
- Modify: `src/plugin/linker.rs`

- [ ] **Step 1: Write failing host tests for the metadata-contract and pattern guards**

Append to the `mod tests` block in `src/plugin/host.rs` (after the existing `host_files_remove_dir_recursive_removes_subtree` test):

```rust
// ── commands:exec host tests (spec §10) ─────────────────────────────

fn ctx_with_allowed(env: &mut ShellEnv, patterns: &[&str]) -> HostContext {
    let mut ctx = bound_env_ctx(env);
    ctx.allowed_commands = patterns
        .iter()
        .map(|s| super::super::pattern::CommandPattern::parse(s).expect("valid pattern"))
        .collect();
    ctx
}

#[test]
fn host_commands_exec_metadata_contract_denied_when_env_null() {
    let mut ctx = null_env_ctx();
    let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hi".into()]);
    assert!(matches!(result, Err(ErrorCode::Denied)));
}

#[test]
fn host_commands_exec_invalid_argument_on_empty_program() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = bound_env_ctx(&mut env);
    let result = host_commands_exec(&mut ctx, String::new(), vec![]);
    assert!(matches!(result, Err(ErrorCode::InvalidArgument)));
}

#[test]
fn host_commands_exec_pattern_not_allowed_when_no_match() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = ctx_with_allowed(&mut env, &["ls:*"]);
    let result = host_commands_exec(&mut ctx, "echo".into(), vec!["hi".into()]);
    assert!(matches!(result, Err(ErrorCode::PatternNotAllowed)));
}

#[test]
fn host_commands_exec_runs_when_pattern_matches() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = ctx_with_allowed(&mut env, &["/bin/echo:*"]);
    let result = host_commands_exec(
        &mut ctx,
        "/bin/echo".into(),
        vec!["hello".into()],
    )
    .expect("echo should succeed");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, b"hello\n");
    assert!(result.stderr.is_empty());
}

#[test]
fn host_commands_exec_captures_stderr_separately() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
    let result = host_commands_exec(
        &mut ctx,
        "/bin/sh".into(),
        vec!["-c".into(), "echo out; echo err 1>&2".into()],
    )
    .expect("sh should succeed");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, b"out\n");
    assert_eq!(result.stderr, b"err\n");
}

#[test]
fn host_commands_exec_propagates_nonzero_exit() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
    let result = host_commands_exec(
        &mut ctx,
        "/bin/sh".into(),
        vec!["-c".into(), "exit 42".into()],
    )
    .expect("sh should run to exit");
    assert_eq!(result.exit_code, 42);
}

#[test]
fn host_commands_exec_returns_not_found_for_missing_binary() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = ctx_with_allowed(&mut env, &["/no/such/binary-xyz:*"]);
    let result = host_commands_exec(
        &mut ctx,
        "/no/such/binary-xyz".into(),
        vec![],
    );
    assert!(matches!(result, Err(ErrorCode::NotFound)));
}

#[test]
fn host_commands_exec_timeout_after_1000ms() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
    let start = std::time::Instant::now();
    let result = host_commands_exec(
        &mut ctx,
        "/bin/sleep".into(),
        vec!["5".into()],
    );
    let elapsed = start.elapsed();
    assert!(matches!(result, Err(ErrorCode::Timeout)));
    // Hard cap is 1000ms + 100ms grace + a generous slack for thread
    // scheduling. Anything past 2 seconds means the timeout enforcement
    // is broken, not just slow.
    assert!(
        elapsed < std::time::Duration::from_millis(2000),
        "timeout took {:?}, expected <2000ms",
        elapsed
    );
}
```

- [ ] **Step 2: Run the new tests to verify they fail to compile**

Run: `cargo test -p yosh host::tests::host_commands_exec`
Expected: FAIL with "cannot find function `host_commands_exec`".

- [ ] **Step 3: Implement `host_commands_exec` and `spawn_with_timeout`**

In `src/plugin/host.rs`, replace the `// ── yosh:plugin/commands host imports ───` block with the full implementation. The deny stub stays; add the real impl above it:

```rust
// ── yosh:plugin/commands host imports ───────────────────────────────

pub(super) fn host_commands_exec(
    ctx: &mut HostContext,
    program: String,
    args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    // argv = [program, args...]; pattern matcher consumes the literal
    // strings (no PATH resolution, no basename normalization — see
    // spec §5).
    let mut argv = Vec::with_capacity(1 + args.len());
    argv.push(program.clone());
    argv.extend(args.iter().cloned());

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    spawn_with_timeout(&program, &args, std::time::Duration::from_millis(1000))
}

pub(super) fn deny_commands_exec(
    _ctx: &mut HostContext,
    _program: String,
    _args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}

fn spawn_with_timeout(
    program: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };

    // Drain stdout and stderr concurrently so a buffer-full child does
    // not deadlock waiting on us. Each thread reads to EOF, which only
    // happens after the child exits or its pipe is closed.
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let (out_tx, out_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    let (err_tx, err_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stdout_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = out_tx.send(r);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stderr_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = err_tx.send(r);
    });

    let deadline = Instant::now() + timeout;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => return Err(ErrorCode::IoFailed),
        }
        if Instant::now() >= deadline {
            // Timeout: SIGTERM, 100ms grace, then SIGKILL.
            let pid = nix::unistd::Pid::from_raw(child.id() as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
            let grace = Instant::now() + std::time::Duration::from_millis(100);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    _ => {}
                }
                if Instant::now() >= grace {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
            // Drain pipes; the reader threads exit once the child
            // closes its end. Discard whatever was buffered.
            let _ = out_rx.recv_timeout(std::time::Duration::from_millis(100));
            let _ = err_rx.recv_timeout(std::time::Duration::from_millis(100));
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(std::time::Duration::from_millis(10));
    };

    let stdout = out_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();
    let stderr = err_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();

    Ok(ExecOutput {
        exit_code: exit_status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}
```

- [ ] **Step 4: Replace the deny-only linker arm with the real wiring**

Edit `src/plugin/linker.rs`.

Add `CAP_COMMANDS_EXEC` to the `use yosh_plugin_api::{...}` block:
```rust
use yosh_plugin_api::{
    CAP_COMMANDS_EXEC, CAP_FILES_READ, CAP_FILES_WRITE, CAP_FILESYSTEM, CAP_IO,
    CAP_VARIABLES_READ, CAP_VARIABLES_WRITE,
};
```

Add `host_commands_exec` to the `use super::host::{...}` block:
```rust
use super::host::{
    HostContext,
    deny_commands_exec, host_commands_exec,
    /* … */
};
```

In the `// ── yosh:plugin/commands ──` block, replace the unconditional deny binding with the real `if/else`:

```rust
if has(allowed, CAP_COMMANDS_EXEC) {
    commands.func_wrap(
        "exec",
        |mut store, (program, args): (String, Vec<String>)| {
            Ok((host_commands_exec(store.data_mut(), program, args),))
        },
    )?;
} else {
    commands.func_wrap(
        "exec",
        |mut store, (program, args): (String, Vec<String>)| {
            Ok((deny_commands_exec(store.data_mut(), program, args),))
        },
    )?;
}
```

- [ ] **Step 5: Run the host tests**

Run: `cargo test -p yosh host::tests::host_commands_exec`
Expected: PASS (8 new tests).

Note: the timeout test spawns `/bin/sleep 5` and waits up to ~1.1 s. On a slow CI, expand the assertion bound before bisecting.

- [ ] **Step 6: Run the full plugin module tests for regressions**

Run: `cargo test -p yosh plugin::`
Expected: PASS (existing tests still green; `linker_construction_smoke` exercises both arms).

- [ ] **Step 7: Commit**

```bash
git add src/plugin/host.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
feat(plugin): implement host_commands_exec with 1000ms timeout

Real host implementation of the commands:exec capability. Spawns the
child via std::process::Command with stdin=/dev/null, captures
stdout/stderr separately on background reader threads, polls
try_wait() until either exit or 1000ms deadline, escalates to
SIGTERM then SIGKILL after a 100ms grace period.

Pattern matching uses the literal program string (no PATH resolution
or basename normalization) per spec §5. argv = [program, args...] is
matched against HostContext.allowed_commands; an empty allowlist
means every call returns PatternNotAllowed.

Linker now wires the granted arm to host_commands_exec.

Spec §5, §6: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Config — `allowed_commands` field and `commands:exec` parsing

**Files:**
- Modify: `src/plugin/config.rs`

- [ ] **Step 1: Write failing tests for the new config field and capability arm**

In `src/plugin/config.rs::tests`, append:

```rust
#[test]
fn parse_allowed_commands_field() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    write!(
        f,
        r#"
[[plugin]]
name = "git-prompt"
path = "/tmp/git-prompt.wasm"
capabilities = ["commands:exec"]
allowed_commands = ["git status:*", "git rev-parse:*"]
"#
    )
    .unwrap();
    let config = PluginConfig::load(f.path()).unwrap();
    let entry = &config.plugin[0];
    assert_eq!(
        entry.allowed_commands,
        Some(vec![
            "git status:*".to_string(),
            "git rev-parse:*".to_string(),
        ])
    );
}

#[test]
fn parse_missing_allowed_commands_is_none() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    write!(
        f,
        r#"
[[plugin]]
name = "no-exec"
path = "/tmp/x.wasm"
"#
    )
    .unwrap();
    let config = PluginConfig::load(f.path()).unwrap();
    assert!(config.plugin[0].allowed_commands.is_none());
}

#[test]
fn parse_commands_exec_capability_string_to_bitflag() {
    use yosh_plugin_api::CAP_COMMANDS_EXEC;
    assert_eq!(capability_from_str("commands:exec"), Some(CAP_COMMANDS_EXEC));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p yosh config::tests::parse_allowed_commands_field config::tests::parse_commands_exec_capability_string_to_bitflag`
Expected: FAIL with "no field `allowed_commands`" / "expected `Some(...)`, got `None`".

- [ ] **Step 3: Add the field and the capability arm**

Edit `src/plugin/config.rs`. In `pub struct PluginEntry`, after the `cache_key` field, add:

```rust
/// Per-plugin allowlist of argv patterns that the `commands:exec`
/// capability is restricted to. `None` or empty means no command is
/// permitted; matching is OR across the list.
#[serde(default)]
pub allowed_commands: Option<Vec<String>>,
```

In `pub fn capability_from_str(s: &str)`, add the arm before `_ => None,`:

```rust
"commands:exec" => Some(yosh_plugin_api::CAP_COMMANDS_EXEC),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p yosh config::`
Expected: PASS (existing + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src/plugin/config.rs
git commit -m "$(cat <<'EOF'
feat(plugin): add allowed_commands field and commands:exec parsing

PluginEntry gains `allowed_commands: Option<Vec<String>>` for the
per-plugin argv allowlist. `capability_from_str` learns the
`commands:exec` string. The loader (next commit) will parse the raw
strings into CommandPattern values and stash them on HostContext.

TOML key is snake_case (allowed_commands) to match cwasm_path / cache_key
convention on the same struct.

Spec §3, §9: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Thread `allowed_commands` through `PluginManager::load_one`

**Files:**
- Modify: `src/plugin/mod.rs`

The plugin loader needs to (a) accept allow-pattern strings, (b) parse them via `CommandPattern::parse`, (c) populate `HostContext.allowed_commands` before instantiation. Existing call sites (`load_from_config`, `load_plugin`, `test_helpers::load_plugin_with_caps`) need an extra parameter.

- [ ] **Step 1: Inspect existing `load_one` call sites**

Run: `grep -n "load_one\|load_plugin_with_caps" src/plugin/mod.rs tests/plugin.rs`

You should see five call sites: `load_from_config` (line ~151), `load_plugin` (line ~169), `test_helpers::load_plugin_with_caps` (line ~609), `test_helpers::load_plugin_with_cache` (line ~623), and the doc comment in the impl. All must pass the new `allowed_commands` argument.

- [ ] **Step 2: Update `load_one` signature**

Edit `src/plugin/mod.rs`. Change the signature of `pub(super) fn load_one`:

```rust
pub(super) fn load_one(
    &mut self,
    path: &Path,
    env: &mut ShellEnv,
    config_capabilities: Option<u32>,
    cwasm_path: Option<&Path>,
    expected_key: Option<&CacheKey>,
    allowed_commands: &[String],
) -> Result<(), String> {
```

Inside the function body, **before** building `scratch_linker` (around line 259), parse the patterns:

```rust
let parsed_allowed_commands: Vec<self::pattern::CommandPattern> = allowed_commands
    .iter()
    .map(|s| {
        self::pattern::CommandPattern::parse(s).map_err(|e| {
            format!("{}: invalid allowed_commands pattern '{}': {}", path.display(), s, e)
        })
    })
    .collect::<Result<_, _>>()?;
```

When constructing the **real** `HostContext` (around line 311), set the field. Replace:

```rust
let mut store = Store::new(
    &self.engine,
    HostContext::new_for_plugin(plugin_info.name.clone(), effective_capabilities),
);
```

with:

```rust
let mut host_ctx = HostContext::new_for_plugin(
    plugin_info.name.clone(),
    effective_capabilities,
);
host_ctx.allowed_commands = parsed_allowed_commands;
let mut store = Store::new(&self.engine, host_ctx);
```

`HostContext::allowed_commands` is `pub(super)`, so this works inside `mod plugin`.

- [ ] **Step 3: Update `load_from_config`**

In `pub fn load_from_config`, change the `load_one` call (around line 151) to pass `entry.allowed_commands`:

```rust
let entry_allowed_commands: Vec<String> = entry
    .allowed_commands
    .clone()
    .unwrap_or_default();
if let Err(e) = self.load_one(
    &path,
    env,
    config_caps,
    entry.cwasm_path.as_deref(),
    entry.cache_key.as_ref(),
    &entry_allowed_commands,
) {
    eprintln!("yosh: plugin: {}", e);
}
```

- [ ] **Step 4: Update `load_plugin` and `test_helpers`**

In `pub fn load_plugin` (around line 168), pass an empty slice:
```rust
pub fn load_plugin(&mut self, path: &Path, env: &mut ShellEnv) -> Result<(), String> {
    self.load_one(path, env, None, None, None, &[])
}
```

In `test_helpers::load_plugin_with_caps` (around line 603), add the parameter and forward it:
```rust
pub fn load_plugin_with_caps(
    manager: &mut PluginManager,
    path: &Path,
    env: &mut ShellEnv,
    caps: u32,
    allowed_commands: &[String],
) -> Result<(), String> {
    manager.load_one(path, env, Some(caps), None, None, allowed_commands)
}
```

In `test_helpers::load_plugin_with_cache` (around line 615), add the parameter:
```rust
pub fn load_plugin_with_cache(
    manager: &mut PluginManager,
    path: &Path,
    env: &mut ShellEnv,
    caps: u32,
    cwasm_path: &Path,
    expected_key: &super::cache::CacheKey,
    allowed_commands: &[String],
) -> Result<(), String> {
    manager.load_one(path, env, Some(caps), Some(cwasm_path), Some(expected_key), allowed_commands)
}
```

- [ ] **Step 5: Update existing `tests/plugin.rs` call sites**

`tests/plugin.rs` has 15 calls to `test_helpers::load_plugin_with_caps` / `load_plugin_with_cache` (verified via `grep -c`). Each needs an extra `&[]` argument as the new last parameter.

Run: `grep -n "load_plugin_with_caps\|load_plugin_with_cache" tests/plugin.rs`

For each occurrence, find the matching `)` (note: most calls span multiple lines, with the closing `)` followed by `.expect(...)` or `;`) and insert `, &[]` immediately before it.

Build after each edit if uncertain:

Run: `cargo build --features test-helpers --tests`
Expected: PASS once all 15 sites have been updated; the compile error messages name the exact file:line of any remaining un-updated call.

- [ ] **Step 6: Build and run all tests**

Run: `cargo build -p yosh && cargo test -p yosh plugin::`
Expected: PASS.

Run: `cargo test --features test-helpers --test plugin -- --test-threads=1`
Expected: PASS (all existing integration tests t01–t15 unaffected; they pass `&[]` for the new field).

Note: the integration test suite needs `cargo component build` for the wasm fixtures. If they aren't already built from Task 3, run those commands first.

- [ ] **Step 7: Commit**

```bash
git add src/plugin/mod.rs tests/plugin.rs
git commit -m "$(cat <<'EOF'
feat(plugin): thread allowed_commands through PluginManager::load_one

load_one now accepts a slice of pattern strings, parses them into
CommandPattern values via src/plugin/pattern.rs, and stashes the
parsed list on HostContext before plugin instantiation. Invalid
patterns abort plugin load with a clear error. load_from_config
sources the strings from PluginEntry.allowed_commands;
test_helpers::load_plugin_with_caps gains an &[String] parameter so
integration tests can build allowlists per case.

Spec §5, §9: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: SDK helper — `exec(program, args)`

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Add the SDK helper and re-exports**

Edit `crates/yosh-plugin-sdk/src/lib.rs`. After the existing `pub use self::yosh::plugin::files::{DirEntry, FileStat};` line, add:

```rust
pub use self::yosh::plugin::commands as host_commands;
pub use self::yosh::plugin::commands::ExecOutput;
```

After the `pub fn remove_dir_all(...)` function (the last existing helper), append:

```rust
// ── commands:exec helpers ────────────────────────────────────────────

/// Run an external command. Subject to the host's `commands:exec`
/// capability and `allowed_commands` allowlist, plus a 1000ms timeout.
///
/// Returns the captured stdout/stderr and exit code on a normal
/// process exit. Returns `Err(ErrorCode::PatternNotAllowed)` if the
/// argv is not in the plugin's allowlist, `Err(ErrorCode::Timeout)`
/// if the 1000ms cap is hit, `Err(ErrorCode::NotFound)` on PATH miss,
/// `Err(ErrorCode::Denied)` if the capability isn't granted.
pub fn exec(program: &str, args: &[&str]) -> Result<ExecOutput, ErrorCode> {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    host_commands::exec(program, &args_owned)
}
```

- [ ] **Step 2: Verify the SDK builds**

Run: `cargo build -p yosh-plugin-sdk`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(plugin-sdk): add exec() helper for commands:exec capability

Thin wrapper over host_commands::exec that takes &[&str] for ergonomic
argv construction. Re-exports ExecOutput and the host_commands module.

Spec §8: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Extend `test_plugin` with a `run-echo` command

**Files:**
- Modify: `tests/plugins/test_plugin/src/lib.rs`

- [ ] **Step 1: Add the new command and capability**

Edit `tests/plugins/test_plugin/src/lib.rs`.

Update the import to include the new SDK items:
```rust
use yosh_plugin_sdk::{
    Capability, ErrorCode, HookName, Plugin, exec, export, get_var, print, read_file, set_var,
    write_string,
};
```

In `fn commands(&self)`, append `"run-echo"` to the array:
```rust
fn commands(&self) -> &[&'static str] {
    &[
        "test_cmd",
        "echo_var",
        "trap_now",
        "dump_events",
        "set_post_exec_marker",
        "read-file",
        "write-file",
        "run-echo",
    ]
}
```

In `fn required_capabilities(&self)`, append `Capability::CommandsExec`:
```rust
fn required_capabilities(&self) -> &[Capability] {
    &[
        Capability::VariablesRead,
        Capability::VariablesWrite,
        Capability::Io,
        Capability::HookPreExec,
        Capability::HookOnCd,
        Capability::FilesRead,
        Capability::FilesWrite,
        Capability::CommandsExec,
    ]
}
```

In the `match command { ... }` block in `fn exec`, add the `"run-echo"` arm before the catch-all `_ => 127,`:

```rust
"run-echo" => {
    // Args are passed through as the command's argv tail. The host's
    // allowlist checks the full argv = ["echo", args...], so the
    // integration test sets `allowed_commands` accordingly.
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match exec("echo", &args_refs) {
        Ok(out) => {
            // Print stdout verbatim so the test can assert on it.
            let _ = print(&String::from_utf8_lossy(&out.stdout));
            out.exit_code
        }
        Err(ErrorCode::Denied)            => 100,
        Err(ErrorCode::PatternNotAllowed) => 101,
        Err(ErrorCode::Timeout)           => 102,
        Err(ErrorCode::NotFound)          => 103,
        Err(_)                            => 1,
    }
}
```

- [ ] **Step 2: Rebuild the wasm artifact**

Run:
```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
```
Expected: PASS. Confirms the SDK additions and the new command compile against the WIT.

- [ ] **Step 3: Commit**

```bash
git add tests/plugins/test_plugin/src/lib.rs
git commit -m "$(cat <<'EOF'
test(plugin): add run-echo command to test_plugin for commands:exec

New command exercises sdk::exec("echo", args). Maps each ErrorCode to
a distinct exit code (100=Denied, 101=PatternNotAllowed,
102=Timeout, 103=NotFound) so integration tests can assert which
guard fired.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Integration tests t16–t20

**Files:**
- Modify: `tests/plugin.rs`

- [ ] **Step 1: Add the five test cases**

Append to `tests/plugin.rs` (after the last existing test):

```rust
/// §10 t16 — `commands:exec` granted with matching pattern works.
#[test]
fn t16_commands_exec_granted_with_pattern_works() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_COMMANDS_EXEC | yosh_plugin_api::CAP_IO;
    test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        allowed,
        &["echo:*".to_string()],
    )
    .expect("load test_plugin with commands:exec + echo:* allowlist");

    let exec = mgr.exec_command(&mut env, "run-echo", &["hello".into()]);
    assert!(
        matches!(exec, PluginExec::Handled(0)),
        "run-echo with allowed pattern must Handled(0), got {:?}",
        exec
    );
}

/// §10 t17 — `commands:exec` denied without capability bit.
#[test]
fn t17_commands_exec_denied_without_capability() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    // No CAP_COMMANDS_EXEC bit — even with a matching pattern, the deny
    // stub fires.
    let allowed = yosh_plugin_api::CAP_IO;
    test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        allowed,
        &["echo:*".to_string()],
    )
    .expect("load without commands:exec");

    let exec = mgr.exec_command(&mut env, "run-echo", &["hi".into()]);
    assert!(
        matches!(exec, PluginExec::Handled(100)),
        "run-echo without capability must map to exit 100 (Denied), got {:?}",
        exec
    );
}

/// §10 t18 — `commands:exec` granted but pattern doesn't match.
#[test]
fn t18_commands_exec_pattern_not_allowed_without_match() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_COMMANDS_EXEC | yosh_plugin_api::CAP_IO;
    test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        allowed,
        // Allow `ls:*` but the plugin invokes `echo` — no match.
        &["ls:*".to_string()],
    )
    .expect("load with non-matching allowlist");

    let exec = mgr.exec_command(&mut env, "run-echo", &["hi".into()]);
    assert!(
        matches!(exec, PluginExec::Handled(101)),
        "run-echo without matching pattern must map to exit 101 (PatternNotAllowed), got {:?}",
        exec
    );
}

/// §10 t19 — exact-match pattern (no `:*`) rejects extra args.
#[test]
fn t19_commands_exec_exact_pattern_rejects_extra_args() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_COMMANDS_EXEC | yosh_plugin_api::CAP_IO;
    test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        allowed,
        // Exact-length pattern: argv must be EXACTLY ["echo"].
        &["echo".to_string()],
    )
    .expect("load with exact-length allowlist");

    // `run-echo hi` produces argv = ["echo", "hi"]; pattern "echo" only
    // matches argv = ["echo"], so this is rejected.
    let exec = mgr.exec_command(&mut env, "run-echo", &["hi".into()]);
    assert!(
        matches!(exec, PluginExec::Handled(101)),
        "run-echo with extra args under exact pattern must map to exit 101, got {:?}",
        exec
    );
}

/// §10 t20 — invalid pattern fails plugin load.
#[test]
fn t20_commands_exec_invalid_pattern_fails_plugin_load() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_COMMANDS_EXEC | yosh_plugin_api::CAP_IO;
    let result = test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        allowed,
        // Pattern body is empty after stripping `:*` — should error.
        &[":*".to_string()],
    );
    assert!(
        result.is_err(),
        "load_plugin_with_caps should fail on invalid pattern, got Ok"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("invalid allowed_commands pattern"),
        "error must mention the offending field, got: {}",
        err
    );
}
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test --features test-helpers --test plugin t16 t17 t18 t19 t20 -- --test-threads=1`
Expected: PASS (5 tests).

- [ ] **Step 3: Run the full integration suite for regressions**

Run: `cargo test --features test-helpers --test plugin -- --test-threads=1`
Expected: PASS (existing t01–t15 + new t16–t20).

- [ ] **Step 4: Commit**

```bash
git add tests/plugin.rs
git commit -m "$(cat <<'EOF'
test(plugin): add commands:exec integration tests t16-t20

t16: capability + matching pattern → Handled(0)
t17: no capability bit → exit 100 (Denied via deny stub)
t18: capability granted but pattern doesn't match → exit 101 (PatternNotAllowed)
t19: exact-length pattern rejects extra argv tokens → exit 101
t20: invalid pattern aborts plugin load with clear error

Spec §10: docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Final verification

**Files:**
- None modified; runs the full test suite end-to-end.

- [ ] **Step 1: Run the full default-members build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 2: Run the full default-members test suite**

Run: `cargo test`
Expected: PASS. Covers yosh-plugin-api, yosh-plugin-sdk, yosh-plugin-manager, and yosh.

- [ ] **Step 3: Run the wasm plugin builds and integration tests**

Run:
```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin -- --test-threads=1
```
Expected: PASS.

- [ ] **Step 4: Confirm no E2E regressions**

Run: `./e2e/run_tests.sh`
Expected: PASS (no plugin-related cases here, but the binary must still link and run).

- [ ] **Step 5: Verify clippy / rustfmt cleanliness on the changed files (optional but encouraged)**

Run:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
Expected: clean. Fix anything new in just the touched files.

- [ ] **Step 6: No commit needed if Step 5 was clean. If clippy/fmt produced changes, commit them**

```bash
git add -u
git commit -m "$(cat <<'EOF'
style(plugin): apply rustfmt / clippy fixes for commands:exec branch

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
