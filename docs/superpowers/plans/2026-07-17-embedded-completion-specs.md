# Embedded Completion Specs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the bundled completion specs in `completions/*.toml` work out of the box after `cargo install yosh`, by embedding them in the binary as a fallback layer, with an explicit `yosh completions export` command for users who want to customize a default spec.

**Architecture:** `build.rs` generates a `static EMBEDDED_SPECS: &[(&str, &str)]` array (name → TOML text) from `completions/*.toml` at compile time. `SpecStore` gains a layered lookup: a user file at `~/.config/yosh/completions/<cmd>.toml` always wins; if no file exists, the embedded spec is used. A new `yosh-completions` binary (dispatched as `yosh completions` via the existing git-style `try_subcommand` delegation in `src/main.rs`) provides `list` and `export`.

**Tech Stack:** Rust (edition 2024), no new dependencies. Code generation via the existing `build.rs`. Integration tests via `CARGO_BIN_EXE_*` like `tests/plugin_cli_help.rs`.

## Global Constraints

- No new crate dependencies (embedding uses `build.rs` + `include_str!`, not `include_dir`).
- Error messages from the shell are prefixed `yosh: `; messages from the new binary are prefixed `yosh-completions: ` (matching the `yosh-plugin` delegation pattern).
- Exit codes: 0 success, 1 general error, 2 usage error.
- A user file on disk is ALWAYS authoritative: a broken (unparseable) user file produces a warning and NO completion — it must never silently fall back to the embedded spec.
- `SpecStore::new(dir)` keeps its current disk-only behavior (existing tests in `tests/interactive.rs` construct it with temp dirs and must not start seeing embedded specs).
- Every commit message ends with the standard Claude Code trailer and includes the task context line: `Prompt: completions をインストール時に自動で使えるように（埋め込みフォールバック方式）`.
- Run targeted tests during tasks; run the full `cargo test` suite once at the end (it takes minutes — run it in the background, never with a short timeout).

---

### Task 1: Embed completion specs at compile time

**Files:**
- Modify: `build.rs` (add generation function; call it from `main()`)
- Modify: `src/interactive/spec_completion.rs` (include the generated file; add tests)

**Interfaces:**
- Consumes: `completions/*.toml` (36 files at repo root), `CompletionSpec::parse(&str) -> Result<CompletionSpec, String>` (already exists in `spec_completion.rs`)
- Produces: `pub static EMBEDDED_SPECS: &[(&'static str, &'static str)]` in module `yosh::interactive::spec_completion` — sorted by name, name is the file stem (e.g. `"git"`, `"["`), value is the raw TOML text.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `src/interactive/spec_completion.rs`, right after the existing `bundled_completion_specs_parse` test:

```rust
    #[test]
    fn embedded_specs_parse_and_include_git() {
        assert!(!EMBEDDED_SPECS.is_empty());
        assert!(EMBEDDED_SPECS.iter().any(|(name, _)| *name == "git"));
        for (name, text) in EMBEDDED_SPECS {
            CompletionSpec::parse(text).unwrap_or_else(|err| panic!("embedded {name}: {err}"));
        }
    }

    #[test]
    fn embedded_specs_match_repo_dir() {
        // build.rs must pick up every spec file — a missing entry here
        // means the generator silently skipped one.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("completions");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
            .collect();
        on_disk.sort();
        let embedded: Vec<String> = EMBEDDED_SPECS
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        assert_eq!(embedded, on_disk, "EMBEDDED_SPECS out of sync with completions/");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib embedded_specs 2>&1 | tail -20`
Expected: COMPILE ERROR — `EMBEDDED_SPECS` not found.

- [ ] **Step 3: Add the generator to build.rs**

In `build.rs`, add a call at the end of `main()`:

```rust
    generate_embedded_completions();
```

and append the function:

```rust
/// Generate `$OUT_DIR/embedded_completions.rs`: a static array embedding
/// every `completions/*.toml` so specs work without any user setup.
/// `spec_completion.rs` pulls it in with `include!`.
fn generate_embedded_completions() {
    println!("cargo:rerun-if-changed=completions");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = Path::new(&manifest_dir).join("completions");
    let mut entries: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("completions/ must exist")
        .map(|e| e.expect("readable dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .map(|p| {
            let name = p
                .file_stem()
                .expect("spec file has a stem")
                .to_str()
                .expect("spec file name is UTF-8")
                .to_string();
            (name, p.display().to_string())
        })
        .collect();
    entries.sort();

    let mut code = String::from(
        "/// Completion specs compiled in from `completions/*.toml`, sorted by name.\n\
         /// Used as the fallback layer when no user spec file exists.\n\
         pub static EMBEDDED_SPECS: &[(&str, &str)] = &[\n",
    );
    for (name, path) in &entries {
        code.push_str(&format!("    ({name:?}, include_str!({path:?})),\n"));
    }
    code.push_str("];\n");

    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("embedded_completions.rs");
    std::fs::write(&out, code).expect("write embedded_completions.rs");
}
```

- [ ] **Step 4: Include the generated file in spec_completion.rs**

In `src/interactive/spec_completion.rs`, after the `use serde::Deserialize;` line, add:

```rust
// ── Embedded specs (generated by build.rs from completions/*.toml) ──

include!(concat!(env!("OUT_DIR"), "/embedded_completions.rs"));
```

Also update the module doc comment (lines 1–5) to describe the two layers:

```rust
//! Spec-based tab completion: per-command TOML definition files.
//!
//! Specs come from two layers: `~/.config/yosh/completions/<command>.toml`
//! (user files, always authoritative) with the specs embedded from the
//! repository's `completions/` directory as a fallback. Users drop a
//! `<command>.toml` file into the config directory to define or override
//! completion for any command.
//! See `completion.md` at the repository root for the full design.
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib embedded_specs 2>&1 | tail -5`
Expected: `test result: ok. 2 passed` (both new tests).

Also run the existing spec tests to check for regressions:
Run: `cargo test --lib spec_completion 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add build.rs src/interactive/spec_completion.rs
git commit -m "feat(completion): embed bundled specs in the binary at compile time

build.rs generates a static (name, toml) array from completions/*.toml
so specs can be served without any user setup. Not yet wired into
SpecStore lookup.

Prompt: completions をインストール時に自動で使えるように（埋め込みフォールバック方式）

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016uw49CRUMzUa3wggZ7HKav"
```

---

### Task 2: SpecStore layered lookup (disk first, embedded fallback)

**Files:**
- Modify: `src/interactive/spec_completion.rs:174-226` (SpecStore struct and impl; add tests)
- Modify: `src/interactive/mod.rs:134` (no code change needed if `from_home` gains the fallback — verify only)

**Interfaces:**
- Consumes: `EMBEDDED_SPECS` from Task 1.
- Produces: `SpecStore::with_embedded(dir: std::path::PathBuf) -> SpecStore` (disk-first, embedded-fallback store). `SpecStore::from_home(home: &str)` now returns a fallback-enabled store. `SpecStore::new(dir)` unchanged (disk-only).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `src/interactive/spec_completion.rs`, after the existing SpecStore tests:

```rust
    #[test]
    fn store_with_embedded_falls_back_when_no_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut store = SpecStore::with_embedded(tmp.path().to_path_buf());
        let spec = store.get("git").expect("embedded git spec");
        assert!(spec.subcommands.iter().any(|s| s.name == "log"));
    }

    #[test]
    fn store_disk_file_overrides_embedded() {
        let (_tmp, dir) = {
            let tmp = tempfile::TempDir::new().unwrap();
            let dir = tmp.path().to_path_buf();
            std::fs::write(dir.join("git.toml"), "[[subcommands]]\nname = \"only-mine\"\n")
                .unwrap();
            (tmp, dir)
        };
        let mut store = SpecStore::with_embedded(dir);
        let spec = store.get("git").unwrap();
        assert_eq!(spec.subcommands.len(), 1);
        assert_eq!(spec.subcommands[0].name, "only-mine");
    }

    #[test]
    fn store_broken_disk_file_does_not_fall_back() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("git.toml"), "not [ valid toml").unwrap();
        let mut store = SpecStore::with_embedded(tmp.path().to_path_buf());
        assert!(store.get("git").is_none());
    }

    #[test]
    fn store_plain_new_has_no_embedded_fallback() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut store = SpecStore::new(tmp.path().to_path_buf());
        assert!(store.get("git").is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib store_with_embedded 2>&1 | tail -10`
Expected: COMPILE ERROR — `with_embedded` not found.

- [ ] **Step 3: Implement the fallback in SpecStore**

In `src/interactive/spec_completion.rs`, change the struct and impl:

```rust
pub struct SpecStore {
    dir: std::path::PathBuf,
    cache: std::collections::HashMap<String, Option<CompletionSpec>>,
    exec_env: Option<Vec<(String, String)>>,
    use_embedded: bool,
}
```

In `impl SpecStore`, update `new` and `from_home`, and add `with_embedded`:

```rust
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            cache: std::collections::HashMap::new(),
            exec_env: None,
            use_embedded: false,
        }
    }

    /// Store that serves [`EMBEDDED_SPECS`] for commands with no spec
    /// file in `dir`. A file in `dir` always wins over the embedded
    /// spec, including a file that fails to parse.
    pub fn with_embedded(dir: std::path::PathBuf) -> Self {
        Self {
            use_embedded: true,
            ..Self::new(dir)
        }
    }

    /// Store rooted at the standard location under `home`
    /// (`~/.config/yosh/completions`), with embedded-spec fallback.
    pub fn from_home(home: &str) -> Self {
        Self::with_embedded(std::path::PathBuf::from(home).join(".config/yosh/completions"))
    }
```

Replace `load`:

```rust
    fn load(&self, name: &str) -> Option<CompletionSpec> {
        let path = self.dir.join(format!("{name}.toml"));
        match std::fs::read_to_string(&path) {
            Ok(text) => match CompletionSpec::parse(&text) {
                Ok(spec) => Some(spec),
                // A broken user file must not silently fall back to the
                // embedded spec — the warning tells the user which file
                // to fix, and stale defaults would mask their edits.
                Err(err) => {
                    eprintln!("yosh: completion: {name}.toml: {err}");
                    None
                }
            },
            Err(_) => self.load_embedded(name),
        }
    }

    fn load_embedded(&self, name: &str) -> Option<CompletionSpec> {
        if !self.use_embedded {
            return None;
        }
        let (_, text) = EMBEDDED_SPECS.iter().find(|(n, _)| *n == name)?;
        // Embedded specs are validated by unit tests; a parse failure
        // here means a build-toolchain bug, not a user error.
        CompletionSpec::parse(text).ok()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib spec_completion 2>&1 | tail -5`
Expected: all pass, including the 4 new tests.

- [ ] **Step 5: Verify the interactive shell wiring**

`src/interactive/mod.rs:134` calls `SpecStore::from_home(&home)`, which now has the fallback — no change needed. Confirm the integration tests still pass (they use `SpecStore::new` with temp dirs, so behavior is unchanged):

Run: `cargo test --test interactive 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/interactive/spec_completion.rs
git commit -m "feat(completion): serve embedded specs when no user file exists

SpecStore::from_home now layers lookup: a user file in
~/.config/yosh/completions always wins (even a broken one, which warns
and disables completion for that command); otherwise the spec embedded
at compile time is used. Completion now works out of the box after
cargo install with no manual copying.

Prompt: completions をインストール時に自動で使えるように（埋め込みフォールバック方式）

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016uw49CRUMzUa3wggZ7HKav"
```

---

### Task 3: `yosh completions` CLI (list / export)

**Files:**
- Create: `src/completions_cli.rs`
- Create: `src/bin/yosh-completions.rs`
- Modify: `src/lib.rs` (add `pub mod completions_cli;`)
- Modify: `src/main.rs` (add `mod completions_cli;` is NOT needed — the bin uses the lib; only the help text changes, done in Task 4)
- Test: `tests/completions_cli.rs`

**Interfaces:**
- Consumes: `yosh::interactive::spec_completion::EMBEDDED_SPECS` from Task 1.
- Produces: `yosh::completions_cli::run() -> i32` and the `yosh-completions` binary. `yosh completions <cmd>` works automatically after install via the existing `try_subcommand` PATH delegation in `src/main.rs:193` (same mechanism as `yosh plugin`).

CLI contract:
- `yosh-completions` (no args), `-h`, `--help` → usage text, exit 0
- `yosh-completions list` → one embedded spec name per line, exit 0
- `yosh-completions export [--force] <command>...` → writes each embedded spec to `~/.config/yosh/completions/<command>.toml`, printing each written path; refuses to overwrite an existing file without `--force` (exit 1); unknown spec name → exit 1; no names / unknown flag → exit 2; `HOME` unset → exit 1
- unknown subcommand → exit 2

- [ ] **Step 1: Write the failing integration test**

Create `tests/completions_cli.rs`:

```rust
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yosh-completions"))
}

#[test]
fn help_prints_usage_and_exits_zero() {
    for arg in [None, Some("-h"), Some("--help")] {
        let mut cmd = bin();
        if let Some(a) = arg {
            cmd.arg(a);
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "args {arg:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"), "args {arg:?}");
        assert!(stdout.contains("list"), "args {arg:?}");
        assert!(stdout.contains("export"), "args {arg:?}");
    }
}

#[test]
fn list_prints_embedded_spec_names() {
    let output = bin().arg("list").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = stdout.lines().collect();
    assert!(names.contains(&"git"), "list output: {stdout}");
    assert!(names.contains(&"cd"), "list output: {stdout}");
}

#[test]
fn export_writes_spec_and_refuses_overwrite_without_force() {
    let home = tempfile::TempDir::new().unwrap();
    let spec_path = home.path().join(".config/yosh/completions/git.toml");

    let output = bin().env("HOME", home.path()).args(["export", "git"]).output().unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(spec_path.is_file());
    let text = std::fs::read_to_string(&spec_path).unwrap();
    assert!(text.contains("[[subcommands]]"), "exported file should be the git spec");

    // Second export without --force must fail and leave the file alone.
    std::fs::write(&spec_path, "# user edit\n").unwrap();
    let output = bin().env("HOME", home.path()).args(["export", "git"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "stderr: {stderr}");
    assert_eq!(std::fs::read_to_string(&spec_path).unwrap(), "# user edit\n");

    // --force overwrites.
    let output = bin()
        .env("HOME", home.path())
        .args(["export", "--force", "git"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(std::fs::read_to_string(&spec_path).unwrap().contains("[[subcommands]]"));
}

#[test]
fn export_unknown_name_exits_one() {
    let home = tempfile::TempDir::new().unwrap();
    let output = bin()
        .env("HOME", home.path())
        .args(["export", "no-such-spec-xyz"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-spec-xyz"), "stderr: {stderr}");
}

#[test]
fn export_without_names_is_usage_error() {
    let output = bin().arg("export").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_subcommand_is_usage_error() {
    let output = bin().arg("frobnicate").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test completions_cli 2>&1 | tail -5`
Expected: COMPILE ERROR — no binary `yosh-completions`.

- [ ] **Step 3: Implement the CLI module and binary**

Create `src/completions_cli.rs`:

```rust
//! CLI for the `yosh-completions` binary, reachable as `yosh completions`
//! via the git-style subcommand delegation in `main.rs`. Inspects and
//! exports the completion specs embedded in the shell at compile time.

use crate::interactive::spec_completion::EMBEDDED_SPECS;
use std::path::PathBuf;

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => {
            print_help();
            0
        }
        Some("list") => {
            for (name, _) in EMBEDDED_SPECS {
                println!("{name}");
            }
            0
        }
        Some("export") => export(&args[1..]),
        Some(other) => {
            eprintln!("yosh-completions: unknown command `{other}`");
            eprintln!("Run 'yosh completions --help' for usage.");
            2
        }
    }
}

fn print_help() {
    println!("yosh-completions - manage yosh completion specs");
    println!();
    println!("Usage:  yosh completions <command> [args...]");
    println!();
    println!("Commands:");
    println!("  list                      List embedded completion specs");
    println!("  export [--force] <cmd>..  Copy embedded specs to ~/.config/yosh/completions/");
    println!("                            for customization (won't overwrite without --force)");
}

fn export(args: &[String]) -> i32 {
    let mut force = false;
    let mut names: Vec<&str> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--force" | "-f" => force = true,
            other if other.starts_with('-') => {
                eprintln!("yosh-completions: export: unknown option `{other}`");
                return 2;
            }
            other => names.push(other),
        }
    }
    if names.is_empty() {
        eprintln!("yosh-completions: export requires at least one command name");
        return 2;
    }
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("yosh-completions: HOME is not set");
        return 1;
    };
    let dir = PathBuf::from(home).join(".config/yosh/completions");

    let mut status = 0;
    for name in names {
        let Some((_, text)) = EMBEDDED_SPECS.iter().find(|(n, _)| *n == name) else {
            eprintln!(
                "yosh-completions: no embedded spec for `{name}` (see 'yosh completions list')"
            );
            status = 1;
            continue;
        };
        let path = dir.join(format!("{name}.toml"));
        if path.exists() && !force {
            eprintln!(
                "yosh-completions: {} already exists (use --force to overwrite)",
                path.display()
            );
            status = 1;
            continue;
        }
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, text));
        match written {
            Ok(()) => println!("{}", path.display()),
            Err(err) => {
                eprintln!("yosh-completions: {}: {err}", path.display());
                status = 1;
            }
        }
    }
    status
}
```

Create `src/bin/yosh-completions.rs` (mirrors `src/bin/yosh-plugin.rs`):

```rust
fn main() {
    std::process::exit(yosh::completions_cli::run());
}
```

In `src/lib.rs`, add after the existing `pub mod` list (alphabetical order — after `pub mod builtin;`):

```rust
pub mod completions_cli;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test completions_cli 2>&1 | tail -5`
Expected: `test result: ok. 6 passed`.

- [ ] **Step 5: Manually verify the delegation path**

```bash
cargo build
PATH="$PWD/target/debug:$PATH" ./target/debug/yosh completions list | head -3
```

Expected: first three spec names (`[`, `alias`, `bg`). This exercises `try_subcommand` finding `yosh-completions` in PATH.

- [ ] **Step 6: Commit**

```bash
git add src/completions_cli.rs src/bin/yosh-completions.rs src/lib.rs tests/completions_cli.rs
git commit -m "feat(completion): add yosh-completions binary with list/export

'yosh completions list' shows the embedded specs; 'yosh completions
export [--force] <cmd>...' copies one into ~/.config/yosh/completions/
as a starting point for customization. Dispatched via the existing
git-style yosh-<sub> PATH delegation, like yosh-plugin.

Prompt: completions をインストール時に自動で使えるように（埋め込みフォールバック方式）

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016uw49CRUMzUa3wggZ7HKav"
```

---

### Task 4: Help text and documentation

**Files:**
- Modify: `src/main.rs:77` (Subcommands help section)
- Modify: `tests/cli_help.rs` (assert the new help line)
- Modify: `completions/README.md` (replace the manual-copy instructions)
- Modify: `completion.md` (document the two-layer lookup)

**Interfaces:**
- Consumes: the CLI from Task 3 (help text references `yosh completions --help`).
- Produces: nothing consumed by later tasks (final task).

- [ ] **Step 1: Extend the failing help test**

In `tests/cli_help.rs`, in `help_flag_prints_usage_and_exits_zero`, after the existing `plugin` assertion add:

```rust
    assert!(
        stdout.contains("completions"),
        "should list completions subcommand"
    );
```

Run: `cargo test --test cli_help 2>&1 | tail -5`
Expected: FAIL — help does not mention `completions`.

- [ ] **Step 2: Add the help line**

In `src/main.rs`, change the Subcommands section (line ~77):

```rust
        HelpSection {
            heading: "Subcommands",
            items: &[
                ("plugin", "Manage shell plugins (see 'yosh plugin --help')"),
                (
                    "completions",
                    "Manage completion specs (see 'yosh completions --help')",
                ),
            ],
        },
```

Run: `cargo test --test cli_help 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 3: Update completions/README.md**

Replace the paragraph and code block that currently read:

```
yosh loads specs from `~/.config/yosh/completions/<command>.toml`.
These files are not read from the repository — copy them into place
to use them:

    mkdir -p ~/.config/yosh/completions
    cp completions/*.toml ~/.config/yosh/completions/
```

with (indented here to avoid fence nesting — write it as normal markdown with a fenced `sh` block in the README):

    Every spec in this directory is embedded into the `yosh` binary at
    compile time (see `build.rs`) and works out of the box — no setup
    needed. Lookup is layered: a user file at
    `~/.config/yosh/completions/<command>.toml` always takes precedence
    over the embedded spec.

    To customize a bundled spec, export it as a starting point:

    ```sh
    yosh completions export git    # writes ~/.config/yosh/completions/git.toml
    ```

    To disable a bundled spec, place an empty `<command>.toml` in the
    config directory (an empty spec falls back to default path
    completion).

- [ ] **Step 4: Update completion.md**

In `completion.md`, find the section describing where specs are loaded from (search for `~/.config/yosh/completions`) and document the layered lookup with the same three facts: embedded-by-default, user file wins, broken user file warns and disables (no silent fallback). Match the document's existing tone and heading style — read the surrounding text first.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/cli_help.rs completions/README.md completion.md
git commit -m "docs(completion): document embedded specs and layered lookup

Help lists the completions subcommand; README/completion.md describe
that bundled specs are compiled in, user files always win, and export
is the path to customization.

Prompt: completions をインストール時に自動で使えるように（埋め込みフォールバック方式）

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016uw49CRUMzUa3wggZ7HKav"
```

---

### Task 5: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

Run in the background (takes minutes; never use a short timeout):

```bash
cargo test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 2: Verify the crates.io package still ships the spec files**

`Cargo.toml` has no `include`/`exclude` keys, so `completions/` is packaged automatically — confirm it stays that way:

```bash
cargo package --list -p yosh --allow-dirty 2>/dev/null | grep -c '^completions/.*\.toml$'
```

Expected: `35` (or the current count of `completions/*.toml` — compare against `ls completions/*.toml | wc -l`).

- [ ] **Step 3: End-to-end smoke test with a clean HOME**

```bash
HOME=$(mktemp -d) ./target/debug/yosh completions export git && echo OK
```

Expected: prints the written path and `OK` — proving a fresh install can export without any pre-existing config directory.
