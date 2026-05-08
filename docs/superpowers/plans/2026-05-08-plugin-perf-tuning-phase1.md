# Plugin Performance Tuning — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the measurement infrastructure for plugin performance (perf_plugin fixture, yosh-dhat extensions, expanded Criterion benches, workload scripts, and the `2026-05-08-plugin-perf-report.md` baseline report), so Phase 2..N per-fix specs can be derived from measured hotspots.

**Architecture:** A new wasm-component fixture `perf_plugin` provides minimal-overhead commands (`noop_cmd`, `noop_var`, `burst_var`) and hooks (`pre_prompt`, `pre_exec`, `post_exec`). The `yosh-dhat` binary gains two non-interactive driver flags (`--exec-loop`, `--pre-prompt-loop`) so heap and CPU profiles can target the plugin path without requiring an interactive terminal. Criterion benches add 0/1/3-plugin baselines so Δ% is directly readable. Plugin loading for benches is staged via a per-test `HOME` override that points at a tmpdir with a hand-rolled `plugins.lock`.

**Tech Stack:** wasmtime 27 (component-model), cargo-component 0.18.0, wasm32-wasip2 target, dhat-rs 0.3, samply 0.13.1, Criterion 0.5. All measurement runs use the existing `[profile.profiling]` build profile (`release` + `debug = true` + `strip = false`).

**Spec reference:** `docs/superpowers/specs/2026-05-08-plugin-perf-tuning-design.md` (commit `947efc8`).

---

## File Structure

**New files:**
- `tests/plugins/perf_plugin/Cargo.toml` — wasm-component package manifest.
- `tests/plugins/perf_plugin/src/lib.rs` — Plugin impl with 3 commands + 3 hooks.
- `benches/data/plugin_w3.sh` — script-throughput workload (1000-iter for-loop calling `noop_cmd`).
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` — measurement report (Phase 1 deliverable).

**Modified files:**
- `Cargo.toml` (workspace root) — register `tests/plugins/perf_plugin` as a workspace member (and exclude from `default-members`, mirroring `test_plugin`/`trap_plugin`/`slow_plugin`).
- `src/bin/yosh-dhat.rs` — add `--exec-loop N CMD ARGS...` and `--pre-prompt-loop N` flags.
- `benches/plugin_bench.rs` — refactor existing 3 benches off `test_plugin`, add 5 new benches (pre_prompt 0/1/3, exec_burst_var, hook_pre_exec_zero_plugins).
- `benches/startup_bench.rs` — add `startup_one_plugin` and `startup_three_plugins` (each iter spawns one `yosh -c 'echo hi'` against a staged HOME with the plugin in `plugins.lock`).
- `TODO.md` — record any deferred Phase 2 candidates surfaced by the report.

**Helper module (private to benches):**
- `benches/plugin_bench_helpers.rs` — staging helpers that build a tmpdir HOME + `.config/yosh/plugins.lock` for the bench harness. Imported via `mod plugin_bench_helpers;` from `plugin_bench.rs` and `startup_bench.rs`.

---

## Conventions and shared knowledge

- **Building the wasm artifact:** `cargo component build -p perf_plugin --target wasm32-wasip2 --release` produces `target/wasm32-wasip2/release/perf_plugin.wasm`. Bench fixtures resolve this path via `env!("CARGO_MANIFEST_DIR")` joined with the relative path.
- **Profile:** All measurement runs use `cargo build --profile profiling --features dhat-heap` for `yosh` / `yosh-dhat`. Criterion runs default to the `bench` profile (which inherits from `release`).
- **plugins.lock format:** TOML with a top-level `plugin = [{ name, path, enabled, capabilities, ... }]` array; see `src/plugin/config.rs::PluginEntry`. The bench staging helper writes this synchronously before launching the binary.
- **HOME-based plugin config override:** `src/exec/mod.rs::plugin_config_path()` resolves `~/.config/yosh/plugins.lock` from `$HOME`. Benches override `HOME` to point at a tmpdir to avoid touching the real user config.
- **Three-plugin loading:** Distinct `name` keys in plugins.lock with the same `path` produce three independent `Plugin` records (each gets its own `Store` and `Instance`). Verified in Task 4 before later tasks depend on it. Inside the wasm, the embedded `plugin-info.name` is the same for all three; this is fine for hook-dispatch and startup measurements (which do not depend on the internal name).
- **TDD discipline:** Tasks that write Rust source (perf_plugin commands, yosh-dhat flag parsing) use TDD. Tasks that produce bench infrastructure or measurement reports use a smoke-run validation instead — the test is "the bench produces a valid Criterion median" or "the report has all six sections," not a unit test.

---

## Task 1: Create perf_plugin scaffolding

**Files:**
- Create: `tests/plugins/perf_plugin/Cargo.toml`
- Create: `tests/plugins/perf_plugin/src/lib.rs`
- Modify: `Cargo.toml` (workspace root) — `members` and `default-members` arrays

- [ ] **Step 1: Add perf_plugin to workspace members and exclude from default-members**

Edit `Cargo.toml` (workspace root). In the `members` array, add `"tests/plugins/perf_plugin"`. The `default-members` array already excludes the wasm plugins, so leave it untouched.

After edit, lines 1-22 should look like:

```toml
[workspace]
members = [
    ".",
    "crates/yosh-plugin-api",
    "crates/yosh-plugin-sdk",
    "crates/yosh-plugin-manager",
    "tests/plugins/test_plugin",
    "tests/plugins/trap_plugin",
    "tests/plugins/slow_plugin",
    "tests/plugins/perf_plugin",
]
default-members = [
    ".",
    "crates/yosh-plugin-api",
    "crates/yosh-plugin-sdk",
    "crates/yosh-plugin-manager",
]
```

- [ ] **Step 2: Write `tests/plugins/perf_plugin/Cargo.toml`**

Create the file with this content (mirrors `tests/plugins/test_plugin/Cargo.toml` exactly except for `name` and `package`):

```toml
[package]
name = "perf_plugin"
version = "0.2.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
yosh-plugin-sdk = { path = "../../../crates/yosh-plugin-sdk" }

