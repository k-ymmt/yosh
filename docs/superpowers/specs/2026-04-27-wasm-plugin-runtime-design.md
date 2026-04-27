# WASM Plugin Runtime Migration — Design

**Date:** 2026-04-27
**Scope:** Migrate yosh's plugin execution from `dlopen`-loaded shared libraries (`.dylib` / `.so`) to WebAssembly Components executed via `wasmtime`, exposing the host API through the WIT-based Component Model.
**Target release:** v0.2.0 (breaking change)

## 1. Goals and Non-Goals

### Goals

- Replace `libloading`-based plugin loading with `wasmtime` Component Model execution
- Publish the host API as a versioned WIT package (`yosh:plugin@0.1.0`) so plugins can be authored in any language with `wit-bindgen` support
- Preserve the existing capability allowlist (`plugins.toml` `capabilities`) and the dlopen-era graceful-degradation semantics
- Eliminate platform-specific build pipelines for plugins (single `wasm32-wasip2` target replaces the current 4-platform matrix)
- Remove the macOS ad-hoc resign workaround introduced in commit `abaa1aa`
- Keep `Plugin` trait + `export!` macro ergonomics in the Rust SDK so existing plugin code shape carries over

### Non-Goals

- Adding new plugin capabilities (prompt-segment API, completion hooks, runtime load/unload). These remain on `TODO.md`.
- Introducing fuel / memory / wall-clock execution limits. dlopen had none; preserve scope.
- Supporting OCI-registry distribution (`oci:` source). Stays a future minor-bump addition.
- Async hooks / pre-prompt timeout. Remains a separate `TODO.md` item.

### Compatibility window

**Clean cut.** No dual-stack between dlopen and wasm. Crates.io has had v0.1.5 for a short window with no externally visible plugins beyond the in-tree `tests/plugins/test_plugin`, so the breakage cost is bounded.

## 2. Architecture Overview

```
[Plugin author]
  Rust crate using cargo-component
    wit/world.wit imports yosh:plugin/*
    yosh-plugin-sdk (wit-bindgen wrapper) provides Plugin trait
    `cargo component build --release --target wasm32-wasip2`
      → target/wasm32-wasip2/release/<name>.wasm
      → published as a single asset on a GitHub release

[User]
  ~/.config/yosh/plugins.toml describes desired plugins
  `yosh-plugin sync`
    - downloads <name>.wasm
    - SHA-256 verifies
    - precompiles to ~/.cache/yosh/plugins/<sha>.cwasm (eager)
    - writes plugins.lock with cwasm_path + wasmtime_version

[yosh shell process]
  Single shared wasmtime::Engine
  Per plugin:
    Component::deserialize(&engine, cwasm_bytes)
    Linker built from capability allowlist
    InstancePre = linker.instantiate_pre(&component)
    Store<HostContext> created once, reused across all calls
  Command / hook dispatch:
    Update Store::data_mut().env to current ShellEnv pointer
    Call the instance's WIT export
    Reset env pointer to null on return
```

### Core principles

1. **In-process execution.** No fork-and-probe sandbox process. Single wasmtime runtime inside the shell process, same threading model as dlopen.
2. **WIT is the only contract.** No C ABI, no `unsafe extern "C" fn`. The SDK becomes a `wit-bindgen` wrapper; the host uses `wasmtime::component::bindgen!`.
3. **Capability == linker import policy.** `plugins.toml`'s `capabilities` array decides which host import is registered with the real implementation vs a deny-stub. Hooks (exports) are governed by dispatch suppression in the host.
4. **WASI minimised.** Only `wasi:clocks` (monotonic + wall) and `wasi:random/random` are linked. `wasi:cli/*`, `wasi:filesystem`, `wasi:sockets` are deliberately omitted to keep the capability boundary the only sanctioned surface.
5. **AOT cache absorbs latency.** Precompiled `.cwasm` per `<sha256>.cwasm` keyed on the wasm's SHA-256, with a fallback path on wasmtime-version mismatch.

## 3. WIT Interface Definition

File: `crates/yosh-plugin-api/wit/yosh-plugin.wit`

