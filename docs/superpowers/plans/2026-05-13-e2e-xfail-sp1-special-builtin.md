# SP1 — Special-builtin Diagnostics & Semantics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 11 XFAIL tests in `e2e/posix_spec/4_special_builtin/` and `e2e/posix_spec/2_08_01_consequences_of_shell_errors/` so each runs as a normal expectation.

**Architecture:** Five focused groups of changes against `src/builtin/special.rs`, `src/env/exec_state.rs`, `src/exec/compound.rs`, and `src/exec/simple.rs`. One group = one commit = one or more XFAIL tests cleared. Each task is preceded by a failing-test reproduction step (cargo unit test or e2e) and concluded by a commit.

**Tech Stack:** Rust 2024 edition, `nix` 0.31 (`unistd::execve`), existing `parser::word::is_valid_name` for identifier validation, existing `exec_child` helper for fatal subshell exit.

**Spec:** `docs/superpowers/specs/2026-05-13-e2e-xfail-sp1-special-builtin-design.md`

---

## Task 1 — G2: Identifier validation in unset/readonly/export (3 tests)

**Files:**
- Modify: `src/parser/word.rs` (line 24 — visibility change)
- Modify: `src/builtin/special.rs` (`builtin_unset`, `builtin_readonly`, `builtin_export`)
- Test: `e2e/posix_spec/4_special_builtin/{unset_invalid_name,readonly_invalid_name,export_invalid_name}.sh`
- Test (unit): `src/builtin/special.rs::tests`

POSIX (§2.14.18 unset / §2.14.11 readonly / §2.14.9 export) requires the operand name to match `[A-Za-z_][A-Za-z0-9_]*`. The existing helper `is_valid_name` already implements this rule; we make it `pub(crate)` and call it before each mutating operation.

- [ ] **Step 1.1: Confirm the three tests currently fail with their XFAIL line removed**

