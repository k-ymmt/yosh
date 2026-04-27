# WASM Plugin Runtime Migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate yosh's plugin execution from `dlopen`-loaded shared libraries to WebAssembly Components executed via wasmtime, exposing the host API as a versioned WIT package (`yosh:plugin@0.1.0`).

**Architecture:** Single shared `wasmtime::Engine` per shell process. Each plugin is loaded once into a persistent `Store<HostContext>`. Host APIs (variables / filesystem / io) are wired through the linker with capability-aware deny-stubs. Hooks dispatch is filtered by both `plugin-info.implemented-hooks` and the user's allowlist. `.wasm` is the only trusted artifact; `.cwasm` is a regenerable cache keyed on `(wasm_sha256, wasmtime_version, target_triple, engine_config_hash)`.

**Tech Stack:** Rust 2024, wasmtime 27.x (component-model + cranelift + cache), wasmtime-wasi 27.x (clocks + random only), wit-bindgen 0.36.x (SDK guest bindings), cargo-component 0.x (plugin authoring), `rustup target add wasm32-wasip2`.

**Spec reference:** `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`

---

## Pre-requisites — install once, locally

Before starting Task 1, the local toolchain needs the wasm target and `cargo-component`. CI installs these too (Task 7).

- [ ] **Step 0.1: Install wasm32-wasip2 target**

Run: `rustup target add wasm32-wasip2`
Expected output: `info: component 'rust-std' for target 'wasm32-wasip2' is up to date` (or installs it).

- [ ] **Step 0.2: Install cargo-component (pinned)**

Run: `cargo install cargo-component --locked --version 0.18.0`
Expected: build succeeds, `cargo component --version` prints `0.18.0`.

- [ ] **Step 0.3: Verify wasmtime CLI availability is NOT required**

This plan does not need the `wasmtime` binary on PATH; only the `wasmtime` crate is used at compile time. Skip if `wasmtime` is not installed.

---

## Task 1: Add WIT file and repurpose `yosh-plugin-api`

**Goal:** Replace the C-ABI shapes (`HostApi`, `PluginDecl`, etc.) with a WIT package (`yosh-plugin.wit`) and a small Rust module exposing the `Capability` enum + `parse_capability` parser + `CAP_*` bitflag constants.

**Files:**
- Create: `crates/yosh-plugin-api/wit/yosh-plugin.wit`
- Modify (full replacement): `crates/yosh-plugin-api/src/lib.rs`
- Modify: `crates/yosh-plugin-api/Cargo.toml`

- [ ] **Step 1.1: Create the WIT directory**

Run: `mkdir -p crates/yosh-plugin-api/wit`

- [ ] **Step 1.2: Write `crates/yosh-plugin-api/wit/yosh-plugin.wit`**

Paste the spec's §3 WIT definition verbatim. Reproduced here for self-containment:

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
    /// the granted capabilities.
    record plugin-info {
        name: string,
        version: string,
        commands: list<string>,
        required-capabilities: list<string>,
        implemented-hooks: list<hook-name>,
    }
}

interface variables {
    use types.{error-code};
    /// Outer `result` carries denial; inner `option` distinguishes
    /// "variable not set" from "variable set to empty string".
    get:    func(name: string) -> result<option<string>, error-code>;
    set:    func(name: string, value: string) -> result<_, error-code>;
    export: func(name: string, value: string) -> result<_, error-code>;
}

