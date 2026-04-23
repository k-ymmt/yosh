# cargo-nextest Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bash parallel-job orchestration in `release.sh phase_test` with `cargo-nextest`, measure wall-time before/after, and update `CLAUDE.md` + `TODO.md` accordingly.

**Architecture:** Add `.config/nextest.toml` (PTY serialized via `[test-groups]`). Rewrite `phase_test` to run three parallel jobs — `cargo nextest run --workspace`, `cargo test --doc --workspace`, `./e2e/run_tests.sh` — replacing the 17-job array + mkdir lock. Measure via warm-up + fastest-of-three. Spec: `docs/superpowers/specs/2026-04-23-nextest-migration-design.md`.

**Tech Stack:** Bash (`.claude/skills/release/scripts/release.sh`), `cargo-nextest 0.9.x` (installed via mise), Rust workspace (`yosh`, `yosh-plugin-api`, `yosh-plugin-sdk`, `yosh-plugin-manager`).

---

## File map

- `.config/nextest.toml` — new, workspace-root nextest config (Task 2)
- `.claude/skills/release/scripts/release.sh` — rewrite `phase_test` and delete legacy orchestration (Task 2)
- `CLAUDE.md` — **Build & Test** section replaced with nextest-based commands and install note (Task 3)
- `TODO.md` — drop the `release.sh test` variance line (Task 4)
- `docs/superpowers/specs/2026-04-23-nextest-migration-design.md` — fill the Measurement Results table (Task 5)

---

## Pre-flight

Run these once before Task 1 to ensure the environment is sane:

- [ ] **Step P1: Confirm cargo-nextest is installed**

Run: `cargo nextest --version`
Expected: `cargo-nextest 0.9.x …` (mise-provided).
If missing, run `mise install` from the repo root and retry.

- [ ] **Step P2: Confirm working tree is clean on main**

Run: `git status --porcelain && git branch --show-current`
Expected: empty `status --porcelain` output; branch `main`.

- [ ] **Step P3: Confirm HEAD is at the spec commits**

Run: `git log --oneline -3`
Expected: top two commits are `docs(spec): reconcile line-count numbers in nextest migration design` and `docs(spec): add cargo-nextest migration design`.

---

### Task 1: Record baseline wall-time (old `release.sh`)

**Files:**
- No source changes.
- Create empty commit with three baseline measurements in the message.

**Why this task first:** The old `phase_test` must still be intact to measure the pre-migration baseline.

- [ ] **Step 1: Warm up filesystem + compiler caches**

Run: `cargo build && cargo test --no-run --workspace`
Expected: exit 0. The second command may print "Finished `test` profile …" and produce many test binaries under `target/debug/deps/`. Ignore wall time of the warm-up run itself.

- [ ] **Step 2: Record three timed runs of the old `phase_test`**

For each run (repeat three times), execute:

```bash
/usr/bin/time -p .claude/skills/release/scripts/release.sh test 2>&1 | tail -5
```

Copy the `real` value (first line of `/usr/bin/time -p` output) of each run. If any run exits non-zero, discard it (including the warm-up state it leaves behind) and rerun after a fresh `cargo build && cargo test --no-run --workspace` warm-up.

Expected per successful run: script prints `yosh-release: all tests passed` near the end and exits 0. `/usr/bin/time -p` prints `real <seconds>`.

Record the three `real` values and identify the fastest.

- [ ] **Step 3: Create an empty commit recording the baseline**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
perf(release): record baseline phase_test wall time pre-nextest

Three back-to-back runs of the current `release.sh test` (16-job bash
parallel + mkdir PTY lock) after `cargo build && cargo test --no-run
--workspace` warm-up:

  run 1: <REAL_1> s
  run 2: <REAL_2> s
  run 3: <REAL_3> s
  fastest: <FASTEST> s

