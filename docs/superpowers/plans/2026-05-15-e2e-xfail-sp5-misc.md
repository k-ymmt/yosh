# SP5 — Miscellaneous POSIX Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the 8 SP5 XFAIL tests by implementing the corresponding POSIX behaviors (PPID, PS4, reserved word after assignment, `$?` from standalone `$(...)`, redirect-only command, redirect L-to-R, subshell EXIT trap, async INT trap).

**Architecture:** Eight targeted patches across parser, expander, redirect layer, trap machinery, env startup, xtrace formatting. Decomposed into 4 subsystem groups (env+xtrace, parser, exec/simple, trap), executed in reverse-risk order. One commit per sub-task (8 implementation commits) plus a final cleanup commit.

**Tech Stack:** Rust 2024 edition, `nix` crate for syscalls (`getppid`, `fork`, `sigaction`), POSIX shell semantics per IEEE Std 1003.1-2017. Test harness: built-in `cargo test` + `./e2e/run_tests.sh`.

**Spec:** [`docs/superpowers/specs/2026-05-15-e2e-xfail-sp5-misc-design.md`](../specs/2026-05-15-e2e-xfail-sp5-misc-design.md)

---

## File Surface (overview)

| File | Tasks touching it | Responsibility |
|------|-------------------|----------------|
| `src/env/mod.rs` | T1 | `ShellEnv::new` startup — add `PPID` init |
| `src/exec/simple.rs` | T1, T3, T4 | `exec_simple_command` — xtrace prefix, `$?` propagation, redirect-only |
| `src/parser/ast.rs` | T2 | `CompoundCommand` — add `assignments` field |
| `src/parser/compound.rs` | T2 | `CompoundCommand` construction site |
| `src/parser/mod.rs` | T2 | `parse_command` — consume prefix assignments |
| `src/exec/compound.rs` | T2, T6 | `exec_compound_command` apply temp assignments; `exec_subshell` fire EXIT trap |
| `src/exec/redirect.rs` | T5 | Diagnostic + fix location TBD (likely apply_one ordering) |
| `src/exec/control.rs` | T7 | `exec_complete_command` — drain pending signals |
| `e2e/posix_spec/**/*.sh` | T1-T7 | XFAIL header strip per sub-task |
| `TODO.md`, memory MEMORY.md | T8 | Roadmap closure, follow-up records |

---

## Task 1 — G4: PPID at startup + PS4 in xtrace (1 commit)

**Files:**
- Modify: `src/env/mod.rs:58-88` — `ShellEnv::new`
- Modify: `src/exec/simple.rs:174-176` — xtrace eprintln
- Modify: `e2e/posix_spec/8_env_vars/PPID_is_set.sh:4` — strip `# XFAIL: …`
- Modify: `e2e/posix_spec/8_env_vars/PS4_assigned.sh:4` — strip `# XFAIL: …`
- Test: `src/env/mod.rs` `mod tests` (add `test_shell_env_sets_ppid`)
- Test: `src/exec/simple.rs` `mod tests` (add `test_xtrace_prefix_uses_ps4`, `test_xtrace_prefix_default_when_ps4_unset`)

### Steps

- [ ] **Step 1: Write the failing unit test for PPID**

Add to `src/env/mod.rs` `mod tests` block:

```rust
#[test]
fn test_shell_env_sets_ppid_to_getppid() {
    let env = ShellEnv::new("yosh", vec![]);
    let ppid = env.vars.get("PPID").expect("PPID must be set");
    let expected = nix::unistd::getppid().as_raw().to_string();
    assert_eq!(
        ppid, expected,
        "$PPID must equal nix::unistd::getppid() at shell start"
    );
    let n: i32 = ppid.parse().expect("PPID must parse as integer");
    assert!(n > 0, "$PPID must be a positive integer, got {}", n);
}
```

- [ ] **Step 2: Write the failing unit tests for PS4**

Add to `src/exec/simple.rs` `mod tests` (or create the block if missing). The xtrace prefix logic is a one-liner so we extract it into a private helper for testability:

```rust
// In src/exec/simple.rs (production code section), expose:
fn xtrace_prefix(env: &crate::env::ShellEnv) -> &str {
    env.vars.get("PS4").unwrap_or("+ ")
}

// In tests module:
#[test]
fn test_xtrace_prefix_uses_ps4_when_set() {
    let mut env = crate::env::ShellEnv::new("yosh", vec![]);
    env.vars.set("PS4", "TRACE> ").unwrap();
    assert_eq!(xtrace_prefix(&env), "TRACE> ");
}

#[test]
fn test_xtrace_prefix_default_when_ps4_unset() {
    let env = crate::env::ShellEnv::new("yosh", vec![]);
    // PS4 is not set by ShellEnv::new, so the helper falls back to "+ ".
    assert_eq!(xtrace_prefix(&env), "+ ");
}
```

- [ ] **Step 3: Run the failing tests**

```sh
cargo test --lib --no-fail-fast \
  test_shell_env_sets_ppid_to_getppid \
  test_xtrace_prefix_uses_ps4_when_set \
  test_xtrace_prefix_default_when_ps4_unset
```

Expected: all three FAIL. PPID test fails with `PPID must be set`. PS4 tests fail with `cannot find function xtrace_prefix in this scope`.

- [ ] **Step 4: Implement PPID init in `ShellEnv::new`**

In `src/env/mod.rs`, modify the `OPTIND` init block (around line 62):

```rust
// POSIX: "OPTIND shall be initialized to 1 when the shell is invoked."
let _ = vars.set("OPTIND", "1");
// POSIX §2.5.3: $PPID is the parent PID of the invoking shell,
// captured once at startup. Subshells inherit the value.
let _ = vars.set("PPID", nix::unistd::getppid().as_raw().to_string());
```

`nix::unistd::getppid()` is already in `nix::unistd` and returns a `Pid`. Use `.as_raw()` to get the i32 representation.

- [ ] **Step 5: Implement PS4 lookup via `xtrace_prefix` helper**

In `src/exec/simple.rs`, add the helper above `impl Executor` and replace the xtrace eprintln at line 174:

```rust
fn xtrace_prefix(env: &crate::env::ShellEnv) -> &str {
    env.vars.get("PS4").unwrap_or("+ ")
}
```

Then at line 174–176, replace:

