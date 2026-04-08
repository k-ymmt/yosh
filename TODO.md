# TODO

## Phase 1: Known Limitations

- [ ] Nested command substitution edge cases: `$(echo $(echo ')'))` may fail due to balanced-paren approach in lexer (`src/lexer/mod.rs` `read_balanced_parens`)
- [ ] Unquoted here-document body is stored as single Literal — needs expansion parsing in Phase 3 (use `HereDoc.quoted` flag to determine)
- [ ] Arithmetic expressions inside `$((...))` stored as raw string — `$var` references not pre-parsed; handle in Phase 3 expansion
- [ ] `Lexer.pending_heredocs` is `pub` — consider accessor methods for better encapsulation

## Remaining Phases

- [ ] Phase 2: Basic execution engine (fork/exec, pipelines, lists)
- [ ] Phase 3: Word expansion (tilde, parameter, command sub, arithmetic, field splitting, pathname, quote removal)
- [ ] Phase 4: Redirections and here-document I/O
- [ ] Phase 5: Control structure execution (if, for, while, until, case, functions)
- [ ] Phase 6: Special builtins (set, export, trap, eval, exec, etc.) + alias expansion
- [ ] Phase 7: Signals and errexit
- [ ] Phase 8: Subshell environment isolation
