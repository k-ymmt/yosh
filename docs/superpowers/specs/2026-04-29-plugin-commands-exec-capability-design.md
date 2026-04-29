# Plugin `commands:exec` Capability

## Goal

Add a new plugin capability — `commands:exec` — that lets WASM plugins
run external commands on the host, gated by a per-plugin allowlist of
glob-style argv patterns. The driving use case is a git-aware prompt
plugin that needs `git status --porcelain` (or similar) to count staged
and unstaged files. `.git/index` is a binary format, so reading it via
`files:read` is not a realistic substitute.

## Non-Goals

- Glob/regex patterns beyond a fixed `<argv-prefix>` and optional trailing
  `:*` "wildcard suffix". No middle-position wildcards, no character
  classes, no negation.
- Shell builtin invocation. `exec` runs external programs only;
  builtins live behind their own host imports (e.g., `variables:write`
  already covers `export`).
- Plugin-side overrides for `cwd`, `env`, or `stdin`. v1 always inherits
  shell `cwd` + full env, and feeds `/dev/null` to `stdin`.
- Per-plugin timeout configuration. v1 has a hard-coded 1000ms timeout.
- Streaming stdout/stderr via WIT `resource` handles. Whole-buffer
  capture is enough for the prompt use case.
- Concurrent `exec` calls from a single plugin. The wasmtime `call`
  surface is synchronous per plugin instance, so concurrency is not
  reachable without a deeper async refactor.
- Per-pattern denylist (`deny-commands`). The allowlist is sufficient
  for the threat model; deny is additive future work.

## Threat Model

Plugins are **trusted code** — the user explicitly opts in by editing
`plugins.toml` or running `yosh-plugin install`. The defence we are
building is against *unintended* permission grants, not against
malicious plugins. Mitigations:

1. **Default-deny in three layers** (described in §7).
2. **Two-stage gating**: the `commands:exec` bit must be granted AND
   `allowed_commands` must contain a matching pattern. Granting the bit
   without populating `allowed_commands` lets the plugin call `exec`
   but every call returns `Err(pattern-not-allowed)`.
3. **Hard 1000ms timeout** — protects against `git status` hanging on a
   slow filesystem from freezing the prompt indefinitely.
4. **No shell injection surface**: `exec` takes `program: string` and
   `args: list<string>` (argv tokens). The host never invokes a shell;
   `args` go straight to `Command::args`, so embedded `;`, `|`, `$()`,
   etc. are inert.
5. **Documentation warning**: `commands:exec` is the most powerful
   capability shipped so far. Even with a tight allowlist, `git:*`
   gives access to `git config --global ...`, `git push`, etc. Users
   should write the narrowest pattern that the plugin actually needs
   (e.g., `git status:*` instead of `git:*`).

A complete sandbox would need argv-content scanning, environment
scrubbing, or seccomp-style syscall filtering. Those are deliberately
out of scope; the design is structured so they can layer on top
(§10 Open Questions).

## 1. Architecture Overview

The `commands:exec` capability follows the same default-deny three-layer
shape as the existing `files:*` and `variables:*` pairs, with one new
moving part: **the allowlist pattern matcher**.

- **WIT** (`crates/yosh-plugin-api/wit/yosh-plugin.wit`): new
  `interface commands { ... }`; `world plugin-world` gains
  `import commands`. `error-code` enum extended with `timeout` and
  `pattern-not-allowed`.
- **Capability bit** (`crates/yosh-plugin-api/src/lib.rs`):
  `CAP_COMMANDS_EXEC = 0x400` (next free bit after `CAP_FILES_WRITE =
  0x200`). New `Capability::CommandsExec` variant. String mapping
  `"commands:exec"` in `parse_capability`, `Capability::as_str`,
  `Capability::to_bitflag`. Added to `CAP_ALL`.
- **Pattern type** (new `src/plugin/pattern.rs`): `CommandPattern`
  struct + parser + matcher. Owns the textual pattern grammar so
  `host.rs` and `config.rs` both stay focused.
- **Config parser** (`src/plugin/config.rs`):
  - `PluginEntry` gains `allowed_commands: Option<Vec<String>>`.
  - `capability_from_str` gains `"commands:exec"` arm.
  - At plugin-load time, the raw strings are parsed into
    `Vec<CommandPattern>` and stored on `HostContext`.