```rust
if self.env.mode.options.xtrace && !expanded.is_empty() {
    eprintln!("+ {}", expanded.join(" "));
}
```

With:

```rust
if self.env.mode.options.xtrace && !expanded.is_empty() {
    eprintln!("{}{}", xtrace_prefix(&self.env), expanded.join(" "));
}
```

- [ ] **Step 6: Run the unit tests, verify pass**

```sh
cargo test --lib --no-fail-fast \
  test_shell_env_sets_ppid_to_getppid \
  test_xtrace_prefix_uses_ps4_when_set \
  test_xtrace_prefix_default_when_ps4_unset
```

Expected: all three PASS.

- [ ] **Step 7: Strip XFAIL from the two E2E tests**

In `e2e/posix_spec/8_env_vars/PPID_is_set.sh`, delete the line:
```sh
# XFAIL: not yet implemented (TODO: set $PPID to parent process ID in shell startup)
```

In `e2e/posix_spec/8_env_vars/PS4_assigned.sh`, delete the line:
```sh
# XFAIL: not yet implemented (TODO: set -x PS4 prefix not honoured; hardcoded to '+ ')
```

- [ ] **Step 8: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=PPID_is_set
./e2e/run_tests.sh --filter=PS4_assigned
```

Expected: both PASS with `[PASS]` markers and no `[XFAIL]`/`[FAIL]`/`[XPass]`.

- [ ] **Step 9: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail line: `… XFail: 19 …` (21 − 2 = 19).

- [ ] **Step 10: Commit**

```sh
git add src/env/mod.rs src/exec/simple.rs \
  e2e/posix_spec/8_env_vars/PPID_is_set.sh \
  e2e/posix_spec/8_env_vars/PS4_assigned.sh
git commit -m "$(cat <<'EOF'
feat(env,exec): set $PPID at startup; honour PS4 in set -x

Two SP5 closures bundled because both add env-var-driven behavior.
- env/mod.rs: $PPID = getppid() captured once in ShellEnv::new
- exec/simple.rs: xtrace prefix derived from $PS4 (default "+ ")
- Strips XFAIL from PPID_is_set.sh + PS4_assigned.sh
- Tracked in SP5 spec sections §5 G4-1 / G4-2

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — G1: Reserved word after assignment (1 commit)

**Files:**
- Modify: `src/parser/ast.rs:62-66` — `CompoundCommand` struct, add `assignments` field
- Modify: `src/parser/compound.rs:33` — `CompoundCommand` constructor, init new field
- Modify: `src/parser/mod.rs:256-269` — `parse_command`, consume prefix assignments
- Modify: `src/exec/compound.rs:17-59` — `exec_compound_command`, apply/restore temp assignments
- Modify: `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh:4` — strip `# XFAIL: …`
- Test: `src/parser/simple.rs` `mod tests` (add 4 parser tests)
- Test: `src/exec/compound.rs` `mod tests` (add executor test)

### Steps

- [ ] **Step 1: Extend the `CompoundCommand` AST node**

In `src/parser/ast.rs`, modify the struct around line 62:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct CompoundCommand {
    pub kind: CompoundCommandKind,
    pub line: usize,
    /// Prefix assignments collected before a reserved-word-led compound,
    /// e.g. `x=1 if true; then ...; fi` carries `[Assignment { name: "x", value: Some("1") }]`.
    /// Applied as temporary assignments around the compound body per POSIX §2.9.1
    /// (extended to compound commands per §2.4 reserved-word recognition rules).
    pub assignments: Vec<Assignment>,
}
```

- [ ] **Step 2: Update the construction site**

In `src/parser/compound.rs:33`, change:

```rust
Ok(CompoundCommand { kind, line })
```

To:

```rust
Ok(CompoundCommand { kind, line, assignments: Vec::new() })
```

- [ ] **Step 3: Run cargo build to surface any other construction sites**

```sh
cargo build 2>&1 | grep -E "CompoundCommand.*missing|error\[" | head
```

Expected: no errors (only one construction site exists). If errors appear, set `assignments: Vec::new()` at each.

- [ ] **Step 4: Write the failing parser tests**

Add to `src/parser/simple.rs` `mod tests`:

```rust
#[test]
fn assignment_prefix_before_if_reserved_word_attaches_to_compound() {
    use super::super::ast::{Command, CompoundCommandKind};
    let mut parser = Parser::new("x=1 if true; then echo y; fi\n");
    let prog = parser.parse_program().unwrap();
    let cc = &prog.commands[0];
    let (aol, _) = &cc.items[0];
    let cmd = &aol.first.commands[0];
    let Command::Compound(comp, _redirs) = cmd else {
        panic!("expected Compound, got {:?}", cmd);
    };
    assert!(matches!(comp.kind, CompoundCommandKind::If { .. }));
    assert_eq!(comp.assignments.len(), 1);
    assert_eq!(comp.assignments[0].name, "x");
    assert_eq!(
        comp.assignments[0].value.as_ref().unwrap().as_literal(),
        Some("1")
    );
}

#[test]
fn assignment_prefix_before_while_attaches_to_compound() {
    use super::super::ast::{Command, CompoundCommandKind};
    let mut parser = Parser::new("a=hi while false; do :; done\n");
    let prog = parser.parse_program().unwrap();
    let Command::Compound(comp, _) = &prog.commands[0].items[0].0.first.commands[0] else {
        panic!()
    };
    assert!(matches!(comp.kind, CompoundCommandKind::While { .. }));
    assert_eq!(comp.assignments.len(), 1);
    assert_eq!(comp.assignments[0].name, "a");
}

#[test]
fn no_assignment_prefix_does_not_create_phantom_assignments() {
    use super::super::ast::Command;
    let mut parser = Parser::new("if true; then echo y; fi\n");
    let prog = parser.parse_program().unwrap();
    let Command::Compound(comp, _) = &prog.commands[0].items[0].0.first.commands[0] else {
        panic!()
    };
    assert!(comp.assignments.is_empty());
}

