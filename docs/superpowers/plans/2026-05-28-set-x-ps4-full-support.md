# `set -x` PS4 Full Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `set -x` trace prefixes expand `PS4` (parameter/arithmetic/command substitution) and replicate the first character by function/dot-script nesting level, per POSIX.

**Architecture:** Extract the existing prompt double-quote-expansion logic into a shared `expand::expand_dquoted` helper; add an `ExecState.indirection_level` counter incremented on function-call and dot-script entry; rewrite `xtrace_prefix` to expand PS4 (preserving `$?`) and replicate its first character `level + 1` times.

**Tech Stack:** Rust, yosh's Lexer/Expander/Executor pipeline, Criterion-free unit tests + shell-based e2e tests.

**Design:** `docs/superpowers/specs/2026-05-28-set-x-ps4-full-support-design.md`

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src/expand/mod.rs` | Shared `expand_dquoted` helper | Create fn + tests |
| `src/interactive/prompt.rs` | PS1/PS2 prompt expansion | Delegate to `expand_dquoted`; drop local `parse_prompt_word` |
| `src/env/exec_state.rs` | Execution state | Add `indirection_level: usize` |
| `src/env/mod.rs` | `ShellEnv::new` constructor | Init `indirection_level: 0` |
| `src/exec/function.rs` | Function-call execution | inc/dec `indirection_level` |
| `src/exec/mod.rs` | `source_file` (dot/source) | inc/dec `indirection_level` |
| `src/exec/simple.rs` | `xtrace_prefix` + call site | Rewrite fn; update call site & tests |
| `e2e/posix_spec/8_env_vars/PS4_*.sh` | POSIX compliance tests | Add 3 e2e tests |
| `TODO.md` | Tracking | Remove 3 completed items |

---

## Task 1: `expand_dquoted` shared helper

**Files:**
- Modify: `src/expand/mod.rs` (add fn after `expand_word_to_string`, ~line 192; tests in the `mod tests` block ~line 196)
- Modify: `src/interactive/prompt.rs` (delegate `expand_prompt`, remove `parse_prompt_word` + unused imports)

- [ ] **Step 1: Write the failing test**

In `src/expand/mod.rs`, inside `mod tests` (after the existing `make_env` helper), add:

```rust
#[test]
fn expand_dquoted_expands_parameter() {
    let mut env = make_env();
    env.vars.set("x", "hi").unwrap();
    assert_eq!(expand_dquoted(&mut env, "v=$x"), "v=hi");
}

#[test]
fn expand_dquoted_unset_param_is_empty() {
    let mut env = make_env();
    assert_eq!(expand_dquoted(&mut env, "[$nope]"), "[]");
}

