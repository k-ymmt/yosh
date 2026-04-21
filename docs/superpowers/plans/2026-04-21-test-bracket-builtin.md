# `test` / `[` Builtin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote `test` and `[` from external commands to POSIX-compliant Regular builtins, eliminating ~1001 `fork`+`execvp` calls per W2 benchmark run (see `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`).

**Architecture:** New module `src/builtin/test.rs` exposing `builtin_test(name, args) -> i32`. Evaluation dispatches on operand count (0..=4) per POSIX §2.14. `classify_builtin` classifies `test` / `[` as `Regular`. Kept in a dedicated module to avoid bloating the existing `regular.rs` (~900 LOC).

**Tech Stack:** Rust (stable), `nix::unistd::{access, isatty}`, `std::fs::{metadata, symlink_metadata}`, `std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt}`, `tempfile` (test fixtures — already a dev-dep), Criterion (bench).

---

## File Structure

**New files:**
- `src/builtin/test.rs` — `builtin_test`, `evaluate`, `TestError`, operator helpers. ~350 LOC + ~200 LOC tests.
- `e2e/posix_spec/2_14_test/` — 15 E2E files per spec §5.3.

**Modified files:**
- `src/builtin/mod.rs`:
  - Add `"test"` and `"["` to `BUILTIN_NAMES`.
  - Add `"test" | "["` to `classify_builtin` Regular arm.
  - Add `pub mod test;` module declaration.
  - Add dispatch in `exec_regular_builtin`.
- `benches/exec_bench.rs` — add `exec_bracket_loop_200` bench.
- `performance.md` — corrections per spec §6.2.

---

## Task 1: Add `exec_bracket_loop_200` Criterion bench + capture baseline

**Files:**
- Modify: `benches/exec_bench.rs`

This task adds the bench and captures a **pre-implementation** Criterion baseline. Running the bench with the current `[`-as-external behavior produces the slow-path numbers that later tasks will compare against.

- [ ] **Step 1: Read the current `benches/exec_bench.rs`**

Run: `cat benches/exec_bench.rs`
Expected: see existing `run_script` helper, `LOOP_SCRIPT`, `FUNCTION_SCRIPT`, `EXPANSION_SCRIPT` constants, and `bench_exec` function.

- [ ] **Step 2: Add `BRACKET_LOOP_SCRIPT` constant and bench**

In `benches/exec_bench.rs`, add this constant after the existing `EXPANSION_SCRIPT` (around line 41):

```rust
const BRACKET_LOOP_SCRIPT: &str = r#"
i=0
while [ "$i" -lt 200 ]; do
    i=$((i + 1))
done
"#;
```

Then inside the existing `fn bench_exec(c: &mut Criterion) {` body, append a new bench call (after the existing `exec_param_expansion_200` call):

```rust
    c.bench_function("exec_bracket_loop_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(BRACKET_LOOP_SCRIPT));
            assert_eq!(status, 0);
        });
    });
```

- [ ] **Step 3: Verify the bench compiles and runs**

Run: `cargo bench --bench exec_bench -- exec_bracket_loop_200 --warm-up-time 1 --measurement-time 2` (timeout ≥ 300000ms)
Expected: the bench runs and reports a median (will be slow — hundreds of ms per iteration because `[` is external). Record the median for later comparison.

- [ ] **Step 4: Commit the bench**

