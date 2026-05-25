# readonly `-p` listing-symmetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `readonly`'s `-p` listing trigger first-position-only (like `export`), so `readonly -- -p` rejects `-p` as a bad identifier (rc=1) instead of printing the variable list (rc=0).

**Architecture:** A one-line change to the listing condition in `builtin_readonly` (`args.iter().any(|a| a == "-p")` → `args[0] == "-p"`), making it structurally identical to `builtin_export`. Locked down by unit tests and a symmetric E2E pair; documented in the prior spec and TODO.

**Tech Stack:** Rust (yosh shell), `cargo test` for unit tests, `./e2e/run_tests.sh` for POSIX E2E tests.

Design spec: `docs/superpowers/specs/2026-05-25-readonly-dash-p-listing-symmetry-design.md`

---

### Task 1: readonly listing-trigger fix (TDD)

**Files:**
- Modify (tests): `src/builtin/special.rs` — tests module, the
  `readonly_double_dash_then_dash_p_triggers_listing` fn at lines
  1173-1187
- Modify (core): `src/builtin/special.rs:184-188` — `builtin_readonly`
  listing condition + leading comment

- [ ] **Step 1: Update the existing test and add two new tests (RED)**

In `src/builtin/special.rs`, replace the entire
`readonly_double_dash_then_dash_p_triggers_listing` test (lines
1173-1187, including its `#[test]` attribute and comment) with the
following three tests:

```rust
    #[test]
    fn readonly_double_dash_then_dash_p_is_invalid_identifier() {
        // `--` ends options (XBD §12.2 Guideline 10), so the trailing
        // `-p` is validated as an operand and rejected as a bad
        // identifier — mirrors
        // export_double_dash_then_dash_p_is_invalid_identifier.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_p_then_double_dash_remains_listing() {
        // Regression guard: `readonly -p --` triggers listing because
        // `args[0] == "-p"` matches first; helper is never reached.
        // Mirrors export_p_then_double_dash_remains_listing.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "--".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
    }

    #[test]
    fn readonly_operand_then_dash_p_is_invalid_identifier() {
        // `readonly foo -p`: `-p` is no longer matched anywhere in args,
        // so foo is set readonly and `-p` is rejected as a bad
        // identifier (rc=1). Symmetric with export's operand-then-option
        // handling.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["foo".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
        assert!(executor
            .env
            .vars
            .get_var("foo")
            .map(|v| v.readonly)
            .unwrap_or(false));
    }
```

- [ ] **Step 2: Run the tests to verify the new behavior fails (RED)**

Run: `cargo test --lib readonly_`
Expected (compiles; cargo build may take 1-3 min):
- `readonly_double_dash_then_dash_p_is_invalid_identifier` → **FAILED**
  (`assertion failed: left == right`, left `0`, right `1`)
- `readonly_operand_then_dash_p_is_invalid_identifier` → **FAILED**
  (left `0`, right `1`)
- `readonly_p_then_double_dash_remains_listing` → **ok** (passes both
  before and after the fix; it is a guard)
- other `readonly_*` unit tests → **ok**

- [ ] **Step 3: Apply the core fix (GREEN)**

In `src/builtin/special.rs`, change `builtin_readonly` (lines 184-188).
Replace:

```rust
fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.14.11: "When invoked with no arguments or with the -p
    // option, readonly shall write...". bash/dash treat -p as a listing
    // trigger that suppresses any operand processing.
    if args.is_empty() || args.iter().any(|a| a == "-p") {
```

with:

```rust
fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.14.11: "When invoked with no arguments or with the -p
    // option, readonly shall write...". Only `-p` in the first position
    // triggers listing; `-p` after operands or after `--` (end of
    // options, XBD §12.2 Guideline 10) is validated as a bad identifier.
    // Mirrors builtin_export.
    if args.is_empty() || args[0] == "-p" {
```

- [ ] **Step 4: Run the tests to verify they pass (GREEN)**

Run: `cargo test --lib readonly_`
Expected: all `readonly_*` unit tests **ok**, including the three from
Step 1.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/special.rs
git commit -m "fix(builtin): make readonly -p listing first-position-only like export

readonly's listing trigger used args.iter().any(|a| a == \"-p\"), so a
-p anywhere (including after --) printed the variable list. Change to
args[0] == \"-p\" to mirror export: readonly -- -p now rejects -p as a
bad identifier (rc=1), symmetric with export -- -p.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Symmetric E2E pair

**Files:**
- Create: `e2e/posix_spec/4_special_builtin/readonly_dash_dash_dash_p.sh`
- Create: `e2e/posix_spec/4_special_builtin/export_dash_dash_dash_p.sh`

- [ ] **Step 1: Create the readonly E2E test**

Write `e2e/posix_spec/4_special_builtin/readonly_dash_dash_dash_p.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly (XBD 12.2 Guideline 10)
# DESCRIPTION: readonly -- ends options; trailing -p is a bad identifier
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
readonly -- -p
```

- [ ] **Step 2: Create the export E2E test**

