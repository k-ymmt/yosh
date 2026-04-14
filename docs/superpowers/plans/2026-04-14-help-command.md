# Help Command Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add colored `--help` and `--version` support to both the kish shell binary and the kish-plugin manager binary.

**Architecture:** kish main gets manual `--help`/`--version` with `owo-colors` for colored output. kish-plugin manager replaces manual arg parsing with `clap` derive macros. Both binaries embed git commit hash and build date via `build.rs` scripts.

**Tech Stack:** Rust, `owo-colors` (kish main color output), `clap` 4 with `derive` + `color` features (kish-plugin manager)

**Spec:** `docs/superpowers/specs/2026-04-14-help-command-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `build.rs` | Create | Git info embedding for kish main binary |
| `Cargo.toml` | Modify | Add `owo-colors` dependency |
| `src/main.rs` | Modify | `--help` / `--version` handling with colored output |
| `tests/cli_help.rs` | Create | Integration tests for kish `--help` / `--version` |
| `crates/kish-plugin-manager/build.rs` | Create | Git info embedding for plugin manager binary |
| `crates/kish-plugin-manager/Cargo.toml` | Modify | Add `clap` dependency |
| `crates/kish-plugin-manager/src/main.rs` | Modify | Replace manual parsing with clap |
| `crates/kish-plugin-manager/tests/cli_help.rs` | Create | Integration tests for plugin manager `--help` / `--version` |

---

### Task 1: Build info embedding for kish main

**Files:**
- Create: `build.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Create `build.rs` for kish main**

Create `build.rs` at the workspace root (next to `Cargo.toml`):

```rust
use std::process::Command;

fn main() {
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let date = Command::new("git")
        .args(["log", "-1", "--format=%ci"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.get(..10).map(|d| d.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=KISH_GIT_HASH={}", hash);
    println!("cargo:rustc-env=KISH_BUILD_DATE={}", date);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
```

- [ ] **Step 2: Verify build succeeds**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds without errors.

- [ ] **Step 3: Commit**

```bash
git add build.rs
git commit -m "build: add build.rs for git info embedding in kish main"
```

---

### Task 2: Colored help output for kish main

**Files:**
- Modify: `Cargo.toml` (add `owo-colors`)
- Modify: `src/main.rs`

- [ ] **Step 1: Write integration test for `--help`**

Create `tests/cli_help.rs`:

```rust
use std::process::Command;

fn kish_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kish"))
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    let output = kish_bin().arg("--help").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kish - A POSIX-compliant shell"), "should contain description");
    assert!(stdout.contains("Usage:"), "should contain Usage section");
    assert!(stdout.contains("Options:"), "should contain Options section");
    assert!(stdout.contains("--help"), "should list --help option");
    assert!(stdout.contains("--version"), "should list --version option");
    assert!(stdout.contains("-c <command>"), "should list -c option");
    assert!(stdout.contains("plugin"), "should list plugin subcommand");
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    let output = kish_bin().arg("--version").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("kish "), "should start with 'kish '");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "should contain package version");
    // Version format: kish 0.1.0 (hash date)
    assert!(stdout.contains('('), "should contain build info in parens");
}

#[test]
fn help_output_goes_to_stdout() {
    let output = kish_bin().arg("--help").output().unwrap();
    assert!(!output.stdout.is_empty(), "stdout should have content");
    assert!(output.stderr.is_empty(), "stderr should be empty");
}

#[test]
fn help_no_color_when_env_set() {
    let output = kish_bin()
        .arg("--help")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // ANSI escape sequences start with \x1b[
    assert!(!stdout.contains('\x1b'), "should not contain ANSI escapes when NO_COLOR is set");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli_help 2>&1`
Expected: Compilation error — `--help` is not handled yet so tests won't even match expected output.

- [ ] **Step 3: Add `owo-colors` dependency**

In `Cargo.toml`, add to `[dependencies]`:

```toml
owo-colors = "4"
```

- [ ] **Step 4: Implement `--help` and `--version` in `src/main.rs`**

Add these imports at the top of `src/main.rs`:

```rust
use owo_colors::OwoColorize;
```

Add two functions before `fn main()`:

