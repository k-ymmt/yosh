# Release Pipeline Performance Improvements — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Shave 4-7 minutes off `release.sh` by removing redundant verify builds, simplifying the e2e runner timer, and parallelizing `phase_test`.

**Architecture:** Three small, independent shell-level changes + a TODO note. No Rust code changes. Each change can be landed and tested on its own. Spec: `docs/superpowers/specs/2026-04-22-release-perf-design.md`.

**Tech Stack:** POSIX sh (`e2e/run_tests.sh`), Bash (`.claude/skills/release/scripts/release.sh`), `cargo`.

---

## File map

- `TODO.md` — add cargo-test parallelization note (Task 1, item D)
- `e2e/run_tests.sh` — simplify per-test timer (Task 2, item B)
- `.claude/skills/release/scripts/release.sh` — parallelize `phase_test` (Task 3, item C) + `--no-verify` on publish (Task 4, item A)
- Final verification (Task 5) — no file changes, just a full `phase_test` smoke run

---

### Task 1: Record cargo-test parallelization idea in TODO.md (item D)

**Files:**
- Modify: `TODO.md` — append a single bullet to the existing `## Future: Release Skill Enhancements` section (currently ends at line 118)

- [ ] **Step 1: Append the TODO bullet**

Add this bullet at the end of the `## Future: Release Skill Enhancements` section in `TODO.md`:

```markdown
- [ ] `cargo test` parallelization — measured 2026-04-22: wall 1720 s, CPU 176 s (~10 % utilization). 24 integration-test binaries run serially; `interactive` (144 tests), `pty_interactive` (20 tests with `Duration::from_secs(15)` timeouts), `signals`, `subshell` dominate wall time on subprocess/PTY waits. Run each test binary (`cargo test --test <name>`) in parallel, coordinated via a PTY-exclusion group so `pty_interactive` does not race other PTY consumers. Estimated savings: 10-20 minutes per release.
```

- [ ] **Step 2: Verify the diff**

Run: `git diff TODO.md`
Expected: exactly one bullet added under `## Future: Release Skill Enhancements`, no other changes.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(todo): record cargo test parallelization as future release-skill work

Follow-up from /superpowers:systematic-debugging measuring release.sh.
cargo test wall=1720s CPU=176s (~10% util) — 24 test binaries serial,
PTY/subprocess waits dominate. Defer parallelization per spec
docs/superpowers/specs/2026-04-22-release-perf-design.md."
```

---

### Task 2: Simplify e2e runner timer (item B)

**Files:**
- Modify: `e2e/run_tests.sh:183-203` (timer subshell + wait/kill block)

**Why before A/C:** independently testable by re-running the full e2e suite and comparing wall time.

- [ ] **Step 1: Write a smoke test for the timeout path**

Create `e2e/_timeout_smoke.sh` with permissions `644` (see CLAUDE.md):

```sh
#!/bin/sh
# POSIX_REF: n/a (smoke test — verify runner timeout kicks in)
# DESCRIPTION: runs an infinite loop so runner's TIMEOUT (5s) must kill it
# EXPECT_EXIT: 0
# Runner reports timeouts via the marker file and prints [TIME] regardless
# of EXPECT_EXIT; Timedout counter in the summary is the assertion.
while true; do :; done
```

Shell command to create it portably: `cat > e2e/_timeout_smoke.sh <<'EOF' … EOF` then `chmod 644 e2e/_timeout_smoke.sh`.

- [ ] **Step 2: Capture baseline timeout behaviour against the unchanged runner**

Run: `/usr/bin/time -p ./e2e/run_tests.sh --filter=_timeout_smoke 2>&1 | tail -10`
Expected: a `[TIME]  _timeout_smoke.sh` line, `Timedout: 1` in the summary, and wall time around 5-6 s (the existing timer kicks in at `TIMEOUT=5`).

- [ ] **Step 3: Replace the timer block**

In `e2e/run_tests.sh`, the current block at **lines 183-198** is:

```sh
    # Timeout logic
    (
        _elapsed=0
        while [ "$_elapsed" -lt "$TIMEOUT" ]; do
            sleep 1
            _elapsed=$((_elapsed + 1))
            # Check if process is still running
            if ! kill -0 "$_pid" 2>/dev/null; then
                exit 0
            fi
        done
        # Timed out — kill the process
        kill -9 "$_pid" 2>/dev/null
        echo "timeout" >"$_exit_file"
    ) &
    _timer_pid=$!
```

Replace it with:

```sh
    # Timeout logic: single-shot sleep + kill.
    # Set YOSH_E2E_NO_TIMEOUT=1 to skip the timer entirely (local fast runs).
    if [ "${YOSH_E2E_NO_TIMEOUT:-0}" = "1" ]; then
        _timer_pid=""
    else
        (
            sleep "$TIMEOUT"
            kill -9 "$_pid" 2>/dev/null && echo "timeout" >"$_exit_file"
        ) &
        _timer_pid=$!
    fi
