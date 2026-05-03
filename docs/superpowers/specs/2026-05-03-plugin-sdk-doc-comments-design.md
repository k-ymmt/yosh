# `yosh-plugin-sdk` public API doc comments

**Date:** 2026-05-03
**Type:** Documentation (no behavior change)
**Scope:** `crates/yosh-plugin-sdk/src/lib.rs`

## Background

The `yosh-plugin-sdk` crate is the primary touchpoint for plugin authors.
Its `lib.rs` re-exports the WIT-generated host-import bindings under stable
names and adds typed Rust helper wrappers (`get_var`, `read_file`,
`write_string`, `exists`, ...). It also defines the `Plugin` trait every
plugin must implement.

Today every public helper in `lib.rs` is undocumented except `exec()`.
`Plugin` trait methods carry only a one-line comment on `implemented_hooks`.
Several behaviors are non-obvious and easy to misuse — they were called
out as a code-review follow-up from the 2026-04-29 `files-rw` branch and
recorded in `TODO.md`:

- **(a) `exists(path)`** returns `false` not just for missing files but
  ALSO for `Denied` (capability not granted) and any I/O error. The
  caller cannot distinguish these.
- **(b) `read_to_string(path)`** collapses `String::from_utf8` failure
  onto `ErrorCode::InvalidArgument` — the same code the host layer uses
  for empty-path errors. A plugin author calling `read_to_string` on a
  binary file could mistake it for "I passed a bad path".
- **(c) `files:write` family** lets the plugin clobber any user-writable
  file. The spec calls this out in its threat model, but the SDK surface
  doesn't surface it to consumers.

## Goals

- Add rustdoc to every public function in `lib.rs`, mirroring `std::fs`
  documentation style.
- Document each `Plugin` trait method.
- Surface the three subtle behaviors (a)/(b)/(c) above as inline hazard
  notes on the affected functions, not just buried in module-level prose.
- Make capability requirements explicit on every host-import wrapper so
  authors know which capability to declare in `required_capabilities`.

## Non-goals

- **Behavior changes.** This is a docs-only patch.
- **`# Examples` sections.** Host-context-dependent examples are
  nontrivial to write correctly and add maintenance cost. Skip them;
  the WIT and host implementation already serve as ground truth.
- **`crates/yosh-plugin-sdk/src/style.rs`.** TODO.md scopes the work
  to `lib.rs`. `style.rs` has its own gaps but is out of scope here.
- **`crates/yosh-plugin-sdk/src/export.rs`.** Internal macro plumbing,
  not user-authored API surface.
- **WIT-generated re-exports** (`FileStat`, `DirEntry`, `ExecOutput`,
  `IoStream`, `ErrorCode`, `HookName`, `PluginInfo`). Doc strings for
  these belong in `crates/yosh-plugin-api/wit/yosh-plugin.wit`. Restoring
  WIT inline comments is its own TODO item and is tracked separately.

## Design

### Documentation template

Every public helper gets, in order:

1. A one-line summary (imperative voice, ends in a period).
2. A short paragraph if behavior is non-obvious. Include the **Capability**
   required (e.g. "Requires `variables:read`").
3. A `# Errors` section listing every `ErrorCode` variant the function
   can return, one bullet per variant, with the condition.

The existing `exec()` doc (`lib.rs:164-180`) is the reference template.

### Helper coverage

For each function below, list the variants of `ErrorCode` the
implementation can produce (cross-referenced against `src/plugin/host.rs`).

#### `variables` group

| Function | Capability | `# Errors` |
|---|---|---|
| `get_var(name)` | `variables:read` | `Denied` |
| `set_var(name, value)` | `variables:write` | `Denied`, `IoFailed` |
| `export_var(name, value)` | `variables:write` | `Denied`, `IoFailed` |

`get_var` returns `Result<Option<String>, _>`: `Ok(None)` means the
variable is unset, `Ok(Some(""))` means it's set to the empty string.
This distinction matters and goes in the summary paragraph.

#### `filesystem` group

The `filesystem` capability is **not** split into read/write — it is a
single coarse capability covering both `cwd()` and `set_cwd()`.

| Function | Capability | `# Errors` |
|---|---|---|
| `cwd()` | `filesystem` | `Denied`, `IoFailed` |
| `set_cwd(path)` | `filesystem` | `Denied`, `IoFailed` |

#### `io` group

The `io` capability is also a single capability (no `io:write`/`io:read`
split today; the only host-import in this group is `write`).

| Function | Capability | `# Errors` |
|---|---|---|
| `print(s)` | `io` | `Denied`, `IoFailed` |
| `eprint(s)` | `io` | `Denied`, `IoFailed` |
| `write_bytes(stream, data)` | `io` | `Denied`, `IoFailed` |

`print`/`eprint` are UTF-8 string convenience over `write_bytes`.

#### `files:read` group

| Function | Capability | `# Errors` |
|---|---|---|
| `read_file(path)` | `files:read` | `Denied`, `InvalidArgument` (empty path), `NotFound`, `IoFailed` |
| `read_to_string(path)` | `files:read` | as above, plus `InvalidArgument` for non-UTF-8 content (**hazard (b)**) |
| `read_dir(path)` | `files:read` | `Denied`, `InvalidArgument`, `NotFound`, `IoFailed` |
| `metadata(path)` | `files:read` | `Denied`, `InvalidArgument`, `NotFound`, `IoFailed` |
| `exists(path)` | `files:read` | none — returns `bool` (**hazard (a)**) |