```rust
fn should_colorize() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some() {
        return true;
    }
    nix::unistd::isatty(std::io::stdout()).unwrap_or(false)
}

fn print_help() {
    let color = should_colorize();

    let header = "kish - A POSIX-compliant shell";
    if color {
        println!("{}", header.bold());
    } else {
        println!("{}", header);
    }
    println!();

    if color {
        println!("{}  kish [options] [file [argument...]]", "Usage:".yellow().bold());
    } else {
        println!("Usage:  kish [options] [file [argument...]]");
    }
    println!();

    if color {
        println!("{}", "Options:".yellow().bold());
        println!("  {}    Read commands from command_string", "-c <command>".green());
        println!("  {}  Parse and dump AST (debug)", "--parse <code>".green());
        println!("  {}          Show this help message", "--help".green());
        println!("  {}       Show version information", "--version".green());
    } else {
        println!("Options:");
        println!("  -c <command>    Read commands from command_string");
        println!("  --parse <code>  Parse and dump AST (debug)");
        println!("  --help          Show this help message");
        println!("  --version       Show version information");
    }
    println!();

    if color {
        println!("{}", "Subcommands:".yellow().bold());
        println!("  {}          Manage shell plugins (see '{}')",
            "plugin".green(), "kish plugin --help".green());
    } else {
        println!("Subcommands:");
        println!("  plugin          Manage shell plugins (see 'kish plugin --help')");
    }
}

fn print_version() {
    println!("kish {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("KISH_GIT_HASH"),
        env!("KISH_BUILD_DATE"));
}
```

In `fn main()`, modify the `_ =>` arm to check `--help` and `--version` first. Replace the entire `_ =>` block:

```rust
        _ => {
            if args[1] == "--help" {
                print_help();
            } else if args[1] == "--version" {
                print_version();
            } else if args[1] == "-c" {
```

And close the block with the same existing logic. The full replacement of the `_ =>` arm in the `match args.len()` should become:

```rust
        _ => {
            if args[1] == "--help" {
                print_help();
            } else if args[1] == "--version" {
                print_version();
            } else if args[1] == "-c" {
                if args.len() < 3 {
                    eprintln!("kish: -c requires an argument");
                    process::exit(2);
                }
                let rest_start = if args.len() > 3 && args[3] == "--" { 4 } else { 3 };
                let sn = if rest_start < args.len() { args[rest_start].clone() } else { shell_name };
                let positional: Vec<String> = if rest_start + 1 < args.len() { args[rest_start + 1..].to_vec() } else { vec![] };
                let status = run_string(&args[2], sn, positional, true);
                process::exit(status);
            } else if args[1] == "--parse" {
                if args.len() < 3 {
                    eprintln!("kish: --parse requires an argument");
                    process::exit(2);
                }
                let input = if args[2] == "-" {
                    let mut buf = String::new();
                    io::stdin().read_to_string(&mut buf).unwrap();
                    buf
                } else {
                    args[2].clone()
                };
                match parser::Parser::new(&input).parse_program() {
                    Ok(ast) => println!("{:#?}", ast),
                    Err(e) => { eprintln!("{}", e); process::exit(2); }
                }
            } else if let Some(status) = try_subcommand(&args[1..]) {
                process::exit(status);
            } else {
                let positional: Vec<String> = args[2..].to_vec();
                let status = run_file(&args[1], shell_name, positional);
                process::exit(status);
            }
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test cli_help 2>&1`
Expected: All 4 tests pass.

- [ ] **Step 6: Manually verify colored output**

Run: `cargo run -- --help`
Expected: Colored help output with yellow bold section names and green flags.

Run: `NO_COLOR=1 cargo run -- --help`
Expected: Plain text without ANSI escapes.

Run: `cargo run -- --version`
Expected: Something like `kish 0.1.0 (abc1234 2026-04-14)`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/main.rs tests/cli_help.rs
git commit -m "feat: add --help and --version with colored output to kish main"
```

---

### Task 3: Build info embedding for kish-plugin manager

**Files:**
- Create: `crates/kish-plugin-manager/build.rs`

- [ ] **Step 1: Create `build.rs` for kish-plugin manager**

Create `crates/kish-plugin-manager/build.rs`:

```rust
use std::process::Command;

