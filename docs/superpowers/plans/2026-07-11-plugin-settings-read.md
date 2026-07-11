# Plugin Settings Read API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let every plugin read its own `~/.config/yosh/plugins/{name}/settings.toml` through a new capability-free `yosh:plugin/settings` WIT interface.

**Architecture:** A new `settings` interface (single `read` function, no path argument) is added to the `yosh:plugin@0.2.1` WIT package — the version stays 0.2.1 so already-built plugins keep loading unchanged. The host resolves the settings path once at load time from the plugin name and stores it on `HostContext`; the linker registers the real implementation unconditionally (no capability gate, no deny stub).

**Tech Stack:** Rust 2024, wasmtime component model, wit-bindgen (SDK side), cargo-component for test plugin builds.

**Spec:** `docs/superpowers/specs/2026-07-11-plugin-settings-read-design.md`

## Global Constraints

- WIT package version stays `@0.2.1` — do NOT bump it anywhere.
- All four WIT copies must stay byte-identical: `wit/yosh-plugin.wit`, `crates/yosh-plugin-api/wit/yosh-plugin.wit`, `crates/yosh-plugin-sdk/wit/yosh-plugin.wit`, `crates/yosh-plugin-manager/wit/yosh-plugin.wit` (`build.rs` enforces this at compile time).
- Never run `cargo build --workspace` or `cargo test --workspace` — the wasm plugin crates fail to host-build. Use plain `cargo build` / `cargo test`.
- `cargo build` takes 1–3 min; run long test suites with generous timeouts (600000 ms).
- Plugin integration tests need `--features test-helpers` and the wasm artefact: `cargo component build -p test_plugin --target wasm32-wasip2 --release` (the test builds it automatically via `ensure_built`, but cargo-component must be installed).
- Error messages / codes follow the existing host error-mapping style: missing file is NOT an error for settings (`Ok(None)`), other I/O failures map to `ErrorCode::IoFailed`.

---

### Task 1: `settings_path_for` helper in config.rs

**Files:**
- Modify: `src/plugin/config.rs` (add function after `expand_tilde`, ~line 131; tests at the end of the `tests` module)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn settings_path_for(name: &str) -> Option<PathBuf>` in `crate::plugin::config` — used by Task 4 (`load_one` wiring).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/plugin/config.rs`:

```rust
#[test]
fn settings_path_for_builds_config_dir_path() {
    // HOME is always set in the test environment.
    let path = settings_path_for("git-prompt").expect("HOME is set");
    assert!(
        path.to_string_lossy()
            .ends_with(".config/yosh/plugins/git-prompt/settings.toml"),
        "unexpected path: {}",
        path.display()
    );
    assert!(!path.to_string_lossy().starts_with("~"));
}

#[test]
fn settings_path_for_rejects_unsafe_names() {
    // Not a single safe path component → None. Do NOT test the
    // HOME-unset branch: mutating HOME races with parallel tests.
    assert_eq!(settings_path_for(""), None);
    assert_eq!(settings_path_for("."), None);
    assert_eq!(settings_path_for(".."), None);
    assert_eq!(settings_path_for("a/b"), None);
    assert_eq!(settings_path_for("../escape"), None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib plugin::config::tests::settings_path_for -- --nocapture`
Expected: compile error "cannot find function `settings_path_for`".

- [ ] **Step 3: Write the implementation**

Add after `expand_tilde` in `src/plugin/config.rs`:

```rust
/// Absolute path of a plugin's own settings file:
/// `$HOME/.config/yosh/plugins/<name>/settings.toml`.
///
/// Returns `None` when HOME is unset or when `name` is not a safe
/// single path component (empty, `.`, `..`, or contains `/`) —
/// lockfile names are trusted, but this is cheap defense in depth.
/// A `None` makes `settings.read` report "no settings file".
pub fn settings_path_for(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config/yosh/plugins")
            .join(name)
            .join("settings.toml"),
    )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib plugin::config::tests -- --nocapture`
Expected: all pass, including the two new tests.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/config.rs
git commit -m "feat(plugin): add settings_path_for helper resolving per-plugin settings.toml