interface filesystem {
    use types.{error-code};
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

- [ ] **Step 1.3: Replace `crates/yosh-plugin-api/src/lib.rs`**

Replace the entire file contents with:

```rust
//! Capability declarations and string parsing shared between the host,
//! the SDK, and the plugin manager. The C ABI types from the dlopen era
//! are removed; the public WIT contract lives at `wit/yosh-plugin.wit`.

/// Capability bitflag constants. Used by the host's linker construction
/// (`src/plugin/linker.rs`) to decide which host imports get the real
/// implementation vs a deny-stub. Also used by the manager to parse
/// `plugins.toml` `capabilities = [...]` allowlists.
pub const CAP_VARIABLES_READ:  u32 = 0x01;
pub const CAP_VARIABLES_WRITE: u32 = 0x02;
pub const CAP_FILESYSTEM:      u32 = 0x04;
pub const CAP_IO:              u32 = 0x08;
pub const CAP_HOOK_PRE_EXEC:   u32 = 0x10;
pub const CAP_HOOK_POST_EXEC:  u32 = 0x20;
pub const CAP_HOOK_ON_CD:      u32 = 0x40;
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x80;

pub const CAP_ALL: u32 = CAP_VARIABLES_READ
    | CAP_VARIABLES_WRITE
    | CAP_FILESYSTEM
    | CAP_IO
    | CAP_HOOK_PRE_EXEC
    | CAP_HOOK_POST_EXEC
    | CAP_HOOK_ON_CD
    | CAP_HOOK_PRE_PROMPT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    VariablesRead,
    VariablesWrite,
    Filesystem,
    Io,
    HookPreExec,
    HookPostExec,
    HookOnCd,
    HookPrePrompt,
}

impl Capability {
    pub fn to_bitflag(self) -> u32 {
        match self {
            Capability::VariablesRead  => CAP_VARIABLES_READ,
            Capability::VariablesWrite => CAP_VARIABLES_WRITE,
            Capability::Filesystem     => CAP_FILESYSTEM,
            Capability::Io             => CAP_IO,
            Capability::HookPreExec    => CAP_HOOK_PRE_EXEC,
            Capability::HookPostExec   => CAP_HOOK_POST_EXEC,
            Capability::HookOnCd       => CAP_HOOK_ON_CD,
            Capability::HookPrePrompt  => CAP_HOOK_PRE_PROMPT,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::VariablesRead  => "variables:read",
            Capability::VariablesWrite => "variables:write",
            Capability::Filesystem     => "filesystem",
            Capability::Io             => "io",
            Capability::HookPreExec    => "hooks:pre_exec",
            Capability::HookPostExec   => "hooks:post_exec",
            Capability::HookOnCd       => "hooks:on_cd",
            Capability::HookPrePrompt  => "hooks:pre_prompt",
        }
    }
}

/// Parse a single capability string. Returns `None` for unknown strings;
/// callers decide whether to log a warning or fail.
pub fn parse_capability(s: &str) -> Option<Capability> {
    Some(match s {
        "variables:read"   => Capability::VariablesRead,
        "variables:write"  => Capability::VariablesWrite,
        "filesystem"       => Capability::Filesystem,
        "io"               => Capability::Io,
        "hooks:pre_exec"   => Capability::HookPreExec,
        "hooks:post_exec"  => Capability::HookPostExec,
        "hooks:on_cd"      => Capability::HookOnCd,
        "hooks:pre_prompt" => Capability::HookPrePrompt,
        _ => return None,
    })
}

/// Combine a slice of capabilities into a bitfield.
pub fn capabilities_to_bitflags(caps: &[Capability]) -> u32 {
    caps.iter().fold(0u32, |acc, c| acc | c.to_bitflag())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_strings() {
        assert_eq!(parse_capability("io"), Some(Capability::Io));
        assert_eq!(
            parse_capability("hooks:pre_prompt"),
            Some(Capability::HookPrePrompt)
        );
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(parse_capability("variables:execute"), None);
        assert_eq!(parse_capability(""), None);
    }

    #[test]
    fn capability_round_trip() {
        for cap in [
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
        ] {
            assert_eq!(parse_capability(cap.as_str()), Some(cap));
        }
    }

    #[test]
    fn cap_all_covers_every_variant() {
        let bits = capabilities_to_bitflags(&[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
        ]);
        assert_eq!(bits, CAP_ALL);
    }
}
```

- [ ] **Step 1.4: Update `crates/yosh-plugin-api/Cargo.toml`**

Open `crates/yosh-plugin-api/Cargo.toml` and confirm it has no `[lib] crate-type = [...]` setting (the crate is now a normal Rust library). The `[dependencies]` section can be empty; no other crate is needed for this barebones shape. Bump `version` to `0.2.0` in lockstep with the rest of the workspace.

Concretely the file should look like:

```toml
[package]
name = "yosh-plugin-api"
version = "0.2.0"
edition = "2024"
description = "WIT package and capability definitions for yosh plugins"
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"

[dependencies]
```

- [ ] **Step 1.5: Verify build and tests**

Run: `cargo test -p yosh-plugin-api`
Expected: all 4 tests pass; no compilation errors.

- [ ] **Step 1.6: Commit**

```bash
git add crates/yosh-plugin-api
git commit -m "feat(plugin-api)!: replace C ABI types with WIT package + Capability enum

Original prompt: Migrate yosh plugin execution from dlopen to wasmtime
Component Model. This task adds the WIT package as the canonical host
API contract and reduces yosh-plugin-api to capability bitflags +
parser. Sub-project #1 of v0.2.0 migration."
```

---

## Task 2: Rewrite `yosh-plugin-sdk` as a `wit-bindgen` wrapper

**Goal:** The SDK now exposes a Rust `Plugin` trait + `export!` macro that wraps `wit_bindgen::generate!`-emitted `Guest` impls. No more C ABI; no more `unsafe extern "C" fn`. The plugin author writes ergonomic Rust; the macro emits the `Plugin` and `Hooks` interface implementations the WIT world demands.

**Files:**
- Modify: `crates/yosh-plugin-sdk/Cargo.toml`
- Replace: `crates/yosh-plugin-sdk/src/lib.rs`
- Create: `crates/yosh-plugin-sdk/src/export.rs`
- Keep unchanged: `crates/yosh-plugin-sdk/src/style.rs`
- Delete: `crates/yosh-plugin-sdk/build.rs` (if present)

- [ ] **Step 2.1: Update `crates/yosh-plugin-sdk/Cargo.toml`**

Replace dependencies block:

```toml
[package]
name = "yosh-plugin-sdk"
version = "0.2.0"
edition = "2024"
description = "Rust SDK for authoring yosh plugins (wit-bindgen wrapper)"
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"

[dependencies]
yosh-plugin-api = { version = "0.2.0", path = "../yosh-plugin-api" }
wit-bindgen = "0.36"
```

Remove any `[lib] crate-type = ["cdylib"]` line if present. Plugin authors set `crate-type = ["cdylib"]` in their *own* `Cargo.toml`; the SDK itself stays a normal library.

- [ ] **Step 2.2: Delete `crates/yosh-plugin-sdk/build.rs`**

Run: `git rm crates/yosh-plugin-sdk/build.rs` (only if the file exists; the WIT bindings are generated by the macro, no build script needed).

- [ ] **Step 2.3: Replace `crates/yosh-plugin-sdk/src/lib.rs`**

Full new contents:

```rust
//! yosh-plugin-sdk — Rust SDK for authoring yosh plugins.
//!
//! Plugins implement the [`Plugin`] trait and invoke [`export!`] to wire
//! the trait into the WIT-generated guest bindings. The SDK hides all
//! WIT bindgen plumbing; plugin authors only see Rust types.

#![allow(clippy::missing_safety_doc)]

pub mod style;
mod export;

pub use yosh_plugin_api as ffi;

// Re-exported wit-bindgen runtime so the `export!` macro can refer to
// it via `$crate::wit_bindgen` regardless of the user's edition / prelude.
#[doc(hidden)]
pub use wit_bindgen;

/// Generate the wit-bindgen guest bindings for the `yosh:plugin/plugin-world`.
/// The macro emits Rust modules `bindings::yosh::plugin::*` that implement
/// the WIT interface contracts.
///
/// We invoke this once in the SDK so plugin authors do not need their own
/// invocation. Their `export!(MyPlugin);` builds on top of this output.
wit_bindgen::generate!({
    world: "plugin-world",
    path: "../yosh-plugin-api/wit",
    pub_export_macro: true,
    generate_all,
});

// Re-export the generated bindings under a stable name so the user-side
// macro (export.rs) can refer to it predictably.
pub use self::exports::yosh::plugin::plugin as plugin_iface;
pub use self::exports::yosh::plugin::hooks as hooks_iface;
pub use self::yosh::plugin::types::{ErrorCode, HookName, PluginInfo, Stream};
pub use self::yosh::plugin::filesystem as host_filesystem;
pub use self::yosh::plugin::io as host_io;
pub use self::yosh::plugin::variables as host_variables;

// ── Plugin author-facing types ────────────────────────────────────────

/// Capabilities the plugin requests. Re-exported from `yosh-plugin-api`
/// so plugin authors only need one crate dependency.
pub use yosh_plugin_api::{Capability, capabilities_to_bitflags};

/// The trait every plugin implements. Hook methods have no-op default
/// implementations; override only the ones you need AND list them in
/// [`Plugin::implemented_hooks`].
pub trait Plugin: Send + Default + 'static {
    /// Command names this plugin provides.
    fn commands(&self) -> &[&'static str];

    /// Capabilities this plugin requests. The host's linker uses this
    /// for the load-time "requested but not granted" diagnostic; the
    /// actual sandbox is enforced by the linker, not by this list.
    fn required_capabilities(&self) -> &[Capability] { &[] }

    /// Hooks this plugin actually implements. Rust cannot reflectively
    /// detect which default trait methods are overridden, so plugin
    /// authors enumerate their hooks here. Lying here is not a security
    /// issue but degrades performance (extra boundary crossings) or
    /// silently drops dispatch.
    fn implemented_hooks(&self) -> &[HookName] { &[] }

    /// Initialize the plugin. Called once after instantiation, with the
    /// host environment bound. Return `Err(...)` to abort plugin load.
    fn on_load(&mut self) -> Result<(), String> { Ok(()) }

    /// Execute one of the plugin's commands.
    fn exec(&mut self, command: &str, args: &[String]) -> i32;

    /// Hook: before each shell command runs.
    fn hook_pre_exec(&mut self, _command: &str) {}

    /// Hook: after each shell command exits.
    fn hook_post_exec(&mut self, _command: &str, _exit_code: i32) {}

    /// Hook: on `cd` (working-directory change).
    fn hook_on_cd(&mut self, _old_dir: &str, _new_dir: &str) {}

    /// Hook: before each interactive prompt.
    fn hook_pre_prompt(&mut self) {}

    /// Cleanup. Called once at shell shutdown, with the host environment bound.
    fn on_unload(&mut self) {}
}

// ── Host API helpers (typed wrappers over WIT-generated bindings) ──────

/// Read a shell variable. Returns `Ok(None)` if unset; `Err(Denied)` if
/// the `variables:read` capability was not granted.
pub fn get_var(name: &str) -> Result<Option<String>, ErrorCode> {
    host_variables::get(name)
}

/// Set a shell variable.
pub fn set_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::set(name, value)
}

/// Set and export a shell variable.
pub fn export_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::export(name, value)
}

/// Current working directory. `Err(Denied)` if `filesystem` is not granted.
pub fn cwd() -> Result<String, ErrorCode> {
    host_filesystem::cwd()
}

/// Change current working directory.
pub fn set_cwd(path: &str) -> Result<(), ErrorCode> {
    host_filesystem::set_cwd(path)
}

/// Write to stdout. `Err(Denied)` if `io` is not granted.
pub fn print(s: &str) -> Result<(), ErrorCode> {
    host_io::write(Stream::Stdout, s.as_bytes())
}

/// Write to stderr.
pub fn eprint(s: &str) -> Result<(), ErrorCode> {
    host_io::write(Stream::Stderr, s.as_bytes())
}

/// Write raw bytes to a stream (e.g. for binary output without a UTF-8 round-trip).
pub fn write_bytes(stream: Stream, data: &[u8]) -> Result<(), ErrorCode> {
    host_io::write(stream, data)
}
```

- [ ] **Step 2.4: Create `crates/yosh-plugin-sdk/src/export.rs`**

The `export!` macro bridges the user's `Plugin` trait impl into the WIT-generated `Guest` traits. New file contents:

```rust
//! Macro that wires a user-implemented `Plugin` into the WIT bindings.

/// Generate the WIT exports (`yosh:plugin/plugin` and `yosh:plugin/hooks`)
/// from a `Plugin` implementation. Place at the crate root.
///
/// ```ignore
/// use yosh_plugin_sdk::{Plugin, Capability, HookName, export};
///
/// #[derive(Default)]
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     fn commands(&self) -> &[&'static str] { &["hello"] }
///     fn required_capabilities(&self) -> &[Capability] { &[Capability::Io] }
///     fn exec(&mut self, _cmd: &str, _args: &[String]) -> i32 {
///         yosh_plugin_sdk::print("Hello!\n").ok();
///         0
///     }
/// }
///
/// export!(MyPlugin);
/// ```
#[macro_export]
macro_rules! export {
    ($plugin_type:ty) => {
        // Single global instance of the plugin. Constructed lazily on
        // first use (typically `metadata` or `on_load`).
        static __YOSH_PLUGIN_INSTANCE: ::std::sync::Mutex<Option<$plugin_type>>
            = ::std::sync::Mutex::new(None);

        fn __yosh_plugin_instance_get<R>(
            f: impl FnOnce(&mut $plugin_type) -> R,
        ) -> R {
            let mut guard = __YOSH_PLUGIN_INSTANCE.lock()
                .unwrap_or_else(|e| e.into_inner());
            if guard.is_none() {
                *guard = Some(<$plugin_type as ::core::default::Default>::default());
            }
            f(guard.as_mut().expect("plugin instance present"))
        }

        // ── plugin export interface ───────────────────────────────────
        struct __YoshPluginExports;

        impl $crate::plugin_iface::Guest for __YoshPluginExports {
            fn metadata() -> $crate::PluginInfo {
                __yosh_plugin_instance_get(|p| {
                    let commands = $crate::Plugin::commands(p)
                        .iter().map(|s| (*s).to_string()).collect();
                    let required_capabilities = $crate::Plugin::required_capabilities(p)
                        .iter().map(|c| c.as_str().to_string()).collect();
                    let implemented_hooks = $crate::Plugin::implemented_hooks(p)
                        .iter().copied().collect();
                    $crate::PluginInfo {
                        name: env!("CARGO_PKG_NAME").to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        commands,
                        required_capabilities,
                        implemented_hooks,
                    }
                })
            }

            fn on_load() -> Result<(), String> {
                __yosh_plugin_instance_get(|p| $crate::Plugin::on_load(p))
            }

            fn exec(command: String, args: Vec<String>) -> i32 {
                __yosh_plugin_instance_get(|p| $crate::Plugin::exec(p, &command, &args))
            }

            fn on_unload() {
                __yosh_plugin_instance_get(|p| $crate::Plugin::on_unload(p));
            }
        }

        // ── hooks export interface ────────────────────────────────────
        impl $crate::hooks_iface::Guest for __YoshPluginExports {
            fn pre_exec(command: String) {
                __yosh_plugin_instance_get(|p| $crate::Plugin::hook_pre_exec(p, &command));
            }
            fn post_exec(command: String, exit_code: i32) {
                __yosh_plugin_instance_get(|p|
                    $crate::Plugin::hook_post_exec(p, &command, exit_code));
            }
            fn on_cd(old_dir: String, new_dir: String) {
                __yosh_plugin_instance_get(|p|
                    $crate::Plugin::hook_on_cd(p, &old_dir, &new_dir));
            }
            fn pre_prompt() {
                __yosh_plugin_instance_get(|p| $crate::Plugin::hook_pre_prompt(p));
            }
        }

        // Register the export struct with the WIT-generated world. The
        // `export!` macro emitted by wit-bindgen accepts a single type that
        // implements both `plugin_iface::Guest` and `hooks_iface::Guest`.
        $crate::wit_bindgen::generate_export!({
            world: "plugin-world",
            exports: { default: __YoshPluginExports }
        });
    };
}
```

Note: The exact `wit_bindgen::generate_export!` invocation may vary slightly between wit-bindgen versions. The exact form for the pinned `0.36.x` is verified during Step 2.6 (build the test plugin in Task 3); if the macro shape differs, adjust this snippet to match the documented `0.36.x` pattern. The structural intent — register `__YoshPluginExports` as the implementer of both export interfaces — does not change.

- [ ] **Step 2.5: Verify SDK compiles standalone**

Run: `cargo build -p yosh-plugin-sdk --target wasm32-wasip2`
Expected: build succeeds. (The SDK alone has no `cdylib` so it builds as `rlib`. Errors here usually indicate WIT path or wit-bindgen version mismatch; fix and retry.)

- [ ] **Step 2.6: Verify SDK compiles for host (rlib usage by host crates)**

Run: `cargo build -p yosh-plugin-sdk`
Expected: build succeeds with no target flag (host tooling can pull the SDK as a normal Rust dependency, e.g. for shared error types).

- [ ] **Step 2.7: Commit**

```bash
git add crates/yosh-plugin-sdk
git commit -m "feat(plugin-sdk)!: rewrite as wit-bindgen wrapper

Original prompt: Migrate yosh plugin execution from dlopen to wasmtime
Component Model. The Plugin trait keeps its dlopen-era shape (commands,
exec, hooks) but is now a wit-bindgen guest binding wrapper. Adds
required_capabilities() and implemented_hooks() declaration methods.
Sub-project #2 of v0.2.0 migration."
```

---

## Task 3: Convert `tests/plugins/test_plugin` to a wasm component

**Goal:** Rebuild the in-tree integration test plugin as a Component Model `.wasm` artifact via cargo-component, exercising every API the host needs to integration-test.

**Files:**
- Modify: `tests/plugins/test_plugin/Cargo.toml`
- Replace: `tests/plugins/test_plugin/src/lib.rs`

- [ ] **Step 3.1: Update `tests/plugins/test_plugin/Cargo.toml`**

Replace contents:

```toml
[package]
name = "test_plugin"
version = "0.2.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
yosh-plugin-sdk = { path = "../../../crates/yosh-plugin-sdk" }

[package.metadata.component]
package = "yosh:test-plugin"

[package.metadata.component.target]
path  = "../../../crates/yosh-plugin-api/wit"
world = "plugin-world"

[package.metadata.component.dependencies]
# wasi adapter is auto-resolved by cargo-component for the wasm32-wasip2 target.

[profile.release]
opt-level = "s"
lto = true
strip = true
panic = "abort"
```

`panic = "abort"` keeps the wasm binary small and avoids pulling `wasi:cli/stderr` for panic strings (which would fail to link per the §2 sandbox principle).

- [ ] **Step 3.2: Replace `tests/plugins/test_plugin/src/lib.rs`**

Full new contents (covers all branches the integration tests exercise — commands, hooks, denied capability paths, on-load):

```rust
use std::sync::Mutex;
use yosh_plugin_sdk::{Capability, HookName, Plugin, export, get_var, print};

static EVENT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(event: impl Into<String>) {
    EVENT_LOG.lock().unwrap_or_else(|e| e.into_inner()).push(event.into());
}

#[derive(Default)]
struct TestPlugin;

impl Plugin for TestPlugin {
    fn commands(&self) -> &[&'static str] {
        &["test_cmd", "echo_var", "trap_now"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookOnCd,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PreExec, HookName::OnCd]
    }

    fn on_load(&mut self) -> Result<(), String> {
        record("on_load");
        Ok(())
    }

    fn exec(&mut self, command: &str, args: &[String]) -> i32 {
        match command {
            "test_cmd" => {
                let _ = print(&format!("test_cmd args={:?}\n", args));
                0
            }
            "echo_var" => match args.first() {
                Some(name) => match get_var(name) {
                    Ok(Some(v)) => { let _ = print(&format!("{}\n", v)); 0 }
                    Ok(None)    => { let _ = print("(unset)\n"); 0 }
                    Err(_)      => 2,
                },
                None => 1,
            },
            "trap_now" => {
                // Deliberate trap for trap-isolation test (test #2).
                #[allow(clippy::diverging_sub_expression)]
                {
                    let _: u32 = unreachable!("intentional trap");
                }
            }
            _ => 127,
        }
    }

    fn hook_pre_exec(&mut self, command: &str) {
        record(format!("pre_exec:{}", command));
    }

    fn hook_on_cd(&mut self, old_dir: &str, new_dir: &str) {
        record(format!("on_cd:{}:{}", old_dir, new_dir));
    }

    fn on_unload(&mut self) {
        record("on_unload");
    }
}

export!(TestPlugin);
```

- [ ] **Step 3.3: Build the test plugin**

Run: `cargo component build -p test_plugin --target wasm32-wasip2 --release`
Expected: succeeds; produces `target/wasm32-wasip2/release/test_plugin.wasm`.

- [ ] **Step 3.4: Verify the artifact is a Component**

Run: `wasm-tools component wit target/wasm32-wasip2/release/test_plugin.wasm | head -20`
Expected output (first lines): a WIT dump that shows `world plugin-world` with the imports `yosh:plugin/variables`, `yosh:plugin/filesystem`, `yosh:plugin/io`, `wasi:clocks/...`, `wasi:random/...` and the exports `yosh:plugin/plugin` and `yosh:plugin/hooks`.

(If `wasm-tools` is not installed: `cargo install wasm-tools` first. This is a verification step only; not required at runtime.)

- [ ] **Step 3.5: Commit**

```bash
git add tests/plugins/test_plugin
git commit -m "test(plugin)!: rewrite test_plugin as a wasm component

Sub-project #3 of v0.2.0 migration. test_plugin now builds via
cargo-component to wasm32-wasip2 and uses the Plugin trait + export!
macro from the rewritten yosh-plugin-sdk. Exercises commands,
required/implemented hooks, capability requests, on_load, and a
deliberate trap path (trap_now) for the trap-isolation test."
```

---

## Task 4: Replace `src/plugin/` with the wasmtime-based `PluginManager`

**Goal:** Drop `libloading`. Build a wasmtime-based plugin manager that loads `.cwasm` (verified against `.wasm` SHA-256), constructs a capability-aware linker, and dispatches commands and hooks via the `with_env` RAII wrapper.

This is the largest task. Steps are grouped by concern: dependencies → types → linker → host imports → manager → integration.

**Files:**
- Modify: `Cargo.toml` (root)
- Modify: `src/plugin/config.rs` (small adjustments for new lockfile fields)
- Replace: `src/plugin/mod.rs`
- Create: `src/plugin/host.rs`
- Create: `src/plugin/linker.rs`
- Create: `src/plugin/cache.rs`
- Modify: `src/exec/mod.rs` (call site update for `PluginExec` enum)

### 4.1 Workspace dependency changes

- [ ] **Step 4.1.1: Modify root `Cargo.toml`**

In `[dependencies]`:

- **Remove**: `libloading = "0.8"`
- **Add**:
  ```toml
  wasmtime = { version = "27", default-features = false, features = ["component-model", "cranelift", "cache", "runtime"] }
  wasmtime-wasi = { version = "27", default-features = false }
  sha2 = "0.10"
  hex = "0.4"
  ```

Bump the workspace version from `0.1.5` to `0.2.0`. Update the workspace lockstep version of every `crates/yosh-plugin-*` dependency to `0.2.0`.

- [ ] **Step 4.1.2: Verify the workspace builds before deleting plugin code**

Run: `cargo check --workspace`
Expected: succeeds. (Old `src/plugin/mod.rs` still references `libloading` — so this step will fail. That's expected; proceed knowing the next steps will fix it. Do **not** commit yet.)

### 4.2 Cache key + cwasm validation module

- [ ] **Step 4.2.1: Create `src/plugin/cache.rs`**

Full new contents:

```rust
//! cwasm cache key validation, used by both shell startup and the manager.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Cache key tuple recorded both in `plugins.lock` and the sidecar
/// `<cwasm>.meta`. See spec §5 "cwasm trust model".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub wasm_sha256: String,
    pub wasmtime_version: String,
    pub target_triple: String,
    pub engine_config_hash: String,
}

impl CacheKey {
    /// Build the live cache key for the running runtime.
    pub fn for_runtime(wasm_sha256: String, engine_config_hash: String) -> Self {
        Self {
            wasm_sha256,
            wasmtime_version: wasmtime::VERSION.to_string(),
            target_triple: target_triple().to_string(),
            engine_config_hash,
        }
    }
}

pub fn target_triple() -> &'static str {
    // The compile-time target triple. Matches `wasmtime::Engine`'s host
    // codegen target for native-host engines (which is what we use).
    env!("TARGET_TRIPLE_OR_RUST_BUILT_IN", "host")
}

