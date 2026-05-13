# SP1 — Special-builtin error diagnostics & semantics

**Date:** 2026-05-13
**Roadmap:** `docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`
**Status:** Design (implementation plan TBD via writing-plans skill)
**Scope:** 11 XFAIL tests in `e2e/posix_spec/4_special_builtin/` and
`e2e/posix_spec/2_08_01_consequences_of_shell_errors/`.

## 1. Background

The E2E XFAIL roadmap partitions 55 XFAIL tests into seven
sub-projects. SP1 — the first sub-project — fixes special-builtin
behavior bugs that cause yosh to silently accept invalid input, return
the wrong exit status, or skip diagnostics that POSIX requires. Each
test in scope already declares the correct
`EXPECT_OUTPUT`/`EXPECT_EXIT`/`EXPECT_STDERR`; closing the sub-project
means removing the `# XFAIL: …` line so the test runs as a normal
expectation.

## 2. Scope adjustment to the roadmap

`exec_redir_input.sh` was listed under SP1 in the roadmap but its body
calls `read line`. Until `read` ships in SP3, the test cannot verify
`exec <` regardless of whether the `exec` redirect works. Move
`exec_redir_input.sh` from SP1 to SP3.

After the move:

- SP1 covers 11 tests (was 12).
- SP3 covers 9 tests (was 8).
- Total stays at 55.

The roadmap spec is updated in the same commit as this design
document.

## 3. Tests in scope (11)

| Group | Test | Current | Expected |
|-------|------|---------|----------|
| G1 | `4_special_builtin/break_outside_loop.sh` | exit 0, no stderr | exit 1, stderr contains `break` |
| G1 | `4_special_builtin/continue_outside_loop.sh` | exit 0, no stderr | exit 1, stderr contains `continue` |
| G1 | `4_special_builtin/continue_n_exceeds_depth.sh` | prints `a` only | prints `a\nb` (continue against outermost loop) |
| G2 | `4_special_builtin/unset_invalid_name.sh` | exit 0, no stderr | exit 1, stderr contains `unset` |
| G2 | `4_special_builtin/readonly_invalid_name.sh` | exit 0, no stderr | exit 1, stderr contains `readonly` |
| G2 | `4_special_builtin/export_invalid_name.sh` | exit 0, no stderr | exit 1, stderr contains `export` |
| G3 | `4_special_builtin/unset_f_function.sh` | function persists | exit 127 (command not found) |
| G3 | `4_special_builtin/unset_f_keeps_variable.sh` | empty output | `var-value` |
| G4 | `4_special_builtin/readonly_p_listing.sh` | empty output | `readonly myvar=v` listed |
| G5 | `4_special_builtin/exec_keeps_env.sh` | empty output | `kept` |
| G5 | `2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh` | prints `not-reached` | empty (subshell exits on redir error) |

## 4. Design by group

### 4.1 G1 — Loop-depth tracking and break/continue checks (3 tests)

POSIX (§2.14.1 `break`, §2.14.5 `continue`) requires:

- `break` and `continue` outside any loop are treated as no-op-with-error
  (in non-interactive shells the behavior is implementation-defined but
  must include a diagnostic; the e2e tests demand exit 1 + diagnostic).
- For `continue n` (or `break n`), "if n is greater than the number of
  enclosing loops, the outermost enclosing loop shall be used."

**Changes:**

- `src/env/exec_state.rs`: add `pub loop_depth: usize` to `ExecState`,
  default `0`.
- `src/exec/compound.rs`: in `exec_for` and `exec_while_until`,
  increment `loop_depth` before executing the body and decrement on
  every exit path. Use a scope-local guard (either `scopeguard::defer!`
  if the crate is already on the dep list, or a small RAII wrapper
  defined in this file) so the decrement runs even when the body
  triggers `Break`/`Continue`/`Return` flow-control or `ShellError`.
- `src/builtin/special.rs::builtin_break`: at function entry, check
  `env.exec.loop_depth == 0`. If so, write
  `yosh: break: only meaningful in a 'for', 'while', or 'until' loop`
  to stderr, return `Ok(1)`, do not set `flow_control`.
- `src/builtin/special.rs::builtin_continue`: same guard for
  out-of-loop case. For in-loop case, clamp the operand:
  `let clamped = n.min(env.exec.loop_depth); FlowControl::Continue(clamped)`.
  Same clamp applies to `break n`.

**Why a scope guard**: the body of `exec_for` /`exec_while_until` has
several early-return paths (`return Err(...)`, `return cond_status`
on flow_control, plain `break` from the Rust loop). Forgetting to
decrement on any one path leaves stale depth state for the rest of the
shell session. The guard is the only way to guarantee correctness
without auditing every existing and future return.

