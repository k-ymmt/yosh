# cargo test Parallelization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parallelize per-test-binary execution in `release.sh phase_test` to cut release-pipeline wall time from ~1720 s to ≤ 900 s.

**Architecture:** Replace the single `cargo test` invocation (which runs 24 integration-test binaries serially) with a bash-level parallel runner. Pre-build all test binaries via `cargo test --no-run --workspace`, then launch one `cargo test --test <name>` per binary concurrently. Serialize only `pty_interactive` via a `mkdir`-based atomic lock (portable across Linux + macOS, unlike `flock(1)`). Collect per-job logs in a temp dir; on any failure, print only the failed jobs' logs.

**Tech Stack:** bash 3.2+ (macOS default `/bin/bash`), cargo, `mktemp`, `mkdir`/`rmdir`, POSIX `trap`.

**Spec:** `docs/superpowers/specs/2026-04-23-cargo-test-parallelization-design.md`

**TDD adaptation note:** No bash unit-test framework exists in this repo. Validation is empirical — `./release.sh test --dry-run` for plumbing, `time ./release.sh test` for timing, injected failure for error paths. Each task ends with a commit.

---

## Task 1: Add the JOBS array and helper function stubs

Introduce the job list and the two new helpers (`_run_test_job`, `_run_all_tests_parallel`). Keep them unused for now so this task is purely additive; syntax only.

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh:78-118` (will prepend helpers above `phase_test`; do not call them yet)

- [ ] **Step 1: Add JOBS array constant and helper stubs above `phase_test`**

Insert the following block at `.claude/skills/release/scripts/release.sh` immediately before the existing `phase_test()` definition (currently line 78):

```bash
# Job list for parallel test execution. Format: "name|group|cargo-args..."
# group: "pty" = serialized via PTY lock, "free" = unbounded parallel.
# Edit this list when adding/removing test binaries or workspace crates.
PHASE_TEST_JOBS=(
  "lib|free|test --lib -p yosh"
  "doc|free|test --doc -p yosh"
  "plugin-api|free|test -p yosh-plugin-api"
  "plugin-sdk|free|test -p yosh-plugin-sdk"
  "plugin-manager|free|test -p yosh-plugin-manager"
  "cli_help|free|test --test cli_help"
  "errexit|free|test --test errexit"
  "history|free|test --test history"
  "ignored_on_entry|free|test --test ignored_on_entry"
  "interactive|free|test --test interactive"
  "parser_integration|free|test --test parser_integration"
  "plugin|free|test --test plugin"
  "plugin_cli_help|free|test --test plugin_cli_help"
  "signals|free|test --test signals"
  "subshell|free|test --test subshell"
  "pty_interactive|pty|test --test pty_interactive"
)

# Set by phase_test at invocation time. Absent path = unlocked, present = held.
PTY_LOCK_DIR=""

# Run one test job. Locks the PTY group via mkdir. Writes output to $log.
# Args: $1=name  $2=group  $3=log  $4..=cargo args
_run_test_job() {
  local name="$1" group="$2" log="$3"
  shift 3

  if [[ "$group" == "pty" ]]; then
    while ! mkdir "$PTY_LOCK_DIR" 2>/dev/null; do sleep 0.05; done
    trap 'rmdir "$PTY_LOCK_DIR" 2>/dev/null' EXIT
  fi

  cargo "$@" >"$log" 2>&1
}

