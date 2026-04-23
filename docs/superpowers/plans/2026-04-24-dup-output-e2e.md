# §2.7.6 DupOutput E2E Test Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 4 dedicated POSIX §2.7.6 (`>&`) E2E tests under `e2e/posix_spec/2_07_redirection/`, mirroring the existing §2.7.5 DupInput suite, and remove the corresponding TODO.md entry.

**Architecture:** Pure test-suite addition. No Rust code changes. Each new test is a POSIX shell script with standard metadata header (`POSIX_REF`, `DESCRIPTION`, `EXPECT_OUTPUT`, `EXPECT_EXIT`, `EXPECT_STDERR`) consumed by `e2e/run_tests.sh`. All 4 tests are independent and self-contained; they use `$TEST_TMPDIR` for filesystem state and `exec N> FILE` + `>&N` patterns that yosh already supports per `src/exec/redirect.rs:123-141`. File permissions must be `644` (CLAUDE.md rule).

**Tech Stack:** POSIX sh, `e2e/run_tests.sh` runner, yosh debug binary (`target/debug/yosh`).

**Spec reference:** `docs/superpowers/specs/2026-04-24-dup-output-e2e-design.md`

---

## File Structure

**Create (4 files):**
- `e2e/posix_spec/2_07_redirection/dup_output_basic.sh` — canonical `exec 3>FILE; echo >&3`
- `e2e/posix_spec/2_07_redirection/dup_output_param_expansion.sh` — `>&"$fd"` parameter-expanded fd
- `e2e/posix_spec/2_07_redirection/dup_output_bad_fd.sh` — unopened fd rejection
- `e2e/posix_spec/2_07_redirection/dup_output_close.sh` — `>&-` close + subsequent `>&3` rejection

**Modify (1 file):**
- `TODO.md` — remove the `§2.7.6 >& (DupOutput) dedicated E2E tests` bullet under `## Future: E2E Test Expansion`

**Precondition:**
- yosh debug build present (`target/debug/yosh` exists and is current). If missing or stale, run `cargo build` first.

---

## Task 1: Ensure yosh debug binary is current

**Files:** none (build artifact only)

- [ ] **Step 1: Build debug binary**

Run: `cargo build`
Expected: build completes; `target/debug/yosh` exists. First build can take 1–3 minutes — allow generous timeout. Incremental rebuild after spec commit should be seconds.

---

## Task 2: Add `dup_output_basic.sh`

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_output_basic.sh`

- [ ] **Step 1: Write the test file**

Write this exact content to `e2e/posix_spec/2_07_redirection/dup_output_basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N duplicates output fd N to fd 1 for the command
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_out"
exec 3> "$f"
echo hello >&3
exec 3>&-
cat "$f"
```

- [ ] **Step 2: Set file permissions to 644**

Run: `chmod 644 e2e/posix_spec/2_07_redirection/dup_output_basic.sh`
Expected: no output. Verify with `ls -l e2e/posix_spec/2_07_redirection/dup_output_basic.sh` showing `-rw-r--r--`.

- [ ] **Step 3: Run the test and verify it passes**

Run: `./e2e/run_tests.sh --filter=dup_output_basic`
Expected: `[PASS]` line for `dup_output_basic.sh`, summary shows `1 passed`, exit 0.

If the test fails, do NOT modify yosh source code. Re-read spec §Task 2 Step 1 and confirm file contents byte-for-byte (especially the `EXPECT_OUTPUT: hello` line — trailing whitespace breaks match).

---

## Task 3: Add `dup_output_param_expansion.sh`

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_output_param_expansion.sh`

- [ ] **Step 1: Write the test file**

Write this exact content to `e2e/posix_spec/2_07_redirection/dup_output_param_expansion.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&"$fd" accepts an fd number via parameter expansion
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_out_pe"
exec 3> "$f"
fd=3
echo hi >&"$fd"
exec 3>&-
cat "$f"
```

- [ ] **Step 2: Set file permissions to 644**

Run: `chmod 644 e2e/posix_spec/2_07_redirection/dup_output_param_expansion.sh`

- [ ] **Step 3: Run the test and verify it passes**

Run: `./e2e/run_tests.sh --filter=dup_output_param_expansion`
Expected: `[PASS]` for `dup_output_param_expansion.sh`, summary shows `1 passed`, exit 0.

---

## Task 4: Add `dup_output_bad_fd.sh`

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_output_bad_fd.sh`

- [ ] **Step 1: Write the test file**

Write this exact content to `e2e/posix_spec/2_07_redirection/dup_output_bad_fd.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N for an unopened fd N is a redirection error
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
echo hello >&9
```

- [ ] **Step 2: Set file permissions to 644**

Run: `chmod 644 e2e/posix_spec/2_07_redirection/dup_output_bad_fd.sh`

- [ ] **Step 3: Run the test and verify it passes**

Run: `./e2e/run_tests.sh --filter=dup_output_bad_fd`
Expected: `[PASS]` for `dup_output_bad_fd.sh`, summary shows `1 passed`, exit 0.

Note: this test asserts exit=1 and that stderr contains substring `yosh:` (the yosh error prefix convention per CLAUDE.md). If it fails with a different exit code, compare against how `dup_input_bad_fd.sh` behaves (`./e2e/run_tests.sh --filter=dup_input_bad_fd -v` if the runner supports verbose) — the DupOutput path should error symmetrically.

---

## Task 5: Add `dup_output_close.sh`

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_output_close.sh`