[package.metadata.component]
package = "yosh:perf-plugin"

[package.metadata.component.target]
path  = "../../../crates/yosh-plugin-api/wit"
world = "plugin-world"

[package.metadata.component.dependencies]
# wasi adapter is auto-resolved by cargo-component for the wasm32-wasip2 target.
```

- [ ] **Step 3: Write skeleton `tests/plugins/perf_plugin/src/lib.rs`**

Create the file with an empty Plugin impl that compiles but does nothing yet (commands and hooks come in Task 2):

```rust
//! perf_plugin — minimal-overhead fixture for plugin performance benches.
//!
//! Used by `benches/plugin_bench.rs` and `benches/startup_bench.rs`. Has no
//! stdout side-effects (unlike `test_plugin`'s `print()` calls), so Criterion
//! measurements are not polluted.

use yosh_plugin_sdk::{Capability, HookName, Plugin, export, get_var};

#[derive(Default)]
struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn commands(&self) -> &[&'static str] {
        &["noop_cmd", "noop_var", "burst_var"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::HookPrePrompt,
            Capability::HookPreExec,
            Capability::HookPostExec,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PrePrompt, HookName::PreExec, HookName::PostExec]
    }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        // Filled in by Task 2.
        127
    }
}

export!(PerfPlugin);
```

- [ ] **Step 4: Build the wasm artifact and verify it appears under target/**

Run:

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
```

Expected: build succeeds, `target/wasm32-wasip2/release/perf_plugin.wasm` exists.

Verify with: `ls target/wasm32-wasip2/release/perf_plugin.wasm`

If the build fails because `cargo-component` is not installed, install per CLAUDE.md:

```bash
cargo install cargo-component --locked --version 0.18.0
rustup target add wasm32-wasip2
```

- [ ] **Step 5: Verify host build still passes**

Run:

```bash
cargo build
```