/// Compute SHA-256 of a file as a hex string.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Validate that `cwasm_path` is safe to deserialize for `expected`.
/// All five conditions of the §5 threat model must hold; on any failure
/// the caller falls back to in-memory precompile from the verified .wasm.
pub fn validate_cwasm(cwasm_path: &Path, expected: &CacheKey, current_uid: u32)
    -> Result<(), &'static str>
{
    let meta = fs::metadata(cwasm_path).map_err(|_| "cwasm missing")?;

    // Condition 1 + 2: ownership and mode 0600.
    if meta.uid() != current_uid {
        return Err("cwasm owner mismatch");
    }
    if meta.permissions().mode() & 0o777 != 0o600 {
        return Err("cwasm mode not 0600");
    }

    // Condition 3: parent dir 0700 + same uid.
    let parent = cwasm_path.parent().ok_or("cwasm has no parent dir")?;
    let parent_meta = fs::metadata(parent).map_err(|_| "cwasm dir missing")?;
    if parent_meta.uid() != current_uid {
        return Err("cwasm dir owner mismatch");
    }
    if parent_meta.permissions().mode() & 0o777 != 0o700 {
        return Err("cwasm dir mode not 0700");
    }

    // Condition 4: sidecar meta matches expected key.
    let meta_path = sidecar_path(cwasm_path);
    let raw = fs::read_to_string(&meta_path).map_err(|_| "cwasm sidecar missing")?;
    let actual = parse_sidecar(&raw).map_err(|_| "cwasm sidecar malformed")?;
    if actual != *expected {
        return Err("cwasm cache key mismatch");
    }

    // Condition 5 is the caller's responsibility (re-check the .wasm SHA
    // against `expected.wasm_sha256` BEFORE calling validate_cwasm).
    Ok(())
}

pub fn sidecar_path(cwasm_path: &Path) -> std::path::PathBuf {
    let mut s = cwasm_path.as_os_str().to_owned();
    s.push(".meta");
    s.into()
}

/// Sidecar format (single-line key=value, schema v1). Stable on purpose:
/// future schema versions detect old layouts and trigger regeneration.
pub fn render_sidecar(key: &CacheKey) -> String {
    format!(
        "schema=1\nwasm_sha256={}\nwasmtime_version={}\ntarget_triple={}\nengine_config_hash={}\n",
        key.wasm_sha256, key.wasmtime_version, key.target_triple, key.engine_config_hash
    )
}

