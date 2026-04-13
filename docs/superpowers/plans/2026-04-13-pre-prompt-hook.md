# Pre-Prompt Hook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `pre_prompt` hook to the plugin system so plugins can set PS1 before each prompt display in interactive mode.

**Architecture:** A new `CAP_HOOK_PRE_PROMPT` capability flows through three layers: API (bitflag), SDK (enum + trait method + export macro), and host (symbol discovery + dispatch). The interactive REPL calls the hook before PS1 expansion on each iteration.

**Tech Stack:** Rust, libloading, kish-plugin-api/kish-plugin-sdk crates

---

### Task 1: Add `CAP_HOOK_PRE_PROMPT` to Plugin API

**Files:**
- Modify: `crates/kish-plugin-api/src/lib.rs:6-23`

- [ ] **Step 1: Add the capability constant and update CAP_ALL**

In `crates/kish-plugin-api/src/lib.rs`, add `CAP_HOOK_PRE_PROMPT` after `CAP_HOOK_ON_CD` and update `CAP_ALL`:

```rust
pub const CAP_HOOK_ON_CD: u32 = 0x40;
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x80;

/// All capability bits OR'd together.
pub const CAP_ALL: u32 = CAP_VARIABLES_READ
    | CAP_VARIABLES_WRITE
    | CAP_FILESYSTEM
    | CAP_IO
    | CAP_HOOK_PRE_EXEC
    | CAP_HOOK_POST_EXEC
    | CAP_HOOK_ON_CD
    | CAP_HOOK_PRE_PROMPT;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p kish-plugin-api`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add crates/kish-plugin-api/src/lib.rs
git commit -m "feat(plugin-api): add CAP_HOOK_PRE_PROMPT capability constant"
```

---

### Task 2: Add `HookPrePrompt` to Plugin SDK

**Files:**
- Modify: `crates/kish-plugin-sdk/src/lib.rs:4-28` (Capability enum + to_bitflag)
- Modify: `crates/kish-plugin-sdk/src/lib.rs:38-67` (Plugin trait)
- Modify: `crates/kish-plugin-sdk/src/lib.rs:163-328` (export! macro)

- [ ] **Step 1: Add `HookPrePrompt` variant to Capability enum**

In `crates/kish-plugin-sdk/src/lib.rs`, add the variant to the enum (after `HookOnCd`):

```rust
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
```

And add the match arm in `to_bitflag`:

```rust
Capability::HookPrePrompt => ffi::CAP_HOOK_PRE_PROMPT,
```

- [ ] **Step 2: Add `hook_pre_prompt` to the Plugin trait**

Add the method after `hook_on_cd` (around line 63):

```rust
    /// Hook: called before the interactive prompt is displayed.
    fn hook_pre_prompt(&mut self, _api: &PluginApi) {}
```

- [ ] **Step 3: Add FFI export to the `export!` macro**

Add the following block after the `kish_plugin_hook_on_cd` export function (after line 314), before `kish_plugin_destroy`:

```rust
        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn kish_plugin_hook_pre_prompt(
            api: *const $crate::ffi::HostApi,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                if let Some(p) = plugin.as_mut() {
                    $crate::Plugin::hook_pre_prompt(p, &plugin_api);
                }
            }));
        }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p kish-plugin-sdk`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add crates/kish-plugin-sdk/src/lib.rs
git commit -m "feat(plugin-sdk): add HookPrePrompt capability, trait method, and export macro"
```

---

### Task 3: Add `hook_pre_prompt` to test plugin

**Files:**
- Modify: `tests/plugins/test_plugin/src/lib.rs:1-56`

- [ ] **Step 1: Add `HookPrePrompt` to required capabilities**

In `tests/plugins/test_plugin/src/lib.rs`, add `Capability::HookPrePrompt` to the `required_capabilities` list:

```rust
    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
        ]
    }
```

- [ ] **Step 2: Implement `hook_pre_prompt`**

Add after the `hook_on_cd` method:

```rust
    fn hook_pre_prompt(&mut self, api: &PluginApi) {
        let _ = api.set_var("TEST_PRE_PROMPT", "1");
    }
```

- [ ] **Step 3: Verify the test plugin compiles**