### 4.2 G2 — Identifier validation in unset/readonly/export (3 tests)

POSIX (§2.14.18 unset, §2.14.11 readonly, §2.14.9 export) requires that
the operand be a valid name (`[A-Za-z_][A-Za-z0-9_]*`). On failure, the
shell shall write a diagnostic and the builtin shall return non-zero
status.

**Changes:**

- `src/parser/word.rs`: change `pub(super) fn is_valid_name(s: &str) -> bool`
  to `pub(crate) fn is_valid_name(s: &str) -> bool`. No code-level
  changes to the function body; only the visibility.
- `src/builtin/special.rs`:
  - In `builtin_unset`: before calling `env.vars.unset(name)`, check
    `is_valid_name(name)`. On failure:
    `eprintln!("yosh: unset: `{}': not a valid identifier", name);`,
    `status = 1`, `continue` to the next operand.
  - In `builtin_readonly`: after splitting on `=`, validate the LHS
    (`name`) the same way. Bare operand without `=` is also validated.
    On failure: same diagnostic format with `readonly:` prefix.
  - In `builtin_export`: same pattern with `export:` prefix.
- POSIX semantics: an invalid operand sets exit status to 1 but does
  not abort the remaining operands. This matches the existing
  per-iteration `status = 1; continue;` pattern in `builtin_export`.

### 4.3 G3 — `unset -f` / `-v` flag handling (2 tests)

POSIX (§2.14.18): `-f` unsets a function; `-v` unsets a variable
(default). The two are mutually exclusive.

**Changes:**

- `src/builtin/special.rs::builtin_unset`: rewrite the argument loop to
  first parse leading flags:
  - `-f` selects function mode.
  - `-v` selects variable mode (explicit form of the default).
  - `-fv` / `-vf` are option strings; reject with "yosh: unset: cannot
    simultaneously unset a function and a variable" (status 2) — POSIX
    says the result is unspecified, but rejection is the safest
    behavior and matches dash/bash.
  - `--` ends option parsing; the rest are operand names.
  - Bare `-` and anything else are treated as operand names.
- Variable mode: existing `env.vars.unset(name)` path.
- Function mode: `env.functions.remove(name); Ok(())`. The HashMap
  removal is infallible — no readonly-equivalent for functions in
  POSIX. Removing a non-existent function is a no-op with exit 0
  (POSIX explicitly allows this).
- Identifier validation from G2 applies in both modes.

### 4.4 G4 — `readonly -p` listing (1 test)

POSIX (§2.14.11): `readonly -p` lists read-only variables in a form
that can be re-input to the shell.

**Changes:**

- `src/builtin/special.rs::builtin_readonly`: change the entry guard
  from `if args.is_empty()` to `if args.is_empty() || args[0] == "-p"`.
  Reuse the existing listing branch. The output format is already
  `readonly NAME=value` per line, which satisfies `grep '^readonly
  myvar'` in the test.

### 4.5 G5 — `exec` env propagation and special-builtin redirect-error exit (2 tests)

#### exec_keeps_env

POSIX (§2.14.10): when `exec` replaces the shell with a command, the
new process inherits the exported environment.

**Current bug**: `builtin_exec` uses `nix::unistd::execvp`, which
inherits the calling process's environment. yosh maintains its own
variable store; `export` marks a variable as exported but the process
environment is not necessarily kept in sync, so `execvp` sees stale or
missing values.

**Fix**: build an explicit `envp: Vec<CString>` from
`env.vars.environ()` (which returns the canonical `(name, value)` list
of currently-exported variables) and pass it via `execvpe`. On macOS
`nix` does not expose `execvpe`; resolve the path via the existing
`src/exec/command.rs::lookup_in_path` helper and call
`nix::unistd::execve` directly.

```rust
let envp: Vec<CString> = env.vars.environ()
    .iter()
    .filter_map(|(k, v)| CString::new(format!("{}={}", k, v)).ok())
    .collect();
// resolve path, then:
let err = execve(&resolved, &c_args, &envp).unwrap_err();
```

For relative or bare command names, walk `PATH` exactly as the
existing `find_in_path` does. If not found, return the existing
`CommandNotFound` error.

#### special_builtin_redir_error_exits

POSIX (§2.8.1): a redirection error on a special builtin in a
non-interactive shell shall exit the shell.

**Current bug**: `src/exec/simple.rs` returns
`Err(ShellError::runtime(RedirectFailed, …))` for both special and
regular builtins; the caller treats both alike (status 1, continue).

