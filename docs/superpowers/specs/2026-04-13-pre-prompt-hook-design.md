# Pre-Prompt Hook Design

## Overview

Add a `pre_prompt` hook to kish's plugin system, enabling plugins to modify PS1 before the prompt is displayed. This allows plugins to implement rich prompts (like starship) by setting PS1 via the existing `set_var` API.

## Motivation

The plugin system currently has three hooks: `pre_exec`, `post_exec`, and `on_cd`. None of these fire reliably before every prompt display. Specifically:

- **Shell startup**: No command has been executed, so `post_exec` never fires for the first prompt.
- **Empty Enter**: The REPL `continue`s without executing a command.
- **Syntax errors**: The input is rejected before reaching command execution.

A `pre_prompt` hook fires in the interactive loop immediately before prompt expansion, covering all these cases.

## Design Decisions

1. **Void return, no arguments**: `hook_pre_prompt(&mut self, api: &PluginApi)` — consistent with existing hooks. The plugin uses `api.set_var("PS1", ...)` to set the prompt.
2. **PS1 only**: The hook fires only for PS1 (primary prompt), not PS2 (continuation prompt).
3. **Information via PluginApi**: The plugin retrieves context (exit code, cwd, etc.) through `api.get_var()` rather than receiving explicit arguments. This keeps the API stable.

## Scope

### In scope

- `CAP_HOOK_PRE_PROMPT` capability constant (API, SDK, config)
- `hook_pre_prompt` method on `Plugin` trait (SDK)
- `call_pre_prompt` on `PluginManager` (host)
- Hook invocation in interactive loop before prompt display
- `export!` macro update
- Tests (hook dispatch, capability enforcement)

### Out of scope

- Actual prompt plugin implementation (e.g., git branch, language version display)
- Right prompt (RPROMPT) support
- Command duration measurement
- Transient prompt (replace previous prompt after command execution)

## Implementation

### 1. API Layer (`crates/kish-plugin-api/src/lib.rs`)

Add capability constant:

```rust
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x80;
```

Update `CAP_ALL`:

```rust
pub const CAP_ALL: u32 = 0xFF; // was 0x7F
```

No changes to `HostApi` or `PluginDecl` structs. The hook function is discovered via `dlsym` at load time, same as existing hooks.

### 2. SDK Layer (`crates/kish-plugin-sdk/src/lib.rs`)

Add variant to `Capability` enum:

```rust
pub enum Capability {
    // ... existing variants ...
    HookPrePrompt,
}
```

Update `Capability::to_flag()`:

```rust
Capability::HookPrePrompt => kish_plugin_api::CAP_HOOK_PRE_PROMPT,
```

Add trait method with default implementation:

```rust
pub trait Plugin: Send + Default {
    // ... existing methods ...
    fn hook_pre_prompt(&mut self, _api: &PluginApi) {}
}
```

Add FFI export in `export!` macro:

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kish_plugin_hook_pre_prompt() {
    // Same pattern as hook_pre_exec: lock plugin, call hook_pre_prompt
}
```

### 3. Host Layer (`src/plugin/mod.rs`)

Add field to `LoadedPlugin`:

```rust
pub struct LoadedPlugin {
    // ... existing fields ...
    pub has_pre_prompt: bool,
}
```

During plugin loading, check for the symbol:

```rust
let has_pre_prompt = unsafe { lib.get::<Symbol<unsafe extern "C" fn()>>(b"kish_plugin_hook_pre_prompt") }.is_ok();
```

Add dispatch method:

```rust
pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
    for plugin in &self.plugins {
        if !plugin.has_pre_prompt { continue; }
        if plugin.capabilities & CAP_HOOK_PRE_PROMPT == 0 { continue; }
        // Update HostContext env pointer, call the function
    }
}
```

Add `deny_pre_prompt` function (for sandbox enforcement when capability not granted).

### 4. Config Layer (`src/plugin/config.rs`)

Add mapping in `capability_from_str`:

```rust
"hooks:pre_prompt" => Some(CAP_HOOK_PRE_PROMPT),
```

Example config:

```toml
[[plugin]]
name = "my-prompt"
path = "~/.kish/plugins/libmy_prompt.dylib"
capabilities = ["variables:read", "variables:write", "hooks:pre_prompt"]
```

### 5. Integration (`src/interactive/mod.rs`)

In the REPL loop, before prompt expansion, call the hook for PS1 only:

```rust
if input_buffer.is_empty() {
    self.executor.plugins.call_pre_prompt(&mut self.executor.env);
}
let prompt = expand_prompt(&mut self.executor.env, prompt_var);
```

### 6. Tests

Following existing patterns in `tests/plugin.rs`:

1. **Hook dispatch test**: Load test plugin with `CAP_HOOK_PRE_PROMPT`, call `call_pre_prompt`, verify the plugin was invoked (via a variable it sets).
2. **Capability denial test**: Load plugin without `CAP_HOOK_PRE_PROMPT`, verify hook does not fire.
3. **Test plugin update**: Add `hook_pre_prompt` implementation that sets `PRE_PROMPT_CALLED=1`.

## File Changes Summary

| File | Change |
|------|--------|
| `crates/kish-plugin-api/src/lib.rs` | Add `CAP_HOOK_PRE_PROMPT`, update `CAP_ALL` |
| `crates/kish-plugin-sdk/src/lib.rs` | Add `HookPrePrompt` variant, trait method, `export!` macro |
| `src/plugin/mod.rs` | Add `has_pre_prompt`, `call_pre_prompt`, deny function |
| `src/plugin/config.rs` | Add `"hooks:pre_prompt"` mapping |
| `src/interactive/mod.rs` | Call `call_pre_prompt` before prompt display |
| `tests/plugins/test_plugin/src/lib.rs` | Add `hook_pre_prompt` implementation |
| `tests/plugin.rs` | Add dispatch and capability tests |
