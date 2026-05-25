# export / readonly `--` end-of-options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the SP1 regression where `export -- foo=v` and `readonly -- foo=v` report `--` as an invalid identifier, by routing `--` end-of-options handling through a shared helper used by export / readonly / unset.

**Architecture:** Add a private pure helper `consume_end_of_options(args, idx) -> usize` to `src/builtin/special.rs`. Apply at the operand-loop entry of `builtin_export` and `builtin_readonly`. Refactor `builtin_unset`'s existing inline `--` skip to call the helper (behavior preserved). Tighten 2 false-positive e2e tests and add 1 new e2e test.

**Tech Stack:** Rust 2024 edition, cargo, existing `#[cfg(test)] mod tests` in `src/builtin/special.rs`, POSIX e2e harness `e2e/run_tests.sh`.

**Spec:** `docs/superpowers/specs/2026-05-25-export-readonly-double-dash-design.md`

---

## File Structure

- **Modify** `src/builtin/special.rs`
  - Add private fn `consume_end_of_options` near other private helpers
  - Update `builtin_export` body (line 85-120)
  - Update `builtin_readonly` body (line 173-216)
  - Update `builtin_unset` body (line 122-171) — refactor only
  - Add ~11 new unit tests in existing `mod tests` (line 895+)
- **Modify** `e2e/posix_spec/4_special_builtin/export_dash_dash.sh` — tighten
- **Modify** `e2e/posix_spec/4_special_builtin/readonly_dash_dash.sh` — tighten
- **Create** `e2e/posix_spec/4_special_builtin/unset_dash_dash.sh` — new
- **Modify** `TODO.md` — delete the closed SP1 follow-up item (per project convention: delete completed items, never mark `[x]`)

---

### Task 1: Add `consume_end_of_options` helper + unit tests

**Files:**
- Modify: `src/builtin/special.rs` — add helper near line 84 (just before `builtin_export`), add unit tests in `mod tests` at line 895+

- [ ] **Step 1: Write failing unit tests for the helper**

Insert these at the bottom of `mod tests` in `src/builtin/special.rs` (after line 1013):

```rust
    #[test]
    fn consume_end_of_options_skips_double_dash() {
        let args = vec!["--".to_string(), "foo".to_string()];
        assert_eq!(consume_end_of_options(&args, 0), 1);
    }

    #[test]
    fn consume_end_of_options_leaves_idx_when_not_double_dash() {
        let args = vec!["foo".to_string()];
        assert_eq!(consume_end_of_options(&args, 0), 0);
    }

    #[test]
    fn consume_end_of_options_handles_empty_args() {
        let args: Vec<String> = vec![];
        assert_eq!(consume_end_of_options(&args, 0), 0);
    }

    #[test]
    fn consume_end_of_options_handles_idx_at_double_dash_mid_array() {
        let args = vec!["-f".to_string(), "--".to_string(), "x".to_string()];
        assert_eq!(consume_end_of_options(&args, 1), 2);
    }

    #[test]
    fn consume_end_of_options_handles_idx_out_of_range() {
        let args = vec!["foo".to_string()];
        assert_eq!(consume_end_of_options(&args, 5), 5);
    }
```

- [ ] **Step 2: Run tests to verify they fail with "not defined"**

```bash
cargo test --lib builtin::special::tests::consume_end_of_options 2>&1 | tail -10
```

Expected: compile error `cannot find function 'consume_end_of_options' in this scope`.

- [ ] **Step 3: Add the helper to `src/builtin/special.rs`**

Insert immediately before `fn builtin_export` (around line 84):

```rust
// POSIX XBD §12.2 Utility Syntax Guideline 10: `--` marks end of options.
// Shared by export / readonly / unset to keep operand validation consistent.
fn consume_end_of_options(args: &[String], idx: usize) -> usize {
    if args.get(idx).map(String::as_str) == Some("--") {
        idx + 1
    } else {
        idx
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib builtin::special::tests::consume_end_of_options 2>&1 | tail -10
```