#[test]
fn assignment_then_simple_command_still_lands_in_simple() {
    use super::super::ast::Command;
    let mut parser = Parser::new("x=1 echo y\n");
    let prog = parser.parse_program().unwrap();
    let Command::Simple(sc) = &prog.commands[0].items[0].0.first.commands[0] else {
        panic!("expected Simple, got compound")
    };
    assert_eq!(sc.assignments.len(), 1);
    assert_eq!(sc.assignments[0].name, "x");
    assert_eq!(sc.words.len(), 1);
    assert_eq!(sc.words[0].as_literal(), Some("echo"));
}
```

- [ ] **Step 5: Write the failing executor test**

Add to `src/exec/compound.rs` `mod tests` (create the block if absent):

```rust
#[cfg(test)]
mod tests {
    use crate::exec::Executor;
    use crate::parser::Parser;

    #[test]
    fn compound_with_assignment_prefix_runs_inside_temp_scope() {
        let source = "y=initial\nx=replaced if true; then echo $x; fi\necho post=$x";
        let prog = Parser::new(source).parse_program().unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        // Capture stdout by running in-process; for simplicity rely on
        // env after execution to verify scope behavior.
        exec.exec_program(&prog);
        // The temp assignment must NOT persist past the compound.
        assert_eq!(exec.env.vars.get("x"), None, "x must not leak past compound");
        // The earlier permanent assignment must remain.
        assert_eq!(exec.env.vars.get("y"), Some("initial"));
    }
}
```

- [ ] **Step 6: Run the failing tests**

```sh
cargo test --lib --no-fail-fast \
  assignment_prefix_before_if_reserved_word_attaches_to_compound \
  assignment_prefix_before_while_attaches_to_compound \
  no_assignment_prefix_does_not_create_phantom_assignments \
  assignment_then_simple_command_still_lands_in_simple \
  compound_with_assignment_prefix_runs_inside_temp_scope
```

Expected: parser tests FAIL with `expected Compound, got Simple` (parser still treats `if` as a word). Executor test FAILS because `x` leaks.

- [ ] **Step 7: Modify `parse_command` to consume prefix assignments**

In `src/parser/mod.rs`, replace `parse_command` (lines 256-269):

```rust
pub(super) fn parse_command(&mut self) -> error::Result<Command> {
    if self.is_compound_command_start() {
        let compound = self.parse_compound_command()?;
        let redirects = self.parse_redirect_list()?;
        return Ok(Command::Compound(compound, redirects));
    }

    // POSIX §2.4: reserved words are recognized at command position even
    // after a leading assignment prefix. Consume any leading assignments
    // first; if the next token starts a compound command, attach the
    // assignments to the compound. Otherwise the assignments flow into
    // the simple-command path.
    let mut prefix_assignments = Vec::new();
    while let Token::Word(word_text) = &self.current.token {
        let word_clone = word_text.clone();
        if let Some(a) = Self::try_parse_assignment(&word_clone) {
            self.advance()?;
            prefix_assignments.push(a);
        } else {
            break;
        }
    }

    if !prefix_assignments.is_empty() && self.is_compound_command_start() {
        let mut compound = self.parse_compound_command()?;
        compound.assignments = prefix_assignments;
        let redirects = self.parse_redirect_list()?;
        return Ok(Command::Compound(compound, redirects));
    }

    if let Some(func_def) = self.try_parse_function_def()? {
        if !prefix_assignments.is_empty() {
            // POSIX: assignments before function definition are a syntax error.
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "assignments may not prefix a function definition",
            ));
        }
        return Ok(Command::FunctionDef(func_def));
    }

    // Hand the consumed assignments back into simple-command construction.
    let mut simple = self.parse_simple_command()?;
    if !prefix_assignments.is_empty() {
        prefix_assignments.append(&mut simple.assignments);
        simple.assignments = prefix_assignments;
    }
    Ok(Command::Simple(simple))
}
```

The `Token::Word(word_text)` matches `Word(Word)` from the lexer — confirm by checking `crate::lexer::token::Token` if it stores a `Word` struct or a string. Based on `src/parser/simple.rs:22` (`Token::Word(word)` then `word.clone()` and pass to `try_parse_assignment(&word)`), the variant holds a `Word` value directly.

- [ ] **Step 8: Modify `exec_compound_command` to apply prefix assignments**

In `src/exec/compound.rs:17-59`, modify the function to apply assignments before the kind dispatch and restore after:

```rust
pub(crate) fn exec_compound_command(
    &mut self,
    compound: &CompoundCommand,
    redirects: &[Redirect],
) -> Result<i32, ShellError> {
    let _ = self.env.vars.set("LINENO", compound.line.to_string());

    // POSIX §2.9.1 + §2.4: prefix assignments on a compound are
    // applied as temp assignments scoped to the compound body.
    let saved = if !compound.assignments.is_empty() {
        match self.apply_temp_assignments(&compound.assignments) {
            Ok(s) => s,
            Err(e) => {
                self.env.exec.last_exit_status = 1;
                return Err(e);
            }
        }
    } else {
        Vec::new()
    };

    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(redirects, &mut self.env, true) {
        if !compound.assignments.is_empty() {
            self.restore_assignments(saved);
        }
        self.env.exec.last_exit_status = 1;
        return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
    }

    let status = match &compound.kind {
        // ... existing arms unchanged ...
    };

    redirect_state.restore();
    if !compound.assignments.is_empty() {
        self.restore_assignments(saved);
    }
    self.env.exec.last_exit_status = status;
    Ok(status)
}
```

Reuse the existing `apply_temp_assignments` (`src/exec/simple.rs:574`) and `restore_assignments` (`src/exec/simple.rs:592`) helpers — they are `pub(crate)`.

- [ ] **Step 9: Run all G1 tests to verify pass**

```sh
cargo test --lib --no-fail-fast \
  assignment_prefix_before_if_reserved_word_attaches_to_compound \
  assignment_prefix_before_while_attaches_to_compound \
  no_assignment_prefix_does_not_create_phantom_assignments \
  assignment_then_simple_command_still_lands_in_simple \
  compound_with_assignment_prefix_runs_inside_temp_scope
```

Expected: all PASS.

- [ ] **Step 10: Strip XFAIL from the E2E test**

In `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh`, delete the line:
```sh
# XFAIL: not yet implemented (TODO: reserved word not recognized after assignment prefix; yosh treats it as a command name)
```

- [ ] **Step 11: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=reserved_after_assignment
```

Expected: PASS.

- [ ] **Step 12: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail: `… XFail: 18 …` (19 − 1 = 18).

- [ ] **Step 13: Commit**