- [ ] **Step 1: Write the test file**

Write this exact content to `e2e/posix_spec/2_07_redirection/dup_output_close.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&- closes an output fd; subsequent >&N on the same fd fails
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
f="$TEST_TMPDIR/dup_out_close"
exec 3> "$f"
exec 3>&-
echo gone >&3
```

- [ ] **Step 2: Set file permissions to 644**

Run: `chmod 644 e2e/posix_spec/2_07_redirection/dup_output_close.sh`

- [ ] **Step 3: Run the test and verify it passes**

Run: `./e2e/run_tests.sh --filter=dup_output_close`
Expected: `[PASS]` for `dup_output_close.sh`, summary shows `1 passed`, exit 0.

---

## Task 6: Run full DupOutput filter to confirm suite cohesion

**Files:** none

- [ ] **Step 1: Run all 4 new tests together**

Run: `./e2e/run_tests.sh --filter=dup_output`
Expected: 4 `[PASS]` lines, summary `4 passed`, exit 0.

- [ ] **Step 2: Confirm DupInput suite still passes (symmetry check)**

Run: `./e2e/run_tests.sh --filter=dup_input`
Expected: 4 `[PASS]` lines for the existing dup_input_* tests, exit 0.

---

## Task 7: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the DupOutput bullet**

Locate this line in the `## Future: E2E Test Expansion` section:

```markdown
- [ ] §2.7.6 `>&` (DupOutput) dedicated E2E tests — analogous to the §2.7.5 suite in `e2e/posix_spec/2_07_redirection/dup_input_*.sh`. Current coverage via `e2e/redirection/stderr_to_stdout.sh` (legacy dir, no `POSIX_REF`) is incidental. Add `dup_output_basic`, `dup_output_param_expansion`, `dup_output_bad_fd`, `dup_output_close` mirroring the DupInput suite.
```

Delete the entire line (including the trailing newline). Do not replace with `[x]` — CLAUDE.md rule: "Delete completed items rather than marking them with `[x]`."

- [ ] **Step 2: Verify removal**

Run: `grep -n "DupOutput" TODO.md || echo "not found"`
Expected: `not found` (bullet is fully removed).

- [ ] **Step 3: Verify no other bullets were disturbed**

Run: `grep -c "^- \[ \]" TODO.md`
Expected: previous count minus 1. (Record the pre-edit count first if paranoid, or rely on `git diff TODO.md` showing a single deletion.)

---

## Task 8: Run full E2E suite for regression check

**Files:** none

- [ ] **Step 1: Run the full E2E suite**

Run: `./e2e/run_tests.sh`
Expected: all tests pass; summary count increases by exactly 4 vs. the pre-change baseline.

Per CLAUDE.md: PTY tests (under `tests/pty_interactive.rs`) may be flaky. This task runs only the shell-script E2E suite in `e2e/`, which has no PTY dependency — flakes should be zero. If a transient failure occurs outside the 4 new tests, re-run once; if it still fails, investigate before proceeding.

---

## Task 9: Commit all changes

**Files:** all 4 new tests + TODO.md

- [ ] **Step 1: Stage changes**

Run:
```bash
git add e2e/posix_spec/2_07_redirection/dup_output_basic.sh \
        e2e/posix_spec/2_07_redirection/dup_output_param_expansion.sh \
        e2e/posix_spec/2_07_redirection/dup_output_bad_fd.sh \
        e2e/posix_spec/2_07_redirection/dup_output_close.sh \
        TODO.md
```

- [ ] **Step 2: Verify staged diff**

Run: `git diff --cached --stat`
Expected: 5 files changed — 4 new test files (each ~6–11 lines), TODO.md (1 deletion).

- [ ] **Step 3: Create commit**

Run:
```bash
git commit -m "$(cat <<'EOF'
test(e2e): add §2.7.6 DupOutput dedicated test suite

Add 4 POSIX-annotated E2E tests mirroring the §2.7.5 DupInput suite:
basic, param_expansion, bad_fd, and close. Replaces the incidental
coverage from e2e/redirection/stderr_to_stdout.sh (legacy dir, no
POSIX_REF) with canonical §2.7.6 coverage under e2e/posix_spec/.

Closes TODO entry under "Future: E2E Test Expansion".

Spec: docs/superpowers/specs/2026-04-24-dup-output-e2e-design.md
Prompt: "TODO.md の中から優先度が高そうなものを1つ対応してください"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 4: Verify commit landed**

Run: `git log -1 --oneline`
Expected: the new commit is the tip of `main`.

Run: `git status`
Expected: `nothing to commit, working tree clean`.

---

## Done Criteria

- 4 new E2E tests pass individually and as a group (`--filter=dup_output` → 4 passed).
- Existing DupInput suite still passes (symmetry preserved).
- Full E2E suite passes with test count +4 vs. baseline.
- TODO.md DupOutput bullet is removed (not marked `[x]`).
- One commit on `main` containing exactly: 4 new tests + TODO.md deletion.
- All 4 test files have permission `644` (not `755`).
