# Phase 5: Control Structure Execution — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement execution of all POSIX control structures (if/for/while/until/case), brace groups, subshells, function definition/invocation, and break/continue builtins.

**Architecture:** Add `FlowControl` enum and function store to `ShellEnv` for break/continue/return propagation and function registration. Implement compound command dispatcher in `Executor` that delegates to per-structure handlers. All control structures use a shared `exec_body` helper that executes command lists and checks for flow control signals after each command.

**Tech Stack:** Rust (edition 2024), nix 0.31, libc 0.2

---

## File Structure

| File | Changes |
|------|---------|
| `src/env/mod.rs` | Add `FlowControl` enum, `functions: HashMap<String, FunctionDef>`, `flow_control: Option<FlowControl>` to `ShellEnv` |
| `src/exec/mod.rs` | Add compound command dispatch, `exec_body`, handlers for each control structure, function invocation. Modify `exec_command`, `exec_simple_command`, `exec_and_or`, `exec_complete_command` for flow control propagation. |
| `src/builtin/mod.rs` | Add `break`, `continue` builtins. Modify `return` to set flow control. |
| `tests/parser_integration.rs` | Add Phase 5 integration tests |

No new files.

---

### Task 1: Infrastructure — FlowControl, function store, compound dispatch

**Files:**
- Modify: `src/env/mod.rs`
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Add FlowControl enum and function store to ShellEnv**

In `src/env/mod.rs`:

```rust
use std::collections::HashMap;
use crate::parser::ast::FunctionDef;

/// Flow control signals for break, continue, and return.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}
```

Add fields to `ShellEnv`:

```rust
pub struct ShellEnv {
    pub vars: VarStore,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    pub positional_params: Vec<String>,
    pub last_bg_pid: Option<i32>,
    pub functions: HashMap<String, FunctionDef>,
    pub flow_control: Option<FlowControl>,
}
```

Initialize in `ShellEnv::new`:

```rust
functions: HashMap::new(),
flow_control: None,
```

- [ ] **Step 2: Add exec_body helper and compound command dispatch**

In `src/exec/mod.rs`, update imports to include all AST types needed:

```rust
use crate::parser::ast::{
    AndOrList, AndOrOp, Assignment, CaseItem, CaseTerminator, Command, CompoundCommand,
    CompoundCommandKind, CompleteCommand, FunctionDef, Program, Redirect, SeparatorOp,
    SimpleCommand, Word,
};
use crate::env::FlowControl;
```

Add the `exec_body` helper method to `Executor`:

```rust
/// Execute a list of complete commands (a compound-list / body).
/// Checks for flow control signals after each command.
fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
    let mut status = 0;
    for cmd in body {
        status = self.exec_complete_command(cmd);
        if self.env.flow_control.is_some() {
            break;
        }
    }
    status
}
```

Add the `exec_compound_command` dispatcher:

```rust
/// Execute a compound command, applying any redirects around it.
fn exec_compound_command(
    &mut self,
    compound: &CompoundCommand,
    redirects: &[Redirect],
) -> i32 {
    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(redirects, &mut self.env, true) {
        eprintln!("kish: {}", e);
        self.env.last_exit_status = 1;
        return 1;
    }

    let status = match &compound.kind {
        CompoundCommandKind::BraceGroup { body } => self.exec_brace_group(body),
        CompoundCommandKind::Subshell { body } => self.exec_subshell(body),
        CompoundCommandKind::If {
            condition,
            then_part,
            elif_parts,
            else_part,
        } => self.exec_if(condition, then_part, elif_parts, else_part),
        CompoundCommandKind::While { condition, body } => {
            self.exec_loop(condition, body, false)
        }
        CompoundCommandKind::Until { condition, body } => {
            self.exec_loop(condition, body, true)
        }
        CompoundCommandKind::For { var, words, body } => {
            self.exec_for(var, words, body)
        }
        CompoundCommandKind::Case { word, items } => self.exec_case(word, items),
    };

    redirect_state.restore();
    self.env.last_exit_status = status;
    status
}
```

Add stub methods (will be implemented in subsequent tasks):