# Launch all jobs in PHASE_TEST_JOBS plus e2e in parallel, wait, aggregate.
# Prints only failed jobs' logs; fails the script with a summary on any failure.
_run_all_tests_parallel() {
  local log_dir
  log_dir="$(mktemp -d -t yosh-parallel-tests.XXXXXX)"
  trap 'rm -rf "$log_dir"; rmdir "$PTY_LOCK_DIR" 2>/dev/null' EXIT INT TERM

  local -a pids names logs
  local idx=0 job name group cmd log

  for job in "${PHASE_TEST_JOBS[@]}"; do
    IFS='|' read -r name group cmd <<< "$job"
    log="$log_dir/$name.log"
    ( _run_test_job "$name" "$group" "$log" $cmd ) &
    pids[$idx]=$!
    names[$idx]="$name"
    logs[$idx]="$log"
    idx=$((idx+1))
  done

  # e2e as an additional parallel job alongside the cargo jobs.
  local e2e_log="$log_dir/e2e.log"
  ( ./e2e/run_tests.sh >"$e2e_log" 2>&1 ) &
  pids[$idx]=$!
  names[$idx]="e2e"
  logs[$idx]="$e2e_log"

  local -a failed
  local i
  for i in "${!pids[@]}"; do
    if ! wait "${pids[$i]}"; then
      failed+=("$i")
    fi
  done

  if [[ ${#failed[@]} -gt 0 ]]; then
    for i in "${failed[@]}"; do
      echo "--- ${names[$i]} output ---" >&2
      cat "${logs[$i]}" >&2
    done
    local -a failed_names
    for i in "${failed[@]}"; do failed_names+=("${names[$i]}"); done
    fail "tests failed: ${failed_names[*]} — fix and rerun"
  fi
}

```

- [ ] **Step 2: Syntax-check the modified script**

Run: `bash -n .claude/skills/release/scripts/release.sh`
Expected: exit code 0, no output.

- [ ] **Step 3: Verify existing `phase_test` behavior is untouched**

Run: `bash -c 'source .claude/skills/release/scripts/release.sh; declare -f phase_test | head -5'`
Expected: output shows the original `phase_test` function body, starting with `phase_test ()` and the existing `cargo build` call. Confirms the new code is additive only.

- [ ] **Step 4: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "$(cat <<'EOF'
perf(release): add parallel test runner helpers (unused)

Introduces PHASE_TEST_JOBS array and _run_test_job /
_run_all_tests_parallel helpers as pure additions. phase_test still
calls the legacy single 'cargo test' path; next task wires the helpers
in behind a --dry-run flag for plumbing verification.

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 2: Add `--dry-run` flag and exercise the job list

Wire the `--dry-run` entry point at the top of `phase_test`. This exercises `PHASE_TEST_JOBS` without running any tests — fast feedback that the list is well-formed before doing heavier integration in Task 3.

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh` — `phase_test` function (currently starts at line 78; now below the helpers from Task 1)

- [ ] **Step 1: Replace the first line of `phase_test` to accept `--dry-run`**

Find this line at the start of `phase_test`:

```bash
phase_test() {
  echo "yosh-release: building debug binary for e2e..." >&2
```

Replace with:

```bash
phase_test() {
  local dry_run=0
  if [[ "${1:-}" == "--dry-run" ]]; then
    dry_run=1
    shift
  fi

  if [[ $dry_run -eq 1 ]]; then
    echo "yosh-release: dry-run — ${#PHASE_TEST_JOBS[@]} jobs + e2e would run:" >&2
    local job
    for job in "${PHASE_TEST_JOBS[@]}"; do
      echo "  $job" >&2
    done
    echo "  e2e|-|./e2e/run_tests.sh" >&2
    return 0
  fi

  echo "yosh-release: building debug binary for e2e..." >&2
```

- [ ] **Step 2: Syntax-check**

Run: `bash -n .claude/skills/release/scripts/release.sh`
Expected: exit code 0, no output.

- [ ] **Step 3: Exercise the dry-run path**

Run: `./.claude/skills/release/scripts/release.sh test --dry-run`
Expected output on stderr (exit 0):

```
yosh-release: dry-run — 16 jobs + e2e would run:
  lib|free|test --lib -p yosh
  doc|free|test --doc -p yosh
  plugin-api|free|test -p yosh-plugin-api
  plugin-sdk|free|test -p yosh-plugin-sdk
  plugin-manager|free|test -p yosh-plugin-manager
  cli_help|free|test --test cli_help
  errexit|free|test --test errexit
  history|free|test --test history
  ignored_on_entry|free|test --test ignored_on_entry
  interactive|free|test --test interactive
  parser_integration|free|test --test parser_integration
  plugin|free|test --test plugin
  plugin_cli_help|free|test --test plugin_cli_help
  signals|free|test --test signals
  subshell|free|test --test subshell
  pty_interactive|pty|test --test pty_interactive
  e2e|-|./e2e/run_tests.sh
```

Verify: exactly 16 jobs (check count in first line), exactly one `pty` group (`pty_interactive`), all 11 integration test binaries present, all 3 workspace crates present (`plugin-api`, `plugin-sdk`, `plugin-manager`), `lib` + `doc` present. If any job is missing or duplicated, STOP and fix the `PHASE_TEST_JOBS` array before proceeding.

- [ ] **Step 4: Verify non-dry-run path is still the legacy code**

Run: `grep -n 'echo "yosh-release: running cargo test and e2e tests in parallel..."' .claude/skills/release/scripts/release.sh`
Expected: one match inside the old `phase_test` body. Confirms this task did not accidentally remove the legacy path (that happens in Task 3).

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "$(cat <<'EOF'
perf(release): add --dry-run to release.sh test

Exercises PHASE_TEST_JOBS without running cargo. Confirms the job list
is well-formed before wiring the parallel runner into phase_test.

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 3: Replace legacy phase_test body with the parallel runner

Swap the existing single-`cargo test` + single-`./e2e/run_tests.sh` body for the pre-build + `_run_all_tests_parallel` flow. After this task, the old inline code is gone and all tests run via the new path.

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh` — `phase_test` function body

- [ ] **Step 1: Replace the post-dry-run body of `phase_test`**

In `phase_test`, locate the block starting with `echo "yosh-release: building debug binary for e2e..." >&2` (comes immediately after the dry-run early-return added in Task 2) and extending through the closing `}` of the function. The whole block currently looks like:

```bash
  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"

  local cargo_log e2e_log
  cargo_log="$(mktemp -t yosh-cargo-test.XXXXXX)"
  e2e_log="$(mktemp -t yosh-e2e.XXXXXX)"

  echo "yosh-release: running cargo test and e2e tests in parallel..." >&2
  echo "yosh-release: cargo test output is buffered (shown only on failure); this can take 20-30 min" >&2
  cargo test >"$cargo_log" 2>&1 &
  local cargo_pid=$!
  ./e2e/run_tests.sh >"$e2e_log" 2>&1 &
  local e2e_pid=$!

  local cargo_rc=0 e2e_rc=0
  wait "$cargo_pid" || cargo_rc=$?
  wait "$e2e_pid"   || e2e_rc=$?

  if [[ $cargo_rc -ne 0 || $e2e_rc -ne 0 ]]; then
    if [[ $cargo_rc -ne 0 ]]; then
      echo "--- cargo test output ---" >&2
      cat "$cargo_log" >&2
    fi
    if [[ $e2e_rc -ne 0 ]]; then
      echo "--- e2e output ---" >&2
      cat "$e2e_log" >&2
    fi
    rm -f "$cargo_log" "$e2e_log"
    if [[ $cargo_rc -ne 0 && $e2e_rc -ne 0 ]]; then
      fail "cargo test AND e2e tests failed — fix both and rerun"
    elif [[ $cargo_rc -ne 0 ]]; then
      fail "cargo test failed — fix tests and rerun"
    else
      fail "e2e tests failed — fix tests and rerun"
    fi
  fi

  rm -f "$cargo_log" "$e2e_log"
  echo "yosh-release: all tests passed" >&2
}
```

Replace that entire block with:

```bash
  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"

  echo "yosh-release: pre-compiling test binaries..." >&2
  cargo test --no-run --workspace \
    || fail "cargo test --no-run failed — fix and rerun"

  # Reserve a unique lock path. mktemp -d creates it; rmdir removes it so the
  # path is absent on entry. Absent = unlocked, present = held.
  PTY_LOCK_DIR="$(mktemp -d -t yosh-pty-lock.XXXXXX)"
  rmdir "$PTY_LOCK_DIR"

  echo "yosh-release: running ${#PHASE_TEST_JOBS[@]} test jobs + e2e in parallel..." >&2
  echo "yosh-release: output is buffered (shown only on failure); this can take 15-30 min" >&2
  _run_all_tests_parallel

  echo "yosh-release: all tests passed" >&2
}
```

- [ ] **Step 2: Syntax-check**

Run: `bash -n .claude/skills/release/scripts/release.sh`
Expected: exit code 0, no output.

- [ ] **Step 3: Re-exercise dry-run to confirm no regression in Task 2 behavior**

Run: `./.claude/skills/release/scripts/release.sh test --dry-run`
Expected: same output as Task 2 Step 3 (16 jobs + e2e listed). If the count changes or the dry-run branch breaks, STOP and inspect the edit.

- [ ] **Step 4: Verify the legacy inline code is gone**

Run: `grep -cE 'cargo_log|e2e_log|yosh-cargo-test|yosh-e2e' .claude/skills/release/scripts/release.sh`
Expected: `0`

If non-zero, the legacy code was not fully removed. Inspect and fix before continuing.

- [ ] **Step 5: End-to-end smoke — run a single fast job via targeted invocation**

Rather than running the full 20–30 min pipeline at this stage, manually invoke the smallest job to confirm the path works:

Run: `cargo test --no-run --workspace` (warm up target/)
Expected: completes without error.

Run: `bash -c 'source .claude/skills/release/scripts/release.sh; PHASE_TEST_JOBS=("history|free|test --test history"); PTY_LOCK_DIR="$(mktemp -d -t t.XXXXXX)"; rmdir "$PTY_LOCK_DIR"; _run_all_tests_parallel'`
Expected: exits 0 with no stderr/stdout beyond what `e2e/run_tests.sh` prints. `history` test binary is very small (1 test) so this completes in seconds.

Note: this smoke skips e2e in isolation of `_run_all_tests_parallel`'s dependency on `./e2e/run_tests.sh` — the function launches e2e unconditionally. If e2e fails because the debug binary isn't up to date, run `cargo build` first.

- [ ] **Step 6: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "$(cat <<'EOF'
perf(release): wire parallel runner into phase_test

Replaces the single-cargo-test + single-e2e inline body with:
  cargo build
  cargo test --no-run --workspace  (pre-build, avoids target/ contention)
  _run_all_tests_parallel          (16 jobs + e2e in parallel)

PTY exclusion lock is set up but not yet exercised (pty_interactive
is the only pty-group job and doesn't contend with itself). Added in
the next task verification, where mkdir-lock semantics are tested.

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 4: Verify PTY lock behavior under contention

`_run_test_job` already contains the `mkdir` lock logic (added in Task 1). This task verifies it under a synthetic contention scenario since the normal run has only one `pty` job. No code changes; verification only.

**Files:**
- None (verification-only task)

- [ ] **Step 1: Run a contention scenario with 3 fake pty jobs**

Run:

```bash
bash -c '
set -euo pipefail
source .claude/skills/release/scripts/release.sh
PTY_LOCK_DIR="$(mktemp -d -t lock.XXXXXX)"
rmdir "$PTY_LOCK_DIR"
log=$(mktemp -d -t pty-test.XXXXXX)

