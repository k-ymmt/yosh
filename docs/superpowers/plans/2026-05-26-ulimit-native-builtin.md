# Native `ulimit` builtin (POSIX-minimal `-f`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `ulimit` as a native regular builtin scoped to POSIX's `-f` (512-byte-block file-size limit), closing the last remaining e2e XFAIL.

**Architecture:** A pure parse function (`parse_ulimit`) and a pure formatter (`format_fsize_limit`) keep all decision logic unit-testable; a thin `libc::{getrlimit,setrlimit}` layer performs the side effects. Errors print to stderr and exit 1 (matching the `read` builtin precedent). The builtin lives in `src/builtin/regular.rs` next to `umask` and is registered in `src/builtin/mod.rs`.

**Tech Stack:** Rust, `libc` crate (`getrlimit`/`setrlimit`/`rlimit`/`RLIMIT_FSIZE`/`RLIM_INFINITY`), yosh's existing builtin dispatch.

**Spec:** `docs/superpowers/specs/2026-05-26-ulimit-native-builtin-design.md`

---

## File Structure

- `src/builtin/regular.rs` — add `UlimitAction`, `UlimitArgError` (private enums), `parse_ulimit`, `format_fsize_limit`, `set_fsize`, `builtin_ulimit` (pub), and unit tests in the existing `#[cfg(test)] mod tests` (starts at line 558, has `use super::*`).
- `src/builtin/mod.rs` — register the name in `BUILTIN_NAMES`, `classify_builtin`, and `exec_regular_builtin`.
- `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh` — drop the `# XFAIL:` line.
- `TODO.md` — remove the completed `ulimit` item (and its now-empty section).

**Note on intermediate warnings:** `parse_ulimit` / `format_fsize_limit` are private and only referenced by tests until Task 3 wires them into `builtin_ulimit`. The per-task verify steps use `cargo test`, which compiles the test code that *does* use them, so no `dead_code` warning appears. `cargo clippy` is run only in the final task, by which point everything is wired up.

---

### Task 1: Argument parsing (`parse_ulimit`) + action/error enums

