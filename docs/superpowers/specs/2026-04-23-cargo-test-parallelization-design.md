# Design: `cargo test` Parallelization in `phase_test`

- **Date**: 2026-04-23
- **Status**: Approved (pending implementation plan)
- **Scope**: `.claude/skills/release/scripts/release.sh` `phase_test` function only
- **Supersedes / follows-up**: TODO.md line 119 (release skill enhancement)
- **Related follow-ups resolved**: TODO.md line 120 (phase_test temp file trap cleanup)

## 1. Background

The release script's `phase_test` already runs `cargo test` (whole workspace) and `./e2e/run_tests.sh` in parallel. Measurement from 2026-04-22:

- Wall clock: 1720 s
- CPU time: 176 s
- Utilization: ~10 %

The bottleneck is inside `cargo test`: 24 integration-test binaries run serially (cargo compiles in parallel but executes binaries one at a time). PTY-heavy binaries (`pty_interactive`, ~20 tests with 15 s timeouts), subprocess-heavy binaries (`signals`, `subshell`), and the large `interactive` binary (144 tests) dominate wall time on subprocess/PTY waits.

Splitting each test binary into its own `cargo test --test <name>` invocation and running them concurrently targets **wall 500–900 s** (approximately 50–70 % reduction, or 10–20 minutes saved per release).

## 2. Goals / Non-Goals

**Goals**:

- Reduce `phase_test` wall time by parallelizing per-test-binary execution.
- Keep test output formatting predictable on failure (per-job labeled log).
- Cross-platform (Linux + macOS) without adding external tool dependencies.
- Resolve the existing temp-file-trap-cleanup gap (TODO line 120) as a side effect.

**Non-goals**:

- Changing the developer workflow. `cargo test` on the command line is unaffected.
- Introducing `cargo nextest` or other external test runners.
- Parallelizing inside `phase_bump`/`phase_publish`/`phase_push`.
- Adding CI macOS runners (separate TODO item).

## 3. Architecture

### 3.1 Flow

```
phase_test
├─ (1) cargo build                              # existing, unchanged
├─ (2) cargo test --no-run --workspace          # NEW: pre-build all test bins
├─ (3) parallel block (JOBS + e2e):
│   ├─ cargo test --lib -p yosh
│   ├─ cargo test --doc -p yosh
│   ├─ cargo test -p yosh-plugin-api
│   ├─ cargo test -p yosh-plugin-sdk
│   ├─ cargo test -p yosh-plugin-manager
│   ├─ cargo test --test cli_help
│   ├─ cargo test --test errexit
│   ├─ cargo test --test history
│   ├─ cargo test --test ignored_on_entry
│   ├─ cargo test --test interactive
│   ├─ cargo test --test parser_integration
│   ├─ cargo test --test plugin
│   ├─ cargo test --test plugin_cli_help
│   ├─ cargo test --test signals
│   ├─ cargo test --test subshell
│   ├─ cargo test --test pty_interactive   (PTY-exclusive group)
│   └─ ./e2e/run_tests.sh
└─ (4) Aggregate — on failure, print logs of failed jobs only, then `fail`
```

### 3.2 Components

- **`JOBS` array** — flat, explicit list of `"name|group|cargo-args..."` at the top of `phase_test`. Groups: `pty` (mutually exclusive) or `free` (unbounded parallel).
- **`_run_test_job`** — runs a single job, routing through the PTY lock if `group == pty`. Writes stdout/stderr to a per-job log file.
- **`_run_all_tests_parallel`** — launches every job + e2e in the background, waits for all, collects failures, and prints logs of only the failed jobs.
- **PTY exclusion via `mkdir` atomic lock** — `mkdir $PTY_LOCK_DIR` is atomic on both Linux and macOS. `flock(1)` is rejected because it is not part of macOS's default toolchain.

### 3.3 Build-cache strategy

Running `cargo test --no-run --workspace` once before the parallel block ensures every integration test binary (and library/crate test bins) is compiled and cached in `target/`. Subsequent `cargo test --test <name>` invocations in the parallel block perform only a fresh-check and execute, avoiding contention on `target/.rustc_info.json` and other cargo-internal locks.

Note: `cargo test --no-run` does not include doc tests (rustdoc compiles them separately). The `doc` job in the parallel block therefore performs its own compile step via `cargo test --doc`. This is intentional and does not contend with integration-test binaries — doc-test compilation uses rustdoc's own target area and completes quickly relative to the PTY-bound jobs that dominate wall time.

## 4. Job Classification