Part of the plugin settings read API (spec
docs/superpowers/specs/2026-07-11-plugin-settings-read-design.md):
plugins read ~/.config/yosh/plugins/{name}/settings.toml without any
capability."
```

---

### Task 2: WIT `settings` interface (all four copies)

**Files:**
- Modify: `wit/yosh-plugin.wit`
- Modify: `crates/yosh-plugin-api/wit/yosh-plugin.wit`
- Modify: `crates/yosh-plugin-sdk/wit/yosh-plugin.wit`
- Modify: `crates/yosh-plugin-manager/wit/yosh-plugin.wit`

**Interfaces:**
- Consumes: nothing.
- Produces: WIT interface `yosh:plugin/settings@0.2.1` with `read: func() -> result<option<string>, error-code>`; `world plugin-world` gains `import settings;`. Host bindgen exposes `generated::yosh::plugin::settings` (Task 3/4); SDK bindgen exposes `self::yosh::plugin::settings` (Task 5).

- [ ] **Step 1: Add the interface to all four WIT files**

In EACH of the four files, insert this block between `interface commands { ... }` and `interface plugin {` (identical text in all four — `build.rs` verifies byte equality):

```wit
interface settings {
    use types.{error-code};

    /// Contents of this plugin's own settings file
    /// (`~/.config/yosh/plugins/<plugin-name>/settings.toml`).
    ///
    /// Requires no capability — every plugin may read its own
    /// settings. Returns `none` when the file does not exist (the
    /// normal "no settings" case). The host returns the raw TOML
    /// text; parsing is the plugin's concern.
    read: func() -> result<option<string>, error-code>;
}
```

And in EACH file change the world block to add the import (keep existing lines untouched):

```wit
world plugin-world {
    import variables;
    import filesystem;
    import files;
    import io;
    import commands;
    import settings;

    export plugin;
    export hooks;
}
```

- [ ] **Step 2: Verify the host workspace still builds**

Run: `cargo build` (timeout 600000)
Expected: success. bindgen regenerates and now includes `generated::yosh::plugin::settings`; nothing references it yet, which is fine.

Note: components built from the new WIT import `yosh:plugin/settings@0.2.1`, but the host linker does not provide it until Task 4. Do NOT rebuild the test plugin wasm artefacts between this task and Task 4 completing.

- [ ] **Step 3: Verify the existing unit tests pass**

Run: `cargo test --lib plugin` (timeout 600000)
Expected: all pass (WIT addition is purely additive).

- [ ] **Step 4: Commit**

```bash
git add wit/yosh-plugin.wit crates/yosh-plugin-api/wit/yosh-plugin.wit crates/yosh-plugin-sdk/wit/yosh-plugin.wit crates/yosh-plugin-manager/wit/yosh-plugin.wit
git commit -m "feat(plugin): add capability-free settings interface to WIT contract

yosh:plugin/settings@0.2.1 read() returns the plugin's own
settings.toml content, none when absent. Package version deliberately
stays 0.2.1: adding an interface changes no import string of
already-built plugins, so installed plugins keep loading unchanged."
```

---

### Task 3: `HostContext.settings_path` + host implementation

**Files:**
- Modify: `src/plugin/host/mod.rs` (new field, module decl, re-export, test helper)
- Create: `src/plugin/host/settings.rs`

**Interfaces:**
- Consumes: `HostContext::ensure_bound` (existing), `generated::yosh::plugin::types::ErrorCode` (existing).
- Produces:
  - field `pub(super) settings_path: Option<std::path::PathBuf>` on `HostContext` (default `None`)
  - `pub(super) fn host_settings_read(ctx: &HostContext) -> Result<Option<String>, ErrorCode>` re-exported from `plugin::host` — used by Task 4 (linker)
  - test helper `pub fn ctx_with_settings_path(env: &mut ShellEnv, path: &std::path::Path) -> HostContext` in `host::test_helpers`

- [ ] **Step 1: Add the field and module wiring in `src/plugin/host/mod.rs`**

Add to the `HostContext` struct (after `files_root`):

```rust
    /// Absolute path of this plugin's own settings.toml, resolved at
    /// load time from `$HOME/.config/yosh/plugins/<name>/settings.toml`
    /// (`config::settings_path_for`). `None` when HOME is unset or the
    /// plugin name is unsafe as a path component — `settings.read`
    /// then reports "no settings file".
    pub(super) settings_path: Option<std::path::PathBuf>,
```

In `new_for_plugin`, add `settings_path: None,` to the struct literal (after `files_root: None,`).

Add the module declaration (alphabetical, after `mod io;`):

```rust
mod settings;
```

Add the re-export (after the `io` re-export line):

```rust
pub(super) use settings::host_settings_read;
```

Add to `test_helpers` (after `ctx_with_files_root`):

```rust
    pub fn ctx_with_settings_path(env: &mut ShellEnv, path: &std::path::Path) -> HostContext {
        let mut ctx = bound_env_ctx(env);
        ctx.settings_path = Some(path.to_path_buf());
        ctx
    }
```

- [ ] **Step 2: Write `src/plugin/host/settings.rs` with failing tests**

```rust
//! `yosh:plugin/settings` host import — read the plugin's own
//! settings.toml. Capability-free: the linker always registers the
//! real implementation (there is no deny variant). The path is fixed
//! at load time (`~/.config/yosh/plugins/<name>/settings.toml`, see
//! `config::settings_path_for`), so a plugin can structurally reach
//! only its own settings file.
//!
//! Error mapping:
//! - `settings_path == None` (no HOME / unsafe name) → `Ok(None)`
//! - file does not exist                             → `Ok(None)`
//! - any other I/O error (incl. invalid UTF-8)       → `IoFailed`

use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub fn host_settings_read(ctx: &HostContext) -> Result<Option<String>, ErrorCode> {
    ctx.ensure_bound()?;
    let Some(path) = &ctx.settings_path else {
        return Ok(None);
    };
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{bound_env_ctx, ctx_with_settings_path, null_env_ctx};
    use super::*;
    use crate::env::ShellEnv;
    use tempfile::tempdir;

    /// Metadata contract: no env binding → Denied, like every other
    /// host import.
    #[test]
    fn settings_read_denied_when_env_null() {
        let ctx = null_env_ctx();
        assert_eq!(host_settings_read(&ctx), Err(ErrorCode::Denied));
    }

    /// No resolved path (HOME unset / unsafe plugin name) behaves as
    /// "no settings file", not as an error.
    #[test]
    fn settings_read_none_when_path_unresolved() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        assert_eq!(host_settings_read(&ctx), Ok(None));
    }

    #[test]
    fn settings_read_none_when_file_missing() {
        let dir = tempdir().unwrap();
        // Parent dir of the path doesn't exist either — still NotFound.
        let path = dir.path().join("plugins/demo/settings.toml");
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_settings_path(&mut env, &path);
        assert_eq!(host_settings_read(&ctx), Ok(None));
    }

    #[test]
    fn settings_read_returns_file_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "greeting = \"hello\"\n").unwrap();
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_settings_path(&mut env, &path);
        assert_eq!(
            host_settings_read(&ctx),
            Ok(Some("greeting = \"hello\"\n".to_string()))
        );
    }

    /// TOML must be UTF-8; junk bytes surface as IoFailed, not a panic
    /// and not silently-lossy text.
    #[test]
    fn settings_read_invalid_utf8_is_io_failed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_settings_path(&mut env, &path);
        assert_eq!(host_settings_read(&ctx), Err(ErrorCode::IoFailed));
    }
}
```

- [ ] **Step 3: Run the new tests**

Run: `cargo test --lib plugin::host::settings -- --nocapture`
Expected: all 5 pass. (They were "failing" only as compile errors before Step 1+2 were both in place; the two steps are one compile unit.)

- [ ] **Step 4: Run the full plugin unit-test module**

Run: `cargo test --lib plugin` (timeout 600000)
Expected: all pass (no regression from the new field — `new_for_plugin` initializes it).

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host/mod.rs src/plugin/host/settings.rs
git commit -m "feat(plugin): host_settings_read reads the plugin's own settings.toml

HostContext gains settings_path (resolved at load time, Task 4 wires
it). Missing file and unresolved path are Ok(None); other I/O errors
map to IoFailed. ensure_bound keeps the metadata contract."
```

