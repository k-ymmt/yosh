# POSIX Chapter 2 Gaps — Sub-project 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add E2E coverage for POSIX §2.7.5 (`<&`), §2.7.7 (`<>`), and §2.14.13 (`times`); correct the stale "times not implemented" note in TODO.md.

**Architecture:** 11 new `.sh` files under two new directories (`e2e/posix_spec/2_07_redirection/` and `e2e/posix_spec/2_14_13_times/`). No `src/` changes. Harness (`e2e/run_tests.sh`) already supports everything we need — `EXPECT_STDERR` is substring match, `$TEST_TMPDIR` is provided and auto-cleaned.

**Tech Stack:** POSIX `/bin/sh` test scripts, yosh debug binary (`target/debug/yosh`), existing `e2e/run_tests.sh` harness.

**Spec:** `docs/superpowers/specs/2026-04-19-posix-ch2-gaps-subproject1-design.md`

---

## Prerequisites (one-time, before Task 1)

- [ ] **Step 0.1: Build the debug binary**

Run:
```bash
cargo build
```
Expected: builds cleanly. The E2E harness hard-codes `./target/debug/yosh`.

- [ ] **Step 0.2: Record baseline E2E status**

Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: a summary line like `Total: N  Passed: P  Failed: F  ...`. Remember `Failed` — it must not increase after this sub-project.

---

## Task 1: `times` builtin E2E tests (§2.14.13)

**Files:**
- Create: `e2e/posix_spec/2_14_13_times/times_exit_code.sh` (mode 644)
- Create: `e2e/posix_spec/2_14_13_times/times_format.sh` (mode 644)
- Create: `e2e/posix_spec/2_14_13_times/times_in_subshell.sh` (mode 644)

### Step 1.1: Write `times_exit_code.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times returns exit status 0 on success
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
times >/dev/null
echo $?
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_14_13_times/times_exit_code.sh
```

### Step 1.2: Write `times_format.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times prints two lines in "NmS.sssS NmS.sssS" shape
# EXPECT_OUTPUT omitted: CPU-time values are non-deterministic; shape verified in-script.
# EXPECT_EXIT: 0
out=$(times)
line1=$(echo "$out" | sed -n '1p')
line2=$(echo "$out" | sed -n '2p')
case "$line1" in
    *m*s\ *m*s) ;;
    *) echo "bad line1: $line1" >&2; exit 1 ;;
esac
case "$line2" in
    *m*s\ *m*s) ;;
    *) echo "bad line2: $line2" >&2; exit 1 ;;
esac
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_14_13_times/times_format.sh
```

### Step 1.3: Write `times_in_subshell.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times works inside a subshell; output retains two-line shape
# EXPECT_OUTPUT omitted: CPU-time values are non-deterministic; shape verified in-script.
# EXPECT_EXIT: 0
out=$( ( times ) )
line1=$(echo "$out" | sed -n '1p')
line2=$(echo "$out" | sed -n '2p')
case "$line1" in
    *m*s\ *m*s) ;;
    *) echo "bad line1 in subshell: $line1" >&2; exit 1 ;;
esac
case "$line2" in
    *m*s\ *m*s) ;;
    *) echo "bad line2 in subshell: $line2" >&2; exit 1 ;;
esac
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_14_13_times/times_in_subshell.sh
```

### Step 1.4: Run the three tests

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=2_14_13_times
```
Expected summary:
```
Total: 3  Passed: 3  Failed: 0  Timedout: 0  XFail: 0  XPass: 0
```

If any file fails: read the failure reason, add `# XFAIL: <specific technical reason>` to that file's header (after `# DESCRIPTION:`, before `# EXPECT_EXIT:`). Re-run; the count should be `Passed: N  Failed: 0  XFail: M` with `N + M = 3`.

### Step 1.5: Update TODO.md — remove stale `times` entry

- [ ] Open `TODO.md`. Under "Future: POSIX Conformance Gaps (Chapter 2)", delete the line:

```
- [ ] §2.14.13 times builtin not implemented
```

(The `times` builtin has been implemented at `src/builtin/special.rs:466` for some time; the TODO entry was incorrect.)

### Step 1.6: Commit