#[test]
fn expand_dquoted_plain_literal_unchanged() {
    let mut env = make_env();
    assert_eq!(expand_dquoted(&mut env, "+ "), "+ ");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p yosh --lib expand_dquoted 2>&1 | tail -20`
Expected: FAIL — compile error `cannot find function 'expand_dquoted' in this scope`.

- [ ] **Step 3: Implement `expand_dquoted`**

In `src/expand/mod.rs`, immediately after the `expand_word_to_string` function (after line 192), add:

```rust
/// Parse `raw` as the body of a double-quoted word and expand it
/// (parameter expansion, command substitution, arithmetic expansion;
/// no field splitting, no pathname expansion). On lexer/parser or
/// expansion error, fall back to returning `raw` unchanged.
///
/// Shared by PS1/PS2 prompt expansion and `set -x` PS4 expansion.
pub fn expand_dquoted(env: &mut ShellEnv, raw: &str) -> String {
    // Wrap in double quotes so the lexer yields a double-quoted Word.
    let input = format!("\"{}\"", raw);
    let mut lexer = crate::lexer::Lexer::new(&input);
    let word = match lexer.next_token() {
        Ok(tok) => match tok.token {
            crate::lexer::token::Token::Word(word) => word,
            _ => return raw.to_string(),
        },
        Err(_) => return raw.to_string(),
    };
    expand_word_to_string(env, &word).unwrap_or_else(|_| raw.to_string())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p yosh --lib expand_dquoted 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Refactor `prompt.rs` to delegate**

In `src/interactive/prompt.rs`, replace the imports block (lines 1-6) with:

```rust
use super::display_width::display_width;
use crate::env::ShellEnv;
```

Delete the entire `parse_prompt_word` function (the doc comment + fn, originally lines 24-47).

Replace the body of `expand_prompt` (originally lines 54-72) with:

```rust
pub fn expand_prompt(env: &mut ShellEnv, var_name: &str) -> String {
    // 1. Get the raw value, or use the default.
    let raw = match env.vars.get(var_name) {
        Some(v) => v.to_string(),
        None => return default_prompt(var_name).to_string(),
    };

    // 2. Empty string => empty prompt.
    if raw.is_empty() {
        return String::new();
    }

    // 3. Expand as a double-quoted string (param/command-sub/arith).
    //    Errors are non-fatal: expand_dquoted falls back to the raw value.
    crate::expand::expand_dquoted(env, &raw)
}
```

- [ ] **Step 6: Run prompt + expand tests to verify no regression**

Run: `cargo test -p yosh --lib prompt:: 2>&1 | tail -15 && cargo test -p yosh --lib expand:: 2>&1 | tail -15`
Expected: PASS, no unused-import warnings from `prompt.rs`.

- [ ] **Step 7: Commit**

```bash
git add src/expand/mod.rs src/interactive/prompt.rs
git commit -m "refactor(expand): extract shared expand_dquoted helper

Relocate prompt double-quote expansion into expand::expand_dquoted so
set -x PS4 (non-interactive) can reuse it. prompt.rs now delegates.

Design: docs/superpowers/specs/2026-05-28-set-x-ps4-full-support-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `indirection_level` counter

**Files:**
- Modify: `src/env/exec_state.rs` (add field to `ExecState`)
- Modify: `src/env/mod.rs:69-73` (init field)
- Modify: `src/exec/function.rs:10-16` (inc/dec)
- Modify: `src/exec/mod.rs:99-122` (`source_file` inc/dec)
- Test: `src/exec/mod.rs` (`mod tests`)

- [ ] **Step 1: Write the failing test**

In `src/exec/mod.rs`, inside the existing `#[cfg(test)] mod tests` block, add:

```rust
#[test]
fn indirection_level_balanced_after_function_call() {
    use crate::parser::Parser;
    let mut exec = Executor::new("yosh", vec![]);
    let prog = Parser::new("f() { :; }; f").parse_program().unwrap();
    exec.exec_program(&prog);
    assert_eq!(exec.env.exec.indirection_level, 0);
}

#[test]
fn indirection_level_balanced_after_dot_script() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    writeln!(tmp, ":").unwrap();
    let mut exec = Executor::new("yosh", vec![]);
    exec.source_file(tmp.path());
    assert_eq!(exec.env.exec.indirection_level, 0);
}
```

> Note: these are *balance* tests (catch a missing decrement). The mid-execution value (level > 0) is verified end-to-end by the Task 4 e2e nesting tests. `tempfile` is already a dev-dependency used by the sibling `source_file_*` tests.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p yosh --lib indirection_level 2>&1 | tail -20`
Expected: FAIL — compile error `no field 'indirection_level' on type ...ExecState`.

- [ ] **Step 3: Add the field**

In `src/env/exec_state.rs`, add to the `ExecState` struct (after the `loop_depth` field):

```rust
    /// Number of nested function-call and dot-script invocations currently
    /// on the stack. Used only to replicate the first character of PS4 in
    /// `set -x` trace output (POSIX "levels of indirection"). Subshells and
    /// command substitutions are NOT counted.
    pub indirection_level: usize,
```

In `src/env/mod.rs`, update the `ExecState { ... }` literal (lines 69-73) to add the field:

```rust
            exec: ExecState {
                last_exit_status: 0,
                flow_control: None,
                loop_depth: 0,
                indirection_level: 0,
            },
```

- [ ] **Step 4: Add inc/dec on function-call entry**

In `src/exec/function.rs`, edit `exec_function_call` (lines 10-16). Change:

```rust
        self.env.vars.push_scope(args.to_vec());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.exec_compound_command(&func_def.body, &func_def.redirects)
        }));

        self.env.vars.pop_scope();
```

to:

```rust
        self.env.vars.push_scope(args.to_vec());
        self.env.exec.indirection_level += 1;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.exec_compound_command(&func_def.body, &func_def.redirects)
        }));

        self.env.exec.indirection_level -= 1;
        self.env.vars.pop_scope();
