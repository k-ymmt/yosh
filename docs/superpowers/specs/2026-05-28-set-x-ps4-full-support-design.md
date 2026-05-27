# `set -x` PS4 Full Support — Design

**Date:** 2026-05-28
**Status:** Approved (brainstorming)
**POSIX_REF:** XCU 2.5.3 Shell Variables (PS4); 2.5.3 execution trace

## 1. Problem

`set -x` trace lines are prefixed by the `PS4` variable. yosh currently
reads `PS4` and emits it **verbatim** (`src/exec/simple.rs:56`,
`xtrace_prefix`), falling back to `"+ "` when unset. Two POSIX behaviours
are missing:

1. **Expansion.** `PS4` is subject to parameter expansion, command
   substitution, and arithmetic expansion before display. Today
   `PS4='+ $LINENO> '` prints `$LINENO` literally instead of the line
   number.
2. **First-character replication for levels of indirection.** POSIX:
   "the first character of the value of `PS4` is replicated multiple
   times, as necessary, to indicate levels of indirection." yosh never
   replicates.

POSIX leaves the *number* of indirection levels unspecified
(implementation-defined). The existing `PS4_assigned.sh` /
`PS4_default.sh` e2e tests already pass and must stay green.

This closes the SP5 T1 follow-ups (TODO.md "PS4 variable / arithmetic /
command-sub expansion not implemented" and "PS4 first-character-repeat
rule for nesting depth") and the "Future: Interactive Mode Enhancements"
PS4 item.

## 2. Decisions

- **Scope: full support** — implement both expansion and first-character
  replication.
- **Indirection levels counted: function calls + dot/source scripts.**
  Both execute another body of code in the *current* shell environment,
  which is the structural indirection a script author recognises.
  Subshells and command substitutions (which run in child shells) are
  **not** counted. This is the TODO author's stated intent; POSIX permits
  it since the level count is implementation-defined.

### Reference-shell survey (informational)

The replication rule diverges across shells, confirming it is
implementation-defined:

| Shell | function | dot/source | subshell | command-sub |
|---|---|---|---|---|
| dash | no | no | no | no |
| bash 3.2 (macOS) | no | yes | no | yes |
| bash 5.x (per docs) | yes | yes | no | yes |

yosh's chosen semantics (function + dot, not subshell/command-sub) is a
deliberate, documented choice — not a copy of any single shell.

## 3. Architecture

Pipeline-wise this is contained in the **Executor** plus a small shared
helper in the **Expander**.

### 3.1 Shared double-quote expansion helper

PS1/PS2 prompt expansion (`src/interactive/prompt.rs`) already parses a
raw string as a double-quoted word and expands it. `set -x` runs in
non-interactive scripts too, so the shared core must live outside the
`interactive` module.

Extract into `src/expand/`:

```rust
/// Parse `raw` as the body of a double-quoted word and expand it
/// (parameter expansion, command substitution, arithmetic expansion;
/// no field splitting, no pathname expansion). On lexer/parser or
/// expansion error, fall back to returning `raw` unchanged.
pub fn expand_dquoted(env: &mut ShellEnv, raw: &str) -> String
```

This is the existing `parse_prompt_word` + `expand_word_to_string`
(with raw fallback) logic, relocated. `expand_prompt` is refactored to
call `expand_dquoted` after it has handled the unset/empty/default-value
cases (its prompt-specific defaults stay in `prompt.rs`).

### 3.2 Indirection-level counter

Add to `ExecState` (`src/env/exec_state.rs`):

```rust
/// Number of nested function-call and dot-script invocations currently
/// on the stack. Used only to replicate the first character of PS4 in
/// `set -x` trace output (POSIX "levels of indirection").
pub indirection_level: usize,
```

`Default` already yields `0`; no change to `ShellEnv::new`.

**Increment / decrement sites:**

- `exec_function_call` (`src/exec/function.rs`): `+= 1` at entry; `-= 1`
  immediately after `pop_scope()` and before the `match result` (mirrors
  the existing scope-pop placement, so a caught panic that is about to be
  re-raised still decrements on the normal path).
- `source_file` (`src/exec/mod.rs`): `+= 1` where `in_dot_script` is set
  `true`; `-= 1` everywhere `in_dot_script` is restored, including the
  early-return path (`mod.rs:110-111`).

Panic-safety matches the existing `in_dot_script` save/restore pattern
(not RAII-guarded). A RAII guard cannot borrow the counter for the
duration of body execution because the body needs `&mut env`; manual
inc/dec is the pragmatic choice and is consistent with the existing
`loop_depth` / `in_dot_script` handling.

### 3.3 PS4 prefix builder

Change `xtrace_prefix` to:

```rust
fn xtrace_prefix(env: &mut ShellEnv) -> String
```

Steps:

1. `raw = env.vars.get("PS4").unwrap_or("+ ")` (cloned to `String`).
2. Save `let saved = env.exec.last_exit_status;`.
3. `let expanded = expand_dquoted(env, &raw);`.
4. Restore `env.exec.last_exit_status = saved;`.
   — A command substitution inside PS4 must not corrupt `$?` for the
   command being traced (bash preserves it).
5. Replicate the **first character** of `expanded`
   `env.exec.indirection_level + 1` times; the remainder follows once.
   Empty `expanded` → empty string (nothing to replicate).

### 3.4 Call site

`src/exec/simple.rs:199`:

```rust
if self.env.mode.options.xtrace && !expanded.is_empty() {
    let prefix = xtrace_prefix(&mut self.env);
    eprintln!("{}{}", prefix, expanded.join(" "));
}
```

`LINENO` is set at the top of `exec_simple_command` (`simple.rs:63`), so
`$LINENO` inside PS4 resolves to the current command's line.

## 4. Replication rule

`first_char_count = indirection_level + 1`.

| Context | level | first-char count | `PS4='> '` output |
|---|---|---|---|
| top level | 0 | 1× | `> ` |
| inside one function / dot | 1 | 2× | `>> ` |
| double-nested | 2 | 3× | `>>> ` |

For a multi-character PS4 only the first character repeats, e.g.
`PS4='TRACE> '` at level 1 → `TTRACE> ` (matches bash's
first-character-only rule).

## 5. Error handling

PS4 expansion errors (lexer/parser failure, `set -u` on an unset name,
arithmetic error, failing command substitution) are **non-fatal**:
`expand_dquoted` returns the raw value and tracing continues. Matches the
prompt-expansion contract; tracing must never abort the traced command.

## 6. Out of scope

- Trace coverage stays at **simple commands only** (current behaviour).
  Tracing compound commands, pipelines, and assignment-only commands is a
  separate concern.
- `$?` preservation is added on the PS4 path only; `prompt.rs` is not
  changed in this regard.
- No subshell / command-substitution indirection counting (see §2).

## 7. Testing

### Unit (`src/exec/simple.rs` tests)
- Update the two existing tests for the new
  `xtrace_prefix(&mut ShellEnv) -> String` signature.
  `test_xtrace_prefix_uses_ps4_when_set` → `"TRACE> "` at level 0;
  `test_xtrace_prefix_default_when_ps4_unset` → `"+ "` at level 0.
- `$LINENO` expansion inside PS4 resolves to the set value.
- Replication by `indirection_level` (levels 0, 1, 2).
- Empty PS4 (`PS4=''`) → empty prefix.
- `$?` is preserved across a PS4 containing a command substitution.

### E2E (`e2e/posix_spec/8_env_vars/`, 644 perms)
- Keep `PS4_assigned.sh`, `PS4_default.sh` green.
- `PS4_expansion.sh` — `PS4='L$LINENO+ '; set -x; echo a` → stderr shows
  the line number.
- `PS4_nesting.sh` — `set -x` with a function call → inner command's
  trace shows the first char doubled.
- `PS4_dot_nesting.sh` — `set -x` with a dot-sourced script → sourced
  command's trace shows the first char doubled.

## 8. Affected files

- `src/expand/mod.rs` — new `expand_dquoted`.
- `src/interactive/prompt.rs` — delegate to `expand_dquoted`.
- `src/env/exec_state.rs` — new `indirection_level` field.
- `src/exec/function.rs` — inc/dec around function body.
- `src/exec/mod.rs` — inc/dec in `source_file`.
- `src/exec/simple.rs` — rewrite `xtrace_prefix`; update call site + tests.
- `e2e/posix_spec/8_env_vars/PS4_*.sh` — new tests.
- `TODO.md` — remove the three completed PS4 items.
