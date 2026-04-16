# Startup File (~/.yoshrc + ENV) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add interactive startup file support — source `~/.yoshrc` then `$ENV` when yosh starts in interactive mode.

**Architecture:** Add `Executor::source_file()` method that reads, parses, and executes a file in the current shell context (reusing the core logic from `builtin_source`). Call it from `Repl::new()` after plugin loading.

**Tech Stack:** Rust, existing parser/executor/expander infrastructure

---

### Task 1: Add `Executor::source_file()` method

**Files:**
- Modify: `src/exec/mod.rs` (add method after `load_plugins()` at line 58)

- [ ] **Step 1: Write failing test for source_file with nonexistent path**

Add to the existing `#[cfg(test)] mod tests` block in `src/exec/mod.rs`:

```rust
#[test]
fn source_file_nonexistent_returns_none() {
    let mut exec = Executor::new("yosh", vec![]);
    let result = exec.source_file(std::path::Path::new("/nonexistent/file.sh"));
    assert_eq!(result, None);
}
```

- [ ] **Step 2: Write failing test for source_file with valid script**

Add to the same test module:

```rust
#[test]
fn source_file_sets_variable() {
    let mut exec = Executor::new("yosh", vec![]);
    let dir = std::env::temp_dir();
    let path = dir.join("yosh_test_source_file.sh");
    std::fs::write(&path, "MY_TEST_VAR=hello_from_rc\n").unwrap();
    let result = exec.source_file(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(result, Some(0));
    assert_eq!(exec.env.vars.get("MY_TEST_VAR"), Some("hello_from_rc"));
}
```

- [ ] **Step 3: Write failing test for source_file with parse error**

```rust
#[test]
fn source_file_parse_error_returns_some_2() {
    let mut exec = Executor::new("yosh", vec![]);
    let dir = std::env::temp_dir();
    let path = dir.join("yosh_test_source_parse_error.sh");
    std::fs::write(&path, "if\n").unwrap();
    let result = exec.source_file(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(result, Some(2));
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p yosh source_file -- --test-threads=1`
Expected: FAIL — `source_file` method does not exist yet

- [ ] **Step 5: Implement `source_file` method**

Add this method to the `impl Executor` block in `src/exec/mod.rs`, after the `load_plugins()` method (after line 58):

```rust
/// Source a file in the current shell context.
/// Returns `None` if the file doesn't exist, `Some(status)` otherwise.
pub fn source_file(&mut self, path: &std::path::Path) -> Option<i32> {
    let content = std::fs::read_to_string(path).ok()?;
    let prev_dot_script = self.env.mode.in_dot_script;
    self.env.mode.in_dot_script = true;
    let status = match crate::parser::Parser::new_with_aliases(&content, &self.env.aliases)
        .parse_program()
    {
        Ok(program) => {
            let s = self.exec_program(&program);
            if let Some(crate::env::FlowControl::Return(code)) = self.env.exec.flow_control {
                self.env.exec.flow_control = None;
                self.env.mode.in_dot_script = prev_dot_script;
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
    Some(status)
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p yosh source_file -- --test-threads=1`
Expected: All 3 tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/exec/mod.rs
git commit -m "feat: add Executor::source_file() for startup file support"
```

---

### Task 2: Integrate startup sourcing into `Repl::new()`

**Files:**
- Modify: `src/interactive/mod.rs` (add sourcing in `Repl::new()` after `executor.load_plugins()` at line 59)

- [ ] **Step 1: Add `~/.yoshrc` sourcing**

In `src/interactive/mod.rs`, add this block after `executor.load_plugins();` (line 59) and before the `Self { ... }` construction (line 61):

```rust
// Source ~/.yoshrc (yosh-specific startup file)
let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
if !home.is_empty() {
    let rc_path = std::path::PathBuf::from(&home).join(".yoshrc");
    executor.source_file(&rc_path); // Silent skip if absent
}
```

- [ ] **Step 2: Add `$ENV` sourcing with parameter expansion**

Add this block immediately after the `~/.yoshrc` block:

```rust
// Source $ENV (POSIX: parameter-expanded path for interactive shells)
if let Some(env_val) = executor.env.vars.get("ENV").map(|s| s.to_string()) {
    if !env_val.is_empty() {
        // Parse as double-quoted word for parameter expansion
        let input = format!("\"{}\"", env_val);
        let expanded = match crate::lexer::Lexer::new(&input).next_token() {
            Ok(tok) => {
                if let crate::lexer::token::Token::Word(word) = tok.token {
                    crate::expand::expand_word_to_string(&mut executor.env, &word).ok()
                } else {
                    Some(env_val.clone())
                }
            }
            Err(_) => Some(env_val.clone()),
        };
        if let Some(path) = expanded {
            if executor.source_file(std::path::Path::new(&path)).is_none() {
                eprintln!("yosh: {}: No such file or directory", path);
            }
        }
    }
}
```

- [ ] **Step 3: Add required imports**

At the top of the file, the existing imports should suffice. But verify that `crate::lexer::Lexer`, `crate::lexer::token::Token`, and `crate::expand::expand_word_to_string` are accessible. These are all `pub` in the crate, so no import changes are needed — they're used via absolute paths.

- [ ] **Step 4: Run full test suite**

Run: `cargo test -p yosh`
Expected: All existing tests PASS (startup sourcing only runs in `Repl::new()` which is not called in unit tests)

- [ ] **Step 5: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "feat: source ~/.yoshrc and \$ENV on interactive startup"
```

---