- **Host implementation** (`src/plugin/host.rs`):
  - `HostContext` gains `allowed_commands: Vec<CommandPattern>`.
  - `host_commands_exec` (real impl) + `deny_commands_exec` (deny stub).
  - Pattern check + `std::process::Command` spawn + 1000ms timeout +
    SIGTERM-then-SIGKILL escalation.
- **Linker wiring** (`src/plugin/linker.rs`): new
  `yosh:plugin/commands@0.1.0` instance, real-vs-deny chosen by
  `CAP_COMMANDS_EXEC`.
- **SDK helper** (`crates/yosh-plugin-sdk/src/lib.rs`): thin
  `exec(program, args) -> Result<ExecOutput, ErrorCode>` wrapper.

## 2. WIT Interface

Added to `crates/yosh-plugin-api/wit/yosh-plugin.wit`:

```wit
interface types {
    enum error-code {
        denied,
        invalid-argument,
        io-failed,
        not-found,
        other,
        timeout,                // new (appended for ABI stability)
        pattern-not-allowed,    // new (appended for ABI stability)
    }
    /* … existing io-stream / hook-name / plugin-info unchanged … */
}

New variants are **appended** rather than inserted before `other`.
WIT enum discriminants are positional, so inserting in the middle would
shift `other`'s wire encoding from 4 to 6 and break already-compiled
plugins.

interface commands {
    use types.{error-code};

    /// Result of a successful (or process-exit) command run.
    /// Extended in the future by adding new functions, never by
    /// changing this record's shape.
    record exec-output {
        exit-code: s32,
        stdout: list<u8>,
        stderr: list<u8>,
    }

    /// Run an external program with the given argv, capturing
    /// stdout/stderr and returning the exit code.
    ///
    /// Subject to a 1000ms hard timeout enforced by the host.
    /// Subject to the per-plugin `allowed_commands` pattern allowlist.
    /// CWD is the shell's current directory; environment is the
    /// shell's full environment; stdin is `/dev/null`.
    exec: func(program: string, args: list<string>) -> result<exec-output, error-code>;
}

world plugin-world {
    import variables;
    import filesystem;
    import files;
    import io;
    import commands;       // new
    export plugin;
    export hooks;
}
```

Design notes:

- **Reuse `error-code`** rather than introducing a command-specific
  error type. Two new variants:
  - `timeout` — the 1000ms host-enforced cap was hit.
  - `pattern-not-allowed` — `commands:exec` is granted but no entry in
    `allowed_commands` matched the requested argv. Distinguished from
    `denied` (which means the bit itself is missing) so plugin authors
    can debug "did I misconfigure my pattern?" vs "did I forget the
    capability entirely?".
- **`program` is a plain string**, passed unchanged to
  `Command::new`. PATH search uses the host's standard rules. Pattern
  matching also uses the literal string; no PATH resolution or
  basename normalization (§5).
- **`args` is `list<string>`** (argv tokens). The host never invokes a
  shell. Embedded shell metacharacters in args are literal data.
- **`exec-output` always returns** stdout/stderr/exit on a normal
  process exit — including non-zero exits. Failure modes (denied,
  not-found, timeout) come back as the `Err` arm.
- **No streaming**. Whole-buffer capture matches the prompt-style use
  case (small outputs, fully consumed by the plugin).

## 3. Capability Bits & String Parsing

`crates/yosh-plugin-api/src/lib.rs`:

```rust
pub const CAP_VARIABLES_READ:  u32 = 0x001;
pub const CAP_VARIABLES_WRITE: u32 = 0x002;
pub const CAP_FILESYSTEM:      u32 = 0x004;
pub const CAP_IO:              u32 = 0x008;
pub const CAP_HOOK_PRE_EXEC:   u32 = 0x010;
pub const CAP_HOOK_POST_EXEC:  u32 = 0x020;
pub const CAP_HOOK_ON_CD:      u32 = 0x040;
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x080;
pub const CAP_FILES_READ:      u32 = 0x100;
pub const CAP_FILES_WRITE:     u32 = 0x200;
pub const CAP_COMMANDS_EXEC:   u32 = 0x400;   // new

pub const CAP_ALL: u32 =
    /* existing ten bits */
    | CAP_COMMANDS_EXEC;

pub enum Capability {
    /* existing variants */
    CommandsExec,
}
```