- [ ] Run:
```bash
git add e2e/posix_spec/2_14_13_times/ TODO.md
git commit -m "$(cat <<'EOF'
test(times): add §2.14.13 E2E coverage and correct stale TODO

Adds three tests: exit_code, format (shape-only, CPU times are
non-deterministic), and in-subshell. TODO.md previously claimed
§2.14.13 times was "not implemented"; the builtin has existed at
src/builtin/special.rs:466 — drop the stale entry.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds, working tree clean for these paths.

---

## Task 2: `<&` (DupInput) E2E tests (§2.7.5)

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_input_basic.sh` (mode 644)
- Create: `e2e/posix_spec/2_07_redirection/dup_input_param_expansion.sh` (mode 644)
- Create: `e2e/posix_spec/2_07_redirection/dup_input_bad_fd.sh` (mode 644)
- Create: `e2e/posix_spec/2_07_redirection/dup_input_close.sh` (mode 644)

### Step 2.1: Write `dup_input_basic.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N duplicates input fd N to fd 0 for the command
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_in"
echo hello > "$f"
exec 3< "$f"
cat <&3
exec 3<&-
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_input_basic.sh
```

### Step 2.2: Write `dup_input_param_expansion.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&"$fd" accepts an fd number via parameter expansion
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_in_pe"
echo hi > "$f"
exec 3< "$f"
fd=3
cat <&"$fd"
exec 3<&-
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_input_param_expansion.sh
```

### Step 2.3: Write `dup_input_bad_fd.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N for an unopened fd N is a redirection error
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
cat <&9
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_input_bad_fd.sh
```

Note: `EXPECT_STDERR` is substring match (verified in `e2e/run_tests.sh:248`), so `yosh:` matches any stderr containing the shell's error prefix. Expected exit `1` matches POSIX redirection-error convention; if yosh emits a different non-zero code, update this value to match (and note the choice in the description).

### Step 2.4: Write `dup_input_close.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&- closes an input fd; subsequent <&N on the same fd fails
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
f="$TEST_TMPDIR/dup_in_close"
echo gone > "$f"
exec 3< "$f"
exec 3<&-
cat <&3
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_input_close.sh
```

### Step 2.5: Run the four tests

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=dup_input
```
Expected: `Total: 4  Passed: 4  Failed: 0`.

For any failure: `# XFAIL: <specific technical reason — what yosh did and why it diverges from POSIX>`. Re-run until `Failed: 0`.

### Step 2.6: Commit

- [ ] Run:
```bash
git add e2e/posix_spec/2_07_redirection/dup_input_*.sh
git commit -m "$(cat <<'EOF'
test(redir): add §2.7.5 <& (DupInput) E2E coverage

Four tests covering basic duplication, fd via parameter expansion,
unopened-fd error, and fd-close semantics. Uses $TEST_TMPDIR (harness-
provided, auto-cleaned) rather than /tmp/yosh_$$.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `<>` (ReadWrite) E2E tests (§2.7.7)

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/readwrite_basic.sh` (mode 644)
- Create: `e2e/posix_spec/2_07_redirection/readwrite_creates_file.sh` (mode 644)
- Create: `e2e/posix_spec/2_07_redirection/readwrite_param_expansion.sh` (mode 644)
- Create: `e2e/posix_spec/2_07_redirection/readwrite_bidirectional.sh` (mode 644)

### Step 3.1: Write `readwrite_basic.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file opens file for read+write on fd N; written data is readable afterwards
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_basic"
echo hi 1<>"$f"
cat "$f"
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/readwrite_basic.sh
```

### Step 3.2: Write `readwrite_creates_file.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file creates the file if it does not exist
# EXPECT_OUTPUT: created
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_creates"
# File must not exist yet
[ ! -e "$f" ] || { echo "precondition: $f already exists" >&2; exit 1; }
: 1<>"$f"
# After <> the file should exist
if [ -e "$f" ]; then
    echo created
else
    echo "not created" >&2
    exit 1
fi
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/readwrite_creates_file.sh
```

Note: `: 1<>"$f"` uses the null command with a read-write redirect, which opens the file (creating it if absent) and closes it immediately — exactly what POSIX §2.7.7 specifies.

### Step 3.3: Write `readwrite_param_expansion.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>"$file" accepts a filename via parameter expansion
# EXPECT_OUTPUT: roundtrip
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_pe"
echo roundtrip 1<>"$f"
cat "$f"
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/readwrite_param_expansion.sh
```

### Step 3.4: Write `readwrite_bidirectional.sh`

- [ ] Create file with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file accepts both read and write redirects on the same fd
# EXPECT_OUTPUT omitted: POSIX does not specify read-pointer position after write; only that opening with <> succeeds.
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_bidir"
# Seed file with known content
echo seed > "$f"
# Open fd 3 read-write on the same file — no error expected
exec 3<>"$f"
exec 3<&-
```

Set mode 644:
```bash
chmod 644 e2e/posix_spec/2_07_redirection/readwrite_bidirectional.sh
```

