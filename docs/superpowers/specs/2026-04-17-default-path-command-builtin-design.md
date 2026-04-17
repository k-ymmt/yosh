# Default PATH and `command` Builtin — Design

Date: 2026-04-17
Status: Approved for plan writing

## Motivation

POSIX IEEE Std 1003.1-2017 Vol.3 Ch.8 states that when `PATH` is unset or null, path search behavior is **implementation-defined**. The recommended default value is the one returned by `confstr(_CS_PATH)` — defined as "a value for `PATH` that finds all standard utilities." POSIX also requires the shell to provide a `command` builtin, whose `-p` flag specifically uses this default PATH.

yosh currently:

- Does not set a default PATH at shell startup.
- Delegates command lookup to libc `execvp()`, inheriting the process's `PATH`.
- Does not implement the `command` builtin at all.
- Has `src/exec/command.rs` `find_in_path()` marked `#[allow(dead_code)]` (only used in tests).

This design adds both a startup-time default PATH fallback and a full POSIX `command` builtin (`-p` / `-v` / `-V`).

## Research Summary

Verified empirically on macOS 25.3 with `env -i <shell> --no-rc`:

| Shell | PATH when unset | Source |
|---|---|---|
| bash 3.2 (Apple) | `/usr/gnu/bin:/usr/local/bin:/bin:/usr/bin:.` | `DEFAULT_PATH_VALUE` in `config-top.h` |
| bash upstream | `/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin:.` | compile-time constant |
| zsh 5.9 | `/bin:/usr/bin:/usr/ucb:/usr/local/bin` | compile-time constant |
| fish 4.2 | `/usr/bin:/bin:/usr/sbin:/sbin` | `confstr(_CS_PATH)` at runtime |
| macOS `getconf PATH` | `/usr/bin:/bin:/usr/sbin:/sbin` | POSIX recommendation |

**Chosen strategy: fish-style.** Call `libc::confstr(_CS_PATH)` at runtime. This is the strictest POSIX interpretation: the shell uses whatever the OS declares as the POSIX-guaranteed PATH.

## Decisions

1. **Scope** — Implement both `command -p` and PATH-unset startup fallback.
2. **Default PATH source** — Always `libc::confstr(_CS_PATH)`, with `/bin:/usr/bin` as fallback when `confstr` fails.
3. **`command` scope** — Full POSIX `command` with `-p`, `-v`, `-V`, plus the no-flag function-skip form.
4. **When to apply fallback** — At shell startup (bash/zsh/fish style): if env has no `PATH`, yosh sets its own `PATH` variable exported.
5. **Tests must pass on both macOS and Linux** — no hardcoded OS-specific PATH values in assertions.

## §1 Architecture

```
main.rs startup sequence
  └─ ShellEnv::from_environ()               [existing]
  └─ ensure_default_path(&mut env)          [NEW]
       └─ early-exit if env.get_var("PATH").is_some()
       └─ else: env.set_var("PATH", default_path(&env)); env.mark_exported("PATH")

src/env/default_path.rs (NEW)
  pub fn default_path(env: &ShellEnv) -> &str
    - cached via ShellEnv::default_path_cache: OnceLock<String>
    - first call: call_confstr().unwrap_or_else(fallback_default_path)
  fn call_confstr() -> Option<String>
    - unsafe libc::confstr(_CS_PATH, ...)
  pub fn fallback_default_path() -> String
    - returns "/bin:/usr/bin" (pure, test-friendly)

ShellEnv (existing) — add:
  default_path_cache: OnceLock<String>

src/builtin/command.rs (NEW)
  pub fn cmd_command(env, args) -> ExitStatus
    - flag parse (-p / -v / -V / --)
    - dispatches to resolve-only (-v/-V) or execute paths

src/builtin/resolve.rs (NEW)
  pub enum CommandKind {
      Alias(String),
      Function,
      Builtin(BuiltinClass),  // Special | Regular
      Keyword,
      External(PathBuf),
      NotFound,
  }
  pub fn resolve_command_kind(env, name) -> CommandKind
    - walks (matches bash reporting order):
      alias → keyword → function → builtin (Special → Regular) → PATH
```

### Interaction with existing code

| Call site | Existing | Change |
|---|---|---|
| `main.rs` startup | `ShellEnv::from_environ()` | Add one call to `ensure_default_path()` |
| `exec/simple.rs` | `execvp()` | No change — PATH is in env, execvp works as before |
| `builtin/mod.rs` regular registry | — | Register `command` |
| `exec/command.rs` `find_in_path` | `#[allow(dead_code)]` | Remove attribute — used by `command -p/-v/-V` |

### Future extensibility