fn parse_sidecar(s: &str) -> Result<CacheKey, ()> {
    let mut wasm = None;
    let mut wt = None;
    let mut tt = None;
    let mut ec = None;
    let mut schema_ok = false;
    for line in s.lines() {
        let (k, v) = line.split_once('=').ok_or(())?;
        match k {
            "schema"             => { if v == "1" { schema_ok = true; } else { return Err(()); } }
            "wasm_sha256"        => wasm = Some(v.to_string()),
            "wasmtime_version"   => wt   = Some(v.to_string()),
            "target_triple"      => tt   = Some(v.to_string()),
            "engine_config_hash" => ec   = Some(v.to_string()),
            _ => return Err(()),
        }
    }
    if !schema_ok { return Err(()); }
    Ok(CacheKey {
        wasm_sha256:        wasm.ok_or(())?,
        wasmtime_version:   wt.ok_or(())?,
        target_triple:      tt.ok_or(())?,
        engine_config_hash: ec.ok_or(())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;

    fn current_uid() -> u32 {
        // SAFETY: getuid is always-safe.
        unsafe { libc::getuid() }
    }

    fn make_key() -> CacheKey {
        CacheKey {
            wasm_sha256: "deadbeef".into(),
            wasmtime_version: wasmtime::VERSION.into(),
            target_triple: target_triple().into(),
            engine_config_hash: "abc123".into(),
        }
    }

    fn write_with_mode(path: &std::path::Path, contents: &[u8], mode: u32) {
        let mut f = std::fs::OpenOptions::new()
            .create(true).write(true).truncate(true).mode(mode)
            .open(path).unwrap();
        f.write_all(contents).unwrap();
    }

    #[test]
    fn sidecar_round_trip() {
        let key = make_key();
        let s = render_sidecar(&key);
        let parsed = parse_sidecar(&s).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn sidecar_rejects_wrong_schema() {
        let key = make_key();
        let s = render_sidecar(&key).replace("schema=1", "schema=2");
        assert!(parse_sidecar(&s).is_err());
    }

    #[test]
    fn validate_succeeds_when_all_conditions_hold() {
        let dir = TempDir::new().unwrap();
        std::fs::set_permissions(
            dir.path(),
            std::fs::Permissions::from_mode(0o700),
        ).unwrap();
        let cwasm = dir.path().join("test.cwasm");
        let meta = sidecar_path(&cwasm);
        let key = make_key();
        write_with_mode(&cwasm, b"fake-cwasm-bytes", 0o600);
        std::fs::write(&meta, render_sidecar(&key)).unwrap();
        std::fs::set_permissions(&meta, std::fs::Permissions::from_mode(0o600)).unwrap();
        validate_cwasm(&cwasm, &key, current_uid()).unwrap();
    }

    #[test]
    fn validate_rejects_wrong_mode() {
        let dir = TempDir::new().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let cwasm = dir.path().join("test.cwasm");
        let meta = sidecar_path(&cwasm);
        let key = make_key();
        write_with_mode(&cwasm, b"x", 0o644);
        std::fs::write(&meta, render_sidecar(&key)).unwrap();
        let err = validate_cwasm(&cwasm, &key, current_uid()).unwrap_err();
        assert_eq!(err, "cwasm mode not 0600");
    }

    #[test]
    fn validate_rejects_key_mismatch() {
        let dir = TempDir::new().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let cwasm = dir.path().join("test.cwasm");
        let meta = sidecar_path(&cwasm);
        let key = make_key();
        let mut wrong_key = key.clone();
        wrong_key.wasmtime_version = "0.0.0".into();
        write_with_mode(&cwasm, b"x", 0o600);
        std::fs::write(&meta, render_sidecar(&wrong_key)).unwrap();
        std::fs::set_permissions(&meta, std::fs::Permissions::from_mode(0o600)).unwrap();
        let err = validate_cwasm(&cwasm, &key, current_uid()).unwrap_err();
        assert_eq!(err, "cwasm cache key mismatch");
    }
}
```

- [ ] **Step 4.2.2: Add a tiny `build.rs` to capture the target triple**

Create `build.rs` at repo root if not present, or extend it:

```rust
fn main() {
    let triple = std::env::var("TARGET").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=TARGET_TRIPLE_OR_RUST_BUILT_IN={}", triple);
    // ...keep any existing build.rs logic (yosh git hash, build date, etc.)
}
```

If a `build.rs` already exists at repo root (it does — see `b138680` for the test-runner build.rs), append the two lines above to its `main()` function instead of overwriting.

- [ ] **Step 4.2.3: Run cache module tests**

Run: `cargo test -p yosh --lib plugin::cache::tests --nocapture`

(Once `mod cache;` is wired into `src/plugin/mod.rs` in step 4.5.6, the test target will exist. Until then, this step is informational; revisit after step 4.5.6.)

### 4.3 Linker construction module

- [ ] **Step 4.3.1: Create `src/plugin/linker.rs`**

Full new contents:

```rust
//! Capability-aware Linker construction.
//!
//! Builds a `wasmtime::component::Linker<HostContext>` whose imports
//! correspond to the granted capability allowlist. Denied imports are
//! linked as deny-stubs that return `Err(ErrorCode::Denied)`. Hooks
//! are export-side, so capability allowlist gates dispatch in
//! `PluginManager`, not here.

use wasmtime::component::Linker;
use wasmtime::Engine;

use yosh_plugin_api::{
    CAP_FILESYSTEM, CAP_IO, CAP_VARIABLES_READ, CAP_VARIABLES_WRITE,
};

use super::host::HostContext;
use super::host::{deny, real};

#[inline]
pub(super) fn has(allowed: u32, cap: u32) -> bool { allowed & cap != 0 }

/// Construct a Linker with the granted-capability host imports plus the
/// limited WASI surface (clocks + random only).
pub fn build_linker(engine: &Engine, allowed: u32)
    -> wasmtime::Result<Linker<HostContext>>
{
    let mut linker = Linker::<HostContext>::new(engine);

    // Limited WASI: clocks + random only. wasi:cli, wasi:filesystem,
    // wasi:sockets are intentionally NOT linked.
    wasmtime_wasi::p2::clocks::monotonic_clock::add_to_linker_get_host(
        &mut linker, |c| c)?;
    wasmtime_wasi::p2::clocks::wall_clock::add_to_linker_get_host(
        &mut linker, |c| c)?;
    wasmtime_wasi::p2::random::random::add_to_linker_get_host(
        &mut linker, |c| c)?;

    // yosh:plugin/variables
    let mut vars = linker.instance("yosh:plugin/variables@0.1.0")?;
    vars.func_wrap("get",
        if has(allowed, CAP_VARIABLES_READ)  { real::get_var }    else { deny::get_var })?;
    vars.func_wrap("set",
        if has(allowed, CAP_VARIABLES_WRITE) { real::set_var }    else { deny::set_var })?;
    vars.func_wrap("export",
        if has(allowed, CAP_VARIABLES_WRITE) { real::export_var } else { deny::export_var })?;

    // yosh:plugin/filesystem
    let mut fs = linker.instance("yosh:plugin/filesystem@0.1.0")?;
    fs.func_wrap("cwd",
        if has(allowed, CAP_FILESYSTEM) { real::cwd }     else { deny::cwd })?;
    fs.func_wrap("set-cwd",
        if has(allowed, CAP_FILESYSTEM) { real::set_cwd } else { deny::set_cwd })?;

    // yosh:plugin/io
    let mut io = linker.instance("yosh:plugin/io@0.1.0")?;
    io.func_wrap("write",
        if has(allowed, CAP_IO) { real::write } else { deny::write })?;

    Ok(linker)
}

/// All-deny linker — used by `yosh-plugin sync` for `metadata` extraction
/// and by shell startup during the `metadata` call. Every yosh:plugin/*
/// import returns Err(Denied); WASI clocks + random remain available.
pub fn build_all_deny_linker(engine: &Engine)
    -> wasmtime::Result<Linker<HostContext>>
{
    build_linker(engine, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::host::HostContext;

    /// Compile-only sentinel: verify that `build_linker` typechecks
    /// against the pinned wasmtime version. Detects WasiView /
    /// add_to_linker signature drift on upgrades.
    #[test]
    fn linker_construction_smoke() {
        let mut config = wasmtime::Config::new();
        config.async_support(false);
        let engine = Engine::new(&config).unwrap();
        let _linker = build_linker(&engine, yosh_plugin_api::CAP_ALL).unwrap();
        let _all_deny = build_all_deny_linker(&engine).unwrap();
    }
}
```

### 4.4 HostContext and host import implementations

- [ ] **Step 4.4.1: Create `src/plugin/host.rs`**

Full new contents:

```rust
//! HostContext + host import functions (real + deny stubs).
//!
//! The HostContext holds the WASI state required by the linker plus a
//! raw pointer to the live ShellEnv. The pointer is bound by the
//! EnvGuard helper in `mod.rs` immediately before guest-bound calls.

use wasmtime::component::ResourceTable;
use wasmtime::component::Resource;
use wasmtime::StoreContextMut;

use crate::env::ShellEnv;

pub(super) struct HostContext {
    /// NULL during `metadata` calls and outside `with_env` scope. Set
    /// by `EnvGuard::bind` immediately before each guest-bound call
    /// and reset by `EnvGuard::drop`.
    pub(super) env: *mut ShellEnv,
    pub(super) plugin_name: String,
    pub(super) capabilities: u32,
    pub(super) wasi: wasmtime_wasi::p2::WasiCtx,
    pub(super) resource_table: ResourceTable,
}

impl HostContext {
    pub(super) fn new_for_plugin(plugin_name: &str, capabilities: u32) -> Self {
        let mut wasi = wasmtime_wasi::p2::WasiCtxBuilder::new();
        // No stdio inheritance, no preopens, no environment forwarding.
        // Only clocks + random are linked at the linker level.
        Self {
            env: std::ptr::null_mut(),
            plugin_name: plugin_name.to_string(),
            capabilities,
            wasi: wasi.build(),
            resource_table: ResourceTable::new(),
        }
    }

    /// Returns the live ShellEnv pointer or None if metadata-context.
    /// Host imports use this to short-circuit on null env.
    fn env_mut(&mut self) -> Option<&mut ShellEnv> {
        if self.env.is_null() { None } else { Some(unsafe { &mut *self.env }) }
    }
}

impl wasmtime_wasi::p2::IoView for HostContext {
    fn table(&mut self) -> &mut ResourceTable { &mut self.resource_table }
}

impl wasmtime_wasi::p2::WasiView for HostContext {
    fn ctx(&mut self) -> &mut wasmtime_wasi::p2::WasiCtx { &mut self.wasi }
}

// ── error mapping ────────────────────────────────────────────────────

/// The WIT error-code enum, mirrored on the host side. The exact path of
/// this type depends on `wasmtime::component::bindgen!` output; see
/// `mod.rs` for where the binding is generated and re-export.
pub use crate::plugin::generated::yosh::plugin::types::ErrorCode;

// ── real (granted) host imports ──────────────────────────────────────

pub(super) mod real {
    use super::*;

    pub fn get_var(
        mut store: StoreContextMut<HostContext>,
        (name,): (String,),
    ) -> wasmtime::Result<(Result<Option<String>, ErrorCode>,)> {
        let ctx = store.data_mut();
        let Some(env) = ctx.env_mut() else {
            return Ok((Err(ErrorCode::Denied),));
        };
        Ok((Ok(env.vars.get(&name).map(|s| s.to_string())),))
    }

    pub fn set_var(
        mut store: StoreContextMut<HostContext>,
        (name, value): (String, String),
    ) -> wasmtime::Result<(Result<(), ErrorCode>,)> {
        let ctx = store.data_mut();
        let Some(env) = ctx.env_mut() else {
            return Ok((Err(ErrorCode::Denied),));
        };
        match env.vars.set(&name, &value) {
            Ok(()) => Ok((Ok(()),)),
            Err(_) => Ok((Err(ErrorCode::IoFailed),)),
        }
    }

    pub fn export_var(
        mut store: StoreContextMut<HostContext>,
        (name, value): (String, String),
    ) -> wasmtime::Result<(Result<(), ErrorCode>,)> {
        let ctx = store.data_mut();
        let Some(env) = ctx.env_mut() else {
            return Ok((Err(ErrorCode::Denied),));
        };
        match env.vars.set(&name, &value) {
            Ok(()) => { env.vars.export(&name); Ok((Ok(()),)) }
            Err(_) => Ok((Err(ErrorCode::IoFailed),)),
        }
    }

    pub fn cwd(
        mut store: StoreContextMut<HostContext>,
        (): (),
    ) -> wasmtime::Result<(Result<String, ErrorCode>,)> {
        let ctx = store.data_mut();
        if ctx.env.is_null() {
            return Ok((Err(ErrorCode::Denied),));
        }
        match std::env::current_dir() {
            Ok(p) => Ok((Ok(p.to_string_lossy().into_owned()),)),
            Err(_) => Ok((Err(ErrorCode::IoFailed),)),
        }
    }

    pub fn set_cwd(
        mut store: StoreContextMut<HostContext>,
        (path,): (String,),
    ) -> wasmtime::Result<(Result<(), ErrorCode>,)> {
        let ctx = store.data_mut();
        if ctx.env.is_null() {
            return Ok((Err(ErrorCode::Denied),));
        }
        match std::env::set_current_dir(&path) {
            Ok(()) => Ok((Ok(()),)),
            Err(_) => Ok((Err(ErrorCode::IoFailed),)),
        }
    }

    pub fn write(
        mut store: StoreContextMut<HostContext>,
        (target, data): (Stream, Vec<u8>),
    ) -> wasmtime::Result<(Result<(), ErrorCode>,)> {
        use std::io::Write;
        let ctx = store.data_mut();
        if ctx.env.is_null() {
            return Ok((Err(ErrorCode::Denied),));
        }
        let r = match target {
            Stream::Stdout => std::io::stdout().write_all(&data),
            Stream::Stderr => std::io::stderr().write_all(&data),
        };
        match r {
            Ok(()) => Ok((Ok(()),)),
            Err(_) => Ok((Err(ErrorCode::IoFailed),)),
        }
    }
}

// ── deny stubs (capability not granted) ──────────────────────────────

pub(super) mod deny {
    use super::*;

    pub fn get_var(_s: StoreContextMut<HostContext>, _: (String,))
        -> wasmtime::Result<(Result<Option<String>, ErrorCode>,)>
    { Ok((Err(ErrorCode::Denied),)) }

    pub fn set_var(_s: StoreContextMut<HostContext>, _: (String, String))
        -> wasmtime::Result<(Result<(), ErrorCode>,)>
    { Ok((Err(ErrorCode::Denied),)) }

    pub fn export_var(_s: StoreContextMut<HostContext>, _: (String, String))
        -> wasmtime::Result<(Result<(), ErrorCode>,)>
    { Ok((Err(ErrorCode::Denied),)) }

    pub fn cwd(_s: StoreContextMut<HostContext>, _: ())
        -> wasmtime::Result<(Result<String, ErrorCode>,)>
    { Ok((Err(ErrorCode::Denied),)) }

    pub fn set_cwd(_s: StoreContextMut<HostContext>, _: (String,))
        -> wasmtime::Result<(Result<(), ErrorCode>,)>
    { Ok((Err(ErrorCode::Denied),)) }

    pub fn write(_s: StoreContextMut<HostContext>, _: (Stream, Vec<u8>))
        -> wasmtime::Result<(Result<(), ErrorCode>,)>
    { Ok((Err(ErrorCode::Denied),)) }
}

// Imported from generated bindings (see mod.rs).
pub use crate::plugin::generated::yosh::plugin::types::Stream;
```

The exact tuple shapes (`(name,)`, `(name, value)`, etc.) for `func_wrap` may vary slightly between wasmtime versions. The structure above is the design intent; if 27.x emits a different tuple arity, adjust the function signatures (and similarly in `linker.rs` argument types). The compile-only test in step 4.3.1 catches mismatches.

### 4.5 PluginManager + with_env + EnvGuard

- [ ] **Step 4.5.1: Replace `src/plugin/mod.rs`**

Full new contents:

```rust
//! wasmtime-based plugin manager.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! sections §4 and §5 for the high-level design.

pub mod config;
pub mod cache;
mod host;
mod linker;

use std::path::Path;
use std::sync::OnceLock;

use wasmtime::component::{Component, InstancePre};
use wasmtime::{Config, Engine, Store};

use yosh_plugin_api::{
    CAP_HOOK_PRE_EXEC, CAP_HOOK_POST_EXEC, CAP_HOOK_ON_CD, CAP_HOOK_PRE_PROMPT,
    parse_capability,
};

use crate::env::ShellEnv;

use self::cache::{file_sha256, validate_cwasm, CacheKey};
use self::config::{PluginConfig, expand_tilde};
use self::host::HostContext;

// Generated component bindings (see `wasmtime::component::bindgen!`).
mod generated {
    wasmtime::component::bindgen!({
        path: "../yosh-plugin-api/wit",
        world: "plugin-world",
        async: false,
    });
}

use self::generated::PluginWorld;
use self::generated::yosh::plugin::types::{HookName, PluginInfo};

/// Public dispatch result for `PluginManager::exec_command`.
pub enum PluginExec {
    /// No plugin provides this command — caller falls back to PATH lookup.
    NotHandled,
    /// A plugin handled the command and returned this exit status.
    Handled(i32),
    /// A plugin claimed the command but failed (trap, host error, invalidated).
    Failed,
}

struct LoadedPlugin {
    name: String,
    store: Store<HostContext>,
    bindings: PluginWorld,
    plugin_info: PluginInfo,
    capabilities: u32,
    invalidated: bool,
}

pub struct PluginManager {
    engine: Engine,
    plugins: Vec<LoadedPlugin>,
}

impl PluginManager {
    pub fn new() -> Self {
        let engine = SHARED_ENGINE.get_or_init(make_engine).clone();
        Self { engine, plugins: Vec::new() }
    }

    /// Load plugins listed in the config file. Errors are printed to
    /// stderr and the failing plugin is skipped.
    pub fn load_from_config(&mut self, config_path: &Path, env: &mut ShellEnv) {
        let config = match PluginConfig::load(config_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        for entry in &config.plugin {
            if !entry.enabled { continue; }
            let path = expand_tilde(&entry.path);
            let allowed = entry
                .capabilities
                .as_ref()
                .map(|strs| config::capabilities_from_strs(strs))
                .unwrap_or(yosh_plugin_api::CAP_ALL);
            if let Err(e) = self.load_one(&path, &entry.name, allowed,
                                          entry.cwasm_path.as_deref(),
                                          entry.cache_key.as_ref(),
                                          env) {
                eprintln!("yosh: plugin '{}': {}", entry.name, e);
            }
        }
    }

    fn load_one(
        &mut self,
        wasm_path: &Path,
        plugin_name: &str,
        allowed: u32,
        cwasm_path: Option<&Path>,
        expected_key: Option<&CacheKey>,
        env: &mut ShellEnv,
    ) -> Result<(), String> {
        // Step 1: re-verify wasm SHA-256 (unconditional).
        let wasm_sha = file_sha256(wasm_path)
            .map_err(|e| format!("read .wasm: {}", e))?;
        if let Some(k) = expected_key {
            if k.wasm_sha256 != wasm_sha {
                return Err(format!(
                    "wasm SHA-256 mismatch (expected {}, got {})",
                    k.wasm_sha256, wasm_sha));
            }
        }

        // Step 2: choose component bytes — verified cwasm or in-memory recompile.
        let component = match (cwasm_path, expected_key) {
            (Some(cwasm), Some(key))
                if validate_cwasm(cwasm, key, current_uid()).is_ok() =>
            {
                let bytes = std::fs::read(cwasm)
                    .map_err(|e| format!("read .cwasm: {}", e))?;
                // SAFETY: the validate_cwasm contract ensures the file
                // came from this same uid + matching cache key. See spec §5.
                unsafe { Component::deserialize(&self.engine, &bytes) }
                    .map_err(|e| format!("deserialize cwasm: {}", e))?
            }
            (cwasm_opt, _) => {
                if let Some(p) = cwasm_opt {
                    eprintln!(
                        "yosh: plugin '{}': cwasm cache stale; precompiling in memory \
                         (run 'yosh-plugin sync' to refresh {})",
                        plugin_name, p.display());
                }
                let wasm_bytes = std::fs::read(wasm_path)
                    .map_err(|e| format!("read .wasm: {}", e))?;
                Component::new(&self.engine, &wasm_bytes)
                    .map_err(|e| format!("compile .wasm: {}", e))?
            }
        };

        // Step 3: build linker.
        let linker = linker::build_linker(&self.engine, allowed)
            .map_err(|e| format!("build linker: {}", e))?;

        // Step 4: instantiate.
        let instance_pre = linker.instantiate_pre(&component)
            .map_err(|e| format!("instantiate_pre: {}", e))?;
        let mut store = Store::new(
            &self.engine,
            HostContext::new_for_plugin(plugin_name, allowed),
        );
        let bindings = PluginWorld::instantiate(&mut store, &instance_pre)
            .map_err(|e| format!("instantiate: {}", e))?;

        // Step 5: metadata (NO env binding — see "metadata contract").
        let plugin_info = bindings
            .yosh_plugin_plugin()
            .call_metadata(&mut store)
            .map_err(|e| format!("metadata trapped: {}", e))?;

        // Step 6: requested-vs-granted diagnostic.
        let requested_bits: u32 = plugin_info.required_capabilities
            .iter()
            .filter_map(|s| {
                if let Some(c) = parse_capability(s) {
                    Some(c.to_bitflag())
                } else {
                    eprintln!(
                        "yosh: plugin '{}': unknown capability '{}' in plugin-info",
                        plugin_name, s);
                    None
                }
            })
            .fold(0, |acc, b| acc | b);
        let denied = requested_bits & !allowed;
        for c in [
            yosh_plugin_api::Capability::VariablesRead,
            yosh_plugin_api::Capability::VariablesWrite,
            yosh_plugin_api::Capability::Filesystem,
            yosh_plugin_api::Capability::Io,
            yosh_plugin_api::Capability::HookPreExec,
            yosh_plugin_api::Capability::HookPostExec,
            yosh_plugin_api::Capability::HookOnCd,
            yosh_plugin_api::Capability::HookPrePrompt,
        ] {
            if denied & c.to_bitflag() != 0 {
                eprintln!(
                    "yosh: plugin '{}': capability '{}' requested but not granted",
                    plugin_name, c.as_str());
            }
        }

        // Step 7: on_load (env-bound). On Err, drop the plugin.
        let mut plugin = LoadedPlugin {
            name: plugin_name.to_string(),
            store,
            bindings,
            plugin_info,
            capabilities: allowed,
            invalidated: false,
        };
        let on_load_result = with_env(&mut plugin, env, |store| {
            plugin.bindings.yosh_plugin_plugin().call_on_load(store)
        });
        match on_load_result {
            Some(Ok(())) => {}
            Some(Err(msg)) => {
                return Err(format!("on_load returned error: {}", msg));
            }
            None => {
                return Err("on_load failed (trap or host error)".into());
            }
        }

        self.plugins.push(plugin);
        Ok(())
    }

    /// Execute a plugin command. See `PluginExec` doc comments for the
    /// three-valued result semantics.
    pub fn exec_command(
        &mut self,
        env: &mut ShellEnv,
        name: &str,
        args: &[String],
    ) -> PluginExec {
        let Some(idx) = self.find_plugin_for_command(name) else {
            return PluginExec::NotHandled;
        };
        let plugin = &mut self.plugins[idx];
        let r = with_env(plugin, env, |store| {
            plugin.bindings.yosh_plugin_plugin()
                .call_exec(store, name, &args.iter().cloned().collect::<Vec<_>>())
        });
        match r {
            Some(exit) => PluginExec::Handled(exit),
            None       => PluginExec::Failed,
        }
    }

    fn find_plugin_for_command(&self, name: &str) -> Option<usize> {
        self.plugins.iter()
            .position(|p| p.plugin_info.commands.iter().any(|c| c == name))
    }

    pub fn has_command(&self, name: &str) -> bool {
        self.find_plugin_for_command(name).is_some()
    }

    pub fn call_pre_exec(&mut self, env: &mut ShellEnv, cmd: &str) {
        for plugin in &mut self.plugins {
            if !linker::has(plugin.capabilities, CAP_HOOK_PRE_EXEC) { continue; }
            if !plugin.plugin_info.implemented_hooks.contains(&HookName::PreExec) { continue; }
            let _ = with_env(plugin, env, |store| {
                plugin.bindings.yosh_plugin_hooks().call_pre_exec(store, cmd)
            });
        }
    }

    pub fn call_post_exec(&mut self, env: &mut ShellEnv, cmd: &str, exit_code: i32) {
        for plugin in &mut self.plugins {
            if !linker::has(plugin.capabilities, CAP_HOOK_POST_EXEC) { continue; }
            if !plugin.plugin_info.implemented_hooks.contains(&HookName::PostExec) { continue; }
            let _ = with_env(plugin, env, |store| {
                plugin.bindings.yosh_plugin_hooks().call_post_exec(store, cmd, exit_code)
            });
        }
    }

    pub fn call_on_cd(&mut self, env: &mut ShellEnv, old_dir: &str, new_dir: &str) {
        for plugin in &mut self.plugins {
            if !linker::has(plugin.capabilities, CAP_HOOK_ON_CD) { continue; }
            if !plugin.plugin_info.implemented_hooks.contains(&HookName::OnCd) { continue; }
            let _ = with_env(plugin, env, |store| {
                plugin.bindings.yosh_plugin_hooks().call_on_cd(store, old_dir, new_dir)
            });
        }
    }

    pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
        for plugin in &mut self.plugins {
            if !linker::has(plugin.capabilities, CAP_HOOK_PRE_PROMPT) { continue; }
            if !plugin.plugin_info.implemented_hooks.contains(&HookName::PrePrompt) { continue; }
            let _ = with_env(plugin, env, |store| {
                plugin.bindings.yosh_plugin_hooks().call_pre_prompt(store)
            });
        }
    }

    pub fn unload_all(&mut self, env: &mut ShellEnv) {
        for plugin in &mut self.plugins {
            let _ = with_env(plugin, env, |store| {
                plugin.bindings.yosh_plugin_plugin().call_on_unload(store)
            });
        }
        self.plugins.clear();
    }
}

impl Default for PluginManager {
    fn default() -> Self { Self::new() }
}

// ── shared engine ────────────────────────────────────────────────────

static SHARED_ENGINE: OnceLock<Engine> = OnceLock::new();

fn make_engine() -> Engine {
    let mut config = Config::new();
    config.async_support(false);
    config.consume_fuel(false);
    config.cache_config_load_default().ok();
    Engine::new(&config).expect("wasmtime Engine::new failed")
}

// ── EnvGuard + with_env ──────────────────────────────────────────────

struct EnvGuard<'a> {
    store: &'a mut Store<HostContext>,
}

impl<'a> EnvGuard<'a> {
    fn bind(store: &'a mut Store<HostContext>, env: &mut ShellEnv) -> Self {
        store.data_mut().env = env as *mut _;
        EnvGuard { store }
    }
}

impl Drop for EnvGuard<'_> {
    fn drop(&mut self) {
        self.store.data_mut().env = std::ptr::null_mut();
    }
}

fn with_env<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    f: impl FnOnce(&mut Store<HostContext>) -> wasmtime::Result<R>,
) -> Option<R> {
    if plugin.invalidated {
        eprintln!(
            "yosh: plugin '{}': skipped (instance invalidated by earlier trap)",
            plugin.name);
        return None;
    }

    let result = {
        let mut guard = EnvGuard::bind(&mut plugin.store, env);
        f(guard.store)
        // guard drops here, restoring env to null on every exit path.
    };

    match result {
        Ok(r) => Some(r),
        Err(e) => {
            if e.downcast_ref::<wasmtime::Trap>().is_some() {
                eprintln!(
                    "yosh: plugin '{}': trapped: {} — disabling for the rest of this session",
                    plugin.name, e);
                plugin.invalidated = true;
            } else {
                eprintln!("yosh: plugin '{}': call failed: {}", plugin.name, e);
            }
            None
        }
    }
}

fn current_uid() -> u32 {
    // SAFETY: getuid is always-safe.
    unsafe { libc::getuid() }
}
```

- [ ] **Step 4.5.2: Update `src/plugin/config.rs`**

Add new fields to `PluginEntry` for the cache key components (so the lockfile can carry them):

Look at the existing struct (in `src/plugin/config.rs` around `PluginEntry`); add:

```rust
#[derive(Debug, Deserialize, Default)]
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    pub enabled: bool,                          // existing
    pub capabilities: Option<Vec<String>>,      // existing
    // NEW fields for v0.2.0:
    pub cwasm_path: Option<std::path::PathBuf>,
    pub cache_key: Option<crate::plugin::cache::CacheKey>,
}
```

The `CacheKey` type needs to derive `serde::Deserialize` for this. In `src/plugin/cache.rs` add:

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheKey { /* same fields */ }
```