`exists` doc must say: _"Returns `false` if the path is missing, the
capability is not granted, or any I/O error occurs. This function cannot
distinguish those cases. If you need to tell `Denied` apart from "really
not there", call `metadata()` directly and inspect the `Err` variant."_

`read_to_string` doc must say: _"Reads the file and decodes as UTF-8.
Returns `Err(ErrorCode::InvalidArgument)` if the bytes are not valid
UTF-8. Note: the host also returns `InvalidArgument` for an empty path,
so a plugin cannot distinguish "binary content" from "bad path" through
the error code alone. For binary files, prefer `read_file()` and decode
explicitly."_

`metadata().is_symlink` is currently always `false` because the host
calls `std::fs::metadata`, which follows symlinks (`src/plugin/host.rs`
`host_files_metadata`). `read_dir()` entries are different — `DirEntry`
uses `symlink_metadata`-equivalent semantics, so `DirEntry::is_symlink`
*does* correctly detect symlinks. Document this asymmetry on `metadata()`
specifically, not on `read_dir()`. WIT-comment restoration is tracked
as a separate TODO.

#### `files:write` group

All `files:write` helpers share a common hazard. Add a **Sandbox note**
to every one of them:

> _"`files:write` operations have no path allowlist or sandboxing
> beyond the capability grant itself. Once granted, the plugin can
> write to (or delete) any path the host process can. Treat the
> capability grant as the entire access boundary."_

| Function | `# Errors` |
|---|---|
| `write_file(path, data)` | `Denied`, `InvalidArgument`, `IoFailed` |
| `write_string(path, s)` | as above |
| `append_file(path, data)` | `Denied`, `InvalidArgument`, `IoFailed` |
| `create_dir(path)` | `Denied`, `InvalidArgument`, `IoFailed` |
| `create_dir_all(path)` | `Denied`, `InvalidArgument`, `IoFailed` |
| `remove_file(path)` | `Denied`, `InvalidArgument`, `NotFound`, `IoFailed` |
| `remove_dir(path)` | `Denied`, `InvalidArgument`, `NotFound`, `IoFailed` |
| `remove_dir_all(path)` | `Denied`, `InvalidArgument`, `NotFound`, `IoFailed` |

**Asymmetry note (already a TODO):** `write_file`/`append_file`/
`create_dir` collapse `NotFound` (e.g. parent dir missing) into
`IoFailed` rather than mapping to `ErrorCode::NotFound` like the read
side does. The doc on each write helper notes this so authors don't
expect a parent-dir-not-found distinction.

#### `commands:exec` group

| Function | Doc status |
|---|---|
| `exec(program, args)` | already documented; leave as-is and use as the template reference |

### `Plugin` trait

Add doc comments to:

- `Plugin` trait itself (already has a one-liner; expand with the
  contract: must be `Default + Send + 'static`, gets instantiated once
  per load).
- `commands()` — list of subcommand names dispatched to `exec()`.
- `required_capabilities()` — declarative capability list. Default `&[]`.
- `implemented_hooks()` — keep existing comment (already explains the
  default-method-detection limitation).
- `on_load()` — invoked once after capability checks pass.
- `exec(command, args)` — the entry point for every dispatched subcommand.
- `hook_pre_exec` / `hook_post_exec` / `hook_on_cd` / `hook_pre_prompt` —
  each gets one line describing when it fires and the arguments.
- `on_unload()` — best-effort cleanup; not guaranteed to run on host
  crash.

### `metadata()` host-import restriction

The WIT comment on `plugin-info` (yosh-plugin.wit:30-36) states that
`metadata` is the only plugin export the host calls without an active
`ShellEnv` binding, and any host import from inside it returns
`error-code::denied`. This affects how `commands()`,
`required_capabilities()`, and `implemented_hooks()` may be implemented:
they are called as part of `metadata()` synthesis on the guest side
and must be pure functions of `&self`.

Add a brief note in the `Plugin` trait doc (not on each method) so
authors don't try to read env vars to compute the command list.

## Verification

- `cargo build -p yosh-plugin-sdk` — no new warnings.
- `cargo doc -p yosh-plugin-sdk --no-deps` — generates without errors.
  Spot-check the rendered HTML to confirm hazard notes render as
  expected and `# Errors` sections appear under each fn signature.
- `cargo test` — full suite (no behavior changes expected, but run the
  full suite per CLAUDE.md).
- `cargo clippy --all-targets -- -D warnings` (workspace-wide) — confirm
  no `missing_docs`-adjacent regressions.

## File-level summary

Only `crates/yosh-plugin-sdk/src/lib.rs` is touched. No other files,
no Cargo.toml changes, no API surface changes.

## Risks

- **None functional.** Docs-only.
- **Drift.** Doc comments can desynchronize from host behavior over
  time. Mitigation: cite WIT/host file paths inline so future readers
  know where to verify. Since `host.rs` and the WIT are checked into
  the same repo, drift detection is one `grep` away.