Expected: success, no new warnings (the new workspace member must be excluded from default-members so `cargo build` does not try to compile it for the host target).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml tests/plugins/perf_plugin/
git commit -m "feat(plugin-perf): add perf_plugin workspace scaffolding"
```

---

## Task 2: Implement perf_plugin commands

**Files:**
- Modify: `tests/plugins/perf_plugin/src/lib.rs`
- Modify: `tests/plugin.rs` — add an integration test that loads perf_plugin and exercises each command.

- [ ] **Step 1: Write the failing integration test**

Append to `tests/plugin.rs` (use the same `load_plugin_with_caps` helper pattern as existing tests in that file):

```rust
#[test]
fn perf_plugin_commands_exit_zero() {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    env.vars
        .set("PERF_VAR", "perf_value")
        .expect("set PERF_VAR");
    let mut mgr = yosh::plugin::PluginManager::new();

    let wasm = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/perf_plugin.wasm");
    assert!(
        wasm.exists(),
        "perf_plugin.wasm not found at {}; run `cargo component build -p perf_plugin --target wasm32-wasip2 --release`",
        wasm.display()
    );

    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        yosh_plugin_api::CAP_ALL,
        &[],
    )
    .expect("load perf_plugin");

    for cmd in ["noop_cmd", "noop_var", "burst_var"] {
        let result = mgr.exec_command(&mut env, cmd, &[]);
        assert!(
            matches!(result, yosh::plugin::PluginExec::Handled(0)),
            "{} should be Handled(0), got {:?}",
            cmd,
            result
        );
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin perf_plugin_commands_exit_zero
```

Expected: FAIL — the skeleton `exec` returns 127 for every command.

- [ ] **Step 3: Implement the three commands in perf_plugin**

Replace the `exec` method body in `tests/plugins/perf_plugin/src/lib.rs`:

```rust
fn exec(&mut self, command: &str, _args: &[String]) -> i32 {
    match command {
        "noop_cmd" => 0,
        "noop_var" => {
            let _ = get_var("PERF_VAR");
            0
        }
        "burst_var" => {
            for _ in 0..10 {
                let _ = get_var("PERF_VAR");
            }
            0
        }
        _ => 127,
    }
}
```

Note: `get_var` returns `Result<Option<String>, ErrorCode>`. We discard both arms — this is intentional; the bench measures the host-import boundary cost, not the value handling.

- [ ] **Step 4: Rebuild wasm and run the test**

Run:

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin perf_plugin_commands_exit_zero
```

Expected: PASS.

- [ ] **Step 5: Run the full plugin test suite to verify no regression**

Run:

```bash
cargo test --features test-helpers --test plugin
```

Expected: all existing plugin tests still pass.

- [ ] **Step 6: Commit**

```bash
git add tests/plugins/perf_plugin/src/lib.rs tests/plugin.rs
git commit -m "feat(plugin-perf): implement perf_plugin noop_cmd / noop_var / burst_var"
```

---

## Task 3: Implement perf_plugin hooks

**Files:**
- Modify: `tests/plugins/perf_plugin/src/lib.rs`
- Modify: `tests/plugin.rs` — extend the integration test to exercise pre_prompt / pre_exec / post_exec dispatch.

- [ ] **Step 1: Write the failing hook-dispatch test**

Append to `tests/plugin.rs`:

```rust
#[test]
fn perf_plugin_hooks_dispatch_without_panic() {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    let mut mgr = yosh::plugin::PluginManager::new();

    let wasm = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/perf_plugin.wasm");

    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        yosh_plugin_api::CAP_ALL,
        &[],
    )
    .expect("load perf_plugin");

    // Hooks must dispatch without trapping. They have no observable
    // side effect by design (this is a perf fixture, not a behavior test);
    // the assertion is the absence of panic / trap.
    mgr.call_pre_prompt(&mut env);
    mgr.call_pre_exec(&mut env, "noop");
    mgr.call_post_exec(&mut env, "noop", 0);
}
```

- [ ] **Step 2: Run and verify it fails**

Run:

```bash
cargo test --features test-helpers --test plugin perf_plugin_hooks_dispatch_without_panic
```

Expected: FAIL — the SDK's default trait methods panic with "not implemented" or similar when the plugin does not override them. (If it passes immediately, that's also OK — it means the SDK's default no-op behavior already covers it. In that case proceed to Step 3 anyway to add the explicit empty bodies for measurement clarity.)

- [ ] **Step 3: Implement the three hook overrides**

Add to the `impl Plugin for PerfPlugin` block in `tests/plugins/perf_plugin/src/lib.rs`:

```rust
fn hook_pre_prompt(&mut self) {
    // Empty body — measures dispatch overhead, not user work.
}

fn hook_pre_exec(&mut self, _command: &str) {
    // Empty body — measures dispatch overhead.
}

fn hook_post_exec(&mut self, _command: &str, _exit_code: i32) {
    // Empty body — measures dispatch overhead.
}
```

- [ ] **Step 4: Rebuild and run the test**

Run:

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin perf_plugin_hooks_dispatch_without_panic
```

Expected: PASS.

- [ ] **Step 5: Re-run the full plugin suite**

Run:

```bash
cargo test --features test-helpers --test plugin
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add tests/plugins/perf_plugin/src/lib.rs tests/plugin.rs
git commit -m "feat(plugin-perf): implement perf_plugin pre_prompt / pre_exec / post_exec hooks"
```

---

## Task 4: Verify three-plugin loading via aliased plugins.lock entries

**Files:**
- Modify: `tests/plugin.rs` — add a test that loads perf_plugin three times via `load_from_config` against a staged plugins.lock.

This task locks in the assumption (spec §5.2 note) that 3 distinct `name` keys pointing at the same wasm path produce 3 independent `Plugin` records. If the assumption breaks, this task fails early — before any bench depends on it.

- [ ] **Step 1: Write the failing test**

Append to `tests/plugin.rs`:

```rust
#[test]
fn perf_plugin_three_aliases_load_independently() {
    use std::io::Write;

    let wasm = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/perf_plugin.wasm");
    assert!(wasm.exists(), "build perf_plugin first");

    let tmp = tempfile::tempdir().expect("tempdir");
    let lock_path = tmp.path().join("plugins.lock");
    let mut f = std::fs::File::create(&lock_path).expect("create lock");
    writeln!(
        f,
        r#"
[[plugin]]
name = "perf_a"
path = "{wasm}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]

[[plugin]]
name = "perf_b"
path = "{wasm}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]

[[plugin]]
name = "perf_c"
path = "{wasm}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
"#,
        wasm = wasm.display()
    )
    .expect("write lock");

    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    let mut mgr = yosh::plugin::PluginManager::new();
    mgr.load_from_config(&lock_path, &mut env);

    // Hooks should fire on all three; no panic, no error.
    mgr.call_pre_prompt(&mut env);
    mgr.call_pre_exec(&mut env, "noop");

    // We rely on the public test_helpers surface; if the manager exposes
    // a count accessor, use it. Otherwise this is a smoke test only.
}
```

- [ ] **Step 2: Run and verify it fails or passes**

Run:

```bash
cargo test --features test-helpers --test plugin perf_plugin_three_aliases_load_independently
```

If it PASSES on first run: the loader supports 3 aliases out of the box; no Phase 1 loader change needed. Proceed to Step 4 (commit).

If it FAILS with a name-collision error or only loads 1/3: we need a fallback. Two options, in order of preference:

(a) **Loader-side fix** — locate the dedupe check in `src/plugin/mod.rs::load_one` (or wherever rejection happens) and skip name-collision checks when the source path differs only by manifest name. Keep the change tightly scoped (within S2 budget).

(b) **Build 3 thin wasm aliases** — add `perf_plugin_alt_a`, `perf_plugin_alt_b` workspace members that just re-export the same Plugin impl. Update Task 1's workspace member list and Cargo.toml accordingly.

- [ ] **Step 3: If the test failed, apply the chosen fallback and re-run**

After applying the fix, re-run:

```bash
cargo test --features test-helpers --test plugin perf_plugin_three_aliases_load_independently
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add tests/plugin.rs
# If a fallback was applied, also add the modified loader / new alias plugins.
git commit -m "test(plugin-perf): verify perf_plugin loads under three alias names"
```

---

## Task 5: Add `--exec-loop` flag to yosh-dhat

**Files:**
- Modify: `src/bin/yosh-dhat.rs`

- [ ] **Step 1: Read current yosh-dhat structure to design the flag dispatch**

The current binary expects `argv[1] = script-path`. The new shape:

- `yosh-dhat <script-path>` (existing) — runs script (W-P3 dhat path).
- `yosh-dhat --exec-loop N CMD ARGS...` — calls `PluginManager::exec_command(CMD, ARGS)` N times after `load_plugins()`.
- `yosh-dhat --pre-prompt-loop N` — calls `PluginManager::call_pre_prompt(env)` N times after `load_plugins()`. (Implemented in Task 6.)

Use a small hand-rolled arg matcher rather than adding a clap dependency — yosh-dhat is intentionally minimal.

- [ ] **Step 2: Refactor main into mode dispatch (preserves existing script path)**

Replace `src/bin/yosh-dhat.rs` body of `fn main` with:

```rust
fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let args: Vec<String> = std::env::args().collect();

    let status = match args.get(1).map(|s| s.as_str()) {
        Some("--exec-loop") => run_exec_loop(&args[2..]),
        Some("--pre-prompt-loop") => run_pre_prompt_loop(&args[2..]),
        Some(_) => run_script(&args[1]),
        None => {
            eprintln!(
                "usage: {bin} <script-path>\n       {bin} --exec-loop N CMD [ARG ...]\n       {bin} --pre-prompt-loop N",
                bin = args.first().map(String::as_str).unwrap_or("yosh-dhat")
            );
            2
        }
    };

    #[cfg(feature = "dhat-heap")]
    drop(_profiler);
    process::exit(status);
}
```

Then move the existing body into a new function `fn run_script(path: &str) -> i32`:

```rust
fn run_script(script_path: &str) -> i32 {
    let input = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yosh-dhat: {}: {}", script_path, e);
            return 127;
        }
    };

    yosh::signal::init_signal_handling();
    let mut executor = yosh::exec::Executor::new("yosh-dhat", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();

    let program = match yosh::parser::Parser::new(&input).parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("yosh-dhat: parse error: {}", e);
            return 2;
        }
    };

    let status = executor.exec_program(&program);
    executor.process_pending_signals();
    executor.execute_exit_trap();
    status
}
```

- [ ] **Step 3: Implement `run_exec_loop`**

Add to `src/bin/yosh-dhat.rs`:

```rust
fn run_exec_loop(args: &[String]) -> i32 {
    let n: u32 = match args.first().and_then(|s| s.parse().ok()) {
        Some(n) if n > 0 => n,
        _ => {
            eprintln!("yosh-dhat: --exec-loop: missing or invalid N (positive integer)");
            return 2;
        }
    };
    let cmd = match args.get(1) {
        Some(c) => c.clone(),
        None => {
            eprintln!("yosh-dhat: --exec-loop: missing CMD");
            return 2;
        }
    };
    let cmd_args: Vec<String> = args.iter().skip(2).cloned().collect();

    yosh::signal::init_signal_handling();
    let mut executor = yosh::exec::Executor::new("yosh-dhat", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();

    let mut last_status = 0;
    for _ in 0..n {
        let r = executor.plugins.exec_command(&mut executor.env, &cmd, &cmd_args);
        // PluginExec is a 3-valued enum (NotHandled / Handled(i32) / Failed);
        // for the loop we only care that it dispatched. The bench/dhat
        // profiler captures the boundary cost.
        last_status = match r {
            yosh::plugin::PluginExec::Handled(code) => code,
            yosh::plugin::PluginExec::NotHandled => 127,
            yosh::plugin::PluginExec::Failed => 1,
        };
    }
    last_status
}
```

Note: `executor.plugins` and `executor.env` are accessed as struct fields. Verify they are `pub` (or `pub(crate)` reachable via the `yosh` lib) — if not, expose via a small helper. Same applies to `PluginExec::exit_code`. (If access modifiers block this, the test_helpers feature already exposes `PluginManager` internals; use `yosh::plugin::test_helpers` to bridge.)

- [ ] **Step 4: Smoke-test `--exec-loop`**

Stage a HOME with a plugins.lock pointing at perf_plugin, then run:

```bash
HOME=/tmp/yosh-dhat-smoke
mkdir -p "$HOME/.config/yosh"
cat > "$HOME/.config/yosh/plugins.lock" <<EOF
[[plugin]]
name = "perf"
path = "$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
EOF

cargo build --profile profiling --features dhat-heap --bin yosh-dhat
HOME="$HOME" ./target/profiling/yosh-dhat --exec-loop 5 noop_cmd
echo $?
ls -la dhat-heap.json
rm -f dhat-heap.json
```

Expected: exit 0, `dhat-heap.json` written.

- [ ] **Step 5: Add a stub `run_pre_prompt_loop` so the dispatch compiles**

Add a placeholder that errors cleanly until Task 6:

```rust
fn run_pre_prompt_loop(_args: &[String]) -> i32 {
    eprintln!("yosh-dhat: --pre-prompt-loop: not yet implemented");
    2
}
```

- [ ] **Step 6: Verify cargo build still works**

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
```

Expected: success.

- [ ] **Step 7: Commit**

```bash
git add src/bin/yosh-dhat.rs
git commit -m "feat(yosh-dhat): add --exec-loop flag for non-interactive plugin exec measurement"
```

---

## Task 6: Add `--pre-prompt-loop` flag to yosh-dhat

**Files:**
- Modify: `src/bin/yosh-dhat.rs`

- [ ] **Step 1: Implement `run_pre_prompt_loop`**

Replace the stub from Task 5 Step 5 with:

```rust
fn run_pre_prompt_loop(args: &[String]) -> i32 {
    let n: u32 = match args.first().and_then(|s| s.parse().ok()) {
        Some(n) if n > 0 => n,
        _ => {
            eprintln!("yosh-dhat: --pre-prompt-loop: missing or invalid N (positive integer)");
            return 2;
        }
    };

    yosh::signal::init_signal_handling();
    let mut executor = yosh::exec::Executor::new("yosh-dhat", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();

    for _ in 0..n {
        executor.plugins.call_pre_prompt(&mut executor.env);
    }
    0
}
```

- [ ] **Step 2: Smoke-test against perf_plugin**

Reuse the staged HOME from Task 5:

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
HOME=/tmp/yosh-dhat-smoke ./target/profiling/yosh-dhat --pre-prompt-loop 100
echo $?
ls -la dhat-heap.json
rm -f dhat-heap.json
```

Expected: exit 0, `dhat-heap.json` written.

- [ ] **Step 3: Smoke-test with no plugins (zero-plugin baseline)**

```bash
HOME=/tmp/empty-yosh-config
mkdir -p "$HOME/.config/yosh"
echo "" > "$HOME/.config/yosh/plugins.lock"

HOME="$HOME" ./target/profiling/yosh-dhat --pre-prompt-loop 1000
echo $?
ls -la dhat-heap.json
rm -f dhat-heap.json
```

Expected: exit 0, `dhat-heap.json` written. The bytes total should be visibly smaller than the with-plugin run.

- [ ] **Step 4: Commit**

```bash
git add src/bin/yosh-dhat.rs
git commit -m "feat(yosh-dhat): add --pre-prompt-loop flag for hook-path measurement"
```

---

## Task 7: Add bench helper module for plugin config staging

**Files:**
- Create: `benches/plugin_bench_helpers.rs`

The helper provides one function: `stage_home_with_plugin(plugin_count: usize) -> tempfile::TempDir`. Returns a tmpdir whose `.config/yosh/plugins.lock` lists `plugin_count` aliased entries pointing at `target/wasm32-wasip2/release/perf_plugin.wasm`. Callers set `HOME` to `tmpdir.path()` before launching the binary, and the tmpdir cleans up on drop.

- [ ] **Step 1: Write the helper**

Create `benches/plugin_bench_helpers.rs`:

```rust
//! Shared helpers for plugin benches (plugin_bench.rs, startup_bench.rs).
//!
//! Both benches need to stage a HOME directory whose
//! `.config/yosh/plugins.lock` lists 0/1/3 perf_plugin entries. Lifting the
//! staging here keeps each bench focused on its measurement.

#![allow(dead_code)] // imported by benches via `mod plugin_bench_helpers;`

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn perf_plugin_wasm() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/perf_plugin.wasm");
    assert!(
        p.exists(),
        "perf_plugin.wasm not found at {}; build it first with \
         `cargo component build -p perf_plugin --target wasm32-wasip2 --release`",
        p.display()
    );
    p
}