Captured as the pre-migration baseline referenced by
docs/superpowers/specs/2026-04-23-nextest-migration-design.md
for comparison against the post-migration wall time in a later commit.
EOF
)"
```

Before running the command, substitute `<REAL_1>`, `<REAL_2>`, `<REAL_3>`, `<FASTEST>` with the values recorded in Step 2.

Expected: `git log -1 --oneline` shows `perf(release): record baseline phase_test wall time pre-nextest`.

---

### Task 2: Add `.config/nextest.toml` and rewrite `phase_test`

**Files:**
- Create: `.config/nextest.toml`
- Modify: `.claude/skills/release/scripts/release.sh` — replace lines 81-197 (the job array, helpers, and `phase_test`) with the new `phase_test`

- [ ] **Step 1: Create `.config/nextest.toml`**

Create `.config/nextest.toml` with the following content exactly:

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

- [ ] **Step 2: Verify nextest parses the config and applies the override**

Run: `cargo nextest show-config test-groups`

Expected: output lists a `pty-serial` group with `max-threads = 1`. Under it, the `pty_interactive` test binary's tests are listed as assigned to `pty-serial` (there are 20 PTY tests per the existing TODO; the exact count does not matter — only that `pty_interactive::…` entries appear under the group).

If no test-groups output appears or `pty_interactive` tests are not listed under `pty-serial`, inspect the filter string and fix before proceeding.

- [ ] **Step 3: Pre-flight `cargo nextest run --no-run --workspace`**

Run: `cargo nextest run --no-run --workspace`
Expected: exit 0. Output ends with something like `Compiling … Finished … test binaries built`. If it fails (e.g., a workspace member cannot be compiled for tests), stop and diagnose — the new `phase_test` calls the same command.

- [ ] **Step 4: Rewrite `phase_test` in `release.sh`**

Open `.claude/skills/release/scripts/release.sh`. Locate the block that starts at `# Job list for parallel test execution.` (line 78 in the current file) and ends at `echo "yosh-release: all tests passed" >&2` in `phase_test` (line 196). Replace the entire block — lines 78-197 inclusive — with the following exactly:

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

- [ ] **Step 5: Verify release.sh still parses and dispatches correctly**

Run: `bash -n .claude/skills/release/scripts/release.sh`
Expected: exit 0 with no output (syntax OK).

Run: `.claude/skills/release/scripts/release.sh test --dry-run`
Expected: stderr prints

```
yosh-release: dry-run — would run 3 parallel jobs:
  nextest|cargo nextest run --workspace
  doctest|cargo test --doc --workspace
  e2e|./e2e/run_tests.sh
```

Exit 0.

- [ ] **Step 6: End-to-end smoke-run of the new `phase_test`**

Run: `.claude/skills/release/scripts/release.sh test`
Expected: exit 0. stderr ends with `yosh-release: all tests passed`. No PTY lock / job-array error messages.

If a PTY flake occurs (`retries = 0`), rerun once — a single flake does not invalidate the migration. If it flakes repeatedly, stop and investigate before committing.

- [ ] **Step 7: Commit the migration**

```bash
git add .config/nextest.toml .claude/skills/release/scripts/release.sh
git commit -m "$(cat <<'EOF'
build(test): adopt cargo-nextest for workspace tests

Replace the hand-rolled 16-job bash parallel runner + mkdir PTY lock in
release.sh's phase_test with cargo-nextest. PTY serialization now lives
in .config/nextest.toml as a test-group scoped to binary(pty_interactive).
phase_test runs three parallel jobs: nextest, doctest (expanded to
--workspace as a future-proofing gesture; all crates currently have 0
doctests), and e2e/run_tests.sh.

Net release.sh diff: ~108 lines deleted, ~55 added. PHASE_TEST_JOBS,
_run_test_job, _run_all_tests_parallel, and PTY_LOCK_DIR are gone.

Design: docs/superpowers/specs/2026-04-23-nextest-migration-design.md

Original prompt: 'https://nexte.st/ を参考に nextest の実装方法をまとめ、
cargo nextest に移行してください。また、それに伴い既存からどれくらい
早くなるか計測してください。CLAUDE.md や release Skill を更新してください。'
EOF
)"
```

Expected: `git log -1 --oneline` shows `build(test): adopt cargo-nextest for workspace tests`.

---

### Task 3: Update `CLAUDE.md` Build & Test section

**Files:**
- Modify: `CLAUDE.md:5-15` (the **Build & Test** fenced block and surrounding heading)

