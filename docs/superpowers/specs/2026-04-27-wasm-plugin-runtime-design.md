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
    - precompiles to ~/.cache/yosh/plugins/<sha>-<engine_hash>-<triple>.cwasm
      (mode 0600, dir 0700, owner-checked)
    - writes plugins.lock with the full cache key tuple:
      (wasm_sha256, wasmtime_version, target_triple, engine_config_hash)
      plus cached metadata (required_capabilities, implemented_hooks)

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

    enum hook-name {
        pre-exec,
        post-exec,
        on-cd,
        pre-prompt,
    }

    /// Static plugin metadata.
    ///
    /// IMPORTANT: `metadata` is the only export that the host calls
    /// without an active `ShellEnv` binding. Implementations MUST NOT
    /// invoke any `yosh:plugin/*` host import (variables, filesystem,
    /// io) from inside `metadata`. Doing so will receive
    /// `error-code::denied` from a synthetic deny-stub regardless of
    /// the granted capabilities. See §5 of the design spec.
    record plugin-info {
        name: string,
        version: string,
        commands: list<string>,
        /// Capabilities the plugin requests, by canonical string
        /// (e.g. "variables:read", "io", "hooks:pre_exec"). The host
        /// uses this for the load-time "requested but not granted"
        /// diagnostic and for `yosh-plugin list` output. Sandboxing
        /// itself is enforced by the linker, not by this list.
        required-capabilities: list<string>,
        /// Subset of hooks that this plugin actually overrides.
        /// Default-implemented (no-op) hooks MUST NOT appear here so
        /// the host can skip dispatch entirely.
        implemented-hooks: list<hook-name>,
    }
}

interface variables {
    use types.{error-code};
    /// Outer `result` carries denial / future failure modes; inner `option`
    /// distinguishes "variable not set" from "variable set to empty string".
    get:    func(name: string) -> result<option<string>, error-code>;
    set:    func(name: string, value: string) -> result<_, error-code>;
    export: func(name: string, value: string) -> result<_, error-code>;
}