```wit
package yosh:plugin@0.1.0;

interface types {
    enum error-code {
        denied,
        invalid-argument,
        io-failed,
        not-found,
        other,
    }

    enum stream {
        stdout,
        stderr,
    }

    record plugin-info {
        name: string,
        version: string,
        commands: list<string>,
    }
}

interface variables {
    use types.{error-code};
    get:    func(name: string) -> option<string>;
    set:    func(name: string, value: string) -> result<_, error-code>;
    export: func(name: string, value: string) -> result<_, error-code>;
}

interface filesystem {
    use types.{error-code};
    cwd:     func() -> string;
    set-cwd: func(path: string) -> result<_, error-code>;
}

interface io {
    use types.{stream, error-code};
    write: func(target: stream, data: list<u8>) -> result<_, error-code>;
}

interface plugin {
    use types.{plugin-info};
    metadata:  func() -> plugin-info;
    on-load:   func() -> result<_, string>;
    exec:      func(command: string, args: list<string>) -> s32;
    on-unload: func();
}

interface hooks {
    pre-exec:   func(command: string);
    post-exec:  func(command: string, exit-code: s32);
    on-cd:      func(old-dir: string, new-dir: string);
    pre-prompt: func();
}

world plugin-world {
    import variables;
    import filesystem;
    import io;
    import wasi:clocks/monotonic-clock@0.2.0;
    import wasi:clocks/wall-clock@0.2.0;
    import wasi:random/random@0.2.0;

    export plugin;
    export hooks;
}
```

### Interface design notes

- **`io.write` uses `list<u8>`** for byte-transparent output. Variable names/values keep `string` (UTF-8); POSIX shell variable identifiers cannot contain NUL, so this is lossless in practice.
- **`variables:read` and `variables:write` share a single interface.** Function-level deny-stubs in the host linker discriminate. This keeps the SDK's plugin-side dependency surface minimal (one `Cargo.toml` entry per interface).
- **Hooks export is mandatory in WIT** — the SDK provides empty default implementations so plugins that do not implement a given hook still link successfully. Dispatch suppression on the host side prevents unnecessary boundary crossings when the matching capability is denied.
- **`plugin-info` carries no capability list.** WIT import structure is itself the capability declaration. The dlopen-era `YOSH_PLUGIN_API_VERSION` constant is replaced by WIT package semver.
- **All interfaces share `types`** for `error-code` and `stream`, providing a single source of truth for cross-cutting types.

### WIT semver policy

- Package version is `yosh:plugin@MAJOR.MINOR.PATCH`.
- During the 0.x window, minor bumps may carry breaking changes. Initial publication is `0.1.0`, coinciding with this migration.
- Post-1.0, only major bumps may break compatibility.

## 4. Crate Layout and Component Roles

| Crate | Current role | New role | Change scope |
|---|---|---|---|
| `crates/yosh-plugin-api` | C ABI structs (`HostApi`, `PluginDecl`) and `CAP_*` bitflags | **WIT package holder + shared types crate.** Ships `wit/yosh-plugin.wit`. Public Rust types reduce to the `Capability` enum, `parse_capability(&str)`, and `CAP_*` bitflag constants (still `pub const u32`, no longer C-ABI-related). | Medium (full file replacement) |
| `crates/yosh-plugin-sdk` | `Plugin` trait + `export!` macro + `style` | **`wit-bindgen` wrapper.** `Plugin` trait kept, signatures rewritten in WIT-derived types. `export!` rewritten as a bridge between the trait impl and `wit_bindgen::generate!`-emitted `Guest` impl. `style` module unchanged. | Large (macro rewrite) |
| `crates/yosh-plugin-manager` | TOML / lockfile / GitHub releases / SHA-256 / macOS resign | Existing logic kept. Changes: asset template default, removal of macOS ad-hoc resign code, new precompile step and lockfile fields. | Small-to-medium |
| `src/plugin/` | `libloading::Library` + `unsafe extern "C" fn` deny-stubs | Full rewrite around `wasmtime`: `Engine` / `Component` / `InstancePre` / `Store<HostContext>`. Capability-aware `Linker` builder. | Large |
| `tests/plugins/test_plugin/` | `crate-type = ["cdylib"]` dylib | wasm component built via `cargo component build --target wasm32-wasip2` | Medium |

### Workspace dependency changes

**Add (root `Cargo.toml`):**

- `wasmtime` with features `["component-model", "cranelift", "cache"]` (no async)
- `wasmtime-wasi` (only `clocks` and `random` are linked)
- `wit-bindgen` (used as a macro inside the SDK, no `build.rs` needed)

**Add (`crates/yosh-plugin-manager/Cargo.toml`):**

- `wasmtime` with features `["component-model", "cranelift"]` (no `cache` feature; sync-time precompile writes its own files)

