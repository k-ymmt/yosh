# Help Command Enhancement Design

## Overview

Enhance help (`--help`) and version (`--version`) support for both the kish shell binary and the kish-plugin manager binary.

## Approach

- **kish main**: Manual `--help` / `--version` handling with colored output via `owo-colors`
- **kish-plugin manager**: Full `clap` (derive) integration with automatic help generation
- **Build info**: `build.rs` in both crates embeds git commit hash and build date at compile time

## kish Main Binary

### Scope

Add `--help` and `--version` flags to the kish shell binary (`src/main.rs`).

- **No `-h` flag** for help — `-h` is reserved for POSIX `set -h` (hash functions)
- `--help` and `--version` are checked first in the argument match chain, before `-c`, `--parse`, subcommand delegation, and script execution

### Help Output

```
kish - A POSIX-compliant shell

Usage: kish [options] [file [argument...]]

Options:
  -c <command>    Read commands from command_string
  --parse <code>  Parse and dump AST (debug)
  --help          Show this help message
  --version       Show version information

Subcommands:
  plugin          Manage shell plugins (see 'kish plugin --help')
```

Note: `-s` and `-i` are not yet implemented and therefore excluded from the help text. They should be added when implemented.

### Version Output

```
kish 0.1.0 (abc1234 2026-04-14)
```

### Color Scheme

| Element | Color |
|---------|-------|
| Section names (`Usage:`, `Options:`, `Subcommands:`) | Yellow + bold |
| Flags/options (`--help`, `-c <command>`) | Green |
| Subcommand names (`plugin`) | Green |
| Description text | Default terminal color |

### Output Destination

- `--help`: stdout (normal exit, exit code 0)
- Usage errors: stderr

### Argument Priority in `main.rs`

```
args[1] == "--help"    -> show help -> exit 0
args[1] == "--version" -> show version -> exit 0
args[1] == "-c"        -> existing logic
args[1] == "--parse"   -> existing logic
subcommand delegation  -> existing logic
script execution       -> existing logic
```

## kish-plugin Manager Binary

### Scope

Replace manual argument parsing in `crates/kish-plugin-manager/src/main.rs` with `clap` derive macros.

### Dependencies

Add to `crates/kish-plugin-manager/Cargo.toml`:

```toml
clap = { version = "4", features = ["derive", "color"] }
```

### CLI Structure

```rust
#[derive(Parser)]
#[command(name = "kish-plugin", about = "Manage kish shell plugins")]
#[command(version, propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install plugins from plugins.toml
    Sync,
    /// Update installed plugins
    Update,
    /// List installed plugins
    List,
    /// Verify plugin integrity (SHA-256)
    Verify,
}
```

### Help Output

`kish plugin --help`:

```
Manage kish shell plugins

Usage: kish-plugin <COMMAND>

Commands:
  sync    Install plugins from plugins.toml
  update  Update installed plugins
  list    List installed plugins
  verify  Verify plugin integrity (SHA-256)

Options:
  -h, --help     Show this help message
  -V, --version  Show version information
```

Subcommand help (e.g. `kish plugin sync --help`):

```
Install plugins from plugins.toml

Usage: kish-plugin sync

Options:
  -h, --help  Show this help message
```

Both `-h` and `--help` are supported (no POSIX flag conflict in the plugin manager).

## Build Information Embedding

### Mechanism

Each binary gets a `build.rs` that runs at compile time:

1. Execute `git rev-parse --short HEAD` -> commit hash
2. Execute `git log -1 --format=%ci` -> commit date (extract YYYY-MM-DD)
3. Set `cargo:rustc-env=KISH_GIT_HASH=<hash>`
4. Set `cargo:rustc-env=KISH_BUILD_DATE=<date>`
5. Set `cargo:rerun-if-changed=.git/HEAD` for incremental rebuild

### Fallback

If git commands fail (CI shallow clone, tarball build), use `"unknown"` as fallback value.

### Usage

kish main:

```rust
format!("kish {} ({} {})",
    env!("CARGO_PKG_VERSION"),
    env!("KISH_GIT_HASH"),
    env!("KISH_BUILD_DATE"))
```

kish-plugin manager (passed to clap's `#[command(version = ...)]`):

```rust
fn build_version() -> &'static str {
    concat!(env!("CARGO_PKG_VERSION"), " (", env!("KISH_GIT_HASH"), " ", env!("KISH_BUILD_DATE"), ")")
}
```

### Duplicate `build.rs`

Both binaries have their own `build.rs` because Cargo's build scripts are per-crate. The script is short enough that sharing via a utility crate would be over-engineering.

## Color Output and Terminal Detection

### Dependency

Add `owo-colors` to kish main only. kish-plugin manager uses clap's built-in `color` feature.

### Color Disable Logic (priority order)

1. `NO_COLOR` env var is set -> disable colors (https://no-color.org/)
2. `CLICOLOR_FORCE` env var is set -> force enable colors
3. stdout is not a TTY (pipe, redirect) -> disable colors
4. Otherwise -> enable colors

## Files Changed

- `src/main.rs` — Add `--help` / `--version` handling
- `build.rs` (new) — Git info embedding for kish main
- `Cargo.toml` — Add `owo-colors` dependency
- `crates/kish-plugin-manager/src/main.rs` — Replace manual parsing with clap
- `crates/kish-plugin-manager/build.rs` (new) — Git info embedding for plugin manager
- `crates/kish-plugin-manager/Cargo.toml` — Add `clap` dependency
