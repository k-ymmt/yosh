# Phase 5: Control Structure Execution

## Scope

Implement execution of all POSIX control structures that the parser already produces AST nodes for, plus function definition/invocation and loop control builtins.

| Feature | Description |
|---------|-------------|
| if/elif/else | Evaluate condition list, execute matching branch |
| for | Expand word list, iterate with loop variable |
| while | Loop while condition succeeds (exit status 0) |
| until | Loop while condition fails (exit status != 0) |
| case | Glob-based pattern matching with `;;` (break) and `;&` (fall-through) |
| brace group `{}` | Execute command list in current environment |
| subshell `()` | Fork and execute command list in child process (basic) |
| function def | Store function in ShellEnv, return 0 |
| function call | Look up function, swap positional params, execute body |
| break/continue | Loop control builtins with nesting support |

## Design Decisions

- **POSIX strict scoping**: No `local` builtin. All variables in functions are global.
- **Subshell**: Basic fork-based execution in Phase 5. Full environment isolation deferred to Phase 8.
- **No AST or parser changes**: All control structures are already parsed. Work is entirely in execution layer.

## Files to Modify

| File | Changes |
|------|---------|
| `src/exec/mod.rs` | Add `exec_compound_command()` dispatcher and handlers for each control structure |
| `src/env/mod.rs` | Add `functions: HashMap<String, FunctionDef>` to `ShellEnv` |
| `src/builtin/mod.rs` | Add `break` and `continue` builtins |
| `tests/parser_integration.rs` | Add Phase 5 integration tests |

No new files needed.

## Execution Flow

### Compound Command Dispatch

```
exec_command(Command::Compound(compound, redirects))
  -> apply redirects
  -> match compound.kind {
       If { .. }        -> exec_if()
       For { .. }       -> exec_for()
       While { .. }     -> exec_while()
       Until { .. }     -> exec_until()
       Case { .. }      -> exec_case()
       BraceGroup { .. } -> exec_brace_group()
       Subshell { .. }  -> fork + exec in child
     }
  -> restore redirects
```

### Function Definition

```
exec_command(Command::FunctionDef(def))
  -> env.functions.insert(def.name, def)
  -> return 0
```

### Function Invocation

In `exec_simple_command`, after expansion, before external command lookup:

```
if env.functions.contains_key(command_name):
  -> save current positional_params
  -> set positional_params to function arguments
  -> exec_compound_command(function.body)
  -> restore positional_params
  -> return exit status
```

## break/continue Mechanism

Use a `Result<i32, LoopControl>` return type for command execution within loops:

```rust
enum LoopControl {
    Break(usize),    // remaining nesting levels
    Continue(usize), // remaining nesting levels
    Return(i32),     // function return with exit status
}
```

- Loop handlers catch `LoopControl` and decrement the nesting count.
- If count reaches 0, the loop acts on it (break exits, continue skips to next iteration).
- If count > 0, re-propagate upward for outer loops to handle.
- `Return` propagates up to the function call boundary.

## case Pattern Matching

Reuse the existing glob machinery in `src/expand/pathname.rs`. For each `CaseItem`:

1. Expand the case word to a string.
2. For each pattern in the item, expand it and perform glob match against the case word.
3. On first match, execute the item body.
4. If terminator is `;;`, stop. If `;&`, fall through to next item.

## Test Plan

- **if/elif/else**: basic true/false, elif chain, nested if
- **for**: word list, default `$@`, empty list, nested for
- **while/until**: basic loop, exit on condition change
- **case**: exact match, glob patterns, multiple patterns per item, fall-through `;&`, default `*`
- **brace group**: variable side effects visible in parent
- **subshell**: variable changes not visible in parent
- **functions**: basic call, arguments via `$1`/`$@`, recursion, `return` builtin
- **break/continue**: single loop, nested loops with depth argument
- **compound + redirects**: `if ... fi > file`, `for ... done < file`