/// Create a tempdir with `.config/yosh/plugins.lock` listing `count`
/// aliased perf_plugin entries. Caller sets `HOME` to `tempdir.path()`
/// before launching `yosh` / `yosh-dhat`.
pub fn stage_home_with_plugin(count: usize) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join(".config/yosh");
    std::fs::create_dir_all(&dir).expect("create cfg dir");
    let lock = dir.join("plugins.lock");
    let mut f = std::fs::File::create(&lock).expect("create lock");

    if count > 0 {
        let wasm = perf_plugin_wasm();
        for i in 0..count {
            writeln!(
                f,
                r#"
[[plugin]]
name = "perf_{i}"
path = "{wasm}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
"#,
                i = i,
                wasm = wasm.display()
            )
            .expect("write entry");
        }
    }
    drop(f);
    tmp
}
```

- [ ] **Step 2: Verify the helper compiles in isolation**

Add a temporary `mod plugin_bench_helpers;` line at the top of `benches/plugin_bench.rs` (will be removed if benches don't need it later, but for now it forces a build check):

```rust
mod plugin_bench_helpers;
```

Run:

```bash
cargo build --bench plugin_bench --features test-helpers
```

Expected: success. If `tempfile` is not in dev-dependencies, it already is per `Cargo.toml:50`.

- [ ] **Step 3: Commit**

```bash
git add benches/plugin_bench_helpers.rs benches/plugin_bench.rs
git commit -m "feat(plugin-perf): add bench helper for plugins.lock staging"
```

---

## Task 8: Refactor existing plugin_bench.rs benches to use perf_plugin

**Files:**
- Modify: `benches/plugin_bench.rs`

This task replaces the three existing benches (`plugin_exec_test_cmd`, `plugin_exec_echo_var`, `plugin_hook_pre_exec`) with perf_plugin-backed equivalents. The new bench function names use the `_noop` suffix to make the no-side-effect property explicit in the report.

- [ ] **Step 1: Rewrite `make_loaded_manager` to use perf_plugin**

In `benches/plugin_bench.rs`, replace `make_loaded_manager` and `test_plugin_wasm` with:

```rust
fn make_loaded_manager() -> (yosh::plugin::PluginManager, yosh::env::ShellEnv) {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    env.vars
        .set("PERF_VAR", "perf_value")
        .expect("set PERF_VAR");
    let mut mgr = yosh::plugin::PluginManager::new();
    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr,
        &plugin_bench_helpers::perf_plugin_wasm(),
        &mut env,
        yosh_plugin_api::CAP_ALL,
        &[],
    )
    .expect("load perf_plugin");
    (mgr, env)
}
```

Remove the old `test_plugin_wasm()` function (no longer used).

- [ ] **Step 2: Replace `bench_exec_no_host_call` with the noop_cmd version**

Replace the function body:

```rust
fn bench_exec_noop_cmd(c: &mut Criterion) {
    let (mut mgr, mut env) = make_loaded_manager();
    let args: Vec<String> = vec![];
    c.bench_function("plugin_exec_noop_cmd", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "noop_cmd", black_box(&args));
            black_box(r);
        });
    });
}
```

- [ ] **Step 3: Replace `bench_exec_with_var_get` with `noop_var`**

```rust
fn bench_exec_noop_var(c: &mut Criterion) {
    let (mut mgr, mut env) = make_loaded_manager();
    let args: Vec<String> = vec![];
    c.bench_function("plugin_exec_noop_var", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "noop_var", black_box(&args));
            black_box(r);
        });
    });
}
```

- [ ] **Step 4: Update `bench_hook_pre_exec` to use perf_plugin (no rename needed)**

The function body is identical — just the underlying plugin changed. The bench name `plugin_hook_pre_exec` stays for continuity in the Criterion history.

- [ ] **Step 5: Update `criterion_group!` registration**

```rust
criterion_group!(
    plugin_benches,
    bench_exec_noop_cmd,
    bench_exec_noop_var,
    bench_hook_pre_exec,
);
criterion_main!(plugin_benches);
```

- [ ] **Step 6: Run the benches and verify clean output**

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo bench --bench plugin_bench --features test-helpers -- --quick
```