**Remove (root `Cargo.toml`):**

- `libloading`

Concrete version pins (e.g., `wasmtime = "27"`) are decided during implementation against the latest stable releases at that time.

### File layout additions

```
crates/yosh-plugin-api/
├── Cargo.toml
├── wit/
│   └── yosh-plugin.wit            # NEW: single WIT source of truth
└── src/
    └── lib.rs                      # REPLACED: Capability enum + parse_capability + CAP_* bitflags

crates/yosh-plugin-sdk/
├── Cargo.toml
├── (no build.rs)
└── src/
    ├── lib.rs                      # wit_bindgen::generate!() + Guest impl + Plugin trait
    ├── export.rs                   # NEW: export! macro
    └── style.rs                    # unchanged

src/plugin/
├── mod.rs                          # REPLACED: PluginManager (wasmtime)
├── config.rs                       # mostly unchanged (capability string parsing)
├── host.rs                         # NEW: HostContext + host import implementations
├── linker.rs                       # NEW: capability allowlist → Linker construction
└── (cache module not needed; precompile lives in yosh-plugin-manager)
```

### Bindings generation timing

- **Host side** (`src/plugin/`): `wasmtime::component::bindgen!` macro invoked at compile time, reading `crates/yosh-plugin-api/wit/yosh-plugin.wit`. No `build.rs`.
- **Plugin side** (any crate using `yosh-plugin-sdk`): `wit_bindgen::generate!` macro inside the SDK, transitively reading the same WIT through the `yosh-plugin-api` `path =` dependency.

The single WIT file at `crates/yosh-plugin-api/wit/yosh-plugin.wit` is the authoritative source consumed by both host and SDK.

## 5. Execution Model

### Engine

A single shared `wasmtime::Engine` is constructed in `PluginManager::new()`:

```rust
let mut config = wasmtime::Config::new();
config.async_support(false);
config.consume_fuel(false);
config.cache_config_load_default()?;
let engine = Engine::new(&config)?;
```

Synchronous execution only (matches the existing shell execution loop). Fuel / epoch interruption are out of scope for v0.2.0.

### Per-plugin load sequence

For each enabled `plugins.lock` entry, at shell startup:

1. Validate `cwasm_path` and `wasmtime_version`. On mismatch, precompile in place and log a warning.
2. `Component::deserialize(&engine, cwasm_bytes)?`
3. `linker = build_linker(&engine, entry.capabilities)` — see §6 for details.
4. `instance_pre = linker.instantiate_pre(&component)?`
5. `store = Store::new(&engine, HostContext::new(plugin_name))`
6. `instance = instance_pre.instantiate(&mut store)?`
7. Call `plugin.metadata(&mut store)?` to obtain commands.
8. Call `plugin.on_load(&mut store)?`. On `Err`, the plugin is rejected and not added to the manager (matches dlopen behaviour).
9. Push a `LoadedPlugin { store, instance, commands, capabilities, has_pre_exec, has_post_exec, has_on_cd, has_pre_prompt }` entry.

### Store lifecycle — per-plugin, persistent

A `Store<HostContext>` is created once during load and **kept alive for the lifetime of the `LoadedPlugin`** (≈ shell process lifetime). Each command / hook dispatch reuses the same store and instance.

Rationale:

- Plugin-side global state (counters, caches, opened resources) persists across calls, matching dlopen behaviour.
- WASM linear memory reallocation cost is paid once at startup, not per call.
- Host context updates are done via `Store::data_mut()` — a cheap pointer write — rather than full reinstantiation.

The alternative of fresh instantiation per call is rejected: it nullifies the value of `instance_pre`, breaks the dlopen-compatible state model, and offers no security gain (capability sandboxing is the actual boundary).

### `HostContext`

```rust
struct HostContext {
    /// Raw pointer to the live ShellEnv. Updated immediately before each
    /// guest-bound call and reset to null on return. Confined to a single
    /// helper that is the only `unsafe` site in the host binding layer.
    env: *mut ShellEnv,
    plugin_name: String,
    capabilities: u32,
}
```

`ShellEnv` is owned by the shell main loop and cannot be borrowed into the `Store<HostContext>` for the store's lifetime without conflict. The pattern resolves this by:

1. Storing a raw `*mut ShellEnv` in `HostContext`.
2. On each command/hook dispatch, the host calls `store.data_mut().env = env_ptr` immediately before invoking the WIT export, then resets to `ptr::null_mut()` on return.
3. Host import closures dereference the pointer through a single helper that is the only `unsafe` block in the new layer. This is a tighter `unsafe` perimeter than the dlopen version's eight `unsafe extern "C" fn` callbacks.

### Command dispatch

```rust
// PluginManager::exec_command(env, name, args) -> Option<i32>
let plugin = self.find_plugin_for_command(name)?;
plugin.store.data_mut().env = env as *mut _;
let exit = plugin.bindings
    .yosh_plugin_plugin()
    .call_exec(&mut plugin.store, name, args)
    .unwrap_or(1);   // trap → 1 (see §5 error handling)
plugin.store.data_mut().env = ptr::null_mut();
Some(exit)
```

### Hook dispatch

```rust
for plugin in &mut self.plugins {
    if !has(plugin.capabilities, CAP_HOOK_PRE_EXEC) { continue; }
    if !plugin.has_pre_exec { continue; }
    plugin.store.data_mut().env = env_ptr;
    let _ = plugin.bindings
        .yosh_plugin_hooks()
        .call_pre_exec(&mut plugin.store, cmd);
    plugin.store.data_mut().env = ptr::null_mut();
}
```

`has_pre_exec` etc. are recorded at load time. The SDK provides empty default implementations for unimplemented hooks so the WIT export is always present, but dispatch suppression avoids unnecessary boundary crossings when the capability is denied or the plugin trait did not override the hook.

### Error / trap / panic isolation

| Failure mode | Behaviour |
|---|---|
| WIT function returns `result::err(...)` | Plugin's explicit signal. Error is logged to stderr and propagated as call failure (exit code 1 for commands). |
| WASM trap (unreachable, OOB, division by zero) | wasmtime raises `Trap`. Host catches it, logs `yosh: plugin '<name>': trapped: <reason>` to stderr, marks the instance as invalidated. Subsequent calls to the same plugin become no-ops with a warning. Shell process continues. |
| Plugin-side Rust panic | Compiles to `abort` in wasm; surfaces as a trap (same handling). |
| `on-load` returns `Err(_)` | Plugin is dropped from the manager, matching dlopen behaviour. |
| Host import returns `Err(error-code::denied)` | Observable via `Result` on the plugin side. The host emits no additional warning (the WIT error value is the canonical signal; double-logging is avoided). The dlopen-era load-time "capability requested but not granted" warning is preserved separately (§6). |

### Memory and time limits — out of scope

`Store::limiter`, `consume_fuel`, and `epoch_interruption` are all available in wasmtime but are not adopted in v0.2.0. The dlopen plugin system had no such limits; introducing them in this migration would expand scope beyond the §1 goals. They are tracked as a `TODO.md` follow-up: "Plugin runtime limits (fuel / memory caps / pre-prompt timeout)".

## 6. Capability Model and Linker Construction

### Capability string set — preserved verbatim

```
variables:read
variables:write
filesystem
io
hooks:pre_exec
hooks:post_exec
hooks:on_cd
hooks:pre_prompt
```

`plugins.toml` schema and parser (`src/plugin/config.rs::capabilities_from_strs`) are reused as-is. The `CAP_*` `pub const u32` constants in `yosh-plugin-api` remain (non-C-ABI use).

### Capability-to-WIT mapping

| Capability | WIT target | Linker treatment |
|---|---|---|
| `variables:read` | `yosh:plugin/variables.get` | granted: `host_get_var` / denied: `deny_get_var` (returns `None`) |
| `variables:write` | `yosh:plugin/variables.set` and `.export` | granted: real impl / denied: deny-stub returning `Err(Denied)` |
| `filesystem` | `yosh:plugin/filesystem.cwd` and `.set-cwd` | granted: real impl / denied: deny-stub |
| `io` | `yosh:plugin/io.write` | granted: real impl / denied: deny-stub |
| `hooks:pre_exec` | `yosh:plugin/hooks.pre-exec` (export) | dispatch suppression in `PluginManager::call_pre_exec` |
| `hooks:post_exec` | `yosh:plugin/hooks.post-exec` | dispatch suppression |
| `hooks:on_cd` | `yosh:plugin/hooks.on-cd` | dispatch suppression |
| `hooks:pre_prompt` | `yosh:plugin/hooks.pre-prompt` | dispatch suppression |

### `build_linker` — sketch

`src/plugin/linker.rs`:

```rust
pub fn build_linker(
    engine: &Engine,
    allowed: u32,
) -> Result<Linker<HostContext>, Error> {
    let mut linker = Linker::<HostContext>::new(engine);

    // Limited WASI: clocks + random only.
    wasmtime_wasi::p2::clocks::monotonic_clock::add_to_linker(&mut linker, |c| c)?;
    wasmtime_wasi::p2::clocks::wall_clock::add_to_linker(&mut linker, |c| c)?;
    wasmtime_wasi::p2::random::random::add_to_linker(&mut linker, |c| c)?;
    // wasi:cli, wasi:filesystem, wasi:sockets are intentionally NOT linked.

    let mut vars = linker.instance("yosh:plugin/variables@0.1.0")?;
    vars.func_wrap("get",
        if has(allowed, CAP_VARIABLES_READ)   { host_get_var }    else { deny_get_var })?;
    vars.func_wrap("set",
        if has(allowed, CAP_VARIABLES_WRITE)  { host_set_var }    else { deny_set_var })?;
    vars.func_wrap("export",
        if has(allowed, CAP_VARIABLES_WRITE)  { host_export_var } else { deny_export_var })?;

    let mut fs = linker.instance("yosh:plugin/filesystem@0.1.0")?;
    fs.func_wrap("cwd",
        if has(allowed, CAP_FILESYSTEM)       { host_cwd }     else { deny_cwd })?;
    fs.func_wrap("set-cwd",
        if has(allowed, CAP_FILESYSTEM)       { host_set_cwd } else { deny_set_cwd })?;

    let mut io = linker.instance("yosh:plugin/io@0.1.0")?;
    io.func_wrap("write",
        if has(allowed, CAP_IO)               { host_write } else { deny_write })?;

    Ok(linker)
}
```

Specific function signatures (notably `wasmtime_wasi::p2::*` linker addition paths) are validated against the pinned wasmtime version during implementation; the structure above is the design intent.

### Requested-vs-granted detection

dlopen logged the `requested & !granted` capability set at load time (`log_denied_capabilities`). The wasm version reproduces this:

1. At load time, enumerate the component's actual imports via `Component::component_type(&engine)`.
2. Map function names back to capability bits via a lookup table (the inverse of the §6.2 table).
3. The OR of those bits is `requested`; `denied = requested & !allowed`.
4. For each bit in `denied`, log: `yosh: plugin '<name>': capability '<cap>' requested but not granted`.

This is configuration-information feedback (not a runtime warning), preserved from dlopen for parity with users' debugging muscle memory.

### Deny-stub semantics

Deny-stubs return `Err(error-code::denied)` and log nothing. The plugin-side SDK observes the `Result` and either handles it explicitly or surfaces it through error propagation; if the plugin author unwraps the `Err`, the resulting trap is logged at the trap-handling layer (§5). Either path is observable, eliminating the dlopen-era risk of silent capability bypasses going unnoticed.

The single load-time warning above (configuration mismatch) is retained because it is informational rather than a runtime error signal.

## 7. plugins.toml / plugins.lock and `yosh-plugin` Manager

### `plugins.toml` schema — unchanged except defaults

```toml
[[plugin]]
name = "git-status"
source = "github:user/yosh-plugin-git-status"
version = "1.2.0"
enabled = true
capabilities = ["variables:read", "io"]
asset = "{name}.wasm"
```

Asset template tokens are reduced to:

| Token | Value |
|---|---|
| `{name}` | Plugin name as written in `[[plugin]].name` (no underscore normalization; wasm files are platform-independent) |

`{os}`, `{arch}`, `{ext}` tokens are removed. If a `plugins.toml` written for v0.1.x still contains them, the manager rejects the asset template at sync time with a clear migration hint.

### `plugins.lock` schema — two new fields

```toml
[[plugin]]
name             = "git-status"
source           = "github:user/yosh-plugin-git-status"
version          = "1.2.0"
path             = "~/.local/share/yosh/plugins/git-status.wasm"
sha256           = "abc123..."
cwasm_path       = "~/.cache/yosh/plugins/abc123.cwasm"
wasmtime_version = "27.0.0"
```

- `cwasm_path` basename is `<sha256>.cwasm`. SHA-256 of the wasm file is the cache key.
- `wasmtime_version` is `wasmtime::VERSION` at the time the cwasm was produced. Compared against the live wasmtime at shell startup to decide whether a refresh is needed.

### `yosh-plugin install` — minor changes

