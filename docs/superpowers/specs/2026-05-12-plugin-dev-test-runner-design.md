# Plugin Development: Local Run & Declarative Test Runner

Date: 2026-05-12
Status: Draft (awaiting user review)

## 1. Goal

Make yosh plugin development testable and runnable locally without
spinning up a full shell session, and without tying plugin authors to
Rust. Two new `yosh-plugin` subcommands cover the workflow:

- `yosh plugin run <wasm> exec|hook ...` — one-shot invocation against a
  controlled, in-memory host. For ad-hoc smoke tests and CI scripts in
  any language.
- `yosh plugin test [path]` — declarative scenarios in TOML, executed in
  batch with pass/fail reporting. Mirrors the role of yosh's existing
  `e2e/run_tests.sh` but for plugins.

Both subcommands share a new `TestCtx` that implements the
`yosh:plugin/*` host imports against an in-memory test state, so they
exercise the real `wasmtime` boundary that the shell uses at runtime.

This work is language-agnostic by construction: the scenario format and
CLI surface have no Rust assumptions. Plugins written in any language
that targets the WebAssembly Component Model can use the same harness.

## 2. Non-goals

- A Rust-specific mock SDK (`yosh_plugin_sdk::testing`). The wasm
  boundary is the contract worth testing; mocking it in Rust would only
  help one language and would drift from the real host.
- Interactive REPL (`yosh plugin repl <wasm>`). Not useful in CI; can be
  layered on later if needed.
- Hot reload / watch mode in the shell session. Out of scope; the
  separate `yosh plugin run` invocation makes the dev loop fast enough
  for the iteration target this spec addresses.
- Behavioral changes to `src/plugin/` (yosh runtime). All new code
  lives in `crates/yosh-plugin-manager`. The only edit inside
  `src/plugin/` is moving `pattern.rs` into `crates/yosh-plugin-api`
  and re-exporting it from the original location (no semantic change).

## 3. Architecture

### 3.1 Host contexts

yosh already has two `wasmtime` host contexts. This spec adds a third:

| Context | Crate | Backed by | Used by |
|---------|-------|-----------|---------|
| `HostContext` (existing) | yosh | `*mut ShellEnv` via `with_env` RAII | shell runtime |
| `MetadataCtx` (existing) | yosh-plugin-manager | all-deny | `metadata` extraction during `sync` |
| `TestCtx` (**new**) | yosh-plugin-manager | in-memory `TestState` | `run` / `test` subcommands |

`TestCtx` mirrors the precedent of `MetadataCtx`: a self-contained
context type, an empty `WasiCtx` for WASI isolation, and a `Linker`
constructed via the bindgen-generated `add_to_linker`. The empty
`WasiCtx` reuses the same rationale as §6 of the metadata extract
module (cargo-component-built guests pull in `wasi:io` / `wasi:cli`
transitively; the empty context returns empty data rather than failing
at link time).

The two existing host implementations remain untouched. Consolidating
the three contexts onto a shared `HostBackend` trait is left as a
future TODO (the same way `metadata_extract.rs` is flagged today).

### 3.2 TestState

```rust
struct TestState {
    caps: u32,                              // bitmask, same shape as HostContext
    vars: HashMap<String, String>,
    exported: HashSet<String>,
    cwd: PathBuf,                           // virtual; never touches process cwd
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    files: HashMap<PathBuf, Vec<u8>>,       // sandboxed virtual FS
    sandbox_root: Option<PathBuf>,          // real FS access scope, if any
    allow_exec: Vec<CommandPattern>,        // commands:exec allowlist
    exec_log: Vec<ExecRecord>,
    set_log: Vec<(String, String)>,
    export_log: Vec<(String, String)>,
    write_log: Vec<(PathBuf, usize)>,
    deadline_ms: u64,
}
```

Two filesystem modes:
- **Virtual** (default): `read_file` / `write_file` / `create_dir` read
  and mutate `files`. Useful for self-contained scenarios.