# Three fake pty-group jobs that each sleep 1s while holding the lock.
# They should run sequentially, total wall time ~3s+.
fake_pty_job() {
  local n=$1
  while ! mkdir "$PTY_LOCK_DIR" 2>/dev/null; do sleep 0.05; done
  echo "pty job $n: acquired at $(date +%s.%N)" >>"$log/trace"
  sleep 1
  echo "pty job $n: releasing at $(date +%s.%N)" >>"$log/trace"
  rmdir "$PTY_LOCK_DIR"
}

t0=$(date +%s)
fake_pty_job 1 &
fake_pty_job 2 &
fake_pty_job 3 &
wait
t1=$(date +%s)
elapsed=$((t1 - t0))

cat "$log/trace"
echo "elapsed: ${elapsed}s"
[[ $elapsed -ge 3 ]] || { echo "FAIL: elapsed $elapsed < 3s — lock not serializing"; exit 1; }
[[ $elapsed -le 5 ]] || { echo "FAIL: elapsed $elapsed > 5s — excessive contention overhead"; exit 1; }
echo "OK: 3 contending jobs took ${elapsed}s (expected ~3s)"
rm -rf "$log"
'
```

Expected: output shows 3 "acquired"/"releasing" pairs where each "releasing" precedes the next "acquired" (no interleaving), and final line `OK: 3 contending jobs took 3s`.

If jobs overlap (an "acquired" line appears before the previous "releasing"), the lock is broken — STOP and investigate.

- [ ] **Step 2: Verify stale lock survives killed worker**

Run:

```bash
bash -c '
source .claude/skills/release/scripts/release.sh
PTY_LOCK_DIR="$(mktemp -d -t lock.XXXXXX)"
rmdir "$PTY_LOCK_DIR"