fn main() {
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let date = Command::new("git")
        .args(["log", "-1", "--format=%ci"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.get(..10).map(|d| d.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=KISH_GIT_HASH={}", hash);
    println!("cargo:rustc-env=KISH_BUILD_DATE={}", date);
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
}
```

Note: `rerun-if-changed` paths are relative to the crate root (`crates/kish-plugin-manager/`), so `.git` is at `../../.git`.

- [ ] **Step 2: Verify build succeeds**

Run: `cargo build -p kish-plugin-manager 2>&1 | tail -5`
Expected: Build succeeds without errors.

- [ ] **Step 3: Commit**

```bash
git add crates/kish-plugin-manager/build.rs
git commit -m "build: add build.rs for git info embedding in kish-plugin manager"
```

---

### Task 4: Replace kish-plugin manager arg parsing with clap

**Files:**
- Modify: `crates/kish-plugin-manager/Cargo.toml`
- Modify: `crates/kish-plugin-manager/src/main.rs`

- [ ] **Step 1: Write integration tests for kish-plugin help**

Create `crates/kish-plugin-manager/tests/cli_help.rs`:

```rust
use std::process::Command;

fn kish_plugin_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kish-plugin"))
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    let output = kish_plugin_bin().arg("--help").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kish shell plugins"), "should contain description");
    assert!(stdout.contains("sync"), "should list sync command");
    assert!(stdout.contains("update"), "should list update command");
    assert!(stdout.contains("list"), "should list list command");
    assert!(stdout.contains("verify"), "should list verify command");
}

#[test]
fn short_help_flag_works() {
    let output = kish_plugin_bin().arg("-h").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kish shell plugins"), "should contain description");
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    let output = kish_plugin_bin().arg("--version").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "should contain package version");
    assert!(stdout.contains('('), "should contain build info in parens");
}

#[test]
fn subcommand_help_works() {
    for subcmd in &["sync", "update", "list", "verify"] {
        let output = kish_plugin_bin()
            .args([subcmd, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{} --help should exit 0", subcmd);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"), "{} --help should contain Usage", subcmd);
    }
}

#[test]
fn no_args_shows_help_and_exits_error() {
    let output = kish_plugin_bin().output().unwrap();
    // clap exits with code 2 when no subcommand given
    assert!(!output.status.success(), "no args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage:") || stderr.contains("kish-plugin"),
        "should show usage hint on stderr"
    );
}

#[test]
fn unknown_command_exits_error() {
    let output = kish_plugin_bin().arg("bogus").output().unwrap();
    assert!(!output.status.success(), "unknown command should fail");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish-plugin-manager --test cli_help 2>&1`
Expected: Tests fail because current binary doesn't handle `--help` properly.

- [ ] **Step 3: Add `clap` dependency**

In `crates/kish-plugin-manager/Cargo.toml`, add to `[dependencies]`:

```toml
clap = { version = "4", features = ["derive", "color"] }
```

- [ ] **Step 4: Rewrite `main.rs` with clap**

Replace the entire contents of `crates/kish-plugin-manager/src/main.rs`:

```rust
use std::process;

use clap::{Parser, Subcommand};
use kish_plugin_manager::{config, github, lockfile, sync, verify};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("KISH_GIT_HASH"),
    " ",
    env!("KISH_BUILD_DATE"),
    ")"
);

#[derive(Parser)]
#[command(name = "kish-plugin", about = "Manage kish shell plugins")]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install plugins from plugins.toml
    Sync {
        /// Remove plugins not in plugins.toml
        #[arg(long)]
        prune: bool,
    },
    /// Update installed plugins to latest version
    Update {
        /// Only update the named plugin
        name: Option<String>,
    },
    /// List installed plugins
    List,
    /// Verify plugin integrity (SHA-256)
    Verify,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Sync { prune } => cmd_sync(prune),
        Commands::Update { name } => cmd_update(name.as_deref()),
        Commands::List => cmd_list(),
        Commands::Verify => cmd_verify(),
    };
    process::exit(code);
}

fn cmd_sync(prune: bool) -> i32 {
    let result = match sync::sync(prune) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    for name in &result.succeeded {
        eprintln!("  \u{2713} {}", name);
    }
    for (name, err) in &result.failed {
        eprintln!("  \u{2717} {}: {}", name, err);
    }

    if result.failed.is_empty() {
        eprintln!(
            "kish-plugin: sync complete ({} plugins)",
            result.succeeded.len()
        );
        0
    } else {
        eprintln!(
            "kish-plugin: sync partial ({} succeeded, {} failed)",
            result.succeeded.len(),
            result.failed.len()
        );
        1
    }
}