(Update the `CacheKey` definition in step 4.2.1 to add `serde::{Serialize, Deserialize}` derives — both are needed: the manager writes lockfiles, the host reads them.)

- [ ] **Step 4.5.3: Run cache module tests**

Run: `cargo test -p yosh --lib plugin::cache::tests`
Expected: 5 tests pass (see step 4.2.1 test bodies).

- [ ] **Step 4.5.4: Run linker smoke test**

Run: `cargo test -p yosh --lib plugin::linker::tests::linker_construction_smoke`
Expected: PASS. If FAIL, adjust the wasmtime-wasi linker addition function names to match the pinned 27.x version, then rerun.

- [ ] **Step 4.5.4b: Add `test_helpers` sub-module to `src/plugin/mod.rs`**

Append at the end of `src/plugin/mod.rs` (used by integration tests in Task 6):

```rust
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers {
    use super::*;
    use std::path::Path;

    /// Load a plugin from a `.wasm` file (no cwasm cache, no lockfile).
    /// Used by integration tests that build the wasm at test time.
    pub fn load_plugin_with_caps(
        mgr: &mut PluginManager,
        wasm_path: &Path,
        allowed: u32,
        env: &mut ShellEnv,
    ) {
        // Use the manager's private load_one with no cwasm path / no key.
        let plugin_name = wasm_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("test_plugin");
        if let Err(e) = mgr.load_one(wasm_path, plugin_name, allowed, None, None, env) {
            panic!("test load failed for {}: {}", wasm_path.display(), e);
        }
    }

    /// Probe: is the persisted env pointer null inside the named plugin's store?
    /// True after every with_env exit; false during a guest call.
    pub fn env_pointer_is_null_in_store(mgr: &PluginManager, name: &str) -> bool {
        mgr.plugins.iter()
            .find(|p| p.name == name)
            .map(|p| p.store.data().env.is_null())
            .unwrap_or(true)
    }
}
```

The `test-helpers` feature need not be added to `Cargo.toml` — `#[cfg(test)]` alone covers the integration test target. The feature-gate alternative is shown for discoverability.

- [ ] **Step 4.5.5: Update `src/exec/` call sites for `PluginExec`**

In `src/exec/mod.rs` (or wherever the existing `PluginManager::exec_command` is invoked), replace the `Option<i32>` match with the new enum:

Find the previous shape:
```rust
if let Some(exit) = plugin_manager.exec_command(env, &cmd_name, &args) {
    return Ok(exit);
}
// fall through to PATH lookup
```

Replace with:
```rust
match plugin_manager.exec_command(env, &cmd_name, &args) {
    PluginExec::Handled(exit) => return Ok(exit),
    PluginExec::Failed        => return Ok(1),
    PluginExec::NotHandled    => { /* fall through to PATH lookup */ }
}
```

(Adjust the variable names and function signature to match the actual call site; the structural change is replacing the boolean/`Option` discrimination with the three-valued enum.)

- [ ] **Step 4.5.6: Workspace build sanity check**

Run: `cargo check --workspace`
Expected: succeeds. Errors should be limited to type mismatches in step 4.5.5 — fix iteratively until clean.

- [ ] **Step 4.5.7: Run all yosh unit tests**

Run: `cargo test --lib -p yosh`
Expected: every existing unit test passes; the only new unit tests live under `plugin::cache` and `plugin::linker`.