---

### Task 4: Linker registration + load_one wiring + manager stub

**Files:**
- Modify: `src/plugin/linker.rs` (register `yosh:plugin/settings@0.2.1`, extend import list at top)
- Modify: `src/plugin/mod.rs` (`load_one`: set `host_ctx.settings_path`; `test_helpers`: add setter)
- Modify: `crates/yosh-plugin-manager/src/metadata_extract.rs` (deny stub in `register_all_deny_imports`)

**Interfaces:**
- Consumes: `host_settings_read` (Task 3), `config::settings_path_for` (Task 1).
- Produces:
  - host linker instance `yosh:plugin/settings@0.2.1` with `read`, always the real implementation
  - `test_helpers::set_settings_path_for_tests(manager: &mut PluginManager, path: Option<std::path::PathBuf>)` — used by Task 6 integration tests

- [ ] **Step 1: Register the interface in `src/plugin/linker.rs`**

Add `host_settings_read` to the `use super::host::{...}` import list.

Insert before the `// ── yosh:plugin/commands ──` section:

```rust
    // ── yosh:plugin/settings ───────────────────────────────────────────
    //
    // Capability-free: every plugin may read its own settings.toml.
    // The path is fixed per-plugin in `HostContext::settings_path`
    // (resolved at load time from the plugin name), so no broader
    // filesystem access is exposed and no deny stub exists.
    let mut settings = linker.instance("yosh:plugin/settings@0.2.1")?;
    settings.func_wrap("read", |store, (): ()| {
        Ok((host_settings_read(store.data()),))
    })?;
```