```bash
sed -i.bak '/^# XFAIL:/d' \
    e2e/posix_spec/4_special_builtin/unset_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/readonly_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/export_invalid_name.sh
./e2e/run_tests.sh --filter=unset_invalid_name
./e2e/run_tests.sh --filter=readonly_invalid_name
./e2e/run_tests.sh --filter=export_invalid_name
```
Expected: all three FAIL (exit 0, no stderr). Keep the edited files (no `.bak` cleanup yet — we'll commit them at the end of Task 1).

- [ ] **Step 1.2: Promote `is_valid_name` visibility**

Edit `src/parser/word.rs`. Change line 24:

```rust
// Before
pub(super) fn is_valid_name(s: &str) -> bool {

// After
pub(crate) fn is_valid_name(s: &str) -> bool {
```

- [ ] **Step 1.3: Add `use` for `is_valid_name` at top of `special.rs`**

Edit `src/builtin/special.rs`. After the existing `use crate::env::...` block (around line 6), add:

```rust
use crate::parser::word::is_valid_name;
```

- [ ] **Step 1.4: Validate names in `builtin_unset`**

Edit `src/builtin/special.rs::builtin_unset` (currently lines 118–127). Replace with:

```rust
fn builtin_unset(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let mut status = 0;
    for name in args {
        if !is_valid_name(name) {
            eprintln!("yosh: unset: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if let Err(e) = env.vars.unset(name) {
            eprintln!("yosh: unset: {}", e);
            status = 1;
        }
    }
    Ok(status)
}
```

- [ ] **Step 1.5: Validate names in `builtin_readonly`**

Edit `src/builtin/special.rs::builtin_readonly`. In the `for arg in args` loop (currently lines 147–162), replace the body with:

```rust
    for arg in args {
        let name = match arg.find('=') {
            Some(pos) => &arg[..pos],
            None => arg.as_str(),
        };
        if !is_valid_name(name) {
            eprintln!("yosh: readonly: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if let Some(pos) = arg.find('=') {
            let raw_value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, raw_value) {
                eprintln!("yosh: readonly: {}", e);
                status = 1;
                continue;
            }
            env.vars.set_readonly(name);
        } else {
            env.vars.set_readonly(name);
        }
    }
```

- [ ] **Step 1.6: Validate names in `builtin_export`**

Edit `src/builtin/special.rs::builtin_export`. In the `for arg in args` loop (currently lines 98–114), replace the body with:

```rust
    for arg in args {
        let name = match arg.find('=') {
            Some(pos) => &arg[..pos],
            None => arg.as_str(),
        };
        if !is_valid_name(name) {
            eprintln!("yosh: export: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if let Some(pos) = arg.find('=') {
            let raw_value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, raw_value) {
                eprintln!("yosh: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else {
            env.vars.export(name);
        }
    }
```

- [ ] **Step 1.7: Add unit tests in `src/builtin/special.rs::tests`**

Append to the existing `mod tests` block (before its closing `}` at line 823):

```rust
    #[test]
    fn unset_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("unset", &["1foo".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["1foo=v".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn export_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["1foo=v".to_string()], &mut executor);
        assert_eq!(status, 1);
    }
```

- [ ] **Step 1.8: Build and run the unit tests**

```bash
cargo build
cargo test -p yosh --lib special::tests
```
Expected: the three new tests pass, no other test regresses.

- [ ] **Step 1.9: Run the three e2e tests**

```bash
./e2e/run_tests.sh --filter=unset_invalid_name
./e2e/run_tests.sh --filter=readonly_invalid_name
./e2e/run_tests.sh --filter=export_invalid_name
```
Expected: all three PASS.

- [ ] **Step 1.10: Clean up `.bak` files (if any leaked) and commit**

```bash
find e2e -name '*.bak' -delete
git add src/parser/word.rs src/builtin/special.rs \
    e2e/posix_spec/4_special_builtin/unset_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/readonly_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/export_invalid_name.sh
git commit -m "$(cat <<'EOF'
fix(builtin): reject invalid identifiers in unset/readonly/export

Task: SP1 G2 — identifier validation. POSIX §2.14.9/§2.14.11/§2.14.18
require special builtins to reject operands that are not valid names
(i.e., do not match [A-Za-z_][A-Za-z0-9_]*). yosh previously accepted
any string, leaving the e2e tests
unset_invalid_name/readonly_invalid_name/export_invalid_name XFAIL.

Promotes parser::word::is_valid_name to pub(crate) and uses it as a
gate before each mutating operation; on failure, writes a diagnostic
to stderr and continues with the remaining operands (status=1).

Closes XFAIL for:
- e2e/posix_spec/4_special_builtin/unset_invalid_name.sh
- e2e/posix_spec/4_special_builtin/readonly_invalid_name.sh
- e2e/posix_spec/4_special_builtin/export_invalid_name.sh

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — G4: `readonly -p` listing (1 test)

**Files:**
- Modify: `src/builtin/special.rs::builtin_readonly`
- Test: `e2e/posix_spec/4_special_builtin/readonly_p_listing.sh`
- Test (unit): `src/builtin/special.rs::tests`

POSIX (§2.14.11): `readonly -p` writes the list of read-only variables to stdout in a form re-inputtable to the shell.

- [ ] **Step 2.1: Confirm the test currently fails with the XFAIL line removed**

```bash
sed -i.bak '/^# XFAIL:/d' e2e/posix_spec/4_special_builtin/readonly_p_listing.sh
./e2e/run_tests.sh --filter=readonly_p_listing
```
Expected: FAIL (empty output from `readonly -p`).

- [ ] **Step 2.2: Treat `-p` as equivalent to the no-args listing branch**

Edit `src/builtin/special.rs::builtin_readonly`. Change the entry guard (currently line 130):

```rust
// Before
    if args.is_empty() {

// After
    if args.is_empty() || (args.len() == 1 && args[0] == "-p") {
```

The body of the branch (lines 131–144) is unchanged — it already prints `readonly NAME=value` per variable.

- [ ] **Step 2.3: Add a unit test for `readonly -p`**

Append to the `mod tests` block in `src/builtin/special.rs`:

```rust
    #[test]
    fn readonly_p_lists_readonly_var() {
        let mut executor = Executor::new("yosh", vec![]);
        exec_special_builtin("readonly", &["myvar=v".to_string()], &mut executor);
        let status = exec_special_builtin("readonly", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
        // The actual listing is on stdout (println!) which we don't capture here;
        // smoke-test via the e2e suite for output content.
    }
```

- [ ] **Step 2.4: Build and run the unit test**

```bash
cargo build
cargo test -p yosh --lib readonly_p_lists_readonly_var
```
Expected: PASS.

- [ ] **Step 2.5: Run the e2e test**

```bash
./e2e/run_tests.sh --filter=readonly_p_listing
```
Expected: PASS.

- [ ] **Step 2.6: Commit**

```bash
find e2e -name '*.bak' -delete
git add src/builtin/special.rs e2e/posix_spec/4_special_builtin/readonly_p_listing.sh
git commit -m "$(cat <<'EOF'
fix(builtin): treat `readonly -p` as the listing form

Task: SP1 G4 — readonly -p listing. POSIX §2.14.11 requires
`readonly -p` to print read-only variables in re-inputtable form.
yosh previously printed nothing for `-p`, leaving the e2e test XFAIL.

Folds `-p` into the existing no-args branch, which already emits
`readonly NAME=value` per variable.

Closes XFAIL for:
- e2e/posix_spec/4_special_builtin/readonly_p_listing.sh

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 — G3: `unset -f` / `-v` flag handling (2 tests)

**Files:**
- Modify: `src/builtin/special.rs::builtin_unset`
- Test: `e2e/posix_spec/4_special_builtin/{unset_f_function,unset_f_keeps_variable}.sh`
- Test (unit): `src/builtin/special.rs::tests`

POSIX (§2.14.18): `-f` selects function mode, `-v` selects variable mode (default). The two flags must not be combined.

- [ ] **Step 3.1: Confirm the two tests fail with their XFAIL lines removed**

```bash
sed -i.bak '/^# XFAIL:/d' \
    e2e/posix_spec/4_special_builtin/unset_f_function.sh \
    e2e/posix_spec/4_special_builtin/unset_f_keeps_variable.sh
./e2e/run_tests.sh --filter=unset_f_function
./e2e/run_tests.sh --filter=unset_f_keeps_variable
```
Expected: both FAIL.

- [ ] **Step 3.2: Rewrite `builtin_unset` with flag parsing**

Edit `src/builtin/special.rs`. Replace the entire `builtin_unset` function (currently lines 118–127 — note: after Task 1 the lines may differ; locate by function name):

```rust
fn builtin_unset(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // Parse leading flags. Default mode is variable (-v).
    let mut mode_f = false;
    let mut mode_v = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" => mode_f = true,
            "-v" => mode_v = true,
            "--" => {
                i += 1;
                break;
            }
            s if s.starts_with('-') && s.len() > 1 => {
                eprintln!("yosh: unset: {}: invalid option", s);
                return Ok(2);
            }
            _ => break,
        }
        i += 1;
    }
    if mode_f && mode_v {
        eprintln!("yosh: unset: cannot simultaneously unset a function and a variable");
        return Ok(2);
    }
    let function_mode = mode_f;

    let mut status = 0;
    for name in &args[i..] {
        if !is_valid_name(name) {
            eprintln!("yosh: unset: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if function_mode {
            env.functions.remove(name);
        } else if let Err(e) = env.vars.unset(name) {
            eprintln!("yosh: unset: {}", e);
            status = 1;
        }
    }
    Ok(status)
}
```

- [ ] **Step 3.3: Add unit tests for unset -f**

`FunctionDef` is `{ name: String, body: Rc<CompoundCommand>, redirects: Vec<Redirect> }` (see `src/parser/ast.rs:115`). Constructing one literally is verbose; the tests use `eval_string` to install the function via the parser instead.

Append to the `mod tests` block in `src/builtin/special.rs`:

```rust
    #[test]
    fn unset_f_removes_function() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        assert!(executor.env.functions.contains_key("foo"));
        let status = exec_special_builtin("unset", &["-f".to_string(), "foo".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_f_keeps_variable_of_same_name() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        executor.env.vars.set("foo", "bar").unwrap();
        exec_special_builtin("unset", &["-f".to_string(), "foo".to_string()], &mut executor);
        assert_eq!(executor.env.vars.get("foo"), Some("bar"));
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_rejects_combined_f_v() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "-v".to_string(), "x".to_string()],
            &mut executor,
        );
        assert_eq!(status, 2);
    }
```

If `eval_string` does not exist on `Executor`, confirm with:

```bash
grep -n "pub fn eval_string\|fn eval_string" src/exec/mod.rs src/exec/*.rs
```

If it does not exist as a `pub` method but a similar one does (e.g., `exec_program` taking a parsed `Program`), parse first:

```rust
use crate::parser::Parser;
let prog = Parser::new_with_aliases("foo() { :; }", &executor.env.aliases)
    .parse_program()
    .unwrap();
executor.exec_program(&prog);
```

- [ ] **Step 3.4: Build and run the unit tests**

```bash
cargo build
cargo test -p yosh --lib unset_f
```
Expected: 3 tests pass (`unset_f_removes_function`, `unset_f_keeps_variable_of_same_name`, `unset_rejects_combined_f_v`).

- [ ] **Step 3.5: Run the two e2e tests**

```bash
./e2e/run_tests.sh --filter=unset_f_function
./e2e/run_tests.sh --filter=unset_f_keeps_variable
```
Expected: both PASS.

- [ ] **Step 3.6: Commit**

```bash
find e2e -name '*.bak' -delete
git add src/builtin/special.rs \
    e2e/posix_spec/4_special_builtin/unset_f_function.sh \
    e2e/posix_spec/4_special_builtin/unset_f_keeps_variable.sh
git commit -m "$(cat <<'EOF'
fix(builtin): implement `unset -f` / `-v` flag handling

Task: SP1 G3 — unset -f/-v. POSIX §2.14.18 requires -f to select
function mode and -v to select variable mode (default). yosh
previously treated -f as an operand name, so `unset -f foo` never
removed the function and removed only the variable named "-f" (which
didn't exist).

Rewrites the argument parser to recognize -f / -v / -- prefix flags,
rejects -fv / -vf combinations with status 2, and routes operand
removal to env.functions when -f is set.

Closes XFAIL for:
- e2e/posix_spec/4_special_builtin/unset_f_function.sh
- e2e/posix_spec/4_special_builtin/unset_f_keeps_variable.sh

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — G1: Loop-depth tracking for break/continue (3 tests)

**Files:**
- Modify: `src/env/exec_state.rs` (add `loop_depth` field)
- Modify: `src/exec/compound.rs` (`exec_for`, `exec_loop` — instrument depth tracking)
- Modify: `src/builtin/special.rs` (`builtin_break`, `builtin_continue` — guard + clamp)
- Test: `e2e/posix_spec/4_special_builtin/{break_outside_loop,continue_outside_loop,continue_n_exceeds_depth}.sh`
- Test (unit): `src/builtin/special.rs::tests`

POSIX (§2.14.1 / §2.14.5): break/continue outside any loop is invalid; n exceeding the enclosing loop count must use the outermost loop.

- [ ] **Step 4.1: Confirm the three tests fail with their XFAIL lines removed**

```bash
sed -i.bak '/^# XFAIL:/d' \
    e2e/posix_spec/4_special_builtin/break_outside_loop.sh \
    e2e/posix_spec/4_special_builtin/continue_outside_loop.sh \
    e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh
./e2e/run_tests.sh --filter=break_outside_loop
./e2e/run_tests.sh --filter=continue_outside_loop
./e2e/run_tests.sh --filter=continue_n_exceeds_depth
```
Expected: all three FAIL.

- [ ] **Step 4.2: Add `loop_depth` to `ExecState`**

Edit `src/env/exec_state.rs`. Replace the file with:

```rust
/// Flow control signals for break, continue, and return.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}

/// Execution-related state.
#[derive(Debug, Clone, Default)]
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
    /// Number of currently-executing loop bodies (for / while / until).
    /// Used by `break` / `continue` to detect out-of-loop usage and to
    /// clamp `n` against the outermost loop (POSIX §2.14.1 / §2.14.5).
    pub loop_depth: usize,
}
```

If `ExecState` is constructed explicitly with named fields anywhere (search with `grep -rn 'ExecState {' src/`), add `loop_depth: 0` to each construction. If it uses `Default::default()`, no further change needed.

- [ ] **Step 4.3: Verify `ExecState` construction sites**

```bash
grep -rn 'ExecState {' src/ 2>/dev/null
grep -rn 'ExecState::new\|ExecState::default' src/ 2>/dev/null
```
Add `loop_depth: 0` to any literal struct construction that lists all fields by name. Do nothing for `..Default::default()` patterns.

Run `cargo build` to confirm:

```bash
cargo build
```
Expected: clean build, no errors about missing fields.

- [ ] **Step 4.4: Instrument depth tracking in `exec_loop`**

Edit `src/exec/compound.rs::exec_loop` (currently lines 139–184). Wrap the original body with depth increment/decrement. The new function body:

```rust
    fn exec_loop(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        self.env.exec.loop_depth += 1;
        let status = self.exec_loop_inner(condition, body, until);
        self.env.exec.loop_depth -= 1;
        status
    }

    fn exec_loop_inner(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        let mut status = 0;
        loop {
            let cond_status = self.with_errexit_suppressed(|e| e.exec_body(condition));
            if self.env.exec.flow_control.is_some() {
                return cond_status;
            }
            let should_run = if until {
                cond_status != 0
            } else {
                cond_status == 0
            };
            if !should_run {
                break;
            }

            status = self.exec_body(body);

            match self.env.exec.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                }
                Some(other) => {
                    self.env.exec.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        status
    }
```

The `_inner` helper retains the original logic exactly; the outer function adds the balanced depth update.

- [ ] **Step 4.5: Instrument depth tracking in `exec_for`**

Edit `src/exec/compound.rs::exec_for` (currently lines 186–236). Apply the same `_inner`-helper pattern:

```rust
    fn exec_for(
        &mut self,
        var: &str,
        words: &Option<Vec<Word>>,
        body: &[CompleteCommand],
    ) -> Result<i32, ShellError> {
        self.env.exec.loop_depth += 1;
        let result = self.exec_for_inner(var, words, body);
        self.env.exec.loop_depth -= 1;
        result
    }

    fn exec_for_inner(
        &mut self,
        var: &str,
        words: &Option<Vec<Word>>,
        body: &[CompleteCommand],
    ) -> Result<i32, ShellError> {
        let items: Vec<String> = match words {
            Some(word_list) => match expand_words(&mut self.env, word_list) {
                Ok(words) => words,
                Err(e) => {
                    self.env.exec.last_exit_status = 1;
                    return Err(e);
                }
            },
            None => self.env.vars.positional_params().to_vec(),
        };

        let mut status = 0;
        for item in &items {
            if let Err(e) = self.env.vars.set(var, item.as_str()) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::ReadonlyVariable,
                    e.to_string(),
                ));
            }

            status = self.exec_body(body);

            match self.env.exec.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                }
                Some(other) => {
                    self.env.exec.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        Ok(status)
    }
```

- [ ] **Step 4.6: Add the depth guard to `builtin_break` and clamp `n`**

Edit `src/builtin/special.rs::builtin_break`. Replace the function (currently lines 190–212):

```rust
fn builtin_break(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if env.exec.loop_depth == 0 {
        eprintln!("yosh: break: only meaningful in a `for', `while', or `until' loop");
        return Ok(1);
    }
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    "break: loop count must be > 0".to_string(),
                ));
            }
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("break: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    let clamped = n.min(env.exec.loop_depth);
    env.exec.flow_control = Some(FlowControl::Break(clamped));
    Ok(0)
}
```

- [ ] **Step 4.7: Add the depth guard to `builtin_continue` and clamp `n`**

Edit `src/builtin/special.rs::builtin_continue`. Replace the function (currently lines 214–236):

```rust
fn builtin_continue(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if env.exec.loop_depth == 0 {
        eprintln!("yosh: continue: only meaningful in a `for', `while', or `until' loop");
        return Ok(1);
    }
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    "continue: loop count must be > 0".to_string(),
                ));
            }
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("continue: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    let clamped = n.min(env.exec.loop_depth);
    env.exec.flow_control = Some(FlowControl::Continue(clamped));
    Ok(0)
}
```

- [ ] **Step 4.8: Add unit tests for loop-depth behavior**

Append to the `mod tests` block in `src/builtin/special.rs`:

```rust
    #[test]
    fn break_outside_loop_returns_one_and_no_flow_control() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("break", &[], &mut executor);
        assert_eq!(status, 1);
        assert!(executor.env.exec.flow_control.is_none());
    }

    #[test]
    fn continue_outside_loop_returns_one_and_no_flow_control() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("continue", &[], &mut executor);
        assert_eq!(status, 1);
        assert!(executor.env.exec.flow_control.is_none());
    }

    #[test]
    fn continue_n_is_clamped_to_loop_depth() {
        use crate::env::FlowControl;
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.exec.loop_depth = 1;
        let status = exec_special_builtin("continue", &["5".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert_eq!(executor.env.exec.flow_control, Some(FlowControl::Continue(1)));
    }

    #[test]
    fn break_n_is_clamped_to_loop_depth() {
        use crate::env::FlowControl;
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.exec.loop_depth = 2;
        let status = exec_special_builtin("break", &["7".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert_eq!(executor.env.exec.flow_control, Some(FlowControl::Break(2)));
    }
```

- [ ] **Step 4.9: Build and run unit tests**

```bash
cargo build
cargo test -p yosh --lib
```
Expected: all four new tests pass, no regressions in the rest of the suite.

- [ ] **Step 4.10: Run the three e2e tests**

```bash
./e2e/run_tests.sh --filter=break_outside_loop
./e2e/run_tests.sh --filter=continue_outside_loop
./e2e/run_tests.sh --filter=continue_n_exceeds_depth
```
Expected: all three PASS.

- [ ] **Step 4.11: Smoke-test existing loops did not regress**

```bash
./e2e/run_tests.sh --filter=for
./e2e/run_tests.sh --filter=while
./e2e/run_tests.sh --filter=until
```
Expected: no new failures.

- [ ] **Step 4.12: Commit**

```bash
find e2e -name '*.bak' -delete
git add src/env/exec_state.rs src/exec/compound.rs src/builtin/special.rs \
    e2e/posix_spec/4_special_builtin/break_outside_loop.sh \
    e2e/posix_spec/4_special_builtin/continue_outside_loop.sh \
    e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh
git commit -m "$(cat <<'EOF'
fix(exec): track loop depth for break/continue diagnostics and clamp

Task: SP1 G1 — loop depth. POSIX §2.14.1 / §2.14.5 require break and
continue to be no-ops-with-error when used outside any loop, and to
clamp n to the outermost enclosing loop when n exceeds the loop count.

Adds `loop_depth: usize` to ExecState, incremented around `exec_for`
and `exec_loop` bodies via _inner helpers so the decrement is
exception-safe. break/continue now reject the loop_depth==0 case with
a diagnostic + status 1, and clamp n via min(n, loop_depth).

Closes XFAIL for:
- e2e/posix_spec/4_special_builtin/break_outside_loop.sh
- e2e/posix_spec/4_special_builtin/continue_outside_loop.sh
- e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 — G5a: `exec` preserves exported environment (1 test)

**Files:**
- Modify: `src/builtin/special.rs::builtin_exec`
- Test: `e2e/posix_spec/4_special_builtin/exec_keeps_env.sh`

POSIX (§2.14.10): when `exec` replaces the shell with a command, the new process inherits the exported environment. yosh currently calls `nix::unistd::execvp`, which inherits the calling process's `environ(7)` table — but yosh maintains its own variable store, so exports made within the shell session are not in `environ`.

`nix::unistd::execvpe` is Linux-only; on macOS we must resolve the PATH ourselves and call `execve`.

- [ ] **Step 5.1: Confirm the test fails with its XFAIL line removed**

```bash
sed -i.bak '/^# XFAIL:/d' e2e/posix_spec/4_special_builtin/exec_keeps_env.sh
./e2e/run_tests.sh --filter=exec_keeps_env
```
Expected: FAIL (empty output, expected `kept`).

- [ ] **Step 5.2: Rewrite `builtin_exec` to use `execve` with explicit envp**

Edit `src/builtin/special.rs`. Replace the entire `builtin_exec` function (currently lines 320–362):

```rust
fn builtin_exec(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Ok(0);
    }
    let cmd = &args[0];

    // Resolve the executable path. If the command contains `/`, treat as
    // a relative or absolute path. Otherwise walk $PATH.
    let resolved_path: std::path::PathBuf = if cmd.contains('/') {
        std::path::PathBuf::from(cmd)
    } else {
        let path_var = env.vars.get("PATH").unwrap_or("").to_string();
        match crate::exec::command::find_in_path(cmd, &path_var) {
            Some(p) => p,
            None => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::CommandNotFound,
                    format!("exec: {}: not found", cmd),
                ));
            }
        }
    };

    let c_path = match CString::new(resolved_path.as_os_str().as_encoded_bytes()) {
        Ok(s) => s,
        Err(_) => {
            return Err(ShellError::runtime(
                RuntimeErrorKind::ExecFailed,
                format!("exec: {}: invalid path", cmd),
            ));
        }
    };

    let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
    for a in args {
        match CString::new(a.as_str()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::ExecFailed,
                    format!("exec: {}: invalid argument", a),
                ));
            }
        }
    }

    // Build envp from currently-exported variables.
    let envp: Vec<CString> = env
        .vars
        .environ()
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{}={}", k, v)).ok())
        .collect();

    let err = nix::unistd::execve(&c_path, &c_args, &envp).unwrap_err();
    use nix::errno::Errno;
    match err {
        Errno::ENOENT => Err(ShellError::runtime(
            RuntimeErrorKind::CommandNotFound,
            format!("exec: {}: not found", cmd),
        )),
        Errno::EACCES => Err(ShellError::runtime(
            RuntimeErrorKind::PermissionDenied,
            format!("exec: {}: permission denied", cmd),
        )),
        _ => Err(ShellError::runtime(
            RuntimeErrorKind::ExecFailed,
            format!("exec: {}: {}", cmd, err),
        )),
    }
}
```

The `_env` parameter becomes `env` (used now). Update the call site in `exec_special_builtin` (around line 38) — it already passes `&mut executor.env`, so no change needed.

Also remove the now-unused `nix::unistd::execvp` import if `builtin_exec` was the sole consumer. Check with:

```bash
grep -n "use nix::unistd::execvp\|execvp(" src/builtin/special.rs
```

If `execvp` is no longer referenced, change the top `use` statement from:

```rust
use nix::unistd::execvp;
```

to remove the unused import (or just delete the line — `execve` is referenced by full path in the new body).

- [ ] **Step 5.3: Build and confirm no unused-import warnings**

```bash
cargo build 2>&1 | grep -E 'warning|error' | head -20
```
Expected: clean.

- [ ] **Step 5.4: Run the e2e test**

```bash
./e2e/run_tests.sh --filter=exec_keeps_env
```
Expected: PASS, output `kept`.

- [ ] **Step 5.5: Smoke-test that existing exec usages did not regress**

```bash
./e2e/run_tests.sh --filter=exec_
```
Expected: no new failures (the other exec_* tests should retain whatever state they had — only `exec_keeps_env` flips to PASS).

- [ ] **Step 5.6: Commit**

```bash
find e2e -name '*.bak' -delete
git add src/builtin/special.rs e2e/posix_spec/4_special_builtin/exec_keeps_env.sh
git commit -m "$(cat <<'EOF'
fix(builtin): exec preserves yosh's exported environment

Task: SP1 G5a — exec env. POSIX §2.14.10 requires `exec cmd` to pass
the current shell's exported variables to the replacement process.
yosh used nix::unistd::execvp which inherits the host environ table
but bypasses yosh's own variable store — so `export marker=v; exec sh
-c 'echo $marker'` printed empty.

Resolves the executable path via the existing find_in_path helper and
calls execve directly with an envp constructed from
ShellEnv::vars::environ(). execvpe is Linux-only in nix 0.31, hence
the manual PATH resolution for macOS portability.

Closes XFAIL for:
- e2e/posix_spec/4_special_builtin/exec_keeps_env.sh

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6 — G5b: Special-builtin redirect error exits non-interactive shell (1 test)

**Files:**
- Modify: `src/exec/simple.rs` (`BuiltinKind::Special` arm of `exec_simple_command`)
- Test: `e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh`

POSIX (§2.8.1): when a redirection error occurs on a special builtin in a non-interactive shell, the shell shall exit. yosh currently returns `Err(RedirectFailed)` which propagates as exit status 1 without terminating the shell.

- [ ] **Step 6.1: Confirm the test fails with its XFAIL line removed**

```bash
sed -i.bak '/^# XFAIL:/d' e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh
./e2e/run_tests.sh --filter=special_builtin_redir_error_exits
```
Expected: FAIL (test prints `not-reached`).

- [ ] **Step 6.2: Emit diagnostic and exit in `BuiltinKind::Special` redirect-failure path**

Edit `src/exec/simple.rs`. Locate the two `redirect_state.apply(...)` failure sites inside the `BuiltinKind::Special` arm (currently lines 318–322 for the `exec`-no-args case and 327–331 for the general case). For each, when the shell is non-interactive, emit the diagnostic to stderr and call `exit_child(1)` instead of returning an `Err`.

The `exec`-no-args branch (lines 317–325) becomes:

```rust
                // exec with no args: redirects persist (don't save/restore)
                if command_name == "exec" && args.is_empty() {
                    let mut redirect_state = RedirectState::new();
                    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, false) {
                        self.env.exec.last_exit_status = 1;
                        if !self.env.mode.is_interactive {
                            eprintln!("yosh: {}", e);
                            super::exit_child(1);
                        }
                        return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
                    }
                    self.env.exec.last_exit_status = 0;
                    return Ok(0);
                }
```

The general-case branch (lines 327–331) becomes:

```rust
                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    self.env.exec.last_exit_status = 1;
                    if !self.env.mode.is_interactive {
                        eprintln!("yosh: {}", e);
                        super::exit_child(1);
                    }
                    return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
                }
```

Note: `super::exit_child` is `pub(crate)` and is already used elsewhere in this file (e.g., line 492, 526). No new `use` needed.

- [ ] **Step 6.3: Build and run the e2e test**

```bash
cargo build
./e2e/run_tests.sh --filter=special_builtin_redir_error_exits
```
Expected: PASS — the subshell child exits before printing `not-reached`; the parent then runs `:` and exits 0.

- [ ] **Step 6.4: Verify the change does NOT affect interactive shells**

Cannot easily automate; sanity-check via PTY tests:

```bash
cargo test -p yosh --test pty_interactive 2>&1 | tail -5
```
Expected: no regressions. The new branch is gated by `!self.env.mode.is_interactive`.

- [ ] **Step 6.5: Smoke-test that other special-builtin redirect cases still report errors correctly**

```bash
./e2e/run_tests.sh --filter=redir
```
Expected: no new failures.

- [ ] **Step 6.6: Commit**

```bash
find e2e -name '*.bak' -delete
git add src/exec/simple.rs e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh
git commit -m "$(cat <<'EOF'
fix(exec): special-builtin redirect error exits non-interactive shell

Task: SP1 G5b — POSIX §2.8.1. A redirection error on a special
builtin in a non-interactive shell shall cause the shell to exit.
yosh previously returned RedirectFailed and continued execution, so
`(: < /nonexistent; echo not-reached)` printed `not-reached`.

In the BuiltinKind::Special arm of exec_simple_command, when
redirect_state.apply fails and the shell is non-interactive, write
the diagnostic to stderr and call exit_child(1) before returning.
Interactive shells retain the existing status=1-and-continue
behavior.

Closes XFAIL for:
- e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7 — SP1 closure: full verification and TODO.md update

**Files:**
- Modify: `TODO.md` (remove SP1 entry per project convention)

- [ ] **Step 7.1: Confirm all 11 SP1 tests pass and no XFAIL lines remain in the SP1 set**

```bash
for t in break_outside_loop continue_outside_loop continue_n_exceeds_depth \
         unset_invalid_name unset_f_function unset_f_keeps_variable \
         readonly_invalid_name readonly_p_listing \
         export_invalid_name exec_keeps_env \
         special_builtin_redir_error_exits; do
    ./e2e/run_tests.sh --filter="$t" 2>&1 | tail -3
done
grep -l '^# XFAIL:' \
    e2e/posix_spec/4_special_builtin/break_outside_loop.sh \
    e2e/posix_spec/4_special_builtin/continue_outside_loop.sh \
    e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh \
    e2e/posix_spec/4_special_builtin/unset_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/unset_f_function.sh \
    e2e/posix_spec/4_special_builtin/unset_f_keeps_variable.sh \
    e2e/posix_spec/4_special_builtin/readonly_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/readonly_p_listing.sh \
    e2e/posix_spec/4_special_builtin/export_invalid_name.sh \
    e2e/posix_spec/4_special_builtin/exec_keeps_env.sh \
    e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh \
    2>/dev/null
```
Expected: each `./e2e/run_tests.sh` reports PASS; the final `grep -l` prints nothing.

- [ ] **Step 7.2: Full regression**

```bash
cargo test -p yosh
./e2e/run_tests.sh
cargo fmt --all -- --check
```
Expected: cargo test green, e2e suite shows 11 fewer XFAILs and zero new failures, fmt clean.

- [ ] **Step 7.3: Remove the SP1 entry from `TODO.md`**

Edit `TODO.md`. Delete the line:

```markdown
- [ ] SP1 — Special-builtin error diagnostics & semantics (11 tests) — **in progress** — `docs/superpowers/specs/2026-05-13-e2e-xfail-sp1-special-builtin-design.md`
```

(Project convention is to delete completed items, not mark them `[x]`.)

- [ ] **Step 7.4: Commit the closure**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): mark SP1 complete by removing roadmap entry

Task: SP1 closure. All 11 XFAIL tests under SP1 now pass under
./e2e/run_tests.sh; cargo test and cargo fmt --all -- --check are
clean. Per project convention completed TODO items are deleted, not
marked [x].

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Acceptance criterion

SP1 closes when:

- All 11 e2e tests listed in Tasks 1–6 pass under `./e2e/run_tests.sh`.
- No `# XFAIL:` line remains in any of those 11 files.
- `cargo test -p yosh` green.
- `cargo fmt --all -- --check` clean.
- `TODO.md` no longer lists SP1.
- 7 commits added to `main` (one per task) — Tasks 1–6 each close at least one XFAIL, Task 7 records SP1 closure.