interface filesystem {
    use types.{error-code};
    /// `result` so denied access is distinguishable from "cwd is the empty
    /// string", and so future failures (e.g. permission errors when the
    /// host loses access to its own working directory) can be reported.
    cwd:     func() -> result<string, error-code>;
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
- **Hooks export is mandatory in WIT** — the SDK provides empty default implementations so plugins that do not implement a given hook still link successfully. The host uses `plugin-info.implemented-hooks` (declared by the plugin) to decide whether to dispatch at all, avoiding a per-call boundary crossing for un-overridden hooks.
- **`plugin-info` carries `required-capabilities` and `implemented-hooks` explicitly.** WIT import presence cannot be used as a capability-request signal because every SDK-built plugin necessarily imports the full `plugin-world` (the world is fixed). The plugin author declares intent via the `Plugin` trait's `required_capabilities()` and `implemented_hooks()` methods (both returning `&'static [...]`); the SDK's `metadata` glue reads those methods at runtime and serializes the results into `plugin-info`. Rust cannot reflectively detect which default trait methods were overridden, so the `implemented_hooks()` declaration is **explicit**, not autodetected. The dlopen-era `YOSH_PLUGIN_API_VERSION` is still removed in favor of WIT package semver.
- **All interfaces share `types`** for `error-code`, `stream`, and `hook-name`, providing a single source of truth for cross-cutting types.

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

1. Re-verify the SHA-256 of the `.wasm` file at `path` against the lockfile-recorded `sha256`. On mismatch, refuse to load the plugin and log an error. **This check is unconditional — every shell startup re-validates the wasm file.**
2. Validate the `.cwasm` cache key (see §5 "cwasm trust model" below). On any mismatch, treat the cache as missing: re-precompile from the verified `.wasm` into an in-memory `Vec<u8>`, log a warning recommending `yosh-plugin sync`, and proceed.
3. `Component::deserialize(&engine, cwasm_bytes)?` — only after step 1 confirmed wasm integrity and step 2 confirmed cache compatibility.
4. `linker = build_linker(&engine, entry.capabilities)` — see §6 for details.
5. `instance_pre = linker.instantiate_pre(&component)?`
6. `store = Store::new(&engine, HostContext::new_for_plugin(plugin_name, entry.capabilities))`
7. `instance = instance_pre.instantiate(&mut store)?`
8. Call `plugin.metadata(&mut store)?` to obtain `plugin-info` (commands, `required-capabilities`, `implemented-hooks`). The store's `env` pointer remains null during this call; the host's deny-stubs short-circuit any host-API access from inside `metadata` regardless of the granted capabilities (see "metadata contract" below).
9. Compute `denied = required-capabilities & !granted-capabilities` and log per-capability warnings (matches dlopen `log_denied_capabilities` semantics).
10. Bind the env pointer via `with_env`, then call `plugin.on_load(&mut store)?`. On `Err`, drop the plugin (matches dlopen behaviour).
11. Push a `LoadedPlugin { name, store, bindings, plugin_info, capabilities, invalidated: false }` entry (see Appendix A for the full struct shape).

### Store lifecycle — per-plugin, persistent

A `Store<HostContext>` is created once during load and **kept alive for the lifetime of the `LoadedPlugin`** (≈ shell process lifetime). Each command / hook dispatch reuses the same store and instance.

Rationale:

- Plugin-side global state (counters, caches, opened resources) persists across calls, matching dlopen behaviour.
- WASM linear memory reallocation cost is paid once at startup, not per call.
- Host context updates are done via `Store::data_mut()` — a cheap pointer write — rather than full reinstantiation.

The alternative of fresh instantiation per call is rejected: it nullifies the value of `instance_pre`, breaks the dlopen-compatible state model, and offers no security gain (capability sandboxing is the actual boundary).

### cwasm trust model

`.cwasm` is a wasmtime-precompiled artifact: **native code, not validated wasm bytecode**. wasmtime documents `Component::deserialize` as accepting only trusted input. The threat model below states explicitly which trust boundary `.cwasm` lives behind.

**Threat model statement.** Persistent `.cwasm` is **trusted as native code IF AND ONLY IF all of the following hold:**

1. The file is owned by the same uid as the running shell process.
2. The file's mode is `0600`.
3. The containing cache directory is owned by the same uid and has mode `0700`.
4. The cache key tuple recorded both in `plugins.lock` and in the sidecar `.cwasm.meta` matches the live runtime's tuple exactly.
5. The source `.wasm` referenced by `wasm_sha256` exists at `path` and its current SHA-256 matches the lockfile entry.

If **any** condition fails, the cache file is treated as untrusted: it is **not** passed to `Component::deserialize`. The host falls back to in-memory precompile from the SHA-verified `.wasm`.

**Trust boundary.** The trust this places on the `.cwasm` is no greater than the trust placed on any other code in `~/.cache/yosh/` owned by the user — it is a same-uid filesystem trust boundary, not a cryptographic one. A user who can write `0600` files into their own `0700` cache directory has already, by definition, achieved code execution as that user; defending against that scenario is out of scope for any in-process cache.

**Cache key tuple** (recorded in both `plugins.lock` and the sidecar `.cwasm.meta`):

- `wasm_sha256`: SHA-256 of the source wasm file.
- `wasmtime_version`: `wasmtime::VERSION` string.
- `target_triple`: e.g. `aarch64-apple-darwin`. Differs across Rosetta transitions, NFS-shared `~/.cache`, and CI cache restores, all of which the host must reject.
- `engine_config_hash`: stable hash of the `wasmtime::Config` (cache enable, cranelift opt level, fuel/epoch flags). Any flag change triggers regeneration.

**Why no HMAC or signing on `.cwasm`.** The threat model above does not extend `.cwasm` trust beyond the same-uid filesystem boundary. Adding an HMAC keyed by some host secret would only matter if `.cwasm` could be transported between trust domains; this design forbids that (`target_triple` mismatch invalidates cross-host reuse, and same-uid-on-same-host already implies code execution). HMAC adds complexity without a corresponding threat reduction.

### metadata contract — host APIs are unavailable

`plugin.metadata()` is called once per shell startup, before the env pointer is bound. The plugin **MUST NOT** invoke any `yosh:plugin/*` host import from inside `metadata`. To enforce this rather than rely on documentation:

- During the `metadata` call, the linker has been built with the regular host imports, but `Store::data_mut().env` is `null` (the call sequence does not run `with_env`).
- The host import implementations check `env.is_null()` as their first action; if null, they return `Err(error-code::denied)` immediately with no further side effect. The `Plugin` trait's metadata-construction code in the SDK is therefore mechanically prevented from depending on host state.
- `on-load`, `on-unload`, `exec`, and all hooks ARE called under `with_env` and have full host API access subject to capability allowlist.

This is documented in the WIT comment on `plugin-info` (above), in the SDK's `Plugin` trait rustdoc, and asserted by an integration test that registers a plugin whose `metadata` calls `cwd()` and verifies `Err(Denied)` is observed.

### `HostContext`

```rust
struct HostContext {
    /// Raw pointer to the live ShellEnv. Updated by `with_env` immediately
    /// before each guest-bound call and reset to null on return. Confined
    /// to a single helper that is the only `unsafe` site in the host
    /// binding layer. NULL during `metadata` calls (see "metadata contract").
    env: *mut ShellEnv,
    plugin_name: String,
    capabilities: u32,

    // WASI state for `wasi:clocks` / `wasi:random` linker support.
    // `wasmtime_wasi::p2::WasiCtx` carries the random seed and clock policy;
    // `ResourceTable` is required by every wasmtime-wasi P2 host import,
    // even when no resources are exposed by the linked subset, because
    // the generated host bindings receive a `&mut ResourceTable` argument.
    wasi: wasmtime_wasi::p2::WasiCtx,
    resource_table: wasmtime::component::ResourceTable,
}

// Required so wasmtime_wasi::p2::*::add_to_linker can inject host functions
// that read clock state via the standard `WasiView` extractor.
impl wasmtime_wasi::p2::WasiView for HostContext {
    fn ctx(&mut self) -> wasmtime_wasi::p2::WasiCtxView<'_> {
        wasmtime_wasi::p2::WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.resource_table,
        }
    }
}
```

The exact trait/view shape (`WasiView` / `WasiImpl<T>` / `IoView`) varies between wasmtime majors. The pinned `wasmtime` version determines which adapter trait is implemented; the structural intent is "the linker can see `&mut WasiCtx + &mut ResourceTable` from `HostContext`". Sub-project #4 (§9) verifies this against the pinned version with a compile-only unit test that constructs the linker for a no-import sentinel component, so any future `wasmtime` upgrade flagging this signature change fails fast at `cargo build`.

`ShellEnv` is owned by the shell main loop and cannot be borrowed into the `Store<HostContext>` for the store's lifetime without conflict. The pattern resolves this by:

1. Storing a raw `*mut ShellEnv` in `HostContext`.
2. The dispatch wrapper `with_env` (below) manages this pointer's lifetime, ensuring it is reset to null on every exit path including panic/trap unwinding.
3. Host import closures dereference the pointer through a single helper that is the only `unsafe` block in the new layer. This is a tighter `unsafe` perimeter than the dlopen version's eight `unsafe extern "C" fn` callbacks.

### `with_env` — the canonical dispatch wrapper (RAII via `EnvGuard`)

All guest-bound calls that require host API access go through this wrapper. The env-pointer reset is implemented via a `Drop`-based RAII guard so the pointer is restored on **every** exit path — normal return, `Err`, host-import-side Rust panic, or trap unwinding through wasmtime's catch boundary:

```rust
/// RAII guard that ensures `HostContext::env` is null when it is dropped.
/// The lifetime parameter prevents `&mut Store<HostContext>` from being held
/// past the guard's drop point.
struct EnvGuard<'a> {
    store: &'a mut Store<HostContext>,
}

impl<'a> EnvGuard<'a> {
    fn bind(store: &'a mut Store<HostContext>, env: &mut ShellEnv) -> Self {
        store.data_mut().env = env as *mut _;
        EnvGuard { store }
    }

    fn store(&mut self) -> &mut Store<HostContext> { self.store }
}

impl Drop for EnvGuard<'_> {
    fn drop(&mut self) {
        // Always runs — including on panic unwind — because `Drop` is
        // unwinding-safe and the field types here cannot themselves panic
        // during drop.
        self.store.data_mut().env = std::ptr::null_mut();
    }
}

fn with_env<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    f: impl FnOnce(&mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Option<R> {
    if plugin.invalidated {
        eprintln!(
            "yosh: plugin '{}': skipped (instance invalidated by earlier trap)",
            plugin.name
        );
        return None;
    }

    let result = {
        let mut guard = EnvGuard::bind(&mut plugin.store, env);
        f(guard.store())
        // guard drops here, restoring env to null whether `f` returned
        // Ok, Err, or unwound via panic.
    };

    match result {
        Ok(r) => Some(r),
        Err(e) => {
            if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                eprintln!(
                    "yosh: plugin '{}': trapped: {} — disabling for the rest of this session",
                    plugin.name, trap
                );
                plugin.invalidated = true;
            } else {
                eprintln!("yosh: plugin '{}': call failed: {}", plugin.name, e);
            }
            None
        }
    }
}
```

Notes on safety:

- **Panic unwinding through host imports**: a Rust panic raised inside a host import (e.g. invariant violation in `host_get_var`) unwinds through wasmtime's host-call frame back to `f`'s frame. `EnvGuard::drop` runs during that unwind, restoring `env` to null. The panic continues propagating; the shell's outer `catch_unwind` (existing for command execution) catches it and treats it as a host-side bug. The plugin is **not** auto-invalidated for host-side panics — this is a host bug, not a guest bug.
- **Aborting strategy is rejected**: simply requiring `panic = "abort"` for the host binary would also restore env (because abort skips drops, but the process exits anyway), but it would make the shell unusable for everything else. Drop-based guard is the correct boundary tool.
- **The only call site that does NOT use `with_env`** is `metadata` (per "metadata contract" above). For `metadata`, the env pointer is already null and the linker's deny-checking host imports short-circuit on null, so no guard is needed.

This wrapper is used by `exec_command`, all four hook dispatchers, `on_load`, and `on_unload`. The env-pointer reset and invalidation logic lives in exactly one place.

### Command dispatch — three-valued result

`exec_command` returns a dedicated enum so the caller in `src/exec/` cannot accidentally fall through to external command lookup when a plugin handler exists but failed:

```rust
pub enum PluginExec {
    /// No plugin provides this command. The caller falls back to PATH lookup.
    NotHandled,
    /// A plugin handled the command and returned this exit status.
    Handled(i32),
    /// A plugin claimed the command but failed (trap, host error, invalidated).
    /// Distinct from `Handled(1)` so callers can adjust diagnostics.
    Failed,
}

impl PluginManager {
    pub fn exec_command(
        &mut self,
        env: &mut ShellEnv,
        name: &str,
        args: &[String],
    ) -> PluginExec {
        let Some(plugin) = self.find_plugin_for_command_mut(name) else {
            return PluginExec::NotHandled;
        };
        let bindings = plugin.bindings.clone();
        match with_env(plugin, env, |store| {
            bindings.yosh_plugin_plugin().call_exec(store, name, args)
        }) {
            Some(exit) => PluginExec::Handled(exit),
            None => PluginExec::Failed,
        }
    }
}
```

The execution loop in `src/exec/` matches on this enum: `NotHandled` → continue with PATH lookup; `Handled(n)` → use `n`; `Failed` → use exit code 1 and skip PATH lookup (the plugin owned the command, even if it crashed). Mapping `Failed → 1` lives in the caller, not in `with_env`, so future callers (e.g. completion hooks) can surface the failure differently if needed.

### Hook dispatch

```rust
for plugin in &mut self.plugins {
    if !has(plugin.capabilities, CAP_HOOK_PRE_EXEC) { continue; }
    if !plugin.plugin_info.implemented_hooks.contains(&HookName::PreExec) {
        continue;
    }
    let bindings = plugin.bindings.clone();
    let _ = with_env(plugin, env, |store| {
        bindings.yosh_plugin_hooks().call_pre_exec(store, cmd)
    });
}
```

Two filters apply before dispatch:

1. **Capability allowlist** (`plugin.capabilities` bitfield).
2. **Plugin's declared `implemented-hooks`** from `plugin-info`. WIT exports for hooks are mandatory and the SDK provides empty defaults so unimplemented hooks compile. The plugin author explicitly declares which hooks are real overrides via `Plugin::implemented_hooks() -> &'static [HookName]`; the SDK glue serializes the result into `plugin-info`. This eliminates the per-call boundary crossing for default no-op hooks — the previous design's "200 ns × N plugins × 4 hooks" cost is removed entirely.

### Error / trap / panic isolation

All routing is centralised in `with_env`. Direct callers never look at `wasmtime::Error` themselves — they observe `Option<R>` plus the side effect of invalidation/logging.

| Failure mode | Behaviour |
|---|---|
| WIT function returns `result::err(...)` | Plugin's explicit signal. The host caller (e.g. `exec_command`'s caller in `src/exec/`) treats this as a normal `Some(Err(_))` and logs/maps to exit code 1 as appropriate. The plugin instance remains valid. |
| WASM trap (unreachable, OOB, division by zero, stack overflow) | `wasmtime::Error` carries a `wasmtime::Trap`. `with_env` downcasts, logs `yosh: plugin '<name>': trapped: <reason> — disabling for the rest of this session`, sets `LoadedPlugin::invalidated = true`, and returns `None`. All subsequent `with_env` calls for that plugin short-circuit with a single skip warning. |
| Plugin-side Rust panic | Compiles to `abort` in wasm; surfaces as a trap (same handling). |
| `on-load` returns `Err(_)` | Plugin is dropped from the manager (not pushed onto `self.plugins`), matching dlopen behaviour. |
| Host import returns `Err(error-code::denied)` | Observable via `Result` on the plugin side. The host emits no additional runtime warning (the WIT error value is the canonical signal; double-logging is avoided). The load-time "capability requested but not granted" diagnostic is independent and based on `plugin-info.required-capabilities` (§6). |
| Other `wasmtime::Error` (e.g. host-bindings serialization, fuel exhaustion if later enabled) | Logged once with full chain, return `None`. The plugin is **not** invalidated — these are host/policy errors, not guest-state corruption. |

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

### Requested-vs-granted detection — sourced from `plugin-info`

dlopen logged the `requested & !granted` capability set at load time (`log_denied_capabilities`). The wasm version reproduces this from explicit plugin metadata, **not** from import-table introspection:

1. After `metadata` returns, parse `plugin-info.required-capabilities: list<string>` into a bitfield via `parse_capability` (the same parser used for `plugins.toml`).
2. `denied = requested & !allowed`.
3. For each bit in `denied`, log: `yosh: plugin '<name>': capability '<cap>' requested but not granted`.

Why not infer from `Component::component_type()`: every SDK-built plugin necessarily imports the full `plugin-world` (which includes all of `variables`, `filesystem`, `io` regardless of whether the plugin uses any of them at runtime). Inferring "requested" from import presence would mark every plugin as requesting every capability, defeating the diagnostic. Explicit declaration via `plugin-info.required-capabilities` is the only honest signal.

Unknown capability strings in `required-capabilities` (typo, future capability not yet supported by this yosh version) are logged as warnings but do not block the plugin from loading. This subsumes the existing `TODO.md` item "warn on unknown capability strings in `plugins.toml`" — the same parsing/logging path is reused.

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

### `plugins.lock` schema — extended cache key

```toml
[[plugin]]
name             = "git-status"
source           = "github:user/yosh-plugin-git-status"
version          = "1.2.0"
path             = "~/.local/share/yosh/plugins/git-status.wasm"
sha256           = "abc123..."

# Cache key tuple — see §5 "cwasm trust model"
cwasm_path           = "~/.cache/yosh/plugins/abc123-<engine_hash>-<triple>.cwasm"
wasmtime_version     = "27.0.0"
target_triple        = "aarch64-apple-darwin"
engine_config_hash   = "sha256-of-stable-config-fingerprint"

# Cached metadata so `yosh-plugin list` does not need to instantiate
# the plugin. Refreshed at sync time.
required_capabilities = ["variables:read", "io"]
implemented_hooks     = ["pre-prompt"]
```

- The cache key tuple is `(wasm_sha256, wasmtime_version, target_triple, engine_config_hash)`. All four pieces are recorded both in `plugins.lock` and in a sidecar `<basename>.cwasm.meta` next to each cwasm. The sidecar makes orphan cwasm files self-describing for `--prune` and integrity diagnostics.
- `cwasm_path` filename embeds enough of the tuple to avoid collisions when multiple host triples share `~/.cache` (e.g. dotfiles repo synced across machines, Rosetta vs native runs).
- Permissions: cwasm files mode `0600`, cache directory `0700`. The manager refuses to write into a cache directory it does not own (uid mismatch).

### `yosh-plugin install` — minor changes

- GitHub URL form unchanged.
- Local path form requires a `.wasm` extension (was `.dylib` / `.so`). Wrong extensions are rejected with a clear error.

### `yosh-plugin sync` — adds eager precompile

```
1. Read plugins.toml.
2. Compute the cache key tuple: (wasmtime_version, target_triple, engine_config_hash).
3. For each enabled entry:
   a. Resolve and download / locate the .wasm file.
   b. Compute SHA-256 of the wasm bytes.
   c. Look at the existing lockfile entry (if any). If the four-tuple
      (wasm_sha256, wasmtime_version, target_triple, engine_config_hash)
      matches and both the cwasm file and its sidecar .meta exist with
      correct ownership/permissions, skip precompile.
   d. Otherwise: precompile via Engine::precompile_component(&wasm_bytes),
      write to ~/.cache/yosh/plugins/<sha256>-<engine_hash>-<triple>.cwasm
      with mode 0600, write the sidecar .meta.
   e. Stage a lockfile entry with the full four-field cache key.
4. Atomic replace plugins.lock.
```

Per-plugin precompile failures are reported in the existing `failed` list and do not block other plugins. Exit code matches the existing partial-failure semantics (1 if any failed). On precompile failure the lockfile entry omits the cwasm fields so subsequent shell startups will fall back to in-memory precompile (the same path as cache-stale).

### `yosh-plugin sync` — metadata extraction sub-step

To populate the lockfile's cached `required_capabilities` and `implemented_hooks` (used by `yosh-plugin list`), the manager calls `metadata()` on each plugin once per sync. This is guest code execution and needs an explicit threat model:

- **All-deny linker**: the manager constructs a linker where every `yosh:plugin/*` host import is registered as a deny-stub returning `Err(error-code::denied)`. `wasi:clocks` and `wasi:random` are still linked normally (they have no privilege impact). Since `metadata` is contractually forbidden from calling host APIs (§5 "metadata contract"), this is sufficient — a well-behaved plugin sees the same null-env experience as during shell startup, and a misbehaving plugin's host calls are uniformly denied.
- **Trap during `metadata`**: logged as `yosh-plugin: <name>: metadata trapped: <reason>`, the plugin is marked failed in the sync result, no lockfile entry written for that plugin.
- **Hang during `metadata`**: bounded by a 5-second wall-clock timeout enforced via `wasmtime::Engine::increment_epoch` from a watchdog thread. Exceeding the timeout triggers a trap (epoch interruption), which is then handled as the trap case above. This is the only place v0.2.0 uses epoch interruption; the runtime path remains uninstrumented.
- **Forbidden WASI imports in the component**: linker construction fails before `metadata` is called, the plugin is marked failed, no lockfile entry written.
- **Invalid capability strings in `required-capabilities`**: parsed at sync time. Unknown strings produce a one-line warning per string in the `sync` output but do **not** fail the plugin (matches §6 "unknown capabilities are warnings, not errors"). The strings are stored in the lockfile as-is so `yosh-plugin list` can display them faithfully.
- **`implemented-hooks` containing duplicates or unknown variants**: deduplicated and filtered with a warning. Stored as the cleaned set.

This subsystem is single-purpose: it produces the cached metadata fields. Shell startup re-reads `metadata` from the live instance regardless of the lockfile cache, so a stale or missing cached value cannot cause a sandbox bypass — it only affects the `list` UI.

### `yosh-plugin sync --prune`

- Removes orphaned `.wasm` files (existing behaviour).
- Additionally removes orphaned `.cwasm` files in the cache directory (matched via the sidecar `.cwasm.meta` listing the source `wasm_sha256` — entries whose source is no longer in the lockfile are removed).
- Cleans up empty plugin directories (subsumes `TODO.md` item from `crates/yosh-plugin-manager/src/sync.rs`).

### Lockfile ownership: only `yosh-plugin` mutates `plugins.lock`

The shell binary is a strict reader of `plugins.lock`. When shell startup detects a cache key mismatch (wasmtime upgraded, triple changed, engine config differs, cwasm missing/corrupt), it:

1. Re-verifies the `.wasm` SHA-256 against `plugins.lock`. **Refuses to load on mismatch.**
2. Re-precompiles the verified `.wasm` into an in-memory `Vec<u8>` for that session. The cwasm file on disk is **not** rewritten by the shell.
3. Logs a one-line warning to stderr per affected plugin: `yosh: plugin '<name>': cwasm cache stale (<reason>); run 'yosh-plugin sync' to refresh`.
4. Continues with the in-memory cwasm for the rest of the session.

Rationale: `plugins.lock` semantics align with the existing TOML/lockfile pattern (a manager-owned manifest of installed state). Letting the shell mutate the lockfile would create concurrent-write hazards (multiple shells starting at the same time) and confuse the user mental model where "lockfile changes ⇔ user ran yosh-plugin". The single startup warning is enough nudge for the user to run `yosh-plugin sync` at their convenience.

This behaviour is identical to how `cargo` separates `Cargo.lock` reads (running `cargo build` against a stale lock works) from explicit refreshes (`cargo update` rewrites it).

### `yosh-plugin update`

Unchanged. Updates `plugins.toml` version and re-invokes sync, so precompile happens transparently.

### `yosh-plugin list` — capability and cache state

```
git-status   1.2.0   github:user/...   ✓ verified  ✓ cached    [variables:read, io]
my-local     -       local:/path/...   ✓ verified  ✗ stale     [io, hooks:pre_prompt]
trap-test    0.1.0   local:/path/...   ✓ verified  ✓ cached    [- (no capabilities)]
```

Columns:

- **verified**: SHA-256 of the `.wasm` matches the lockfile.
- **cached**: cache key tuple matches the running manager's runtime; `stale` indicates a mismatch on any of `wasmtime_version` / `target_triple` / `engine_config_hash` (re-precompiled on next shell startup or `yosh-plugin sync`).
- **capabilities** (right-aligned bracket): the plugin's `required-capabilities` from `plugin-info`. This is sourced by reading `plugin-info` once during `sync` and storing it in the lockfile, avoiding the need to instantiate the plugin during `list`. Helps users audit "what does this plugin want to do" before running it.

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
2. **WASM trap isolation via `with_env`.** A `tests/plugins/trap_plugin/` triggers `unreachable!()`. After the trap: (a) the same plugin's subsequent calls return `None` with the "skipped" warning, (b) other plugins continue to work, (c) the shell process is alive.
3. **`with_env` resets `env` on every exit path.** A plugin that returns `Err` on its first call and `Ok` on its second; the test verifies host imports observe a non-null `env` on both calls and that no leak/corruption results across the failure boundary.
4. **`metadata` cannot reach host APIs.** A plugin whose `metadata` calls `cwd()` and includes the result in the returned `name` field; the test verifies the plugin loads (not aborted) but the cwd call returned `Err(Denied)`. (Negative test for the §5 "metadata contract".)
5. **`on-load` CAN reach host APIs.** A plugin whose `on-load` writes a marker via `io.write`; the test verifies the marker reaches the host's stdout capture, confirming `with_env` is engaged for `on-load`.
6. **cwasm cache invalidation — wasmtime version.** Tampering with `wasmtime_version` in `plugins.lock` causes startup to (a) re-verify wasm SHA-256, (b) re-precompile in memory, (c) emit a stale-cache warning, (d) **not** rewrite `plugins.lock`.
7. **cwasm cache invalidation — engine config hash.** Same as above with `engine_config_hash` mutated.
8. **cwasm cache invalidation — target triple.** Simulated by writing a different triple into the sidecar `.meta`; same fallback path triggered.
9. **`.cwasm` tampering rejected via wasm SHA mismatch.** Modify the `.wasm` file post-`sync`; the next shell startup detects SHA-256 mismatch and refuses to load (does NOT silently fall back to a tampered cwasm). Negative test for the §5 trust model.
10. **WASI surface lockdown.** A plugin that imports `wasi:cli/stdout` fails to link at load time. Negative test for the §2 sandboxing principle.
11. **Capability metadata mismatch warning.** A plugin whose `plugin-info.required-capabilities` includes `"unknown:capability"` loads successfully but emits a single warning. Verifies the §6 "unknown capabilities are warnings, not errors" rule and subsumes the existing TODO item.
12. **`required & !granted` warning sourced from `plugin-info`.** A plugin declaring `required-capabilities = ["variables:write"]` with `plugins.toml` allowlist `["variables:read"]` emits the parity warning. Verifies the §6 explicit-declaration path.
13. **Hook dispatch suppression for non-overridden hooks.** A plugin that does not override `hook_pre_exec` is loaded; verify that `pre-exec` is never called even though its WIT export is present (using a pre-exec counter accessible via a separate command).
14. **Compile-only WASI linker construction.** A `#[test] fn linker_construction_smoke()` that builds the linker against the pinned wasmtime, instantiates a no-import sentinel component, and verifies success. Locks down the `WasiView` / `add_to_linker` signature against silent breakage on wasmtime upgrades.
15. **Boundary-crossing benchmark.** `benches/plugin_bench.rs` measures `variables.get` calls in a tight loop and an empty-hook baseline; thresholds established for regression detection.

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
| 2 | `yosh-plugin-sdk` rewrite as `wit-bindgen` wrapper | `crates/yosh-plugin-sdk/src/{lib,export}.rs`; remove `build.rs`. The `Plugin` trait keeps `required_capabilities() -> &[Capability]` and gains `implemented_hooks() -> &[HookName] { &[] }` as a default-empty method. Plugin authors override `implemented_hooks` to declare which hook methods they actually implement (Rust cannot detect default-method overrides reflectively, so the declaration is explicit). The `export!` macro reads both methods at runtime once during `metadata` dispatch and serializes them into `plugin-info`. | #1 |
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
| **wasmtime API churn** — particularly `wasmtime-wasi` linker addition paths and `WasiView` shape shifting between majors. | Pin `wasmtime` to a specific major in `Cargo.toml` (likely `27`). Sub-project #4 includes the compile-only `linker_construction_smoke` test (§8 case 14) which fails fast on any signature change. Upgrades coordinated via `release.sh`. |
| **cargo-component pre-1.0 stability** | Pin in `mise.toml` and CI. Lockstep upgrades only. |
| **WIT semver discipline** | Document explicitly in `docs/kish/plugin.md`: the 0.x window allows breaking changes via minor bump; 1.0 transitions to strict semver and aligns with yosh 1.0. |
| **Binary size impact of bundling wasmtime** | Validate during implementation that the `cranelift` feature is necessary at runtime; `winch` is out of scope for v0.2.0 but a future investigation candidate. |
| **`wasi:cli/stderr` import injection from Rust `std`** | The SDK guides plugin authors toward `panic = "abort"` and minimal-`std` builds. If a plugin's component still imports `wasi:cli/stderr`, link fails at load time, producing a clear error rather than silent stdout/stderr leakage. |
| **`implemented-hooks` is plugin-author-declared, not auto-detected** | Rust cannot reflectively determine whether a default trait method was overridden. The `Plugin` trait therefore exposes `fn implemented_hooks() -> &'static [HookName] { &[] }` and plugin authors override it to enumerate the hooks they implement. SDK rustdoc explicitly documents this contract. If an author lies (returns `&[HookName::PreExec]` while leaving `hook_pre_exec` as the default no-op), the only effect is one wasted boundary crossing per dispatch — not a security issue. The opposite mistake (omitting an actually-overridden hook) results in the host never calling that hook; an SDK rustdoc warning and a unit-test pattern (assert that overridden methods correspond to declared `implemented_hooks`) help authors catch this. |
| **No fuel / memory caps in v0.2.0** | Documented as `TODO.md` item ("Plugin runtime limits"). Implementations deferred until concrete user-reported needs surface. |
| **Cache key schema evolution** | The cache key tuple `(wasm_sha256, wasmtime_version, target_triple, engine_config_hash)` may need additional dimensions (e.g. CPU feature flags) in future. The sidecar `.meta` is versioned (`schema = 1`) so future reads can detect old layouts and trigger regeneration rather than misuse. |

---

## Appendix A — Concrete `LoadedPlugin` shape (host side, sketch)

```rust
struct LoadedPlugin {
    name: String,
    store: Store<HostContext>,
    bindings: PluginWorld,                 // bindgen-generated bindings handle
    plugin_info: PluginInfo,               // commands, required-capabilities, implemented-hooks
    capabilities: u32,                     // granted (after allowlist application)
    invalidated: bool,                     // set by with_env on guest trap
}
```

`PluginWorld` is the bindings type emitted by `wasmtime::component::bindgen!` from the WIT `world plugin-world`. It exposes `yosh_plugin_plugin()` and `yosh_plugin_hooks()` accessors for the two export interfaces. `PluginInfo` is the host-side decoded form of `plugin-info` (pure Rust struct, no wasm references).
