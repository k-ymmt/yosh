# TODO

## Phase 1: Known Limitations

- [ ] Nested command substitution edge cases: `$(echo $(echo ')'))` may fail due to balanced-paren approach in lexer (`src/lexer/mod.rs` `read_balanced_parens`)
- [ ] Unquoted here-document body is stored as single Literal — needs expansion parsing in Phase 3 (use `HereDoc.quoted` flag to determine)
- [ ] Arithmetic expressions inside `$((...))` stored as raw string — `$var` references not pre-parsed; handle in Phase 3 expansion
- [ ] `Lexer.pending_heredocs` is `pub` — consider accessor methods for better encapsulation

## Phase 2: Known Limitations

- [ ] `export` output format missing quotes — should be `export FOO="bar"` not `export FOO=bar` (`src/builtin/mod.rs`)
- [ ] `echo -n` flag not handled — POSIX strict doesn't require it but practical shells need it (`src/builtin/mod.rs`)
- [ ] `cd -` (change to OLDPWD) not implemented (`src/builtin/mod.rs`)
- [ ] `VarStore` has no scope mechanism — needed for function execution in Phase 5 (`src/env/vars.rs`)
- [ ] `builtin_exit` calls `process::exit` directly — needs change for EXIT trap support in Phase 7 (`src/builtin/mod.rs`)
- [ ] `TempDir` ID uses nanosecond timestamp — risk of collision under heavy parallel testing (`tests/helpers/mod.rs`)

## Remaining Phases

- [ ] Phase 3: Word expansion (tilde, parameter, command sub, arithmetic, field splitting, pathname, quote removal)
- [ ] Phase 4: Redirections and here-document I/O
- [ ] Phase 5: Control structure execution (if, for, while, until, case, functions)
- [ ] Phase 6: Special builtins (set, export, trap, eval, exec, etc.) + alias expansion
- [ ] Phase 7: Signals and errexit
- [ ] Phase 8: Subshell environment isolation