```

- [ ] **Step 4: Update the wait/kill teardown to tolerate empty _timer_pid**

Current block at **lines 200-203**:

```sh
    wait "$_pid" 2>/dev/null
    _wait_status=$?
    kill "$_timer_pid" 2>/dev/null
    wait "$_timer_pid" 2>/dev/null
```

Replace with:

```sh
    wait "$_pid" 2>/dev/null
    _wait_status=$?
    if [ -n "$_timer_pid" ]; then
        kill "$_timer_pid" 2>/dev/null
        wait "$_timer_pid" 2>/dev/null
    fi
```

- [ ] **Step 5: Verify the smoke-test timeout still triggers**

Run: `/usr/bin/time -p ./e2e/run_tests.sh --filter=_timeout_smoke`
Expected: summary `Timedout: 1`. Wall time ~5-6 s (unchanged — `sleep $TIMEOUT` is the same 5 s).

- [ ] **Step 6: Verify `YOSH_E2E_NO_TIMEOUT=1` path**

Run the suite with the opt-out on a single tiny test (not the hanging one) to confirm the branch executes:

```bash
YOSH_E2E_NO_TIMEOUT=1 ./e2e/run_tests.sh --filter=arithmetic
```

Expected: `Total: 19  Passed: 19  Failed: 0  Timedout: 0`. Wall time noticeably lower than the default path (typical: 0.4-0.6 s vs 1.3-1.4 s).

- [ ] **Step 7: Delete the smoke test and verify full suite still passes**

```bash
rm e2e/_timeout_smoke.sh
/usr/bin/time -p ./e2e/run_tests.sh 2>&1 | tail -5
```

Expected: `Total: 389  Passed: 389  Failed: 0  Timedout: 0  XFail: 0  XPass: 0` and a wall time noticeably below the 53.15 s baseline (target: 30-35 s range; will vary by system load).

- [ ] **Step 8: Commit**

```bash
git add e2e/run_tests.sh
git commit -m "perf(e2e): replace per-test timer while-loop with single sleep

The while-sleep-1 / kill -0 polling timer subshell added 50-120 ms
of shell overhead per test (measured 2026-04-22: 50 iterations at
60-120 ms/iter with timer, 10-25 ms/iter without). Across 389 tests
that is ~25-35 s of pure framework cost.

Replace with a one-shot 'sleep \$TIMEOUT && kill' background, and
add YOSH_E2E_NO_TIMEOUT=1 to let local developers skip the timer
entirely. Timeout behavior for genuinely hung tests is unchanged."
```

---

### Task 3: Parallelize `phase_test` (item C)

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh:78-84` (`phase_test` function body)

- [ ] **Step 1: Read the current `phase_test` to confirm line numbers**

Run: `sed -n '78,84p' .claude/skills/release/scripts/release.sh`

Expected output (current):

```
phase_test() {
  echo "yosh-release: running cargo test..." >&2
  cargo test || fail "cargo test failed — fix tests and rerun"
  echo "yosh-release: running e2e tests..." >&2
  ./e2e/run_tests.sh || fail "e2e tests failed — fix tests and rerun"
  echo "yosh-release: all tests passed" >&2
}
```

- [ ] **Step 2: Replace `phase_test` body**

Replace the body (keep the `phase_test() {` line and the closing `}`) with:

```sh
phase_test() {
  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"

  local cargo_log e2e_log
  cargo_log="$(mktemp -t yosh-cargo-test.XXXXXX)"
  e2e_log="$(mktemp -t yosh-e2e.XXXXXX)"

  echo "yosh-release: running cargo test and e2e tests in parallel..." >&2
  cargo test >"$cargo_log" 2>&1 &
  local cargo_pid=$!
  ./e2e/run_tests.sh >"$e2e_log" 2>&1 &
  local e2e_pid=$!

  wait "$cargo_pid"; local cargo_rc=$?
  wait "$e2e_pid";   local e2e_rc=$?

  if [[ $cargo_rc -ne 0 ]]; then
    echo "--- cargo test output ---" >&2
    cat "$cargo_log" >&2
    rm -f "$cargo_log" "$e2e_log"
    fail "cargo test failed — fix tests and rerun"
  fi
  if [[ $e2e_rc -ne 0 ]]; then
    echo "--- e2e output ---" >&2
    cat "$e2e_log" >&2
    rm -f "$cargo_log" "$e2e_log"
    fail "e2e tests failed — fix tests and rerun"
  fi

  rm -f "$cargo_log" "$e2e_log"
  echo "yosh-release: all tests passed" >&2
}
```

- [ ] **Step 3: Shell-parse check**

Run: `bash -n .claude/skills/release/scripts/release.sh`
Expected: no output (exit code 0).

- [ ] **Step 4: Smoke-run the phase**