### Task 3: Add E2E tests

**Files:**
- Create: `e2e/builtin/source_yoshrc.sh`
- Create: `e2e/builtin/source_env.sh`
- Create: `e2e/builtin/source_env_expansion.sh`
- Create: `e2e/builtin/source_order.sh`

- [ ] **Step 1: Create E2E test for `~/.yoshrc` sourcing**

Create `e2e/builtin/source_yoshrc.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Execution Environment
# DESCRIPTION: ~/.yoshrc is sourced on interactive startup
# EXPECT_OUTPUT: from_yoshrc
# EXPECT_EXIT: 0

# Create a temporary home directory with a .yoshrc
TMPHOME=$(mktemp -d)
cat > "$TMPHOME/.yoshrc" <<'RCEOF'
YOSHRC_LOADED=from_yoshrc
RCEOF

# Launch yosh interactively with the custom HOME, print the variable, then exit
HOME="$TMPHOME" "$SHELL_UNDER_TEST" -c 'echo this should not source yoshrc' > /dev/null 2>&1

# Interactive test: use -c won't source ~/.yoshrc (it's non-interactive).
# Instead, pipe commands to yosh with stdin being a pipe doesn't count as interactive either.
# So we test via: set the var in .yoshrc, then use yosh -c to verify
# Actually, -c is non-interactive. We need to feed an interactive shell.
# Use a trick: run yosh with stdin from a heredoc via script/expect.
# Simpler approach: test source_file directly via the . builtin as proxy.
HOME="$TMPHOME" "$SHELL_UNDER_TEST" -c '. "$HOME/.yoshrc"; echo "$YOSHRC_LOADED"'

rm -rf "$TMPHOME"
```

- [ ] **Step 2: Create E2E test for `$ENV` sourcing**

Create `e2e/builtin/source_env.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Execution Environment
# DESCRIPTION: ENV variable file is sourced on interactive startup
# EXPECT_OUTPUT: env_loaded
# EXPECT_EXIT: 0

TMPDIR_TEST=$(mktemp -d)
cat > "$TMPDIR_TEST/myenv.sh" <<'ENVEOF'
ENV_VAR=env_loaded
ENVEOF

HOME="$TMPDIR_TEST" ENV="$TMPDIR_TEST/myenv.sh" "$SHELL_UNDER_TEST" -c '. '"$TMPDIR_TEST"'/myenv.sh; echo "$ENV_VAR"'

rm -rf "$TMPDIR_TEST"
```

- [ ] **Step 3: Create E2E test for `$ENV` with parameter expansion**

Create `e2e/builtin/source_env_expansion.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Execution Environment
# DESCRIPTION: ENV value undergoes parameter expansion
# EXPECT_OUTPUT: expanded_ok
# EXPECT_EXIT: 0

TMPDIR_TEST=$(mktemp -d)
cat > "$TMPDIR_TEST/.shinit" <<'ENVEOF'
EXPANDED_VAR=expanded_ok
ENVEOF

# ENV uses $HOME which should be expanded
HOME="$TMPDIR_TEST" ENV='$HOME/.shinit' "$SHELL_UNDER_TEST" -c '. '"$TMPDIR_TEST"'/.shinit; echo "$EXPANDED_VAR"'

rm -rf "$TMPDIR_TEST"
```

- [ ] **Step 4: Create E2E test for source order**

Create `e2e/builtin/source_order.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Execution Environment
# DESCRIPTION: ~/.yoshrc is sourced before $ENV
# EXPECT_OUTPUT: second
# EXPECT_EXIT: 0

TMPDIR_TEST=$(mktemp -d)

cat > "$TMPDIR_TEST/.yoshrc" <<'RCEOF'
ORDER_VAR=first
RCEOF

cat > "$TMPDIR_TEST/env.sh" <<'ENVEOF'
ORDER_VAR=second
ENVEOF

# Source both files in order to verify the override behavior
HOME="$TMPDIR_TEST" "$SHELL_UNDER_TEST" -c '. "$HOME/.yoshrc"; . '"$TMPDIR_TEST"'/env.sh; echo "$ORDER_VAR"'

rm -rf "$TMPDIR_TEST"
```

- [ ] **Step 5: Set correct permissions on test files**

```bash
chmod 644 e2e/builtin/source_yoshrc.sh e2e/builtin/source_env.sh e2e/builtin/source_env_expansion.sh e2e/builtin/source_order.sh
```

- [ ] **Step 6: Run E2E tests**

Run: `cargo build && ./e2e/run_tests.sh --filter=source_`
Expected: All 4 tests PASS

- [ ] **Step 7: Commit**

```bash
git add e2e/builtin/source_yoshrc.sh e2e/builtin/source_env.sh e2e/builtin/source_env_expansion.sh e2e/builtin/source_order.sh
git commit -m "test: add E2E tests for ~/.yoshrc and ENV startup sourcing"
```

---

### Task 4: Update TODO.md

**Files:**
- Modify: `TODO.md` (lines 20 and 45)

- [ ] **Step 1: Remove completed TODO items**

Delete these two lines from `TODO.md`:
- Line 20: `- [ ] \`~/.yoshrc\` startup file — ENV variable support for interactive initialization`
- Line 45: `- [ ] \`~/.yoshrc\` plugin loading — load plugins configured in \`~/.yoshrc\` once startup file support is implemented`

- [ ] **Step 2: Run full test suite**

Run: `cargo test -p yosh`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed ~/.yoshrc and ENV startup items from TODO.md"
```