| Job name              | Group | Cargo args                          | Notes                                           |
|-----------------------|-------|-------------------------------------|-------------------------------------------------|
| `lib`                 | free  | `test --lib -p yosh`                | yosh unit tests                                 |
| `doc`                 | free  | `test --doc -p yosh`                | yosh doc tests                                  |
| `plugin-api`          | free  | `test -p yosh-plugin-api`           | workspace crate                                 |
| `plugin-sdk`          | free  | `test -p yosh-plugin-sdk`           | workspace crate                                 |
| `plugin-manager`      | free  | `test -p yosh-plugin-manager`       | includes `tests/sync_integration.rs`            |
| `cli_help`            | free  | `test --test cli_help`              | subprocess (independent process groups)         |
| `errexit`             | free  | `test --test errexit`               | subprocess                                      |
| `history`             | free  | `test --test history`               | subprocess                                      |
| `ignored_on_entry`    | free  | `test --test ignored_on_entry`      | subprocess                                      |
| `interactive`         | free  | `test --test interactive`           | 144 tests, in-process (MockTerminal, no spawn)  |
| `parser_integration`  | free  | `test --test parser_integration`    | pure parser, in-process                         |
| `plugin`              | free  | `test --test plugin`                | subprocess                                      |
| `plugin_cli_help`     | free  | `test --test plugin_cli_help`       | subprocess                                      |
| `signals`             | free  | `test --test signals`               | subprocess + signal handling (independent OS procs) |
| `subshell`            | free  | `test --test subshell`              | subprocess (independent OS procs)               |
| `pty_interactive`     | pty   | `test --test pty_interactive`       | 20 tests, uses `expectrl` PTY                   |

### 4.1 Excluded from JOBS

- `tests/plugins/test_plugin` (workspace member, `cdylib` only — has no executable tests; built as a side effect of `--workspace --no-run` and consumed as an artifact by `plugin` test binary).

### 4.2 Why only `pty_interactive` is in the `pty` group

Verified by `grep -l "expectrl\|PtyProcess\|pty::" tests/*.rs`: only `pty_interactive.rs` matches. Other subprocess-spawning tests (`signals`, `subshell`, etc.) fork independent OS processes that do not share a TTY; their parallelism is safe. `interactive.rs`, despite its size, uses in-process `MockTerminal` and spawns no subprocess.

## 5. Implementation Details

### 5.1 Script structure (pseudo-code)