- [ ] **Step 2: Wire the path in `load_one` (`src/plugin/mod.rs`)**

Immediately after the `host_ctx.files_root = ...` assignment (~line 579–580), add:

```rust
        host_ctx.settings_path = config::settings_path_for(&plugin_info.name);
```

(`config` functions are referenced as `self::config::...` or via existing imports — match the file's existing style: `expand_tilde` is imported via `use self::config::expand_tilde;`, so extend that import to `use self::config::{expand_tilde, settings_path_for};` and call `settings_path_for(&plugin_info.name)` directly.)

- [ ] **Step 3: Add the test helper in `src/plugin/mod.rs` `test_helpers`**

After `set_mem_denied_for_tests`:

```rust
    /// Override the most-recently-loaded plugin's resolved settings
    /// path, so integration tests can point `settings.read` at a
    /// tempdir file without mutating the process HOME.
    pub fn set_settings_path_for_tests(
        manager: &mut PluginManager,
        path: Option<std::path::PathBuf>,
    ) {
        if let Some(plugin) = manager.plugins.last_mut() {
            plugin.store.data_mut().settings_path = path;
        }
    }
```

- [ ] **Step 4: Add the deny stub in `crates/yosh-plugin-manager/src/metadata_extract.rs`**

In `register_all_deny_imports`, after the `commands` block:

```rust
    let mut settings = linker.instance("yosh:plugin/settings@0.2.1")?;
    settings.func_wrap(
        "read",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (): ()| {
            Ok::<_, wasmtime::Error>((Err::<Option<String>, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
```

- [ ] **Step 5: Build and run the linker smoke + plugin unit tests**

Run: `cargo build && cargo test --lib plugin && cargo test -p yosh-plugin-manager --lib` (timeout 600000)
Expected: all pass — `linker_construction_smoke` and the manager's `linker_registration_smoke` now cover the new instance.

- [ ] **Step 6: Commit**

```bash
git add src/plugin/linker.rs src/plugin/mod.rs crates/yosh-plugin-manager/src/metadata_extract.rs
git commit -m "feat(plugin): link settings interface unconditionally and resolve path at load

The host registers yosh:plugin/settings@0.2.1 outside any capability
gate; load_one resolves settings_path from the plugin name. The
manager's metadata-extraction linker gets the matching Denied stub
(metadata contract)."
```

---

### Task 5: SDK `read_settings()` helper

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs`

**Interfaces:**
- Consumes: bindgen-generated `self::yosh::plugin::settings` (exists after Task 2 — the SDK bundles its own WIT copy).
- Produces: `pub fn read_settings() -> Result<Option<String>, ErrorCode>` and `pub use self::yosh::plugin::settings as host_settings;` — used by Task 6's test plugin.

- [ ] **Step 1: Add the re-export**

Next to the other host re-exports (`host_files`, `host_filesystem`, `host_variables`):

```rust
pub use self::yosh::plugin::settings as host_settings;
```

- [ ] **Step 2: Add the helper function**

After the `files` helpers (e.g. after `append_file`), following the file's doc-comment style:

```rust
/// Read this plugin's own settings file
/// (`~/.config/yosh/plugins/<plugin-name>/settings.toml`).
///
/// This is the one capability-free host interface: every plugin may
/// read its own settings without declaring anything in
/// [`Plugin::required_capabilities`]. Returns `Ok(None)` when no
/// settings file exists. The raw TOML text is returned; parsing is
/// the plugin's choice (e.g. the `toml` crate, which compiles to
/// `wasm32-wasip2`).
pub fn read_settings() -> Result<Option<String>, ErrorCode> {
    host_settings::read()
}
```

Also update the crate-level `//! # Capabilities` doc paragraph: after the sentence about every host import being gated, add:

```rust
//! The one exception is `settings` ([`read_settings`]): reading the
//! plugin's own `settings.toml` requires no capability.
```

- [ ] **Step 3: Verify the SDK builds and its tests pass**

Run: `cargo build -p yosh-plugin-sdk && cargo test -p yosh-plugin-sdk` (timeout 600000)
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "feat(sdk): add capability-free read_settings() helper"
```

---

### Task 6: test_plugin command + integration tests

**Files:**
- Modify: `tests/plugins/test_plugin/src/lib.rs` (new `read-settings` command)
- Modify: `tests/plugin.rs` (two integration tests)

**Interfaces:**
- Consumes: `yosh_plugin_sdk::read_settings` (Task 5), `test_helpers::set_settings_path_for_tests` (Task 4), existing test scaffolding (`lock_test`, `test_plugin_wasm`, `fresh_env`, `load_plugin_with_caps`, `PluginExec`).
- Produces: test plugin command `read-settings` — exit 0 = content read (stored in shell var `YOSH_TEST_SETTINGS`), exit 1 = no settings file, exit 2 = error.

- [ ] **Step 1: Add the command to `tests/plugins/test_plugin/src/lib.rs`**

Add `"read-settings"` to the `commands()` array (after `"run-echo"`).

Add the match arm in `exec` (before the catch-all arm), reporting via `set_var` so the test can assert content without capturing stdout:

```rust
            "read-settings" => match yosh_plugin_sdk::read_settings() {
                Ok(Some(text)) => {
                    let _ = set_var("YOSH_TEST_SETTINGS", &text);
                    0
                }
                Ok(None) => {
                    let _ = set_var("YOSH_TEST_SETTINGS", "<none>");
                    1
                }
                Err(_) => 2,
            },
```

(`set_var` is already imported; `required_capabilities` already includes `Capability::VariablesWrite`. Do NOT add any new capability — proving settings needs none is the point.)

- [ ] **Step 2: Rebuild the test plugin wasm**

Run: `cargo component build -p test_plugin --target wasm32-wasip2 --release` (timeout 600000)
Expected: success. The component now imports `yosh:plugin/settings@0.2.1`, which the Task 4 linker provides.

- [ ] **Step 3: Write the integration tests in `tests/plugin.rs`**

Append at the end of the file (before any trailing helpers if present, otherwise at the bottom). Note the capability mask: `variables:write` only — no `files:*` — which proves the settings read is capability-free.

```rust
/// Settings API: a plugin reads its own settings.toml with ZERO files
/// capabilities. Only variables:write is granted (to report the
/// content back to the test); the read itself needs no grant.
#[test]
fn t_settings_read_without_any_files_capability() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_VARIABLES_WRITE;
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, allowed, &[])
        .expect("load test_plugin");

    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    std::fs::write(&settings, "greeting = \"hello\"\n").unwrap();
    test_helpers::set_settings_path_for_tests(&mut mgr, Some(settings));

    let exec = mgr.exec_command(&mut env, "read-settings", &[]);
    assert!(
        matches!(exec, PluginExec::Handled(0)),
        "read-settings must succeed with content, got {:?}",
        exec
    );
    assert_eq!(
        env.vars.get("YOSH_TEST_SETTINGS"),
        Some("greeting = \"hello\"\n"),
        "settings content must round-trip"
    );
}