- GitHub URL form unchanged.
- Local path form requires a `.wasm` extension (was `.dylib` / `.so`). Wrong extensions are rejected with a clear error.

### `yosh-plugin sync` — adds eager precompile

```
1. Read plugins.toml.
2. For each enabled entry:
   a. Resolve and download / locate the .wasm file.
   b. Compute SHA-256.
   c. If lockfile entry's SHA-256 differs (or no entry), schedule precompile.
   d. Precompile: Engine::precompile_component(&wasm_bytes) → write
      ~/.cache/yosh/plugins/<sha256>.cwasm.
   e. Write lockfile entry with cwasm_path and wasmtime_version.
3. Atomic replace plugins.lock.
```

Per-plugin precompile failures are reported in the existing `failed` list and do not block other plugins. Exit code matches the existing partial-failure semantics (1 if any failed).

### `yosh-plugin sync --prune`

- Removes orphaned `.wasm` files (existing behaviour).
- Additionally removes orphaned `.cwasm` files in the cache directory.
- Cleans up empty plugin directories (subsumes `TODO.md` item from `crates/yosh-plugin-manager/src/sync.rs`).

### `yosh-plugin update`

Unchanged. Updates `plugins.toml` version and re-invokes sync, so precompile happens transparently.

### `yosh-plugin list` — new state column

```
git-status   1.2.0   github:user/yosh-plugin-git-status   ✓ verified  ✓ cached
my-local     -       local:/path/to/my-local.wasm         ✓ verified  ✗ stale
```

`stale` indicates `wasmtime_version` mismatch with the running manager. Hint text: "will be re-precompiled on next shell startup".

### `yosh-plugin verify`

SHA-256 verification of `.wasm` files only. cwasm files are regenerable cache artefacts and are not signed.

### macOS ad-hoc resign — removed

All `codesign --sign -` invocations introduced in `abaa1aa` are deleted from `crates/yosh-plugin-manager/src/sync.rs`. Neither `.wasm` nor `.cwasm` are Mach-O binaries; signing does not apply. The corresponding `TODO.md` item "Plugin preload validation in a sandbox process" is also closed: WASM trap isolation (§5) provides equivalent crash containment.

### Manager dependency changes

`crates/yosh-plugin-manager/Cargo.toml`:

- Added: `wasmtime` with `["component-model", "cranelift"]` features for precompile.
- No removals (the manager never depended on `libloading` directly).

The bundled `yosh-plugin` binary (per commit `b24cb6d`) shares the wasmtime statics with the host, so the binary-size impact of adding wasmtime to the manager is zero in the shipped build.

## 8. Test Strategy

### Layered coverage

| Layer | Location | Targets |
|---|---|---|
| Host unit | `src/plugin/{linker,host,cache}.rs::tests` | capability-bit-to-linker mapping, HostContext pointer update, deny-stub returning `Err(Denied)`, cwasm version-mismatch detection |
| Host integration | `tests/plugin.rs` (extended) | Real wasm component load, command exec, hook dispatch, capability denial, WASM trap isolation, cwasm fallback after corruption |
| Plugin build pipeline | `tests/plugins/test_plugin/` | wasm component builds for `wasm32-wasip2`; CI consumes the resulting `.wasm` |
| End-to-end | `e2e/plugin/` (new directory) | Spawn `yosh -c '<plugin-provided command>'`; verify stdout / exit code |

### `tests/plugins/test_plugin/` — wasm conversion

`Cargo.toml` keeps `crate-type = ["cdylib"]` (cargo-component requires it) and gains:

```toml
[package.metadata.component]
package = "yosh:test-plugin"

[package.metadata.component.target]
path  = "../../../crates/yosh-plugin-api/wit"
world = "plugin-world"
```

`src/lib.rs` is rewritten against the new SDK's `Plugin` trait + `export!` macro.

### Build pipeline — adopt `cargo-component`

The plugin build is `cargo component build -p test_plugin --target wasm32-wasip2 --release`. This is preferred over assembling core wasm + `wasm-tools component new --adapt wasi_snapshot_preview1` manually because:

- cargo-component is the de-facto Rust toolchain for Component Model authoring.
- It internalises WIT resolution, world targeting, and WASI adapter injection.
- The two-stage manual pipeline would multiply maintenance points (adapter pinning, post-processing scripts).
- The CI cost (`cargo install cargo-component` once, cached) is amortised across all subsequent builds.