Write `e2e/posix_spec/4_special_builtin/export_dash_dash_dash_p.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export (XBD 12.2 Guideline 10)
# DESCRIPTION: export -- ends options; trailing -p is a bad identifier
# EXPECT_STDERR: export
# EXPECT_EXIT: 1
export -- -p
```

- [ ] **Step 3: Set 644 permissions on both files**

E2E test files must be 644, not 755 (per CLAUDE.md).

```bash
chmod 644 e2e/posix_spec/4_special_builtin/readonly_dash_dash_dash_p.sh \
          e2e/posix_spec/4_special_builtin/export_dash_dash_dash_p.sh
```

- [ ] **Step 4: Build the debug binary (E2E runner needs it)**

Run: `cargo build`
Expected: builds clean (1-3 min). The runner uses `target/debug/yosh`.

- [ ] **Step 5: Run the filtered E2E tests**

Run: `./e2e/run_tests.sh --filter=readonly`
Expected: `readonly_dash_dash_dash_p.sh` **PASS**; existing
`readonly_dash_dash.sh`, `readonly_p_listing.sh`,
`readonly_invalid_name.sh`, etc. stay **PASS**.

Run: `./e2e/run_tests.sh --filter=export`
Expected: `export_dash_dash_dash_p.sh` **PASS** (export already returns
rc=1 for this input); existing export tests stay **PASS**.

- [ ] **Step 6: Commit**

```bash
git add e2e/posix_spec/4_special_builtin/readonly_dash_dash_dash_p.sh \
        e2e/posix_spec/4_special_builtin/export_dash_dash_dash_p.sh
git commit -m "test(e2e): add symmetric readonly/export -- -p end-of-options tests

Locks down that '-- -p' treats -p as a bad-identifier operand (rc=1)
for both builtins at the POSIX-spec level.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Documentation (prior spec notes + TODO deletion)

**Files:**
- Modify: `docs/superpowers/specs/2026-05-25-export-readonly-double-dash-design.md`
  (§3.3 and the §4 matrix)
- Modify: `TODO.md` (delete the resolved SP1 follow-up item)

- [ ] **Step 1: Annotate §3.3 in the prior spec**

In `docs/superpowers/specs/2026-05-25-export-readonly-double-dash-design.md`,
find the end of §3.3 (the line `bash). No behavior change for valid inputs.`)
and insert this note immediately after it:

Replace:

```
bash). No behavior change for valid inputs.

### 3.4 `builtin_unset` changes
```

with:

```
bash). No behavior change for valid inputs.

> **Update (2026-05-25 follow-up):** This deferral is resolved by
> `2026-05-25-readonly-dash-p-listing-symmetry-design.md`. The listing
> condition was changed to `args[0] == "-p"` (first-position only, like
> export), so `readonly -- -p` now yields rc=1 (bad identifier) and is
> symmetric with `export -- -p`.

### 3.4 `builtin_unset` changes
```

- [ ] **Step 2: Annotate the §4 matrix in the prior spec**

In the same file, find the line for the `unset -v -- -f` matrix row
(the last row of the §4 table) and insert this note immediately after
the table (before `## 5. Tests`):

Replace:

```
| `unset -v -- -f` | rc=1, `-f` identifier error | already works | works |

## 5. Tests
```

with:

```
| `unset -v -- -f` | rc=1, `-f` identifier error | already works | works |

> **Update (2026-05-25 follow-up):** the `readonly -- -p` row's
> "Post-fix yosh = listing rc=0" reflects this spec's scope only. The
> asymmetry was later resolved in
> `2026-05-25-readonly-dash-p-listing-symmetry-design.md`; current
> behavior is `-p` identifier error rc=1.

## 5. Tests
```

- [ ] **Step 3: Delete the resolved TODO item**

In `TODO.md`, delete the entire SP1 follow-up bullet describing the
`readonly -- -p` asymmetry. Remove this line in full:

```
- [ ] `readonly -- -p` triggers listing (rc=0) instead of treating `-p` as a bad-identifier operand. Asymmetric with `export -- -p` (rc=1) because `readonly`'s listing condition is `args.iter().any(|a| a == "-p")` — any `-p` in args wins regardless of `--`. Spec `docs/superpowers/specs/2026-05-25-export-readonly-double-dash-design.md` §3.3 + §4 documents this as a known deviation; the test `readonly_double_dash_then_dash_p_triggers_listing` locks down the current behavior. Stricter fix: change listing to `args[0] == "-p"` (first-position only) like export, or route `-p` after `--` through the operand validator. Final-review follow-up from 2026-05-25 export/readonly `--` end-of-options branch.
```

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-05-25-export-readonly-double-dash-design.md TODO.md
git commit -m "docs: note readonly -- -p deviation resolved; drop TODO item

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Verification (after all tasks)

1. `cargo test --lib readonly_` — all readonly unit tests pass.
2. `cargo test special` — no regressions in the special-builtin module.
3. `./e2e/run_tests.sh --filter=readonly` and
   `./e2e/run_tests.sh --filter=export` — the new pair passes; siblings
   stay green.