fn cmd_update(name_filter: Option<&str>) -> i32 {
    let config_path = sync::config_path();
    let decls = match config::load_config(&config_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    let client = github::GitHubClient::new();

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish-plugin: {}: {}", config_path.display(), e);
            return 2;
        }
    };
    let mut new_content = content.clone();
    let mut updated = false;

    for decl in &decls {
        if let Some(filter) = name_filter {
            if decl.name != filter {
                continue;
            }
        }
        if let config::PluginSource::GitHub { owner, repo } = &decl.source {
            match client.latest_version(owner, repo) {
                Ok(latest) => {
                    let current = decl.version.as_deref().unwrap_or("");
                    if latest != current {
                        eprintln!("  {} {} \u{2192} {}", decl.name, current, latest);
                        if !current.is_empty() {
                            new_content = new_content.replacen(
                                &format!("version = \"{}\"", current),
                                &format!("version = \"{}\"", latest),
                                1,
                            );
                        }
                        updated = true;
                    } else {
                        eprintln!("  {} {} (already latest)", decl.name, current);
                    }
                }
                Err(e) => {
                    eprintln!("  \u{2717} {}: {}", decl.name, e);
                }
            }
        }
    }

    if updated {
        if let Err(e) = std::fs::write(&config_path, &new_content) {
            eprintln!("kish-plugin: write {}: {}", config_path.display(), e);
            return 2;
        }
        return cmd_sync(false);
    }

    0
}

fn cmd_list() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    if lockfile.plugin.is_empty() {
        eprintln!("no plugins installed (run 'kish-plugin sync' first)");
        return 0;
    }

    for entry in &lockfile.plugin {
        let version = entry.version.as_deref().unwrap_or("-");
        let verified = match verify::verify_checksum(
            &config::expand_tilde_path(&entry.path),
            &entry.sha256,
        ) {
            Ok(true) => "\u{2713} verified",
            Ok(false) => "\u{2717} checksum mismatch",
            Err(_) => "\u{2717} file missing",
        };
        println!(
            "{:<16} {:<8} {:<48} {}",
            entry.name, version, entry.source, verified
        );
    }

    0
}

fn cmd_verify() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    let mut all_ok = true;
    for entry in &lockfile.plugin {
        let path = config::expand_tilde_path(&entry.path);
        match verify::verify_checksum(&path, &entry.sha256) {
            Ok(true) => {
                eprintln!("  \u{2713} {}", entry.name);
            }
            Ok(false) => {
                eprintln!("  \u{2717} {}: checksum mismatch", entry.name);
                all_ok = false;
            }
            Err(e) => {
                eprintln!("  \u{2717} {}: {}", entry.name, e);
                all_ok = false;
            }
        }
    }

    if all_ok { 0 } else { 1 }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kish-plugin-manager --test cli_help 2>&1`
Expected: All 6 tests pass.

- [ ] **Step 6: Run existing plugin manager tests**

Run: `cargo test -p kish-plugin-manager 2>&1`
Expected: All existing tests (including `sync_integration`) still pass.

- [ ] **Step 7: Manually verify help output**

Run: `cargo run --bin kish-plugin -- --help`
Expected: Colored help with commands listed.

Run: `cargo run --bin kish-plugin -- sync --help`
Expected: Sync subcommand help.

Run: `cargo run --bin kish-plugin -- --version`
Expected: Something like `kish-plugin 0.1.0 (abc1234 2026-04-14)`.

- [ ] **Step 8: Commit**

```bash
git add crates/kish-plugin-manager/Cargo.toml crates/kish-plugin-manager/src/main.rs crates/kish-plugin-manager/tests/cli_help.rs
git commit -m "feat(plugin-manager): replace manual arg parsing with clap for rich help"
```

---

### Task 5: Full test suite verification

- [ ] **Step 1: Run all workspace tests**

Run: `cargo test --workspace 2>&1`
Expected: All tests pass across all crates.

- [ ] **Step 2: Run E2E tests**

Run: `cargo build && ./e2e/run_tests.sh 2>&1 | tail -10`
Expected: E2E tests pass (no regressions from `--help`/`--version` changes).

- [ ] **Step 3: Verify `kish plugin --help` delegation works end-to-end**

Run: `cargo build && PATH="./target/debug:$PATH" ./target/debug/kish plugin --help`
Expected: The kish binary delegates to `kish-plugin` which shows clap-generated help.

- [ ] **Step 4: Final commit if any lockfile changes**

If `Cargo.lock` has changed:

```bash
git add Cargo.lock
git commit -m "chore: update Cargo.lock for owo-colors and clap dependencies"
```