```rust
fn exec_brace_group(&mut self, _body: &[CompleteCommand]) -> i32 { todo!("Task 2") }
fn exec_subshell(&mut self, _body: &[CompleteCommand]) -> i32 { todo!("Task 9") }
fn exec_if(&mut self, _cond: &[CompleteCommand], _then: &[CompleteCommand],
           _elifs: &[(Vec<CompleteCommand>, Vec<CompleteCommand>)],
           _else_: &Option<Vec<CompleteCommand>>) -> i32 { todo!("Task 3") }
fn exec_loop(&mut self, _cond: &[CompleteCommand], _body: &[CompleteCommand],
             _until: bool) -> i32 { todo!("Task 4") }
fn exec_for(&mut self, _var: &str, _words: &Option<Vec<Word>>,
            _body: &[CompleteCommand]) -> i32 { todo!("Task 5") }
fn exec_case(&mut self, _word: &Word, _items: &[CaseItem]) -> i32 { todo!("Task 7") }
```

- [ ] **Step 3: Wire into exec_command and add flow control checks**

Replace the match arms in `exec_command`:

```rust
pub fn exec_command(&mut self, cmd: &Command) -> i32 {
    match cmd {
        Command::Simple(simple) => self.exec_simple_command(simple),
        Command::Compound(compound, redirects) => {
            self.exec_compound_command(compound, redirects)
        }
        Command::FunctionDef(func_def) => {
            self.env
                .functions
                .insert(func_def.name.clone(), func_def.clone());
            0
        }
    }
}
```

Add flow control checks to `exec_and_or` — after `exec_pipeline(&and_or.first)` and inside the loop after each pipeline:

```rust
pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
    let mut status = self.exec_pipeline(&and_or.first);
    if self.env.flow_control.is_some() {
        return status;
    }

    for (op, pipeline) in &and_or.rest {
        match op {
            AndOrOp::And => {
                if status == 0 {
                    status = self.exec_pipeline(pipeline);
                }
            }
            AndOrOp::Or => {
                if status != 0 {
                    status = self.exec_pipeline(pipeline);
                }
            }
        }
        if self.env.flow_control.is_some() {
            break;
        }
    }

    self.env.last_exit_status = status;
    status
}
```

Add flow control check to `exec_complete_command` — after each and_or execution:

```rust
// Inside the for loop, after the else branch that calls exec_and_or:
        } else {
            status = self.exec_and_or(and_or);
        }
        if self.env.flow_control.is_some() {
            break;
        }
```

- [ ] **Step 4: Verify build compiles**

Run: `cargo build 2>&1`
Expected: Compiles successfully (stubs use `todo!()` which compiles but panics at runtime)

- [ ] **Step 5: Commit**

```bash
git add src/env/mod.rs src/exec/mod.rs
git commit -m "feat(phase5): add FlowControl, function store, compound command dispatch

Task: Phase 5 infrastructure — FlowControl enum, function HashMap in ShellEnv,
exec_compound_command dispatcher with stubs, flow control checks in exec_and_or
and exec_complete_command"
```

---

### Task 2: Brace group execution

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration test**

In `tests/parser_integration.rs`:

```rust
// ── Phase 5: Control structure execution tests ──────────────────────────────

#[test]
fn test_exec_brace_group() {
    let out = kish_exec("{ echo hello; echo world; }");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nworld\n");
}

#[test]
fn test_exec_brace_group_exit_status() {
    assert!(kish_exec("{ true; }").status.success());
    assert!(!kish_exec("{ false; }").status.success());
}

#[test]
fn test_exec_brace_group_shares_env() {
    let out = kish_exec("x=hello; { x=world; }; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_brace_group -- --nocapture 2>&1 | tail -5`
Expected: FAIL (todo!() panics)

- [ ] **Step 3: Implement exec_brace_group**

Replace the stub in `src/exec/mod.rs`:

```rust
fn exec_brace_group(&mut self, body: &[CompleteCommand]) -> i32 {
    self.exec_body(body)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_exec_brace_group 2>&1`
Expected: All 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement brace group execution

