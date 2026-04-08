# Phase 6: Special Builtins + Alias Expansion — Design Specification

## Overview

Phase 6 adds POSIX special builtins, shell options (`ShellOptions`), trap registration with EXIT trap execution, alias expansion at the lexer level, and the correct POSIX distinction between special and regular builtins (assignment persistence behavior).

### Scope

- **In scope:** All special builtins (`set`, `eval`, `exec`, `.`, `shift`, `times`, `trap`, `export`, `unset`, `readonly`, `exit`, `return`, `break`, `continue`, `:`), shell options (`-a`, `-e`, `-f`, `-n`, `-u`, `-v`, `-x`, `-C`, `-m`, `-b`, `pipefail`, `ignoreeof`), alias expansion, `alias`/`unalias` regular builtins, POSIX special/regular builtin distinction in execution model
- **Out of scope:** Existing bug fixes (export format, `cd -`, `echo -n`, `${parameter:?word}`), errexit exception logic (Phase 7), signal trap execution (Phase 7), job control / monitor mode (future), interactive features (future)

### Design Decisions

- **Approach A (Layer Separation):** New files `src/builtin/special.rs` and `src/env/aliases.rs`; `ShellOptions` and `TrapStore` added to `src/env/mod.rs`; Executor updated for special/regular distinction
- **trap:** Registration mechanism + EXIT trap execution in Phase 6; signal trap execution deferred to Phase 7
- **Shell options:** All flags settable/displayable; `-e` and `-m` flag behavior deferred to Phase 7+
- **POSIX compliance:** Special builtin prefix assignments persist; regular builtin prefix assignments are temporary

---

## 1. ShellEnv Extensions

### ShellOptions (`src/env/mod.rs`)

```rust
#[derive(Debug, Clone)]
pub struct ShellOptions {
    pub allexport: bool,     // -a: auto-export all variables
    pub notify: bool,        // -b: immediate background job notification
    pub noclobber: bool,     // -C: prevent > from overwriting files
    pub errexit: bool,       // -e: exit on error (behavior in Phase 7)
    pub noglob: bool,        // -f: disable pathname expansion
    pub noexec: bool,        // -n: read commands but do not execute
    pub monitor: bool,       // -m: job control (behavior deferred)
    pub nounset: bool,       // -u: error on unset variable reference
    pub verbose: bool,       // -v: print input lines to stderr
    pub xtrace: bool,        // -x: print command trace to stderr
    pub ignoreeof: bool,     // ignore EOF
    pub pipefail: bool,      // pipeline exit status from all commands
}
```

- `ShellOptions::default()` initializes all fields to `false`
- `to_flag_string(&self) -> String`: returns active flags as string (e.g., `"aex"`) for `$-`
- `set_by_char(&mut self, c: char, on: bool) -> Result<()>`: set/unset by short flag
- `set_by_name(&mut self, name: &str, on: bool) -> Result<()>`: set/unset by long name (`-o` form)
- `display_all(&self)` / `display_restorable(&self)`: for `set -o` / `set +o` output

### TrapStore (`src/env/mod.rs`)

```rust
#[derive(Debug, Clone)]
pub enum TrapAction {
    Default,
    Ignore,
    Command(String),
}

#[derive(Debug, Clone)]
pub struct TrapStore {
    pub exit_trap: Option<TrapAction>,
    pub signal_traps: HashMap<i32, TrapAction>,  // registration only, execution in Phase 7
}
```

- `set_trap(&mut self, condition: &str, action: TrapAction) -> Result<()>`
- `get_trap(&self, condition: &str) -> Option<&TrapAction>`
- `remove_trap(&mut self, condition: &str)`
- `display_all(&self)` / `display(&self, condition: &str)`
- Signal name to number mapping: `EXIT`=0, `HUP`=1, `INT`=2, `QUIT`=3, `TERM`=15, etc.

### AliasStore (`src/env/aliases.rs` — new file)

```rust
#[derive(Debug, Clone, Default)]
pub struct AliasStore {
    aliases: HashMap<String, String>,
}
```

- `set(&mut self, name: &str, value: &str)`
- `get(&self, name: &str) -> Option<&str>`
- `remove(&mut self, name: &str) -> bool`
- `clear(&mut self)`
- `iter(&self) -> impl Iterator<Item = (&str, &str)>`: sorted by name for display

### ShellEnv Changes

```rust
pub struct ShellEnv {
    pub vars: VarStore,
    pub options: ShellOptions,        // NEW
    pub traps: TrapStore,             // NEW
    pub aliases: AliasStore,          // NEW
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    pub positional_params: Vec<String>,
    pub last_bg_pid: Option<i32>,
    pub functions: HashMap<String, FunctionDef>,
    pub flow_control: Option<FlowControl>,
}
```

---

## 2. Special/Regular Builtin Separation

### POSIX Classification

**Special builtins** (errors may exit shell, prefix assignments persist):
`break`, `continue`, `return`, `:`, `eval`, `exec`, `exit`, `export`, `readonly`, `set`, `unset`, `trap`, `.` (source), `shift`, `times`