String mappings extended in `parse_capability`, `Capability::as_str`,
`Capability::to_bitflag`, and `src/plugin/config.rs::capability_from_str`:

```text
"commands:exec" ↔ Capability::CommandsExec ↔ CAP_COMMANDS_EXEC
```

`u32` still has 21 spare bits.

## 4. Pattern Grammar & Matcher

New module `src/plugin/pattern.rs` owns the pattern type, parser, and
matcher.

### Grammar

A pattern is one of two forms:

| Form              | Meaning                                                  |
|-------------------|----------------------------------------------------------|
| `tok1 tok2 … tokN` | argv must be **exactly** `[tok1, tok2, …, tokN]`         |
| `tok1 tok2 … tokN:*` | argv must **start with** `[tok1, tok2, …, tokN]` (then anything, including nothing) |

Tokens are split on ASCII whitespace. The trailing `:*` is the only
wildcard form; it must be the last characters of the pattern string and
attaches to the final token (i.e., `git log:*` parses as
tokens=`["git", "log"]`, glob_suffix=true).

Empty patterns and patterns whose only content is `:*` are rejected at
parse time as `Err`.

### Type

```rust
pub struct CommandPattern {
    pub tokens: Vec<String>,
    pub has_glob_suffix: bool,
}

impl CommandPattern {
    pub fn parse(s: &str) -> Result<Self, String> { /* … */ }

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
```

### Examples

| Pattern        | argv                                         | Match? |
|----------------|----------------------------------------------|--------|
| `git:*`        | `["git"]`                                    | yes    |
| `git:*`        | `["git", "status"]`                          | yes    |
| `git:*`        | `["git", "log", "-p"]`                       | yes    |
| `git:*`        | `["/usr/bin/git", "status"]`                 | no (literal compare) |
| `git status:*` | `["git", "status"]`                          | yes    |
| `git status:*` | `["git", "status", "--porcelain"]`           | yes    |
| `git status:*` | `["git", "log"]`                             | no     |
| `git status`   | `["git", "status"]`                          | yes    |
| `git status`   | `["git", "status", "--porcelain"]`           | no     |

### Allowlist evaluation

`HostContext::allowed_commands: Vec<CommandPattern>`. A request is
allowed iff **at least one** pattern matches. Empty `allowed_commands`
(either `None` in the config or empty array) means no pattern matches,
so every `exec` returns `Err(pattern-not-allowed)`.

## 5. Host Implementation

Added to `src/plugin/host.rs`. Both real impl and deny stub respect the
`env_mut().is_none()` metadata-contract guard.

```rust
// ── yosh:plugin/commands host imports ────────────────────────────────

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

    let argv: Vec<String> = std::iter::once(program.clone())
        .chain(args.iter().cloned())
        .collect();

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    spawn_with_timeout(&program, &args, Duration::from_millis(1000))
}

pub(super) fn deny_commands_exec(
    _ctx: &mut HostContext,
    _program: String,
    _args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

`spawn_with_timeout` (private helper inside `host.rs`):

1. `Command::new(program)`
   - `.args(args)`
   - `.stdin(Stdio::null())`
   - `.stdout(Stdio::piped())`
   - `.stderr(Stdio::piped())`
   - CWD and env are inherited (no overrides).
2. `child = cmd.spawn().map_err(map_io)?`
3. Spawn a thread to `child.wait_with_output()`; communicate via
   `mpsc::channel`. Main thread `recv_timeout(1000ms)`.
4. On timeout:
   - Send SIGTERM to the child for graceful shutdown. `Child::kill`
     sends SIGKILL on Unix, so use the project's existing low-level
     signal helper (`crate::signal::*` or whatever wraps `libc::kill`
     in this codebase) to send SIGTERM specifically. Implementation
     should pick the same crate the rest of `src/exec/` already uses.
   - Wait up to 100ms for graceful exit (poll
     `child.try_wait()` in a tight loop, or reuse the `mpsc` channel).
   - If still running, `child.kill()` for hard SIGKILL.
   - Join the worker thread to avoid handle leaks (the worker will
     return promptly once the child exits) and return `Err(Timeout)`.
5. On normal completion: assemble `ExecOutput { exit_code, stdout,
   stderr }` and return `Ok`.

Error mapping for the spawn-side `io::Error`:

| Condition                              | `ErrorCode`           |
|----------------------------------------|-----------------------|
| `env_mut()` is `None`                  | `Denied`              |
| Empty program                          | `InvalidArgument`     |
| No matching pattern                    | `PatternNotAllowed`   |
| `io::ErrorKind::NotFound` (PATH miss)  | `NotFound`            |
| Other `io::Error` from `spawn`         | `IoFailed`            |
| Timeout enforcement                    | `Timeout`             |

Generated `ExecOutput` comes from
`super::generated::yosh::plugin::commands::ExecOutput` (wit-bindgen
output) and is imported into `host.rs` like `IoStream` already is.

### `HostContext` change

```rust
pub struct HostContext {
    pub env: Option<*mut ShellEnv>,    // existing
    pub allowed_commands: Vec<CommandPattern>,   // new
    /* … */
}
```

`Vec<CommandPattern>` is `Default::default()` (empty) for plugins that
don't request `commands:exec`. The plugin loader populates it from
`PluginEntry::allowed_commands` when constructing the context.

## 6. Linker Wiring

Added to `src/plugin/linker.rs`, immediately after the existing
`yosh:plugin/files` block:

```rust
let mut commands = linker.instance("yosh:plugin/commands@0.1.0")?;

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