- `type` builtin — one call to `resolve_command_kind()`; can be added in a later task.
- `which` (non-POSIX extension) — reuses `resolve_command_kind()` + `External` variant.
- `hash` builtin — future cache can wrap `find_in_path`; current single-entry-point design keeps this grafting trivial.
- `PATH=v cmd` inline assignment unification — the no-flag `command cmd` path should be structured so it can later share a `with_temporary_path_override()` helper with `PATH=v cmd`.

### Performance

- `confstr` is a libc compile-time constant copy — microseconds, called once thanks to `OnceLock`.
- Startup early-exit: if `PATH` is already in environ (the common case), `confstr` is never called.
- `find_in_path` is used only for `command -p/-v/-V`, so its linear syscall cost is acceptable.
- No change to the existing hot path (regular external command execution via `execvp`).

## §2 Data Flow

### Flow A: Startup PATH initialization

```
main()
  └─ ShellEnv::from_environ()
  └─ ensure_default_path(&mut env):
       ├─ env.get_var("PATH").is_some()? → return (zero-cost on common path)
       └─ else:
            ├─ let p = default_path(&env).to_string();
            ├─ env.set_var("PATH", p);
            └─ env.mark_exported("PATH");
```

### Flow B: `command -p cmd args...`

```
cmd_command(env, args)
  ├─ parse_flags(args) → { p: true, v: false, V: false, cmd: "ls", rest: [...] }
  └─ if p:
       ├─ let dp = default_path(env);
       ├─ find_in_path("ls", dp) → Some("/bin/ls")
       └─ execvp("/bin/ls", rest)   // absolute path, no further PATH search
```

### Flow C: `command -v cmd` / `command -V cmd`

```
cmd_command(env, args)
  └─ if v or V:
       └─ let kind = resolve_command_kind(env, "cmd");
       └─ -v: concise output
            Alias(val)       → "alias cmd='val'"
            Function         → "cmd"
            Keyword          → "cmd"
            Builtin(_)       → "cmd"
            External(p)      → "/bin/cmd"
            NotFound         → (no output, exit 1)
       └─ -V: verbose output
            Alias(val)       → "cmd is aliased to 'val'"
            Function         → "cmd is a function"
            Keyword          → "cmd is a shell keyword"
            Builtin(Special) → "cmd is a special shell builtin"
            Builtin(Regular) → "cmd is a shell builtin"
            External(p)      → "cmd is /bin/cmd"
            NotFound         → stderr "yosh: command: cmd: not found", exit 1
```

### Flow D: `command cmd args...` (no flags, skip function lookup)

```
cmd_command(env, args)
  └─ no flags:
       └─ run_without_function_lookup(env, "cmd", rest):
            ├─ check builtin registry (special + regular) → run builtin
            └─ else find_in_path(env.path) → execvp
       (Deliberately skips aliases and functions; matches bash/zsh behavior.
        Reserved keywords are NOT considered here — at execution time "if"
        is just a name, not a parser construct, so it falls through to the
        external-command path.)
```

## §3 Error Handling

### `confstr(_CS_PATH)` failure

```rust
fn call_confstr() -> Option<String> {
    let n = unsafe { libc::confstr(libc::_CS_PATH, ptr::null_mut(), 0) };
    if n == 0 { return None; }
    let mut buf = vec![0u8; n];
    let written = unsafe { libc::confstr(libc::_CS_PATH, buf.as_mut_ptr().cast(), n) };
    if written == 0 || written > n { return None; }
    buf.truncate(written.saturating_sub(1));
    String::from_utf8(buf).ok()
}

pub fn default_path(env: &ShellEnv) -> &str {
    env.default_path_cache.get_or_init(|| {
        call_confstr().unwrap_or_else(fallback_default_path)
    })
}

pub fn fallback_default_path() -> String { "/bin:/usr/bin".to_string() }
```

- Failure silently falls back to `/bin:/usr/bin`.
- Never panics.
- Never writes a warning (POSIX: unset PATH is implementation-defined, no user visibility required).

### `command -v` / `-V` for unknown name

| Flag | stdout | stderr | exit |
|---|---|---|---|
| `-v cmd` not found | (nothing) | (nothing) | 1 |
| `-V cmd` not found | (nothing) | `yosh: command: cmd: not found` | 1 |

### `command -p cmd` not found / not executable

- Not found → `yosh: command: cmd: not found` (stderr), exit 127
- Not executable → exit 126
- Matches yosh's existing exit-code policy (CLAUDE.md).

### Invalid flags

- `command -x` → `yosh: command: -x: invalid option` (stderr), exit 2
- `-p` + `-v` combined: allowed (POSIX does not disallow combining).

### `ShellEnv::default_path_cache`

- `OnceLock::get_or_init` requires the initializer to return a value; the initializer always succeeds via the fallback, so no poisoning.
- Thread-safe should yosh grow concurrent lookups in the future.