`mise.toml` pins `cargo-component` to a specific version for reproducibility across local development, CI, and `release.sh`.

### Integration test helper

`tests/plugin.rs` introduces:

```rust
static TEST_PLUGIN_WASM: OnceLock<PathBuf> = OnceLock::new();

fn ensure_test_plugin_built() -> &'static Path {
    TEST_PLUGIN_WASM.get_or_init(|| {
        let status = Command::new("cargo")
            .args(["component", "build",
                   "-p", "test_plugin",
                   "--target", "wasm32-wasip2",
                   "--release"])
            .status()
            .expect("cargo component build failed");
        assert!(status.success());
        workspace_root().join("target/wasm32-wasip2/release/test_plugin.wasm")
    })
}
```

The existing `TEST_LOCK: Mutex<()>` is preserved across all 17 call sites; the `unwrap_or_else(|e| e.into_inner())` poisoning fix from `TODO.md` is applied as part of the same migration touching every site.

### New required test cases

1. **Capability allowlist applied to linker.** A plugin granted only `variables:read` calls `variables.set` and observes `Err(Denied)`.
2. **WASM trap isolation.** A `tests/plugins/trap_plugin/` triggers `unreachable!()`; the test verifies that a subsequent unrelated command on the host still succeeds.
3. **cwasm cache validity.** Tampering with the recorded `wasmtime_version` causes startup-time fallback that re-precompiles, emits a warning, and proceeds.
4. **WASI surface lockdown.** A plugin that imports `wasi:cli/stdout` fails to link at load time (this is the negative test for the §2 sandboxing principle).
5. **Boundary-crossing benchmark.** `benches/plugin_bench.rs` measures `variables.get` calls in a tight loop; baseline established for regression detection.

### CI changes

- Add `rustup target add wasm32-wasip2`.
- `cargo install cargo-component --locked --version <pin>`, version aligned with `mise.toml`.
- Add `cargo component build -p test_plugin --target wasm32-wasip2 --release` ahead of `cargo test`.
- The cross-platform plugin build matrix (`x86_64-darwin` / `aarch64-darwin` / `x86_64-linux` / `aarch64-linux` × `.dylib`/`.so`) collapses to a single `wasm32-wasip2` target.
- The shell binary `yosh` continues to ship for the same four platforms.

### Tests removed

- dylib-specific cases in `tests/plugin.rs` (dlopen failure paths, symbol lookup errors, ABI version mismatch).
- dylib asset-name cases in `crates/yosh-plugin-manager/tests/`.
- macOS ad-hoc resign tests.

## 9. Migration Plan

### Sub-projects (implementation order)

| # | Sub-project | Primary files | Depends on |
|---|---|---|---|
| 1 | WIT definition + `yosh-plugin-api` repurpose | `crates/yosh-plugin-api/wit/yosh-plugin.wit`, `crates/yosh-plugin-api/src/lib.rs` (full replacement: `Capability` enum + `parse_capability` + `CAP_*` consts) | none |
| 2 | `yosh-plugin-sdk` rewrite as `wit-bindgen` wrapper | `crates/yosh-plugin-sdk/src/{lib,export}.rs`; remove `build.rs` | #1 |
| 3 | Test plugin migration to wasm component | `tests/plugins/test_plugin/Cargo.toml` (add `package.metadata.component`), `src/lib.rs` rewrite | #2 |
| 4 | Host `PluginManager` rewrite around wasmtime | `src/plugin/{mod,host,linker}.rs`; root `Cargo.toml` removes `libloading`, adds `wasmtime` / `wasmtime-wasi` | #1 |
| 5 | `yosh-plugin-manager` precompile + asset simplification | `crates/yosh-plugin-manager/src/{precompile,sync,install,lockfile}.rs`; `Cargo.toml` adds `wasmtime`; remove macOS resign code | #1, #4 |
| 6 | Integration test rewrite | `tests/plugin.rs` (new test cases for trap isolation, cwasm fallback, WASI lockdown); remove dylib-specific cases | #3, #4 |
| 7 | CI / mise / scripts | `mise.toml` adds cargo-component pin; CI workflow adds wasm32-wasip2 install + cargo component build; `e2e/run_tests.sh` precompile step | #3 |
| 8 | Documentation | `docs/kish/plugin.md` full revision; `CLAUDE.md` plugin pipeline references; `TODO.md` items removed | #1–#7 |

### Release plan