**Files:**
- Modify: `src/builtin/regular.rs` (add code before the `#[cfg(test)]` line at 558; tests inside `mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/builtin/regular.rs` (it already has `use super::*;`):

```rust
fn uargs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn test_parse_ulimit_show() {
    assert_eq!(parse_ulimit(&uargs(&[])), Ok(UlimitAction::Show));
    assert_eq!(parse_ulimit(&uargs(&["-f"])), Ok(UlimitAction::Show));
}

#[test]
fn test_parse_ulimit_set_blocks() {
    assert_eq!(parse_ulimit(&uargs(&["-f", "100"])), Ok(UlimitAction::SetBlocks(100)));
    assert_eq!(parse_ulimit(&uargs(&["100"])), Ok(UlimitAction::SetBlocks(100)));
}

#[test]
fn test_parse_ulimit_unlimited() {
    assert_eq!(parse_ulimit(&uargs(&["-f", "unlimited"])), Ok(UlimitAction::SetUnlimited));
    assert_eq!(parse_ulimit(&uargs(&["unlimited"])), Ok(UlimitAction::SetUnlimited));
}

#[test]
fn test_parse_ulimit_unknown_option() {
    assert_eq!(
        parse_ulimit(&uargs(&["-Z"])),
        Err(UlimitArgError::UnknownOption("-Z".to_string()))
    );
}

#[test]
fn test_parse_ulimit_invalid_number() {
    assert_eq!(
        parse_ulimit(&uargs(&["-f", "abc"])),
        Err(UlimitArgError::InvalidNumber("abc".to_string()))
    );
    // A leading '-' in operand position is not an option; negative is rejected.
    assert_eq!(
        parse_ulimit(&uargs(&["-f", "-5"])),
        Err(UlimitArgError::InvalidNumber("-5".to_string()))
    );
}

#[test]
fn test_parse_ulimit_too_many_args() {
    assert_eq!(parse_ulimit(&uargs(&["-f", "1", "2"])), Err(UlimitArgError::TooManyArgs));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib parse_ulimit 2>&1 | tail -20`
Expected: FAIL to compile — `cannot find function 'parse_ulimit'` / `cannot find type 'UlimitAction'`.

- [ ] **Step 3: Write the enums and `parse_ulimit`**

Add to `src/builtin/regular.rs` (e.g. directly after the `umask` functions, before `#[cfg(test)]`):

```rust
/// Parsed action for the `ulimit` builtin (POSIX-minimal `-f`).
#[derive(Debug, PartialEq)]
enum UlimitAction {
    /// Report the current soft file-size limit.
    Show,
    /// Set the file-size limit to N 512-byte blocks.
    SetBlocks(u64),
    /// Set the file-size limit to unlimited.
    SetUnlimited,
}

/// Argument-parse failures for `ulimit`. All map to exit status 1.
#[derive(Debug, PartialEq)]
enum UlimitArgError {
    UnknownOption(String),
    InvalidNumber(String),
    TooManyArgs,
}

/// Parse `ulimit [-f] [blocks]` arguments. Pure: no syscalls.
///
/// Option detection applies only to the leading token. A leading `-f` is the
/// only accepted option; any other leading token starting with `-` (and not
/// exactly `-`) is an unknown option. Remaining tokens are operands, where a
/// leading `-` is not an option (so `-f -5` is an invalid number, not an
/// option). At most one operand is allowed.
fn parse_ulimit(args: &[String]) -> Result<UlimitAction, UlimitArgError> {
    let mut i = 0;
    if let Some(first) = args.first() {
        if first == "-f" {
            i = 1;
        } else if first.starts_with('-') && first != "-" {
            return Err(UlimitArgError::UnknownOption(first.clone()));
        }
    }
    match &args[i..] {
        [] => Ok(UlimitAction::Show),
        [op] => {
            if op == "unlimited" {
                Ok(UlimitAction::SetUnlimited)
            } else {
                op.parse::<u64>()
                    .map(UlimitAction::SetBlocks)
                    .map_err(|_| UlimitArgError::InvalidNumber(op.clone()))
            }
        }
        _ => Err(UlimitArgError::TooManyArgs),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib parse_ulimit 2>&1 | tail -20`
Expected: PASS (6 tests: show, set_blocks, unlimited, unknown_option, invalid_number, too_many_args).

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -F - <<'EOF'
feat(builtin): add ulimit argument parser (POSIX -f)

Pure parse_ulimit + UlimitAction/UlimitArgError enums for the native ulimit
builtin. Decision logic is syscall-free so it is fully unit-testable.

Task: implement native ulimit (last remaining e2e XFAIL); POSIX-minimal -f.
Spec: docs/superpowers/specs/2026-05-26-ulimit-native-builtin-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: Limit formatter (`format_fsize_limit`) + block-size constant

**Files:**
- Modify: `src/builtin/regular.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/builtin/regular.rs`:

```rust
#[test]
fn test_format_fsize_limit() {
    assert_eq!(format_fsize_limit(libc::RLIM_INFINITY), "unlimited");
    assert_eq!(format_fsize_limit(51200), "100"); // 51200 / 512
    assert_eq!(format_fsize_limit(0), "0");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib format_fsize_limit 2>&1 | tail -20`
Expected: FAIL to compile — `cannot find function 'format_fsize_limit'`.

- [ ] **Step 3: Write the constant and formatter**

Add to `src/builtin/regular.rs` (near `parse_ulimit`):

```rust
/// POSIX `ulimit -f` operates in 512-byte blocks.
const BLOCK_SIZE: libc::rlim_t = 512;

/// Render a soft file-size limit (in bytes) as a POSIX block count, or
/// `"unlimited"` for `RLIM_INFINITY`.
fn format_fsize_limit(rlim_cur: libc::rlim_t) -> String {
    if rlim_cur == libc::RLIM_INFINITY {
        "unlimited".to_string()
    } else {
        (rlim_cur / BLOCK_SIZE).to_string()
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib format_fsize_limit 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -F - <<'EOF'
feat(builtin): add ulimit file-size limit formatter

format_fsize_limit renders RLIMIT_FSIZE soft limits as POSIX 512-byte block
counts (or "unlimited"). Pure; unit-tested.

Task: implement native ulimit; POSIX-minimal -f.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: `builtin_ulimit` orchestration + `set_fsize` syscall layer

**Files:**
- Modify: `src/builtin/regular.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/builtin/regular.rs`:

```rust
#[test]
fn test_ulimit_show_returns_ok() {
    // Read-only path: succeeds without altering any limit.
    assert_eq!(builtin_ulimit(&uargs(&["-f"])), Ok(0));
}

#[test]
fn test_ulimit_unknown_option_returns_one() {
    assert_eq!(builtin_ulimit(&uargs(&["-Z"])), Ok(1));
}

#[test]
fn test_ulimit_invalid_number_returns_one() {
    assert_eq!(builtin_ulimit(&uargs(&["-f", "abc"])), Ok(1));
}

#[test]
fn test_ulimit_set_to_current_hard_is_safe() {
    // Setting the limit to the current HARD value is safe inside the shared
    // test process: soft can only rise to hard (never causes SIGXFSZ), hard
    // stays unchanged, and it never fails with EPERM. This exercises the
    // setrlimit success path without lowering any limit for sibling tests.
    let mut rl = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `rl` is a valid, aligned rlimit; getrlimit only writes into it.
    let rc = unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, &mut rl) };
    assert_eq!(rc, 0);
    let operand = if rl.rlim_max == libc::RLIM_INFINITY {
        "unlimited".to_string()
    } else {
        (rl.rlim_max / BLOCK_SIZE).to_string()
    };
    assert_eq!(builtin_ulimit(&uargs(&["-f", &operand])), Ok(0));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ulimit 2>&1 | tail -20`
Expected: FAIL to compile — `cannot find function 'builtin_ulimit'`.

- [ ] **Step 3: Write `builtin_ulimit` and `set_fsize`**

Add to `src/builtin/regular.rs` (near the other ulimit helpers):

```rust
/// `ulimit [-f] [blocks]` — query or set the file-size limit (POSIX-minimal).
pub fn builtin_ulimit(args: &[String]) -> Result<i32, ShellError> {
    let action = match parse_ulimit(args) {
        Ok(a) => a,
        Err(UlimitArgError::UnknownOption(o)) => {
            eprintln!("yosh: ulimit: {o}: invalid option");
            return Ok(1);
        }
        Err(UlimitArgError::InvalidNumber(n)) => {
            eprintln!("yosh: ulimit: {n}: invalid number");
            return Ok(1);
        }
        Err(UlimitArgError::TooManyArgs) => {
            eprintln!("yosh: ulimit: too many arguments");
            return Ok(1);
        }
    };

    match action {
        UlimitAction::Show => {
            let mut rl = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            // SAFETY: `rl` is a valid, aligned rlimit; RLIMIT_FSIZE is a valid
            // resource id. getrlimit only writes into `rl`.
            let rc = unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, &mut rl) };
            if rc != 0 {
                eprintln!("yosh: ulimit: {}", std::io::Error::last_os_error());
                return Ok(1);
            }
            println!("{}", format_fsize_limit(rl.rlim_cur));
            Ok(0)
        }
        UlimitAction::SetBlocks(n) => set_fsize(n.saturating_mul(BLOCK_SIZE)),
        UlimitAction::SetUnlimited => set_fsize(libc::RLIM_INFINITY),
    }
}