### Step 3.5: Run the four tests

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=readwrite
```
Expected: `Total: 4  Passed: 4  Failed: 0`.

For any failure: XFAIL with specific reason.

### Step 3.6: Commit

- [ ] Run:
```bash
git add e2e/posix_spec/2_07_redirection/readwrite_*.sh
git commit -m "$(cat <<'EOF'
test(redir): add §2.7.7 <> (ReadWrite) E2E coverage

Four tests covering basic round-trip, file creation on open, filename
via parameter expansion, and fd-level bidirectional open (shape-only —
POSIX does not specify read-pointer position after write).

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Consolidation and TODO.md cleanup

### Step 4.1: Full E2E regression check

- [ ] Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: `Failed` count matches baseline from Step 0.2; total is baseline + 11 (minus any XFAIL'd files, which still count in Total).

If `Failed` > baseline: find the newly-failing test (use the per-file output above the summary), either XFAIL it or fix the test script. The implementation in `src/` is not to be changed in this sub-project.

### Step 4.2: Rust regression check

- [ ] Run:
```bash
cargo test
```
Expected: clean pass. No `src/` changes, so this is a sanity check only.

### Step 4.3: Update TODO.md — remove the three addressed items

- [ ] Open `TODO.md`. Under "Future: POSIX Conformance Gaps (Chapter 2)", delete the two remaining lines that Tasks 2 and 3 have now fully covered:

```
- [ ] §2.7.5 Duplicating an Input File Descriptor — no dedicated test; add when FD dup tests are expanded
- [ ] §2.7.7 Open File Descriptors for Reading and Writing — no dedicated '<>' test
```

(The §2.14.13 line was already removed in Task 1, Step 1.5.)

If any test was XFAIL'd in Tasks 2 or 3, do NOT remove that section's TODO line. Instead, rewrite it with a specific technical reason naming the file(s) and the divergence from POSIX. Example rewrite if `dup_input_close.sh` was XFAIL'd:

```
- [ ] §2.7.5 `<&-` close semantics — `e2e/posix_spec/2_07_redirection/dup_input_close.sh` XFAIL; <concise description of the divergence>
```

### Step 4.4: Commit TODO.md

- [ ] Run:
```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): close §2.7.5, §2.7.7, §2.14.13 gap items

All three addressed items are now covered by E2E tests under
e2e/posix_spec/{2_07_redirection,2_14_13_times}/. Any tests that
uncovered genuine implementation gaps remain on the list with
concrete XFAIL references.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

### Step 4.5: Final verification

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=2_07_redirection 2>&1 | tail -2
./e2e/run_tests.sh --filter=2_14_13_times 2>&1 | tail -2
git status
```
Expected:
- First command: `Total: 8  Passed: ≥0  Failed: 0  ...  XFail: ≥0` (sum of Passed + XFail = 8).
- Second command: `Total: 3  Passed: ≥0  Failed: 0  ...  XFail: ≥0` (sum = 3).
- `git status`: working tree clean.

If any `Failed` is non-zero, return to the relevant task and XFAIL.

---

## Success Criteria (restated from spec)

- `Failed: 0` on both filtered runs and the full run.
- 11 new files under `e2e/posix_spec/2_07_redirection/` (8) and `e2e/posix_spec/2_14_13_times/` (3), all mode 644, all with `#!/bin/sh` + `POSIX_REF` + `DESCRIPTION` headers.
- `cargo test` remains clean.
- TODO.md's "Future: POSIX Conformance Gaps (Chapter 2)" no longer lists §2.7.5, §2.7.7, §2.14.13 as pure gaps (XFAIL items may remain with concrete reasons).
- 4 commits (one per Task).

## Notes for the executor

- **Do NOT modify `src/`.** If a test reveals an implementation gap, XFAIL the test with a specific reason and leave the implementation work for a later sub-project.
- **Do NOT migrate `e2e/redirection/*`.** That reorganization belongs to a different sub-project.
- **Use `$TEST_TMPDIR`** for all temporary files. The harness creates it per test and removes it automatically (`e2e/run_tests.sh:165, 308`).
- **Mode 644** is a project convention (CLAUDE.md). `chmod 644` after creating each file. Do not use `755`.
- **Exit-code expectation for errors**: yosh's redirection errors typically exit `1`. If any test shows a different code in its failure output, change `EXPECT_EXIT` to match what yosh actually produces — the goal of the error tests is to pin current behavior, not prescribe a POSIX-exact value (POSIX permits any non-zero for redirection errors).