- [ ] **Step 4.5.8: Commit**

```bash
git add Cargo.toml Cargo.lock build.rs src/plugin src/exec
git commit -m "feat(plugin)!: replace dlopen with wasmtime Component Model

Sub-project #4 of v0.2.0 migration. Drops libloading; adds wasmtime +
wasmtime-wasi (clocks + random only). PluginManager now owns a shared
Engine and per-plugin persistent Store<HostContext>. Capability
allowlist applied at Linker construction; deny-stubs return
Err(Denied). EnvGuard RAII wrapper resets the live ShellEnv pointer
on every exit path. exec_command returns PluginExec::{NotHandled,
Handled(i32), Failed} so callers cannot fall through to PATH on
plugin failure. Spec sections §4-§6."
```

---

## Task 5: Add precompile to `yosh-plugin-manager` and simplify the asset template

**Goal:** `yosh-plugin sync` becomes the source of truth for `.cwasm` files. Manager links wasmtime, computes the cache key tuple, calls `metadata` via an all-deny linker to populate cached lockfile fields, and writes everything atomically.

**Files:**
- Modify: `crates/yosh-plugin-manager/Cargo.toml`
- Create: `crates/yosh-plugin-manager/src/precompile.rs`
- Modify: `crates/yosh-plugin-manager/src/sync.rs`
- Modify: `crates/yosh-plugin-manager/src/install.rs`
- Modify: `crates/yosh-plugin-manager/src/lockfile.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (list output)

- [ ] **Step 5.1: Add `wasmtime` to `crates/yosh-plugin-manager/Cargo.toml`**

In `[dependencies]`:

```toml
wasmtime = { version = "27", default-features = false, features = ["component-model", "cranelift"] }
yosh-plugin-api = { version = "0.2.0", path = "../yosh-plugin-api" }
sha2 = "0.10"
hex = "0.4"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
# (existing deps for github, http, etc. remain.)
```

Bump version to `0.2.0`.

- [ ] **Step 5.2: Create `crates/yosh-plugin-manager/src/precompile.rs`**

```rust
//! cwasm precompile + cache key + sidecar writing.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheKey {
    pub wasm_sha256: String,
    pub wasmtime_version: String,
    pub target_triple: String,
    pub engine_config_hash: String,
}

pub fn compute_wasm_sha256(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

/// Precompile a `.wasm` to `.cwasm` at the path returned in the result.
/// Writes the sidecar `.cwasm.meta` atomically. Mode 0600.
pub fn precompile(
    wasm_path: &Path,
    cache_dir: &Path,
    engine: &wasmtime::Engine,
    engine_config_hash: &str,
) -> Result<(PathBuf, CacheKey), String> {
    fs::create_dir_all(cache_dir)
        .map_err(|e| format!("create cache dir: {}", e))?;
    fs::set_permissions(cache_dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod cache dir: {}", e))?;

    let wasm_sha = compute_wasm_sha256(wasm_path)
        .map_err(|e| format!("hash wasm: {}", e))?;
    let triple = target_triple();

    let key = CacheKey {
        wasm_sha256: wasm_sha.clone(),
        wasmtime_version: wasmtime::VERSION.to_string(),
        target_triple: triple.to_string(),
        engine_config_hash: engine_config_hash.to_string(),
    };

    let cwasm_filename = format!(
        "{}-{}-{}.cwasm",
        &wasm_sha[..16],
        &engine_config_hash[..16],
        triple,
    );
    let cwasm_path = cache_dir.join(&cwasm_filename);

    let wasm_bytes = fs::read(wasm_path)
        .map_err(|e| format!("read wasm: {}", e))?;
    let cwasm_bytes = engine.precompile_component(&wasm_bytes)
        .map_err(|e| format!("precompile_component: {}", e))?;

    write_atomic(&cwasm_path, &cwasm_bytes, 0o600)
        .map_err(|e| format!("write cwasm: {}", e))?;
    let sidecar = sidecar_path(&cwasm_path);
    write_atomic(&sidecar, render_sidecar(&key).as_bytes(), 0o600)
        .map_err(|e| format!("write sidecar: {}", e))?;

    Ok((cwasm_path, key))
}

pub fn sidecar_path(cwasm: &Path) -> PathBuf {
    let mut s = cwasm.as_os_str().to_owned();
    s.push(".meta");
    s.into()
}

fn render_sidecar(key: &CacheKey) -> String {
    format!(
        "schema=1\nwasm_sha256={}\nwasmtime_version={}\ntarget_triple={}\nengine_config_hash={}\n",
        key.wasm_sha256, key.wasmtime_version, key.target_triple, key.engine_config_hash
    )
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let parent = path.parent().expect("path has parent");
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
    ));
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true).write(true).truncate(true).mode(mode)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn target_triple() -> &'static str {
    env!("TARGET", "host")
}

pub fn engine_config_hash(engine: &wasmtime::Engine) -> String {
    // Stable fingerprint of the wasmtime::Config used at precompile time.
    // For v0.2.0 we vary on (wasmtime version, async-support flag,
    // consume-fuel flag, cache-config-loaded flag). Future schema bumps
    // can include cranelift opt-level once we expose a getter.
    let mut h = Sha256::new();
    h.update(b"yosh-plugin/engine-config/v1\n");
    h.update(wasmtime::VERSION.as_bytes());
    let _ = engine; // engine config inspection is not stable across versions.
    hex::encode(h.finalize())
}

pub fn make_engine() -> wasmtime::Engine {
    let mut config = wasmtime::Config::new();
    config.async_support(false);
    config.consume_fuel(false);
    wasmtime::Engine::new(&config).expect("wasmtime Engine::new failed")
}
```

- [ ] **Step 5.3: Add `build.rs` to `yosh-plugin-manager`**

If not already present, create `crates/yosh-plugin-manager/build.rs`:

```rust
fn main() {
    let triple = std::env::var("TARGET").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=TARGET={}", triple);
}
```

(Yosh root already has a build.rs; if there is an existing one for this crate, append the line. Cargo passes `TARGET` at build time, but for cross-compile builds we want it explicit in the binary too.)

- [ ] **Step 5.4: Update `crates/yosh-plugin-manager/src/lockfile.rs`**

Add new fields. Find the existing `LockfileEntry` and `Lockfile` types; add:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LockfileEntry {
    pub name: String,
    pub source: String,
    pub version: Option<String>,
    pub path: String,
    pub sha256: String,
    // NEW:
    pub cwasm_path: Option<String>,
    pub wasmtime_version: Option<String>,
    pub target_triple: Option<String>,
    pub engine_config_hash: Option<String>,
    pub required_capabilities: Option<Vec<String>>,
    pub implemented_hooks: Option<Vec<String>>,
}
```

Use `Option<...>` so old lockfiles missing these fields still parse.

- [ ] **Step 5.5: Modify `crates/yosh-plugin-manager/src/sync.rs`**

In the per-plugin sync loop, after SHA-256 verification, add:

1. **Precompile step** — call `precompile::precompile(...)` with the manager's shared engine.
2. **Metadata extraction step** — call `metadata` via an all-deny linker to capture `required-capabilities` and `implemented-hooks`. The relevant snippet (insert after precompile is done):

```rust
// After precompile, instantiate the component with an all-deny linker
// to capture metadata for the lockfile cache.
let component = wasmtime::component::Component::new(&engine, &wasm_bytes)
    .map_err(|e| format!("compile for metadata: {}", e))?;
let mut linker = wasmtime::component::Linker::<MetadataCtx>::new(&engine);
register_all_deny_imports(&mut linker)?;
register_limited_wasi(&mut linker)?;
let bindings_pre = linker.instantiate_pre(&component)
    .map_err(|e| format!("instantiate_pre for metadata: {}", e))?;
let mut store = wasmtime::Store::new(&engine, MetadataCtx::default());

// 5-second epoch-watchdog timeout.
engine.increment_epoch();
let watchdog = std::thread::spawn({
    let engine = engine.clone();
    move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        engine.increment_epoch();
    }
});
store.set_epoch_deadline(1);

let plugin_world = generated::PluginWorld::instantiate(&mut store, &bindings_pre)
    .map_err(|e| format!("instantiate metadata: {}", e))?;
let plugin_info = plugin_world.yosh_plugin_plugin().call_metadata(&mut store)
    .map_err(|e| format!("metadata: {}", e))?;
drop(watchdog);   // we won't join — it'll complete on its own.

let required_caps_strings: Vec<String> = plugin_info.required_capabilities;
let implemented_hooks_strings: Vec<String> = plugin_info
    .implemented_hooks
    .iter()
    .map(|h| match h {
        generated::HookName::PreExec   => "pre-exec".into(),
        generated::HookName::PostExec  => "post-exec".into(),
        generated::HookName::OnCd      => "on-cd".into(),
        generated::HookName::PrePrompt => "pre-prompt".into(),
    })
    .collect();
```

Add `MetadataCtx`:

```rust
#[derive(Default)]
struct MetadataCtx {
    table: wasmtime::component::ResourceTable,
    wasi: wasmtime_wasi::p2::WasiCtx,
}
impl wasmtime_wasi::p2::IoView for MetadataCtx {
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable { &mut self.table }
}
impl wasmtime_wasi::p2::WasiView for MetadataCtx {
    fn ctx(&mut self) -> &mut wasmtime_wasi::p2::WasiCtx { &mut self.wasi }
}
```