```sh
git add src/parser/ast.rs src/parser/compound.rs src/parser/mod.rs \
  src/parser/simple.rs src/exec/compound.rs \
  e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh
git commit -m "$(cat <<'EOF'
feat(parser,exec): recognize reserved words after assignment prefix

Per POSIX §2.4, reserved words are recognized at command position even
when preceded by assignments. parse_command now consumes leading
assignment-form Words and dispatches to compound parsing if the next
token is a reserved-word compound start; the assignments are attached
to CompoundCommand and applied as temp scope around the body via the
existing apply_temp_assignments / restore_assignments helpers.

Closes SP5 §5 G1 (reserved_after_assignment_recognized.sh).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 — G2-1: `$?` propagation from standalone `$(...)` (1 commit)

**Files:**
- Modify: `src/exec/simple.rs:129-172` — assignment-only branch
- Modify: `e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh:6` — strip XFAIL
- Test: `src/exec/simple.rs` `mod tests` — add executor tests

### Steps

- [ ] **Step 1: Write the failing executor test**

Add to `src/exec/simple.rs` tests:

```rust
#[test]
fn standalone_false_cmd_sub_propagates_exit_status() {
    use crate::exec::Executor;
    use crate::parser::Parser;
    let mut exec = Executor::new("yosh", vec![]);
    let prog = Parser::new("$(false)\n").parse_program().unwrap();
    exec.exec_program(&prog);
    assert_eq!(
        exec.env.exec.last_exit_status, 1,
        "$? must reflect the substituted command's exit status"
    );
}

#[test]
fn standalone_true_cmd_sub_propagates_zero() {
    use crate::exec::Executor;
    use crate::parser::Parser;
    let mut exec = Executor::new("yosh", vec![]);
    let prog = Parser::new("$(true)\n").parse_program().unwrap();
    exec.exec_program(&prog);
    assert_eq!(exec.env.exec.last_exit_status, 0);
}

#[test]
fn cmd_sub_followed_by_echo_dollarquestion_outputs_status() {
    // Regression-friendly form: the test ensures the fix does not break
    // the existing assignment-side propagation.
    use crate::exec::Executor;
    use crate::parser::Parser;
    let mut exec = Executor::new("yosh", vec![]);
    let prog = Parser::new("x=$(false)\n").parse_program().unwrap();
    exec.exec_program(&prog);
    assert_eq!(exec.env.exec.last_exit_status, 1);
}
```

- [ ] **Step 2: Run the failing tests**

```sh
cargo test --lib --no-fail-fast \
  standalone_false_cmd_sub_propagates_exit_status \
  standalone_true_cmd_sub_propagates_zero \
  cmd_sub_followed_by_echo_dollarquestion_outputs_status
```

Expected: `standalone_false_cmd_sub_propagates_exit_status` FAILs with `assertion failed: left == right (left: 0, right: 1)`. The `true` and assignment-side variants likely PASS already.

- [ ] **Step 3: Implement the propagation fix**

In `src/exec/simple.rs`, modify the assignment-only branch (around line 129-171). Find the segment:

```rust
if expanded.is_empty() {
    // POSIX §2.9.1: exit status of an assignment-only command is the status
    // of the last command substitution performed, or 0 if none.
    // ...
    let mut last_cmd_sub_status: Option<i32> = None;
    for assignment in &cmd.assignments {
        // ...
    }
    let final_status = last_cmd_sub_status.unwrap_or(0);
    self.env.exec.last_exit_status = final_status;
    return Ok(final_status);
}
```

Replace the initializer line `let mut last_cmd_sub_status: Option<i32> = None;` with a check that seeds from the command-word side if any of `cmd.words` contained a command substitution. The word-side substitution runs during `expand_words` earlier in the function, which has already updated `env.exec.last_exit_status`:

```rust
if expanded.is_empty() {
    // POSIX §2.9.1: exit status of an assignment-only command is the status
    // of the last command substitution performed, or 0 if none. Both
    // command-word (`$(false)` alone) and assignment-value substitutions
    // are eligible. Word-side substitutions already updated
    // env.exec.last_exit_status during expand_words above.
    let words_had_cmd_sub = cmd.words.iter().any(word_has_command_sub);
    let mut last_cmd_sub_status: Option<i32> = if words_had_cmd_sub {
        Some(self.env.exec.last_exit_status)
    } else {
        None
    };
    for assignment in &cmd.assignments {
        // ... existing loop body unchanged ...
    }
    let final_status = last_cmd_sub_status.unwrap_or(0);
    self.env.exec.last_exit_status = final_status;
    return Ok(final_status);
}
```

Verify `word_has_command_sub` is in scope (it is — used at the existing line `assignment.value.as_ref().is_some_and(word_has_command_sub)`).

- [ ] **Step 4: Run the tests to verify pass**

```sh
cargo test --lib --no-fail-fast \
  standalone_false_cmd_sub_propagates_exit_status \
  standalone_true_cmd_sub_propagates_zero \
  cmd_sub_followed_by_echo_dollarquestion_outputs_status
```

Expected: all PASS.

- [ ] **Step 5: Strip XFAIL from the E2E test**

In `e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh`, delete the line:
```sh
# XFAIL: non-POSIX deviation (yosh sets $? to 0 after standalone command substitution; exit status of substituted command is not propagated)
```

- [ ] **Step 6: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=exit_status_propagates_to_parent
```

Expected: PASS.

- [ ] **Step 7: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail: `… XFail: 17 …`.

- [ ] **Step 8: Commit**