- **Sandbox** (`--sandbox-root <path>`): the same calls hit the real
  filesystem but are confined to that root (path canonicalization +
  prefix check). Useful when the plugin must interact with real
  binaries seeded into the sandbox.

`exec` always uses real `std::process::Command`. Allowlist matching
reuses `CommandPattern`, which moves from `src/plugin/pattern.rs` to
`crates/yosh-plugin-api/src/pattern.rs` (re-exported from the original
location for backward compatibility).

### 3.3 New manager modules

```
crates/yosh-plugin-manager/src/
  test_host.rs    # TestCtx, TestState, Host trait impls
  runner.rs       # invoke_exec, invoke_hook, scenario execution
  scenario.rs     # TOML schema + parser + expectation evaluator
```

`yosh plugin run` and `yosh plugin test` are thin CLI shims that
construct `TestState`, drive `runner.rs`, and format output.

## 4. CLI surface

### 4.1 `yosh plugin run`

```
yosh plugin run <wasm> exec <command> [args...]
yosh plugin run <wasm> hook pre-exec  <command-line>
yosh plugin run <wasm> hook post-exec <command-line> <exit-code>
yosh plugin run <wasm> hook on-cd     <old> <new>
yosh plugin run <wasm> hook pre-prompt
```

Common flags:

| Flag | Meaning | Default |
|------|---------|---------|
| `--cap <list>` | comma-separated capabilities to grant (same strings as `plugins.toml` `capabilities`, e.g. `variables:read,io,commands:exec`) | plugin's declared `required_capabilities` |
| `--var KEY=VAL` | seed a shell variable (repeatable) | none |
| `--export KEY=VAL` | seed an exported variable (repeatable) | none |
| `--cwd <path>` | virtual cwd | `.` |
| `--allow-exec <pat>` | allow this `commands:exec` pattern (repeatable) | none (= all denied) |
| `--sandbox-root <path>` | scope real-FS file access to this root | unset (virtual FS) |
| `--timeout <ms>` | epoch deadline for the whole invocation | 5000 |
| `--format <human\|json>` | output format | `human` |

Exit code policy:

| Code | Meaning |
|------|---------|
| 0..=255 | plugin's own `exec` exit code (passed through) |
| 99 | harness-level error (trap / capability denied / timeout / wasm load failure); `--format json` carries `error.kind` for the specific category |
| 2 | clap argument error |