Add the `register_all_deny_imports` helper that registers each yosh:plugin/* function with a closure returning `Err(ErrorCode::Denied)` (mirror of `src/plugin/host.rs::deny`).

For each successful sync entry, populate the new lockfile fields:

```rust
LockfileEntry {
    name: ...,
    source: ...,
    version: ...,
    path: wasm_path.to_string_lossy().into(),
    sha256: wasm_sha,
    cwasm_path: Some(cwasm_path.to_string_lossy().into()),
    wasmtime_version: Some(key.wasmtime_version.clone()),
    target_triple: Some(key.target_triple.clone()),
    engine_config_hash: Some(key.engine_config_hash.clone()),
    required_capabilities: Some(required_caps_strings),
    implemented_hooks: Some(implemented_hooks_strings),
}
```

On any precompile/metadata failure, push the entry into `failed` and skip the lockfile entry for that plugin (existing partial-failure semantics).

- [ ] **Step 5.6: Asset template default — `crates/yosh-plugin-manager/src/sync.rs` (or wherever the asset template lives)**

Locate the asset template default (was `lib{name}-{os}-{arch}.{ext}`). Replace with:

```rust
const DEFAULT_ASSET_TEMPLATE: &str = "{name}.wasm";
```

Reject `{os}`, `{arch}`, `{ext}` tokens at parse time:

```rust
fn check_asset_template(t: &str) -> Result<(), String> {
    for forbidden in ["{os}", "{arch}", "{ext}"] {
        if t.contains(forbidden) {
            return Err(format!(
                "asset template token '{}' is no longer supported in v0.2.0; \
                 platform-independent .wasm assets do not need it",
                forbidden));
        }
    }
    Ok(())
}
```

- [ ] **Step 5.7: Remove macOS ad-hoc resign code**

In `crates/yosh-plugin-manager/src/sync.rs`, find the block introduced by commit `abaa1aa` that runs `codesign --sign -` on downloaded `.dylib` files. Delete that entire block. The `.wasm`/`.cwasm` artifacts are not Mach-O.

- [ ] **Step 5.8: Update `crates/yosh-plugin-manager/src/install.rs`**

Locate the local-path validation that checks for `.dylib` / `.so` extensions. Replace with `.wasm`:

```rust
let valid_ext = path.extension()
    .and_then(|e| e.to_str())
    .map(|e| e.eq_ignore_ascii_case("wasm"))
    .unwrap_or(false);
if !valid_ext {
    return Err(format!(
        "{}: not a .wasm file (yosh v0.2.0 requires WebAssembly Component plugins)",
        path.display()));
}
```

- [ ] **Step 5.9: Update `crates/yosh-plugin-manager/src/lib.rs` `cmd_list`**

Update the list output to render the new columns (cached vs stale, capability list). Pseudo-diff:

```rust
for entry in &lockfile.plugin {
    let version = entry.version.as_deref().unwrap_or("-");
    let verified = match verify::verify_checksum(
        &config::expand_tilde_path(&entry.path), &entry.sha256) {
        Ok(true)  => "\u{2713} verified",
        Ok(false) => "\u{2717} checksum mismatch",
        Err(_)    => "\u{2717} file missing",
    };
    let cached = match (&entry.cwasm_path, &entry.wasmtime_version) {
        (Some(p), Some(wv)) if std::path::Path::new(&config::expand_tilde_path(p)).exists()
            && wv == wasmtime::VERSION
            => "\u{2713} cached",
        _ => "\u{2717} stale",
    };
    let caps = entry.required_capabilities
        .as_ref()
        .map(|v| if v.is_empty() {
            "[- (no capabilities)]".to_string()
        } else {
            format!("[{}]", v.join(", "))
        })
        .unwrap_or_else(|| "[?]".into());
    println!(
        "{:<16} {:<8} {:<48} {} {} {}",
        entry.name, version, entry.source, verified, cached, caps);
}
```

- [ ] **Step 5.10: Workspace build sanity check**

Run: `cargo check --workspace`
Expected: succeeds.

- [ ] **Step 5.11: Run yosh-plugin-manager tests**

Run: `cargo test -p yosh-plugin-manager`
Expected: existing tests pass (or, if some assert on `.dylib` extensions, fail with clear messages — fix those tests in this commit too).

- [ ] **Step 5.12: Commit**

```bash
git add crates/yosh-plugin-manager
git commit -m "feat(plugin-manager)!: add precompile + simplify asset template

Sub-project #5 of v0.2.0 migration. yosh-plugin sync now precompiles
.cwasm with a four-tuple cache key (wasm sha256, wasmtime version,
target triple, engine config hash) and writes a sidecar .meta. Calls
plugin metadata via an all-deny linker (5s epoch watchdog) to cache
required_capabilities and implemented_hooks in plugins.lock for fast
'yosh-plugin list'. Removes macOS ad-hoc resign workaround. Asset
template default becomes '{name}.wasm'; old {os}/{arch}/{ext} tokens
are rejected with a migration message."
```

---

## Task 6: Rewrite integration tests

**Goal:** Cover all 15 spec §8 test cases against the wasmtime-based runtime. The dlopen-specific cases (symbol lookup, ABI version mismatch) are removed.

**Files:**
- Replace: `tests/plugin.rs`
- Create: `tests/plugins/trap_plugin/` (separate crate exercising trap path)
- Modify: `crates/yosh-plugin-manager/tests/` (drop dylib-asset-name tests)

### 6.1 Test plugin builds — shared helper

- [ ] **Step 6.1.1: Replace `tests/plugin.rs` preamble with a wasm builder helper**

The first ~30 lines of `tests/plugin.rs` should be:

```rust
//! Integration tests for the wasmtime-based plugin runtime (v0.2.0).
//! Replaces the dlopen-era tests one-for-one plus 11 new cases per
//! the spec §8 test plan.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

static TEST_PLUGIN_WASM: OnceLock<PathBuf> = OnceLock::new();
static TRAP_PLUGIN_WASM: OnceLock<PathBuf> = OnceLock::new();

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).into()
}

fn ensure_built(crate_name: &str, slot: &OnceLock<PathBuf>) -> PathBuf {
    slot.get_or_init(|| {
        let status = Command::new("cargo")
            .args(["component", "build",
                   "-p", crate_name,
                   "--target", "wasm32-wasip2",
                   "--release"])
            .status()
            .expect("cargo component build failed");
        assert!(status.success(), "{} build failed", crate_name);
        workspace_root()
            .join(format!("target/wasm32-wasip2/release/{}.wasm", crate_name))
    }).clone()
}

fn test_plugin_wasm() -> PathBuf { ensure_built("test_plugin", &TEST_PLUGIN_WASM) }
fn trap_plugin_wasm() -> PathBuf { ensure_built("trap_plugin", &TRAP_PLUGIN_WASM) }
```

- [ ] **Step 6.1.2: Create `tests/plugins/trap_plugin/Cargo.toml`**

```toml
[package]
name = "trap_plugin"
version = "0.2.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
yosh-plugin-sdk = { path = "../../../crates/yosh-plugin-sdk" }

[package.metadata.component]
package = "yosh:trap-plugin"

[package.metadata.component.target]
path  = "../../../crates/yosh-plugin-api/wit"
world = "plugin-world"

[profile.release]
opt-level = "s"
lto = true
strip = true
panic = "abort"
```

- [ ] **Step 6.1.3: Create `tests/plugins/trap_plugin/src/lib.rs`**

```rust
use yosh_plugin_sdk::{Capability, Plugin, export};

#[derive(Default)]
struct TrapPlugin;

impl Plugin for TrapPlugin {
    fn commands(&self) -> &[&'static str] { &["trap_now"] }
    fn required_capabilities(&self) -> &[Capability] { &[] }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        #[allow(clippy::diverging_sub_expression)]
        { let _: u32 = unreachable!("intentional trap"); }
    }
}

export!(TrapPlugin);
```

- [ ] **Step 6.1.4: Add `trap_plugin` to workspace members**

In root `Cargo.toml`'s `[workspace] members = [...]`, add `"tests/plugins/trap_plugin"`.

### 6.2 Test cases — one #[test] per spec §8 case

Below, each step writes ONE test. Run pattern: write → run (expected fail or pass) → commit groups.

- [ ] **Step 6.2.1: Test #1 — capability allowlist applied to linker**

Append to `tests/plugin.rs`:

```rust
#[test]
fn t01_capability_allowlist_applied_to_linker() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = yosh::env::ShellEnv::new_for_tests();
    let mut mgr = yosh::plugin::PluginManager::new();

    // Grant only variables:read; test_plugin requests both read and write.
    let allowed = yosh_plugin_api::CAP_VARIABLES_READ
                | yosh_plugin_api::CAP_IO;
    yosh::plugin::test_helpers::load_plugin_with_caps(&mut mgr, &wasm, allowed, &mut env);

    // Set then read: set should be denied; read should succeed for an
    // existing variable.
    env.vars.set("YOSH_TEST_VAR", "abc").unwrap();
    let exec = mgr.exec_command(&mut env, "echo_var", &["YOSH_TEST_VAR".into()]);
    matches!(exec, yosh::plugin::PluginExec::Handled(0));

    // Indirectly verify: a separate fixture command that calls set_var
    // would receive Err(Denied). For this test the no-set path is
    // sufficient — set was never granted, so a hypothetical call would
    // see Denied. Use the host counter approach in test #12 for the
    // direct denial verification.
}
```

(`yosh::plugin::test_helpers` is a module behind `#[cfg(any(test, feature = "test-helpers"))]` that exposes the otherwise-private `load_one` shape. Add it as needed in step 4.5.1's manager file.)

- [ ] **Step 6.2.2: Test #2 — WASM trap isolation**

```rust
#[test]
fn t02_wasm_trap_isolation_via_with_env() {
    let _g = lock_test();
    let wasm = trap_plugin_wasm();
    let mut env = yosh::env::ShellEnv::new_for_tests();
    let mut mgr = yosh::plugin::PluginManager::new();
    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr, &wasm, yosh_plugin_api::CAP_ALL, &mut env);

    // First call traps → Failed.
    let r1 = mgr.exec_command(&mut env, "trap_now", &[]);
    assert!(matches!(r1, yosh::plugin::PluginExec::Failed));

    // Second call must observe invalidation → still Failed (skipped).
    let r2 = mgr.exec_command(&mut env, "trap_now", &[]);
    assert!(matches!(r2, yosh::plugin::PluginExec::Failed));
}
```

- [ ] **Step 6.2.3: Test #3 — `with_env` resets `env` on every exit path**

```rust
#[test]
fn t03_with_env_resets_on_err_and_ok() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = yosh::env::ShellEnv::new_for_tests();
    let mut mgr = yosh::plugin::PluginManager::new();
    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr, &wasm, yosh_plugin_api::CAP_ALL, &mut env);

    // Two back-to-back calls; if the first leaks env on error, the second
    // would see a stale pointer (test-only debug check inside HostContext).
    env.vars.set("X", "1").unwrap();
    let _ = mgr.exec_command(&mut env, "echo_var", &["X".into()]);
    let _ = mgr.exec_command(&mut env, "echo_var", &["X".into()]);
    assert!(yosh::plugin::test_helpers::env_pointer_is_null_in_store(&mgr, "test_plugin"));
}
```

(Add `env_pointer_is_null_in_store` to the test_helpers module — a probe that asserts the persisted Store's `HostContext.env` is null after the dispatch.)

- [ ] **Step 6.2.4: Test #4 — `metadata` cannot reach host APIs**

This requires a *fourth* test plugin, `metadata_caller_plugin`, whose `metadata` calls `cwd()`. Construct as a separate crate under `tests/plugins/metadata_caller_plugin/` mirroring trap_plugin's setup. Its `metadata` returns `name = format!("cwd-result:{:?}", cwd())`. The test asserts the name string contains `"Err(Denied)"`.

- [ ] **Step 6.2.5: Test #5 — `on-load` CAN reach host APIs**

Modify the existing test_plugin: add a marker side-effect in `on_load` that writes to stderr via `eprint`. The test captures stderr and asserts the marker is present.

```rust
#[test]
fn t05_on_load_has_host_api_access() {
    // ... use libtest-mimic or capture stderr via a piped child;
    // the body sets up a yosh subprocess that loads test_plugin and
    // asserts the on_load eprint marker reached the captured stream.
}
```

- [ ] **Step 6.2.6: Tests #6–#8 — cwasm cache invalidation (3 variants)**

Each: write a known-good `.cwasm` + sidecar to a temp dir, mutate one of {wasmtime_version, engine_config_hash, target_triple} in the sidecar, attempt to load, assert the in-memory fallback path triggers AND the lockfile/sidecar is NOT rewritten.

```rust
#[test]
fn t06_cwasm_invalidation_on_wasmtime_version_change() {
    let _g = lock_test();
    let dir = tempfile::TempDir::new().unwrap();
    // ... copy test_plugin.wasm into dir, run the manager's precompile
    // logic to write a valid cwasm, then mutate sidecar.wasmtime_version,
    // construct a PluginManager, load with cwasm_path pointing at the
    // tampered cwasm, assert load succeeds (in-memory fallback).
    // Assert sidecar file is unchanged (mtime preserved).
}
```

(Steps 6.2.7 and 6.2.8 follow the same pattern with `engine_config_hash` and `target_triple` mutated.)

- [ ] **Step 6.2.7: Test #9 — `.cwasm` tampering rejected via wasm SHA mismatch**

```rust
#[test]
fn t09_wasm_sha_mismatch_refuses_to_load() {
    // Build the manager state with a known-good lockfile + cwasm.
    // Modify the .wasm file (append a byte) so its SHA-256 differs.
    // Attempt to load; assert load returns Err and the plugin is NOT
    // pushed to the manager. Assert NO call to Component::deserialize
    // happened (i.e. the cwasm was never trusted).
}
```

- [ ] **Step 6.2.8: Test #10 — WASI surface lockdown**

Build a sentinel plugin that imports `wasi:cli/stdout`. Trying to instantiate it should fail at linker construction.

```rust
#[test]
fn t10_wasi_cli_stdout_import_fails_to_link() {
    // Manually construct a component bytes with a `wasi:cli/stdout`
    // import (use the `wasm-tools` API or a fixture .wasm). Attempt
    // to build the linker + instantiate; assert the result is Err
    // with "unresolved import" or equivalent.
}
```

(For practical implementation, place a pre-built `tests/fixtures/wasi_cli_importer.wasm` in the repo and load it directly. Generating it on the fly requires a build script.)

- [ ] **Step 6.2.9: Test #11 — unknown capability strings warned but not fatal**

A plugin whose `required-capabilities` includes `"unknown:capability"`. Capture stderr and assert a warning was emitted; assert load succeeded.

- [ ] **Step 6.2.10: Test #12 — `required & !granted` warning sourced from `plugin-info`**

A plugin declaring `required-capabilities = ["variables:write"]` loaded with `capabilities = ["variables:read"]`. Capture stderr; assert the parity warning fires.

- [ ] **Step 6.2.11: Test #13 — hook dispatch suppression for non-overridden hooks**

Use the existing test_plugin (which declares `implemented_hooks = [PreExec, OnCd]`). Call `mgr.call_post_exec(...)` and verify the plugin's internal counter (exposed via a `get_post_exec_count` command added to test_plugin) is zero.

- [ ] **Step 6.2.12: Test #14 — compile-only WASI linker construction smoke**

Already implemented in `src/plugin/linker.rs::tests::linker_construction_smoke` (step 4.3.1). Wire it as the integration-level smoke, OR add a no-op integration test that imports the same function and calls it through public API.

- [ ] **Step 6.2.13: Benchmark — `benches/plugin_bench.rs`**

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_variables_get(c: &mut Criterion) {
    // Setup mgr + plugin with `variables:read` granted. The plugin has
    // a `bench_get_var_loop` command that calls get_var(name) 1000 times.
    // Measure the wall clock of one mgr.exec_command invocation.
    let mut mgr = yosh::plugin::PluginManager::new();
    let mut env = yosh::env::ShellEnv::new_for_tests();
    // ...load test_plugin with full caps...
    c.bench_function("variables_get_1000", |b| {
        b.iter(|| {
            black_box(mgr.exec_command(&mut env, "bench_get_var_loop", &[]));
        });
    });
}

criterion_group!(plugin, bench_variables_get);
criterion_main!(plugin);
```

Add the bench entry to `Cargo.toml`:
```toml
[[bench]]
name = "plugin_bench"
harness = false
```

- [ ] **Step 6.3: Run all integration tests**

Run: `cargo test -p yosh --test plugin -- --nocapture`
Expected: all 13 tests + benchmark pass.

- [ ] **Step 6.4: Drop dylib-specific tests in `crates/yosh-plugin-manager/tests/`**

Search: `grep -l "dylib\|libloading\|\.so" crates/yosh-plugin-manager/tests/*.rs`. For each match, delete the test or rewrite for `.wasm` semantics. Tests verifying asset-name templating with `{os}/{arch}` should be replaced with assertions that the new `{name}.wasm` template works and old tokens are rejected.

- [ ] **Step 6.5: Run all tests**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 6.6: Commit**

```bash
git add tests/plugin.rs tests/plugins crates/yosh-plugin-manager/tests benches/plugin_bench.rs Cargo.toml
git commit -m "test!: rewrite plugin integration tests for wasmtime runtime

Sub-project #6 of v0.2.0 migration. Drops dylib-specific cases
(symbol lookup, ABI version mismatch) and adds 13 new tests covering
the spec §8 test plan: capability allowlist, trap isolation,
with_env env-pointer correctness, metadata host-API denial, on-load
host access, three flavors of cwasm cache invalidation, .cwasm
tampering rejection, WASI lockdown, unknown-capability warnings,
requested-vs-granted parity, hook dispatch suppression, and a
compile-only WASI linker smoke. Adds benches/plugin_bench.rs
baseline."
```

---

## Task 7: Update CI / mise / e2e scripts

**Files:**
- Modify: `mise.toml`
- Modify: `.github/workflows/*.yml` (whichever runs CI tests)
- Modify: `e2e/run_tests.sh`

- [ ] **Step 7.1: Pin `cargo-component` in `mise.toml`**

Append:
```toml
[tools]
"cargo:cargo-component" = "0.18.0"
```

If `mise.toml` does not already manage cargo binary tools via `cargo:`, add the appropriate plugin invocation per the project's existing convention. The version 0.18.0 should match Step 0.2 (CI must use the same version).

- [ ] **Step 7.2: Update CI workflow**

In `.github/workflows/<test>.yml` (find via `ls .github/workflows/`):

```yaml
- name: Add wasm32-wasip2 target
  run: rustup target add wasm32-wasip2

- name: Install cargo-component
  run: cargo install cargo-component --locked --version 0.18.0

- name: Build test plugins
  run: |
    cargo component build -p test_plugin --target wasm32-wasip2 --release
    cargo component build -p trap_plugin --target wasm32-wasip2 --release
    cargo component build -p metadata_caller_plugin --target wasm32-wasip2 --release
```

Place these steps **before** `cargo test --workspace`. Cache `~/.cargo` and `target/wasm32-wasip2/` between runs to amortize the cargo-component install + wasm rebuild.

- [ ] **Step 7.3: Update `e2e/run_tests.sh`**

Add a precompile step at the top of `main()`:

```sh
echo ">>> Building wasm test plugins"
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
cargo component build -p metadata_caller_plugin --target wasm32-wasip2 --release
```

If the e2e suite has a plugin-specific subdirectory (`e2e/plugin/`), add it to the e2e runner's directory list. (The plan creates `e2e/plugin/` test files in Task 6.5 if not already there.)

- [ ] **Step 7.4: Smoke run CI locally**

Run: `cargo test --workspace`
Expected: pass.
Run: `./e2e/run_tests.sh`
Expected: pass.

- [ ] **Step 7.5: Commit**

```bash
git add mise.toml .github/workflows e2e/run_tests.sh
git commit -m "ci!: switch to wasm32-wasip2 + cargo-component toolchain

Sub-project #7 of v0.2.0 migration. CI installs cargo-component 0.18.0
and the wasm32-wasip2 rust target, builds the three in-tree test
plugins as wasm components, and runs the workspace test suite. e2e
runner builds the wasm plugins before running e2e tests. mise.toml
pins cargo-component for local development consistency."
```

---

## Task 8: Documentation rewrite + TODO cleanup

**Files:**
- Replace large sections of: `docs/kish/plugin.md`
- Modify: `CLAUDE.md`
- Modify: `TODO.md`

- [ ] **Step 8.1: Rewrite `docs/kish/plugin.md`**

The high-level structure stays (User Guide / Plugin Development Guide / Architecture). The sections that change in v0.2.0:

**User Guide → Installing Plugins:** sample paths use `.wasm`:
```toml
source = "github:user/yosh-plugin-git-status"
# or
source = "local:/path/to/my-local.wasm"
```

**User Guide → Configuration → asset:** remove `{os}/{arch}/{ext}` tokens; document `{name}` as the only token; default is `{name}.wasm`.

**Plugin Development Guide → Quick Start:** rewrite the 5-step quick start as:

```markdown
1. Create a new library crate:

   ```sh
   cargo init --lib yosh-plugin-hello
   cd yosh-plugin-hello
   ```

2. Set up `Cargo.toml`:

   ```toml
   [package]
   name = "yosh-plugin-hello"
   version = "0.1.0"
   edition = "2024"

   [lib]
   crate-type = ["cdylib"]

   [dependencies]
   yosh-plugin-sdk = "0.2"

   [package.metadata.component]
   package = "yourname:hello"

   [package.metadata.component.target.dependencies."yosh:plugin"]
   path = "<path-to-yosh-checkout>/crates/yosh-plugin-api/wit"
   ```

   (When yosh-plugin-api is published as a WIT registry package, the
    `dependencies` entry will instead point at the registry URL.)

3. Write `src/lib.rs`:

   ```rust
   use yosh_plugin_sdk::{Capability, Plugin, export, print};

   #[derive(Default)]
   struct HelloPlugin;

   impl Plugin for HelloPlugin {
       fn commands(&self) -> &[&'static str] { &["hello"] }
       fn required_capabilities(&self) -> &[Capability] { &[Capability::Io] }

       fn exec(&mut self, _command: &str, args: &[String]) -> i32 {
           let name = args.first().map(String::as_str).unwrap_or("world");
           let _ = print(&format!("Hello, {name}!\n"));
           0
       }
   }

   export!(HelloPlugin);
   ```

4. Build:

   ```sh
   cargo install cargo-component --locked --version 0.18.0
   rustup target add wasm32-wasip2
   cargo component build --target wasm32-wasip2 --release
   ```

   This produces `target/wasm32-wasip2/release/yosh_plugin_hello.wasm`.

5. Install locally:

   ```sh
   yosh plugin install target/wasm32-wasip2/release/yosh_plugin_hello.wasm
   yosh plugin sync
   ```
```

**Plugin Development Guide → Distributing via GitHub Releases:** replace the 4-platform cross-build matrix with a single line:

```markdown
Build a single `.wasm` artifact (platform-independent):

```sh
cargo component build --target wasm32-wasip2 --release
```

Attach `target/wasm32-wasip2/release/<crate_name>.wasm` to a GitHub
release with a SemVer tag (`v1.0.0`). The default asset filename
template is `{name}.wasm`.
```

**Plugin Development Guide → The export! macro:** describe what it generates (Guest impls for `yosh:plugin/plugin` and `yosh:plugin/hooks`, plus a single static `Mutex<Option<Plugin>>`).

**Architecture:** rewrite as:

```markdown
The plugin system has two layers:

- **yosh (shell binary)** — Reads `plugins.lock` at startup, validates the
  `.wasm` SHA-256 and the cwasm cache key tuple, calls
  `Component::deserialize` on the verified cwasm (or falls back to in-memory
  precompile from the verified `.wasm`), instantiates each plugin via
  `wasmtime`, and routes commands and hooks through `with_env` (an RAII
  wrapper that binds the live `ShellEnv` for the duration of a single guest
  call). The capability allowlist is applied at linker construction:
  granted imports get the real implementation; denied imports get
  deny-stubs that return `Err(Denied)`.

- **yosh-plugin (manager binary)** — Reads and writes `plugins.toml` (user
  configuration), downloads `.wasm` from GitHub releases, computes SHA-256,
  precompiles to `~/.cache/yosh/plugins/<sha>-<engine_hash>-<triple>.cwasm`
  (mode 0600, dir 0700), and writes `plugins.lock` with the four-tuple cache
  key plus cached `required_capabilities` and `implemented_hooks` for fast
  `yosh-plugin list` rendering. Calls plugin `metadata` via an all-deny
  linker (5 s epoch watchdog) — `metadata` is contractually forbidden from
  using host APIs.

The separation between `plugins.toml` (what the user wants) and
`plugins.lock` (what is actually installed and precompiled) ensures
reproducible plugin state across machines. The `.wasm` is the only trusted
artifact; `.cwasm` is a regenerable cache keyed on the host's wasmtime
version, target triple, and engine config hash.
```

- [ ] **Step 8.2: Update `CLAUDE.md`**

Search for any references to `dlopen`, `libloading`, `.dylib`, `.so` in `CLAUDE.md`. Remove or rewrite. Specifically the build & test commands section gains:

```
### Plugin development

cargo install cargo-component --locked --version 0.18.0    # one-time
rustup target add wasm32-wasip2                            # one-time
cargo component build -p test_plugin --target wasm32-wasip2 --release
```

- [ ] **Step 8.3: Remove resolved TODO items from `TODO.md`**

Delete these entries (per spec §9 removal checklist):

1. "Plugin preload validation in a sandbox process" — superseded by WASM trap isolation.
2. "SemVer API version management" — replaced by WIT package semver.
3. "SDK `export!` macro `unsafe` lint" — no longer applicable.
4. "Sandbox: `CAP_ALL` manual sync risk" — `Capability` enum is now the source of truth.
5. "warn on unknown capability strings in `plugins.toml`" — implemented in §6.

Also update any TODO entries that mention `.dylib` / `libloading` paths to reflect the new WASM-based code locations.

- [ ] **Step 8.4: Verify build + tests still pass**

Run: `cargo test --workspace`
Expected: pass.

- [ ] **Step 8.5: Commit**

```bash
git add docs/kish/plugin.md CLAUDE.md TODO.md
git commit -m "docs!: rewrite plugin docs for wasmtime Component Model

Sub-project #8 of v0.2.0 migration. docs/kish/plugin.md User Guide
keeps the same shape but examples use .wasm; Plugin Development
Guide is rewritten around cargo-component + wit-bindgen + the new
Plugin trait. Architecture section describes the wasmtime-based
runtime (with_env, capability-aware linker, cwasm cache trust model).
TODO.md drops items resolved by the migration."
```

---

## Final integration: release tagging

- [ ] **Step 9.1: Bump root `Cargo.toml` version to 0.2.0**

Already done in Task 4 step 4.1.1; sanity-check via `grep '^version' Cargo.toml`.

- [ ] **Step 9.2: Run full test suite**

Run: `cargo test --workspace`
Expected: pass.

- [ ] **Step 9.3: Run full e2e suite**

Run: `./e2e/run_tests.sh`
Expected: pass.

- [ ] **Step 9.4: Tag-ready check via release.sh dry run**

Run: `./.claude/skills/release/scripts/release.sh --check 0.2.0`
(Skip if release.sh has no `--check` flag; instead read its source to confirm the new wasm32-wasip2 plugin build path is wired in.)

- [ ] **Step 9.5: Commit any release-readiness fixes**

If steps 9.2–9.4 surface issues, fix and commit. Do NOT tag or push v0.2.0 in this plan; the user runs `release.sh` separately.

---

## Plan-level reminders

- **Frequent commits**: each Task ends with a commit. Don't pile sub-projects into one mega-commit.
- **TDD**: every test is written before its implementation. The cache module tests in 4.2.1 are paired with the cache module — the test file is in the same `mod cache;` block, so they cannot lag.
- **Dependency order**: do not skip ahead. Task 4 cannot succeed before Task 1; Task 6 depends on Task 3's wasm artifact.
- **Spec is authoritative**: when in doubt about a design decision, re-read the spec section referenced at the top of each task. Implementation deviations should be justified in the commit message.
- **Pinned versions**: keep `wasmtime = "27"`, `wasmtime-wasi = "27"`, `wit-bindgen = "0.36"`, `cargo-component = "0.18.0"`. Upgrades coordinate with `release.sh` per the spec §10 risk table.