```

- [ ] **Step 5: Add inc/dec in `source_file`**

In `src/exec/mod.rs`, edit `source_file` (lines 99-122). After `self.env.mode.in_dot_script = true;` add the increment, and add a decrement before **both** the early `return Some(code)` and the final `Some(status)`. The full edited function:

```rust
    pub fn source_file(&mut self, path: &std::path::Path) -> Option<i32> {
        let content = std::fs::read_to_string(path).ok()?;
        let prev_dot_script = self.env.mode.in_dot_script;
        self.env.mode.in_dot_script = true;
        self.env.exec.indirection_level += 1;
        let status = match crate::parser::Parser::new_with_aliases(&content, &self.env.aliases)
            .parse_program()
        {
            Ok(program) => {
                let s = self.exec_program(&program);
                if let Some(crate::env::FlowControl::Return(code)) = self.env.exec.flow_control {
                    self.env.exec.flow_control = None;
                    self.env.mode.in_dot_script = prev_dot_script;
                    self.env.exec.indirection_level -= 1;
                    return Some(code);
                }
                s
            }
            Err(e) => {
                eprintln!("yosh: {}", e);
                2
            }
        };
        self.env.mode.in_dot_script = prev_dot_script;
        self.env.exec.indirection_level -= 1;
        Some(status)
    }
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p yosh --lib indirection_level 2>&1 | tail -20`
Expected: PASS (2 tests).

- [ ] **Step 7: Commit**

```bash
git add src/env/exec_state.rs src/env/mod.rs src/exec/function.rs src/exec/mod.rs
git commit -m "feat(exec): track indirection_level for function/dot nesting

Add ExecState.indirection_level, incremented on function-call and
dot-script entry (decremented on exit, including the early return-code
path). Used by set -x PS4 first-character replication. Subshells and
command substitutions are not counted.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Rewrite `xtrace_prefix` (expand + replicate)

**Files:**
- Modify: `src/exec/simple.rs:56-58` (`xtrace_prefix`)
- Modify: `src/exec/simple.rs:199-201` (call site)
- Test: `src/exec/simple.rs:859-871` (update 2 existing) + new tests in same `mod tests`

- [ ] **Step 1: Write the failing tests**

In `src/exec/simple.rs`, replace the two existing tests (lines 859-871) and add new ones:

```rust
    #[test]
    fn test_xtrace_prefix_uses_ps4_when_set() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "TRACE> ").unwrap();
        assert_eq!(xtrace_prefix(&mut env), "TRACE> ");
    }

    #[test]
    fn test_xtrace_prefix_default_when_ps4_unset() {
        let mut env = ShellEnv::new("yosh", vec![]);
        // PS4 is not set by ShellEnv::new, so the helper falls back to "+ ".
        assert_eq!(xtrace_prefix(&mut env), "+ ");
    }

    #[test]
    fn test_xtrace_prefix_expands_parameter() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("LINENO", "42").unwrap();
        env.vars.set("PS4", "L$LINENO> ").unwrap();
        assert_eq!(xtrace_prefix(&mut env), "L42> ");
    }

    #[test]
    fn test_xtrace_prefix_replicates_first_char_by_level() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "+ ").unwrap();
        env.exec.indirection_level = 0;
        assert_eq!(xtrace_prefix(&mut env), "+ ");
        env.exec.indirection_level = 1;
        assert_eq!(xtrace_prefix(&mut env), "++ ");
        env.exec.indirection_level = 2;
        assert_eq!(xtrace_prefix(&mut env), "+++ ");
    }

    #[test]
    fn test_xtrace_prefix_replicates_only_first_char_multichar() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "TRACE> ").unwrap();
        env.exec.indirection_level = 1;
        assert_eq!(xtrace_prefix(&mut env), "TTRACE> ");
    }

    #[test]
    fn test_xtrace_prefix_empty_ps4_stays_empty() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "").unwrap();
        env.exec.indirection_level = 3;
        assert_eq!(xtrace_prefix(&mut env), "");
    }

    #[test]
    fn test_xtrace_prefix_preserves_exit_status() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.exec.last_exit_status = 7;
        env.vars.set("PS4", "$(exit 3)> ").unwrap();
        let _ = xtrace_prefix(&mut env);
        assert_eq!(env.exec.last_exit_status, 7);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p yosh --lib test_xtrace_prefix 2>&1 | tail -25`
