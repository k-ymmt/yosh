# Plugins

yosh supports plugins as WebAssembly Components (`.wasm`), loaded at shell startup via the [wasmtime](https://wasmtime.dev/) runtime. Plugins can add custom commands and hook into shell events such as command execution, directory changes, and prompt display.

Plugins communicate with yosh through a WIT-defined interface (`yosh:plugin`), with a safe Rust SDK (`yosh-plugin-sdk`) that hides all low-level bindings from plugin authors.

## User Guide

### Installing Plugins

Use `yosh plugin install` to register a plugin in your configuration:

```sh
# From GitHub (downloads the latest release)
yosh plugin install https://github.com/user/yosh-plugin-git-status

# From GitHub (pinned version)
yosh plugin install https://github.com/user/yosh-plugin-git-status@1.2.0

# From a local file
yosh plugin install /path/to/my_local.wasm
```

After installing from GitHub, download the binary:

```sh
yosh plugin sync
```

Local plugins are ready immediately after `sync`.

### Syncing Plugins

`yosh plugin sync` reads `plugins.toml`, downloads any missing GitHub plugin binaries, computes SHA-256 checksums, precompiles each `.wasm` to a cached `.cwasm`, and writes the lock file (`plugins.lock`). yosh loads plugins from the lock file at startup.

```sh
yosh plugin sync           # Download, precompile, and verify all plugins
yosh plugin sync --prune   # Also remove binaries for plugins no longer in config
```

### Updating Plugins

```sh
yosh plugin update              # Update all GitHub plugins to latest version
yosh plugin update git-status   # Update a specific plugin
```

This checks GitHub for the latest release, updates `plugins.toml`, and runs `sync` automatically.

### Listing and Verifying

```sh
yosh plugin list     # Show installed plugins with version and checksum status
yosh plugin verify   # Verify SHA-256 checksums of all plugin binaries
```

### Configuration

Plugin configuration lives in `~/.config/yosh/plugins.toml`:

```toml
[[plugin]]
name = "git-status"
source = "github:user/yosh-plugin-git-status"
version = "1.2.0"
enabled = true

[[plugin]]
name = "my-local"
source = "local:/path/to/my_local.wasm"
enabled = true
```

#### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Plugin name (alphanumeric, hyphens, underscores) |
| `source` | Yes | `github:owner/repo` or `local:/path/to/plugin.wasm` |
| `version` | GitHub only | SemVer version string |
| `enabled` | No | `true` (default) or `false` to disable without removing |
| `capabilities` | No | List of permitted capabilities (default: all requested) |
| `asset` | No | Custom asset filename for GitHub downloads |

#### Restricting Capabilities

By default, a plugin receives all capabilities it requests. You can restrict a plugin to a subset:

```toml
[[plugin]]
name = "untrusted-plugin"
source = "github:someone/yosh-plugin-untrusted"
version = "0.1.0"
capabilities = ["variables:read", "io"]
```

Available capability strings:

| Capability | Description |
|------------|-------------|
| `variables:read` | Read shell variables |
| `variables:write` | Set and export shell variables |
| `filesystem` | Read and change the working directory |
| `io` | Write to stdout and stderr |
| `hooks:pre_exec` | Run before each command |
| `hooks:post_exec` | Run after each command |
| `hooks:on_cd` | Run when the working directory changes |
| `hooks:pre_prompt` | Run before the prompt is displayed |

If a plugin calls a denied capability, yosh returns `Err(error-code::denied)` to the guest. There is no runtime overhead for permitted capabilities.

#### Asset Filename

For GitHub plugins, the default asset filename template is:

```
{name}.wasm
```

Where `{name}` is the plugin name. WebAssembly Components are platform-independent, so a single `.wasm` file serves all operating systems and architectures. Only `{name}` is supported as a template variable; `{os}`, `{arch}`, and `{ext}` are not available.

Override with a custom asset filename:

```toml
[[plugin]]
name = "my-plugin"
source = "github:user/yosh-plugin-my-plugin"
version = "1.0.0"
asset = "yosh_my_plugin.wasm"
```

## Plugin Development Guide

### Quick Start

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
   version = "0.2"

   [profile.release]
   opt-level = "s"
   lto = true
   strip = true
   panic = "abort"
   ```

   The `panic = "abort"` setting is required: it prevents Rust `std`'s
   panic-string formatting from pulling in `wasi:cli/stderr` at link time.

3. Set up `wkg` to resolve the `yosh:plugin` WIT package from
   [wa.dev]:

   ```sh
   cargo install wkg --locked
   wkg config --default-registry wa.dev
   ```

   `cargo component build` (step 5) invokes `wkg` automatically to
   fetch `yosh:plugin@<version>` on first build. This replaces the
   `path = "<yosh-checkout>/..."` form used by yosh's in-repo test
   plugins.

   [wa.dev]: https://wa.dev/

4. Write `src/lib.rs`:

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

5. Build:

   ```sh
   cargo install cargo-component --locked --version 0.18.0
   rustup target add wasm32-wasip2
   cargo component build --target wasm32-wasip2 --release
   ```

   This produces `target/wasm32-wasip2/release/yosh_plugin_hello.wasm`.

6. Install locally:

   ```sh
   yosh plugin install target/wasm32-wasip2/release/yosh_plugin_hello.wasm
   yosh plugin sync
   ```

### The Plugin Trait

The `Plugin` trait defines the interface between yosh and your plugin:

```rust
pub trait Plugin: Default {
    /// Command names this plugin provides. (required)
    fn commands(&self) -> &[&'static str];

    /// Capabilities this plugin requires. (default: none)
    fn required_capabilities(&self) -> &[Capability] { &[] }

    /// Hook names this plugin implements. Must be declared explicitly. (default: none)
    fn implemented_hooks(&self) -> &'static [HookName] { &[] }

    /// Called when the plugin is loaded. Return Err to abort. (optional)
    fn on_load(&mut self) -> Result<(), String> { Ok(()) }

    /// Execute a command. Returns exit status. (required)
    fn exec(&mut self, command: &str, args: &[String]) -> i32;

    /// Called before each command execution. (optional)
    fn hook_pre_exec(&mut self, cmd: &str) {}

    /// Called after each command execution. (optional)
    fn hook_post_exec(&mut self, cmd: &str, exit_code: i32) {}

    /// Called when the working directory changes. (optional)
    fn hook_on_cd(&mut self, old_dir: &str, new_dir: &str) {}

    /// Called before the interactive prompt is displayed. (optional)
    fn hook_pre_prompt(&mut self) {}

    /// Called when the plugin is about to be unloaded. (optional)
    fn on_unload(&mut self) {}
}
```

Your struct must implement `Default` (used by the `export!` macro to instantiate the plugin).

The `implemented_hooks()` method is the explicit declaration mechanism for hooks. yosh only dispatches a hook to your plugin if the hook name appears in the slice returned by `implemented_hooks()`. This avoids unnecessary guest calls for plugins that don't use hooks, and the declaration is also cached in `plugins.lock` for fast startup filtering.

### Plugin API Reference

All host functions are free functions imported from `yosh_plugin_sdk`. Each maps to a capability:

#### Variables (`variables:read`, `variables:write`)

```rust
// Read a shell variable
let value: Result<Option<String>, ErrorCode> = get_var("HOME");

// Set a shell variable
set_var("MY_VAR", "value")?;

// Set and export a variable (visible to child processes)
export_env("MY_VAR", "value")?;
```

#### Filesystem (`filesystem`)

```rust
// Get the current working directory
let cwd: Result<String, ErrorCode> = cwd();
```

#### I/O (`io`)

```rust
// Write to stdout
print("output message\n")?;

// Write to stderr
eprint("error message\n")?;
```

### Hooks

Hooks let your plugin respond to shell events without the user explicitly invoking a command. Declare the corresponding capability, implement the hook method, **and** list the hook in `implemented_hooks()`:

```rust
fn required_capabilities(&self) -> &[Capability] {
    &[
        Capability::Io,
        Capability::HookPrePrompt,
        Capability::HookOnCd,
    ]
}

fn implemented_hooks(&self) -> &'static [HookName] {
    &[HookName::PrePrompt, HookName::OnCd]
}

fn hook_pre_prompt(&mut self) {
    // Update prompt information before each prompt
    let _ = print(&format!("[{}] ", self.compute_status()));
}

fn hook_on_cd(&mut self, _old_dir: &str, new_dir: &str) {
    // React to directory changes
    self.scan_directory(new_dir);
}
```

| Hook | Trigger | Arguments |
|------|---------|-----------|
| `hook_pre_exec` | Before each command | Command string |
| `hook_post_exec` | After each command | Command string, exit code |
| `hook_on_cd` | Directory change | Old path, new path |
| `hook_pre_prompt` | Before prompt display | None |

### Style Utilities

The SDK includes `yosh_plugin_sdk::style` for ANSI terminal styling:

```rust
use yosh_plugin_sdk::style::{Style, Color};

let styled = Style::new()
    .fg(Color::Green)
    .bold()
    .paint("success");
let _ = print(&format!("{styled}\n"));

// 256-color and RGB are also supported
let custom = Style::new().fg(Color::Rgb(255, 100, 0)).paint("orange");
```

### The export! Macro

The `export!` macro bridges your `Plugin` implementation into the WIT-generated guest bindings. Place it at the top level of your crate:

```rust
export!(MyPlugin);
```

This generates all required WIT guest exports automatically, including `metadata`, `exec`, and each hook entry point. There is no `unsafe extern "C" fn` and no `#[no_mangle]` — everything is handled through the Component Model ABI produced by `wit-bindgen`.

The plugin name and version are read from your `Cargo.toml` at compile time via `env!("CARGO_PKG_NAME")` and `env!("CARGO_PKG_VERSION")`.

### Distributing via GitHub Releases

WebAssembly Components are platform-independent — build once, ship once:

```sh
cargo component build --target wasm32-wasip2 --release
```

Attach `target/wasm32-wasip2/release/<crate_name>.wasm` to a GitHub release with a SemVer tag (`v1.0.0` or `1.0.0`). The default asset filename template is `{name}.wasm`.

Users install with:

```sh
yosh plugin install https://github.com/yourname/yosh-plugin-hello
yosh plugin sync
```

## Architecture

The plugin system has two layers:

- **yosh (shell binary)** — Reads `plugins.lock` at startup, validates the
  `.wasm` SHA-256 and the cwasm cache key tuple, instantiates each plugin
  via `wasmtime` (with the granted-capability host import set), and routes
  commands and hooks through `with_env` (an RAII wrapper that binds the
  live `ShellEnv` for the duration of a single guest call). Capability
  allowlists are applied at linker construction: granted imports get the
  real implementation; denied imports get deny-stubs that return
  `Err(error-code::denied)`. Hooks dispatch is filtered both by capability
  and by `plugin-info.implemented-hooks` (declared by the plugin author).

- **yosh-plugin (manager binary)** — Reads and writes `plugins.toml` (user
  configuration), downloads `.wasm` from GitHub releases, computes SHA-256,
  precompiles to `~/.yosh/plugins/<name>/<basename>.cwasm` (mode 0600,
  parent dir 0700), and writes `plugins.lock` with a four-tuple cache key
  `(wasm_sha256, wasmtime_version, target_triple, engine_config_hash)`
  plus cached `required_capabilities` and `implemented_hooks` for fast
  `yosh-plugin list`. Calls each plugin's `metadata` once per sync via an
  all-deny linker (5-second epoch watchdog) — `metadata` is contractually
  forbidden from using host APIs.

The separation between `plugins.toml` (what the user wants) and
`plugins.lock` (what is actually installed and precompiled) ensures
reproducible plugin state across machines. The `.wasm` is the only
trusted artifact; `.cwasm` is a regenerable cache validated at every shell
startup against five conditions: same-uid ownership, file mode 0600, dir
mode 0700, cache key tuple match, and source `.wasm` SHA-256 match.