## §4 Testing Strategy (macOS / Linux parity)

### Level 1 — pure logic (OS-independent)

Covered by standard `#[test]` unit tests with fixtures:

- Flag parser (`-p`, `-v`, `-V`, `--`, combined forms)
- `resolve_command_kind` branch dispatch (alias fixture → Alias, function fixture → Function, etc.)
- `-v` / `-V` output formatting per `CommandKind` variant
- Error message formatting

### Level 2 — `default_path()` behavior (OS-dependent, structural asserts)

**Never do:**
```rust
assert_eq!(default_path(&env), "/usr/bin:/bin:/usr/sbin:/sbin"); // macOS-specific
```

**Instead:**
```rust
#[test]
fn default_path_is_non_empty() {
    let env = ShellEnv::default();
    assert!(!default_path(&env).is_empty());
}

#[test]
fn default_path_contains_bin_or_usr_bin() {
    let env = ShellEnv::default();
    let dp = default_path(&env);
    assert!(
        dp.split(':').any(|d| d == "/bin" || d == "/usr/bin"),
        "expected /bin or /usr/bin in default path, got: {dp}"
    );
}

#[test]
fn default_path_finds_sh() {
    // /bin/sh is POSIX-mandatory; exists on both macOS and Linux.
    let env = ShellEnv::default();
    assert!(find_in_path("sh", default_path(&env)).is_some());
}

#[test]
fn default_path_has_no_cwd_entry() {
    // confstr output never contains "." or empty segments.
    let env = ShellEnv::default();
    assert!(!default_path(&env).split(':').any(|d| d == "." || d.is_empty()));
}
```

### Level 3 — fallback path (no confstr dependency)

`fallback_default_path()` is a pure function; tested directly:

```rust
#[test]
fn fallback_is_bin_usr_bin() {
    assert_eq!(fallback_default_path(), "/bin:/usr/bin");
}
```

### Level 4 — E2E (`e2e/`)

All E2E tests use POSIX-mandatory utilities available on both macOS and Linux:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v finds external commands
# EXPECT_OUTPUT: /bin/sh
# EXPECT_EXIT: 0
PATH=/bin:/usr/bin command -v sh
```

> The explicit `PATH=/bin:/usr/bin` assignment keeps the search deterministic on both macOS and Linux, where `/bin/sh` is POSIX-mandatory.

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -p works when PATH is unset
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset PATH
command -p printf "hello"
```

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command bypasses user function
# EXPECT_OUTPUT: real
# EXPECT_EXIT: 0
printf() { echo "fake"; }
command printf "real"
```

> The function-skip test uses `printf` (a POSIX-mandatory utility available on both macOS and Linux) rather than `ls`, whose output differs across platforms.

### Level 5 — CI

- `cargo test` and `./e2e/run_tests.sh` continue to be the gates.
- Confirm the project's CI runs on both macOS and Linux runners; if only one, add the other for this change.

### Anti-patterns explicitly forbidden

1. Asserting on the exact PATH string (differs macOS vs Linux).
2. Depending on `/usr/local/bin` existing (many Linux containers lack it).
3. Asserting equality with `getconf PATH` output (environment-dependent).
4. Hardcoding macOS `/usr/libexec` or Linux `/usr/games` entries.

## Open Questions / Non-goals

- **`hash` builtin** — out of scope; designed so a future cache can wrap `find_in_path`.
- **`type` builtin** — out of scope; `resolve_command_kind` is reusable for it later.
- **`PATH=v cmd` unification with `command -p`** — out of scope; no-flag `command` signature kept amenable to future refactoring.
- **Non-UTF-8 PATH bytes** — `confstr` result is treated as UTF-8; invalid byte sequences fall back to `/bin:/usr/bin`. Acceptable since yosh does not currently support non-UTF-8 paths.

## Deliverables (for the implementation plan)

1. `src/env/default_path.rs` — new module with `default_path`, `call_confstr`, `fallback_default_path`.
2. `src/env/mod.rs` (or `ShellEnv` definition) — add `default_path_cache: OnceLock<String>` field.
3. `src/main.rs` — call `ensure_default_path(&mut env)` after `ShellEnv::from_environ()`.
4. `src/builtin/command.rs` — new `cmd_command` implementation.
5. `src/builtin/resolve.rs` — new `CommandKind` + `resolve_command_kind`.
6. `src/builtin/mod.rs` — register `command` as a regular builtin.
7. `src/exec/command.rs` — remove `#[allow(dead_code)]` from `find_in_path`.
8. Unit tests covering Levels 1–3 above.
9. E2E tests covering Level 4 above.
10. TODO.md — no new entries expected; this task closes the gap for the `command` builtin.