**Fix**: in `src/exec/simple.rs` at the `BuiltinKind::Special` arm
where `redirect_state.apply` fails, branch on
`self.env.mode.is_interactive`:

- Non-interactive: write the existing diagnostic to stderr, then call
  `std::process::exit(1)`. Since `(...)` subshells run in a fork()ed
  child, exiting the child does not affect the parent process. The
  test `(: < /nonexistent ; echo not-reached) ; :` runs the subshell
  in a child; the child exits before `echo not-reached`; the parent
  then runs `:` and exits 0.
- Interactive: keep existing behavior (status 1, continue).

The diagnostic itself is unchanged.

## 5. Verification

Each group's verification is:

1. Remove `# XFAIL: …` from each test file listed in §3.
2. Run `./e2e/run_tests.sh --filter=<test-name>` per test, expecting
   PASS.
3. Add focused unit tests to `src/builtin/special.rs::tests`:
   - **G1**: `loop_depth == 0` returns `Ok(1)` from `builtin_break` and
     leaves `flow_control` as `None`; `continue 5` inside a 1-deep
     loop produces `FlowControl::Continue(1)`.
   - **G2**: `unset 1foo` returns status 1 and writes a diagnostic
     containing `unset`; `readonly 1foo=v` and `export 1foo=v` likewise.
   - **G3**: after `foo() { … }; unset -f foo`, `env.functions` no
     longer contains `foo`; after `foo() { … }; foo=v; unset -f foo`,
     `env.vars.get("foo") == Some("v")`.
   - **G4**: after `readonly myvar=v`, `builtin_readonly(&["-p"], …)`
     prints a line starting with `readonly myvar`.
   - **G5**: not unit-testable (`execve` replaces the process; subshell
     `process::exit` ends the test process). E2E-only.

4. Run `cargo test`, `./e2e/run_tests.sh` (full), and
   `cargo fmt --all -- --check` before each commit. SP1 is closed only
   when all three are clean.

## 6. Implementation order

1. **G2** (identifier validation) — smallest diff, lowest risk; warms
   up the workflow.
2. **G4** (`readonly -p`) — one-line branch addition reusing existing
   listing code.
3. **G3** (`unset -f`) — argument-parser rewrite; depends on G2 for
   identifier validation in function mode.
4. **G1** (loop depth) — `ExecState` field plus `compound.rs` guards;
   the highest churn but well-isolated.
5. **G5** (`exec` env, special-builtin redirect-error exit) — broadest
   surface (`execve` plumbing + `simple.rs` branch change); save for
   last so earlier wins are not blocked by it.

Each step is its own commit.

## 7. Risks

- **G1 decrement leaks**: every early return in `exec_for` and
  `exec_while_until` must decrement `loop_depth`. The scope-guard
  approach prevents accidental skips. Audit existing tests for
  `for`/`while`/`until` to confirm depth always returns to 0.
- **G5 `execve` portability**: `nix::unistd::execvpe` is Linux-only;
  the macOS code path needs manual `PATH` resolution. Reuse
  `src/exec/command.rs` helpers to avoid divergence.
- **G5 subshell-exit assumption**: relies on `(...)` running in a
  forked child whose `mode.is_interactive` is inherited as `false`
  under non-interactive script execution. Verify with a smoke test
  before relying on `process::exit(1)`. If `mode.is_interactive` is
  somehow `true` in the subshell, fall back to setting a new
  `ShellError::SpecialBuiltinFatal` variant that propagates up and
  causes the subshell-runner to exit. Decide during implementation
  based on what `simple.rs` actually observes.
- **G3 readonly flag combinations**: dash/bash differ slightly on
  `unset -fv name`; the design chooses rejection (status 2). Worth a
  smoke test against the actual e2e suite to confirm no other test
  relies on the bash-permissive behavior.

## 8. Out of scope

- `read` builtin (SP3): `exec_redir_input.sh` is moved there.
- Diagnostic message standardization across all builtins (SP-wide
  refactor): only the 11 specific messages above are touched.
- Function-name identifier semantics beyond POSIX `[A-Za-z_][A-Za-z0-9_]*`:
  bash accepts colons and dots in function names; yosh stays on the
  POSIX subset.

## 9. Acceptance criterion

- All 11 tests listed in §3 pass under `./e2e/run_tests.sh` with no
  `# XFAIL:` line.
- `cargo test` green; `cargo fmt --all -- --check` clean.
- The roadmap spec reflects the SP1→SP3 move of `exec_redir_input.sh`.
- TODO.md `E2E XFAIL Roadmap` section is created during SP1 kickoff
  and the SP1 entry is removed on closure.