Expected: three benches run, no stdout pollution from `print()` calls (this was the existing TODO), Criterion produces medians.

- [ ] **Step 7: Commit**

```bash
git add benches/plugin_bench.rs
git commit -m "refactor(plugin-perf): retarget existing plugin benches at perf_plugin"
```

---

## Task 9: Add 0/1/3-plugin pre_prompt benches

**Files:**
- Modify: `benches/plugin_bench.rs`

These benches measure the dispatch path with 0, 1, and 3 plugins loaded, so the report can express W-P1 cost as a Δ.

- [ ] **Step 1: Add the three pre_prompt benches**

Append to `benches/plugin_bench.rs`:

```rust
fn make_manager_with_n_plugins(n: usize) -> (yosh::plugin::PluginManager, yosh::env::ShellEnv) {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    let mut mgr = yosh::plugin::PluginManager::new();
    let wasm = plugin_bench_helpers::perf_plugin_wasm();
    for _ in 0..n {
        yosh::plugin::test_helpers::load_plugin_with_caps(
            &mut mgr,
            &wasm,
            &mut env,
            yosh_plugin_api::CAP_ALL,
            &[],
        )
        .expect("load perf_plugin");
    }
    (mgr, env)
}

fn bench_pre_prompt_zero_plugins(c: &mut Criterion) {
    let (mut mgr, mut env) = make_manager_with_n_plugins(0);
    c.bench_function("plugin_pre_prompt_zero_plugins", |b| {
        b.iter(|| {
            mgr.call_pre_prompt(black_box(&mut env));
        });
    });
}

fn bench_pre_prompt_one_noop(c: &mut Criterion) {
    let (mut mgr, mut env) = make_manager_with_n_plugins(1);
    c.bench_function("plugin_pre_prompt_one_noop", |b| {
        b.iter(|| {
            mgr.call_pre_prompt(black_box(&mut env));
        });
    });
}

fn bench_pre_prompt_three_noop(c: &mut Criterion) {
    let (mut mgr, mut env) = make_manager_with_n_plugins(3);
    c.bench_function("plugin_pre_prompt_three_noop", |b| {
        b.iter(|| {
            mgr.call_pre_prompt(black_box(&mut env));
        });
    });
}
```

