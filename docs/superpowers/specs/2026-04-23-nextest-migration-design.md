# cargo-nextest migration design

Date: 2026-04-23
Outcome: **reverted 2026-04-24** — see "Outcome" section immediately below.

## Outcome (2026-04-24)

The migration was implemented, measured, and **reverted**. cargo-nextest was
~1.5-2× slower than the pre-existing bash-parallel `release.sh phase_test` on
this workspace.

Root cause: nextest runs each test in its own process. On macOS each process
spawn pays ~150 ms of OS-level overhead (XProtect / Gatekeeper checks, plus
nextest's own per-process `leak-timeout` default). yosh has ~1939 tests,
dominated by microsecond-scale unit tests, so the per-process tail alone is
~290 s — larger than the entire pre-existing parallel run. The old bash
orchestration pays that tail once per test binary (16 binaries), not per test,
which is why it is structurally faster for this suite.

Optimization attempts that did **not** close the gap:

- `leak-timeout = "50ms"` — cut reported nextest test time from 35 s to 5 s
  but did not reduce wall time meaningfully (415-533 s across back-to-back
  runs; macOS process-spawn variance dominated).
- `test-threads = 16` (2× num_cpus) — made wall time *worse* (624 s) due to
  oversubscription on this 8-core machine.

See the "Measurement results" and "Why nextest lost" sections at the end of
this document. The design below is preserved as the historical record of
what was shipped (commits `7ae61af`, `eebd892`, `25dbcee`, `c55f8f6`) and
then reverted (commits `44d224b`, `2f163d7`, `0861825`, `0449ebe`).

## Goal

Replace the bash-based parallel test orchestration in `release.sh phase_test`
with `cargo-nextest`. Measure the wall-time change and update the release skill
and `CLAUDE.md` accordingly.

## Motivation

`release.sh phase_test` currently runs 15 cargo invocations plus `./e2e/run_tests.sh`
in parallel, using a `mkdir`-based lock directory to serialize the PTY test
binary (`pty_interactive`). Three back-to-back runs after the 2026-04-23
parallelization work measured 95 s / 162 s / 178 s (±22 %), tracked in
`TODO.md`.

cargo-nextest ([nexte.st](https://nexte.st/)) is a next-generation Rust test
runner that provides:

- Per-test process isolation across the workspace, eliminating the need for
  manually listing each test binary.
- A `.config/nextest.toml` file with `[test-groups]` that natively limit
  concurrency by filterset expression, replacing the PTY `mkdir` lock.
- First-class structured output and failure aggregation, replacing the hand-rolled
  log aggregation in `_run_all_tests_parallel`.
- A stable upstream binary distributed via mise, already pinned in
  `mise.local.toml` as `"cargo:cargo-nextest" = "latest"`.

The migration deletes roughly 108 lines of bash orchestration and adds back
about 55 lines for the new `phase_test`, a net reduction of ~50 lines while
preserving (and plausibly improving) wall-time characteristics.

## Non-goals

- Adding a CI workflow (`.github/workflows/…`). No CI is set up today; that
  work is tracked separately.
- Replacing `./e2e/run_tests.sh`. E2E tests are shell scripts driving a built
  binary and remain outside cargo-nextest's scope.
- Adding retries to absorb PTY flakes. Retries stay at `0` to preserve
  detection fidelity; PTY flake handling is a separate concern
  (`TODO.md` line tracking `pty_interactive` occasional timeouts).
- Pinning a specific cargo-nextest version beyond what mise resolves from
  `"latest"`. This can be tightened later if drift causes friction.

## Files affected

| File | Change |
|---|---|
| `.config/nextest.toml` | New — default profile + PTY test-group |
| `.claude/skills/release/scripts/release.sh` | Rewrite `phase_test`; delete `PHASE_TEST_JOBS`, `_run_test_job`, `_run_all_tests_parallel`, `PTY_LOCK_DIR` |
| `CLAUDE.md` | Replace **Build & Test** section with nextest-based commands + install note |
| `TODO.md` | Drop the line tracking `release.sh test` variance (obsoleted) |
| `.claude/skills/release/SKILL.md` | No change — phase names and script contract unchanged |

## `.config/nextest.toml`

```toml
# cargo-nextest configuration for yosh.
# Doctests are not supported by nextest; run them via `cargo test --doc --workspace`.

[profile.default]
retries = 0

# pty_interactive uses expectrl against a shared PTY and must not overlap with
# itself. Other test binaries run with unbounded concurrency.
[test-groups]
pty-serial = { max-threads = 1 }

[[profile.default.overrides]]
filter = 'binary(pty_interactive)'
test-group = 'pty-serial'
```

Design notes:

- `retries = 0` — preserves detection fidelity. PTY flakes will fail a release
  and require a manual rerun, matching pre-migration behavior.
- `pty-serial = { max-threads = 1 }` scoped by `filter = 'binary(pty_interactive)'`
  — serializes tests *within* `pty_interactive`, but still lets it run in
  parallel with other binaries (`interactive`, `signals`, `subshell`, …). This
  is strictly more parallel than the current `mkdir` lock, which serialized
  only PTY jobs against each other but blocked on the whole PTY binary from
  the bash side.
- No `slow-timeout` / `leak-timeout` overrides — rely on nextest defaults
  (60 s slow, 100 ms leak) and expectrl per-operation timeouts. Revisit if
  runs surface warnings.
- File path `.config/nextest.toml` is the standard workspace-root location
  nextest looks up automatically.

## `release.sh` new `phase_test`

Deleted (~108 lines; net change after the new `phase_test` below is ~-50):

- `PHASE_TEST_JOBS` array (current L81-98)
- `PTY_LOCK_DIR` variable (L101)
- `_run_test_job` function (L105-115)
- `_run_all_tests_parallel` function (L119-161)
- Old `phase_test` body (L163-197)

New `phase_test`:

```bash
phase_test() {
  local dry_run=0
  if [[ "${1:-}" == "--dry-run" ]]; then
    dry_run=1
    shift
  fi

  if [[ $dry_run -eq 1 ]]; then
    echo "yosh-release: dry-run — would run 3 parallel jobs:" >&2
    echo "  nextest|cargo nextest run --workspace" >&2
    echo "  doctest|cargo test --doc --workspace" >&2
    echo "  e2e|./e2e/run_tests.sh" >&2
    return 0
  fi

  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"

  echo "yosh-release: pre-compiling test binaries..." >&2
  cargo nextest run --no-run --workspace \
    || fail "cargo nextest run --no-run failed — fix and rerun"

  local log_dir
  log_dir="$(mktemp -d -t yosh-parallel-tests.XXXXXX)"
  trap 'rm -rf "$log_dir"' EXIT INT TERM

  local nextest_log="$log_dir/nextest.log"
  local doctest_log="$log_dir/doctest.log"
  local e2e_log="$log_dir/e2e.log"

  echo "yosh-release: running nextest + doctest + e2e in parallel..." >&2
  echo "yosh-release: output is buffered (shown only on failure)" >&2

  ( cargo nextest run --workspace >"$nextest_log" 2>&1 ) &
  local pid_nextest=$!
  ( cargo test --doc --workspace >"$doctest_log" 2>&1 ) &
  local pid_doctest=$!
  ( ./e2e/run_tests.sh >"$e2e_log" 2>&1 ) &
  local pid_e2e=$!

  local -a failed
  wait "$pid_nextest" || failed+=("nextest:$nextest_log")
  wait "$pid_doctest" || failed+=("doctest:$doctest_log")
  wait "$pid_e2e"     || failed+=("e2e:$e2e_log")

  if [[ ${#failed[@]} -gt 0 ]]; then
    local entry name log
    local -a names
    for entry in "${failed[@]}"; do
      name="${entry%%:*}"
      log="${entry#*:}"
      names+=("$name")
      echo "--- $name output ---" >&2
      cat "$log" >&2
    done
    fail "tests failed: ${names[*]} — fix and rerun"
  fi

  echo "yosh-release: all tests passed" >&2
}
```

Design notes:

- `cargo nextest run --no-run --workspace` doubles as pre-compile and
  warm-up. It reduces first-run variance caused by cold compiler/FS caches
  (the root cause recorded in the existing variance TODO).
- Three inlined `( … ) &` invocations replace the 17-element job array. A
  hypothetical `_run_bg` helper was considered but abandoned — the three call
  sites read more clearly inlined.
- `failed` stores `"name:path"` pairs so both the human-readable name and the
  log path survive the `wait` loop together.
- The old `rmdir "$PTY_LOCK_DIR"` cleanup trap disappears — `[test-groups]`
  replaces the mkdir lock entirely.

## `CLAUDE.md` update

Current **Build & Test** section (lines 5-15) is replaced with:

```markdown
## Build & Test

This project uses [cargo-nextest](https://nexte.st/) for unit + integration tests.
Install via mise (see `mise.local.toml`) by running `mise install`, or manually
with `curl -LsSf https://get.nexte.st/latest/mac | tar zxf - -C $CARGO_HOME/bin`.

\`\`\`bash
cargo build                              # Debug build
cargo nextest run --workspace            # Unit + integration tests
cargo nextest run --test <name>          # Single test binary (e.g., interactive, signals, subshell)
cargo nextest run -E 'test(<pat>)'       # Filter by test name using the nextest filterset DSL
cargo test --doc --workspace             # Doctests (nextest does not support doctests)
./e2e/run_tests.sh                       # E2E POSIX compliance tests (requires debug build)
./e2e/run_tests.sh --filter=<pat>        # Filtered E2E tests
cargo bench                              # Criterion benchmarks
\`\`\`

Test configuration lives in `.config/nextest.toml`. The `pty_interactive`
binary is serialized via a `max-threads = 1` test group because its expectrl-based
tests share PTY state.
```

Other sections (`Architecture`, `Key Conventions`, `E2E Test Format`,
`TODO.md`, `PTY Tests`) are unchanged. The existing `PTY Tests` note already
says "use generous timeouts"; nextest's default slow-timeout does not
conflict.

## `TODO.md` update

Delete the line currently reading:

> `release.sh test` wall-time variance observation — after per-test-binary
> parallelization (2026-04-23), 3 back-to-back runs measured 95 s / 162 s /
> 178 s (±22 %, exceeds nominal ±20 % stability threshold). …

Reason: nextest migration re-baselines wall-time. Any residual variance becomes
a new observation and will be re-measured post-migration (see below).

The PTY flake line (`Full E2E suite occasional transient failures`) stays —
`retries = 0` means PTY flakes remain a latent failure mode.

## Measurement protocol

### Baseline (pre-migration)

1. Confirm working tree is clean on `main` with current `release.sh` intact.
2. Warm-up: `cargo build && cargo test --no-run --workspace`.
3. Run three consecutive timed invocations:
   `time .claude/skills/release/scripts/release.sh test`
4. Record each `real` wall-time; take the fastest of the three as the baseline.

### Post-migration

1. After the migration commit, `git status` clean.
2. Warm-up: `cargo build && cargo nextest run --no-run --workspace`.
3. Run three consecutive timed invocations:
   `time .claude/skills/release/scripts/release.sh test`
4. Record each `real` wall-time; take the fastest of the three as the new
   value.

### Rationale for "fastest of three"

All OS-level noise inflates wall-time in one direction only (extra scheduling,
FS cache eviction, background daemons). The fastest observation is the closest
approximation to the true cost of the work. This matches the existing advice
in `TODO.md` about warm-up bias.

## Commit plan

| # | Message prefix | Scope |
|---|---|---|
| 1 | `docs(spec)` | Add this design doc to `docs/superpowers/specs/` |
| 2 | `perf(release)` | Record baseline wall-time (3 runs, fastest) in commit message; no code change |
| 3 | `build(test)` | Add `.config/nextest.toml`; rewrite `phase_test`; delete PHASE_TEST_JOBS and helpers |
| 4 | `docs(claude)` | Update `CLAUDE.md` Build & Test section |
| 5 | `chore(todo)` | Drop the obsoleted `release.sh test` variance TODO |
| 6 | `perf(release)` | Record post-migration wall-time and speedup factor in commit message + in the Measurement Results section of this doc |

### Verification gates

- After commit 3: run `.claude/skills/release/scripts/release.sh test` once
  end-to-end. All three parallel jobs must exit 0.
- Before commit 6: three consecutive timed runs must all PASS. A failure
  (including a PTY flake under `retries = 0`) restarts the measurement set.

## Risks

- **`cargo nextest run --no-run --workspace` on a workspace member we have not
  exercised**: the `tests/plugins/test_plugin` crate is a workspace member but
  has no tests. nextest should treat it as a no-op; verify by dry-running the
  command before commit 3.
- **doctest expansion to `--workspace` surfaces a latent failure**: already
  mitigated — running `cargo test --doc -p <each crate>` during design showed
  0 doctests across all four publishable crates. Safe today; the change is a
  future-proofing gesture.
- **Test-group not applied**: verify with `cargo nextest show-config test-groups`
  after adding `.config/nextest.toml`. If `pty-serial` is not listed as covering
  `pty_interactive`, the filter string is wrong.

## Measurement results

Captured during implementation (2026-04-23 / 2026-04-24) on aarch64-apple-darwin,
8-core machine, after warm-up (`cargo build && cargo test --no-run --workspace`
for baseline, `cargo build && cargo nextest run --no-run --workspace` for
nextest).

### `release.sh test` wall time

| Run | Baseline (pre) — old bash-parallel | Post-migration (as shipped) |
|---|---|---|
| 1 | 373.9 s | 726.7 s |
| 2 | 406.0 s | *not collected — stopped for analysis* |
| 3 | 366.4 s | *not collected* |
| **Fastest** | **366.4 s** | (single data point, slower) |
| **Speedup** | — | **~0.5 × (≈ 2× slower)** |

Only one post-migration end-to-end run was collected before the slowdown was
clear; the remaining two were dropped in favor of diagnostic `cargo nextest`
invocations (next section) to isolate the cause.

### Isolated nextest measurements (no release.sh wrapper)

| Configuration | nextest wall (s) | Reported test time (s) | Notes |
|---|---|---|---|
| `retries=0` + `test-groups` only (shipped) | 327.7 | 35.3 | 1939 tests |
| `+ leak-timeout = "50ms"` (run A) | 533.0 | 5.2 | Test time collapses |
| `+ leak-timeout = "50ms"` (run B, same config) | 415.4 | 5.2 | Back-to-back variance ~100 s |
| `+ leak-timeout = "50ms" + test-threads = 16` | 623.8 | 5.4 | Oversubscription slowdown |
| `cargo test --workspace` (single-binary, no bash parallel) | 483.1 | n/a | Control |

CPU time (`user + sys`) was ~37 s in every nextest run. Wall-time swings
came entirely from OS-level scheduling and macOS security checks.

## Why nextest lost

1. **Per-process overhead is architectural.** nextest's book explicitly
   acknowledges macOS is slower and cites Anti-malware / Gatekeeper as the
   reason. There is no configuration option to switch to a process-per-binary
   model — it is a design invariant.
2. **yosh's test distribution is a worst case.** 1939 tests, of which
   >90 % are microsecond-scale unit tests in `yosh` crate's `--lib`. The fixed
   per-process cost (~150 ms) dwarfs the test work.
3. **The existing `release.sh` was already well-tuned.** It runs 16 parallel
   `cargo test --test <name>` invocations, each of which amortizes the
   per-process macOS tax over hundreds or thousands of intra-binary threaded
   tests. This is effectively "process-per-binary" with thread-parallel
   inside — structurally faster than "process-per-test" on macOS.
4. **Optimizations had secondary effects.** `leak-timeout` shortening cut
   *reported* test time but did not reduce wall time (dominated by OS noise).
   `test-threads` oversubscription made things worse on this 8-core box.

## What was kept from this work

- The baseline commit `514c3f3 perf(release): record baseline phase_test wall
  time pre-nextest` — standalone record of the 366.4 s fastest-of-three baseline.
  Useful for future performance-tracking commits.
- This design document and the implementation plan
  (`docs/superpowers/plans/2026-04-23-nextest-migration.md`), kept as
  historical record of a considered-and-rejected migration. Future engineers
  investigating nextest for this workspace can skip the measurement work.
- The TODO.md `release.sh test` wall-time variance entry (restored by the
  `chore(todo)` revert), since variance was observed pre-nextest and remains
  an open observation on the bash-parallel model.

## Follow-ups

- Consider refactoring `tests/plugin.rs` to not invoke `cargo build` inside
  tests (currently uses a static `TEST_LOCK: Mutex<()>` and shells out per
  test). Independent of runner choice, this is brittle. Not urgent.
- If CI is introduced later, re-measure on Linux (GitHub Actions ubuntu
  runners). The per-process overhead is smaller on Linux and nextest may be
  competitive there; this analysis is macOS-specific.