Expected: FAIL — `xtrace_prefix` still takes `&ShellEnv` / returns `&str`; the new `&mut`/expansion/replication tests do not compile or assert wrongly.

- [ ] **Step 3: Rewrite `xtrace_prefix`**

In `src/exec/simple.rs`, replace the function at lines 56-58:

```rust
fn xtrace_prefix(env: &ShellEnv) -> &str {
    env.vars.get("PS4").unwrap_or("+ ")
}
```

with:

```rust
/// Build the `set -x` trace prefix from PS4.
///
/// PS4 is expanded (parameter/command/arithmetic) like a double-quoted
/// string, then its first character is replicated `indirection_level + 1`
/// times to indicate function/dot-script nesting (POSIX XCU 2.5.3).
/// `$?` is preserved across any command substitution inside PS4.
fn xtrace_prefix(env: &mut ShellEnv) -> String {
    let raw = env.vars.get("PS4").unwrap_or("+ ").to_string();

    // Expand PS4 without letting a command substitution clobber $? for the
    // command being traced (bash preserves it).
    let saved_status = env.exec.last_exit_status;
    let expanded = crate::expand::expand_dquoted(env, &raw);
    env.exec.last_exit_status = saved_status;

    let level = env.exec.indirection_level;
    if level == 0 || expanded.is_empty() {
        return expanded;
    }

    // Replicate only the first character `level + 1` times total.
    let mut chars = expanded.chars();
    let first = chars.next().expect("non-empty checked above");
    let rest = chars.as_str();
    let mut out = String::with_capacity(expanded.len() + level * first.len_utf8());
    for _ in 0..=level {
        out.push(first);
    }
    out.push_str(rest);
    out
}
```

- [ ] **Step 4: Update the call site**

In `src/exec/simple.rs`, replace lines 199-201:

```rust
        if self.env.mode.options.xtrace && !expanded.is_empty() {
            eprintln!("{}{}", xtrace_prefix(&self.env), expanded.join(" "));
        }
```

with:

```rust
        if self.env.mode.options.xtrace && !expanded.is_empty() {
            let prefix = xtrace_prefix(&mut self.env);
            eprintln!("{}{}", prefix, expanded.join(" "));
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p yosh --lib test_xtrace_prefix 2>&1 | tail -25`
Expected: PASS (7 tests).

- [ ] **Step 6: Build the whole crate to catch fallout**

Run: `cargo build -p yosh 2>&1 | tail -15`
Expected: clean build (no warnings about the changed signature).

- [ ] **Step 7: Commit**

```bash
git add src/exec/simple.rs
git commit -m "feat(exec): expand PS4 and replicate first char in set -x trace

xtrace_prefix now expands PS4 (param/command/arith) via expand_dquoted
with \$? preserved, and replicates the first character indirection_level+1
times. Closes the PS4-expansion and first-char-repeat gaps.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: E2E POSIX compliance tests

**Files:**
- Create: `e2e/posix_spec/8_env_vars/PS4_expansion.sh`
- Create: `e2e/posix_spec/8_env_vars/PS4_nesting.sh`
- Create: `e2e/posix_spec/8_env_vars/PS4_dot_nesting.sh`

- [ ] **Step 1: Create `PS4_expansion.sh`**

Write `e2e/posix_spec/8_env_vars/PS4_expansion.sh` with exactly this content (line numbers matter — `echo hi` is line 9, so `$LINENO` → 9):

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 undergoes expansion ($LINENO) before trace display
# EXPECT_STDERR: + 9 echo hi
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
PS4='+ $LINENO '
set -x
echo hi
```

- [ ] **Step 2: Create `PS4_nesting.sh`**

Write `e2e/posix_spec/8_env_vars/PS4_nesting.sh`:

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 first character repeats by function-nesting level
# EXPECT_STDERR: ++ echo deep
# EXPECT_OUTPUT: deep
# EXPECT_EXIT: 0
PS4='+ '
f() { echo deep; }
set -x
f
```

- [ ] **Step 3: Create `PS4_dot_nesting.sh`**

Write `e2e/posix_spec/8_env_vars/PS4_dot_nesting.sh`:

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 first character repeats inside a dot-sourced script
# EXPECT_STDERR: ++ echo sourced
# EXPECT_OUTPUT: sourced
# EXPECT_EXIT: 0
PS4='+ '
d=$(mktemp)
echo 'echo sourced' > "$d"
set -x
. "$d"
rm -f "$d"
```