`use` additions: `super::host::{host_commands_exec, deny_commands_exec}`
and `yosh_plugin_api::CAP_COMMANDS_EXEC`.

The existing `linker_construction_smoke` test exercises both `0` and
`CAP_ALL`. Updating `CAP_ALL` to include `CAP_COMMANDS_EXEC` (§3) makes
the new wiring covered automatically.

## 7. Default-Deny Layers

Three independent layers must all line up before a command runs:

1. **WIT layer**: if the plugin's source omits `import commands`, the
   compiled wasm has no symbols for `exec` at all.
2. **Linker layer**: if the granted bitfield lacks `CAP_COMMANDS_EXEC`,
   `exec` is bound to `deny_commands_exec` and returns `Err(Denied)`
   immediately — the pattern allowlist is never consulted.
3. **Host pattern layer**: even with the bit granted, the `exec` host
   call returns `Err(PatternNotAllowed)` unless `argv` matches at
   least one entry in `allowed_commands`.

Plus the cross-cutting:

4. **Metadata-contract**: `env_mut().is_none()` short-circuits to
   `Err(Denied)`, blocking `exec` calls from inside `metadata()`.

Capability negotiation is unchanged: the plugin declares
`required-capabilities = ["commands:exec"]` in `plugin-info`, the user
allows `capabilities = ["commands:exec"]` in `plugins.toml`, and the
effective grant is the bitwise AND. Omitting `capabilities` in
`plugins.toml` still means "trust everything the plugin asked for"
— but `allowed_commands` is **never** implicit; it must be enumerated
explicitly.

## 8. SDK Helpers

`crates/yosh-plugin-sdk/src/lib.rs`:

```rust
pub use self::yosh::plugin::commands as host_commands;
pub use self::yosh::plugin::commands::ExecOutput;

/// Run an external command. Subject to the host's `commands:exec`
/// capability and `allowed_commands` allowlist, plus a 1000ms timeout.
///
/// Returns the captured stdout/stderr and exit code on a normal
/// process exit. Returns `Err(ErrorCode::PatternNotAllowed)` if
/// `argv` is not in the plugin's allowlist, `Err(ErrorCode::Timeout)`
/// if the 1000ms cap is hit, `Err(ErrorCode::NotFound)` on PATH miss,
/// `Err(ErrorCode::Denied)` if the capability isn't granted.
pub fn exec(program: &str, args: &[&str]) -> Result<ExecOutput, ErrorCode> {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    host_commands::exec(program, &args_owned)
}
```

## 9. Config Parsing

`src/plugin/config.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct PluginEntry {
    /* existing fields */
    #[serde(default)]
    pub allowed_commands: Option<Vec<String>>,    // new
}
```

`capability_from_str` gains:

```rust
"commands:exec" => Some(yosh_plugin_api::CAP_COMMANDS_EXEC),
```

Plugin loader (where `HostContext` is constructed):

```rust
let allowed_commands: Vec<CommandPattern> = entry
    .allowed_commands
    .as_deref()
    .unwrap_or(&[])
    .iter()
    .map(|s| CommandPattern::parse(s))
    .collect::<Result<_, _>>()
    .map_err(|e| format!("plugin '{}': invalid pattern: {}", entry.name, e))?;
```