- v0.2.0 single bundled release (breaking change).
- Pre-existing `.dylib` plugins from v0.1.x will not load. Limited blast radius: no externally observable plugins beyond the in-tree test plugin.
- Cross-build configuration:
  - Host `yosh` binary: continues to ship for x86_64-darwin / aarch64-darwin / x86_64-linux / aarch64-linux.
  - Plugins (responsibility of plugin authors): single `wasm32-wasip2` target. The plugin-related cross-build comments in `release.sh` are removed.
- `release.sh` itself needs only minor updates around the test-plugin build invocation.

### Removal checklist (delete in v0.2.0)

| Target | Location |
|---|---|
| `libloading` dependency | root `Cargo.toml` |
| C ABI structs (`HostApi`, `PluginDecl`) and old `CAP_*` documentation | `crates/yosh-plugin-api/src/lib.rs` (full replacement) |
| `unsafe extern "C" fn` host callbacks (incl. deny stubs) | `src/plugin/mod.rs` (full replacement) |
| `export!` macro C-ABI generation block | `crates/yosh-plugin-sdk/src/lib.rs` |
| macOS ad-hoc resign code | `crates/yosh-plugin-manager/src/sync.rs` |
| `lib{name}-{os}-{arch}.{ext}` asset template documentation | `crates/yosh-plugin-manager/src/sync.rs`, `docs/kish/plugin.md` |
| `TODO.md` item: "Plugin preload validation in a sandbox process" | `TODO.md` |
| `TODO.md` item: "SemVer API version management" (replaced by WIT semver) | `TODO.md` |
| `TODO.md` item: "SDK `export!` macro `unsafe` lint" (no longer applicable) | `TODO.md` |
| `TODO.md` item: "Sandbox: `CAP_ALL` manual sync risk" (machine-derivable from `Capability` enum) | `TODO.md` |

### Documentation revision plan for `docs/kish/plugin.md`

- **User Guide:** Update the `plugins.toml` examples (paths now end in `.wasm`); structure is otherwise unchanged.
- **Plugin Development Guide:** Full rewrite. `crate-type = ["cdylib"]` setup is replaced by a `package.metadata.component` block; build commands move from `cargo build --release` to `cargo component build --target wasm32-wasip2 --release`. The cross-platform release matrix section collapses to a single `wasm32-wasip2` artefact.
- **Architecture:** "dlopen + capability bitflags + deny stubs" is rewritten as "wasmtime Component Model + capability-aware Linker + `Err(Denied)`".

## 10. Risks and Open Questions

| Risk / open item | Mitigation |
|---|---|
| **wasmtime API churn** — particularly `wasmtime-wasi` linker addition paths (`p2::clocks::*`) shifting between major versions. | Pin `wasmtime` to a specific major in `Cargo.toml` (likely `27`). Validate the exact module paths during sub-project #4 implementation. Upgrades are coordinated via `release.sh`. |
| **cargo-component pre-1.0 stability** | Pin in `mise.toml` and CI. Lockstep upgrades only. |
| **WIT semver discipline** | Document explicitly in `docs/kish/plugin.md`: the 0.x window allows breaking changes via minor bump; 1.0 transitions to strict semver and aligns with yosh 1.0. |
| **Binary size impact of bundling wasmtime** | Validate during implementation that the `cranelift` feature is necessary at runtime; `winch` is out of scope for v0.2.0 but a future investigation candidate. |
| **`wasi:cli/stderr` import injection from Rust `std`** | The SDK guides plugin authors toward `panic = "abort"` and minimal-`std` builds. If a plugin's component still imports `wasi:cli/stderr`, link fails at load time, producing a clear error rather than silent stdout/stderr leakage. |
| **No fuel / memory caps in v0.2.0** | Documented as `TODO.md` item ("Plugin runtime limits"). Implementations deferred until concrete user-reported needs surface. |

---

## Appendix A — Concrete `LoadedPlugin` shape (host side, sketch)

```rust
struct LoadedPlugin {
    name: String,
    store: Store<HostContext>,
    instance: PluginWorld,                 // bindgen-generated bindings handle
    commands: Vec<String>,
    capabilities: u32,
    has_pre_exec: bool,
    has_post_exec: bool,
    has_on_cd: bool,
    has_pre_prompt: bool,
}
```

`PluginWorld` is the bindings type emitted by `wasmtime::component::bindgen!` from the WIT `world plugin-world`. It exposes `yosh_plugin_plugin()` and `yosh_plugin_hooks()` accessors for the two export interfaces.