# Simulate a crashed job: acquire lock, never release, check next acquirer blocks.
mkdir "$PTY_LOCK_DIR"
echo "lock held"

# Next would-be acquirer should time out quickly (we manually clean up after 2s).
( sleep 2; rmdir "$PTY_LOCK_DIR" 2>/dev/null; echo "external cleanup done" ) &
cleaner_pid=$!

t0=$(date +%s)
while ! mkdir "$PTY_LOCK_DIR" 2>/dev/null; do sleep 0.05; done
t1=$(date +%s)
echo "second acquisition waited $((t1 - t0))s (expected ~2)"
rmdir "$PTY_LOCK_DIR"
wait "$cleaner_pid"
'
```

Expected: "second acquisition waited 2s". Confirms stale-lock-after-crash behavior is recoverable only by human intervention (expected — in the real flow the EXIT trap in `_run_test_job` handles cleanup).

- [ ] **Step 3: Commit (verification notes)**

No code changes; record the verification in a commit message for traceability:

```bash
git commit --allow-empty -m "$(cat <<'EOF'
test(release): verify mkdir-lock serializes pty group under contention

Manual verification (not a checked-in test — bash script has no unit
framework). Three synthetic pty jobs holding a 1s sleep serialize to
~3s wall time, confirming the mkdir-based lock behaves correctly.
Stale lock from a crashed holder requires external cleanup (handled
by EXIT trap in _run_test_job in the normal flow).

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 5: Verify trap cleanup on SIGINT / SIGTERM