Run: `cargo build --manifest-path tests/plugins/test_plugin/Cargo.toml`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add tests/plugins/test_plugin/src/lib.rs
git commit -m "test(plugin): add hook_pre_prompt to test plugin"
```

---

### Task 4: Add `call_pre_prompt` to PluginManager

**Files:**
- Modify: `src/plugin/mod.rs:14-23` (LoadedPlugin struct)
- Modify: `src/plugin/mod.rs:152-168` (plugin loading, symbol check)
- Modify: `src/plugin/mod.rs:174-193` (log_denied_capabilities)
- Modify: `src/plugin/mod.rs:232-315` (hook dispatch methods — add new one after call_on_cd)

- [ ] **Step 1: Write the failing test for hook dispatch**

In `tests/plugin.rs`, add after the `hook_on_cd` test (after line 119):

```rust
#[test]
fn hook_pre_prompt() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_pre_prompt(&mut env);
    assert_eq!(env.vars.get("TEST_PRE_PROMPT"), Some("1"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test plugin hook_pre_prompt`
Expected: FAIL — `call_pre_prompt` method does not exist

- [ ] **Step 3: Add `has_pre_prompt` field to `LoadedPlugin`**

In `src/plugin/mod.rs`, add the field to the struct (after `has_on_cd`):

```rust
struct LoadedPlugin {
    name: String,
    #[allow(dead_code)]
    library: libloading::Library,
    commands: Vec<String>,
    capabilities: u32,
    has_pre_exec: bool,
    has_post_exec: bool,
    has_on_cd: bool,
    has_pre_prompt: bool,
}
```

- [ ] **Step 4: Discover the symbol during plugin loading**

In `src/plugin/mod.rs`, add the symbol check after `has_on_cd` detection (after line 158):

```rust
        let has_pre_prompt =
            unsafe { library.get::<*const ()>(b"kish_plugin_hook_pre_prompt").is_ok() };
```

And add `has_pre_prompt` to the `LoadedPlugin` struct construction (around line 160-168):

```rust
        self.plugins.push(LoadedPlugin {
            name,
            library,
            commands,
            capabilities: effective_capabilities,
            has_pre_exec,
            has_post_exec,
            has_on_cd,
            has_pre_prompt,
        });
```

- [ ] **Step 5: Add `CAP_HOOK_PRE_PROMPT` to `log_denied_capabilities`**

In `src/plugin/mod.rs`, add the entry to the `caps` array in `log_denied_capabilities` (after the `CAP_HOOK_ON_CD` entry):

```rust
            (CAP_HOOK_PRE_PROMPT, "hooks:pre_prompt"),
```

Also add the import: change `use kish_plugin_api::*;` is already there so just add the entry.

- [ ] **Step 6: Implement `call_pre_prompt` dispatch method**

In `src/plugin/mod.rs`, add after the `call_on_cd` method (after line 315):

```rust
    /// Call pre_prompt hook on all plugins that have it.
    pub fn call_pre_prompt(&self, env: &mut ShellEnv) {
        for plugin in &self.plugins {
            if !plugin.has_pre_prompt {
                continue;
            }
            if plugin.capabilities & kish_plugin_api::CAP_HOOK_PRE_PROMPT == 0 {
                continue;
            }
            let mut ctx = HostContext::new(env, &plugin.name);
            let mut api = build_host_api(plugin.capabilities);
            api.ctx = &mut ctx as *mut HostContext as *mut c_void;
            unsafe {
                if let Ok(hook_fn) = plugin.library.get::<
                    unsafe extern "C" fn(*const HostApi),
                >(b"kish_plugin_hook_pre_prompt")
                {
                    hook_fn(&api);
                }
            }
        }
    }
```

- [ ] **Step 7: Run the test to verify it passes**

Run: `cargo test --test plugin hook_pre_prompt`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/plugin/mod.rs tests/plugin.rs
git commit -m "feat(plugin): add call_pre_prompt dispatch to PluginManager"
```

---

### Task 5: Add capability enforcement tests for `pre_prompt`

**Files:**
- Modify: `tests/plugin.rs`

- [ ] **Step 1: Write the test for capability denial**

In `tests/plugin.rs`, add after the `sandbox_hook_not_fired_without_capability` test (after line 201). Extend the existing test to also check `pre_prompt`:

```rust
#[test]
fn sandbox_pre_prompt_not_fired_without_capability() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Load with variables:write + io but NO hook capabilities
    let caps = kish_plugin_api::CAP_VARIABLES_READ
        | kish_plugin_api::CAP_VARIABLES_WRITE
        | kish_plugin_api::CAP_IO;
    manager
        .load_plugin_with_capabilities(&dylib, &mut env, Some(caps))
        .unwrap();

    // pre_prompt hook should not fire
    manager.call_pre_prompt(&mut env);
    assert_eq!(env.vars.get("TEST_PRE_PROMPT"), None);
}
```

- [ ] **Step 2: Write the test for selective capability grant**

```rust
#[test]
fn sandbox_selective_pre_prompt_capability() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Grant variables:write (for hook to set_var) + only pre_prompt hook
    let caps = kish_plugin_api::CAP_VARIABLES_READ
        | kish_plugin_api::CAP_VARIABLES_WRITE
        | kish_plugin_api::CAP_IO
        | kish_plugin_api::CAP_HOOK_PRE_PROMPT;
    manager
        .load_plugin_with_capabilities(&dylib, &mut env, Some(caps))
        .unwrap();

    // pre_prompt should fire
    manager.call_pre_prompt(&mut env);
    assert_eq!(env.vars.get("TEST_PRE_PROMPT"), Some("1"));

    // pre_exec should NOT fire (not granted)
    manager.call_pre_exec(&mut env, "echo hello");
    assert_eq!(env.vars.get("TEST_PRE_EXEC"), None);
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --test plugin sandbox_pre_prompt`
Expected: both tests PASS

- [ ] **Step 4: Commit**

```bash
git add tests/plugin.rs
git commit -m "test(plugin): add sandbox enforcement tests for pre_prompt hook"
```

---

### Task 6: Add `hooks:pre_prompt` to config parser

**Files:**
- Modify: `src/plugin/config.rs:42-54` (capability_from_str)

- [ ] **Step 1: Write the failing test**

In `src/plugin/config.rs`, in the existing `parse_capability_string_to_bitflags` test (line 179), add after the `hooks:on_cd` assertion:

```rust
        assert_eq!(
            capability_from_str("hooks:pre_prompt"),
            Some(CAP_HOOK_PRE_PROMPT)
        );
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib plugin::config::tests::parse_capability_string_to_bitflags`
Expected: FAIL — `capability_from_str("hooks:pre_prompt")` returns `None`

- [ ] **Step 3: Add the mapping**

In `src/plugin/config.rs`, add to `capability_from_str` match (after the `hooks:on_cd` arm):

```rust
        "hooks:pre_prompt" => Some(kish_plugin_api::CAP_HOOK_PRE_PROMPT),
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib plugin::config::tests::parse_capability_string_to_bitflags`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/plugin/config.rs
git commit -m "feat(plugin-config): add hooks:pre_prompt capability string mapping"
```

---

### Task 7: Integrate `call_pre_prompt` into the interactive REPL

**Files:**
- Modify: `src/interactive/mod.rs:69-80`

- [ ] **Step 1: Add the hook call before prompt expansion**

In `src/interactive/mod.rs`, add the `call_pre_prompt` call after job notification display and before prompt expansion. Replace lines 74-76:

```rust
            // Choose PS1 or PS2
            let prompt_var = if input_buffer.is_empty() { "PS1" } else { "PS2" };
            let prompt = expand_prompt(&mut self.executor.env, prompt_var);
```

with:

```rust
            // Fire pre_prompt hook for PS1 (not PS2 continuation)
            if input_buffer.is_empty() {
                self.executor.plugins.call_pre_prompt(&mut self.executor.env);
            }

            // Choose PS1 or PS2
            let prompt_var = if input_buffer.is_empty() { "PS1" } else { "PS2" };
            let prompt = expand_prompt(&mut self.executor.env, prompt_var);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Run all plugin tests to verify no regressions**

Run: `cargo test --test plugin`
Expected: all tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "feat(interactive): call pre_prompt hook before PS1 expansion"
```

---

### Task 8: Update TODO.md and run full test suite

**Files:**
- Modify: `TODO.md:43`

- [ ] **Step 1: Remove the completed TODO item**

In `TODO.md`, delete line 43:

```
- [ ] `pre_prompt` hook — fire before prompt display in interactive mode; hook infrastructure is ready (`src/plugin/mod.rs`)
```

- [ ] **Step 2: Run the full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed pre_prompt hook item"
```