- [ ] **Step 1: Replace the Build & Test section**

Open `CLAUDE.md`. The current section is:

```markdown
## Build & Test

\`\`\`bash
cargo build                          # Debug build
cargo test                           # Unit + integration tests
cargo test --test <name>             # Single test file (e.g., interactive, signals, subshell)
cargo test <test_name>               # Single test by name
./e2e/run_tests.sh                   # E2E POSIX compliance tests (requires debug build)
./e2e/run_tests.sh --filter=<pat>    # Filtered E2E tests
cargo bench                          # Criterion benchmarks
\`\`\`
```

Replace it with:

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

- [ ] **Step 2: Verify the diff**

Run: `git diff CLAUDE.md`
Expected: the Build & Test section changes from the 7-command list to the 8-command list plus two new prose paragraphs. No other sections touched.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "$(cat <<'EOF'
docs(claude): switch test commands to cargo-nextest

Build & Test section now documents cargo-nextest as the runner, with
a mise-based install note, the filterset DSL syntax for name filters,
and an explicit line for cargo test --doc --workspace since nextest
does not run doctests. Also documents .config/nextest.toml and the
pty-serial test-group.
EOF
)"
```

Expected: `git log -1 --oneline` shows `docs(claude): switch test commands to cargo-nextest`.

---

### Task 4: Drop the obsolete `release.sh test` variance TODO

**Files:**
- Modify: `TODO.md` — delete the single-line entry describing `release.sh test` wall-time variance

- [ ] **Step 1: Locate the entry**

Run: `grep -n "release.sh.*test.*wall-time variance" TODO.md`
Expected: one hit. Note the line number.

- [ ] **Step 2: Delete the entry**

Remove the entire bullet — from its leading `- [ ] ` through the trailing `(`.claude/skills/release/scripts/release.sh`).` on the same logical line. The entry is a single Markdown list item; preserve all other list items and headings around it.

Concretely, the bullet begins with:

```
- [ ] `release.sh test` wall-time variance observation — after per-test-binary parallelization (2026-04-23), 3 back-to-back runs measured 95 s / 162 s / 178 s …
```

…and ends with:

```
… before timed measurements to reduce first-run bias (`.claude/skills/release/scripts/release.sh`).
```

Delete exactly that bullet.

- [ ] **Step 3: Verify the diff removes only the intended lines**

Run: `git diff TODO.md`
Expected: only the single bullet is deleted. No other TODO items are touched. Surrounding entries (e.g., `YOSH_E2E_NO_TIMEOUT` help wording) remain.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): drop release.sh test variance TODO obsoleted by nextest

The ±22 % variance entry measured the previous bash-parallel phase_test.
cargo-nextest replaces that orchestration, so the observation is stale.
Post-migration variance, if any, is captured in the Measurement Results
section of the migration design doc.
EOF
)"
```

Expected: `git log -1 --oneline` shows `chore(todo): drop release.sh test variance TODO obsoleted by nextest`.

---

### Task 5: Record post-migration wall-time and fill the design doc

**Files:**
- Modify: `docs/superpowers/specs/2026-04-23-nextest-migration-design.md` (Measurement Results table at the bottom)

- [ ] **Step 1: Warm up filesystem + compiler caches**

