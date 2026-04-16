# Publish yosh to crates.io Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename kish to yosh across the entire codebase and prepare all 4 workspace crates for crates.io publication.

**Architecture:** Mechanical rename (kish → yosh) across directories, Cargo.toml files, source code, tests, and scripts. Add crates.io metadata and MIT license. Final step renames the GitHub repository.

**Tech Stack:** Rust/Cargo, git, gh CLI

---

### Task 1: Create LICENSE file and rename crate directories

**Files:**
- Create: `LICENSE`
- Rename: `crates/kish-plugin-api/` → `crates/yosh-plugin-api/`
- Rename: `crates/kish-plugin-sdk/` → `crates/yosh-plugin-sdk/`
- Rename: `crates/kish-plugin-manager/` → `crates/yosh-plugin-manager/`

- [ ] **Step 1: Create MIT LICENSE file**

```
MIT License

Copyright (c) 2026 Kazuki Yamamoto

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Rename crate directories using git mv**

```bash
git mv crates/kish-plugin-api crates/yosh-plugin-api
git mv crates/kish-plugin-sdk crates/yosh-plugin-sdk
git mv crates/kish-plugin-manager crates/yosh-plugin-manager
```

- [ ] **Step 3: Commit**

```bash
git add LICENSE
git commit -m "chore: add MIT license and rename crate directories kish → yosh"
```

---

### Task 2: Update all Cargo.toml files

**Files:**
- Modify: `Cargo.toml` (root)
- Modify: `crates/yosh-plugin-api/Cargo.toml`
- Modify: `crates/yosh-plugin-sdk/Cargo.toml`
- Modify: `crates/yosh-plugin-manager/Cargo.toml`
- Modify: `tests/plugins/test_plugin/Cargo.toml`

- [ ] **Step 1: Update root Cargo.toml**

Replace the entire content with:

```toml
[workspace]
members = [
    ".",
    "crates/yosh-plugin-api",
    "crates/yosh-plugin-sdk",
    "crates/yosh-plugin-manager",
    "tests/plugins/test_plugin",
]

[package]
name = "yosh"
version = "0.1.0"
edition = "2024"
description = "A POSIX-compliant shell implemented in Rust"
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"

[dependencies]
nix = { version = "0.31", features = ["signal", "process", "fs", "poll", "term"] }
libc = "0.2"
crossterm = "0.29"
libloading = "0.8"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
yosh-plugin-api = { version = "0.1.0", path = "crates/yosh-plugin-api" }
unicode-width = "0.2"
owo-colors = "4"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3"
crossterm = "0.29"
expectrl = "0.8"

[[bench]]
name = "lexer_bench"
harness = false

[[bench]]
name = "parser_bench"
harness = false

[[bench]]
name = "expand_bench"
harness = false
```

- [ ] **Step 2: Update crates/yosh-plugin-api/Cargo.toml**

```toml
[package]
name = "yosh-plugin-api"
version = "0.1.0"
edition = "2024"
description = "Plugin API for the yosh shell"
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"

[dependencies]
```

- [ ] **Step 3: Update crates/yosh-plugin-sdk/Cargo.toml**

```toml
[package]
name = "yosh-plugin-sdk"
version = "0.1.0"
edition = "2024"
description = "Plugin SDK for the yosh shell"
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"

[dependencies]
yosh-plugin-api = { version = "0.1.0", path = "../yosh-plugin-api" }

[lib]
crate-type = ["rlib"]
```

- [ ] **Step 4: Update crates/yosh-plugin-manager/Cargo.toml**

```toml
[package]
name = "yosh-plugin-manager"
version = "0.1.0"
edition = "2024"
description = "Plugin manager for the yosh shell"
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"

[lib]
name = "yosh_plugin_manager"
path = "src/lib.rs"

[[bin]]
name = "yosh-plugin"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive", "color"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
toml_edit = "0.22"
serde_json = "1"
ureq = "3"
sha2 = "0.10"
tempfile = "3"

