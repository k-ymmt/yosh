# Plugins

kish supports plugins as shared libraries (`.dylib` on macOS, `.so` on Linux) loaded at shell startup. Plugins can add custom commands and hook into shell events such as command execution, directory changes, and prompt display.

Plugins communicate with kish through a stable C ABI boundary, with a safe Rust SDK (`kish-plugin-sdk`) that hides all `unsafe` details from plugin authors.

## User Guide

### Installing Plugins

Use `kish plugin install` to register a plugin in your configuration:

```sh
# From GitHub (downloads the latest release)
kish plugin install https://github.com/user/kish-plugin-git-status

# From GitHub (pinned version)
kish plugin install https://github.com/user/kish-plugin-git-status@1.2.0

# From a local file
kish plugin install /path/to/libmy_plugin.dylib
```

After installing from GitHub, download the binary:

```sh
kish plugin sync
```

Local plugins are ready immediately after `sync`.

### Syncing Plugins

`kish plugin sync` reads `plugins.toml`, downloads any missing GitHub plugin binaries, computes SHA-256 checksums, and writes the lock file (`plugins.lock`). kish loads plugins from the lock file at startup.

```sh
kish plugin sync           # Download and verify all plugins
kish plugin sync --prune   # Also remove binaries for plugins no longer in config
```

### Updating Plugins

```sh
kish plugin update              # Update all GitHub plugins to latest version
kish plugin update git-status   # Update a specific plugin
```

This checks GitHub for the latest release, updates `plugins.toml`, and runs `sync` automatically.

### Listing and Verifying

```sh
kish plugin list     # Show installed plugins with version and checksum status
kish plugin verify   # Verify SHA-256 checksums of all plugin binaries
```

### Configuration

Plugin configuration lives in `~/.config/kish/plugins.toml`:

```toml
[[plugin]]
name = "git-status"
source = "github:user/kish-plugin-git-status"
version = "1.2.0"
enabled = true

[[plugin]]
name = "my-local-plugin"
source = "local:/path/to/libmy_plugin.dylib"
enabled = true
```

#### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Plugin name (alphanumeric, hyphens, underscores) |
| `source` | Yes | `github:owner/repo` or `local:/path/to/lib` |
| `version` | GitHub only | SemVer version string |
| `enabled` | No | `true` (default) or `false` to disable without removing |
| `capabilities` | No | List of permitted capabilities (default: all requested) |
| `asset` | No | Custom asset template for GitHub downloads |

#### Restricting Capabilities

By default, a plugin receives all capabilities it requests. You can restrict a plugin to a subset:

```toml
[[plugin]]
name = "untrusted-plugin"
source = "github:someone/kish-plugin-untrusted"
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

If a plugin calls a denied capability, kish prints an error to stderr and the call returns a failure code. There is no runtime overhead for permitted capabilities.

#### Custom Asset Templates

For GitHub plugins, the default asset filename template is:

```
lib{name}-{os}-{arch}.{ext}
```

Where `{name}` is the plugin name (hyphens replaced with underscores), `{os}` is `macos` or `linux`, `{arch}` is `x86_64` or `aarch64`, and `{ext}` is `dylib` or `so`.

Override with a custom template:

```toml
[[plugin]]
name = "my-plugin"
source = "github:user/kish-plugin-my-plugin"
version = "1.0.0"
asset = "kish_{name}-{os}-{arch}.{ext}"
```

## Plugin Development Guide

### Quick Start

1. Create a new library crate:

```sh
cargo init --lib kish-plugin-hello
cd kish-plugin-hello
```

2. Set the crate type and add the SDK dependency in `Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
kish-plugin-sdk = { git = "https://github.com/k-ymmt/kish", path = "crates/kish-plugin-sdk" }
```

3. Implement the plugin in `src/lib.rs`:

```rust
use kish_plugin_sdk::{Plugin, PluginApi, Capability, export};

#[derive(Default)]
struct HelloPlugin;

impl Plugin for HelloPlugin {
    fn commands(&self) -> &[&str] {
        &["hello"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[Capability::Io]
    }

    fn exec(&mut self, api: &PluginApi, _command: &str, args: &[&str]) -> i32 {
        let name = args.first().unwrap_or(&"world");
        api.print(&format!("Hello, {name}!\n"));
        0
    }
}

export!(HelloPlugin);
```

4. Build and install locally:

```sh
cargo build --release
kish plugin install target/release/libkish_plugin_hello.dylib
kish plugin sync
```

5. Restart kish and use your plugin:

```sh
$ hello
Hello, world!
$ hello kish
Hello, kish!
```

### The Plugin Trait

The `Plugin` trait defines the interface between kish and your plugin:

```rust
pub trait Plugin: Send + Default {
    /// Command names this plugin provides. (required)
    fn commands(&self) -> &[&str];

    /// Capabilities this plugin requires. (default: none)
    fn required_capabilities(&self) -> &[Capability] { &[] }

    /// Called when the plugin is loaded. Return Err to abort. (optional)
    fn on_load(&mut self, api: &PluginApi) -> Result<(), String> { Ok(()) }