`_run_all_tests_parallel` already installs `trap 'rm -rf "$log_dir"; rmdir "$PTY_LOCK_DIR" 2>/dev/null' EXIT INT TERM` (added in Task 1). Verify cleanup happens when Ctrl+C is pressed mid-run.

**Files:**
- None (verification-only task)

- [ ] **Step 1: Start a run in the background, interrupt, verify cleanup**

Run:

```bash
# Snapshot baseline of potential temp dirs before the run.
before=$(ls -d "${TMPDIR:-/tmp}"/yosh-parallel-tests.* "${TMPDIR:-/tmp}"/yosh-pty-lock.* 2>/dev/null | wc -l)

# Start the pipeline in the background.
./.claude/skills/release/scripts/release.sh test &
pid=$!

# Let it enter the parallel block (pre-compile + launch takes ~15-60s).
# Sleep 120s to guarantee we're past the `cargo test --no-run` phase.
sleep 120

# Send SIGINT.
kill -INT "$pid"
wait "$pid" || true

# Check that no lingering temp dirs exist from this run.
after=$(ls -d "${TMPDIR:-/tmp}"/yosh-parallel-tests.* "${TMPDIR:-/tmp}"/yosh-pty-lock.* 2>/dev/null | wc -l)

echo "temp dirs before: $before, after: $after"
[[ "$after" -eq "$before" ]] && echo "OK: cleanup worked" || echo "FAIL: lingering temp dirs"

# If FAIL, inspect and manually clean:
# ls -ld "${TMPDIR:-/tmp}"/yosh-parallel-tests.* "${TMPDIR:-/tmp}"/yosh-pty-lock.*
```

Expected: final line is `OK: cleanup worked`. If `FAIL`, the `trap` is not firing — investigate whether Ctrl+C reaches bash (may need `kill -INT -$pid` to target the process group) before concluding the trap is broken.

- [ ] **Step 2: Verify no zombie cargo processes**

Run: `pgrep -af 'cargo test'`
Expected: no output (or only unrelated cargo processes from other work). Confirms child cargo processes were reaped by bash's default SIGINT propagation.

If zombies remain: this indicates the child processes were detached. The current design relies on bash's default behavior where SIGINT from terminal goes to the foreground process group. The plan continues as-is since this is expected behavior for interactive release use; if this becomes a problem in automated contexts, add `kill 0` to the trap.