```sh
git add src/exec/simple.rs \
  e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh
git commit -m "$(cat <<'EOF'
fix(exec/simple): propagate $? from standalone command substitution

A simple command consisting only of `$(...)` left $? as 0 because the
assignment-only branch did not consider word-side command substitutions
when reducing last_cmd_sub_status. Seed last_cmd_sub_status from the
current env.exec.last_exit_status when any cmd.words contained a
substitution, mirroring the existing assignment-side rule (POSIX §2.9.1).

Closes SP5 §5 G2-1 (exit_status_propagates_to_parent.sh).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — G2-2: Redirect-only command applies redirects (1 commit)

**Files:**
- Modify: `src/exec/simple.rs:129-172` — assignment-only branch
- Modify: `e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh:6` — strip XFAIL
- Test: `src/exec/simple.rs` `mod tests` — add 3 tests

### Steps

- [ ] **Step 1: Write the failing executor tests**

Add to `src/exec/simple.rs` tests:

```rust
#[test]
fn redirect_only_creates_file() {
    use crate::exec::Executor;
    use crate::parser::Parser;

    let tmp = std::env::temp_dir().join(format!(
        "yosh_redirect_only_{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);

    let source = format!(">{}\n", tmp.display());
    let prog = Parser::new(&source).parse_program().unwrap();
    let mut exec = Executor::new("yosh", vec![]);
    exec.exec_program(&prog);

    assert!(tmp.exists(), "redirect-only command must create {}", tmp.display());
    let metadata = std::fs::metadata(&tmp).unwrap();
    assert_eq!(metadata.len(), 0, "the file must be truncated to zero bytes");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn redirect_only_truncates_existing_file() {
    use crate::exec::Executor;
    use crate::parser::Parser;

    let tmp = std::env::temp_dir().join(format!(
        "yosh_redirect_only_trunc_{}.txt",
        std::process::id()
    ));
    std::fs::write(&tmp, b"existing-content").unwrap();

    let source = format!(">{}\n", tmp.display());
    let prog = Parser::new(&source).parse_program().unwrap();
    let mut exec = Executor::new("yosh", vec![]);
    exec.exec_program(&prog);

    let metadata = std::fs::metadata(&tmp).unwrap();
    assert_eq!(metadata.len(), 0, "existing file must be truncated");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn redirect_only_returns_zero() {
    use crate::exec::Executor;
    use crate::parser::Parser;

    let tmp = std::env::temp_dir().join(format!(
        "yosh_redirect_only_rc_{}.txt",
        std::process::id()
    ));
    let source = format!(">{}\n", tmp.display());
    let prog = Parser::new(&source).parse_program().unwrap();
    let mut exec = Executor::new("yosh", vec![]);
    let status = exec.exec_program(&prog);

    assert_eq!(status, 0, "redirect-only command returns 0 on success");
    let _ = std::fs::remove_file(&tmp);
}
```

- [ ] **Step 2: Run the failing tests**

```sh
cargo test --lib --no-fail-fast \
  redirect_only_creates_file \
  redirect_only_truncates_existing_file \
  redirect_only_returns_zero
```

Expected: all FAIL because the file is never created.

- [ ] **Step 3: Implement redirect application in assignment-only branch**

In `src/exec/simple.rs`, modify the assignment-only branch (the same one Task 3 touched). Wrap the assignment loop in a redirect apply/restore:

```rust
if expanded.is_empty() {
    // ... existing words_had_cmd_sub seed from Task 3 ...
    let mut last_cmd_sub_status: Option<i32> = if words_had_cmd_sub {
        Some(self.env.exec.last_exit_status)
    } else {
        None
    };

    // POSIX §2.9.1: a command consisting only of redirections must still
    // apply them. Apply with save=true so the shell-process fds are
    // restored after this "command" completes (matches bash/dash).
    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
        self.env.exec.last_exit_status = 1;
        return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
    }

    for assignment in &cmd.assignments {
        // ... existing loop body unchanged ...
    }

    let final_status = last_cmd_sub_status.unwrap_or(0);
    redirect_state.restore();
    self.env.exec.last_exit_status = final_status;
    return Ok(final_status);
}
```

`RedirectState` is already imported at the top of the file (`use super::redirect::RedirectState;`). `ShellError` and `RuntimeErrorKind` are also already in scope.

- [ ] **Step 4: Run the tests to verify pass**

```sh
cargo test --lib --no-fail-fast \
  redirect_only_creates_file \
  redirect_only_truncates_existing_file \
  redirect_only_returns_zero
```

Expected: all PASS.

- [ ] **Step 5: Strip XFAIL from the E2E test**

In `e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh`, delete the line:
```sh
# XFAIL: not yet implemented (TODO: redirect-only command (no command word) should still apply redirections and create/truncate the file)
```

- [ ] **Step 6: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=redirection_only_creates_file
```

Expected: PASS.

- [ ] **Step 7: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail: `… XFail: 16 …`.

- [ ] **Step 8: Commit**

```sh
git add src/exec/simple.rs \
  e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh
git commit -m "$(cat <<'EOF'
fix(exec/simple): apply redirections on redirect-only commands

POSIX §2.9.1: when a simple command consists only of redirections (no
command word and no assignments — or assignment-only with redirects),
the redirections must still be performed. Wrap the assignment-only
branch in RedirectState apply/restore so `>f` creates / truncates the
target file.

Closes SP5 §5 G2-2 (redirection_only_creates_file.sh).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 — G2-3: Redirect L-to-R for external commands (1 commit, diagnostic + fix)

**Files (TBD until diagnostic):**
- Likely: `src/exec/simple.rs` or `src/exec/redirect.rs` — fix the ordering bug
- Modify: `e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh:4` — strip XFAIL
- Test: location depends on where the bug lives

### Steps

- [ ] **Step 1: Reproduce the failing E2E test in isolation**

```sh
cargo build
./e2e/run_tests.sh --filter=redir_order_left_to_right
```

Expected: `[XFAIL]` (reason: "yosh's actual output goes to f instead of just stdout going to f"). Use `bash -x` against the test script directly to see expected behavior:

```sh
bash e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh
```

bash's `cat f` should print only `out`.

- [ ] **Step 2: Run yosh against the test script and observe failure**

```sh
TEST_TMPDIR=$(mktemp -d) ./target/debug/yosh e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh
```

Capture both stdout and the file's contents. The expected mismatch is that the file contains both `out` and `err` lines, or other ordering deviation.

- [ ] **Step 3: Trace redirect application in `src/exec/redirect.rs::apply_one`**

Add temporary diagnostic eprintlns at the top of each `apply_one` arm to log `(target_fd, src/path)` and the order of arms entered. Re-run the test:

```sh
cargo build && \
  TEST_TMPDIR=$(mktemp -d) ./target/debug/yosh -c \
    'sh -c "echo out; echo err >&2" 2>&1 >/tmp/f; cat /tmp/f' 2>&1 | head -20
```

Confirm whether `apply_one` is entered in `2>&1` then `>f` order (L-to-R) or reversed.

- [ ] **Step 4: Identify root cause and decide fix location**

Possibilities (pick the one matching diagnostic output):

- **(a) AST stores redirects in reverse:** check `src/parser/redirect.rs` `parse_redirect_list`. Fix in parser by appending instead of prepending.
- **(b) `RedirectState::apply` iterates the wrong direction:** check `src/exec/redirect.rs:47`. Fix by ensuring `for redirect in redirects` (not `.rev()`).
- **(c) External-command fork path applies redirects via a different code path that reverses:** check `src/exec/simple.rs` and `src/exec/pipeline.rs` for places that call `RedirectState::apply` or `apply_one` directly.
- **(d) Other root cause** — document and fix.

If diagnostic shows L-to-R is correctly attempted but `dup2`-sequence still produces wrong output, look for `into_raw_fd` ownership leaks or duplicate `save_fd` calls.

Remove the temporary eprintlns once root cause is found.

- [ ] **Step 5: Write a regression unit test at the identified location**

If the fix is in `RedirectState::apply`, add a test in `src/exec/redirect.rs::tests`:

```rust
#[test]
fn redirect_dup_then_output_routes_stderr_to_terminal_and_stdout_to_file() {
    use crate::env::ShellEnv;
    use crate::parser::ast::{Redirect, RedirectKind, Word};

    let mut env = ShellEnv::new("yosh", vec![]);
    let tmp = std::env::temp_dir().join(format!(
        "yosh_lr_order_{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);

    // Save original fd 1 and 2 so the test does not corrupt the test runner.
    let saved_stdout = unsafe { libc::dup(1) };
    let saved_stderr = unsafe { libc::dup(2) };

    let redirs = vec![
        Redirect {
            fd: Some(2),
            kind: RedirectKind::DupOutput(Word::literal("1")),
        },
        Redirect {
            fd: Some(1),
            kind: RedirectKind::Output(Word::literal(tmp.to_str().unwrap())),
        },
    ];

    let mut state = crate::exec::redirect::RedirectState::new();
    state.apply(&redirs, &mut env, true).unwrap();

    // After L-to-R apply:
    //   fd 2 should be a dup of the *original* stdout (saved_stdout target)
    //   fd 1 should point at tmp file
    // Write to stdout — goes to file. Write to stderr — does NOT go to file.
    unsafe {
        libc::write(1, b"to-file\n".as_ptr() as *const _, 8);
        libc::write(2, b"to-stderr\n".as_ptr() as *const _, 10);
    }
    state.restore();

    // Restore the test runner's fds.
    unsafe {
        libc::dup2(saved_stdout, 1);
        libc::dup2(saved_stderr, 2);
        libc::close(saved_stdout);
        libc::close(saved_stderr);
    }

    let contents = std::fs::read_to_string(&tmp).unwrap();
    assert_eq!(contents, "to-file\n", "only stdout-side write must reach the file");
    let _ = std::fs::remove_file(&tmp);
}
```

This test is sensitive to fd corruption — if it leaves the test runner with mangled fds, other tests may fail. Run it standalone first.

- [ ] **Step 6: Run the failing test**

```sh
cargo test --lib --no-fail-fast \
  redirect_dup_then_output_routes_stderr_to_terminal_and_stdout_to_file -- --test-threads=1
```

Expected: FAIL (file contains both `to-file` and `to-stderr`).

- [ ] **Step 7: Apply the fix at the identified location**

Based on Step 4's diagnostic, edit the code to restore L-to-R ordering. The fix is one of:

- Reverse the parser's reversal (if (a)).
- Remove a `.rev()` from a redirect iterator (if (b)).
- Replace a divergent external-command redirect path with `RedirectState::apply` (if (c)).
- Fix the identified bug (if (d)).

- [ ] **Step 8: Run the test and verify pass**

```sh
cargo test --lib --no-fail-fast \
  redirect_dup_then_output_routes_stderr_to_terminal_and_stdout_to_file -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 9: Strip XFAIL from the E2E test**

In `e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh`, delete the line:
```sh
# XFAIL: not yet implemented (TODO: redirection left-to-right ordering; 2>&1 before >f should dup to original stdout not the post-redir target)
```

- [ ] **Step 10: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=redir_order_left_to_right
```

Expected: PASS.

- [ ] **Step 11: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail: `… XFail: 15 …`.

Pay particular attention to existing redirect E2E tests:
```sh
./e2e/run_tests.sh --filter=redir
./e2e/run_tests.sh --filter=2_07
```

- [ ] **Step 12: Commit**

```sh
git add <touched files> e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh
git commit -m "$(cat <<'EOF'
fix(exec/<path>): apply redirections left-to-right per POSIX §2.7

Diagnostic of `sh -c '...' 2>&1 >f` showed [describe root cause from
Step 4 — e.g., "the AST stored redirects in reverse" or "apply_one
captured a stale fd value before dup2"]. Restoring L-to-R semantics
makes `2>&1 >f` route stderr to the *current* stdout (terminal) and
then stdout to f, matching bash and dash.

Closes SP5 §5 G2-3 (redir_order_left_to_right.sh).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

The commit message body must be filled in with the actual root cause discovered in Step 4 before committing.

---

## Task 6 — G3-1: Subshell EXIT trap fires before exit_child (1 commit)

**Files:**
- Modify: `src/exec/compound.rs:86-100` — `exec_subshell` child branch
- Modify: `e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh:4` — strip XFAIL
- Test: `src/exec/compound.rs` `mod tests` — add subshell EXIT trap tests via process spawn

### Steps

- [ ] **Step 1: Write the failing integration test**

Subshell EXIT trap firing only manifests across a `fork`. We test via the actual yosh binary spawned as a subprocess. Add to `src/exec/compound.rs` `mod tests`:

```rust
#[test]
fn subshell_exit_trap_fires_on_paren_exit() {
    use std::process::Command;
    let yosh = env!("CARGO_BIN_EXE_yosh");
    let out = Command::new(yosh)
        .arg("-c")
        .arg("(trap 'echo bye' 0; :)")
        .output()
        .expect("yosh -c");
    assert!(out.status.success(), "yosh -c must exit 0; got {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("bye"),
        "subshell EXIT trap must fire; stdout was {:?}",
        stdout
    );
}

#[test]
fn subshell_exit_trap_runs_even_when_subshell_exits_nonzero() {
    use std::process::Command;
    let yosh = env!("CARGO_BIN_EXE_yosh");
    let out = Command::new(yosh)
        .arg("-c")
        .arg("(trap 'echo bye' 0; exit 5)")
        .output()
        .expect("yosh -c");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bye"), "stdout was {:?}", stdout);
    assert_eq!(out.status.code(), Some(5), "subshell exit code propagates");
}
```

The `env!("CARGO_BIN_EXE_yosh")` macro requires that the crate's main binary is named `yosh`. Confirm by checking `Cargo.toml` `[[bin]]` section — if the binary name differs, adjust the macro argument.

- [ ] **Step 2: Run the failing tests**

```sh
cargo build && cargo test --lib --no-fail-fast \
  subshell_exit_trap_fires_on_paren_exit \
  subshell_exit_trap_runs_even_when_subshell_exits_nonzero
```

Expected: both FAIL with `bye` missing from stdout.

- [ ] **Step 3: Modify `exec_subshell` to fire EXIT trap before `exit_child`**

In `src/exec/compound.rs:92-98`, modify the child branch:

```rust
Ok(ForkResult::Child) => {
    let ignored = self.env.traps.ignored_signals();
    self.env.traps.reset_non_ignored();
    signal::reset_child_signals(&ignored);
    let status = self.exec_body(body);
    // POSIX §2.11: EXIT pseudo-signal handler runs on shell exit,
    // including subshell exit. Fire BEFORE _exit so the action runs
    // in the child's environment.
    self.execute_exit_trap();
    super::exit_child(status);
}
```

`execute_exit_trap` is defined at `src/exec/mod.rs:153` and is already on the `Executor` impl.

- [ ] **Step 4: Run the tests to verify pass**

```sh
cargo build && cargo test --lib --no-fail-fast \
  subshell_exit_trap_fires_on_paren_exit \
  subshell_exit_trap_runs_even_when_subshell_exits_nonzero
```

Expected: both PASS.

- [ ] **Step 5: Strip XFAIL from the E2E test**

In `e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh`, delete the line:
```sh
# XFAIL: not yet implemented (TODO: trap 0/EXIT not fired on subshell exit)
```

- [ ] **Step 6: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=trap_zero_runs_on_exit
```

Expected: PASS.

- [ ] **Step 7: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail: `… XFail: 14 …`.

Pay attention to existing trap tests:
```sh
./e2e/run_tests.sh --filter=trap
```

- [ ] **Step 8: Commit**

```sh
git add src/exec/compound.rs \
  e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh
git commit -m "$(cat <<'EOF'
fix(exec/compound): fire EXIT trap on subshell child exit

Per POSIX §2.11, the EXIT pseudo-signal handler runs on shell exit
including subshell exit. exec_subshell child branch now calls
self.execute_exit_trap() before super::exit_child(status) so traps
installed inside `(trap 'cmd' 0; ...)` fire as expected.

Pipeline-child EXIT trap firing is intentionally out of scope (no SP5
test requires it; recorded as a TODO follow-up).

Closes SP5 §5 G3-1 (trap_zero_runs_on_exit.sh).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7 — G3-2: Drain pending signals at command boundary (1 commit)

**Files:**
- Modify: `src/exec/control.rs:156-…` — `exec_complete_command`
- Modify: `e2e/posix_spec/4_special_builtin/trap_int_handler.sh:4` — strip XFAIL
- Test: `src/exec/control.rs` `mod tests` — add an async signal test via process spawn

### Steps

- [ ] **Step 1: Write the failing integration test**

Add to `src/exec/control.rs` `mod tests`:

```rust
#[test]
fn sigint_trap_fires_between_commands() {
    use std::process::Command;
    let yosh = env!("CARGO_BIN_EXE_yosh");
    let script = "trap 'echo caught' INT\nkill -INT $$ 2>/dev/null\nsleep 0.05 2>/dev/null\necho after\n";
    let out = Command::new(yosh)
        .arg("-c")
        .arg(script)
        .output()
        .expect("yosh -c");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Both 'caught' and 'after' must appear, and 'caught' must come first.
    let caught_idx = stdout.find("caught").expect("stdout must contain 'caught'");
    let after_idx = stdout.find("after").expect("stdout must contain 'after'");
    assert!(
        caught_idx < after_idx,
        "trap output must precede 'after' line; got stdout = {:?}",
        stdout
    );
}
```

- [ ] **Step 2: Run the failing test**

```sh
cargo build && cargo test --lib --no-fail-fast sigint_trap_fires_between_commands
```

Expected: FAIL. Either `caught` is missing entirely or comes after `after`.

- [ ] **Step 3: Add the per-command signal drain**

In `src/exec/control.rs`, find `exec_complete_command` (around line 156). Add `self.process_pending_signals();` immediately before the final `status` return. The exact shape depends on the current function body — look for the last `return` or implicit-return expression and insert the drain right before it.

Conceptually:

```rust
pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
    // ... existing body (loop over items, AndOrList exec, etc.) ...

    self.process_pending_signals();   // NEW: handle async signals (POSIX §2.11)
    status
}
```

If the function has multiple return points, prefer adding the drain at each terminal location, or refactor to a single-return shape if cleaner.

`process_pending_signals` is on `Executor` (`src/exec/mod.rs:162`) and is `pub` — already accessible.

- [ ] **Step 4: Run the test and verify pass**

```sh
cargo build && cargo test --lib --no-fail-fast sigint_trap_fires_between_commands
```

Expected: PASS.

- [ ] **Step 5: Strip XFAIL from the E2E test**

In `e2e/posix_spec/4_special_builtin/trap_int_handler.sh`, delete the line:
```sh
# XFAIL: non-POSIX deviation (yosh defers INT trap to end-of-script; handler runs after 'after')
```

- [ ] **Step 6: Build and run the E2E filter**

```sh
cargo build && ./e2e/run_tests.sh --filter=trap_int_handler
```

Expected: PASS.

- [ ] **Step 7: Run the full test gate**

```sh
cargo test --lib --no-fail-fast
cargo test --features test-helpers --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green. E2E tail: `… XFail: 13 …`.

Pay particular attention to:
```sh
./e2e/run_tests.sh --filter=trap
./e2e/run_tests.sh --filter=signal
```

Also re-run interactive and pty tests because this change affects per-command flow:
```sh
cargo test --test interactive
cargo test --test pty_interactive  # may be slow
```

- [ ] **Step 8: Commit**

```sh
git add src/exec/control.rs \
  e2e/posix_spec/4_special_builtin/trap_int_handler.sh
git commit -m "$(cat <<'EOF'
fix(exec): drain pending signals at command boundary

POSIX §2.11 says trap actions for non-EXIT signals run as soon as the
shell is ready to accept them. yosh previously drained the self-pipe
only at script end, so `trap 'cmd' INT` followed by `kill -INT $$`
ran the trap after subsequent commands. Add a process_pending_signals
call at the end of exec_complete_command so async traps fire at the
next inter-command boundary.

$? interaction with trap action is left to the script (matches bash);
nested re-entry is acceptable because the self-pipe drain is idempotent.

Closes SP5 §5 G3-2 (trap_int_handler.sh).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8 — Final cleanup: TODO.md + memory + verify (1 commit)

**Files:**
- Modify: `TODO.md` — remove SP5 line from `## E2E XFAIL Roadmap`, append `### SP5 follow-ups (non-blocking)` section if any items were recorded during T1–T7
- Modify: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md` — update SP5 status to COMPLETE

### Steps

- [ ] **Step 1: Verify the E2E XFail count**

```sh
cargo build && ./e2e/run_tests.sh 2>&1 | tail -3
```

Expected tail line: `Total: <N>  Passed: <P>  Failed: 0  Timedout: 0  XFail: 13  XPass: 0`.

If `XFail != 13`, identify which test regressed and revisit the corresponding task.

- [ ] **Step 2: Confirm all 8 target tests pass (no XFAIL/XPass)**

```sh
for t in \
  posix_spec/2_04_reserved_words/reserved_after_assignment_recognized \
  posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent \
  posix_spec/2_07_redirection/redir_order_left_to_right \
  posix_spec/2_09_01_simple_commands/redirection_only_creates_file \
  posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit \
  posix_spec/4_special_builtin/trap_int_handler \
  posix_spec/8_env_vars/PPID_is_set \
  posix_spec/8_env_vars/PS4_assigned ; do
  echo "=== $t ==="
  ./e2e/run_tests.sh --filter="$(basename $t)"
done
```

Each should show `[PASS]`, no `[XFAIL]` / `[FAIL]` / `[XPass]`.

- [ ] **Step 3: Remove SP5 line from TODO.md**

In `TODO.md`, delete the line:
```markdown
- [ ] SP5 — Miscellaneous small POSIX features (8 tests)
```

If any non-blocking follow-ups were discovered during T1–T7, append a `### SP5 follow-ups (non-blocking)` section under the existing `### SP4 follow-ups (non-blocking)` block, mirroring the SP4 shape. Typical follow-up candidates from the SP5 spec:

- PS4 variable / arithmetic / command-sub expansion (literal-only is currently implemented).
- PS4 first-character-repeat rule for nesting depth.
- Pipeline-child EXIT trap firing (only subshell is fixed).
- `process_pending_signals` invocation in `exec_body` iteration tails or `exec_function_call` returns (currently only top-level `exec_complete_command`).

If none surfaced, skip adding the section.

- [ ] **Step 4: Update the memory file**

Read `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md` and:

1. Update the description frontmatter to mention SP5 complete.
2. Update the `Status (as of YYYY-MM-DD)` line.
3. Replace the `SP5 pending` bullet with a `SP5 COMPLETE (2026-05-15)` bullet pointing to this plan and the spec.
4. Update the closing arithmetic: `After SP1+SP2+SP3+SP4+SP5: 55 - 11 - 5 - 9 - 9 - 8 = 13 XFails remain`.

- [ ] **Step 5: Update MEMORY.md hook line**

Read `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/MEMORY.md` and update the SP5 line under "E2E XFAIL roadmap status":

```markdown
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1+SP2+SP3+SP4+SP5 COMPLETE (2026-05-15, 13 XFails remain); SP6-SP7 pending
```

- [ ] **Step 6: Re-run full gates one final time**

```sh
cargo test --lib --no-fail-fast
cargo test --features test-helpers --no-fail-fast
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./e2e/run_tests.sh
```

Expected: all green, `XFail: 13`.

- [ ] **Step 7: Commit**

```sh
git add TODO.md
# Memory paths are outside the repo; the memory update is not committed here.
git commit -m "$(cat <<'EOF'
chore(sp5): close SP5 — remove roadmap entry, record follow-ups

All 8 SP5 tests now pass under ./e2e/run_tests.sh (XFail: 13 = 21 - 8).
Per project convention, completed roadmap items are deleted (not
marked [x]). Any non-blocking polish items from T1–T7 are tracked in
TODO.md under "### SP5 follow-ups (non-blocking)".

Closes SP5 from the E2E XFAIL roadmap. SP6 + SP7 remain.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Acceptance (whole-plan)

- `./e2e/run_tests.sh` reports `XFail: 13`.
- `cargo test --lib`, `cargo test --features test-helpers`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` all green.
- 7 implementation commits + 1 chore commit on `main` (or PR branch).
- All 8 target tests' `# XFAIL` lines are removed.
- TODO.md roadmap section reflects SP5 removed.
- Memory `project_e2e_xfail_roadmap.md` reflects SP5 COMPLETE.

## Self-Review Notes

Spec coverage verified:
- §5 G4-1 PPID → T1 Step 4
- §5 G4-2 PS4 → T1 Step 5
- §5 G1 reserved-after-assign → T2 Steps 1, 2, 7, 8
- §5 G2-1 `$?` from `$(...)` → T3
- §5 G2-2 redirect-only → T4
- §5 G2-3 redirect L-to-R → T5 (diagnostic-driven, root cause TBD until Step 4)
- §5 G3-1 subshell EXIT → T6 Step 3
- §5 G3-2 INT async → T7 Step 3
- §10 acceptance → T8

Placeholder check: T5 has an intentional diagnostic phase (Steps 1-4)
because the spec deliberately defers root-cause identification to the
plan task. Commit message body in T5 Step 12 contains a literal
"[describe root cause from Step 4 …]" that MUST be filled in before
committing.

Type consistency: `apply_temp_assignments` and `restore_assignments`
are used in T2 Step 8 with the same signatures as the existing
call sites in `src/exec/simple.rs` (`pub(crate) fn apply_temp_assignments(&mut self, assignments: &[Assignment]) -> Result<Vec<(String, Option<String>)>, ShellError>` and `pub(crate) fn restore_assignments(&mut self, saved: Vec<(String, Option<String>)>)`).
