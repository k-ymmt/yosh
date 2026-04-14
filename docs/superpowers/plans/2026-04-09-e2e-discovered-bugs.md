# E2E Discovered Bugs Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all 10 bugs discovered via E2E tests (TODO.md "Discovered via E2E Tests" section).

**Architecture:** Three categories of fixes — expand (7 items covering `$-`, backslash, IFS, glob test, arithmetic), builtin (1 item for `export -p`), and signal (2 items for trap/signal name lookup and subshell inheritance). Each fix is isolated; signal fixes share dependencies (Task 8 before Task 9).

**Tech Stack:** Rust, POSIX shell semantics, bash E2E test runner

---

## File Map

| File | Changes |
|------|---------|
| `src/env/mod.rs` | Add `cmd_string` to `ShellOptions`, fix `to_flag_string()`, replace `TrapStore::signal_name_to_number()` |
| `src/main.rs` | Set `cmd_string` flag on `-c` invocation |
| `src/lexer/mod.rs` | Add `self.advance()` in backslash handler |
| `src/expand/field_split.rs` | Mark empty IFS fields as `was_quoted` |
| `src/expand/arith.rs` | Add comma operator, change `evaluate()` return type |
| `src/expand/mod.rs` | Handle `evaluate()` error for exit status |
| `src/builtin/special.rs` | Handle `-p` flag in export, quote values |
| `src/signal.rs` | Modify `reset_child_signals()` to preserve ignored signals |
| `src/exec/mod.rs` | Pass ignored signals to `reset_child_signals()` |
| `e2e/field_splitting/glob_dot_files.sh` | Fix test to use subdirectory, remove XFAIL |

---

### Task 1: `$-` Special Parameter

**Files:**
- Modify: `src/env/mod.rs:168-199`
- Modify: `src/main.rs:38`