**Regular builtins** (errors don't exit shell, prefix assignments temporary):
`cd`, `echo`, `true`, `false`, `read`, `test`, `type`, `alias`, `unalias`

### File Structure

- `src/builtin/mod.rs` — dispatch logic + `BuiltinKind` enum + regular builtins (`cd`, `echo`, `true`, `false`, `alias`, `unalias`)
- `src/builtin/special.rs` — special builtins (`set`, `eval`, `exec`, `trap`, `.`, `shift`, `times`; existing `export`, `unset`, `readonly`, `exit`, `return`, `break`, `continue`, `:` moved here)

### Dispatch

```rust
// builtin/mod.rs
pub enum BuiltinKind {
    Special,
    Regular,
    NotBuiltin,
}

pub fn classify_builtin(name: &str) -> BuiltinKind { ... }
```

### Executor Changes (`exec_simple_command`)

New flow per POSIX Section 2.9.1:

1. Separate assignments and redirections from words
2. Expand command name
3. No command name → apply assignments to current environment, exit 0
4. Command name present:
   - **Special builtin** → apply assignments to current environment (persistent) → execute
   - **Shell function** → save old values → apply assignments temporarily → execute → restore
   - **Regular builtin** → save old values → apply assignments temporarily → execute → restore
   - **External command** → fork, apply assignments + redirects in child → execve

Temporary assignment restoration: save old values before applying, restore after execution.

### Builtin Signatures

```rust
// special.rs — eval and . need Executor access
pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 { ... }

// mod.rs — regular builtins only need ShellEnv
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 { ... }
```

---

## 3. Special Builtins — Individual Design

### `set`

```
set                    → display all variables (name=value format)
set -o                 → display all options (on/off format)
set +o                 → display in restorable format (set -o xxx / set +o xxx)
set -abCefmnuvx        → enable options (multiple at once)
set +abCefmnuvx        → disable options
set -- arg1 arg2 ...   → replace positional params
set - arg1 arg2 ...    → disable -x and -v + replace positional params
```

`$-` special parameter: `env.options.to_flag_string()` called from `expand/param.rs`.

### `eval`

```rust
fn builtin_eval(args: &[String], executor: &mut Executor) -> i32 {
    let input = args.join(" ");
    // parse input → execute via executor
}
```

- Concatenates arguments with spaces, re-parses, and executes
- Requires `&mut Executor` (not just `&mut ShellEnv`)

### `exec`

```
exec command args...   → replace current process (execvp, no fork)
exec < file            → apply redirects permanently (no command)
exec                   → no-op (exit status 0)
```

- No command + redirects: redirects are applied to current shell permanently (no restore)
- With command: calls `execvp` directly (does not return on success)

### `.` (source)

```
. filename [args]      → execute file content in current environment
```

- Search PATH for filename
- Read file content, parse, execute via Executor
- Requires `&mut Executor`

### `shift`

```
shift [n]              → left-shift positional params by n (default 1)
```

- `n > positional_params.len()` → error, exit status 1

### `times`

- Display accumulated user/system time for shell and children
- Uses `libc::times()`

### `trap`

```
trap                       → display current trap settings
trap action signal...      → set trap
trap '' signal...          → ignore signal
trap - signal...           → reset to default
trap -p [signal...]        → display specified trap(s)
```

- Signal names: `EXIT`, `HUP`, `INT`, `QUIT`, `TERM`, etc. (also by number)
- EXIT trap: executed at `builtin_exit` and at normal shell termination (before main exits)

---

## 4. Alias Expansion

### Lexer Changes (`src/lexer/mod.rs`)

```rust
pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    // ... existing fields
    alias_stack: Vec<(String, usize)>,  // (expanded text, read position)
    expanding_aliases: HashSet<String>,  // recursion prevention set
}
```

### Expansion Rules (POSIX Section 2.3.1)

1. After reading a token in `CommandPosition`, check if the word matches an alias
2. If match found:
   - Add alias name to `expanding_aliases` (recursion prevention)
   - Push alias value onto `alias_stack`
   - Next tokens are read from the stack
3. If alias value **ends with whitespace**, the next token is also subject to alias expansion
4. Names in `expanding_aliases` are not expanded (prevents infinite recursion)
5. When stack entry is fully consumed, remove alias name from `expanding_aliases`

### Parser / Lexer Integration

```rust
impl<'a> Parser<'a> {
    pub fn new(input: &'a str, aliases: &'a AliasStore) -> Self {
        Parser {
            lexer: Lexer::new(input, aliases),
            // ...
        }
    }
}
```

All call sites using Parser/Lexer (`main.rs`, `eval`, `.`, command substitution) must propagate the `AliasStore` reference.

### `alias` / `unalias` (Regular Builtins)

```
alias                  → display all aliases ("name='value'" format)
alias name             → display specified alias
alias name=value       → define alias
unalias name           → remove alias
unalias -a             → remove all aliases
```

---

## 5. ShellOptions Impact on Existing Modules

### Options with behavior enabled in Phase 6

| Option | Module | Behavior |
|--------|--------|----------|
| `-a` (allexport) | `env/vars.rs` | Auto-set `exported = true` on variable assignment |
| `-C` (noclobber) | `exec/redirect.rs` | Prevent `>` from overwriting existing files; `>\|` overrides |
| `-f` (noglob) | `expand/mod.rs` | Skip pathname expansion stage |
| `-n` (noexec) | `exec/mod.rs` | Read/parse commands but do not execute |
| `-u` (nounset) | `expand/param.rs` | Error on unset variable reference |
| `-v` (verbose) | `exec/mod.rs` | Print input lines to stderr when read |
| `-x` (xtrace) | `exec/mod.rs` | Print `+ cmd args...` to stderr before execution |
| `pipefail` | `exec/pipeline.rs` | Already implemented; change to read from `env.options` |

### Options with behavior deferred

| Option | Deferred to | Reason |
|--------|------------|--------|
| `-e` (errexit) | Phase 7 | Requires exception rules (if/while/until conditions, `!`, AND-OR) |
| `-m` (monitor) | Future | Job control, interactive feature |
| `-b` (notify) | Future | Depends on `-m` |
| `ignoreeof` | Future | Interactive mode feature |

All flags are settable/displayable in Phase 6; only actual behavior is deferred.

### `$-` Special Parameter

`expand/param.rs` handles `$-` by calling `env.options.to_flag_string()`.

### Summary of Existing File Changes

- **`expand/mod.rs`**: Add `-f` check before pathname expansion
- **`expand/param.rs`**: Add `-u` check + `$-` expansion
- **`exec/redirect.rs`**: Add `-C` check on `>` redirect
- **`exec/pipeline.rs`**: Read `pipefail` from `env.options`
- **`exec/mod.rs`**: Add `-n`, `-v`, `-x` checks + special/regular builtin dispatch + prefix assignment handling
- **`lexer/mod.rs`**: Alias expansion with stack and recursion prevention
- **`parser/mod.rs`**: Accept `AliasStore` reference, pass to Lexer
- **`main.rs`**: Propagate alias reference + execute EXIT trap at termination
- **`env/vars.rs`**: `-a` (allexport) auto-export on assignment

---

## 6. Testing Strategy

### Unit Tests

**`env/mod.rs`** — ShellOptions:
- Default values (all false), flag set/unset, `to_flag_string()` output, `set_by_char`/`set_by_name`

**`env/mod.rs`** — TrapStore:
- Register/get/remove/display traps, signal name-to-number mapping

**`env/aliases.rs`** — AliasStore:
- Set/get/remove/clear/iterate aliases

**`builtin/special.rs`** — Each special builtin:
- `set`: option set/unset, positional params replacement, `-o`/`+o` output, `set --`
- `eval`: string re-parse and execution
- `exec`: no command (no-op), redirect-only
- `trap`: register/display/reset/ignore
- `.`: file read and execution
- `shift`: normal shift, out-of-range error

**`builtin/mod.rs`** — Regular builtins:
- `alias`/`unalias`: register/display/remove

**`lexer/mod.rs`** — Alias expansion:
- Basic expansion, recursion prevention, trailing-whitespace chained expansion

### Integration Tests

**Special builtins:**
- `set -x` xtrace output verification
- `set -f` glob disabled verification
- `set -u` unset variable error verification
- `set -- a b c` + `echo $1 $2 $3`
- `eval 'echo hello'`
- `shift` + positional params verification
- `trap 'echo bye' EXIT` + execution at exit

**Builtin distinction:**
- `VAR=val special_builtin` — VAR persists after execution (special)
- `VAR=val regular_builtin` — VAR does not persist after execution (regular)

**Alias:**
- `alias ll='ls -l'; ll` execution
- Recursive alias prevention
- Trailing-whitespace chained expansion
- `unalias -a`

**Shell options impact:**
- `-C` (noclobber): `>` overwrite prevented, `>|` forced overwrite
- `-v` (verbose): input lines appear on stderr
- `-n` (noexec): commands are not executed

---

## 7. New and Modified Files Summary

### New Files
- `src/builtin/special.rs` — special builtin implementations
- `src/env/aliases.rs` — AliasStore

### Modified Files
- `src/env/mod.rs` — add ShellOptions, TrapStore, update ShellEnv struct
- `src/env/vars.rs` — allexport support
- `src/builtin/mod.rs` — BuiltinKind enum, classify_builtin, move special builtins out, add alias/unalias
- `src/exec/mod.rs` — special/regular dispatch, prefix assignment handling, `-n`/`-v`/`-x` checks
- `src/exec/redirect.rs` — noclobber check
- `src/exec/pipeline.rs` — read pipefail from env.options
- `src/expand/mod.rs` — noglob check
- `src/expand/param.rs` — nounset check, `$-` expansion
- `src/lexer/mod.rs` — alias expansion with stack and recursion prevention
- `src/parser/mod.rs` — accept AliasStore reference
- `src/main.rs` — alias propagation, EXIT trap execution
