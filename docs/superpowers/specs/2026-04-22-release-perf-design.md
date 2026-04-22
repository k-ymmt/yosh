# Release Pipeline Performance Improvements

## Problem

`release.sh` takes ~35-40 minutes end-to-end. Measured breakdown (debug build, M-series Mac, current HEAD):

| Phase | Wall time | CPU time | Notes |
|---|---|---|---|
| `phase_test` (`cargo test` + e2e) | ~30 min | ~3 min | CPU utilization ~10% — mostly I/O / subprocess waits |
| `phase_bump` (`cargo build`) | 10-30 s | — | incremental |
| `phase_publish` (4 × verify build) | 3-6 min | — | each `cargo publish` re-builds from a pristine copy |
| `phase_push` | <5 s | — | git only |

Two independent inefficiencies dominate:

1. **Redundant verify builds at publish time.** `phase_test` already runs `cargo test` against the workspace, so the 4 × `cargo publish` verify builds compile code that was just proven to build and pass tests.
2. **Serial `phase_test` + per-test shell overhead in e2e.**
   - `cargo test` (28 min) and `./e2e/run_tests.sh` (53 s) run back-to-back even though they can overlap.
   - The e2e runner forks a per-test `while sleep 1; kill -0` timer subshell, adding 50-120 ms of shell overhead per test (~25-35 s across 389 tests, quantified by replacing yosh with `true` in a micro-benchmark).

A third, larger issue exists — `cargo test` itself sits idle 90 % of its 28-minute wall time waiting on PTY / subprocess tests — but it requires test-harness changes and is deferred to TODO.md.

## Scope

This spec covers three contained, low-risk improvements and a TODO note:

- **A.** Pass `--no-verify` to every `cargo publish` call.
- **B.** Replace the e2e runner's timer subshell with a single `sleep && kill` background, plus an opt-out env var.
- **C.** Parallelize `phase_test` so `cargo test` and `./e2e/run_tests.sh` run concurrently.
- **D.** Record the cargo-test parallelization idea as a future task in `TODO.md`.

Out of scope: cargo-test parallelization itself, `target/` bloat cleanup (`kish-*` leftovers from the rename), CI workflow changes.

Expected savings from A+B+C: **4-7 minutes per release**, with no test-code changes.

## Design

### A. `cargo publish --no-verify`

**Change:** `.claude/skills/release/scripts/release.sh`, `phase_publish`.

For each crate, replace

```sh
cmd=(cargo publish)                  # for "yosh"
cmd=(cargo publish -p "$crate")      # for the three sub-crates
```

with

```sh
cmd=(cargo publish --no-verify)
cmd=(cargo publish --no-verify -p "$crate")
```

**Why safe:**
- `phase_test` just compiled and tested the entire workspace. The source being packaged is byte-identical.
- `cargo publish` still checks package metadata, runs `cargo package`, and lets crates.io validate on upload. Only the local clean-build of the extracted tarball is skipped.
- If a packaging problem slips through (e.g. missing file in `include`), the upload itself fails and `--from <crate>` resume still works.

### B. e2e runner timer simplification

**Change:** `e2e/run_tests.sh`, the per-test timer subshell (~lines 155-176).

Current structure (shortened):

```sh
(
    _elapsed=0
    while [ "$_elapsed" -lt "$TIMEOUT" ]; do
        sleep 1
        _elapsed=$((_elapsed + 1))
        if ! kill -0 "$_pid" 2>/dev/null; then exit 0; fi
    done
    kill -9 "$_pid" 2>/dev/null
    echo "timeout" >"$_exit_file"
) &
```

Replace with:

```sh
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

And adjust the teardown to skip `kill/wait` when `_timer_pid` is empty:

```sh
wait "$_pid" 2>/dev/null
_wait_status=$?
if [ -n "$_timer_pid" ]; then
    kill "$_timer_pid" 2>/dev/null
    wait "$_timer_pid" 2>/dev/null