- [ ] **Step 3: Commit (verification notes)**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
test(release): verify trap cleanup on SIGINT mid-run

Manually interrupted ./release.sh test after entering the parallel
block; trap 'rm -rf log_dir; rmdir PTY_LOCK_DIR' EXIT INT TERM
correctly removes both temp artifacts. Resolves TODO.md line 120
(phase_test temp file trap cleanup) as a side effect.

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 6: Timing baseline — full pipeline run

Run the full `./release.sh test` and capture wall time to confirm the design's ≤ 900 s target.

**Files:**
- None (measurement-only task)

- [ ] **Step 1: Ensure warm target/ for a fair measurement**

Run: `cargo build`
Expected: `Finished` in seconds (target/ already warm from earlier tasks).

- [ ] **Step 2: Run and time the full pipeline**

Run: `time ./.claude/skills/release/scripts/release.sh test 2>&1 | tee /tmp/release-test-timing.log`
Expected: exit 0, final line `yosh-release: all tests passed`. `time` reports real/user/sys at the end.

- [ ] **Step 3: Record the measurement**

Extract: `grep -E '^(real|user|sys)' /tmp/release-test-timing.log | tail -3`
Expected format:

```
real    <MM>m<SS>.<FF>s
user    <MM>m<SS>.<FF>s
sys     <MM>m<SS>.<FF>s
```

Verify wall (`real`) is ≤ 15 minutes (900 s). If > 15 min: the parallelism is not delivering. Investigate by checking per-job durations: `ls -la "${TMPDIR:-/tmp}"/yosh-parallel-tests.*/` during a next run to see if one job dominates. Record actual numbers for the commit message regardless.

- [ ] **Step 4: Run twice more for stability**

Run: `time ./.claude/skills/release/scripts/release.sh test` (twice)
Expected: exit 0 both times, wall time variance within ±20 %. Confirms no intermittent flake.

- [ ] **Step 5: Commit (measurement notes)**

Replace `<WALL>` with the observed `real` time from Step 3, and `<VAR>` with the observed variance across the 3 runs from Step 4 (rounded to a whole percent).