Note: this calls `load_plugin_with_caps` three times against the same path (in the n=3 case). If Task 4 confirmed this works at the `load_from_config` level, it will also work at the `load_plugin_with_caps` level (same underlying `load_one`). If Task 4 had to apply a fallback (alias plugins or loader change), update this helper accordingly — for example, take a `&[PathBuf]` parameter and load each path once.

- [ ] **Step 2: Register the three benches in `criterion_group!`**

```rust
criterion_group!(
    plugin_benches,
    bench_exec_noop_cmd,
    bench_exec_noop_var,
    bench_hook_pre_exec,
    bench_pre_prompt_zero_plugins,
    bench_pre_prompt_one_noop,
    bench_pre_prompt_three_noop,
);
```

- [ ] **Step 3: Run the new benches**

```bash
cargo bench --bench plugin_bench --features test-helpers -- --quick \
    plugin_pre_prompt
```

Expected: three benches run, three medians produced. The three_noop median should be measurably larger than zero_plugins (otherwise dispatch is free, which is itself a finding for the report).

- [ ] **Step 4: Commit**

```bash
git add benches/plugin_bench.rs
git commit -m "feat(plugin-perf): add 0/1/3-plugin pre_prompt dispatch benches"
```

---

## Task 10: Add burst_var bench and pre_exec zero-plugin baseline

**Files:**
- Modify: `benches/plugin_bench.rs`

- [ ] **Step 1: Add `bench_exec_burst_var`**

Append to `benches/plugin_bench.rs`:

```rust
fn bench_exec_burst_var(c: &mut Criterion) {
    let (mut mgr, mut env) = make_loaded_manager();
    env.vars
        .set("PERF_VAR", "perf_value")
        .expect("set PERF_VAR");
    let args: Vec<String> = vec![];
    c.bench_function("plugin_exec_burst_var", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "burst_var", black_box(&args));
            black_box(r);
        });
    });
}
```

- [ ] **Step 2: Add `bench_pre_exec_zero_plugins`**

```rust
fn bench_pre_exec_zero_plugins(c: &mut Criterion) {
    let (mut mgr, mut env) = make_manager_with_n_plugins(0);
    c.bench_function("plugin_pre_exec_zero_plugins", |b| {
        b.iter(|| {
            mgr.call_pre_exec(black_box(&mut env), "noop");
        });
    });
}
```

- [ ] **Step 3: Update `criterion_group!`**

```rust
criterion_group!(
    plugin_benches,
    bench_exec_noop_cmd,
    bench_exec_noop_var,
    bench_exec_burst_var,
    bench_hook_pre_exec,
    bench_pre_exec_zero_plugins,
    bench_pre_prompt_zero_plugins,
    bench_pre_prompt_one_noop,
    bench_pre_prompt_three_noop,
);
```

- [ ] **Step 4: Run the new benches**

```bash
cargo bench --bench plugin_bench --features test-helpers -- --quick \
    plugin_exec_burst_var plugin_pre_exec_zero_plugins
```

Expected: two new medians produced.

- [ ] **Step 5: Commit**

```bash
git add benches/plugin_bench.rs
git commit -m "feat(plugin-perf): add burst_var and pre_exec zero-plugin benches"
```

---

## Task 11: Add startup_one_plugin and startup_three_plugins benches

**Files:**
- Modify: `benches/startup_bench.rs`

- [ ] **Step 1: Wire the helper module into startup_bench.rs**

Add the same `mod plugin_bench_helpers;` line at the top:

```rust
#[path = "plugin_bench_helpers.rs"]
mod plugin_bench_helpers;
```

The `#[path]` attribute is needed because Cargo's bench harness treats each `[[bench]]` as an independent crate; without it the module path resolution fails.

- [ ] **Step 2: Add the two new bench functions**

Append to `benches/startup_bench.rs`:

```rust
fn bench_startup_with_n_plugins(c: &mut Criterion, n: usize, name: &str) {
    let yosh = yosh_binary();
    let home = plugin_bench_helpers::stage_home_with_plugin(n);
    let home_path = home.path().to_owned();

    c.bench_function(name, |b| {
        b.iter(|| {
            let status = Command::new(black_box(&yosh))
                .args(["-c", "echo hi"])
                .env("HOME", &home_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("failed to spawn yosh");
            assert!(status.success(), "yosh -c 'echo hi' failed");
        });
    });
    drop(home); // explicit; tmpdir cleans up here
}

fn bench_startup_one_plugin(c: &mut Criterion) {
    bench_startup_with_n_plugins(c, 1, "startup_one_plugin");
}

fn bench_startup_three_plugins(c: &mut Criterion) {
    bench_startup_with_n_plugins(c, 3, "startup_three_plugins");
}
```

- [ ] **Step 3: Update `criterion_group!`**

```rust
criterion_group!(
    benches,
    bench_startup_echo,
    bench_startup_one_plugin,
    bench_startup_three_plugins,
);
```

- [ ] **Step 4: Run the new benches**

Build perf_plugin first (the helper asserts the wasm exists):

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo bench --bench startup_bench -- --quick \
    startup_one_plugin startup_three_plugins
```

Expected: two medians produced. The one_plugin median should be visibly larger than `startup_echo_hi`.

- [ ] **Step 5: Commit**

```bash
git add benches/startup_bench.rs
git commit -m "feat(plugin-perf): add startup_one_plugin / startup_three_plugins benches"
```

---

## Task 12: Add benches/data/plugin_w3.sh workload script

**Files:**
- Create: `benches/data/plugin_w3.sh`

- [ ] **Step 1: Write the workload script**

Create `benches/data/plugin_w3.sh`:

```sh
#!/bin/sh
# W-P3 — command-throughput workload for plugin perf measurement.
#
# Calls the perf_plugin `noop_cmd` 1000 times under a yosh `for` loop.
# Drive via:
#   yosh-dhat benches/data/plugin_w3.sh   (heap)
#   samply record -- yosh benches/data/plugin_w3.sh   (CPU)
#
# Requires perf_plugin to be enabled in the active plugins.lock. For
# isolation, wrap in `HOME=<staged-dir> ...`.

i=0
while [ "$i" -lt 1000 ]; do
    noop_cmd
    i=$((i + 1))
done
```

- [ ] **Step 2: Set permissions per repo convention**

E2E test convention is 644 for shell tests; bench scripts are not e2e. Use 644 for consistency:

```bash
chmod 644 benches/data/plugin_w3.sh
```

- [ ] **Step 3: Smoke-test the script via yosh-dhat**

```bash
HOME=/tmp/yosh-dhat-smoke \
    cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/plugin_w3.sh
ls -la dhat-heap.json
mv dhat-heap.json target/perf/dhat-plugin-w3.json 2>/dev/null || mkdir -p target/perf && mv dhat-heap.json target/perf/dhat-plugin-w3.json
```

Expected: script runs 1000 iterations, dhat-heap.json written to `target/perf/dhat-plugin-w3.json`.

- [ ] **Step 4: Commit**

```bash
git add benches/data/plugin_w3.sh
git commit -m "feat(plugin-perf): add W-P3 command-throughput workload script"
```

---

## Task 13: Run all measurements and produce the Phase 1 report

**Files:**
- Create: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`

This is the Phase 1 deliverable. The report has six sections (mirroring `performance.md`).

- [ ] **Step 1: Build all artifacts and run Criterion**

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo build --profile profiling --features dhat-heap --bin yosh --bin yosh-dhat
cargo bench --bench plugin_bench --features test-helpers
cargo bench --bench startup_bench
```

Capture the medians from `target/criterion/<bench>/<function>/new/estimates.json` (`["median"]["point_estimate"]` field, in nanoseconds).

- [ ] **Step 2: Run dhat for W-P1 (pre_prompt)**

```bash
mkdir -p target/perf
HOME=/tmp/yosh-perf-1plugin
mkdir -p "$HOME/.config/yosh"
cat > "$HOME/.config/yosh/plugins.lock" <<EOF
[[plugin]]
name = "perf"
path = "$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
EOF

HOME="$HOME" ./target/profiling/yosh-dhat --pre-prompt-loop 1000
mv dhat-heap.json target/perf/dhat-plugin-w1.json

python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w1.json 10 \
    > target/perf/dhat-plugin-w1.md
```

- [ ] **Step 3: Run dhat for W-P3 (command throughput)**

```bash
HOME="$HOME" ./target/profiling/yosh-dhat benches/data/plugin_w3.sh
mv dhat-heap.json target/perf/dhat-plugin-w3.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w3.json 10 \
    > target/perf/dhat-plugin-w3.md
```

- [ ] **Step 4: Run dhat for W-P5 (startup, 1 plugin)**

`yosh-dhat` only takes a script path / `--exec-loop` / `--pre-prompt-loop` (not `-c`), so the W-P5 dhat run uses a tmp script containing `echo hi`. This still captures load_plugins + first-command dispatch, which is the W-P5 startup signal.

```bash
echo hi > target/perf/echo-hi.sh
HOME="$HOME" ./target/profiling/yosh-dhat target/perf/echo-hi.sh
mv dhat-heap.json target/perf/dhat-plugin-w5.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w5.json 10 \
    > target/perf/dhat-plugin-w5.md
```

- [ ] **Step 5: Run samply for W-P1 / W-P3 / W-P5**

```bash
samply record --save-only --output target/perf/samply-plugin-w1.json -- \
    env HOME="$HOME" ./target/profiling/yosh-dhat --pre-prompt-loop 5000

samply record --save-only --output target/perf/samply-plugin-w3.json -- \
    env HOME="$HOME" ./target/profiling/yosh benches/data/plugin_w3.sh

samply record --save-only --output target/perf/samply-plugin-w5.json -- \
    sh -c 'for i in $(seq 1 50); do HOME="$HOME" ./target/profiling/yosh -c "echo hi" >/dev/null; done'

python3 scripts/perf/samply_top_n.py target/perf/samply-plugin-w1.json 10 \
    > target/perf/samply-plugin-w1.md