/// Settings API: absent settings.toml is the normal case — read()
/// returns none (test plugin maps it to exit 1 + '<none>' sentinel).
#[test]
fn t_settings_read_missing_file_is_none() {
    let _g = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    test_helpers::load_plugin_with_caps(
        &mut mgr,
        &wasm,
        &mut env,
        yosh_plugin_api::CAP_VARIABLES_WRITE,
        &[],
    )
    .expect("load test_plugin");

    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("settings.toml"); // never created
    test_helpers::set_settings_path_for_tests(&mut mgr, Some(missing));

    let exec = mgr.exec_command(&mut env, "read-settings", &[]);
    assert!(
        matches!(exec, PluginExec::Handled(1)),
        "read-settings on a missing file must report none (exit 1), got {:?}",
        exec
    );
    assert_eq!(env.vars.get("YOSH_TEST_SETTINGS"), Some("<none>"));
}
```

If `env.vars.get` returns a type that doesn't compare directly with `Some("...")` (check how `t13` asserts: `assert_eq!(env.vars.get("YOSH_TEST_POST_EXEC_FIRED"), Some("0"))`), mirror t13's exact comparison style.

- [ ] **Step 4: Run the integration tests**

Run: `cargo test --features test-helpers --test plugin t_settings -- --nocapture` (timeout 600000)
Expected: both new tests pass.

- [ ] **Step 5: Run the whole plugin integration suite for regressions**

Run: `cargo test --features test-helpers --test plugin` (timeout 600000)
Expected: all pass. (Every other test grants explicit caps and never calls `read-settings`; the always-linked settings instance must not disturb them.)

- [ ] **Step 6: Commit**

```bash
git add tests/plugins/test_plugin/src/lib.rs tests/plugin.rs
git commit -m "test(plugin): cover capability-free settings read end-to-end