```bash
git commit --allow-empty -m "$(cat <<'EOF'
perf(release): measure phase_test wall time after parallelization

Baseline (2026-04-22): wall 1720s, CPU 176s (~10% utilization).
After parallelization: wall <WALL>, 3 runs stable (±<VAR>%).

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 7: Failure-path validation — verify only failed jobs' logs print

Temporarily break a test, confirm the aggregation path prints only that job's log and fails with a clear summary, then revert.

**Files:**
- Temporarily modify: `tests/cli_help.rs` — inject a failing assertion
- Modify: none permanent (changes reverted at end of task)

- [ ] **Step 1: Pick a short assertion in `tests/cli_help.rs` and break it**

Run: `grep -n 'assert_eq!' tests/cli_help.rs | head -3`
Expected: some assertions printed with line numbers. Pick the first one, e.g. a line that looks like `assert_eq!(<left>, <right>);`.

Edit that line to a guaranteed-fail assertion. For example, change:

```rust
assert_eq!(stdout.contains("Usage:"), true);
```

to:

```rust
assert_eq!(stdout.contains("Usage:"), false);  // INTENTIONALLY BROKEN
```

- [ ] **Step 2: Run `phase_test` and capture output**

Run: `./.claude/skills/release/scripts/release.sh test 2>&1 | tee /tmp/release-fail-test.log; echo "exit=$?"`
Expected:
- Exit code: non-zero (ends with `exit=1`)
- stderr contains `--- cli_help output ---` followed by cargo test failure output
- stderr contains `yosh-release: tests failed: cli_help — fix and rerun`
- stderr does NOT contain `--- interactive output ---` or any other `--- <name> output ---` block (only the failing job's log is printed)

- [ ] **Step 3: Verify other jobs still ran to completion**

Run: `grep -E 'tests failed: ' /tmp/release-fail-test.log`
Expected: single line `yosh-release: tests failed: cli_help — fix and rerun`. Confirms no early abort dropped other jobs (they all finished; only `cli_help` is in the failed list).

- [ ] **Step 4: Revert the injected failure**

Run: `git checkout -- tests/cli_help.rs`
Expected: no output.

Confirm with: `git diff tests/cli_help.rs`
Expected: empty output.

- [ ] **Step 5: Re-run to confirm green after revert**

Run: `./.claude/skills/release/scripts/release.sh test`
Expected: exit 0, `yosh-release: all tests passed`.

- [ ] **Step 6: Commit (verification notes)**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
test(release): verify parallel runner's failure-path aggregation

Injected an assert failure into tests/cli_help.rs, confirmed
phase_test:
  - prints only the failed job's log block ('--- cli_help output ---')
  - reports 'tests failed: cli_help — fix and rerun'
  - does NOT abort other jobs early (all 16+e2e completed)

Reverted the injection; green run confirms no residual damage.

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Task 8: Remove satisfied TODO.md entries

Two TODO items are now resolved. Delete (per CLAUDE.md: "Delete completed items rather than marking them with `[x]`").

**Files:**
- Modify: `TODO.md` — remove lines 119 and 120

- [ ] **Step 1: Show the two lines slated for deletion**

Run: `sed -n '119p;120p' TODO.md`
Expected: two bullet lines — line 119 the `cargo test` parallelization entry, line 120 the `phase_test` temp file trap cleanup.

If the line numbers have shifted (e.g., TODO.md edited during this plan's execution), locate them by content instead:

Run: `grep -nE 'cargo. test. parallelization|phase_test. temp file trap' TODO.md`

- [ ] **Step 2: Delete both lines**

Edit `TODO.md`: remove the two bullet lines identified in Step 1. Use the Edit tool:

For the parallelization line, use its full content as `old_string` and an empty string as `new_string` (including the trailing newline).
For the trap cleanup line, same approach.

Exact current content of the two lines (from the 2026-04-23 reading of TODO.md):

Line 119:
```
- [ ] `cargo test` parallelization — measured 2026-04-22: wall 1720 s, CPU 176 s (~10 % utilization). 24 integration-test binaries run serially; `interactive` (144 tests), `pty_interactive` (20 tests with `Duration::from_secs(15)` timeouts), `signals`, `subshell` dominate wall time on subprocess/PTY waits. Run each test binary (`cargo test --test <name>`) in parallel, coordinated via a PTY-exclusion group so `pty_interactive` does not race other PTY consumers. Estimated savings: 10-20 minutes per release.
```

Line 120:
```
- [ ] `phase_test` temp file trap cleanup — `cargo_log`/`e2e_log` created by `mktemp` leak on SIGINT/SIGTERM because `phase_test` has no `trap`. Add `trap 'rm -f "$cargo_log" "$e2e_log"' EXIT INT TERM` immediately after the two `mktemp` lines (`.claude/skills/release/scripts/release.sh`). Code-review follow-up from 2026-04-22 release-perf work.
```

- [ ] **Step 3: Verify no stray references to the deleted items remain**

Run: `grep -nE 'cargo_log|e2e_log.*mktemp|phase_test. temp file trap|cargo. test. parallelization' TODO.md`
Expected: no output. Confirms both entries are fully removed from TODO.md.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): remove items resolved by cargo test parallelization

- cargo test parallelization (implemented in release.sh phase_test)
- phase_test temp file trap cleanup (subsumed by new trap EXIT INT TERM)

Part of 2026-04-23 cargo test parallelization plan.
EOF
)"
```

---

## Post-implementation checklist

After Task 8:

- [ ] `git log --oneline` shows 8 commits from this plan (some `--allow-empty` for verification traceability).
- [ ] `./release.sh test --dry-run` produces the 16-job listing.
- [ ] `./release.sh test` wall time meets the ≤ 900 s target (measured in Task 6).
- [ ] `TODO.md` no longer mentions cargo test parallelization or `phase_test` trap cleanup.
- [ ] No TBD / placeholder comments in `release.sh`.

## Rollback procedure (if regression found post-merge)

If the new `phase_test` misbehaves in production (e.g., flaky PTY lock under a specific macOS version), revert with:

```bash
git revert <commit-sha-of-Task-3>  # restores the legacy single-cargo-test path
```

Tasks 1, 2, and 8 are additive or cosmetic and can remain reverted separately if needed.