python3 scripts/perf/samply_top_n.py target/perf/samply-plugin-w3.json 10 \
    > target/perf/samply-plugin-w3.md
python3 scripts/perf/samply_top_n.py target/perf/samply-plugin-w5.json 10 \
    > target/perf/samply-plugin-w5.md
```

- [ ] **Step 6: Write the report document**

Create `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` with the six sections from spec §5.5. The structure (verbatim from the design spec):

```markdown
# Plugin Performance Report — Phase 1 Baseline

**Measurement date:** 2026-05-08
**Commit:** [git rev-parse --short HEAD]
**Environment:** [uname -srm + rustc --version]
**Build profile:** profiling (release + debug = true + strip = false)

## 1. Executive Summary
[Top remaining hotspots, ranked. Include the G2 carve-out: explicit Δ for pre_prompt
between 0 plugins and 1 plugin (e.g., "single-plugin pre_prompt overhead = X µs").]

## 2. Methodology
### 2.1 Workloads
[Define W-P1 / W-P3 / W-P5 with the exact driver commands.]

### 2.2 Fixture
[Describe perf_plugin: 3 commands, 3 hooks, no I/O side effects, CAP_VARIABLES_READ only.]

### 2.3 yosh-dhat extensions
[Describe --exec-loop and --pre-prompt-loop flags and why they exist (interactive proxy).]

### 2.4 Build profile
[Same paragraph as performance.md §2.3.]

## 3. Results

### 3.1 W-P1: pre_prompt heavy

#### Criterion (HEAD <sha>)
| Bench | Median | Min | Max |
|-------|--------|-----|-----|
| plugin_pre_prompt_zero_plugins | ? | ? | ? |
| plugin_pre_prompt_one_noop    | ? | ? | ? |
| plugin_pre_prompt_three_noop  | ? | ? | ? |

#### dhat Top-10 by bytes
[Paste from target/perf/dhat-plugin-w1.md]

#### dhat Top-10 by call count
[Paste from same JSON, by-calls view]

#### samply Top-10 total time
[Paste from target/perf/samply-plugin-w1.md]

### 3.2 W-P3: command throughput
[Same structure as 3.1, but for the script-driven workload.]

### 3.3 W-P5: startup with plugins
[Same structure, with the n=0/1/3 Criterion comparison and dhat for n=1.]

## 4. Findings
[One subsection per identified hotspot. For each:
 - Location (file:line)
 - Measurement (which workload, which metric)
 - Suspected cause (with code-reading evidence)
 - Fix candidates (ordered by plausibility)]

## 5. Recommendations

### 5.1 Priority matrix
[Impact (High/Medium/Low) × Effort (Low/Medium/High); P0/P1/P2 assignment.]

### 5.2 Next-project queue
[Ordered list of Phase 2 spec candidates.]

### 5.3 Items to add to TODO.md
[Anything that didn't make the Phase 2 cut.]

## 6. Reproducibility
[Verbatim commands from the spec §10, tightened to whatever HOME / config staging
is the actual layout used.]
```

Fill in every `?` and `[...]` with the data captured in Steps 1–5.

- [ ] **Step 7: Self-review the report**

Per the writing-plans self-review pattern: skim the report and check for:
- Placeholder text (`?`, `TBD`, `TODO`) — fix inline.
- Internal consistency (do the §4 findings match the §3 tables they cite?).
- At least three findings ranked in §5.1.
- §1 Executive Summary explicitly states the pre_prompt Δ in µs (G2 requirement).

- [ ] **Step 8: Commit**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md
git commit -m "docs(plugin-perf): add Phase 1 measurement report with Top-N hotspots"
```

---

## Task 14: Update TODO.md with deferred items

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: For each finding in the report's §5.3, add a TODO entry**

Format (matches existing TODO.md style):

```markdown
- [ ] Plugin perf: <one-line description> — <attribution to report §X.Y>. <Path>:<line>.
```

Example (illustrative; actual entries depend on what the report finds):

```markdown
- [ ] Plugin perf: `PluginManager::call_pre_prompt` iterates all plugins even when none implement the hook — see `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` §4.1. `src/plugin/mod.rs:581`.
```

- [ ] **Step 2: Mark the resolved TODO entries as completed**

The "Future: Plugin System Enhancements" section already has these items that this Phase 1 work resolves:

- `benches/plugin_bench.rs` output noise — resolved by Task 8 (perf_plugin has no `print()` calls).
- (Optional) Cargo workspace profile warning — only mark resolved if Task 1 actually addressed it; otherwise leave.

Per CLAUDE.md convention "Delete completed items rather than marking them with `[x]`", remove the resolved lines from TODO.md.

- [ ] **Step 3: Run the full test suite to verify no regression**

```bash
cargo test --features test-helpers
./e2e/run_tests.sh
```

Expected: green. Per CLAUDE.md, all tests pass before commit.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "docs(todo): record Phase 1 plugin-perf findings; remove resolved items"
```

---

## Phase 1 Definition of Done

After Task 14:
- `perf_plugin` builds and is exercised by integration tests.
- `yosh-dhat` has `--exec-loop` and `--pre-prompt-loop` modes.
- `benches/plugin_bench.rs` has 8 benches (3 retargeted + 5 new).
- `benches/startup_bench.rs` has 3 benches (existing + 2 new).
- `benches/data/plugin_w3.sh` exists and is exercised by Task 13.
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` exists with §1–§6 populated and at least 3 ranked findings.
- The §1 Executive Summary states an explicit `pre_prompt` Δ in µs (G2 carve-out).
- `TODO.md` records every Phase 2 candidate not picked up immediately.
- `cargo test --features test-helpers` and `./e2e/run_tests.sh` are green at the final commit.

Phase 2 onward picks the highest-priority finding from the report's §5.2 and writes a per-fix design spec at `docs/superpowers/specs/YYYY-MM-DD-plugin-<short-name>-design.md`.