test_plugin gains a read-settings command (reports via shell var);
integration tests prove content round-trip and the missing-file none
path with only variables:write granted."
```

---

### Task 7: User & developer documentation

**Files:**
- Modify: `docs/yosh/plugin.md`

**Interfaces:**
- Consumes: final API shape from Tasks 2 and 5.
- Produces: documentation only.

- [ ] **Step 1: Document the settings file in the User Guide**

In `docs/yosh/plugin.md`, under `### Configuration` (near the `#### Restricting Capabilities` / `#### Confining files Access` subsections), add:

```markdown
#### Per-Plugin Settings File

Each plugin can read its own settings file at:

```
~/.config/yosh/plugins/<plugin-name>/settings.toml
```

Create the file to configure a plugin that supports it (consult the
plugin's README for its keys). Reading this file requires **no
capability** — it is always available to the plugin, read-only, and a
plugin can only ever see its own file, never another plugin's.
```

- [ ] **Step 2: Document the API in the Plugin Development Guide**

Under `### Plugin API Reference`, after the `#### I/O` subsection, add:

```markdown
#### Settings (no capability required)

```rust
// Contents of ~/.config/yosh/plugins/<your-plugin>/settings.toml.
// Ok(None) when the file doesn't exist. Raw TOML text — parse it
// yourself (the `toml` crate compiles to wasm32-wasip2).
pub fn read_settings() -> Result<Option<String>, ErrorCode>;
```

`settings` is the one capability-free host interface: do not add
anything to `required_capabilities` for it. Typical use is in
`on_load`. Note that `metadata()` runs before the shell binds the
environment, so calling `read_settings` there returns
`Err(ErrorCode::Denied)` like every other host call.
```

- [ ] **Step 3: Commit**

```bash
git add docs/yosh/plugin.md
git commit -m "docs(plugin): document per-plugin settings.toml and read_settings()"
```

---

### Task 8: Full verification pass

**Files:** none (verification only).

- [ ] **Step 1: Full unit + integration test run**

Run: `cargo test` (timeout 600000)
Expected: all pass.

- [ ] **Step 2: Plugin integration suite with helpers**

Run: `cargo test --features test-helpers` (timeout 600000)
Expected: all pass (this includes tests/plugin.rs; test_plugin/trap_plugin wasm artefacts were built in Task 6 / are rebuilt by `ensure_built`. If trap_plugin is stale, run `cargo component build -p trap_plugin --target wasm32-wasip2 --release` first).

- [ ] **Step 3: E2E POSIX suite (sanity — plugins don't affect it, but it's the session convention)**

Run: `./e2e/run_tests.sh` (timeout 600000; requires `cargo build` debug binary, which Step 1 produced)
Expected: all pass, 0 XFAIL.

- [ ] **Step 4: Confirm clean tree**

Run: `git status --short`
Expected: empty output. If anything is uncommitted, review and commit it with an appropriate message.