```bash
PTY_LOCK_DIR=""

phase_test() {
  local dry_run=0
  if [[ "${1:-}" == "--dry-run" ]]; then
    dry_run=1; shift
  fi

  local JOBS=(
    "lib|free|test --lib -p yosh"
    # ... (full table from §4) ...
    "pty_interactive|pty|test --test pty_interactive"
  )

  if [[ $dry_run -eq 1 ]]; then
    echo "yosh-release: dry-run — ${#JOBS[@]} jobs + e2e would run:" >&2
    for j in "${JOBS[@]}"; do echo "  $j" >&2; done
    echo "  e2e|-|./e2e/run_tests.sh" >&2
    return 0
  fi

  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"

  echo "yosh-release: pre-compiling test binaries..." >&2
  cargo test --no-run --workspace \
    || fail "cargo test --no-run failed — fix and rerun"

  # Reserve a unique path for the lock. mktemp -d creates it; we remove it so
  # the path is absent on entry (absent = unlocked, present = held).
  PTY_LOCK_DIR="$(mktemp -d -t yosh-pty-lock.XXXXXX)"
  rmdir "$PTY_LOCK_DIR"

  echo "yosh-release: running ${#JOBS[@]} test jobs + e2e in parallel..." >&2
  _run_all_tests_parallel "${JOBS[@]}"

  echo "yosh-release: all tests passed" >&2
}

_run_test_job() {
  # Args: name group log -- cargo-args...
  local name="$1" group="$2" log="$3"; shift 3

  if [[ "$group" == "pty" ]]; then
    while ! mkdir "$PTY_LOCK_DIR" 2>/dev/null; do sleep 0.05; done
    trap 'rmdir "$PTY_LOCK_DIR" 2>/dev/null' EXIT
  fi

  cargo "$@" >"$log" 2>&1
}

_run_all_tests_parallel() {
  local log_dir
  log_dir="$(mktemp -d -t yosh-parallel-tests.XXXXXX)"
  trap 'rm -rf "$log_dir"; rmdir "$PTY_LOCK_DIR" 2>/dev/null' EXIT INT TERM

  local -a pids names logs
  local idx=0

  for job in "$@"; do
    IFS='|' read -r name group cmd <<< "$job"
    local log="$log_dir/$name.log"
    ( _run_test_job "$name" "$group" "$log" $cmd ) &
    pids[$idx]=$!
    names[$idx]="$name"
    logs[$idx]="$log"
    idx=$((idx+1))
  done

  # e2e as an additional parallel job
  local e2e_log="$log_dir/e2e.log"
  ( ./e2e/run_tests.sh >"$e2e_log" 2>&1 ) &
  pids[$idx]=$!
  names[$idx]="e2e"
  logs[$idx]="$e2e_log"

  local -a failed
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

### 5.2 Key design decisions

1. **`--no-run --workspace` pre-build** — avoids `target/` lock contention between parallel `cargo test --test` invocations.
2. **`mkdir` as atomic lock primitive** — works on macOS without `flock(1)`; the 50 ms polling overhead is negligible because only one job (`pty_interactive`) ever holds the lock.
3. **Per-job log files in a single `log_dir`** — failure output formatting matches the current "one labeled block per failure" pattern; cleanup is a single `rm -rf`.
4. **Wait for all jobs, no early abort** — prevents zombie processes and provides complete failure picture. Aligns with cargo's `--no-fail-fast` philosophy.
5. **`--dry-run` flag** — lists all jobs without execution. Useful for CI debugging and future maintenance.
6. **`$PTY_LOCK_DIR` is unique per invocation** (`mktemp -d`), preventing stale locks if a previous run crashed without cleanup.

### 5.3 Cross-platform note

All primitives used (`mktemp -d`, `mkdir`, `rmdir`, `sleep 0.05`, `cat`, `wait`, `trap`) are POSIX-standard and present on macOS 15+ (Darwin) and modern Linux. Bash 3.2 (macOS default `/bin/bash`) is sufficient; no bash 4/5-only features are used (associative arrays are used via `declare -a`, not `-A`).

## 6. Error Handling

| Failure                                      | Behavior                                                                 |
|----------------------------------------------|--------------------------------------------------------------------------|
| `cargo build` fails                          | Immediate `fail` (unchanged from current)                                |
| `cargo test --no-run --workspace` fails      | Immediate `fail`, no parallel block enters                               |
| 1 job fails in parallel block                | Wait for all others, print failed-job log, `fail` with job name          |
| Multiple jobs fail                           | Wait for all, print each failed job's log, `fail` with all names joined  |
| Ctrl+C (SIGINT) during parallel block        | `trap` removes `log_dir` and `$PTY_LOCK_DIR`; child procs inherit SIGINT |
| Shell unexpected exit                        | `trap EXIT` covers cleanup in all paths                                  |
| Concurrent `release.sh test` invocations     | Each uses its own `mktemp -d` lock dir — no cross-run interference       |

## 7. Testing & Validation

No bash unit tests (no framework in this repo). Validation is empirical:

1. **Dry-run** — `./release.sh test --dry-run` prints the job list; visually verify all binaries present, no duplicates.
2. **Timing baseline** — run on a warm `target/` and record `time ./release.sh test`. Target: wall ≤ 900 s, ideally ≤ 600 s.
3. **All-green reproducibility** — run 3 times back-to-back; all pass.
4. **Single-job-failure scenario** — temporarily rename an assertion in `tests/cli_help.rs` to force failure, run `./release.sh test`, verify:
   - All other jobs complete
   - Only `cli_help` log is printed in the failure block
   - `fail "tests failed: cli_help — fix and rerun"` appears
   - Revert the assertion.
5. **Ctrl+C cleanup** — start `./release.sh test`, Ctrl+C mid-run, verify `$TMPDIR/yosh-parallel-tests.*` and `$TMPDIR/yosh-pty-lock.*` are cleaned up.
6. **PTY-exclusion fuzz** — temporarily add a second fake job to the `pty` group, verify both run serialized (logs show non-overlapping timestamps).

## 8. Implementation Order (for plan phase)

1. Add `JOBS` array and `_run_test_job` / `_run_all_tests_parallel` helper functions. Keep `phase_test` behaviorally identical at first (only one non-pty job invoked, to verify plumbing).
2. Wire the full `JOBS` list into `phase_test`. Remove legacy inline `cargo test` / `e2e` parallelization.
3. Add `mkdir`-based PTY lock around `pty_interactive`.
4. Add `trap` cleanup for `log_dir` and `$PTY_LOCK_DIR` on EXIT/INT/TERM.
5. Add `--dry-run` flag.
6. Empirical timing validation (§7 steps 1–3).
7. Failure-path validation (§7 steps 4–6).
8. Delete TODO.md line 119 (parallelization request) and line 120 (trap cleanup follow-up).

## 9. Open Questions / Future Work

- **Parallelism cap**: left unbounded in this design. If an overloaded CI or low-RAM dev machine shows thrashing, introduce `PARALLEL_MAX=$(nproc)` or similar. Track empirically first.
- **Progress indicator during the parallel block**: 5-minute+ silence may be unsettling. Can be added later (a background loop counting remaining jobs every 30 s). Explicitly deferred to avoid scope creep.
- **macOS CI coverage** (TODO line 84): separate item; this design does not address it but the cross-platform lock choice (`mkdir`) is a prerequisite for that work to reuse the script.