[dev-dependencies]
mockito = "1"
```

- [ ] **Step 5: Update tests/plugins/test_plugin/Cargo.toml**

```toml
[package]
name = "test_plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
yosh-plugin-sdk = { path = "../../../crates/yosh-plugin-sdk" }
```

- [ ] **Step 6: Verify workspace resolves**

Run: `cargo metadata --format-version 1 > /dev/null`
Expected: success, no errors

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: update all Cargo.toml files with yosh names and crates.io metadata"
```

---

### Task 3: Rename build scripts (KISH_ → YOSH_ env vars)

**Files:**
- Modify: `build.rs`
- Modify: `crates/yosh-plugin-manager/build.rs`
- Modify: `src/main.rs` (env! macro references)
- Modify: `crates/yosh-plugin-manager/src/main.rs` (env! macro references)

- [ ] **Step 1: Update root build.rs**

In `build.rs`, replace all occurrences:
- `KISH_GIT_HASH` → `YOSH_GIT_HASH`
- `KISH_BUILD_DATE` → `YOSH_BUILD_DATE`

The two `println!` lines become:
```rust
    println!("cargo:rustc-env=YOSH_GIT_HASH={}", hash);
    println!("cargo:rustc-env=YOSH_BUILD_DATE={}", date);
```

- [ ] **Step 2: Update crates/yosh-plugin-manager/build.rs**

Same changes:
```rust
    println!("cargo:rustc-env=YOSH_GIT_HASH={}", hash);
    println!("cargo:rustc-env=YOSH_BUILD_DATE={}", date);
```

- [ ] **Step 3: Update src/main.rs env! references**

In `src/main.rs:78-79`, change:
```rust
        env!("YOSH_GIT_HASH"),
        env!("YOSH_BUILD_DATE"));
```

- [ ] **Step 4: Update crates/yosh-plugin-manager/src/main.rs env! references**

In `crates/yosh-plugin-manager/src/main.rs:7-10`, change:
```rust
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("YOSH_GIT_HASH"),
    " ",
    env!("YOSH_BUILD_DATE"),
    ")"
);
```

- [ ] **Step 5: Commit**

```bash
git add build.rs crates/yosh-plugin-manager/build.rs src/main.rs crates/yosh-plugin-manager/src/main.rs
git commit -m "chore: rename KISH_ build env vars to YOSH_"
```

---

### Task 4: Rename plugin API crate symbols

**Files:**
- Modify: `crates/yosh-plugin-api/src/lib.rs`

- [ ] **Step 1: Rename KISH_PLUGIN_API_VERSION and doc comments**

In `crates/yosh-plugin-api/src/lib.rs`:

Replace `KISH_PLUGIN_API_VERSION` → `YOSH_PLUGIN_API_VERSION` (line 4).

Replace all doc comments referencing `kish`:
- Line 3: `"API version for compatibility checks between kish and plugins."` → `"API version for compatibility checks between yosh and plugins."`
- Line 27: `"Plugin metadata returned by kish_plugin_decl()."` → `"Plugin metadata returned by yosh_plugin_decl()."`
- Line 41: `"API callbacks kish provides to plugins."` → `"API callbacks yosh provides to plugins."`
- Line 43: `"ctx is an opaque pointer to kish internals."` → `"ctx is an opaque pointer to yosh internals."`

- [ ] **Step 2: Commit**

```bash
git add crates/yosh-plugin-api/src/lib.rs
git commit -m "chore: rename KISH symbols to YOSH in plugin API crate"
```

---

### Task 5: Rename plugin SDK crate symbols

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Rename all kish references in SDK**

In `crates/yosh-plugin-sdk/src/lib.rs`:

1. Line 3: `pub use kish_plugin_api as ffi;` → `pub use yosh_plugin_api as ffi;`

2. Line 165: `/// Usage: \`kish_plugin_sdk::export!(MyPlugin);\`` → `/// Usage: \`yosh_plugin_sdk::export!(MyPlugin);\``

