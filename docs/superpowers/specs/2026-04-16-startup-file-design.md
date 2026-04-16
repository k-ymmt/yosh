# Startup File Support (~/.yoshrc + ENV)

## Summary

Add startup file support to yosh's interactive mode. When an interactive shell starts, it sources `~/.yoshrc` (yosh-specific) followed by the file specified by the `ENV` variable (POSIX-compliant).

## Scope

- **In scope:** `~/.yoshrc` auto-sourcing, `ENV` variable support, `Executor::source_file()` method
- **Out of scope:** Login shell (`-l`/`--login`), `~/.profile`, non-interactive startup files

## Startup Sequence

Interactive shell startup in `Repl::new()`, after plugin loading:

1. Source `~/.yoshrc` if it exists (silent skip if absent)
2. Read `$ENV`, expand it via parameter expansion, source the resulting file (if `ENV` is set)

## Design

### `Executor::source_file(path: &Path) -> Option<i32>`

New method on `Executor` in `src/exec/mod.rs`:

- Read file content with `std::fs::read_to_string()`
- If file doesn't exist, return `None`
- Set `env.mode.in_dot_script = true`
- Parse content as a shell program
- Execute in the current shell context (variables, functions, aliases persist)
- Consume `FlowControl::Return` (same as `builtin_source`)
- Restore `in_dot_script` flag
- Return `Some(exit_status)`

This extracts the core logic shared with `builtin_source` (the `.` builtin). The builtin adds PATH search and argument handling on top; `source_file` is the minimal version for known absolute paths.

### `ENV` Variable Expansion

POSIX requires parameter expansion on the `ENV` value before using it as a path. For example, `ENV=$HOME/.shinit` must expand `$HOME`.

Implementation:
1. Get raw `ENV` value from `self.env.vars`
2. Parse the value as a shell Word
3. Call `expand_word_to_string()` to perform parameter expansion
4. Pass the expanded path to `source_file()`

### Integration Point in `Repl::new()`

After `executor.load_plugins()` and before constructing `Self`:

```
// Source ~/.yoshrc
if let Some(home) = executor.env.vars.get("HOME") {
    let rc_path = PathBuf::from(home).join(".yoshrc");
    executor.source_file(&rc_path);  // None return = doesn't exist, silently skip
}

// Source $ENV (POSIX)
if let Some(env_val) = executor.env.vars.get("ENV") {
    // expand env_val via parameter expansion
    // source the expanded path, print error if file not found
}
```

### Error Handling

| Case | Behavior |
|------|----------|
| `~/.yoshrc` does not exist | Silent skip (return `None`) |
| `~/.yoshrc` parse error | Print error to stderr, continue shell startup |
| `~/.yoshrc` runtime error | Continue shell startup |
| `ENV` not set | Skip |
| `ENV` file does not exist | Print `yosh: <path>: No such file or directory` to stderr |
| `ENV` file parse error | Print error to stderr, continue shell startup |

### Source Order Rationale

`~/.yoshrc` before `ENV`: The yosh-specific config provides defaults; `ENV` (POSIX standard) can override. This follows the principle that more specific configuration loads last.

## Files Modified

- `src/exec/mod.rs` — Add `source_file()` method
- `src/interactive/mod.rs` — Call startup sourcing in `Repl::new()`

## Testing

- Unit test: `source_file` with a temp file containing variable assignments, verify variables are set
- Unit test: `source_file` with nonexistent path returns `None`
- E2E test: interactive shell with `~/.yoshrc` setting a variable, verify it's available
- E2E test: `ENV` variable pointing to a file, verify it's sourced after `~/.yoshrc`

## TODO.md Cleanup

Remove these completed items after implementation:
- Line 20: `~/.yoshrc` startup file — ENV variable support
- Line 45: `~/.yoshrc` plugin loading (partially addressed — plugins still load from `plugins.lock`, but `~/.yoshrc` can now configure the shell)
