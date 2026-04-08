# TODO

## Phase 1: Known Limitations

- [ ] Nested command substitution edge cases: `$(echo $(echo ')'))` may fail due to balanced-paren approach in lexer (`src/lexer/mod.rs` `read_balanced_parens`)
- [ ] `Lexer.pending_heredocs` is `pub` — consider accessor methods for better encapsulation

## Phase 2: Known Limitations

- [ ] `export` output format missing quotes — should be `export FOO="bar"` not `export FOO=bar` (`src/builtin/mod.rs`)
- [ ] `echo -n` flag not handled — POSIX strict doesn't require it but practical shells need it (`src/builtin/mod.rs`)
- [ ] `cd -` (change to OLDPWD) not implemented (`src/builtin/mod.rs`)
- [ ] `VarStore` has no scope mechanism — needed for function execution in Phase 5 (`src/env/vars.rs`)
- [ ] `builtin_exit` calls `process::exit` directly — needs change for EXIT trap support in Phase 7 (`src/builtin/mod.rs`)
- [ ] `TempDir` ID uses nanosecond timestamp — risk of collision under heavy parallel testing (`tests/helpers/mod.rs`)

## Phase 3: Known Limitations

- [ ] Unquoted `$@` should produce separate fields per positional param, currently joins with space (`src/expand/mod.rs`)
- [ ] `set -f` (noglob) not checked — pathname expansion cannot be disabled yet (`src/expand/pathname.rs`)
- [ ] Arithmetic compound assignment operators (`+=`, `-=`, `*=`, etc.) not implemented (`src/expand/arith.rs`)
- [ ] `${parameter:?word}` should exit non-interactive shell, currently only prints error (`src/expand/param.rs`)
- [ ] Deeply nested command substitution edge cases untested

## Phase 4: Known Limitations

- [ ] Heredoc + pipeline not working — `cat <<EOF | tr a-z A-Z` produces empty output due to redirect timing in child process (`src/exec/pipeline.rs`)

## Phase 5: Known Limitations

- [ ] `$N` (positional params) inside `$((...))` arithmetic not supported — use temp variable workaround: `x=$1; echo $((x - 1))` (`src/expand/arith.rs`)
- [ ] Subshell environment isolation is basic (fork-based) — full isolation deferred to Phase 8
- [ ] Function-scoped assignments with prefix syntax (`VAR=val func`) not implemented — assignments only apply to external commands

## Remaining Phases

- [x] Phase 5: Control structure execution (if, for, while, until, case, functions)
- [ ] Phase 6: Special builtins (set, export, trap, eval, exec, etc.) + alias expansion
- [ ] Phase 7: Signals and errexit
- [ ] Phase 8: Subshell environment isolation
