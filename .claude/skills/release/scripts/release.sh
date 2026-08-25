#!/usr/bin/env bash
# Deterministic release phases for yosh.
# Invoked by .claude/skills/release/SKILL.md, or runnable standalone for recovery.
# Usage:
#   release.sh test
#   release.sh bump
#   release.sh publish [--from <crate-name>]
#   release.sh publish-wit
#   release.sh push

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
cd "${REPO_ROOT}"

# Order matters for `cargo publish`: each crate is published before its
# dependents. With the v0.2.0 wasmtime migration:
#   yosh-plugin-api  — leaf (the WIT package + Capability enum)
#   yosh-plugin-sdk  — depends on -api
#   yosh-plugin-manager — depends on -api (via wasmtime bindgen!)
#   yosh            — depends on -api and -manager
# So this is a true dependency-ordered list, not a convention.
CRATES=(yosh-plugin-api yosh-plugin-sdk yosh-plugin-manager yosh)

fail() {
  echo "yosh-release: $*" >&2
  exit 1
}

# Extract [package].version from a Cargo.toml file.
# Scans only within the [package] section to avoid matching dependency versions.
read_package_version() {
  local file="$1"
  awk '
    /^\[package\]/ { in_pkg = 1; next }
    /^\[/ && !/^\[package\]/ { in_pkg = 0 }
    in_pkg && /^version = "/ {
      sub(/^version = "/, "")
      sub(/"$/, "")
      print
      exit
    }
  ' "$file"
}

# Rewrite [package].version (first match under [package]) from old -> new.
rewrite_package_version() {
  local file="$1" old="$2" new="$3"
  awk -v old="$old" -v new="$new" '
    BEGIN { in_pkg = 0; done = 0 }
    /^\[package\]/ { in_pkg = 1; print; next }
    /^\[/ && !/^\[package\]/ { in_pkg = 0 }
    in_pkg && !done && $0 == ("version = \"" old "\"") {
      print "version = \"" new "\""
      done = 1
      next
    }
    { print }
    END { if (!done) exit 1 }
  ' "$file" > "$file.tmp" || { rm -f "$file.tmp"; return 1; }
  mv "$file.tmp" "$file"
}

# Rewrite `<dep> = { version = "<old>"` to `<dep> = { version = "<new>"` (all occurrences).
# Targets workspace crate pins; safe because the string includes the dep name.
rewrite_dep_version() {
  local file="$1" dep="$2" old="$3" new="$4"
  local from="${dep} = { version = \"${old}\""
  local to="${dep} = { version = \"${new}\""
  awk -v from="$from" -v to="$to" '
    {
      # Literal string replace (gsub treats regex metachars, so we escape via index loop).
      out = ""
      rest = $0
      while ( (p = index(rest, from)) > 0 ) {
        out = out substr(rest, 1, p-1) to
        rest = substr(rest, p + length(from))
      }
      print out rest
    }
  ' "$file" > "$file.tmp" || { rm -f "$file.tmp"; return 1; }
  mv "$file.tmp" "$file"
}

# Compute SHA-256 of a WIT file's content with the `package yosh:plugin@<x>;`
# declaration line stripped. The version line is rewritten by phase_publish_wit
# itself; excluding it makes SHA equality strictly track interface equality.
# Output: 64 hex chars on stdout, no trailing newline beyond what `cut` emits.
compute_wit_content_sha() {
  local wit="$1"
  grep -v '^package yosh:plugin@' "$wit" | shasum -a 256 | cut -d' ' -f1
}

# Job list for parallel test execution. Format: "name|group|cargo-args..."
# group: "pty" = mutually-exclusive serial slot via mkdir lock — for tests
# that race on shared resources (real PTYs, signal delivery, foreground
# process group). "free" = unbounded parallel.
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
  "plugin|free|test --test plugin --features test-helpers"
  "plugin_cli_help|free|test --test plugin_cli_help"
  "signals|pty|test --test signals"
  "subshell|free|test --test subshell"
  "pty_interactive|pty|test --test pty_interactive"
  "pty_posix|pty|test --test pty_posix"
)

# Set by phase_test at invocation time. Absent path = unlocked, present = held.
PTY_LOCK_DIR=""

# Run one test job. Locks the PTY group via mkdir. Writes output to $log.
# Args: $1=name  $2=group  $3=log  $4..=cargo args
_run_test_job() {
  local name="$1" group="$2" log="$3"
  shift 3

  # Per-job worker-thread cap. Without this, each libtest binary defaults to
  # num_cpus threads, producing ~17 × 8 ≈ 130 concurrent test threads on 8
  # cores — intermittent timeouts in signals/PTY/e2e tests.
  #
  # Split by group: free-group jobs use 4 (≈7× total oversubscription, fast
  # enough not to choke rustdoc-heavy `doc`). PTY-group jobs use 2 because
  # they drive yosh through expectrl with a 15s prompt-wait budget
  # (tests/helpers/pty.rs) — the serial lock only excludes other PTY jobs,
  # so 14 free jobs are still consuming CPU while a PTY job runs, and the
  # budget breaks at threads=4 (every test times out).
  local threads=4
  local t0 t1 t2
  t0="$(date +%s)"
  if [[ "$group" == "pty" ]]; then
    while ! mkdir "$PTY_LOCK_DIR" 2>/dev/null; do sleep 0.05; done
    trap 'rmdir "$PTY_LOCK_DIR" 2>/dev/null' EXIT
    threads=2
  fi
  t1="$(date +%s)"

  local rc=0
  RUST_TEST_THREADS="$threads" cargo "$@" >"$log" 2>&1 || rc=$?
  t2="$(date +%s)"
  # Per-job wall-time record: name, lock-wait, run, total (seconds).
  # Appends are short single lines to an O_APPEND fd — safe across the
  # concurrent job subshells.
  printf '%s\t%s\t%s\t%s\n' "$name" "$((t1 - t0))" "$((t2 - t1))" "$((t2 - t0))" \
    >>"$(dirname "$log")/durations.tsv"
  return "$rc"
}

# Print the per-job duration table (sorted by total desc) to stderr.
_print_job_durations() {
  local tsv="$1"
  [[ -f "$tsv" ]] || return 0
  echo "yosh-release: per-job wall times (lock-wait / run / total):" >&2
  sort -t$'\t' -k4 -rn "$tsv" \
    | awk -F'\t' '{ printf "  %-18s %5ss %6ss %6ss\n", $1, $2, $3, $4 }' >&2
}

# Launch all jobs in PHASE_TEST_JOBS plus e2e in parallel, wait, aggregate.
# Prints only failed jobs' logs; fails the script with a summary on any failure.
_run_all_tests_parallel() {
  local log_dir
  log_dir="$(mktemp -d -t yosh-parallel-tests.XXXXXX)"
  # Trap is a safety net for signals (INT/TERM) and abrupt exits (set -e
  # mid-run, fail() inside this function). The success path below clears the
  # trap explicitly: $log_dir is function-local, so a deferred EXIT firing
  # after we return would expand it as unset under `set -u`.
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

  # e2e as an additional parallel job alongside the cargo jobs. Bump the
  # per-test budget from the standalone default (15s): under release-time
  # contention, even trivially fast tests can stall past 15s while the
  # cargo jobs hog CPU. 45s is well above the worst observed stall and
  # still tight enough to surface real hangs.
  local e2e_log="$log_dir/e2e.log"
  (
    t0="$(date +%s)"
    rc=0
    YOSH_E2E_TIMEOUT=45 ./e2e/run_tests.sh >"$e2e_log" 2>&1 || rc=$?
    t1="$(date +%s)"
    printf 'e2e\t0\t%s\t%s\n' "$((t1 - t0))" "$((t1 - t0))" \
      >>"$log_dir/durations.tsv"
    exit "$rc"
  ) &
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

  _print_job_durations "$log_dir/durations.tsv"

  if [[ ${#failed[@]} -gt 0 ]]; then
    for i in "${failed[@]}"; do
      echo "--- ${names[$i]} output ---" >&2
      cat "${logs[$i]}" >&2
    done
    local -a failed_names
    for i in "${failed[@]}"; do failed_names+=("${names[$i]}"); done
    fail "tests failed: ${failed_names[*]} — fix and rerun"
  fi

  rm -rf "$log_dir"
  rmdir "$PTY_LOCK_DIR" 2>/dev/null || true
  trap - EXIT INT TERM
}

# Refuse to start the test phase on an already-loaded machine. The suite's
# wall-clock budgets (1s plugin-exec, 15s PTY prompt, 45s e2e) assume the
# parallel window is the dominant load; a pre-existing CPU hog pushes
# process-spawn latency into the 10-45s range and fails those tests on
# timing they never meant to assert. Real case (2026-07-11/12): eight
# orphaned `yes` processes from a debugging session saturated all 8 cores
# for a day and were misdiagnosed as a dyld-contention flake across two
# release attempts. Override with YOSH_RELEASE_SKIP_LOAD_CHECK=1.
check_baseline_load() {
  [[ "${YOSH_RELEASE_SKIP_LOAD_CHECK:-0}" == "1" ]] && return 0
  local max_load="${YOSH_RELEASE_MAX_LOAD:-4}"
  local load1
  load1="$(sysctl -n vm.loadavg 2>/dev/null | awk '{print int($2)}')"
  [[ -n "$load1" ]] || return 0  # can't sample (non-macOS) — skip the guard
  if (( load1 >= max_load )); then
    echo "yosh-release: 1-min load average is ${load1} (limit ${max_load}) BEFORE tests start." >&2
    echo "yosh-release: top CPU consumers:" >&2
    ps -Ao pcpu,pid,lstart,comm -r | head -6 >&2
    fail "machine is already loaded — kill stray processes (or YOSH_RELEASE_SKIP_LOAD_CHECK=1 to override)"
  fi
}

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

  check_baseline_load

  local ts_start ts_build ts_precompile ts_tests ts_end
  ts_start="$(date +%s)"

  echo "yosh-release: building debug binary for e2e..." >&2
  cargo build || fail "cargo build failed — fix and rerun"
  ts_build="$(date +%s)"

  # Pre-compile each job's exact cargo invocation. A single
  # `cargo test --no-run --workspace` is NOT equivalent: --workspace
  # unifies features across every member's dev-deps (mockito -> tokio/
  # hyper via yosh-plugin-manager, wit-bindgen -> wasmparser via the
  # wasm test plugins), producing different build variants of tokio/
  # wasmtime/wasmtime-wasi than the per-job `-p yosh` / `--features
  # test-helpers` graphs. The 2026-07-11 release run precompiled only
  # the workspace variant, so the jobs rebuilt their own variants
  # DURING the parallel test window (7-17 min of compile + cargo-lock
  # serialization), starving the 5-15s timeout budgets in signals/PTY/
  # plugin-exec tests. Precompiling per-job guarantees a compile-free
  # test window and self-adapts as PHASE_TEST_JOBS evolves.
  echo "yosh-release: pre-compiling test binaries (per job)..." >&2
  local job name group cmd
  for job in "${PHASE_TEST_JOBS[@]}"; do
    IFS='|' read -r name group cmd <<<"$job"
    # `cargo test --doc` rejects --no-run, and its doctest lib unit is
    # NOT produced by `--lib --no-run` (verified 2026-07-11: one 10s
    # compile leaked into the test window). Doctests are currently
    # zero, so just run the doc job here to warm its cache.
    if [[ "$name" == "doc" ]]; then
      # shellcheck disable=SC2086
      cargo $cmd >/dev/null 2>&1 \
        || fail "cargo $cmd failed during precompile — fix and rerun"
      continue
    fi
    # shellcheck disable=SC2086  # cmd is intentionally word-split
    cargo $cmd --no-run \
      || fail "cargo $cmd --no-run failed — fix and rerun"
  done
  ts_precompile="$(date +%s)"

  # Reserve a unique lock path. mktemp -d creates it; rmdir removes it so the
  # path is absent on entry. Absent = unlocked, present = held.
  PTY_LOCK_DIR="$(mktemp -d -t yosh-pty-lock.XXXXXX)"
  rmdir "$PTY_LOCK_DIR"

  echo "yosh-release: running ${#PHASE_TEST_JOBS[@]} test jobs + e2e in parallel..." >&2
  # Wall-time expectation (measured 2026-07-19 with per-job instrumentation):
  # ~90 s fully warm, ~3.5 min after a dependency rebuild. Critical path is
  # e2e (~86 s); if this drifts past ~5 min, read the per-job table below.
  echo "yosh-release: output is buffered (shown only on failure); typically ~2-4 min (longer after big rebuilds)" >&2
  _run_all_tests_parallel
  ts_tests="$(date +%s)"

  # Catch packaging-time bugs that workspace builds miss — e.g. bindgen!
  # macros with workspace-relative paths that resolve in the workspace
  # but not after `cargo install` extracts the crate standalone.
  # `cargo publish --dry-run` packages, then compiles the packaged tarball
  # in isolation, which is exactly what crates.io users hit.
  #
  # yosh (root) and yosh-plugin-manager are skipped here: their dry-run
  # resolves sibling workspace deps against the registry, so whenever this
  # release adds new API surface to yosh-plugin-api (or yosh-plugin-sdk),
  # verify fails because the not-yet-published version is the only one that
  # contains the new items. The remaining leaf check on yosh-plugin-api
  # (no internal deps) is sufficient to catch the standalone-build bug
  # class this guards against.
  echo "yosh-release: dry-run packaging api/sdk to catch standalone-build bugs..." >&2
  local check
  for check in yosh-plugin-api yosh-plugin-sdk; do
    cargo publish --dry-run --allow-dirty -p "$check" >/dev/null 2>&1 \
      || fail "cargo publish --dry-run failed for $check — rerun manually for full output: cargo publish --dry-run --allow-dirty -p $check"
  done
  ts_end="$(date +%s)"

  echo "yosh-release: phase timing: build=$((ts_build - ts_start))s precompile=$((ts_precompile - ts_build))s parallel-tests=$((ts_tests - ts_precompile))s package-dry-run=$((ts_end - ts_tests))s total=$((ts_end - ts_start))s" >&2
  echo "yosh-release: all tests passed" >&2
}

phase_bump() {
  local manifests=(
    "Cargo.toml"
    "crates/yosh-plugin-api/Cargo.toml"
    "crates/yosh-plugin-sdk/Cargo.toml"
    "crates/yosh-plugin-manager/Cargo.toml"
  )

  local old
  old="$(read_package_version "Cargo.toml")"
  [[ -n "$old" ]] || fail "could not read version from Cargo.toml"

  # Verify all crates share the same version.
  local m ver
  for m in "${manifests[@]}"; do
    ver="$(read_package_version "$m")"
    [[ "$ver" == "$old" ]] || fail "version mismatch: $m has '$ver', expected '$old'. Reconcile manually."
  done

  # Compute new patch version.
  local new
  new="$(awk -F. -v v="$old" 'BEGIN { split(v, a, "."); print a[1] "." a[2] "." a[3] + 1 }')"
  [[ -n "$new" ]] || fail "could not compute new version from '$old'"

  echo "yosh-release: bumping $old -> $new across ${#manifests[@]} manifests" >&2

  # Rewrite [package].version in each manifest.
  for m in "${manifests[@]}"; do
    rewrite_package_version "$m" "$old" "$new" \
      || fail "failed to rewrite [package].version in $m — run 'git checkout Cargo.toml crates/*/Cargo.toml' to revert"
  done

  # Rewrite workspace dep pins (yosh-plugin-api is pinned in root and in sdk;
  # yosh-plugin-manager is pinned in root so `cargo install yosh` can bundle
  # the yosh-plugin bin via a versioned crates.io dependency).
  rewrite_dep_version "Cargo.toml" "yosh-plugin-api" "$old" "$new" \
    || fail "failed to rewrite yosh-plugin-api pin in Cargo.toml"
  rewrite_dep_version "crates/yosh-plugin-sdk/Cargo.toml" "yosh-plugin-api" "$old" "$new" \
    || fail "failed to rewrite yosh-plugin-api pin in yosh-plugin-sdk/Cargo.toml"
  rewrite_dep_version "Cargo.toml" "yosh-plugin-manager" "$old" "$new" \
    || fail "failed to rewrite yosh-plugin-manager pin in Cargo.toml"

  # Refresh Cargo.lock.
  echo "yosh-release: refreshing Cargo.lock (cargo build)..." >&2
  cargo build \
    || fail "cargo build failed after bump — check diff, run 'git checkout Cargo.toml crates/*/Cargo.toml Cargo.lock' to revert"

  # Commit.
  git add Cargo.toml crates/yosh-plugin-api/Cargo.toml crates/yosh-plugin-sdk/Cargo.toml crates/yosh-plugin-manager/Cargo.toml Cargo.lock
  git commit -m "chore: release v${new}

- yosh, yosh-plugin-api, yosh-plugin-sdk, yosh-plugin-manager: ${old} -> ${new}" \
    || fail "git commit failed after bump — resolve manually and rerun 'release.sh bump'"

  echo "yosh-release: bump complete ($old -> $new, committed)" >&2
  # Expose new version to callers. Format is stable for grep: `NEW_VERSION=<ver>`.
  echo "NEW_VERSION=$new"
}

phase_publish() {
  local from=""
  if [[ "${1:-}" == "--from" ]]; then
    from="${2:-}"
    [[ -n "$from" ]] || fail "--from requires a crate name (one of: ${CRATES[*]})"
    shift 2 || true
  fi

  # Validate --from value if given.
  if [[ -n "$from" ]]; then
    local valid=0
    local c
    for c in "${CRATES[@]}"; do
      [[ "$c" == "$from" ]] && valid=1 && break
    done
    [[ $valid -eq 1 ]] || fail "--from '$from' is not a known crate (expected one of: ${CRATES[*]})"
  fi

  local started=0
  [[ -z "$from" ]] && started=1

  local crate cmd
  for crate in "${CRATES[@]}"; do
    if [[ $started -eq 0 ]]; then
      if [[ "$crate" == "$from" ]]; then
        started=1
      else
        echo "yosh-release: skipping $crate (resuming from $from)" >&2
        continue
      fi
    fi

    echo "yosh-release: publishing $crate..." >&2
    cmd=(cargo publish --no-verify -p "$crate")

    if ! "${cmd[@]}"; then
      cat >&2 <<EOF
yosh-release: 'cargo publish' failed for $crate.
  - Earlier crates in this run are already on crates.io and cannot be unpublished (only yanked).
  - After fixing the cause, resume with:
      .claude/skills/release/scripts/release.sh publish --from $crate
  - If you need to restart from the beginning of the publish phase:
      .claude/skills/release/scripts/release.sh publish
    (earlier crates will fail with 'already published' — use --from instead.)
EOF
      exit 1
    fi
  done

  echo "yosh-release: all crates published" >&2
}

phase_publish_wit() {
  local wit="crates/yosh-plugin-api/wit/yosh-plugin.wit"
  local sha_file="crates/yosh-plugin-api/.last-published-wit.sha256"
  # The canonical WIT is mirrored verbatim in three more places; build.rs
  # byte-comparison checks fail if the version rewrite touches only the
  # canonical copy.
  local wit_mirrors=(
    "wit/yosh-plugin.wit"
    "crates/yosh-plugin-sdk/wit/yosh-plugin.wit"
    "crates/yosh-plugin-manager/wit/yosh-plugin.wit"
  )

  # 1. wkg available?
  command -v wkg >/dev/null \
    || fail "wkg not found in PATH. Install via 'cargo install wkg --locked'"

  # 1b. wasm-tools available? Needed to strip the `package-docs` custom
  # section from the built WIT wasm: wkg 0.15 emits package-docs v1, but
  # wa.dev currently rejects anything other than v0 with
  # "expected package-docs version 0, got Some(1)".
  command -v wasm-tools >/dev/null \
    || fail "wasm-tools not found in PATH. Install via 'cargo install wasm-tools --locked'"

  # 2. Crate version (must already be bumped by phase_bump).
  local crate_ver
  crate_ver="$(read_package_version "crates/yosh-plugin-api/Cargo.toml")"
  [[ -n "$crate_ver" ]] \
    || fail "could not read yosh-plugin-api version"

  # 3. WIT package line present exactly once?
  local pkg_count
  pkg_count="$(grep -c '^package yosh:plugin@' "$wit" || true)"
  [[ "$pkg_count" -eq 1 ]] \
    || fail "WIT package declaration missing or duplicated in $wit"

  # 4. HEAD is the bump commit produced by phase_bump?
  local head_subj
  head_subj="$(git log -1 --format=%s)"
  [[ "$head_subj" == "chore: release v${crate_ver}" ]] \
    || fail "HEAD is not 'chore: release v${crate_ver}' — saw '${head_subj}'. Reconcile manually."

  # 5–6. Compare content SHA against last-published.
  local new_sha old_sha=""
  new_sha="$(compute_wit_content_sha "$wit")"
  [[ -f "$sha_file" ]] && old_sha="$(cat "$sha_file")"

  # 7. Skip when content unchanged.
  if [[ "$new_sha" == "$old_sha" ]]; then
    echo "yosh-release: WIT unchanged (sha256=$new_sha), skip publish" >&2
    return 0
  fi

  # 8. Rewrite WIT package version to match the crate version, then sync
  # the mirror copies so the byte-comparison build.rs checks keep passing.
  # `-i.bak + rm` is portable between BSD sed (macOS) and GNU sed (Linux).
  sed -i.bak "s|^package yosh:plugin@.*|package yosh:plugin@${crate_ver};|" "$wit"
  rm -f "${wit}.bak"
  # Only sync mirrors whose directory exists (all of them, in the real
  # repo — the guard keeps the test harness's minimal fake repos, which
  # materialize just the canonical copy, working). Track what was synced
  # so the amend below stages exactly those paths.
  local mirror
  local synced_mirrors=()
  for mirror in "${wit_mirrors[@]}"; do
    [[ -d "$(dirname "$mirror")" ]] || continue
    cp "$wit" "$mirror" || fail "failed to sync WIT mirror $mirror"
    synced_mirrors+=("$mirror")
  done

  # 9. Build the WIT directory into a .wasm package, then publish.
  # `wkg wit publish` does not exist; the supported flow is
  # `wkg wit build` (encode the WIT dir into a .wasm) followed by
  # `wkg publish <file>` (upload to the default registry).
  local wit_tmpdir
  wit_tmpdir="$(mktemp -d -t yosh_plugin_pkg.XXXXXX)"
  local wit_pkg="$wit_tmpdir/yosh_plugin.wasm"
  local wit_pkg_stripped="$wit_tmpdir/yosh_plugin_stripped.wasm"
  echo "yosh-release: building yosh:plugin@${crate_ver} package..." >&2
  wkg wit build -d "$(dirname "$wit")" -o "$wit_pkg" \
    || { rm -rf "$wit_tmpdir"; fail "wkg wit build failed — see stderr above"; }
  # Strip the `package-docs` custom section before upload: wa.dev rejects
  # the v1 format that wkg 0.15 emits. See precondition note above.
  wasm-tools strip -d 'package-docs' "$wit_pkg" -o "$wit_pkg_stripped" \
    || { rm -rf "$wit_tmpdir"; fail "wasm-tools strip failed — see stderr above"; }
  echo "yosh-release: publishing yosh:plugin@${crate_ver} to wa.dev..." >&2
  wkg publish "$wit_pkg_stripped" \
    || { rm -rf "$wit_tmpdir"; fail "wkg publish failed (auth / network / dup version) — see stderr above"; }
  rm -rf "$wit_tmpdir"

  # 10. Persist new SHA.
  echo "$new_sha" > "$sha_file"

  # 11. Amend the bump commit so the WIT/SHA changes are part of the
  # release tag that phase_push will create.
  # ${arr[@]+...} expansion stays safe under `set -u` when no mirror
  # was synced (minimal fake repos in the test harness).
  git add "$wit" ${synced_mirrors[@]+"${synced_mirrors[@]}"} "$sha_file" \
    || fail "git add failed for WIT/SHA files"
  git commit --amend --no-edit \
    || fail "git commit --amend failed — recover manually"

  echo "yosh-release: WIT published as yosh:plugin@${crate_ver}, sha256=${new_sha}" >&2
}

phase_push() {
  local ver
  ver="$(read_package_version "Cargo.toml")"
  [[ -n "$ver" ]] || fail "could not read version from Cargo.toml"
  local tag="v${ver}"

  # phase_push's contract is "release == HEAD": the tag must end up at
  # HEAD on origin. Probe the remote before mutating it at all, so a
  # same-name-different-commit tag fails without first advancing
  # origin/main, and a rerun after a partial push stays idempotent.
  # Both patterns: the exact ref alone omits the peeled '^{}' line an
  # annotated tag needs for commit comparison.
  local head_sha remote_ref
  head_sha="$(git rev-parse HEAD)" || fail "git rev-parse HEAD failed"
  remote_ref="$(git ls-remote --tags origin "refs/tags/${tag}" "refs/tags/${tag}^{}")" \
    || fail "git ls-remote origin failed — check network/auth then rerun 'release.sh push'"

  local tag_already_on_remote=0
  if [[ -n "$remote_ref" ]]; then
    local remote_sha
    # Annotated tags list a peeled '^{}' line holding the commit; prefer
    # it. Lightweight tags have a single line already at the commit.
    remote_sha="$(printf '%s\n' "$remote_ref" | awk '/\^\{\}$/ {sha=$1} END {if (sha == "") exit 1; print sha}')" \
      || remote_sha="$(printf '%s\n' "$remote_ref" | awk 'NR==1 {print $1}')"
    [[ "$remote_sha" == "$head_sha" ]] \
      || fail "tag ${tag} exists on origin at ${remote_sha} but HEAD is ${head_sha} — delete the remote tag or bump the version, then rerun 'release.sh push'"
    tag_already_on_remote=1
  fi

  # Create (or repoint) the local tag at HEAD. A leftover local tag from
  # an earlier failed attempt may sit on a commit that a rebase has since
  # replaced; -f keeps it in sync with what is actually being released.
  if [[ $tag_already_on_remote -eq 0 ]]; then
    local local_sha=""
    if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
      local_sha="$(git rev-parse "refs/tags/${tag}^{commit}")"
    fi
    if [[ "$local_sha" == "$head_sha" ]]; then
      echo "yosh-release: tag ${tag} already at HEAD locally, skipping tag creation" >&2
    else
      [[ -n "$local_sha" ]] \
        && echo "yosh-release: local tag ${tag} was at ${local_sha}, repointing to HEAD" >&2
      echo "yosh-release: creating tag ${tag}..." >&2
      git tag -f "${tag}" \
        || fail "git tag ${tag} failed — create manually and rerun 'git push origin ${tag}'"
    fi
  fi

  echo "yosh-release: pushing main to origin..." >&2
  git push origin main \
    || fail "git push origin main failed — resolve remote divergence then rerun 'release.sh push'"

  if [[ $tag_already_on_remote -eq 1 ]]; then
    echo "yosh-release: tag ${tag} already on origin at the same commit, skipping tag push" >&2
  else
    echo "yosh-release: pushing tag ${tag}..." >&2
    git push origin "${tag}" \
      || fail "git push origin ${tag} failed — rerun 'git push origin ${tag}' manually"
  fi

  echo "yosh-release: push complete (main + ${tag})" >&2
}

main() {
  local phase="${1:-}"
  shift || true
  case "${phase}" in
    test)        phase_test "$@" ;;
    bump)        phase_bump "$@" ;;
    publish)     phase_publish "$@" ;;
    publish-wit) phase_publish_wit "$@" ;;
    push)        phase_push "$@" ;;
    "")          fail "usage: release.sh {test|bump|publish|publish-wit|push}" ;;
    *)           fail "unknown phase: ${phase}" ;;
  esac
}

# Only dispatch when executed as a script. Sourcing (e.g., from tests) imports
# the helpers without firing main.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