3. Rename all `kish_plugin_*` exported FFI function names in the `export!` macro (these are ABI symbols loaded by `src/plugin/mod.rs`):
   - `kish_plugin_decl` → `yosh_plugin_decl` (line 187)
   - `KISH_PLUGIN_API_VERSION` → `YOSH_PLUGIN_API_VERSION` (line 196)
   - `kish_plugin_init` → `yosh_plugin_init` (line 211)
   - `kish_plugin_commands` → `yosh_plugin_commands` (line 230)
   - `"kish_plugin_commands called before init"` → `"yosh_plugin_commands called before init"` (line 233)
   - `kish_plugin_exec` → `yosh_plugin_exec` (line 249)
   - `kish_plugin_hook_pre_exec` → `yosh_plugin_hook_pre_exec` (line 274)
   - `kish_plugin_hook_post_exec` → `yosh_plugin_hook_post_exec` (line 290)
   - `kish_plugin_hook_on_cd` → `yosh_plugin_hook_on_cd` (line 307)
   - `kish_plugin_hook_pre_prompt` → `yosh_plugin_hook_pre_prompt` (line 325)
   - `kish_plugin_destroy` → `yosh_plugin_destroy` (line 339)

- [ ] **Step 2: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "chore: rename kish symbols to yosh in plugin SDK crate"
```

---

### Task 6: Rename plugin manager crate source

**Files:**
- Modify: `crates/yosh-plugin-manager/src/main.rs`
- Modify: `crates/yosh-plugin-manager/src/sync.rs`
- Modify: `crates/yosh-plugin-manager/src/github.rs`
- Modify: `crates/yosh-plugin-manager/src/config.rs`
- Modify: `crates/yosh-plugin-manager/src/lockfile.rs`
- Modify: `crates/yosh-plugin-manager/src/install.rs`
- Modify: `crates/yosh-plugin-manager/src/resolve.rs`

- [ ] **Step 1: Update main.rs**

In `crates/yosh-plugin-manager/src/main.rs`:

1. Line 4: `use kish_plugin_manager::` → `use yosh_plugin_manager::`
2. Line 16: `name = "kish-plugin", about = "Manage kish shell plugins"` → `name = "yosh-plugin", about = "Manage yosh shell plugins"`
3. All `"kish plugin sync"` → `"yosh plugin sync"` (line 68)
4. All `"kish-plugin: "` error prefixes → `"yosh-plugin: "` (lines 73, 83, 97, 103, 116, 126, 164, 178, 184, 212)
5. Line 184: `"no plugins installed (run 'kish-plugin sync' first)"` → `"no plugins installed (run 'yosh-plugin sync' first)"`

- [ ] **Step 2: Update sync.rs**

In `crates/yosh-plugin-manager/src/sync.rs`:

1. All `.kish/plugins` paths → `.yosh/plugins` (lines 11, 13)
2. All `.config/kish` paths → `.config/yosh` (lines 19, 21)
3. All `"kish-plugin: "` error prefixes → `"yosh-plugin: "` (lines 48, 65, 78, 80, 127, 132)
4. Line 115: `"~/.kish/plugins/{}/{}"` → `"~/.yosh/plugins/{}/{}"`
5. Line 159: `"~/.kish/plugins/{}/{}"` → `"~/.yosh/plugins/{}/{}"`

- [ ] **Step 3: Update github.rs**

In `crates/yosh-plugin-manager/src/github.rs`:

1. Lines 46, 119: `"User-Agent", "kish-plugin-manager"` → `"User-Agent", "yosh-plugin-manager"`

- [ ] **Step 4: Update config.rs**

In `crates/yosh-plugin-manager/src/config.rs`:

Only test data contains `kish` references:
1. Line 134: `"local:~/.kish/plugins/lib.dylib"` → `"local:~/.yosh/plugins/lib.dylib"`
2. Line 137: `"~/.kish/plugins/lib.dylib"` → `"~/.yosh/plugins/lib.dylib"`
3. Line 168: `"local:~/.kish/plugins/liblocal.dylib"` → `"local:~/.yosh/plugins/liblocal.dylib"`

- [ ] **Step 5: Update lockfile.rs**

In `crates/yosh-plugin-manager/src/lockfile.rs`:

Test data:
1. Line 65: `"~/.kish/plugins/git-status/libgit_status.dylib"` → `"~/.yosh/plugins/git-status/libgit_status.dylib"`
2. Line 103: `"~/.kish/plugins/liblocal.dylib"` → `"~/.yosh/plugins/liblocal.dylib"`
3. Line 107: `"local:~/.kish/plugins/liblocal.dylib"` → `"local:~/.yosh/plugins/liblocal.dylib"`

- [ ] **Step 6: Update install.rs**

In `crates/yosh-plugin-manager/src/install.rs`:

Test data only — these reference "kish-plugin-foo" as an example repo name, which is a user-facing convention name. Keep the test data referencing "kish-plugin-foo" as-is since it's a hypothetical external plugin name, not part of the yosh project itself.

No changes needed.

- [ ] **Step 7: Update resolve.rs**

In `crates/yosh-plugin-manager/src/resolve.rs`:

Test data:
1. Line 74: `"kish_{name}-{os}-{arch}.{ext}"` → `"yosh_{name}-{os}-{arch}.{ext}"`
2. Line 75: `"kish_auto_env-..."` → `"yosh_auto_env-..."`

- [ ] **Step 8: Commit**

```bash
git add crates/yosh-plugin-manager/src/
git commit -m "chore: rename kish to yosh in plugin manager crate"
```

---

### Task 7: Rename core source code (src/)

**Files:**
- Modify: `src/main.rs`
- Modify: `src/error.rs`
- Modify: `src/env/mod.rs`
- Modify: `src/exec/mod.rs`
- Modify: `src/exec/simple.rs`
- Modify: `src/exec/pipeline.rs`
- Modify: `src/exec/compound.rs`
- Modify: `src/exec/command.rs`
- Modify: `src/exec/redirect.rs`
- Modify: `src/expand/mod.rs`
- Modify: `src/expand/arith.rs`
- Modify: `src/expand/param.rs`
- Modify: `src/expand/command_sub.rs`
- Modify: `src/expand/field_split.rs`
- Modify: `src/expand/pathname.rs`
- Modify: `src/builtin/mod.rs`
- Modify: `src/builtin/special.rs`
- Modify: `src/builtin/regular.rs`
- Modify: `src/interactive/mod.rs`
- Modify: `src/plugin/mod.rs`
- Modify: `src/plugin/config.rs`

- [ ] **Step 1: Rename in src/main.rs**

All remaining `"kish"` string literals → `"yosh"`:
- Line 35: `"kish - A POSIX-compliant shell"` → `"yosh - A POSIX-compliant shell"`
- Line 44: `"kish [options]"` → `"yosh [options]"`
- Line 46: `"kish [options]"` → `"yosh [options]"`
- Line 68: `"kish plugin --help"` → `"yosh plugin --help"`
- Line 71: `"kish plugin --help"` → `"yosh plugin --help"`
- Line 76: `"kish {}"` → `"yosh {}"`
- Line 84: `"kish"` (default shell name) → `"yosh"`
- Line 95: `"kish: {}"` → `"yosh: {}"`
- Line 111: `"kish: -c requires"` → `"yosh: -c requires"`
- Line 124: `"kish: --parse requires"` → `"yosh: --parse requires"`
- Line 149: doc comment `kish` → `yosh`
- Line 157: `"kish-{}"` → `"yosh-{}"` (subcommand binary name)
- Line 166: `"kish: {}: {}"` → `"yosh: {}: {}"`
- Line 232: `"kish: {}: {}"` → `"yosh: {}: {}"`

- [ ] **Step 2: Rename in src/error.rs**

Replace all `"kish: "` → `"yosh: "` (lines 64, 65).

- [ ] **Step 3: Rename in src/env/mod.rs**

Test code only: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (lines 81, 82, 91, 97).

- [ ] **Step 4: Rename in src/exec/mod.rs**

1. Line 54: comment `~/.config/kish/plugins.lock` → `~/.config/yosh/plugins.lock`
2. Line 661: `".config/kish/plugins.lock"` → `".config/yosh/plugins.lock"`
3. All error messages `"kish: "` → `"yosh: "` (lines 245, 341, 348, 420, 466, 474, 482, 492, 530, 538, 546, 556, 561)
4. All test code `Executor::new("kish"` → `Executor::new("yosh"` (many lines in #[cfg(test)])

- [ ] **Step 5: Rename in src/exec/simple.rs**

All `"kish: "` → `"yosh: "` (lines 58, 94, 119, 144, 178, 187, 197, 219, 300, 310, 322, 339, 363, 367, 371).

- [ ] **Step 6: Rename in src/exec/pipeline.rs**

All `"kish: "` → `"yosh: "` (lines 42, 55, 79, 87, 182).

- [ ] **Step 7: Rename in src/exec/compound.rs**

All `"kish: "` → `"yosh: "` (lines 21, 86, 204).

- [ ] **Step 8: Rename in src/exec/command.rs**

`"kish: waitpid"` → `"yosh: waitpid"` (line 34).

- [ ] **Step 9: Rename in src/exec/redirect.rs**

Test code: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (line 247).

- [ ] **Step 10: Rename in src/expand/mod.rs**

1. All error messages `"kish: "` → `"yosh: "` (lines 259, 412)
2. All test code `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (lines 627, 657, 671, 686, 706, 723, 848, 861, 871)
3. Line 839: `"mykish"` → `"myyosh"` and line 843 assertion update

- [ ] **Step 11: Rename in src/expand/arith.rs**

1. Error message: `"kish: arithmetic"` → `"yosh: arithmetic"` (line 20)
2. Test code: all `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` and `"kish"` string literals in tests (lines 712, 794, 803, 806, 813, 829)

- [ ] **Step 12: Rename in src/expand/param.rs**

1. Error messages `"kish: "` → `"yosh: "` (lines 14, 80)
2. Test code: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (lines 223, 440)

- [ ] **Step 13: Rename in src/expand/command_sub.rs**

All `"kish: "` → `"yosh: "` (lines 19, 27, 82, 95).

- [ ] **Step 14: Rename in src/expand/field_split.rs**

Test code: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (lines 188, 194).

- [ ] **Step 15: Rename in src/expand/pathname.rs**

Test code: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (line 190).

- [ ] **Step 16: Rename in src/builtin/mod.rs**

1. Error messages `"kish: "` → `"yosh: "` (lines 55, 60, 64)
2. Test code: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (line 76)

- [ ] **Step 17: Rename in src/builtin/special.rs**

1. All error messages `"kish: "` → `"yosh: "` (lines 38, 55, 87, 104, 134, 148, 157, 172, 177, 192, 197, 246, 256, 278, 292, 301, 309, 310, 311, 342, 351, 369, 377, 392, 405, 411, 422, 442, 459, 471, 578, 588, 607, 616, 634)
2. Line 575: `"kish_fc"` → `"yosh_fc"` (tempfile prefix)
3. Test code: `Executor::new("kish"` → `Executor::new("yosh"` (lines 679, 688)

- [ ] **Step 18: Rename in src/builtin/regular.rs**

All error messages `"kish: "` → `"yosh: "` (lines 11, 19, 46, 52, 84, 95, 103, 112, 123, 133, 144, 156, 200, 208, 261, 271, 286, 312, 326).

- [ ] **Step 19: Rename in src/interactive/mod.rs**

1. Line 49: `".kish_history"` → `".yosh_history"`
2. Line 136: `"kish: Use"` → `"yosh: Use"`
3. Line 195: `"kish: {}"` → `"yosh: {}"`

- [ ] **Step 20: Rename in src/plugin/mod.rs**

1. Line 7: `use kish_plugin_api::{HostApi, PluginDecl, KISH_PLUGIN_API_VERSION};` → `use yosh_plugin_api::{HostApi, PluginDecl, YOSH_PLUGIN_API_VERSION};`
2. All `"kish: "` error messages → `"yosh: "` (lines 53, 193, 522, 535, 549, 563, 575, 589, 603)
3. Line 79: `b"kish_plugin_decl"` → `b"yosh_plugin_decl"`
4. Line 81: `"not a valid kish plugin"` → `"not a valid yosh plugin"`
5. Line 85: `KISH_PLUGIN_API_VERSION` → `YOSH_PLUGIN_API_VERSION` (2 occurrences, lines 85, 89)
6. Line 119: `b"kish_plugin_init"` → `b"yosh_plugin_init"`
7. Line 120: `"missing kish_plugin_init"` → `"missing yosh_plugin_init"`
8. Line 135: `b"kish_plugin_commands"` → `b"yosh_plugin_commands"`
9. Line 155: `b"kish_plugin_hook_pre_exec"` → `b"yosh_plugin_hook_pre_exec"`
10. Line 157: `b"kish_plugin_hook_post_exec"` → `b"yosh_plugin_hook_post_exec"`
11. Line 159: `b"kish_plugin_hook_on_cd"` → `b"yosh_plugin_hook_on_cd"`
12. Line 161: `b"kish_plugin_hook_pre_prompt"` → `b"yosh_plugin_hook_pre_prompt"`
13. Line 179: `use kish_plugin_api::*;` → `use yosh_plugin_api::*;`
14. Line 247: `kish_plugin_api::CAP_HOOK_PRE_EXEC` → `yosh_plugin_api::CAP_HOOK_PRE_EXEC`
15. Line 257: `b"kish_plugin_hook_pre_exec"` → `b"yosh_plugin_hook_pre_exec"`
16. Same pattern for post_exec (line 274, 285), on_cd (line 305, 316), pre_prompt (line 329, 336)
17. Line 350: `b"kish_plugin_destroy"` → `b"yosh_plugin_destroy"`
18. Line 614: `use kish_plugin_api::*;` → `use yosh_plugin_api::*;`

- [ ] **Step 21: Rename in src/plugin/config.rs**

All `kish_plugin_api::` → `yosh_plugin_api::` (lines 51-58, 187, 216).

- [ ] **Step 22: Verify build compiles**

Run: `cargo check`
Expected: success with no errors

- [ ] **Step 23: Commit**

```bash
git add src/
git commit -m "chore: rename kish to yosh across all source code"
```

---

### Task 8: Rename test plugin and integration tests

**Files:**
- Modify: `tests/plugins/test_plugin/src/lib.rs`
- Modify: `tests/helpers/mod.rs`
- Modify: `tests/helpers/mock_terminal.rs` (if contains kish)
- Modify: `tests/cli_help.rs`
- Modify: `tests/signals.rs`
- Modify: `tests/history.rs`
- Modify: `tests/subshell.rs`
- Modify: `tests/errexit.rs`
- Modify: `tests/interactive.rs`
- Modify: `tests/plugin.rs`
- Modify: `tests/parser_integration.rs`
- Modify: `tests/pty_interactive.rs`

- [ ] **Step 1: Update test plugin**

In `tests/plugins/test_plugin/src/lib.rs`:
- Line 1: `use kish_plugin_sdk::` → `use yosh_plugin_sdk::`

- [ ] **Step 2: Update tests/helpers/mod.rs**

Line 20: `"kish-test-{}-{}"` → `"yosh-test-{}-{}"`

- [ ] **Step 3: Update tests/cli_help.rs**

1. Line 3-4: `fn kish_bin()` → `fn yosh_bin()` and `CARGO_BIN_EXE_kish` → `CARGO_BIN_EXE_yosh`
2. All calls to `kish_bin()` → `yosh_bin()` (lines 9, 23, 32, 43, 50, 62, 74)
3. Assertions: `"kish - A POSIX-compliant shell"` → `"yosh - A POSIX-compliant shell"` (lines 12, 26)
4. Line 35: `"kish "` → `"yosh "`
5. Line 37: comment `kish 0.1.0` → `yosh 0.1.0`

- [ ] **Step 4: Update tests/signals.rs**

1. Lines 11-12: `fn kish_exec` → `fn yosh_exec` and `CARGO_BIN_EXE_kish` → `CARGO_BIN_EXE_yosh`
2. Line 15: `"failed to execute kish"` → `"failed to execute yosh"`
3. Lines 21-37: `fn kish_exec_timeout` → `fn yosh_exec_timeout`, `kish-test` → `yosh-test`, `CARGO_BIN_EXE_kish` → `CARGO_BIN_EXE_yosh`, `"failed to spawn kish"` → `"failed to spawn yosh"`
4. Line 54: `"kish timed out"` → `"yosh timed out"`
5. Line 61: `"error waiting for kish"` → `"error waiting for yosh"`
6. All calls to `kish_exec(` → `yosh_exec(` and `kish_exec_timeout(` → `yosh_exec_timeout(`

- [ ] **Step 5: Update tests/history.rs**

1. Lines 3-4: `fn kish_exec` → `fn yosh_exec`, `CARGO_BIN_EXE_kish` → `CARGO_BIN_EXE_yosh`
2. Line 7: `"failed to execute kish"` → `"failed to execute yosh"`
3. All calls `kish_exec(` → `yosh_exec(`

- [ ] **Step 6: Update tests/subshell.rs**

1. Lines 5-6: `fn kish_exec` → `fn yosh_exec`, `CARGO_BIN_EXE_kish` → `CARGO_BIN_EXE_yosh`
2. Line 9: `"failed to execute kish"` → `"failed to execute yosh"`
3. All calls `kish_exec(` → `yosh_exec(`
4. Lines containing `kish-fd-test` → `yosh-fd-test`, `kish-exec-persist` → `yosh-exec-persist`, `kish-return-test` → `yosh-return-test`

- [ ] **Step 7: Update tests/errexit.rs**

1. Lines 5-6: `fn kish_exec` → `fn yosh_exec`, `CARGO_BIN_EXE_kish` → `CARGO_BIN_EXE_yosh`
2. Line 9: `"failed to execute kish"` → `"failed to execute yosh"`
3. All calls `kish_exec(` → `yosh_exec(`

- [ ] **Step 8: Update tests/interactive.rs**

1. All `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (lines 185, 193, 201, 209, 218)
2. Line 1108: `"kish-tab-test5"` → `"yosh-tab-test5"`
3. Lines 1154, 1178-1179, 1191: `"kish_test_mycmd"` → `"yosh_test_mycmd"`, `"kish_test_my"` → `"yosh_test_my"`

- [ ] **Step 9: Update tests/plugin.rs**

All `ShellEnv::new("kish"` → `ShellEnv::new("yosh"` (lines 36, 48, 61, 78, 90, 102, 114, 126, 137, 147, 170, 194, 220, 249, 277, 297, 322).

- [ ] **Step 10: Update tests/parser_integration.rs**

Lines 21-22: `"kish"` → `"yosh"` (comment and $0 name).

- [ ] **Step 11: Update tests/pty_interactive.rs**

1. Line 26: `"kish-pty-test"` → `"yosh-pty-test"`
2. Lines 263, 280: `"kish_tab_test_unique.txt"` → `"yosh_tab_test_unique.txt"`
3. Lines 336, 343: `"kish_argcomp_unique.txt"` → `"yosh_argcomp_unique.txt"`

- [ ] **Step 12: Update tests in crates/yosh-plugin-manager/tests/**

In `crates/yosh-plugin-manager/tests/cli_help.rs`:
1. Line 3-4: `fn kish_plugin_bin()` → `fn yosh_plugin_bin()`, `CARGO_BIN_EXE_kish-plugin` → `CARGO_BIN_EXE_yosh-plugin`
2. All calls `kish_plugin_bin()` → `yosh_plugin_bin()`
3. Line 12, 25: `"kish shell plugins"` → `"yosh shell plugins"`
4. Line 57: `"kish-plugin"` → `"yosh-plugin"`

In `crates/yosh-plugin-manager/tests/sync_integration.rs`:
1. Line 6: `".config/kish"` → `".config/yosh"`
2. Line 7: `".kish/plugins"` → `".yosh/plugins"`
3. All `kish_plugin_manager::` → `yosh_plugin_manager::`

- [ ] **Step 13: Commit**

```bash
git add tests/ crates/yosh-plugin-manager/tests/
git commit -m "chore: rename kish to yosh in all tests"
```

---

### Task 9: Rename E2E tests, benchmarks, and CLAUDE.md

**Files:**
- Modify: `e2e/run_tests.sh`
- Modify: `e2e/README.md`
- Modify: `e2e/variable_and_expansion/at_vs_star_unquoted.sh`
- Modify: `e2e/field_splitting/glob_no_match.sh`
- Modify: `benches/lexer_bench.rs`
- Modify: `benches/parser_bench.rs`
- Modify: `benches/expand_bench.rs`
- Modify: `benches/data/large_script.sh`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update e2e/run_tests.sh**

1. Line 2: `# POSIX E2E Test Runner for kish` → `# POSIX E2E Test Runner for yosh`
2. Line 9: `SHELL_UNDER_TEST="./target/debug/kish"` → `SHELL_UNDER_TEST="./target/debug/yosh"`
3. Line 165: `kish_e2e` → `yosh_e2e`

- [ ] **Step 2: Update e2e/README.md**

Replace `kish` with `yosh` in all references:
- Line 3: `kish's` → `yosh's`
- Line 9: `# Build kish first` → `# Build yosh first`

- [ ] **Step 3: Update e2e/variable_and_expansion/at_vs_star_unquoted.sh**

Line 7: `# kish bug:` → `# yosh bug:`

- [ ] **Step 4: Update e2e/field_splitting/glob_no_match.sh**

Lines 4-5: `kish_nonexistent_glob_test_` → `yosh_nonexistent_glob_test_`

- [ ] **Step 5: Update bench files**

In `benches/lexer_bench.rs`:
- Line 2: `use kish::` → `use yosh::`
- Line 24: `kish::lexer::token::Token::Eof` → `yosh::lexer::token::Token::Eof`

In `benches/parser_bench.rs`:
- Line 2: `use kish::` → `use yosh::`

In `benches/expand_bench.rs`:
- Lines 2-4: `use kish::` → `use yosh::` (3 lines)
- Lines 9, 26, 40: `ShellEnv::new("kish"` → `ShellEnv::new("yosh"`

In `benches/data/large_script.sh`:
- Line 37: `"/usr/local/bin/kish"` → `"/usr/local/bin/yosh"`

Note: `benches/post-refactoring.txt` is historical benchmark output — leave as-is.

- [ ] **Step 6: Update CLAUDE.md**

1. Line 1: `# kish - POSIX Shell in Rust` → `# yosh - POSIX Shell in Rust`
2. Line 26: `kish: ` → `yosh: `

- [ ] **Step 7: Commit**

```bash
git add e2e/ benches/ CLAUDE.md
git commit -m "chore: rename kish to yosh in e2e tests, benchmarks, and CLAUDE.md"
```

---

### Task 10: Build, test, and verify

- [ ] **Step 1: Full build**

Run: `cargo build`
Expected: successful build, binary at `target/debug/yosh`

- [ ] **Step 2: Run unit and integration tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 3: Run E2E tests**

Run: `./e2e/run_tests.sh`
Expected: all tests pass (0 failures)

- [ ] **Step 4: Verify binary name**

Run: `./target/debug/yosh --version`
Expected: output starts with `yosh 0.1.0`

- [ ] **Step 5: Verify cargo install works locally**

Run: `cargo install --path .`
Expected: installs `yosh` binary

- [ ] **Step 6: Commit any fixes if needed, then final commit**

```bash
git add -A
git commit -m "chore: complete kish to yosh rename for crates.io publication

Full rename of the project from kish to yosh:
- All 4 workspace crates renamed
- crates.io metadata added (description, license, repository)
- MIT license file added
- Error messages, plugin ABI symbols, config paths updated
- All tests, e2e tests, and benchmarks updated"
```

Only create this commit if there are uncommitted changes from fixes.

---

### Task 11: GitHub repository rename

- [ ] **Step 1: Push all changes**

Run: `git push`

- [ ] **Step 2: Rename GitHub repository**

Run: `gh repo rename yosh`
Expected: repository renamed to `k-ymmt/yosh`

- [ ] **Step 3: Verify new URL**

Run: `gh repo view --json url -q .url`
Expected: `https://github.com/k-ymmt/yosh`