/// Set both the soft and hard `RLIMIT_FSIZE` to `bytes`. Setting both when no
/// `-H`/`-S` selector is given matches bash, ksh, and dash.
fn set_fsize(bytes: libc::rlim_t) -> Result<i32, ShellError> {
    let rl = libc::rlimit { rlim_cur: bytes, rlim_max: bytes };
    // SAFETY: `rl` is a valid rlimit; RLIMIT_FSIZE is a valid resource id.
    // setrlimit only reads `rl`.
    let rc = unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &rl) };
    if rc != 0 {
        eprintln!("yosh: ulimit: {}", std::io::Error::last_os_error());
        return Ok(1);
    }
    Ok(0)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ulimit 2>&1 | tail -20`
Expected: PASS (all `parse_ulimit`, `format_fsize_limit`, and `builtin_ulimit` tests). Note: the unknown-option/invalid-number tests print `yosh: ulimit: ...` to stderr — this is expected test noise.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -F - <<'EOF'
feat(builtin): implement native ulimit -f (get/set RLIMIT_FSIZE)

builtin_ulimit orchestrates parse -> getrlimit/setrlimit. Show prints the
soft limit; set updates both soft and hard (bash/ksh/dash convention). All
errors exit 1. set_fsize is the thin syscall layer.

Unit tests cover the show path, error paths, and a safe set round-trip
(set to current hard value — never lowers a limit, never EPERMs) so the
shared test process cannot hit SIGXFSZ.

Task: implement native ulimit; POSIX-minimal -f.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: Register `ulimit` in builtin dispatch

**Files:**
- Modify: `src/builtin/mod.rs` (`BUILTIN_NAMES` ~line 18, `classify_builtin` ~line 40, `exec_regular_builtin` ~line 54)

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/builtin/mod.rs`:

```rust
#[test]
fn test_classify_ulimit() {
    assert!(matches!(classify_builtin("ulimit"), BuiltinKind::Regular));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib test_classify_ulimit 2>&1 | tail -20`
Expected: FAIL — `ulimit` classifies as `NotBuiltin`, the `matches!` assertion fails.

- [ ] **Step 3: Register the name in all three places**

In `src/builtin/mod.rs`, add `"ulimit"` after `"umask"` in `BUILTIN_NAMES`:

```rust
    "jobs", "umask", "ulimit", "test", "[", "type", "hash", "read", "getopts",
```

Add `| "ulimit"` after `"umask"` in the `classify_builtin` regular arm:

```rust
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "ulimit" | "test" | "[" | "type" | "hash" | "read"
        | "getopts" => BuiltinKind::Regular,
```

Add the dispatch arm after the `"umask"` arm in `exec_regular_builtin`:

```rust
        "umask" => regular::builtin_umask(args),
        "ulimit" => regular::builtin_ulimit(args),
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib test_classify_ulimit test_builtin_names_consistent_with_classify 2>&1 | tail -20`
Expected: PASS (both the new classify test and the existing consistency test).

- [ ] **Step 5: Commit**

```bash
git add src/builtin/mod.rs
git commit -F - <<'EOF'
feat(builtin): register ulimit as a regular builtin

Add ulimit to BUILTIN_NAMES, classify_builtin (Regular), and the
exec_regular_builtin dispatch so the name resolves to the native builtin
instead of /usr/bin/ulimit.

Task: implement native ulimit; POSIX-minimal -f.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: Flip the e2e XFAIL to PASS

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh` (remove the `# XFAIL:` line)

- [ ] **Step 1: Build the debug binary (e2e runner requires it)**

Run: `cargo build 2>&1 | tail -3`
Expected: `Finished ... target(s)`. (May take 1–3 min.)

- [ ] **Step 2: Confirm the test is currently XFAIL**

Run: `./e2e/run_tests.sh --filter=ulimit 2>&1 | tail -20`
Expected: 3 ulimit tests run; `ulimit_set_filesize.sh` and `ulimit_show_filesize.sh` PASS, `ulimit_unknown_option.sh` shows `[XFAIL]` (or, now that the builtin behaves correctly, an `[XPASS]`/unexpected-pass warning — either way the `# XFAIL:` line must go).

- [ ] **Step 3: Remove the `# XFAIL:` line**

Delete this line from `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`:

```
# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
```

The file should then read:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit with unknown option is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: ulimit
ulimit -Z 2>&1 1>/dev/null
```

- [ ] **Step 4: Run the filtered e2e suite to verify all three PASS**

Run: `./e2e/run_tests.sh --filter=ulimit 2>&1 | tail -20`
Expected: 3 PASS, 0 XFAIL, 0 FAIL. `ulimit -Z` now produces `yosh: ulimit: -Z: invalid option` (stderr substring `ulimit` matches) and exit 1.

- [ ] **Step 5: Commit**

```bash
git add e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh
git commit -F - <<'EOF'
test(e2e): un-XFAIL ulimit unknown-option now that ulimit is native