- [ ] **Step 4: Set 644 permissions on the new tests**

Run: `chmod 644 e2e/posix_spec/8_env_vars/PS4_expansion.sh e2e/posix_spec/8_env_vars/PS4_nesting.sh e2e/posix_spec/8_env_vars/PS4_dot_nesting.sh`
Expected: no output.

- [ ] **Step 5: Build, then run the new + existing PS4 e2e tests**

Run:
```bash
cargo build -p yosh 2>&1 | tail -3
./e2e/run_tests.sh --filter=PS4 2>&1 | tail -30
```
Expected: all `PS4_*` tests PASS (existing `PS4_assigned`, `PS4_default` + 3 new).

> Troubleshooting: if `PS4_expansion.sh` reports a different number than `9`, run `./target/debug/yosh e2e/posix_spec/8_env_vars/PS4_expansion.sh` directly, read the actual `+ N echo hi` line off stderr, and set `EXPECT_STDERR` to that value (yosh assigns `$LINENO` from the physical source line; the count above assumes it includes the shebang + 5 header lines).

- [ ] **Step 6: Commit**

```bash
git add e2e/posix_spec/8_env_vars/PS4_expansion.sh e2e/posix_spec/8_env_vars/PS4_nesting.sh e2e/posix_spec/8_env_vars/PS4_dot_nesting.sh
git commit -m "test(e2e): add PS4 expansion + nesting-replication tests

Cover parameter expansion of PS4 (\$LINENO) and first-character
replication inside function calls and dot-sourced scripts.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Remove completed TODO items

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Delete the SP5 PS4-expansion item**

In `TODO.md`, delete the bullet beginning `- [ ] PS4 variable / arithmetic / command-sub expansion not implemented.` through its closing `from SP5 T1.` (originally lines 141-148).

- [ ] **Step 2: Delete the SP5 PS4 first-char-repeat item**

In `TODO.md`, delete the bullet beginning `- [ ] PS4 first-character-repeat rule for nesting depth —` through its closing `Final-review follow-up from SP5 T1.` (originally lines 149-154).

- [ ] **Step 3: Delete the Interactive-Mode `set -x` PS4 item**

In `TODO.md`, under "Future: Interactive Mode Enhancements", delete the bullet beginning `- [ ] `set -x` PS4 prefix —` through its closing `(`src/exec/simple.rs`)` (originally line 397). This is the stale entry that references the now-passing `PS4_assigned.sh`.

- [ ] **Step 4: Verify no PS4 TODO references remain**

Run: `grep -n "PS4" TODO.md`
Expected: no output (no remaining PS4 items).

- [ ] **Step 5: Commit**

```bash
git add TODO.md
git commit -m "docs(todo): remove completed PS4 expansion/replication items

Closes the SP5 T1 PS4 follow-ups and the Interactive-Mode set -x PS4
prefix item; implemented in this branch.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Full verification

- [ ] **Step 1: Run the full library test suite**

Run: `cargo test -p yosh --lib 2>&1 | tail -15`
Expected: PASS (no failures).

- [ ] **Step 2: Run the full e2e suite**

Run: `./e2e/run_tests.sh 2>&1 | tail -20`
Expected: same pass count as before plus 3 new PS4 tests; no new failures.

- [ ] **Step 3: Manual smoke check**

PS4 MUST be **single-quoted** so `$LINENO` is stored literally and re-expanded per
trace. (Double-quoting `PS4="@$LINENO+ "` expands `$LINENO` once at assignment time —
line 1 — so every trace shows `@1+`; this matches bash and is correct, just not what
this check demonstrates.)

```bash
cat > /tmp/ps4smoke.sh <<'EOF'
PS4='@$LINENO+ '
set -x
echo a
h() { echo b; }
h
EOF
./target/debug/yosh /tmp/ps4smoke.sh
```
Lines: `echo a` = 3, `h` = 5, `echo b` (body of `h`) = 4.
Expected on **stderr** (PS4 first char `@`; top level 1×, inside `h` level 2×):
```
@3+ echo a
@5+ h
@@4+ echo b
```
Expected on **stdout**: `a` then `b`. Confirm the inner `echo b` trace line begins with
the first character doubled (`@@`) — this is yosh's function/dot nesting replication
(bash 3.2 shows a single `@4` here; the doubling is a deliberate design divergence).