Invalid pattern strings fail plugin load with a clear error (rather
than silently dropping like unknown capability bits do today). Rationale:
unknown capabilities degrade gracefully because newer plugins may
request bits an older yosh doesn't know about; `allowed_commands`
strings are user-authored and a typo (`gti:*`) is almost always a bug.

The TOML key is `allowed_commands` (snake_case) to match the existing
convention used by `cwasm_path` and `cache_key` on the same struct.
No `serde(rename)` attribute is needed.

## 10. Tests

**Pattern matcher unit tests** (`src/plugin/pattern.rs::tests`):

- `parse_glob_suffix_separates_tokens`: `"git log:*"` → tokens=`["git",
  "log"]`, glob=true
- `parse_no_suffix_is_exact`: `"git log"` → tokens=`["git", "log"]`,
  glob=false
- `parse_empty_string_errors`
- `parse_lone_glob_suffix_errors`: `":*"` → `Err`
- `match_glob_suffix_zero_extra`: `git:*` matches `["git"]`
- `match_glob_suffix_many_extra`: `git:*` matches `["git", "a", "b"]`
- `match_exact_requires_equal_length`
- `match_literal_compare`: `git:*` does NOT match `["/usr/bin/git"]`

**Host unit tests** (`src/plugin/host.rs::tests`):

- `host_commands_exec_metadata_contract_denied_when_env_null`
- `host_commands_exec_invalid_argument_on_empty_program`
- `host_commands_exec_pattern_not_allowed_when_no_match`
- `host_commands_exec_runs_when_pattern_matches`: spawn `/bin/echo
  hello`, assert stdout=`b"hello\n"`, exit=0
- `host_commands_exec_captures_stderr_separately`: spawn a shell
  one-liner that writes to stderr, assert stderr non-empty / stdout
  empty
- `host_commands_exec_propagates_nonzero_exit`
- `host_commands_exec_returns_not_found_for_missing_binary`
- `host_commands_exec_timeout_after_1000ms`: spawn `/bin/sleep 5`,
  assert `Err(Timeout)` returned in <1500ms
- `host_commands_exec_kills_child_on_timeout`: same as above, plus
  assert no `sleep` process remains after the call (best-effort —
  reap via wait status, not pgrep)

These tests use real `/bin/echo` / `/bin/sleep` paths; they're macOS+Linux
stable. The host tests run with the existing `cfg(test)` `null_env_ctx`
helper plus a `with_allowed_commands(...)` builder.

**Linker tests**: `linker_construction_smoke` auto-covers the new
wiring once `CAP_ALL` includes `CAP_COMMANDS_EXEC`.

**Integration tests** (`tests/plugin.rs` + `tests/plugins/test_plugin/`):

Add one command to `test_plugin`:

- `run-echo <args...>` → calls `sdk::exec("echo", args)`, prints
  stdout/exit, maps errors to non-zero exit codes (e.g., 100 for
  `Denied`, 101 for `PatternNotAllowed`, 102 for `Timeout`,
  103 for `NotFound`).

Add new test cases:

(Renumbered to t20-t24 at implementation time because t16-t19 are
already in use by the `files:read`/`files:write` integration suite.)

- `t20_commands_exec_granted_with_pattern_works`: capability +
  `allowed_commands = ["echo:*"]`, assert exit code 0. Stdout
  byte-level assertion is covered by the host-side unit test
  `host_commands_exec_runs_when_pattern_matches` because the
  integration harness does not capture host stdout.
- `t21_commands_exec_denied_without_capability`: no capability,
  assert exit code reflects `Denied`
- `t22_commands_exec_pattern_not_allowed_without_match`: capability
  granted, `allowed_commands = ["ls:*"]`, attempt `echo`, assert exit
  code reflects `PatternNotAllowed`
- `t23_commands_exec_exact_pattern_rejects_extra_args`:
  `allowed_commands = ["echo"]` (no `:*`), attempt `echo hello`,
  assert `PatternNotAllowed`
- `t24_commands_exec_invalid_pattern_fails_plugin_load`:
  `allowed_commands = [":*"]` causes plugin-load error

`tests/plugins/test_plugin/Cargo.toml` (the wit metadata) gets
`required-capabilities` extended with `commands:exec`. Per the
workspace caveat in CLAUDE.md, the test plugin is rebuilt with
`cargo component build -p test_plugin --target wasm32-wasip2 --release`,
and integration tests run with `--features test-helpers`.