Task: Phase 5 — brace group runs command list in current environment"
```

---

### Task 3: if/elif/else execution

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_if_true() {
    let out = kish_exec("if true; then echo yes; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_if_false() {
    let out = kish_exec("if false; then echo yes; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_if_else() {
    let out = kish_exec("if false; then echo no; else echo yes; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_if_elif() {
    let out = kish_exec("if false; then echo 1; elif true; then echo 2; elif true; then echo 3; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn test_exec_if_elif_else() {
    let out = kish_exec("if false; then echo 1; elif false; then echo 2; else echo 3; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn test_exec_if_exit_status() {
    // No branch taken and no else → exit status 0
    assert!(kish_exec("if false; then echo no; fi").status.success());
}

#[test]
fn test_exec_nested_if() {
    let out = kish_exec("if true; then if false; then echo no; else echo yes; fi; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_if -- --nocapture 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Implement exec_if**

Replace the stub in `src/exec/mod.rs`:

```rust
fn exec_if(
    &mut self,
    condition: &[CompleteCommand],
    then_part: &[CompleteCommand],
    elif_parts: &[(Vec<CompleteCommand>, Vec<CompleteCommand>)],
    else_part: &Option<Vec<CompleteCommand>>,
) -> i32 {
    let cond_status = self.exec_body(condition);
    if self.env.flow_control.is_some() {
        return cond_status;
    }

    if cond_status == 0 {
        return self.exec_body(then_part);
    }

    for (elif_cond, elif_body) in elif_parts {
        let cond_status = self.exec_body(elif_cond);
        if self.env.flow_control.is_some() {
            return cond_status;
        }
        if cond_status == 0 {
            return self.exec_body(elif_body);
        }
    }

    if let Some(else_body) = else_part {
        return self.exec_body(else_body);
    }

    0
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_exec_if 2>&1`
Expected: All 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement if/elif/else execution

Task: Phase 5 — if/elif/else with condition evaluation and branch selection"
```

---

### Task 4: while/until loop execution

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_while_loop() {
    let out = kish_exec("x=0; while test $x -lt 3; do echo $x; x=$((x + 1)); done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n2\n");
}

#[test]
fn test_exec_while_false_no_exec() {
    let out = kish_exec("while false; do echo never; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_until_loop() {
    let out = kish_exec("x=0; until test $x -ge 3; do echo $x; x=$((x + 1)); done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n2\n");
}

#[test]
fn test_exec_until_true_no_exec() {
    let out = kish_exec("until true; do echo never; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_while_exit_status() {
    // Exit status is from the last body command executed
    let out = kish_exec("x=0; while test $x -lt 1; do x=$((x+1)); false; done; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_while -- --nocapture 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Implement exec_loop (shared by while and until)**

Replace the stub in `src/exec/mod.rs`:

```rust
/// Execute a while or until loop.
/// `until=false` → while (run while condition succeeds)
/// `until=true`  → until (run while condition fails)
fn exec_loop(
    &mut self,
    condition: &[CompleteCommand],
    body: &[CompleteCommand],
    until: bool,
) -> i32 {
    let mut status = 0;
    loop {
        let cond_status = self.exec_body(condition);
        if self.env.flow_control.is_some() {
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

        match self.env.flow_control.take() {
            Some(FlowControl::Break(n)) => {
                if n > 1 {
                    self.env.flow_control = Some(FlowControl::Break(n - 1));
                }
                break;
            }
            Some(FlowControl::Continue(n)) => {
                if n > 1 {
                    self.env.flow_control = Some(FlowControl::Continue(n - 1));
                    break;
                }
                // n <= 1: continue this loop (re-evaluate condition)
            }
            Some(other) => {
                self.env.flow_control = Some(other);
                break;
            }
            None => {}
        }
    }
    status
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_exec_while test_exec_until 2>&1`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement while/until loop execution

Task: Phase 5 — shared exec_loop with flow control propagation for break/continue"
```

---

### Task 5: for loop execution

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_for_loop() {
    let out = kish_exec("for i in a b c; do echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\nc\n");
}

#[test]
fn test_exec_for_empty_list() {
    let out = kish_exec("for i in; do echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_for_with_expansion() {
    let out = kish_exec("items='x y z'; for i in $items; do echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\ny\nz\n");
}

#[test]
fn test_exec_for_default_positional_params() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh", "for i; do echo $i; done\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .args([script.to_str().unwrap(), "hello", "world"])
        .output()
        .expect("failed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\nworld\n");
}

#[test]
fn test_exec_nested_for() {
    let out = kish_exec("for i in 1 2; do for j in a b; do echo $i$j; done; done");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1a\n1b\n2a\n2b\n"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_for -- --nocapture 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Implement exec_for**

Replace the stub in `src/exec/mod.rs`:

```rust
fn exec_for(
    &mut self,
    var: &str,
    words: &Option<Vec<Word>>,
    body: &[CompleteCommand],
) -> i32 {
    let items: Vec<String> = match words {
        Some(word_list) => expand_words(&mut self.env, word_list),
        None => self.env.positional_params.clone(),
    };

    let mut status = 0;
    for item in &items {
        if let Err(e) = self.env.vars.set(var, item.as_str()) {
            eprintln!("kish: {}", e);
            return 1;
        }

        status = self.exec_body(body);

        match self.env.flow_control.take() {
            Some(FlowControl::Break(n)) => {
                if n > 1 {
                    self.env.flow_control = Some(FlowControl::Break(n - 1));
                }
                break;
            }
            Some(FlowControl::Continue(n)) => {
                if n > 1 {
                    self.env.flow_control = Some(FlowControl::Continue(n - 1));
                    break;
                }
                // n <= 1: continue this loop
            }
            Some(other) => {
                self.env.flow_control = Some(other);
                break;
            }
            None => {}
        }
    }
    status
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_exec_for 2>&1`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement for loop execution

Task: Phase 5 — for loop with word expansion and default positional params"
```

---

### Task 6: break/continue builtins

**Files:**
- Modify: `src/builtin/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_break() {
    let out = kish_exec("for i in 1 2 3; do if test $i = 2; then break; fi; echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

#[test]
fn test_exec_continue() {
    let out = kish_exec("for i in 1 2 3; do if test $i = 2; then continue; fi; echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n3\n");
}

#[test]
fn test_exec_break_nested() {
    // break 2 exits both loops
    let out = kish_exec(
        "for i in 1 2; do for j in a b c; do if test $j = b; then break 2; fi; echo $i$j; done; done",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1a\n");
}

#[test]
fn test_exec_continue_nested() {
    // continue 2 skips to next iteration of outer loop
    let out = kish_exec(
        "for i in 1 2; do for j in a b; do if test $j = b; then continue 2; fi; echo $i$j; done; done",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1a\n2a\n");
}

#[test]
fn test_exec_break_while() {
    let out = kish_exec("x=0; while true; do x=$((x+1)); if test $x = 3; then break; fi; echo $x; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_break test_exec_continue -- --nocapture 2>&1 | tail -5`
Expected: FAIL (break/continue are not recognized as builtins)

- [ ] **Step 3: Add break and continue builtins**

In `src/builtin/mod.rs`, add to `is_builtin`:

```rust
pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "exit" | "cd" | "export" | "unset" | "readonly" | "true" | "false" | ":" | "echo"
            | "return" | "break" | "continue"
    )
}
```

Add to `exec_builtin` match:

```rust
"break" => builtin_break(args, env),
"continue" => builtin_continue(args, env),
```

Add the implementations:

```rust
fn builtin_break(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: break: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: break: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(crate::env::FlowControl::Break(n));
    0
}

fn builtin_continue(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: continue: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: continue: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(crate::env::FlowControl::Continue(n));
    0
}
```

- [ ] **Step 4: Modify return builtin to set flow control**

Change `builtin_return` signature from `&ShellEnv` to `&mut ShellEnv` and set flow control:

```rust
fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
    let code = if args.is_empty() {
        env.last_exit_status & 0xFF
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: return: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    env.flow_control = Some(crate::env::FlowControl::Return(code));
    code
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_exec_break test_exec_continue 2>&1`
Expected: All 5 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/builtin/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement break/continue builtins with nesting support

Task: Phase 5 — break/continue set FlowControl in ShellEnv, return also sets FlowControl"
```

---

### Task 7: case statement execution

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_case_basic() {
    let out = kish_exec("case foo in foo) echo yes;; bar) echo no;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_case_no_match() {
    let out = kish_exec("case baz in foo) echo no;; bar) echo no;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_case_glob_pattern() {
    let out = kish_exec("case hello in h*) echo matched;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "matched\n");
}

#[test]
fn test_exec_case_multiple_patterns() {
    let out = kish_exec("case bar in foo|bar|baz) echo matched;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "matched\n");
}

#[test]
fn test_exec_case_default() {
    let out = kish_exec("case xyz in foo) echo no;; *) echo default;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "default\n");
}

#[test]
fn test_exec_case_with_variable() {
    let out = kish_exec("x=hello; case $x in hello) echo yes;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_case_fallthrough() {
    let out = kish_exec("case a in a) echo first;& b) echo second;; c) echo third;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "first\nsecond\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_case -- --nocapture 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Implement exec_case**

Replace the stub in `src/exec/mod.rs`. Add import at the top of the file:

```rust
use crate::expand::expand_word_to_string;
```

```rust
fn exec_case(&mut self, word: &Word, items: &[CaseItem]) -> i32 {
    let case_word = expand_word_to_string(&mut self.env, word);
    let mut status = 0;
    let mut falling_through = false;

    for item in items {
        if !falling_through {
            let mut matched = false;
            for pattern in &item.patterns {
                let pat = expand_word_to_string(&mut self.env, pattern);
                if crate::expand::pattern::matches(&pat, &case_word) {
                    matched = true;
                    break;
                }
            }
            if !matched {
                continue;
            }
        }

        status = self.exec_body(&item.body);
        if self.env.flow_control.is_some() {
            break;
        }

        match item.terminator {
            CaseTerminator::Break => break,
            CaseTerminator::FallThrough => {
                falling_through = true;
            }
        }
    }

    status
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_exec_case 2>&1`
Expected: All 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement case statement execution

Task: Phase 5 — case with glob pattern matching, multiple patterns, and fallthrough"
```

---

### Task 8: Function definition and invocation

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_function_basic() {
    let out = kish_exec("greet() { echo hello; }; greet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_exec_function_args() {
    let out = kish_exec("greet() { echo \"hello $1\"; }; greet world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_exec_function_dollar_at() {
    let out = kish_exec("show() { echo \"$@\"; }; show a b c");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a b c\n");
}

#[test]
fn test_exec_function_return() {
    let out = kish_exec("myfn() { return 42; echo never; }; myfn; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn test_exec_function_return_default() {
    let out = kish_exec("myfn() { true; }; myfn; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

#[test]
fn test_exec_function_recursion() {
    // Countdown: prints 3, 2, 1
    let out = kish_exec(
        "countdown() { if test $1 -gt 0; then echo $1; countdown $(($1 - 1)); fi; }; countdown 3",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n2\n1\n");
}

#[test]
fn test_exec_function_global_vars() {
    // POSIX: function variables are global (no local keyword)
    let out = kish_exec("x=before; setx() { x=after; }; setx; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "after\n");
}

#[test]
fn test_exec_function_restores_positional_params() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file(
        "test.sh",
        "show() { echo \"func: $1\"; }; show inner; echo \"script: $1\"\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .args([script.to_str().unwrap(), "outer"])
        .output()
        .expect("failed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "func: inner\nscript: outer\n"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_function -- --nocapture 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Add exec_function_call method**

In `src/exec/mod.rs`, add:

```rust
/// Invoke a function: save/restore positional params, execute body.
fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
    let saved_params =
        std::mem::replace(&mut self.env.positional_params, args.to_vec());

    let status =
        self.exec_compound_command(&func_def.body, &func_def.redirects);

    // Handle return flow control
    let final_status = match self.env.flow_control.take() {
        Some(FlowControl::Return(s)) => s,
        Some(other) => {
            self.env.flow_control = Some(other);
            status
        }
        None => status,
    };

    self.env.positional_params = saved_params;
    self.env.last_exit_status = final_status;
    final_status
}
```

- [ ] **Step 4: Wire function lookup into exec_simple_command**

In `exec_simple_command`, after expanding words and before the builtin check, add function lookup. The new code goes right after `let args: Vec<String> = expanded[1..].to_vec();`:

```rust
        // Check for function call (before builtins, matching POSIX lookup order
        // for non-special builtins)
        if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.env.last_exit_status = status;
            return status;
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_exec_function 2>&1`
Expected: All 8 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement function definition and invocation

Task: Phase 5 — functions stored in ShellEnv, positional params saved/restored,
return sets FlowControl::Return, recursion supported"
```

---

### Task 9: Subshell execution

**Files:**
- Modify: `src/exec/mod.rs`
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write failing integration tests**

In `tests/parser_integration.rs`:

```rust
#[test]
fn test_exec_subshell_basic() {
    let out = kish_exec("(echo hello)");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_exec_subshell_isolation() {
    // Variable changes in subshell should not affect parent
    let out = kish_exec("x=before; (x=after; echo $x); echo $x");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "after\nbefore\n"
    );
}

#[test]
fn test_exec_subshell_exit_status() {
    assert!(kish_exec("(true)").status.success());
    assert!(!kish_exec("(false)").status.success());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_exec_subshell -- --nocapture 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Implement exec_subshell**

Replace the stub in `src/exec/mod.rs`:

```rust
fn exec_subshell(&mut self, body: &[CompleteCommand]) -> i32 {
    match unsafe { fork() } {
        Err(e) => {
            eprintln!("kish: fork: {}", e);
            1
        }
        Ok(ForkResult::Child) => {
            let status = self.exec_body(body);
            std::process::exit(status);
        }
        Ok(ForkResult::Parent { child }) => command::wait_child(child),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_exec_subshell 2>&1`
Expected: All 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs tests/parser_integration.rs
git commit -m "feat(phase5): implement subshell execution via fork

Task: Phase 5 — subshell forks child process, parent waits for exit status"
```

---

### Task 10: Compound command redirects and comprehensive integration tests

**Files:**
- Test: `tests/parser_integration.rs`

- [ ] **Step 1: Write comprehensive integration tests**

In `tests/parser_integration.rs`:

```rust
// ── compound command redirects ──────────────────────────────────────────────

#[test]
fn test_exec_brace_group_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!("{{ echo hello; echo world; }} > {}", outfile.display()));
    assert_eq!(
        std::fs::read_to_string(&outfile).unwrap(),
        "hello\nworld\n"
    );
}

#[test]
fn test_exec_if_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!(
        "if true; then echo yes; fi > {}",
        outfile.display()
    ));
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "yes\n");
}

#[test]
fn test_exec_for_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!(
        "for i in a b; do echo $i; done > {}",
        outfile.display()
    ));
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "a\nb\n");
}

// ── complex / combined tests ────────────────────────────────────────────────

#[test]
fn test_exec_if_with_pipeline_condition() {
    let out = kish_exec("if echo hello | grep -q hello; then echo found; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "found\n");
}

#[test]
fn test_exec_for_in_function() {
    let out = kish_exec("each() { for i in \"$@\"; do echo $i; done; }; each x y z");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\ny\nz\n");
}

#[test]
fn test_exec_case_in_loop() {
    let out = kish_exec(
        "for f in a.txt b.rs c.txt; do case $f in *.txt) echo $f;; esac; done",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a.txt\nc.txt\n");
}

#[test]
fn test_exec_nested_control_structures() {
    let out = kish_exec(
        "if true; then for i in 1 2 3; do case $i in 2) echo two;; *) echo other;; esac; done; fi",
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "other\ntwo\nother\n"
    );
}

#[test]
fn test_exec_function_with_control() {
    let out = kish_exec(
        "first_match() { for i in \"$@\"; do if test $i = target; then echo found; return 0; fi; done; return 1; }; first_match a b target c; echo $?",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "found\n0\n");
}

#[test]
fn test_exec_while_with_read_like_pattern() {
    // Simulate counting with arithmetic
    let out = kish_exec(
        "sum=0; for i in 1 2 3 4 5; do sum=$((sum + i)); done; echo $sum",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n");
}

#[test]
fn test_exec_script_with_functions() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file(
        "test.sh",
        "greet() {\n  echo \"Hello, $1!\"\n}\nfor name in Alice Bob; do\n  greet $name\ndone\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output()
        .expect("failed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello, Alice!\nHello, Bob!\n"
    );
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests PASS (existing + new)

- [ ] **Step 3: Commit**

```bash
git add tests/parser_integration.rs
git commit -m "test(phase5): add comprehensive integration tests for control structures

Task: Phase 5 — tests for compound redirects, nested structures, functions with
control flow, and script-level scenarios"
```

---

### Task 11: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Mark Phase 5 complete and add known limitations**

Update `TODO.md`:
- Mark Phase 5 checkbox as done
- Add Phase 5 Known Limitations section with any issues found during implementation

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "update TODO.md: mark Phase 5 complete

Task: Phase 5 control structure execution complete"
```
