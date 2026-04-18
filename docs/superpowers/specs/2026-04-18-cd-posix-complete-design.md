# POSIX-Complete `cd` Builtin Design

**Date**: 2026-04-18
**Sub-project**: 1 of 4 (XFAIL E2E test remediation — XCU Chapter 2 gaps)
**Target XFAIL**: `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`

## Context

The `cd` builtin in `src/builtin/regular.rs` currently calls
`std::env::set_current_dir(target)` and then reads `std::env::current_dir()`
to set `PWD`. On macOS this canonicalizes `/tmp` to `/private/tmp`,
violating POSIX §2.5.3 which requires `cd` without `-P` to preserve the
logical path. There is also no `-L`/`-P` option parsing and no `CDPATH`
support, both of which POSIX §4 requires.

This design closes the XFAIL and implements the full POSIX `cd`
specification as a single, focused change.

## Goals

1. `cd /tmp; echo $PWD` shall print `/tmp` on every platform (flip XFAIL).
2. Support `cd [-L|-P] [directory]` and `cd -`.
3. Support `CDPATH` search with empty-entry (`.`) semantics.
4. Preserve logical path in `PWD` / `OLDPWD` under default (`-L`) mode.
5. Match dash/bash behavior on option conflicts and error messages.

## Non-goals

- `cd -e` (ksh extension for symlink ephemeral-error mode) — out of scope.
- Restructuring other builtins or the `ShellEnv` API.
- Interactive directory-stack (`pushd`/`popd`) — unrelated.

## Architecture

`builtin_cd(args, env)` is reorganized into small private helpers, with
side effects confined to the outer entry function:

```
builtin_cd(args, env)              (entry; performs chdir and mutates env)
  ├─ parse_cd_options(args)                -> (CdMode, Option<&str>)
  ├─ resolve_target(operand, env)          -> (String, bool /* from_cdpath */)
  ├─ lexical_canonicalize(path, pwd)       -> String          (pure)
  └─ (inline in entry)
       ├─ Logical  : new_pwd = lexical_canonicalize(target, old_pwd)
       │             set_current_dir(new_pwd)
       ├─ Physical : set_current_dir(target)
       │             new_pwd = std::env::current_dir()
       ├─ env.vars.set("OLDPWD", old_pwd)
       ├─ env.vars.set("PWD", new_pwd)
       └─ if from_cdpath { println!(new_pwd) }
```

The three named helpers (`parse_cd_options`, `resolve_target`,
`lexical_canonicalize`) are pure and independently unit-testable. Side
effects (`set_current_dir`, `env.vars.set`, `println!`) live inline in
`builtin_cd` because splitting them into a separate helper would require
threading too much state.

## Option parsing

`parse_cd_options(args: &[String]) -> Result<(CdMode, Option<&str>), ShellError>`

Accepts the POSIX grammar:

```
cd [-L|-P] [directory]
cd -
```

Rules:

| Input | Result |
|---|---|
| `cd` | `(Logical, None)` → HOME |
| `cd -` | `(Logical, Some("-"))` — `-` is an operand, not an option |
| `cd -L` | `(Logical, None)` |
| `cd -P` | `(Physical, None)` |
| `cd -L foo` / `cd -P foo` | each mode with operand `foo` |
| `cd -LP foo` / `cd -PL foo` | last flag wins (dash/bash) |
| `cd -L -P foo` | same: last wins |
| `cd --` | `(Logical, None)` |
| `cd -- -foo` | operand `-foo` |
| `cd -x` | error: `cd: -x: invalid option` (exit 2) |
| `cd a b` | error: `cd: too many arguments` (exit 1, no chdir) |

Option scanning stops at the first non-`-L`/`-P`/`--` argument and
treats it as the operand. `-` is always an operand.

## Target resolution

`resolve_target(operand: Option<&str>, env: &ShellEnv) -> Result<(String, bool), ShellError>`

Returns `(path, from_cdpath)`. `from_cdpath = true` triggers stdout
printing of the final `PWD` (per POSIX §4 cd step 7).

Algorithm (POSIX §4 cd steps 1–5):

```
1. operand is None:
     value = env.vars.get("HOME"); empty/unset -> error "cd: HOME not set" (exit 1)
     from_cdpath = false
2. operand == "-":
     value = env.vars.get("OLDPWD"); unset -> error "cd: OLDPWD not set" (exit 1)
     from_cdpath = true   (POSIX: cd - shall print the new PWD)
3. operand starts with "/":
     value = operand; from_cdpath = false   (skip CDPATH)
4. operand is ".", "..", "./", "../" prefix:
     value = operand; from_cdpath = false   (skip CDPATH, treated as PWD-relative)
5. else (relative, not dot-prefixed):
     for each dir in CDPATH.split(':'):
       dir' = if dir == "" { "." } else { dir }
       curpath = dir' + "/" + operand
       if curpath exists and is-directory (following symlinks):
         value = curpath; from_cdpath = true; break
     if no match:
       value = operand   (falls through to PWD-prefix in compute_new_pwd)
       from_cdpath = false
```

CDPATH errors during probing (permission denied, etc.) are silently
skipped to the next entry (dash compatible).

## Logical canonicalization

`lexical_canonicalize(path: &str, pwd: &str) -> String`

Pure string operation; does **not** touch the filesystem.

```
1. If path does not start with '/', prepend pwd + '/'.
2. Split on '/'; drop empty components (collapses '//', '///').
3. Walk components into a stack:
     '.'        -> skip
     '..'       -> if stack non-empty and top != '..': pop; else push
     otherwise  -> push
4. Result = '/' + components.join('/'); empty stack -> '/'.
```

Examples:

| path | pwd | result |
|---|---|---|
| `/tmp` | `/Users/foo` | `/tmp` |
| `/tmp/../etc` | `/` | `/etc` |
| `../bar` | `/tmp/foo` | `/tmp/bar` |
| `./foo/./bar` | `/tmp` | `/tmp/foo/bar` |
| `/tmp//foo` | `/` | `/tmp/foo` |
| `/..` | `/` | `/` |
| `a/b/../..` | `/tmp/x` | `/tmp/x` |

POSIX step 8.e allows implementation-defined handling when `..` crosses
a symlink; bash/dash always pop lexically, which is what we do.

## chdir and PWD/OLDPWD update (inline in `builtin_cd`)

After `parse_cd_options` and `resolve_target` succeed:

```
1. old_pwd = env.vars.get("PWD")
               or fallback to std::env::current_dir()
2. Compute new_pwd:
     Logical:  new_pwd = lexical_canonicalize(target, old_pwd)
               set_current_dir(new_pwd)
     Physical: set_current_dir(target)
               new_pwd = std::env::current_dir()  (canonicalized)
3. On chdir error:
     eprintln!("yosh: cd: {target}: {err}"); return Ok(1)
     PWD/OLDPWD untouched.
4. env.vars.set("OLDPWD", old_pwd)
   env.vars.set("PWD", new_pwd)
5. if from_cdpath { println!("{new_pwd}"); }
6. Ok(0)
```

Notes:

- OLDPWD is taken from the shell variable `PWD` (logical) rather than
  `current_dir()` so that `cd -` cycles through logical paths cleanly.
- The existing `if is_dash { println!(target) }` branch is removed;
  `from_cdpath = true` for `cd -` already triggers the correct output.

## Error handling

| Case | stderr | exit | chdir | PWD/OLDPWD |
|---|---|---|---|---|
| `cd -x` | `cd: -x: invalid option` | 2 | no | unchanged |
| `cd a b` | `cd: too many arguments` | 1 | no | unchanged |
| `cd` with HOME unset | `cd: HOME not set` | 1 | no | unchanged |
| `cd -` with OLDPWD unset | `cd: OLDPWD not set` | 1 | no | unchanged |
| `cd /no/such` | `cd: /no/such: No such file or directory` | 1 | no | unchanged |
| `cd /etc/passwd` | `cd: /etc/passwd: Not a directory` | 1 | no | unchanged |
| `cd /root` (EACCES) | `cd: /root: Permission denied` | 1 | no | unchanged |
| CDPATH entry not a dir | (silent, skip to next) | — | — | — |

All stderr messages are prefixed with `yosh: ` per project convention.

Exit code strategy (confirmed against `src/error.rs` line 108:
`InvalidArgument` and `InvalidOption` map to exit 2, `IoError` maps to
exit 1):

- `-x: invalid option` → returns `Err(ShellError::runtime(InvalidArgument, …))`
  (auto-maps to exit 2).
- All other errors (too many arguments, HOME not set, OLDPWD not set,
  chdir failures) must exit 1 but there is no 1-mapped variant
  semantically appropriate for these. Solution: `eprintln!("yosh: cd:
  …")` directly and `return Ok(1)`. This keeps error messages consistent
  in form while preserving the correct exit code.

## Testing

### Unit tests (`src/builtin/regular.rs` `#[cfg(test)]`)

- `lexical_canonicalize` — table-driven coverage of the examples above
  plus edge cases (`""`, `"/"`, trailing slashes, repeated `..`).
- `parse_cd_options` — each input pattern in the parsing table,
  including conflicting flags and `--`.
- `resolve_target` CDPATH split — empty entries, leading/trailing
  colons, single-colon-only (`CDPATH=:` means `.`).

### E2E XFAIL flip

Remove the `XFAIL:` metadata line from
`e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh` so it runs as
PASS.

### New E2E tests under `e2e/builtin/`

| File | Purpose |
|---|---|
| `cd_logical_default.sh` | `cd /tmp && echo $PWD` → `/tmp` |
| `cd_physical_flag.sh` | `cd -P /tmp && echo $PWD` — portability check: accept `/tmp` or `/private/tmp` |
| `cd_logical_dotdot.sh` | `cd /tmp/../etc && echo $PWD` → `/etc` |
| `cd_double_dash.sh` | `cd -- -foo` treats `-foo` as operand |
| `cd_invalid_option.sh` | `cd -x` → exit 2, stderr contains `invalid option` |
| `cd_too_many_args.sh` | `cd /tmp /etc` → exit 1, stderr contains `too many arguments` |
| `cd_dash_prints_pwd.sh` | `cd -` prints new PWD to stdout |
| `cd_cdpath_basic.sh` | `CDPATH=$TEST_TMPDIR cd sub` enters `$TEST_TMPDIR/sub` |
| `cd_cdpath_empty_entry.sh` | Leading `:` in CDPATH means current directory |
| `cd_cdpath_not_found.sh` | CDPATH miss falls through to normal resolution |
| `cd_oldpwd_logical.sh` | `OLDPWD` stores the logical previous `PWD` |

All new files: POSIX metadata header, permissions `644`.

### Regression

- `e2e/builtin/cd_basic.sh` and `cd_dash_oldpwd.sh` must continue to
  pass unchanged.
- `cargo test` suite green.

## Completion criteria

1. `cargo test` all green, `cargo clippy` no warnings, `cargo fmt`
   clean.
2. `./e2e/run_tests.sh` summary: `XFail: 3, XPass: 0, Failed: 0,
   Timedout: 0` (the three remaining XFAILs are the tilde/empty-list/
   LINENO gaps handled by sub-projects 2–4).
3. `TODO.md` entries for `§2.5.3 PWD logical path` removed (completion
   per project convention — delete rather than mark `[x]`).