The timeout integration test (`/bin/sleep 5` end-to-end) is **omitted**
from `tests/plugin.rs` to keep the integration suite fast; the host
unit test already covers the timeout path.

**Out of scope**: e2e tests (plugins are excluded from `e2e/`),
benchmarks (process spawn cost dwarfs wasmtime call cost — meaningless
to bench here).

## 11. Versioning & Compat

- WIT package version stays at `yosh:plugin@0.1.0` — adding new
  interfaces/imports and adding new `error-code` enum variants are
  *minor* surface changes. Existing plugins that do not `import
  commands` continue to instantiate cleanly.
- Adding enum variants to `error-code` is backward-compatible at the
  WIT level (consumers must already handle the catch-all `other`
  arm), but plugin-side match exhaustiveness will break on rebuild.
  Plugins that exhaustively match `error-code` will need to add
  `Timeout` and `PatternNotAllowed` arms — surface in release notes.
- `CAP_ALL` widens; only matters to call sites that build a capability
  mask from `CAP_ALL`, all of which live inside this repo.
- `plugins.toml` parser silently ignores unknown capability strings
  (existing behavior in `capabilities_from_strs`), so older yosh
  binaries reading a v0.2.x `plugins.toml` with `"commands:exec"` will
  drop the unknown bit. **However**, an older yosh will also silently
  ignore the new `allowed_commands` field (serde `#[serde(default)]`
  on unknown fields), which means the plugin gets neither the
  capability nor the allowlist — a fail-safe degradation. Document
  this in the release notes.
- New `error-code` variants on the host side: any older plugin using
  the host-side trait dispatch will not see `Timeout` /
  `PatternNotAllowed` because those code paths only fire from the new
  `commands:exec` host impl. Existing capabilities are untouched.

## 12. Open Questions / Future Work

- **Per-plugin timeout configuration** in `plugins.toml`
  (`exec-timeout-ms = 500`). Useful when an explicit-invocation plugin
  (e.g., `yosh-plugin git-status` user command) wants a longer cap
  than the prompt-style 1000ms default. Additive: the v1 host already
  takes a `Duration`, so the wiring is small.
- **Plugin-supplied timeout** as an `exec` argument
  (`exec-with-timeout: func(... timeout-ms: u32) -> ...`). Useful for
  multi-call workflows where each call wants its own bound. Subject
  to a hard upper bound from `plugins.toml` to prevent a malicious
  plugin from passing `u32::MAX`.
- **`stdin` / `cwd` / `env` overrides**. Add a parallel
  `exec-detailed: func(opts: exec-options) -> ...` that takes a record
  of optional fields. Keep the simple `exec` for the 90% case.
- **Basename-mode pattern matching** (`/usr/bin/git` matched by
  `git:*`). Add as a per-pattern flag (`/git:*` for absolute,
  `git:*` for basename, current behavior `git:*` for literal) or as
  a global plugin-level switch. Defer until a real use case requests
  it.
- **`deny-commands` field**. Useful for `allowed_commands = ["git:*"]`
  but `deny-commands = ["git push:*", "git config --global:*"]`.
  Evaluation order: deny wins over allow. Defer until allowlist alone
  proves insufficient.
- **Streaming stdout/stderr** via WIT `resource` handles. Useful for
  long-running commands where the plugin wants to surface progress.
  Not in scope for v1; the prompt use case captures a few KB at most.
- **Argument-content scanning**. Even with `git status:*`, a plugin
  could pass `git status --porcelain --git-dir=/tmp/evil`. Per-flag
  allowlists (`--git-dir` blocked, `--porcelain` allowed) would be a
  separate sub-DSL. Probably premature.
- **Process group / terminal control**. `exec` doesn't put the child
  in its own process group; SIGINT to the shell could propagate.
  Acceptable for prompt-style short-lived calls, but worth a
  `setpgid` if longer-running invocations land later.
- **Global timeout vs pre-prompt-hook timeout**. The general
  pre-prompt timeout TODO ("Pre-prompt hook timeout — protect against
  slow `pre_prompt` plugins blocking prompt display") is orthogonal:
  it caps the *plugin call*, this design caps the *child process*.
  Both bounds compose (plugin timeout >= sum of `exec` timeouts in
  worst case). Revisit if they conflict in practice.