Harness-level errors are always printed to stderr in human form and
always exit with 99 (not the plugin's exit code). This avoids
colliding with legitimate plugin exit codes in the 120–127 range,
which POSIX shells already use for signal/not-found semantics. The
specific category is preserved in the JSON output and in the human
stderr line (e.g. `yosh-plugin: trap: unreachable executed`).

Human output:

```
[stdout]
<bytes plugin wrote via print()>
[stderr]
<bytes plugin wrote via eprint()>
[exit] 0
[vars set]    FOO=bar
[vars export] PATH=/usr/local/bin
[files write] /tmp/out.txt (17 bytes)
[exec]        echo hello → exit 0 (3 bytes stdout)
```

`json` output emits the same fields as a single JSON object per
invocation:

```json
{
  "exit": 0,
  "stdout": "...",
  "stderr": "...",
  "vars_set":    [{"key":"FOO", "value":"bar"}],
  "vars_export": [{"key":"PATH","value":"/usr/local/bin"}],
  "files_write": [{"path":"/tmp/out.txt","bytes":17}],
  "exec":        [{"program":"echo","args":["hello"],"exit":0,"stdout_bytes":3}],
  "error":       null
}
```

### 4.2 `yosh plugin test`

```
yosh plugin test [path]             # default: tests/
yosh plugin test --filter <regex>
yosh plugin test --format json
```

Walks the given directory for `*.toml` files, executes each scenario
serially, and reports a summary. Exit code 0 iff all scenarios pass.

`--format json` produces one JSON line per scenario plus a final
summary line:

```jsonl
{"file":"tests/echo_var.toml","status":"pass","steps":2}
{"file":"tests/on_cd_writes.toml","status":"fail","step":2,"reason":"vars_set mismatch","expected":{"LAST_CD":"/home"},"got":{}}
{"summary":{"passed":6,"failed":1,"total":7}}
```

## 5. Scenario file format

One scenario per `.toml` file.

```toml
plugin      = "../target/wasm32-wasip2/release/my_plugin.wasm"
description = "echo_var prints the named variable"

[env]
caps         = ["variables:read", "io"]
vars         = { GREETING = "hello" }
exported     = []
cwd          = "/tmp"
allow_exec   = []                   # patterns for commands:exec
sandbox_root = ""                   # empty = virtual FS
timeout_ms   = 5000

# Seed virtual FS contents (only when sandbox_root is empty).
[files]
"/tmp/in.txt" = "seed contents\n"

[[step]]
call = "exec"
args = ["echo_var", "GREETING"]

  [step.expect]
  exit   = 0
  stdout = "hello\n"

[[step]]
call = "hook"
name = "on-cd"
args = ["/tmp", "/home"]

  [step.expect]
  vars_set = { LAST_CD = "/home" }
```

`call` values:

| call | name | args |
|------|------|------|
| `exec` | — | `[command, args...]` |
| `hook` | `pre-exec` | `[command-line]` |
| `hook` | `post-exec` | `[command-line, exit-code]` |
| `hook` | `on-cd` | `[old, new]` |
| `hook` | `pre-prompt` | `[]` |

Supported `expect` keys:

| Key | Meaning |
|-----|---------|
| `exit` | plugin exit code (exec only) |
| `stdout` / `stderr` | exact match |
| `stdout_contains` / `stderr_contains` | substring match |
| `stdout_regex` / `stderr_regex` | regex match |
| `vars_set` | table of key=value the plugin set during this step |
| `vars_export` | table of key=value the plugin exported during this step |
| `files_write` | table of `path = bytes-string` or `path = { len = N }` |
| `exec_called` | array of `{ program, args, [exit] }` in invocation order |
| `trap` | bool — a WASM trap was expected |
| `denied` | bool — at least one capability-denied error was expected |

Logs (`vars_set`, `files_write`, `exec_called`, …) reset between steps.
If a step has no `expect` block, the step is only required to complete
without trap/timeout.

## 6. Error handling

| Error | Detection | Surface |
|-------|-----------|---------|
| Wasm load failure | `Component::from_file` / `instantiate_pre` returns `Err` | exit 99 (`error.kind = "load"`), names the file and underlying wasmtime error |
| Trap | `wasmtime::Trap` from any guest call | exit 99 (`error.kind = "trap"`), includes trap kind and any backtrace wasmtime can provide |
| Capability denied | host import returns `Err(ErrorCode::Denied)` | counted and surfaced per step; if the scenario has `denied: true`, the step passes |
| Timeout | epoch deadline elapsed | exit 99 (`error.kind = "timeout"`), names which step/hook was in flight |
| Argument error | clap parse failure or scenario schema error | exit 2 |

Hints for the most common stumbles:

- `metadata called a host import` → "The `metadata` function must be
  side-effect-free. See `docs/yosh/plugin.md` §Plugin Development
  Guide."
- `commands:exec denied for "<argv>"` → "Re-run with
  `--allow-exec '<pattern>'` or add the pattern to the scenario's
  `env.allow_exec`."
- `files:read denied` → "Add `files:read` to `env.caps` (or `--cap`)
  and either populate `[files]` (virtual FS) or pass `--sandbox-root`."

All error paths are routed through `log` so `RUST_LOG=yosh_plugin_manager::runner=debug`
traces each host import call.

## 7. Engine and linker reuse

- Engine: reuse `precompile::make_engine()`. It already sets
  `epoch_interruption(true)`, parallel codegen, and the cache config.
  `TestCtx` uses the same epoch tick thread pattern as
  `metadata_extract` (`Duration::from_millis(timeout_ms)` for the whole
  invocation).
- Linker: build once per `run` / per scenario and reuse across that
  invocation's steps. Granted-capability filtering applies the same
  deny-stub policy as `src/plugin/linker.rs`. The linker construction
  helper lives in `test_host.rs`; no attempt to share with
  `linker.rs` at this stage.
- WASI: `wasmtime_wasi::add_to_linker_sync` + empty `WasiCtxBuilder`
  (no preopens, no stdio, no env, no args). Same rationale as
  `metadata_extract` §Sandboxing.

## 8. Shared code move

`src/plugin/pattern.rs` (which defines `CommandPattern` and its
matcher) moves to `crates/yosh-plugin-api/src/pattern.rs`. yosh
re-exports it from `src/plugin/pattern.rs` so `src/plugin/host/commands.rs`
keeps compiling. This is the only cross-crate change required.

Rationale: the pattern matcher is the only piece of code that both
the real host and `TestCtx` must agree on (allowlist semantics for
`commands:exec`). Duplicating it would risk drift the same way the
TODO already flags for the metadata-extract deny stubs.

## 9. Documentation

`docs/yosh/plugin.md` gains a new section "Testing Locally" at the end
of "Plugin Development Guide":

1. Build your plugin (`cargo component build --target wasm32-wasip2 --release`).
2. Quick smoke: `yosh plugin run target/.../my_plugin.wasm exec hello world`.
3. Write a scenario in `tests/hello.toml` (minimal example).
4. Run the suite: `yosh plugin test`.
5. CI snippet (GitHub Actions): `cargo component build` step followed
   by `yosh plugin test --format json | tee result.jsonl`.

## 10. Testing this work

Integration tests live under
`crates/yosh-plugin-manager/tests/runner.rs` and reuse the existing
in-repo `tests/plugins/test_plugin.wasm` artefact.

Cases:
1. `run exec` happy path — exit code 0, stdout/stderr captured.
2. `run hook on-cd` — `vars_set` recorded.
3. `run` with insufficient `--cap` — denied error, exit 121.
4. `run` with `commands:exec` allowed pattern — real `echo` runs and
   stdout flows back.
5. `run` with timeout — slow_plugin triggers exit 122.
6. `test` parses TOML correctly — fixture scenario passes.
7. `test` reports failure with step index and reason — fixture scenario
   fails on a mismatched `vars_set`.
8. Scenario schema validation — unknown `expect` keys produce a clean
   error rather than silent ignore.

Fixture scenarios live under
`crates/yosh-plugin-manager/tests/scenarios/`.

## 11. Out of scope / future work

- Consolidate `HostContext`, `MetadataCtx`, and `TestCtx` onto a shared
  `HostBackend` trait. Mirrors the existing TODO about deriving
  metadata-extract deny stubs from the bindgen `Host` traits.
- Interactive REPL (`yosh plugin repl <wasm>`).
- Watch mode (`yosh plugin run --watch`).
- Multi-plugin scenarios (one scenario file driving two plugins
  cooperating). Defer until a real use case appears.
- Shell-script-with-metadata scenario format. The TOML form is the
  current direction; reconsider if scenario authors push back.

## 12. Open questions

None blocking. The design as written is internally consistent. Two
points are explicitly left flexible:

- Default for `--cap` is "everything the plugin declares". Could be
  flipped to "deny by default, opt-in via flag" for stricter local
  testing. Easy to revisit once real users weigh in.
- Whether to publish the manager's runner as a library API
  (`yosh-plugin-manager` as a crates.io library) for third-party
  tooling. Currently the binary is the public surface; a library
  surface can be added without breaking changes.