Expected: `5 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/special.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add consume_end_of_options helper

POSIX XBD §12.2 Utility Syntax Guideline 10 helper to be applied at the
operand-loop entry of export / readonly / unset. Pure function with 5
unit tests covering the match / no-match / empty / mid-array / out-of-range
cases.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Apply helper in `builtin_export` + unit tests + e2e tighten

**Files:**
- Modify: `src/builtin/special.rs::builtin_export` (line 85-120)
- Modify: `src/builtin/special.rs` `mod tests` — 4 new tests
- Modify: `e2e/posix_spec/4_special_builtin/export_dash_dash.sh`

- [ ] **Step 1: Write failing unit tests for export `--` behavior**

Append to `mod tests` in `src/builtin/special.rs`:

```rust
    #[test]
    fn export_double_dash_then_assignment_succeeds() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["--".to_string(), "foo=hi".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), Some("hi"));
    }

    #[test]
    fn export_double_dash_alone_is_noop_rc0() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["--".to_string()], &mut executor);
        assert_eq!(status, 0);
    }

    #[test]
    fn export_double_dash_then_dash_p_is_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn export_dash_p_alone_remains_listing() {
        // Regression guard: -p as the only arg still triggers listing rc=0,
        // helper must not interfere with the listing branch.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib builtin::special::tests::export_double_dash 2>&1 | tail -15
```

Expected: `export_double_dash_then_assignment_succeeds` fails (status=1, expected 0); `export_double_dash_alone_is_noop_rc0` fails. The other two should already pass.

- [ ] **Step 3: Apply helper in `builtin_export`**

Replace lines 85-120 of `src/builtin/special.rs` (the entire `fn builtin_export`) with:

```rust
fn builtin_export(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() || args[0] == "-p" {
        // Print all exported variables in POSIX re-input format
        let mut exported: Vec<(String, String)> = env.vars.environ().to_vec();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            println!("export {}=\"{}\"", name, value);
        }
        return Ok(0);
    }

    let start = consume_end_of_options(args, 0);
    let mut status = 0;
    for arg in &args[start..] {
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
            if let Err(e) = env.assign_var(name, raw_value) {
                eprintln!("yosh: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else {
            env.vars.export(name);
        }
    }
    Ok(status)
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib builtin::special::tests::export 2>&1 | tail -15
```

Expected: all `export_*` tests pass, including the 4 new ones and the pre-existing `export_rejects_invalid_identifier`.

- [ ] **Step 5: Tighten the e2e test**

Overwrite `e2e/posix_spec/4_special_builtin/export_dash_dash.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export (XBD 12.2 Guideline 10)
# DESCRIPTION: export -- treats following operands as names; -- itself is consumed
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
export -- foo=hi || exit 99
sh -c 'echo "$foo"'
```

```bash
chmod 644 e2e/posix_spec/4_special_builtin/export_dash_dash.sh
```

- [ ] **Step 6: Run e2e to verify pass**

```bash
./e2e/run_tests.sh --filter=export_dash_dash 2>&1 | tail -5
```

Expected: `[PASS]  posix_spec/4_special_builtin/export_dash_dash.sh` and `Total: 1  Passed: 1  Failed: 0`.

- [ ] **Step 7: Commit**

```bash
git add src/builtin/special.rs e2e/posix_spec/4_special_builtin/export_dash_dash.sh
git commit -m "$(cat <<'EOF'
fix(builtin): honor `--` as end-of-options in export

Previously `export -- foo=v` reported `--` as not a valid identifier
(SP1 regression). Now uses consume_end_of_options to skip a leading `--`
before operand validation. 4 unit tests cover -- + assignment, --
alone, -- + -p (invalid id), and -p alone (listing regression guard).
The existing e2e test was a false positive (only checked the last
command's exit) — tightened with `|| exit 99` so a regression
surfaces.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Apply helper in `builtin_readonly` + unit tests + e2e tighten

**Files:**
- Modify: `src/builtin/special.rs::builtin_readonly` (line 173-216)
- Modify: `src/builtin/special.rs` `mod tests` — 4 new tests
- Modify: `e2e/posix_spec/4_special_builtin/readonly_dash_dash.sh`

- [ ] **Step 1: Write failing unit tests for readonly `--` behavior**

Append to `mod tests` in `src/builtin/special.rs`:

```rust
    #[test]
    fn readonly_double_dash_then_assignment_succeeds() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "foo=ok".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), Some("ok"));
        assert!(executor
            .env
            .vars
            .get_var("foo")
            .map(|v| v.readonly)
            .unwrap_or(false));
    }

    #[test]
    fn readonly_double_dash_alone_is_noop_rc0() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["--".to_string()], &mut executor);
        assert_eq!(status, 0);
    }

    #[test]
    fn readonly_double_dash_then_dash_p_is_invalid_identifier() {
        // After `--`, `-p` is treated as an operand and fails identifier validation.
        // The listing branch is suppressed because args[0] is "--", not "-p".
        // Note: readonly's listing condition uses `any(|a| a == "-p")`, so an
        // earlier `-p` would still trigger listing. We pin "-- then -p" → rc=1.
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("seen", "0").unwrap();
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        // Existing listing condition `any(|a| a == "-p")` makes this trigger
        // listing rc=0 (rather than identifier error). This is acknowledged
        // as a pre-existing behavior outside the scope of this fix; assert
        // the rc=0 listing outcome to lock down current behavior.
        assert_eq!(status, 0);
    }

    #[test]
    fn readonly_dash_p_alone_remains_listing() {
        // Regression guard: -p alone still triggers listing rc=0.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
    }
```

**Note on readonly_double_dash_then_dash_p**: the spec §3.3 chose to keep `readonly`'s listing condition (`args.iter().any(|a| a == "-p")`) unchanged. This means `readonly -- -p` still triggers listing because `-p` is in args, even after `--`. The test above documents this as known behavior. If the user later wants stricter handling (only `-p` *before* `--` triggers listing), that becomes a follow-up.

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib builtin::special::tests::readonly_double_dash 2>&1 | tail -15
```

Expected: `readonly_double_dash_then_assignment_succeeds` fails; `readonly_double_dash_alone_is_noop_rc0` fails.

- [ ] **Step 3: Apply helper in `builtin_readonly`**

Replace lines 173-216 of `src/builtin/special.rs` (the entire `fn builtin_readonly`) with:

```rust
fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.14.11: "When invoked with no arguments or with the -p
    // option, readonly shall write...". bash/dash treat -p as a listing
    // trigger that suppresses any operand processing.
    if args.is_empty() || args.iter().any(|a| a == "-p") {
        let readonly_vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .map(|(k, v)| (k.to_string(), v.value.clone()))
            .collect();
        let mut sorted = readonly_vars;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in sorted {
            println!("readonly {}={}", name, value);
        }
        return Ok(0);
    }

    let start = consume_end_of_options(args, 0);
    let mut status = 0;
    for arg in &args[start..] {
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
    Ok(status)
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib builtin::special::tests::readonly 2>&1 | tail -15
```

Expected: all `readonly_*` tests pass.

- [ ] **Step 5: Tighten the e2e test**

Overwrite `e2e/posix_spec/4_special_builtin/readonly_dash_dash.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly (XBD 12.2 Guideline 10)
# DESCRIPTION: readonly -- treats following operands as names; -- itself is consumed
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
readonly -- foo=ok || exit 99
echo "$foo"
```

```bash
chmod 644 e2e/posix_spec/4_special_builtin/readonly_dash_dash.sh
```

- [ ] **Step 6: Run e2e to verify pass**

```bash
./e2e/run_tests.sh --filter=readonly_dash_dash 2>&1 | tail -5
```

Expected: `[PASS]  posix_spec/4_special_builtin/readonly_dash_dash.sh`.

- [ ] **Step 7: Commit**

```bash
git add src/builtin/special.rs e2e/posix_spec/4_special_builtin/readonly_dash_dash.sh
git commit -m "$(cat <<'EOF'
fix(builtin): honor `--` as end-of-options in readonly

Mirrors the export fix: route `--` through consume_end_of_options before
operand validation. 4 unit tests cover -- + assignment, -- alone, -- + -p
(listing branch still triggered by any -p arg, documented as known
behavior), and -p alone (listing regression guard). The existing e2e
test was a false positive; tightened with `|| exit 99`.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Refactor `builtin_unset` to use the helper + unit tests + new e2e

**Files:**
- Modify: `src/builtin/special.rs::builtin_unset` (line 122-171, specifically the inline `--` branch around line 131-134)
- Modify: `src/builtin/special.rs` `mod tests` — 3 new tests
- Create: `e2e/posix_spec/4_special_builtin/unset_dash_dash.sh`

- [ ] **Step 1: Write unit tests for unset `--` behavior (pre-refactor regression guard)**

Append to `mod tests` in `src/builtin/special.rs`:

```rust
    #[test]
    fn unset_double_dash_unsets_following_name() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("foo", "v").unwrap();
        let status = exec_special_builtin(
            "unset",
            &["--".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), None);
    }

    #[test]
    fn unset_f_then_double_dash_unsets_function() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "--".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_v_then_double_dash_invalid_operand() {
        // After `-v --`, `-f` is an operand and must fail identifier validation.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-v".to_string(), "--".to_string(), "-f".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }
```

- [ ] **Step 2: Run tests to verify they pass (pre-refactor)**

```bash
cargo test --lib builtin::special::tests::unset 2>&1 | tail -15
```

Expected: all 3 new tests pass — unset already handles `--` inline. These tests serve as the regression guard for the refactor in Step 3.

- [ ] **Step 3: Refactor the inline `--` branch in `builtin_unset` to call the helper**

In `src/builtin/special.rs`, find the block at lines 131-134:

```rust
        if arg == "--" {
            idx += 1;
            break;
        }
```

Replace it with:

```rust
        if arg == "--" {
            idx = consume_end_of_options(args, idx);
            break;
        }
```

- [ ] **Step 4: Run tests to verify the refactor preserves behavior**

```bash
cargo test --lib builtin::special::tests::unset 2>&1 | tail -15
```

Expected: all 3 new tests + all 5 pre-existing unset tests pass.

- [ ] **Step 5: Add new e2e test for unset `--`**

Create `e2e/posix_spec/4_special_builtin/unset_dash_dash.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset (XBD 12.2 Guideline 10)
# DESCRIPTION: unset honors -- after flag parsing
# EXPECT_OUTPUT: empty
# EXPECT_EXIT: 0
m=set
unset -- m || exit 99
echo "${m-empty}"
```

```bash
chmod 644 e2e/posix_spec/4_special_builtin/unset_dash_dash.sh
```

- [ ] **Step 6: Run e2e to verify pass**

```bash
./e2e/run_tests.sh --filter=unset_dash_dash 2>&1 | tail -5
```

Expected: `[PASS]  posix_spec/4_special_builtin/unset_dash_dash.sh`.

- [ ] **Step 7: Commit**

```bash
git add src/builtin/special.rs e2e/posix_spec/4_special_builtin/unset_dash_dash.sh
git commit -m "$(cat <<'EOF'
refactor(builtin): route unset's `--` handling through shared helper

Replaces the inline `idx += 1; break;` in builtin_unset's flag-parse
loop with `idx = consume_end_of_options(args, idx); break;`. Pure
refactor — semantically identical, exercised by 3 new unit tests
(--name, -f -- name, -v -- -f) and a new e2e test unset_dash_dash.sh
to lock down the contract now that all three special builtins share
the same Guideline 10 routing.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Full regression + TODO cleanup

**Files:**
- Modify: `TODO.md` — delete the closed SP1 follow-up line (project convention: delete completed items, no `[x]` marker)

- [ ] **Step 1: Full unit + integration test run**

```bash
cargo build 2>&1 | tail -3
cargo test 2>&1 | tail -10
```

Expected: build succeeds, all tests pass. Note from memory: `cargo build` takes ~1-3 min; full `cargo test` is several minutes — be patient or run in background.

- [ ] **Step 2: Full e2e regression**

```bash
./e2e/run_tests.sh 2>&1 | tail -15
```

Expected: prior PASS / FAIL counts preserved (no new regressions). The 3 dash_dash tests should be PASS; no previously passing test should regress.

- [ ] **Step 3: Delete the SP1 follow-up item from TODO.md**

Find this line in `TODO.md`:

```
- [ ] `export -- foo=v` and `readonly -- foo=v` now report `--` as not a valid identifier (visible regression after SP1 G2's strict gate). Should consume `--` as POSIX end-of-options before validation (`src/builtin/special.rs::builtin_export`, `::builtin_readonly`).
```

Delete it entirely (do not mark with `[x]`; project convention is deletion).

- [ ] **Step 4: Commit the TODO cleanup**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): close SP1 export/readonly -- end-of-options follow-up

Resolved by the `consume_end_of_options` helper applied to export /
readonly and the unset refactor that shares the same routing. 11 unit
tests + 3 e2e tests lock down the behavior.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Confirm clean state**

```bash
git status
git log --oneline -7
```

Expected: working tree clean; 5 new commits (Task 1-5) on top of `602cd4f` (the spec commit).

---

## Self-Review Notes

**Spec coverage:**
- §3.1 helper → Task 1
- §3.2 export → Task 2
- §3.3 readonly → Task 3
- §3.4 unset refactor → Task 4
- §4 behavior matrix → 11 unit tests across Tasks 1-4
- §5.1 unit tests (11 total) → 5 (helper) + 4 (export) + 4 (readonly) + 3 (unset) = 16 actually, exceeds the spec's 11 by adding 5 helper tests. Acceptable — helper warrants direct coverage.
- §5.2 e2e tests → Tasks 2, 3, 4 steps 5-6
- §6 verification → Task 5

**Known nuance:** Task 3 Step 1 documents that `readonly -- -p` triggers listing (not identifier error) because `readonly`'s listing condition uses `any(|a| a == "-p")`. The spec §3.3 chose this intentionally to keep the listing condition unchanged. The test asserts the rc=0 listing outcome and the doc comment makes the trade-off explicit.

**Placeholders:** none. All code, all commands, all expected output specified.
