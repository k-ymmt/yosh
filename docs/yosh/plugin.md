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
| `allowed_commands` | No | Argv patterns the `commands:exec` capability may run (default: none) |
| `files_root` | No | Directory that confines `files:read`/`files:write` (default: unconfined) |
| `max_memory_mb` | No | Linear-memory cap in MiB (default 256, max 4096) |
| `hook_timeout_ms` | No | Budget for `pre_exec`/`post_exec`/`on_cd` hooks in ms; `0` = unlimited (default 5000) |
| `command_timeout_ms` | No | Budget for plugin commands in ms; `0` = unlimited (default) |
| `pre_prompt_timeout_ms` | No | Per-plugin `pre_prompt` budget in ms, 1–60000 (default 500; overrides `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`) |

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
| `files:read` | Read files and directories (see `files_root` below) |
| `files:write` | Create, modify, and delete files and directories (see `files_root` below) |
| `commands:exec` | Run external commands matching `allowed_commands` |

If a plugin calls a denied capability, yosh returns `Err(error-code::denied)` to the guest. There is no runtime overhead for permitted capabilities.

#### Confining `files` Access

**Without `files_root`, `files:read` / `files:write` grant access to the
entire filesystem** — everything the shell user can read or write, the
plugin can too. To confine a plugin to a directory:

```toml
[[plugin]]
name = "notes-plugin"
source = "github:someone/yosh-plugin-notes"
version = "0.1.0"
capabilities = ["files:read", "files:write"]
files_root = "~/notes"
```

With `files_root` set, every `files` host call is restricted to paths
inside that directory. Paths are canonicalized before the check, so
`..` traversal and symlinks pointing outside the root are denied.
Relative paths resolve against the root. `commands:exec` resolves
relative program names through the shell's `$PATH` and runs matching
commands with the shell's privileges; prefer absolute paths in
`allowed_commands` patterns when the plugin is untrusted.

#### Resource Limits

Every plugin runs under a linear-memory cap (`max_memory_mb`, default
256 MiB) and per-call time budgets. A plugin that exceeds a budget or
the memory cap is interrupted, disabled for the rest of the session,
and reported on stderr with the entry point and limit that tripped.
Hooks (`pre_exec`, `post_exec`, `on_cd`) default to a 5-second budget;
`pre_prompt` defaults to 500 ms; custom commands are unlimited by
default because users invoke them interactively — set
`command_timeout_ms` to bound them. Out-of-range values are clamped
with a warning at load time.

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

### Testing Locally

yosh ships two subcommands to exercise a plugin without starting a
shell session. Both run the plugin through the same `wasmtime` host
that yosh uses at runtime, but with an in-memory test backend instead
of a live `ShellEnv`. This works for plugins written in any language
that targets the WebAssembly Component Model.

#### One-shot: `yosh plugin run`

```sh
yosh plugin run target/wasm32-wasip2/release/yosh_plugin_hello.wasm \
    exec hello world
```

Flags scope what the plugin can see:

| Flag | Effect |
|------|--------|
| `--cap` | Capabilities to grant (defaults to the plugin's `required_capabilities`) |
| `--var KEY=VAL` | Seed a shell variable |
| `--export KEY=VAL` | Seed an exported variable |
| `--cwd <path>` | Virtual cwd |
| `--allow-exec <pat>` | Allowlist a `commands:exec` argv pattern (e.g. `--allow-exec 'git status:*'`) |
| `--sandbox-root <path>` | Real-FS scope for `files:read`/`files:write` (otherwise virtual) |
| `--timeout <ms>` | Watchdog deadline (default 5000) |
| `--max-memory-mb <N>` | Linear-memory cap for the plugin store in MiB (default 256) |
| `--watch` | Re-run the invocation whenever the wasm changes (300 ms mtime polling; Ctrl-C to stop) |
| `--format <human\|json>` | Output format |

Harness-level failures exit 99, but the surface differs by phase. Load
and metadata failures (compiling, instantiating, or extracting
`metadata()`) happen before any invocation and print a
`yosh-plugin: <kind>: <message>` line on stderr — plus a `hint:` line
when there is an obvious fix. With `--format json` the same object is
also emitted on stdout as `{"error":{"kind":...,"message":...,"hint":...}}`,
so CI never scrapes stderr. Trap, timeout, and memory failures happen
*during* the invocation and instead appear inside the run output: a
`[error]`/`[hint]` line pair in human output, or the outcome object's
`"error"` field in JSON — same `kind`/`message`/`hint` shape either
way. Capability denials are not errors (the plugin decides how to
react); they are listed in a `[denied]` section (JSON: `"denied"`
array) with per-capability remediation hints.

Set `YOSH_PLUGIN_TRACE=1` to trace every host-import call and runner
phase on stderr (`yosh-plugin[trace]: ...`).

Hooks are invoked similarly:

```sh
yosh plugin run my-plugin.wasm hook pre-exec "ls -l"
yosh plugin run my-plugin.wasm hook on-cd /old /new
yosh plugin run my-plugin.wasm hook pre-prompt
```

#### Declarative: `yosh plugin test`

Drop scenario files under `tests/` next to your plugin source. Each
`*.toml` is one scenario:

```toml
plugin = "../target/wasm32-wasip2/release/my_plugin.wasm"
description = "hello prints a greeting"

[env]
caps = ["io"]
timeout_ms = 5000
max_memory_mb = 256

[[step]]
call = "exec"
args = ["hello", "world"]

  [step.expect]
  exit = 0
  stdout = "Hello, world!\n"
```

Run them:

```sh
yosh plugin test                  # walks tests/
yosh plugin test --format json    # JSON-lines for CI
```

Supported `[step.expect]` keys: `exit`, `stdout`, `stderr`,
`stdout_contains`, `stderr_contains`, `stdout_regex`, `stderr_regex`,
`vars_set`, `vars_export`, `files_write`, `exec_called`, `trap`,
`denied`. `files_write = { "/path" = "bytes" }` compares the written
content (use `{ len = N }` to assert length only); `denied = true`
passes iff at least one host call was capability-denied during the
step. Failing scenarios in `--format json` carry structured `step`,
`check`, `expected`, and `got` fields alongside the freeform `reason`.

#### Example: CI integration

```yaml
- run: cargo install cargo-component --locked --version 0.18.0
- run: rustup target add wasm32-wasip2
- run: cargo component build --target wasm32-wasip2 --release
- run: yosh plugin test --format json | tee result.jsonl
```

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