    /// Execute a command. Returns exit status. (required)
    fn exec(&mut self, api: &PluginApi, command: &str, args: &[&str]) -> i32;

    /// Called before each command execution. (optional)
    fn hook_pre_exec(&mut self, api: &PluginApi, cmd: &str) {}

    /// Called after each command execution. (optional)
    fn hook_post_exec(&mut self, api: &PluginApi, cmd: &str, exit_code: i32) {}

    /// Called when the working directory changes. (optional)
    fn hook_on_cd(&mut self, api: &PluginApi, old_dir: &str, new_dir: &str) {}

    /// Called before the interactive prompt is displayed. (optional)
    fn hook_pre_prompt(&mut self, api: &PluginApi) {}

    /// Called when the plugin is about to be unloaded. (optional)
    fn on_unload(&mut self) {}
}
```

Your struct must implement `Default` (used by the `export!` macro to instantiate the plugin) and `Send` (plugins may be accessed from different threads).

### PluginApi Reference

The `PluginApi` provides safe access to shell internals. Each method maps to a capability:

#### Variables (`variables:read`, `variables:write`)

```rust
// Read a shell variable
let value: Option<String> = api.get_var("HOME");

// Set a shell variable
api.set_var("MY_VAR", "value")?;

// Set and export a variable (visible to child processes)
api.export_var("MY_VAR", "value")?;
```

#### Filesystem (`filesystem`)

```rust
// Get the current working directory
let cwd: String = api.cwd();

// Change the working directory
api.set_cwd("/tmp")?;
```

#### I/O (`io`)

```rust
// Write to stdout
api.print("output message\n");

// Write to stderr
api.eprint("error message\n");
```

### Hooks

Hooks let your plugin respond to shell events without the user explicitly invoking a command. Declare the corresponding capability and implement the hook method:

```rust
fn required_capabilities(&self) -> &[Capability] {
    &[
        Capability::Io,
        Capability::HookPrePrompt,
        Capability::HookOnCd,
    ]
}

fn hook_pre_prompt(&mut self, api: &PluginApi) {
    // Update prompt information before each prompt
    api.print(&format!("[{}] ", self.compute_status()));
}

fn hook_on_cd(&mut self, api: &PluginApi, _old_dir: &str, new_dir: &str) {
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

The SDK includes `kish_plugin_sdk::style` for ANSI terminal styling:

```rust
use kish_plugin_sdk::style::{Style, Color};

let styled = Style::new()
    .fg(Color::Green)
    .bold()
    .paint("success");
api.print(&format!("{styled}\n"));

// 256-color and RGB are also supported
let custom = Style::new().fg(Color::Rgb(255, 100, 0)).paint("orange");
```

### The export! Macro

The `export!` macro generates all required C ABI entry points for your plugin. Place it at the top level of your crate:

```rust
export!(MyPlugin);
```

This generates the following exported symbols automatically:
- `kish_plugin_decl` - Returns plugin metadata (name, version, API version, capabilities)
- `kish_plugin_init` - Initializes the plugin instance
- `kish_plugin_commands` - Returns the list of commands
- `kish_plugin_exec` - Dispatches command execution
- `kish_plugin_hook_*` - Hook entry points
- `kish_plugin_destroy` - Cleanup

The plugin name and version are read from your `Cargo.toml` at compile time.

### Distributing via GitHub Releases

To distribute your plugin through `kish plugin install`:

1. Build shared libraries for each target platform:

```sh
# macOS (Apple Silicon)
cargo build --release --target aarch64-apple-darwin

# macOS (Intel)
cargo build --release --target x86_64-apple-darwin

# Linux (x86_64)
cargo build --release --target x86_64-unknown-linux-gnu

# Linux (ARM64)
cargo build --release --target aarch64-unknown-linux-gnu
```

2. Name the release assets following the default template:

```
lib{name}-{os}-{arch}.{ext}
```

For example, a plugin crate named `kish-plugin-git-status`:

```
libkish_plugin_git_status-macos-aarch64.dylib
libkish_plugin_git_status-macos-x86_64.dylib
libkish_plugin_git_status-linux-x86_64.so
libkish_plugin_git_status-linux-aarch64.so
```

Note: hyphens in the crate name are replaced with underscores in the library filename.

3. Create a GitHub release with a SemVer tag (e.g., `v1.0.0` or `1.0.0`) and attach the binaries as release assets.

Users can then install your plugin with:

```sh
kish plugin install https://github.com/yourname/kish-plugin-git-status
```

## Architecture

The plugin system has two layers:

- **kish (shell binary)** - Reads `plugins.lock` at startup, loads shared libraries via `dlopen`, dispatches commands and hooks to plugins, and enforces capability sandboxing at the API table level.

- **kish-plugin (manager binary)** - Reads and writes `plugins.toml` (user configuration), downloads binaries from GitHub releases, computes SHA-256 checksums, and generates `plugins.lock`.

The separation between `plugins.toml` (what the user wants) and `plugins.lock` (what is actually installed) ensures reproducible plugin state across machines.