fi
```

**Why:**
- The current loop polls `kill -0` every second purely to exit the timer subshell early. But the main script already kills the timer when the test finishes, so the polling loop is redundant work.
- One `sleep $TIMEOUT` + one `kill` per timer is enough. If the test finishes first, the main thread kills the timer during its sleep — same outcome.
- `YOSH_E2E_NO_TIMEOUT=1` lets local developers run without the timer at all when they trust their build.

**Measured savings:** ~50 ms/test → ~20 s across 389 tests. Micro-benchmarked by comparing subshell variants.

**Risk:** a truly hung test (infinite loop in yosh) now waits `$TIMEOUT` seconds before being killed, same as today. Behaviour unchanged for the timeout case itself.

### C. `phase_test` parallelization

**Change:** `.claude/skills/release/scripts/release.sh`, `phase_test`.

New structure:

```sh
phase_test() {
  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"

  echo "yosh-release: running cargo test and e2e in parallel..." >&2
  local cargo_log e2e_log
  cargo_log="$(mktemp -t yosh-cargo-test.XXXXXX)"
  e2e_log="$(mktemp -t yosh-e2e.XXXXXX)"

  cargo test >"$cargo_log" 2>&1 &
  local cargo_pid=$!
  ./e2e/run_tests.sh >"$e2e_log" 2>&1 &
  local e2e_pid=$!

  wait "$cargo_pid"; local cargo_rc=$?
  wait "$e2e_pid";   local e2e_rc=$?

  if [ $cargo_rc -ne 0 ]; then
    cat "$cargo_log" >&2
    rm -f "$cargo_log" "$e2e_log"
    fail "cargo test failed — fix tests and rerun"
  fi
  if [ $e2e_rc -ne 0 ]; then
    cat "$e2e_log" >&2
    rm -f "$cargo_log" "$e2e_log"
    fail "e2e tests failed — fix tests and rerun"
  fi

  rm -f "$cargo_log" "$e2e_log"
  echo "yosh-release: all tests passed" >&2
}
```

**Why safe:**
- `cargo build` first guarantees `./target/debug/yosh` exists before `e2e/run_tests.sh` starts.
- After that, `cargo test` only writes to `target/debug/deps/` (test binaries), and `e2e/run_tests.sh` only *reads* `./target/debug/yosh`. No write conflict.
- Cargo's own target-dir lock handles any residual coordination.
- Net savings ≈ 53 s (the e2e wall time is hidden inside the much longer `cargo test` wall time).

**Failure mode:** stderr/stdout is captured to temp files so the two streams don't interleave. On failure the script dumps the relevant log. On success both logs are discarded.

### D. TODO.md note

Add (or create) a `## Performance` section at the top of `TODO.md`:

```
## Performance

- `cargo test` wall time is ~28 min with only ~10% CPU utilization (measured 2026-04-22):
  subprocess-spawning integration tests (`interactive`, `pty_interactive`, `signals`, `subshell`)
  serialize on I/O and `Duration::from_secs(...)` waits. Parallelize by running each test binary
  concurrently (`cargo test --test <name>` processes) with a PTY-exclusion group so that
  `pty_interactive` doesn't race other PTY consumers. Estimated savings: 10-20 minutes per release.
```

## Testing Plan

1. **A alone:** impossible to test without publishing. Rely on review + the fact that `cargo publish --no-verify --dry-run` accepts the flag; no actual publish during verification.
2. **B alone:** run `./e2e/run_tests.sh` and confirm 389/389 pass. Also run with `YOSH_E2E_NO_TIMEOUT=1`. Record wall time and compare against the 53 s baseline — expect ~30-35 s.
3. **C alone:** run `.claude/skills/release/scripts/release.sh test` and confirm it exits 0. Record wall time.
4. **Combined:** run the `test` phase end-to-end. Expected wall time ~28-29 min (dominated by `cargo test`), saving ~53 s vs. baseline.

## Files Touched

- `.claude/skills/release/scripts/release.sh` — A and C
- `e2e/run_tests.sh` — B
- `TODO.md` — D

## Non-Goals

- No changes to test code.
- No changes to `phase_bump`, `phase_publish` failure semantics, or `phase_push`.
- No `cargo clean` / target-dir changes. The 13 GB / 863 K-file `target/` bloat is real but orthogonal.