Run: `/usr/bin/time -p .claude/skills/release/scripts/release.sh test 2>&1 | tail -5`
Expected: exits 0 with `yosh-release: all tests passed`. Wall time dominated by `cargo test` (~28 min on this machine); e2e should finish during that window so overall wall time is roughly `max(cargo_test, e2e)` = cargo test.

Note: this step is slow. If you only want to confirm the code path without waiting for the full cargo test, temporarily replace `cargo test` with `cargo test --lib -p yosh-plugin-api` for a one-off sanity run, then restore.

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "perf(release): parallelize phase_test (cargo test + e2e)

cargo test (~28 min wall, ~3 min CPU) and e2e (~35-55 s wall) run
in parallel. cargo build up front guarantees target/debug/yosh
exists before e2e starts; after that cargo test only writes to
target/debug/deps/ while e2e only reads target/debug/yosh, so no
write conflict. Streams go to temp files and are dumped on failure.

Measured savings: ~53 s per release (e2e wall time folds into
cargo test's longer window)."
```

---

### Task 4: Add `--no-verify` to every `cargo publish` (item A)

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh:178-182` (the `cmd=(…)` block inside `phase_publish`)

- [ ] **Step 1: Read the current `phase_publish` loop body**

Run: `sed -n '177,184p' .claude/skills/release/scripts/release.sh`

Expected (current):

```
    echo "yosh-release: publishing $crate..." >&2
    if [[ "$crate" == "yosh" ]]; then
      cmd=(cargo publish)
    else
      cmd=(cargo publish -p "$crate")
    fi
```

- [ ] **Step 2: Replace the `cmd=(…)` branches**

Replace the two assignments so both include `--no-verify`:

```sh
    echo "yosh-release: publishing $crate..." >&2
    if [[ "$crate" == "yosh" ]]; then
      cmd=(cargo publish --no-verify)
    else
      cmd=(cargo publish --no-verify -p "$crate")
    fi
```

- [ ] **Step 3: Shell-parse check**

Run: `bash -n .claude/skills/release/scripts/release.sh`
Expected: no output (exit code 0).

- [ ] **Step 4: Confirm the flag is accepted by cargo**

Run: `cargo publish --no-verify --help | head -3`
Expected: the help header for `cargo publish` (no "unrecognized option" error).

Note: we do not actually run `cargo publish --dry-run --no-verify` here because `phase_bump` hasn't been run and the current crate version may already exist on crates.io. Reviewers should verify by inspection; the flag is vetted by `cargo publish --help`.

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "perf(release): pass --no-verify to cargo publish

phase_test already compiles and tests the workspace against the
exact source being packaged, so cargo publish's default verify
build (a clean rebuild of each extracted tarball, ~3-6 min total
for the 4 crates) is pure duplicate work. Skip it.

Package metadata checks and the crates.io server-side validation
still run; only the local clean-build of the extracted tarball
is elided. If packaging metadata regresses, cargo publish still
surfaces it before the upload."
```

---

### Task 5: Final end-to-end validation

**Files:** none modified.

- [ ] **Step 1: Confirm all four commits landed**

Run: `git log --oneline -6`

Expected (most recent first): commits from Tasks 1-4 plus the prior spec commit `fc93a32`. Example:

```
<sha> perf(release): pass --no-verify to cargo publish
<sha> perf(release): parallelize phase_test (cargo test + e2e)
<sha> perf(e2e): replace per-test timer while-loop with single sleep
<sha> docs(todo): record cargo test parallelization as future release-skill work
fc93a32 docs(plan): release pipeline performance improvements (A/B/C + TODO D)
```

- [ ] **Step 2: Run the test phase end-to-end**

Run: `/usr/bin/time -p .claude/skills/release/scripts/release.sh test`
Expected: exit 0 with `yosh-release: all tests passed`. Wall time dominated by `cargo test` (~28 min).

- [ ] **Step 3: Confirm e2e speedup independently**

Run: `/usr/bin/time -p ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: `Passed: 389  Failed: 0  Timedout: 0` and wall time in the 30-35 s range (was 53 s pre-change).

- [ ] **Step 4: Report**

Summarize to the user: commits landed, observed e2e wall time, full-phase test result, and remind them that `phase_publish` behavior is covered by inspection (no safe dry-run in this project state).

---

## Notes for the executing engineer

- **Do not run `cargo clean`** during this plan. The 13 GB / 863 K-file `target/` bloat is real but out of scope; cleaning would invalidate the 28-minute cargo-test baseline used for verification.
- **Do not touch PTY or integration-test code.** Task D in the spec explicitly defers cargo-test parallelization; attempting it here breaks the plan's bounded scope.
- **`mktemp -t` is portable across macOS and Linux.** On macOS it creates `/var/folders/.../yosh-cargo-test.XXXXXX`; on Linux `/tmp/...`. Both fine.
- **The `--no-verify` flag exists in all supported cargo versions** (stable since Rust 1.5). No version-gating needed.