```bash
git add benches/exec_bench.rs
git commit -m "$(cat <<'EOF'
perf(bench): add exec_bracket_loop_200 Criterion benchmark

Measures the cost of a while-loop condition that uses `[`.
Baseline captured before promoting `[` / `test` to builtins so the
improvement is directly observable.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Capture the full pre-implementation Criterion baseline**

Run: `cargo bench --bench exec_bench -- --save-baseline pre-bracket-builtin` (timeout ≥ 600000ms; Criterion warm-up for each bench takes ~10s, 4 benches means ~1-2 minutes total)
Expected: Criterion writes `target/criterion/*/base/*.json` under the `pre-bracket-builtin` name. No git commit — baselines are gitignored.

---

## Task 2: Create `test.rs` skeleton + wire dispatch + 0/1-operand + `[` bracket validation

**Files:**
- Create: `src/builtin/test.rs`
- Modify: `src/builtin/mod.rs`

- [ ] **Step 1: Create `src/builtin/test.rs` with module-level doc, type, and skeleton `builtin_test`**

Contents:

```rust
//! POSIX `test` and `[` builtin implementation (§2.14).
//!
//! Evaluation dispatches by operand count. Operators outside POSIX
//! (e.g. `<`, `>`, `-a`, `-o`, deep `(` `)` nesting) are deliberately
//! not supported — see the design doc for rationale.

/// Error returned by `evaluate`. Always produces exit status 2 plus a
/// message prefixed by `yosh: {name}: ` in the caller.
struct TestError {
    message: String,
}

impl TestError {
    fn syntax(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

/// Implements POSIX `test` and `[` builtins. Returns exit status directly;
/// `test` failures are normal exit statuses, not flow-control errors.
pub fn builtin_test(name: &str, args: &[String]) -> i32 {
    // `[` requires a closing `]` as the last argument.
    let operand_slice: &[String] = if name == "[" {
        match args.last() {
            Some(s) if s == "]" => &args[..args.len() - 1],
            _ => {
                eprintln!("yosh: [: missing ']'");
                return 2;
            }
        }
    } else {
        args
    };

    let operands: Vec<&str> = operand_slice.iter().map(|s| s.as_str()).collect();
    match evaluate(&operands) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("yosh: {}: {}", name, e.message);
            2
        }
    }
}

fn evaluate(args: &[&str]) -> Result<bool, TestError> {
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        _ => Err(TestError::syntax(format!(
            "unsupported operand count: {}",
            args.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(args: &[&str]) -> i32 {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        builtin_test("test", &owned)
    }

    fn b(args: &[&str]) -> i32 {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        builtin_test("[", &owned)
    }

    #[test]
    fn zero_operands_is_false() {
        assert_eq!(t(&[]), 1);
    }

    #[test]
    fn one_empty_operand_is_false() {
        assert_eq!(t(&[""]), 1);
    }

    #[test]
    fn one_nonempty_operand_is_true() {
        assert_eq!(t(&["x"]), 0);
        assert_eq!(t(&["false"]), 0); // string "false" is nonempty → true
    }

    #[test]
    fn bracket_requires_closing() {
        assert_eq!(b(&["-n", "x"]), 2); // missing `]`
    }

    #[test]
    fn bracket_with_closing_matches_test() {
        assert_eq!(b(&["x", "]"]), 0); // 1-operand nonempty → true
        assert_eq!(b(&["", "]"]), 1); // 1-operand empty → false
    }
}
```

- [ ] **Step 2: Declare the module in `src/builtin/mod.rs`**

At the top of `src/builtin/mod.rs`, change:

```rust
pub mod command;
pub mod regular;
pub mod resolve;
pub mod special;
```

to:

```rust
pub mod command;
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
```

- [ ] **Step 3: Update `BUILTIN_NAMES` in `src/builtin/mod.rs`**

Replace the existing `BUILTIN_NAMES` array with:

```rust
/// All builtin command names (special + regular) for tab-completion.
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export", "readonly", "return", "set",
    "shift", "times", "trap", "unset", "fc", // Regular builtins
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[",
];
```

- [ ] **Step 4: Update `classify_builtin` in `src/builtin/mod.rs`**

Replace the `classify_builtin` function with:

```rust
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export" | "readonly"
        | "return" | "set" | "shift" | "times" | "trap" | "unset" | "fc" => BuiltinKind::Special,
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "test" | "[" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}
```

- [ ] **Step 5: Add dispatch to `exec_regular_builtin` in `src/builtin/mod.rs`**

In the `match name` expression inside `exec_regular_builtin`, add an arm before the `_` fallback:

```rust
        "test" | "[" => Ok(test::builtin_test(name, args)),
```

It should sit after the existing `"command"` arm. The full `exec_regular_builtin` `match name` becomes (showing the relevant lines):

```rust
        "command" => {
            eprintln!("yosh: command: internal error");
            Ok(1)
        }
        "test" | "[" => Ok(test::builtin_test(name, args)),
        _ => {
            eprintln!("yosh: {}: not a regular builtin", name);
            Ok(1)
        }
```

- [ ] **Step 6: Run the new unit tests**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 5 tests pass — `zero_operands_is_false`, `one_empty_operand_is_false`, `one_nonempty_operand_is_true`, `bracket_requires_closing`, `bracket_with_closing_matches_test`.

- [ ] **Step 7: Run the full unit-test suite to check classify consistency**

Run: `cargo test -p yosh --lib` (timeout ≥ 300000ms)
Expected: all tests pass, including `test_builtin_names_consistent_with_classify` (which now verifies `test` and `[` are classified as non-`NotBuiltin`).

- [ ] **Step 8: Commit**

```bash
git add src/builtin/test.rs src/builtin/mod.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add test / [ builtin skeleton with 0/1-operand forms

Classify test and [ as Regular builtins. Dispatch through a new
src/builtin/test.rs module. Initial implementation covers the
zero-operand (false), one-operand (nonempty) forms and the
closing-bracket requirement for [.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: 2-operand forms — unary string ops (`-n`, `-z`) and negation (`!`)

**Files:**
- Modify: `src/builtin/test.rs`

- [ ] **Step 1: Write failing tests for 2-operand forms**

Add these tests inside the existing `mod tests` block in `src/builtin/test.rs`:

```rust
    #[test]
    fn negation_of_empty_is_true() {
        assert_eq!(t(&["!", ""]), 0);
    }

    #[test]
    fn negation_of_nonempty_is_false() {
        assert_eq!(t(&["!", "x"]), 1);
    }

    #[test]
    fn dash_n_nonempty_is_true() {
        assert_eq!(t(&["-n", "x"]), 0);
    }

    #[test]
    fn dash_n_empty_is_false() {
        assert_eq!(t(&["-n", ""]), 1);
    }

    #[test]
    fn dash_z_empty_is_true() {
        assert_eq!(t(&["-z", ""]), 0);
    }

    #[test]
    fn dash_z_nonempty_is_false() {
        assert_eq!(t(&["-z", "x"]), 1);
    }

    #[test]
    fn unknown_unary_operator_errors() {
        // An unknown unary operator produces exit 2.
        assert_eq!(t(&["-Z", "x"]), 2);
    }
```

- [ ] **Step 2: Run the new tests to confirm they fail**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 7 new tests fail with "unsupported operand count: 2" (or similar). The 5 pre-existing tests still pass.

- [ ] **Step 3: Extend `evaluate` to handle 2-operand forms, add `eval_unary`**

Replace the current `evaluate` function with:

```rust
fn evaluate(args: &[&str]) -> Result<bool, TestError> {
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            eval_unary(args[0], args[1])
        }
        _ => Err(TestError::syntax(format!(
            "unsupported operand count: {}",
            args.len()
        ))),
    }
}

fn eval_unary(op: &str, arg: &str) -> Result<bool, TestError> {
    match op {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),
        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 12 tests pass (5 old + 7 new).

- [ ] **Step 5: Commit**

```bash
git add src/builtin/test.rs
git commit -m "$(cat <<'EOF'
feat(builtin): support 2-operand forms in test / [ (-n, -z, !)

Adds unary string operators and negation. An unknown unary operator
produces exit 2 with a diagnostic on stderr.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Unary file tests via `metadata` (`-e`, `-f`, `-d`, `-h`/`-L`, `-p`, `-S`, `-s`, `-b`, `-c`)

**Files:**
- Modify: `src/builtin/test.rs`

- [ ] **Step 1: Write failing tests for filesystem predicates**

Add this module-level test helper and tests to `mod tests`:

```rust
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn dash_e_existing_file_is_true() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-e", &path]), 0);
    }

    #[test]
    fn dash_e_missing_file_is_false() {
        assert_eq!(t(&["-e", "/no/such/path/__yosh_test__"]), 1);
    }

    #[test]
    fn dash_f_regular_file_is_true() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "data").unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-f", &path]), 0);
    }

    #[test]
    fn dash_f_directory_is_false() {
        assert_eq!(t(&["-f", "/tmp"]), 1);
    }

    #[test]
    fn dash_d_directory_is_true() {
        assert_eq!(t(&["-d", "/tmp"]), 0);
    }

    #[test]
    fn dash_d_regular_file_is_false() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-d", &path]), 1);
    }

    #[test]
    fn dash_h_and_L_detect_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let link_str = link.to_str().unwrap().to_string();
        assert_eq!(t(&["-h", &link_str]), 0);
        assert_eq!(t(&["-L", &link_str]), 0);
        let target_str = target.to_str().unwrap().to_string();
        assert_eq!(t(&["-h", &target_str]), 1);
        assert_eq!(t(&["-L", &target_str]), 1);
    }

    #[test]
    fn dash_s_nonempty_file() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "data").unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-s", &path]), 0);
    }

    #[test]
    fn dash_s_empty_file_is_false() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-s", &path]), 1);
    }
```

- [ ] **Step 2: Run the new tests to confirm they fail**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 10 new tests fail with "unknown operator". Older tests still pass.

- [ ] **Step 3: Extend `eval_unary` with file tests**

Replace `eval_unary` with:

```rust
fn eval_unary(op: &str, arg: &str) -> Result<bool, TestError> {
    use std::os::unix::fs::FileTypeExt;

    match op {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),

        // -e follows symlinks (bash/dash semantics): dangling links → false.
        "-e" => Ok(std::fs::metadata(arg).is_ok()),
        "-f" => Ok(std::fs::metadata(arg).map(|m| m.is_file()).unwrap_or(false)),
        "-d" => Ok(std::fs::metadata(arg).map(|m| m.is_dir()).unwrap_or(false)),
        // -h / -L do NOT follow symlinks.
        "-h" | "-L" => Ok(std::fs::symlink_metadata(arg)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)),
        "-s" => Ok(std::fs::metadata(arg).map(|m| m.len() > 0).unwrap_or(false)),
        "-p" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_fifo())
            .unwrap_or(false)),
        "-S" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false)),
        "-b" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_block_device())
            .unwrap_or(false)),
        "-c" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_char_device())
            .unwrap_or(false)),

        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 22 tests pass (12 + 10 new).

- [ ] **Step 5: Commit**

```bash
git add src/builtin/test.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add metadata-based file predicates to test / [

Implements -e, -f, -d, -h/-L, -p, -S, -s, -b, -c using
std::fs::metadata and FileTypeExt. Follows bash semantics: -e uses
metadata (follows symlinks) so dangling links return false.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Unary file tests via `access` / `isatty` (`-r`, `-w`, `-x`, `-t`)

**Files:**
- Modify: `src/builtin/test.rs`

- [ ] **Step 1: Write failing tests for access/isatty predicates**

Add these tests to `mod tests`:

```rust
    #[test]
    fn dash_r_readable_file_is_true() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-r", &path]), 0);
    }

    #[test]
    fn dash_r_missing_file_is_false() {
        assert_eq!(t(&["-r", "/no/such/__yosh_test__"]), 1);
    }

    #[test]
    fn dash_w_writable_file_is_true() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-w", &path]), 0);
    }

    #[test]
    fn dash_x_executable_is_true_for_chmod_bit() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(t(&["-x", &path]), 0);
    }

    #[test]
    fn dash_x_nonexecutable_is_false() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(t(&["-x", &path]), 1);
    }

    #[test]
    fn dash_t_non_tty_fd_is_false() {
        // FD 99 is almost certainly not open, so isatty returns false.
        assert_eq!(t(&["-t", "99"]), 1);
    }

    #[test]
    fn dash_t_non_integer_errors() {
        assert_eq!(t(&["-t", "abc"]), 2);
    }
```

- [ ] **Step 2: Run the new tests to confirm they fail**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 7 new tests fail with "unknown operator" or similar. Older tests still pass.

- [ ] **Step 3: Add the access/isatty arms to `eval_unary`**

Insert before the `_ =>` arm in `eval_unary`:

```rust
        "-r" => Ok(nix::unistd::access(arg, nix::unistd::AccessFlags::R_OK).is_ok()),
        "-w" => Ok(nix::unistd::access(arg, nix::unistd::AccessFlags::W_OK).is_ok()),
        "-x" => Ok(nix::unistd::access(arg, nix::unistd::AccessFlags::X_OK).is_ok()),
        "-t" => {
            let fd: i32 = arg
                .trim()
                .parse()
                .map_err(|_| TestError::syntax(format!("{}: integer expression expected", arg)))?;
            Ok(nix::unistd::isatty(fd).unwrap_or(false))
        }
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 29 tests pass (22 + 7 new).

- [ ] **Step 5: Commit**

```bash
git add src/builtin/test.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add -r, -w, -x (access) and -t (isatty) to test / [

Uses nix::unistd::access (real UID per POSIX) and nix::unistd::isatty.
A non-integer argument to -t is reported as an integer-expression
error with exit status 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Unary file mode bits (`-u`, `-g`)

**Files:**
- Modify: `src/builtin/test.rs`

- [ ] **Step 1: Write failing tests**

Add to `mod tests`:

```rust
    #[test]
    fn dash_u_setuid_bit() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o4755)).unwrap();
        assert_eq!(t(&["-u", &path]), 0);
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o0755)).unwrap();
        assert_eq!(t(&["-u", &path]), 1);
    }

    #[test]
    fn dash_g_setgid_bit() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o2755)).unwrap();
        assert_eq!(t(&["-g", &path]), 0);
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o0755)).unwrap();
        assert_eq!(t(&["-g", &path]), 1);
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 2 new tests fail with "unknown operator".

- [ ] **Step 3: Add the mode-bit arms to `eval_unary`**

Insert before the `_ =>` arm in `eval_unary`:

```rust
        "-u" => Ok(std::fs::metadata(arg)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o4000 != 0
            })
            .unwrap_or(false)),
        "-g" => Ok(std::fs::metadata(arg)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o2000 != 0
            })
            .unwrap_or(false)),
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 31 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/test.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add -u, -g setuid/setgid bit tests to test / [

Uses PermissionsExt::mode bitmask (0o4000 / 0o2000).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: 3-operand forms — binary string (`=`, `!=`), binary integer, negation, parens

**Files:**
- Modify: `src/builtin/test.rs`

- [ ] **Step 1: Write failing tests for 3-operand forms**

Add to `mod tests`:

```rust
    #[test]
    fn binary_string_eq() {
        assert_eq!(t(&["abc", "=", "abc"]), 0);
        assert_eq!(t(&["abc", "=", "xyz"]), 1);
    }

    #[test]
    fn binary_string_neq() {
        assert_eq!(t(&["abc", "!=", "xyz"]), 0);
        assert_eq!(t(&["abc", "!=", "abc"]), 1);
    }

    #[test]
    fn binary_integer_eq() {
        assert_eq!(t(&["3", "-eq", "3"]), 0);
        assert_eq!(t(&["3", "-eq", "4"]), 1);
    }

    #[test]
    fn binary_integer_ne_lt_gt_le_ge() {
        assert_eq!(t(&["3", "-ne", "4"]), 0);
        assert_eq!(t(&["3", "-lt", "4"]), 0);
        assert_eq!(t(&["4", "-gt", "3"]), 0);
        assert_eq!(t(&["3", "-le", "3"]), 0);
        assert_eq!(t(&["4", "-ge", "4"]), 0);
    }

    #[test]
    fn binary_integer_strips_whitespace() {
        assert_eq!(t(&[" 42 ", "-eq", "42"]), 0);
    }

    #[test]
    fn binary_integer_signed() {
        assert_eq!(t(&["-3", "-lt", "0"]), 0);
        assert_eq!(t(&["+3", "-eq", "3"]), 0);
    }

    #[test]
    fn binary_integer_parse_error() {
        assert_eq!(t(&["abc", "-eq", "0"]), 2);
        assert_eq!(t(&["0", "-eq", "abc"]), 2);
    }

    #[test]
    fn negation_of_2op_form() {
        assert_eq!(t(&["!", "-z", ""]), 1); // -z "" is true, negation is false
        assert_eq!(t(&["!", "-n", ""]), 0); // -n "" is false, negation is true
    }

    #[test]
    fn paren_grouping_1op() {
        assert_eq!(t(&["(", "x", ")"]), 0);
        assert_eq!(t(&["(", "", ")"]), 1);
    }

    #[test]
    fn unknown_binary_operator_errors() {
        assert_eq!(t(&["a", "-Z", "b"]), 2);
    }
```

- [ ] **Step 2: Run tests to confirm failure**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 10 new tests fail with "unsupported operand count: 3".

- [ ] **Step 3: Extend `evaluate` for 3-operand and add `eval_binary` + `parse_integer`**

Replace the `evaluate` function with:

```rust
fn evaluate(args: &[&str]) -> Result<bool, TestError> {
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            eval_unary(args[0], args[1])
        }
        3 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            if args[0] == "(" && args[2] == ")" {
                return evaluate(&args[1..2]);
            }
            eval_binary(args[0], args[1], args[2])
        }
        _ => Err(TestError::syntax(format!(
            "unsupported operand count: {}",
            args.len()
        ))),
    }
}
```

Add two new helpers at the end of the file (above `mod tests`):

```rust
fn eval_binary(lhs: &str, op: &str, rhs: &str) -> Result<bool, TestError> {
    match op {
        "=" => Ok(lhs == rhs),
        "!=" => Ok(lhs != rhs),
        "-eq" | "-ne" | "-lt" | "-gt" | "-le" | "-ge" => {
            let l = parse_integer(lhs)?;
            let r = parse_integer(rhs)?;
            Ok(match op {
                "-eq" => l == r,
                "-ne" => l != r,
                "-lt" => l < r,
                "-gt" => l > r,
                "-le" => l <= r,
                "-ge" => l >= r,
                _ => unreachable!(),
            })
        }
        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}

fn parse_integer(s: &str) -> Result<i64, TestError> {
    s.trim()
        .parse::<i64>()
        .map_err(|_| TestError::syntax(format!("{}: integer expression expected", s)))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 41 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/test.rs
git commit -m "$(cat <<'EOF'
feat(builtin): support 3-operand forms in test / [

Adds binary string (=, !=) and integer (-eq/-ne/-lt/-gt/-le/-ge)
comparison, leading-! negation of 2-operand forms, and parentheses
grouping around a 1-operand form. Integer parsing trims surrounding
whitespace and accepts signed input; parse failure yields exit 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: 4-operand forms and ≥5-operand rejection

**Files:**
- Modify: `src/builtin/test.rs`

- [ ] **Step 1: Write failing tests**

Add to `mod tests`:

```rust
    #[test]
    fn four_operand_negation_of_binary() {
        assert_eq!(t(&["!", "a", "=", "b"]), 0); // not (a = b)
        assert_eq!(t(&["!", "a", "=", "a"]), 1);
    }

    #[test]
    fn four_operand_paren_wraps_unary() {
        assert_eq!(t(&["(", "-n", "x", ")"]), 0);
        assert_eq!(t(&["(", "-n", "", ")"]), 1);
    }

    #[test]
    fn four_operand_invalid_shape() {
        // Not starting with ! and not wrapped in ( ).
        assert_eq!(t(&["a", "b", "c", "d"]), 2);
    }

    #[test]
    fn five_or_more_operands_is_error() {
        assert_eq!(t(&["a", "b", "c", "d", "e"]), 2);
    }
```

- [ ] **Step 2: Run tests to confirm failure**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 4 new tests fail with "unsupported operand count: 4" (or 5).

- [ ] **Step 3: Extend `evaluate` for 4-operand and ≥5-operand rejection**

Replace the `evaluate` function with its final form:

```rust
fn evaluate(args: &[&str]) -> Result<bool, TestError> {
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            eval_unary(args[0], args[1])
        }
        3 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            if args[0] == "(" && args[2] == ")" {
                return evaluate(&args[1..2]);
            }
            eval_binary(args[0], args[1], args[2])
        }
        4 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            if args[0] == "(" && args[3] == ")" {
                return evaluate(&args[1..3]);
            }
            Err(TestError::syntax(format!(
                "{}: unexpected operator",
                args[1]
            )))
        }
        _ => Err(TestError::syntax("too many arguments".to_string())),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p yosh builtin::test::tests --lib` (timeout ≥ 300000ms)
Expected: 45 tests pass.

- [ ] **Step 5: Run the full unit-test suite**

Run: `cargo test -p yosh --lib` (timeout ≥ 300000ms)
Expected: all workspace unit tests pass. No regressions elsewhere.

- [ ] **Step 6: Commit**

```bash
git add src/builtin/test.rs
git commit -m "$(cat <<'EOF'
feat(builtin): complete test / [ with 4-operand and too-many-args

Adds leading-! negation of a 3-operand expression and parentheses
wrapping a 2-operand expression. 4-operand expressions that match
neither pattern, and ≥5-operand expressions, report a syntax error
with exit status 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: POSIX §2.14 E2E test coverage

**Files:**
- Create: `e2e/posix_spec/2_14_test/*.sh` (15 files)

All files must have `644` permissions (per `CLAUDE.md`), `#!/bin/sh` shebang, and POSIX metadata headers (`POSIX_REF: 2.14 test`, `DESCRIPTION`, `EXPECT_OUTPUT` or omit if not applicable, `EXPECT_EXIT`).

- [ ] **Step 1: Create the directory**

Run: `mkdir -p e2e/posix_spec/2_14_test`
Expected: directory created.

- [ ] **Step 2: Write `test_no_args.sh`**

Create `e2e/posix_spec/2_14_test/test_no_args.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: test with no operands returns exit 1
# EXPECT_EXIT: 1
test
```

- [ ] **Step 3: Write `test_string_nonempty.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: 1-operand form: nonempty string is true
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
if [ "hello" ]; then
    echo ok
fi
```

- [ ] **Step 4: Write `test_bracket_requires_closing.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: [ without closing ] reports syntax error exit 2
# EXPECT_EXIT: 2
[ -n x
```

- [ ] **Step 5: Write `test_integer_compare.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: integer comparison operators
# EXPECT_OUTPUT: lt eq ge
# EXPECT_EXIT: 0
[ 1 -lt 2 ] && printf 'lt '
[ 3 -eq 3 ] && printf 'eq '
[ 4 -ge 4 ] && printf 'ge'
echo
```

- [ ] **Step 6: Write `test_integer_parse_error.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: non-integer operand to -eq reports exit 2
# EXPECT_EXIT: 2
[ abc -eq 0 ]
```

- [ ] **Step 7: Write `test_file_exists.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -e is true for an existing file
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/file_exists_$$"
: > "$f"
[ -e "$f" ] && echo yes
rm -f "$f"
```

- [ ] **Step 8: Write `test_file_regular.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -f is true for a regular file, false for a directory
# EXPECT_OUTPUT: regular notdir
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/regular_$$"
: > "$f"
[ -f "$f" ] && printf 'regular '
[ -f "$TEST_TMPDIR" ] || printf 'notdir'
echo
rm -f "$f"
```

- [ ] **Step 9: Write `test_file_readable.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -r reflects read permission (via chmod)
# EXPECT_OUTPUT: readable
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/readable_$$"
: > "$f"
chmod 0644 "$f"
[ -r "$f" ] && echo readable
rm -f "$f"
```

- [ ] **Step 10: Write `test_file_symlink.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -h and -L both detect symbolic links
# EXPECT_OUTPUT: h L
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
target="$TEST_TMPDIR/sym_target_$$"
link="$TEST_TMPDIR/sym_link_$$"
: > "$target"
ln -s "$target" "$link"
[ -h "$link" ] && printf 'h '
[ -L "$link" ] && printf 'L'
echo
rm -f "$link" "$target"
```

- [ ] **Step 11: Write `test_string_eq_neq.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: string = and != comparisons
# EXPECT_OUTPUT: eq neq
# EXPECT_EXIT: 0
[ "abc" = "abc" ] && printf 'eq '
[ "abc" != "xyz" ] && printf 'neq'
echo
```

- [ ] **Step 12: Write `test_negation.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: ! negates 1-, 2-, and 3-operand forms
# EXPECT_OUTPUT: empty nempty neq
# EXPECT_EXIT: 0
[ ! "" ] || printf 'empty '        # ! "" → true
[ ! -z "x" ] && printf 'nempty '   # ! -z "x" → true
[ ! "a" = "b" ] && printf 'neq'    # ! (a = b) → true
echo
```

- [ ] **Step 13: Write `test_paren_grouping.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: ( E ) grouping around 1- and 2-operand forms
# EXPECT_OUTPUT: one two
# EXPECT_EXIT: 0
[ \( "x" \) ] && printf 'one '
[ \( -n "x" \) ] && printf 'two'
echo
```

- [ ] **Step 14: Write `test_unknown_operator.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: unknown unary operator reports exit 2
# EXPECT_EXIT: 2
[ -Z foo ]
```

- [ ] **Step 15: Write `test_isatty_fd.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -t is false for a non-terminal stdin
# EXPECT_EXIT: 1
[ -t 0 ] < /dev/null
```

- [ ] **Step 16: Write `test_too_many_args.sh`**

```sh
#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: more than 4 operands reports exit 2
# EXPECT_EXIT: 2
[ a b c d e ]
```

- [ ] **Step 17: Set permissions to 644 on all new files**

Run: `chmod 644 e2e/posix_spec/2_14_test/*.sh`
Expected: all 15 files have `-rw-r--r--` permission.

- [ ] **Step 18: Build yosh (debug) — E2E harness requires the debug binary**

Run: `cargo build` (timeout ≥ 300000ms)
Expected: `target/debug/yosh` exists.

- [ ] **Step 19: Run the new E2E tests**

Run: `./e2e/run_tests.sh --filter=2_14_test` (timeout ≥ 120000ms)
Expected: all 15 tests pass.

- [ ] **Step 20: Run the full E2E suite to confirm no regressions elsewhere**

Run: `./e2e/run_tests.sh` (timeout ≥ 300000ms)
Expected: all tests pass. The 10+ existing files already using `[` will continue to pass — now via the new builtin path.

- [ ] **Step 21: Commit**

```bash
git add e2e/posix_spec/2_14_test/
git commit -m "$(cat <<'EOF'
test(e2e): add POSIX §2.14 coverage for test / [ builtins

Fifteen representative scripts exercising 0..=4 operand dispatch,
file predicates (-e, -f, -r, -h/-L), binary string and integer
comparisons, negation, parens, unknown-operator and
too-many-arguments error paths, and -t fd.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Post-implementation measurement

**Files:** none (measurement only)

- [ ] **Step 1: Re-run Criterion against the `pre-bracket-builtin` baseline**

Run: `cargo bench --bench exec_bench -- --baseline pre-bracket-builtin` (timeout ≥ 600000ms)
Expected: Criterion prints per-bench comparison. Capture each median and delta. Expected outcomes:
- `exec_bracket_loop_200` — ≥10× improvement (target ≥100×)
- `exec_function_call_200` — substantial improvement because its body uses `while [ ]`
- `exec_for_loop_200`, `exec_param_expansion_200` — no material change (no `[` inside)

Save the console output to a local note (not git-tracked) for the next step.

- [ ] **Step 2: Re-run dhat on W2**

Run:
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
```
(timeout ≥ 300000ms)

Then: `mv dhat-heap.json target/perf/dhat-heap-w2-post-bracket.json`

- [ ] **Step 3: Re-extract the dhat top-10**

Run: `python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-post-bracket.json 10`
Expected: `build_environ` / `build_env_vars` no longer dominate. Top hotspots shift to `pathname::expand`, `pattern::matches`, `field_split::emit`. Record the new top-10 for §6.2.

- [ ] **Step 4: No commit**

These artifacts live under `target/perf/` which is gitignored. The numbers feed the next task's documentation edits.

---

## Task 11: Update `performance.md`

**Files:**
- Modify: `performance.md`

Use the numbers captured in Task 10 wherever the instructions below say "post-fix measurement."

- [ ] **Step 1: Rewrite Executive-Summary hotspot #1**

In `performance.md`, locate the section "§1. Executive Summary" → "Top 5 hotspots (W2)". Replace bullet 1 with:

```markdown
1. **`[` / `test` dispatched as an external command per while-loop iteration** — W2 Section B's `while [ "$i" -lt 1000 ]` forked and `execvp`'d `/usr/bin/[` once per iteration, producing ~1001 `build_env_vars` allocations (rank #5 dhat site, 1001 outer-Vec allocs). Fixed 2026-04-21 by classifying `test` / `[` as Regular builtins (see `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`). Post-fix W2 total allocation dropped from 68.1 MB to ~<POST-FIX> MB.
```

Replace `<POST-FIX>` with the actual value captured in Task 10 Step 3.

- [ ] **Step 2: Correct §4.1 "Suspected cause" paragraph**

In §4.1, locate the paragraph beginning `**Suspected cause:** \`VarStore::environ()\` returns ...`. Replace that entire paragraph with:

```markdown
**Root cause (corrected 2026-04-21):** The original diagnosis was wrong. `build_env_vars` is called **only** from the `NotBuiltin` dispatch path in `src/exec/simple.rs:383` — never for builtins. The real driver was that `classify_builtin` (`src/builtin/mod.rs`) did not list `test` / `[`. W2 Section B's `while [ ... ]; do ... done` therefore forked + `execvp`'d `/usr/bin/[` once per iteration. The 1001-call rank-5 dhat entry matches the loop iteration count exactly.
```

- [ ] **Step 3: Mark fix candidate #1 in §4.1 as obsolete and record the executed fix**

In §4.1 under "Fix candidates:", replace the whole numbered list with:

```markdown
**Fix applied (2026-04-21):**

**Promote `test` / `[` to `Regular` builtins** per POSIX §2.14. Eliminates 1001 `fork`+`execvp` per W2 run. See `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md` and the implementing commits.

**Originally proposed fixes, re-evaluated:**

1. ~~Skip `build_env_vars` entirely for builtins~~ — already in place; provided no benefit because builtins never entered that path.
2. **Return a reference/iterator from `environ()` and defer the `.to_vec()`** — still applicable for the few remaining genuine external-command invocations. Deferred to a future P1/P2 if post-fix measurements still show allocation pressure here.
3. **Scoped cache invalidation** — only bump the environ cache when an *exported* variable changes. Still applicable; see §4.2.
```

- [ ] **Step 4: Refresh §3.2 dhat Top-10 with post-fix numbers**

Locate the "dhat Top-10 by bytes (W2)" and "dhat Top-10 by call count (W2)" tables. Either:
- replace the tables with the post-fix numbers captured in Task 10, and update the paragraph prose above each to reflect the new hotspots; OR
- keep both pre- and post-fix tables side-by-side under clearly labeled subsections "Pre-fix (commit 1e1b738)" and "Post-fix (commit <HEAD>)".

The second option is preferable for audit trail. Use the HEAD SHA from `git rev-parse --short HEAD` at the time of this edit as the post-fix commit identifier.

- [ ] **Step 5: Update §5.1 Priority matrix**

Locate the Priority matrix table row "4.1 — `environ().to_vec()` per command". Replace its Priority cell `**P0**` with `**done**` and append to the Notes cell:

```
Completed 2026-04-21 via `[`/`test` builtin promotion. See `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`.
```

- [ ] **Step 6: Update §5.2 Next-project queue**

At the top of "In order:", delete the item that was "P0 — Fix 4.1, starting with candidate #1 (`build_env_vars` skip for builtins)" and renumber the remaining items. Promote the §4.2 function-call investigation to position 1.

Also add a note immediately under the "In order:" header:

```markdown
**Note (2026-04-21):** Item 4.1 has been completed via `test`/`[` builtin promotion. The §4.2 function-call Criterion baseline must be re-captured because its benchmark used `while [ ]` internally and therefore inherited the external-`[` overhead in the original measurement.
```

- [ ] **Step 7: Append §4.6 — audit trail of the correction**

At the end of §4 (after §4.5), append a new section:

```markdown
### 4.6. Correction to §4.1 root-cause analysis (added 2026-04-21)

The original §4.1 diagnosis ("every command execution clones the full exported-env snapshot, even for builtins") was derived from the dhat line-attributed call counts without verifying the actual dispatch path in `src/exec/simple.rs`. Code inspection during the fix work showed that `build_env_vars` has always been gated behind `BuiltinKind::NotBuiltin`. The real driver of the 1001-call rank-5 site was that `classify_builtin` did not list `test` / `[`, so every iteration of `while [ ... ]` in W2 Section B spawned `/usr/bin/[` through fork + execvp.

This mischaracterization is preserved here (rather than silently rewriting §4.1) so that future readers can see both the original mistake and the correction. The lesson: when a dhat call count does not round-trip to a plausible code path, verify the dispatch path before recommending a fix.
```

- [ ] **Step 8: Verify the rendered markdown is coherent**

Run: `cat performance.md | head -200` and `grep -n "§4" performance.md`
Expected: no dangling references to the removed prose, section numbering intact, corrections visible.

- [ ] **Step 9: Commit**

```bash
git add performance.md
git commit -m "$(cat <<'EOF'
docs(perf): correct §4.1 root cause and record [ builtin outcome

Replaces the incorrect "environ cloned per builtin" diagnosis with
the verified cause: classify_builtin omitted test / [, so while-loop
conditions forked /usr/bin/[ once per iteration. Records the fix
(test / [ promoted to Regular builtins) and the post-fix dhat/
Criterion numbers. Adds §4.6 preserving the audit trail of the
mischaracterization.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Post-plan verification

- [ ] **Step 1: Full workspace test suite**

Run: `cargo test --workspace` (timeout ≥ 300000ms)
Expected: all tests pass.

- [ ] **Step 2: Full E2E suite**

Run: `cargo build && ./e2e/run_tests.sh` (timeout ≥ 300000ms)
Expected: all tests pass.

- [ ] **Step 3: Confirm the Criterion comparison report matches the documented numbers**

Re-run: `cargo bench --bench exec_bench -- --baseline pre-bracket-builtin | tee /tmp/final-bench.txt` (timeout ≥ 600000ms)
Expected: the medians agree with what was recorded in `performance.md` §3.2 (within Criterion noise, ±5%).