Run: `cargo build && cargo nextest run --no-run --workspace`
Expected: exit 0. Compilation is likely a no-op (Task 2's smoke-run already built everything), but run it anyway so FS caches are warm.

- [ ] **Step 2: Record three timed runs of the new `phase_test`**

For each run (repeat three times), execute:

```bash
/usr/bin/time -p .claude/skills/release/scripts/release.sh test 2>&1 | tail -5
```

Copy the `real` value for each run. Discard and rerun any failed run (rewarm if needed). All three measured runs must PASS.

Expected per successful run: script prints `yosh-release: all tests passed` and exits 0.

- [ ] **Step 3: Fill the Measurement Results table**

Open `docs/superpowers/specs/2026-04-23-nextest-migration-design.md` and locate the final section:

```markdown
## Measurement results

*To be populated after commits 2 and 6.*

| Run | Baseline (pre) | Post-migration |
|---|---|---|
| 1 | TBD | TBD |
| 2 | TBD | TBD |
| 3 | TBD | TBD |
| **Fastest** | TBD | TBD |
| **Speedup** | — | TBD × |
```

Replace it with the actual numbers. Substitute:
- `<BASELINE_1..3>` from Task 1 Step 2.
- `<POST_1..3>` from Task 5 Step 2.
- `<BASELINE_FASTEST>` / `<POST_FASTEST>` from the fastest of each set.
- `<SPEEDUP>` computed as `<BASELINE_FASTEST> / <POST_FASTEST>` rounded to two decimals.

Also remove the italicized `*To be populated after commits 2 and 6.*` line.

Example (replace with real numbers):

```markdown
## Measurement results

| Run | Baseline (pre) | Post-migration |
|---|---|---|
| 1 | 95.2 s | 58.4 s |
| 2 | 162.1 s | 61.0 s |
| 3 | 178.4 s | 59.8 s |
| **Fastest** | **95.2 s** | **58.4 s** |
| **Speedup** | — | **1.63 ×** |
```

- [ ] **Step 4: Verify the diff**

Run: `git diff docs/superpowers/specs/2026-04-23-nextest-migration-design.md`
Expected: only the Measurement Results table and the italicized placeholder line change. Numbers are real values, no `TBD` remains.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-04-23-nextest-migration-design.md
git commit -m "$(cat <<'EOF'
perf(release): record post-nextest wall time and speedup

Three back-to-back runs of the new `release.sh test` (cargo-nextest +
--doc --workspace + e2e in parallel) after `cargo build && cargo nextest
run --no-run --workspace` warm-up:

  run 1: <POST_1> s
  run 2: <POST_2> s
  run 3: <POST_3> s
  fastest: <POST_FASTEST> s

Baseline fastest: <BASELINE_FASTEST> s (see prior commit `perf(release):
record baseline phase_test wall time pre-nextest`).

Speedup: <SPEEDUP> × over the pre-nextest 16-job bash parallel runner.

Measurement Results table in
docs/superpowers/specs/2026-04-23-nextest-migration-design.md updated
to reflect the final numbers.
EOF
)"
```

Substitute the placeholders with the same values used in Step 3 before running the command.

Expected: `git log -1 --oneline` shows `perf(release): record post-nextest wall time and speedup`.

---

## Post-implementation housekeeping (optional, non-blocking)

- [ ] **Update auto-memory entry**

File: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/feedback_cargo_build_slow.md`

This memory currently says `cargo build 1-3min; full cargo test ~28min (PTY/subprocess-heavy). Always background full suites.` After the migration, the "28 min" figure refers to pre-parallelization and is doubly stale. Update the body to reflect the new fastest wall time (from Task 5) and note the runner is cargo-nextest.

Do not commit memory files — they live under `~/.claude/` and are not part of the repo.

---

## Plan self-review

- **Spec coverage**:
  - `.config/nextest.toml` → Task 2 Step 1.
  - `release.sh phase_test` rewrite → Task 2 Step 4.
  - Deletion of `PHASE_TEST_JOBS` / `_run_test_job` / `_run_all_tests_parallel` / `PTY_LOCK_DIR` → Task 2 Step 4 (replacement block subsumes them all).
  - `CLAUDE.md` Build & Test → Task 3.
  - `TODO.md` variance line → Task 4.
  - Baseline measurement → Task 1.
  - Post-migration measurement + doc table → Task 5.
  - All six spec commits are mapped 1:1 (spec commit 1 already done before this plan; commits 2-6 are Tasks 1-5 in order).
- **Placeholder scan**: `<REAL_1>` etc. are user-provided measurement values inside commit-message templates — these are instructions, not TODOs. Every code block has literal content; every command has expected output. No `TBD`, `fill in details`, or `similar to Task N` remain.
- **Type consistency**: Bash variable names (`failed`, `names`, `pid_*`, `log_dir`) appear only in Task 2 Step 4 and are self-contained to the new `phase_test` function. TOML keys (`retries`, `test-groups`, `pty-serial`, `max-threads`, `filter`, `test-group`) match nexte.st documentation verbatim.