- [ ] **Step 1: Run failing E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=special_var_hyphen --verbose
```

Expected: XFAIL (exit code 1, `$-` is empty so `test -n` fails).

- [ ] **Step 2: Add `cmd_string` field to `ShellOptions`**

In `src/env/mod.rs`, add `cmd_string: bool` to `ShellOptions` struct (after line 180):

```rust
pub struct ShellOptions {
    pub allexport: bool,  // -a
    pub notify: bool,     // -b
    pub noclobber: bool,  // -C
    pub errexit: bool,    // -e
    pub noglob: bool,     // -f
    pub noexec: bool,     // -n
    pub monitor: bool,    // -m
    pub nounset: bool,    // -u
    pub verbose: bool,    // -v
    pub xtrace: bool,     // -x
    pub ignoreeof: bool,
    pub pipefail: bool,
    pub cmd_string: bool, // -c (invocation mode, not settable via `set`)
}
```

Update `to_flag_string()` to include `c`:

```rust
pub fn to_flag_string(&self) -> String {
    let mut s = String::new();
    if self.allexport  { s.push('a'); }
    if self.notify     { s.push('b'); }
    if self.cmd_string { s.push('c'); }
    if self.noclobber  { s.push('C'); }
    if self.errexit    { s.push('e'); }
    if self.noglob     { s.push('f'); }
    if self.monitor    { s.push('m'); }
    if self.noexec     { s.push('n'); }
    if self.nounset    { s.push('u'); }
    if self.verbose    { s.push('v'); }
    if self.xtrace     { s.push('x'); }
    s
}
```

- [ ] **Step 3: Set `cmd_string` in `main.rs` for `-c` invocation**

In `src/main.rs`, after creating the executor in `run_string`, set the flag. Modify `run_string()`:

```rust
fn run_string(input: &str, shell_name: String, positional: Vec<String>, cmd_string: bool) -> i32 {
    signal::init_signal_handling();
    let mut executor = Executor::new(shell_name, positional);
    executor.env.options.cmd_string = cmd_string;
    executor.verbose_print(input);
    // ... rest unchanged
```

Update the two call sites in `main()`:
- Line 38: `let status = run_string(&args[2], sn, positional, true);` (for `-c`)
- Line 125 in `run_file`: `run_string(&content, shell_name, positional, false);`

- [ ] **Step 4: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'echo "$-"'
```

Expected output: `c`

- [ ] **Step 5: Run E2E test to verify XFAIL becomes XPASS**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=special_var_hyphen --verbose
```

Expected: XPASS (test now passes, but XFAIL marker still present).

- [ ] **Step 6: Remove XFAIL marker from E2E test**

In `e2e/variable_and_expansion/special_var_hyphen.sh`, remove line 4:
```
# XFAIL: kish does not implement $- special parameter
```

- [ ] **Step 7: Run E2E test to confirm PASS**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=special_var_hyphen --verbose
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/env/mod.rs src/main.rs e2e/variable_and_expansion/special_var_hyphen.sh
git commit -m "fix: implement \$- special parameter by tracking -c invocation flag

Add cmd_string field to ShellOptions and set it when invoked with -c.
to_flag_string() now includes 'c' in output, making test -n \"\$-\" pass.

Resolves: \$- special parameter not implemented (TODO.md E2E bug #1)"
```

---

### Task 2: Double-Quote Backslash Duplication

**Files:**
- Modify: `src/lexer/mod.rs:704-707`

- [ ] **Step 1: Run failing E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=backslash_non_special --verbose
```

Expected: XFAIL (output `\aa` instead of `\a`).

- [ ] **Step 2: Add `self.advance()` in the `_` arm**

In `src/lexer/mod.rs`, line 704-707, add `self.advance()` to consume the non-special character:

```rust
            _ => {
                // backslash is kept literally
                self.advance();
                Ok(WordPart::Literal(format!("\\{}", ch as char)))
            }
```

- [ ] **Step 3: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'echo "\a"'
./target/release/kish -c 'echo "\n"'
```

Expected output: `\a` and `\n` (each on its own line).

- [ ] **Step 4: Run full test suite to check for regressions**

```bash
cargo test 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 5: Remove XFAIL marker and verify E2E**

In `e2e/quoting/backslash_non_special_in_dquotes.sh`, remove line 4:
```
# XFAIL: kish double-quote backslash handling duplicates the following character
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=backslash_non_special --verbose
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/lexer/mod.rs e2e/quoting/backslash_non_special_in_dquotes.sh
git commit -m "fix(lexer): consume non-special char after backslash in double quotes

read_backslash_in_double_quote() was not advancing past the non-special
character, causing it to be processed twice (e.g. \"\a\" → \aa).

Resolves: double-quote backslash duplication (TODO.md E2E bug #2)"
```

---

### Task 3: IFS Consecutive Non-Whitespace Delimiters

**Files:**
- Modify: `src/expand/field_split.rs:104-105`

- [ ] **Step 1: Run failing E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=ifs_non_whitespace --verbose
```

Expected: XFAIL (2 fields instead of 3).

- [ ] **Step 2: Understand the root cause**

The field_split state machine correctly produces empty fields (unit test `test_double_colon_empty_field` passes). But `expand_word()` in `src/expand/mod.rs:80` filters out empty unquoted fields:

```rust
.filter(|f| !f.is_empty() || f.was_quoted)
```

Empty IFS fields have `was_quoted: false`, so they get dropped.

- [ ] **Step 3: Mark empty IFS fields as preserved**

In `src/expand/field_split.rs`, line 104-105, change the empty field push to set `was_quoted: true`:

```rust
                else if is_nws {
                    // An IFS non-whitespace delimiter immediately after
                    // Start/AfterNws → emit an empty field.
                    out.push(ExpandedField { was_quoted: true, ..ExpandedField::new() });
                    state = State::AfterNws;
                    i += 1;
                }
```

This preserves empty fields through the downstream `filter(|f| !f.is_empty() || f.was_quoted)` in `expand_word()`.

- [ ] **Step 4: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'IFS=: ; x="a::b"; for w in $x; do echo "[$w]"; done'
```

Expected output:
```
[a]
[]
[b]
```

- [ ] **Step 5: Run unit tests**

```bash
cargo test field_split -- --nocapture 2>&1 | tail -10
```

Expected: all field_split tests pass.

- [ ] **Step 6: Remove XFAIL marker and verify E2E**

In `e2e/field_splitting/ifs_non_whitespace_consecutive.sh`, remove line 4:
```
# XFAIL: kish field splitting does not produce empty fields from consecutive delimiters
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=ifs_non_whitespace --verbose
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/expand/field_split.rs e2e/field_splitting/ifs_non_whitespace_consecutive.sh
git commit -m "fix(expand): preserve empty fields from consecutive IFS delimiters

Empty fields produced by consecutive non-whitespace IFS delimiters were
being dropped by the empty-field filter in expand_word(). Mark them with
was_quoted=true so they survive downstream filtering.

Resolves: consecutive IFS delimiters empty field (TODO.md E2E bug #3)"
```

---

### Task 4: Glob Dot Files Test Fix

**Files:**
- Modify: `e2e/field_splitting/glob_dot_files.sh`

- [ ] **Step 1: Confirm kish glob is already correct**

```bash
mkdir -p /tmp/kish_gtest && echo x > /tmp/kish_gtest/visible.txt && echo x > /tmp/kish_gtest/.hidden.txt && ./target/release/kish -c 'cd /tmp/kish_gtest; count=0; for f in *; do count=$((count + 1)); done; echo $count' && rm -rf /tmp/kish_gtest
```

Expected: `1` (only `visible.txt` matched).

- [ ] **Step 2: Fix the E2E test to use a subdirectory**

The test currently `cd`s into `$TEST_TMPDIR` which contains the test runner's temp files (`_stdout`, `_stderr`, `_exit`). Fix by using a subdirectory.

Replace the full content of `e2e/field_splitting/glob_dot_files.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13.3 Patterns Used for Filename Expansion
# DESCRIPTION: Glob * does not match dot files
# EXPECT_EXIT: 0
dir="$TEST_TMPDIR/globtest"
mkdir "$dir"
cd "$dir"
echo x > visible.txt
echo x > .hidden.txt
count=0
for f in *; do
  count=$((count + 1))
done
test "$count" = 1
```

- [ ] **Step 3: Run the fixed E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=glob_dot --verbose
```

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add e2e/field_splitting/glob_dot_files.sh
git commit -m "fix(e2e): glob_dot_files test uses subdirectory to avoid runner temp files

The test was cd'ing into TEST_TMPDIR which contains the runner's _stdout,
_stderr, _exit files, inflating the glob count. Use a subdirectory instead.
kish glob correctly excludes dot files — this was a test-only issue.

Resolves: glob dot files false XFAIL (TODO.md E2E bug #4)"
```

---

### Task 5: Division/Modulo by Zero Exit Code

**Files:**
- Modify: `src/expand/arith.rs:5-24`
- Modify: `src/expand/mod.rs:372-378`

- [ ] **Step 1: Run failing E2E tests**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=division_by_zero --verbose
bash e2e/run_tests.sh --shell=./target/release/kish --filter=modulo_by_zero --verbose
```

Expected: both XFAIL (exit code 0 instead of 1).

- [ ] **Step 2: Change `evaluate()` return type to `Result`**

In `src/expand/arith.rs`, change the `evaluate` function (lines 5-24):

```rust
pub fn evaluate(env: &mut ShellEnv, expr: &str) -> Result<String, String> {
    // Step 1: expand $VAR and ${VAR} references
    let expanded = expand_vars(env, expr);

    // Step 2: parse and evaluate
    let bytes = expanded.as_bytes();
    let mut parser = ArithParser {
        input: bytes,
        pos: 0,
        env,
    };

    match parser.expr() {
        Ok(val) => Ok(val.to_string()),
        Err(msg) => {
            eprintln!("kish: arithmetic: {}", msg);
            Err(msg)
        }
    }
}
```

- [ ] **Step 3: Remove duplicate `eprintln` in `multiplicative()`**

In `src/expand/arith.rs`, the `multiplicative()` function (lines 349-351, 357-359) already prints error messages before returning `Err`. Since `evaluate()` now also prints on `Err`, remove the `eprintln` calls from `multiplicative()` to avoid double printing:

Lines 349-351 (division by zero):
```rust
            if right == 0 {
                return Err("division by zero".to_string());
            }
```

Lines 357-359 (modulo by zero):
```rust
            if right == 0 {
                return Err("division by zero (modulo)".to_string());
            }
```

- [ ] **Step 4: Update the caller in `expand/mod.rs`**

In `src/expand/mod.rs`, lines 372-379, handle the `Result`:

```rust
        WordPart::ArithSub(expr) => {
            match arith::evaluate(env, expr) {
                Ok(result) => {
                    if in_double_quote {
                        fields.last_mut().unwrap().push_quoted(&result);
                    } else {
                        fields.last_mut().unwrap().push_unquoted(&result);
                    }
                }
                Err(_) => {
                    env.last_exit_status = 1;
                    let zero = "0";
                    if in_double_quote {
                        fields.last_mut().unwrap().push_quoted(zero);
                    } else {
                        fields.last_mut().unwrap().push_unquoted(zero);
                    }
                }
            }
        }
```

- [ ] **Step 5: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'echo $((1/0))'; echo "exit: $?"
./target/release/kish -c 'echo $((1%0))'; echo "exit: $?"
```

Expected: error message to stderr, `0` to stdout, exit code 1 for both.

- [ ] **Step 6: Run unit tests**

```bash
cargo test 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 7: Remove XFAIL markers and verify E2E**

In `e2e/arithmetic/division_by_zero.sh`, remove line 4:
```
# XFAIL: prints error to stderr but returns exit code 0 instead of 1
```

In `e2e/arithmetic/modulo_by_zero.sh`, remove line 4:
```
# XFAIL: prints error to stderr but returns exit code 0 instead of 1
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=division_by_zero --verbose
bash e2e/run_tests.sh --shell=./target/release/kish --filter=modulo_by_zero --verbose
```

Expected: both PASS

- [ ] **Step 8: Commit**

```bash
git add src/expand/arith.rs src/expand/mod.rs e2e/arithmetic/division_by_zero.sh e2e/arithmetic/modulo_by_zero.sh
git commit -m "fix(arith): set exit status 1 on division/modulo by zero

Change evaluate() to return Result<String, String>. On arithmetic error,
the caller sets env.last_exit_status = 1. Remove duplicate eprintln from
multiplicative() since evaluate() now handles error output.

Resolves: division/modulo by zero exit code (TODO.md E2E bugs #5, #6)"
```

---

### Task 6: Comma Operator in Arithmetic

**Files:**
- Modify: `src/expand/arith.rs:103-105`

- [ ] **Step 1: Run failing E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=comma_operator --verbose
```

Expected: XFAIL.

- [ ] **Step 2: Add `comma()` method**

In `src/expand/arith.rs`, change `expr()` to call `comma()` and add the `comma()` method:

```rust
    // ── Top-level expression ─────────────────────────────────────────────────

    fn expr(&mut self) -> Result<i64, String> {
        self.comma()
    }

    // ── Comma: a, b, c (lowest precedence) ──────────────────────────────────

    fn comma(&mut self) -> Result<i64, String> {
        let mut result = self.assignment()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b',' {
                self.pos += 1;
                result = self.assignment()?;
            } else {
                break;
            }
        }
        Ok(result)
    }
```

Note: The comma operator needs to call `assignment()` (not `ternary()`) because assignments like `a=1, b=2` must work. Check if an `assignment()` method exists. If not, `ternary()` handles assignments already — use `ternary()` instead:

```rust
    fn comma(&mut self) -> Result<i64, String> {
        let mut result = self.ternary()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b',' {
                self.pos += 1;
                result = self.ternary()?;
            } else {
                break;
            }
        }
        Ok(result)
    }
```

- [ ] **Step 3: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'echo $((1,2,3))'
./target/release/kish -c 'echo $((a=1, b=2, a+b))'
./target/release/kish -c 'echo "$a $b"'
```

Expected: `3`, `3`, `1 2`

- [ ] **Step 4: Run unit tests**

```bash
cargo test arith -- --nocapture 2>&1 | tail -10
```

Expected: all pass.

- [ ] **Step 5: Remove XFAIL marker and verify E2E**

In `e2e/arithmetic/comma_operator.sh`, remove line 4:
```
# XFAIL: comma operator in arithmetic not fully implemented
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=comma_operator --verbose
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/expand/arith.rs e2e/arithmetic/comma_operator.sh
git commit -m "feat(arith): implement comma operator in arithmetic expansion

Add comma() method with lowest precedence in the arithmetic parser.
Evaluates left-to-right, returns the value of the rightmost expression.
Supports expressions like \$((a=1, b=2, a+b)).

Resolves: comma operator not implemented (TODO.md E2E bug #7)"
```

---

### Task 7: `export -p` Output

**Files:**
- Modify: `src/builtin/special.rs:53-61`

- [ ] **Step 1: Run failing E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=export_format --verbose
```

Expected: XFAIL.

- [ ] **Step 2: Handle `-p` flag and quote values**

In `src/builtin/special.rs`, modify `builtin_export()` lines 53-62:

```rust
fn builtin_export(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() || args[0] == "-p" {
        // Print all exported variables in POSIX re-input format
        let mut exported: Vec<(String, String)> = env.vars.to_environ();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            println!("export {}=\"{}\"", name, value);
        }
        return 0;
    }

    let mut status = 0;
    // ... rest unchanged
```

- [ ] **Step 3: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'FOO=bar; export FOO; export -p' 2>&1 | grep FOO
```

Expected: `export FOO="bar"`

- [ ] **Step 4: Remove XFAIL marker and verify E2E**

In `e2e/builtin/export_format.sh`, remove line 4:
```
# XFAIL: Phase 3 limitation — export -p does not output variables
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=export_format --verbose
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/builtin/special.rs e2e/builtin/export_format.sh
git commit -m "fix(builtin): handle export -p flag and quote output values

export -p was treated as a variable name instead of a flag. Now handled
alongside the empty-args case. Output uses POSIX format: export NAME=\"VALUE\".

Resolves: export -p not working (TODO.md E2E bug #8)"
```

---

### Task 8: `trap '' SIGNAL` — Fix Signal Name Lookup

**Files:**
- Modify: `src/env/mod.rs:32-48`

- [ ] **Step 1: Run failing E2E test**

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=trap_ignore_empty --verbose
```

Expected: XFAIL (USR1 kills process).

- [ ] **Step 2: Understand the root cause**

The `TrapStore::signal_name_to_number()` in `src/env/mod.rs` has a hardcoded table missing USR1/USR2. When `trap '' USR1` is called, it returns `None` → "invalid signal name". The signal infrastructure (self-pipe, `HANDLED_SIGNALS`) already handles USR1 correctly — the only problem is the name lookup.

Verify: `./target/release/kish -c 'trap "" 10; kill -10 $$; echo survived'` → prints "survived" (works with numeric signal).

- [ ] **Step 3: Replace `TrapStore::signal_name_to_number()` with delegation**

In `src/env/mod.rs`, replace the `signal_name_to_number` method (lines 32-48):

```rust
    pub fn signal_name_to_number(name: &str) -> Option<i32> {
        // Try numeric parse first
        if let Ok(n) = name.parse::<i32>() {
            return Some(n);
        }
        // "EXIT" is trap-specific (signal 0)
        if name.eq_ignore_ascii_case("EXIT") {
            return Some(0);
        }
        // Delegate to the canonical signal table
        crate::signal::signal_name_to_number(name).ok()
    }
```

Also update `signal_number_to_name` (lines 51-62):

```rust
    fn signal_number_to_name(num: i32) -> &'static str {
        if num == 0 {
            return "EXIT";
        }
        crate::signal::signal_number_to_name(num).unwrap_or("UNKNOWN")
    }
```

- [ ] **Step 4: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'trap "" USR1; kill -USR1 $$; echo survived'
```

Expected: `survived`

- [ ] **Step 5: Run unit tests**

```bash
cargo test trap_store -- --nocapture 2>&1 | tail -15
```

Expected: all pass. Note: `test_trap_store_signal_name_to_number` tests HUP/INT/QUIT/TERM — these still work via delegation.

- [ ] **Step 6: Remove XFAIL marker and verify E2E**

In `e2e/signal_and_trap/trap_ignore_empty.sh`, remove line 5:
```
# XFAIL: USR1 signal kills process even with trap '' (shell limitation)
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=trap_ignore_empty --verbose
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/env/mod.rs e2e/signal_and_trap/trap_ignore_empty.sh
git commit -m "fix(trap): delegate signal name lookup to signal::SIGNAL_TABLE

TrapStore had a hardcoded signal name table missing USR1/USR2. Delegate to
signal::signal_name_to_number() which uses the canonical SIGNAL_TABLE.
This fixes 'trap \"\" USR1' which previously failed with 'invalid signal name'.

Resolves: trap '' SIGNAL not ignoring signals (TODO.md E2E bug #9)"
```

---

### Task 9: Subshell Signal Inheritance

**Files:**
- Modify: `src/signal.rs:180-193`
- Modify: `src/exec/mod.rs:199-201`

- [ ] **Step 1: Run failing E2E test**

After Task 8 is committed, run:

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=subshell_trap_inherit --verbose
```

Check if it already passes (since the test sends `kill -USR1 $$` to the parent, not the subshell). If it passes as XPASS, skip to Step 5.

- [ ] **Step 2: Modify `reset_child_signals()` to accept ignored signals**

In `src/signal.rs`, change the function signature to accept a set of signals to keep ignored (lines 180-193):

```rust
/// Reset signals after fork for child processes.
/// `ignored` signals retain SIG_IGN; all others reset to SIG_DFL.
pub fn reset_child_signals(ignored: &[i32]) {
    for &(num, _) in HANDLED_SIGNALS {
        if ignored.contains(&num) {
            ignore_signal(num);
        } else {
            default_signal(num);
        }
    }

    // Close self-pipe fds if they exist.
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}
```

- [ ] **Step 3: Add helper method to `TrapStore` for ignored signal numbers**

In `src/env/mod.rs`, add a method to `TrapStore`:

```rust
    /// Return signal numbers that have TrapAction::Ignore disposition.
    pub fn ignored_signals(&self) -> Vec<i32> {
        self.signal_traps
            .iter()
            .filter(|(_, action)| matches!(action, TrapAction::Ignore))
            .map(|(&num, _)| num)
            .collect()
    }
```

- [ ] **Step 4: Update all `reset_child_signals()` call sites**

In `src/exec/mod.rs`, `exec_subshell()` (line 201):
```rust
            Ok(ForkResult::Child) => {
                self.env.traps.reset_non_ignored();
                let ignored = self.env.traps.ignored_signals();
                signal::reset_child_signals(&ignored);
                let status = self.exec_body(body);
                std::process::exit(status);
            }
```

Search for all other calls to `reset_child_signals()` in `src/exec/mod.rs` and `src/expand/command_sub.rs` and update them similarly. For external commands (exec path), ignored signals should also be passed. Use `grep -rn 'reset_child_signals' src/` to find all call sites.

For each call site:
- If the context has access to `env.traps`, pass `env.traps.ignored_signals()`
- For external commands where traps are not relevant post-exec, pass `&[]` (empty — exec replaces the process anyway, and SIG_IGN is preserved across exec per POSIX)

Actually, POSIX says: signals set to SIG_IGN by the shell are inherited by exec'd processes. So for external commands, pass the ignored signals too:
```rust
signal::reset_child_signals(&self.env.traps.ignored_signals());
```

- [ ] **Step 5: Build and verify**

```bash
cargo build --release 2>&1 | tail -3
./target/release/kish -c 'trap "" USR1; ( kill -USR1 $$; echo survived )'
```

Expected: `survived`

- [ ] **Step 6: Run unit tests**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 7: Remove XFAIL marker and verify E2E**

In `e2e/subshell/subshell_trap_inherit_ignore.sh`, remove line 5:
```
# XFAIL: USR1 signal kills subshell process even with trap '' (shell limitation)
```

```bash
bash e2e/run_tests.sh --shell=./target/release/kish --filter=subshell_trap_inherit --verbose
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/signal.rs src/exec/mod.rs src/env/mod.rs src/expand/command_sub.rs e2e/subshell/subshell_trap_inherit_ignore.sh
git commit -m "fix(signal): preserve SIG_IGN across fork for ignored traps

reset_child_signals() now accepts a list of signals to keep as SIG_IGN.
Subshells inherit ignored signal disposition from parent as POSIX requires.

Resolves: subshell signal inheritance (TODO.md E2E bug #10)"
```

---

### Task 10: Final Verification and Cleanup

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Run full E2E test suite**

```bash
cargo build --release 2>&1 | tail -3
bash e2e/run_tests.sh --shell=./target/release/kish --verbose 2>&1 | tail -20
```

Verify: no new failures. The 10 previously-XFAIL tests should now be PASS. Remaining XFAILs should be from other phases' known limitations.

- [ ] **Step 2: Run full unit/integration test suite**

```bash
cargo test 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 3: Remove completed items from TODO.md**

Delete the entire "Discovered via E2E Tests" section from `TODO.md` (lines 52-63):

```
## Discovered via E2E Tests

- [ ] `$-` special parameter not implemented — `test -n "$-"` fails (`src/expand/param.rs`)
- [ ] Double-quote backslash handling duplicates following character — `"\a"` outputs `\aa` instead of `\a` (`src/expand/mod.rs`)
- [ ] Consecutive non-whitespace IFS delimiters don't produce empty fields — `IFS=: x="a::b"` gives 2 fields instead of 3 (`src/expand/field_split.rs`)
- [ ] Glob `*` matches dot files — POSIX says `*` should not match filenames starting with `.` (`src/expand/pathname.rs`)
- [ ] Division by zero returns exit code 0 instead of 1 (`src/expand/arith.rs`)
- [ ] Modulo by zero returns exit code 0 instead of 1 (`src/expand/arith.rs`)
- [ ] Comma operator not implemented in arithmetic expansion (`src/expand/arith.rs`)
- [ ] `export -p` does not output exported variables (`src/builtin/mod.rs`)
- [ ] `trap '' SIGNAL` does not properly ignore signals — USR1 still kills process (`src/signal.rs`)
- [ ] Subshell does not inherit ignored signal disposition — `trap ''` not propagated to child (`src/signal.rs`)
```

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed E2E discovered bugs from TODO.md

All 10 bugs from the 'Discovered via E2E Tests' section have been fixed:
- \$- special parameter, backslash duplication, IFS empty fields,
  glob dot files test, division/modulo by zero exit code, comma operator,
  export -p, trap signal name lookup, subshell signal inheritance"
```