The native builtin returns exit 1 with "yosh: ulimit: -Z: invalid option",
satisfying EXPECT_EXIT 1 / EXPECT_STDERR ulimit. All three ulimit e2e tests
now pass.

Task: implement native ulimit; POSIX-minimal -f.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 6: Remove the completed TODO item + full verification

**Files:**
- Modify: `TODO.md` (remove the `## Future: POSIX Required Builtin Implementation` section — `ulimit` is its only entry)

- [ ] **Step 1: Remove the completed section from `TODO.md`**

Delete the entire `## Future: POSIX Required Builtin Implementation` section, including its preamble paragraph and the single `ulimit` bullet (the block beginning `## Future: POSIX Required Builtin Implementation` and ending with `— unknown-option case)`). Per CLAUDE.md, completed items are deleted, not marked `[x]`. Since `ulimit` is the only entry, the whole section goes.

- [ ] **Step 2: Verify the section is gone**

Run: `grep -n "POSIX Required Builtin\|ulimit" TODO.md`
Expected: no output (no matches).

- [ ] **Step 3: Run the full unit/integration suite**

Run (background — the build + tests can take several minutes): `cargo test 2>&1 | tail -25`
Expected: all tests pass, including the new `ulimit` tests and `test_classify_ulimit`.

- [ ] **Step 4: Verify formatting and lints are clean**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets 2>&1 | tail -25`
Expected: `cargo fmt` produces no output (clean); `cargo clippy` reports no warnings on the new code.

- [ ] **Step 5: Run the full e2e suite to confirm no regressions**

Run: `./e2e/run_tests.sh 2>&1 | tail -15`
Expected: summary shows 0 FAIL and 0 XFAIL (the ulimit XFAIL was the last one).

- [ ] **Step 6: Commit**

```bash
git add TODO.md
git commit -F - <<'EOF'
docs(todo): remove completed ulimit item (native -f implemented)

ulimit was the sole entry under "POSIX Required Builtin Implementation" and
the last remaining e2e XFAIL; the native -f builtin closes it. Section removed.

Task: implement native ulimit; POSIX-minimal -f.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
```

---

## Self-Review

**Spec coverage:**
- §3 regular builtin, `umask`-style signature → Task 3 (`builtin_ulimit`), Task 4 (registration). ✓
- §3.1 `parse_ulimit` + table → Task 1. ✓
- §3.2 `format_fsize_limit` + 512-byte blocks → Task 2. ✓
- §3.3 syscall layer, set both soft+hard, show soft → Task 3 (`set_fsize`, `Show` arm). ✓
- §3.4 errors exit 1, `yosh:`-prefixed messages → Task 3. ✓
- §4.1 safety constraint (no restrictive `setrlimit` in unit tests) → Task 3 uses the current-hard-value round-trip. ✓
- §4.2 unit tests → Tasks 1–3. ✓
- §4.3 e2e (drop XFAIL, keep set/show) → Task 5. ✓
- §5 acceptance criteria → Task 6 (full `cargo test`, fmt, clippy, full e2e). ✓
- §6 files touched → all four files covered (regular.rs, mod.rs, ulimit_unknown_option.sh, TODO.md). ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"/"similar to" — every code/command step is concrete. ✓

**Type consistency:** `UlimitAction` (`Show`/`SetBlocks(u64)`/`SetUnlimited`), `UlimitArgError` (`UnknownOption(String)`/`InvalidNumber(String)`/`TooManyArgs`), `parse_ulimit(&[String]) -> Result<UlimitAction, UlimitArgError>`, `format_fsize_limit(libc::rlim_t) -> String`, `set_fsize(libc::rlim_t) -> Result<i32, ShellError>`, `builtin_ulimit(&[String]) -> Result<i32, ShellError>`, `BLOCK_SIZE: libc::rlim_t`. Names/signatures are consistent across Tasks 1–4 and match the dispatch call `regular::builtin_ulimit(args)`. ✓
