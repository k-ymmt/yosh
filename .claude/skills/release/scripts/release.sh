#!/usr/bin/env bash
# Deterministic release phases for yosh.
# Invoked by .claude/skills/release/SKILL.md, or runnable standalone for recovery.
# Usage:
#   release.sh test
#   release.sh bump
#   release.sh publish [--from <crate-name>]
#   release.sh push

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
cd "${REPO_ROOT}"

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
    if [[ "$crate" == "yosh" ]]; then
      cmd=(cargo publish --no-verify)
    else
      cmd=(cargo publish --no-verify -p "$crate")
    fi

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

phase_push() {
  local ver
  ver="$(read_package_version "Cargo.toml")"
  [[ -n "$ver" ]] || fail "could not read version from Cargo.toml"
  local tag="v${ver}"

  echo "yosh-release: pushing main to origin..." >&2
  git push origin main \
    || fail "git push origin main failed — resolve remote divergence then rerun 'release.sh push'"

  if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "yosh-release: tag ${tag} already exists locally, skipping tag creation" >&2
  else
    echo "yosh-release: creating tag ${tag}..." >&2
    git tag "${tag}" \
      || fail "git tag ${tag} failed — create manually and rerun 'git push origin ${tag}'"
  fi

  echo "yosh-release: pushing tag ${tag}..." >&2
  git push origin "${tag}" \
    || fail "git push origin ${tag} failed — rerun 'git push origin ${tag}' manually"

  echo "yosh-release: push complete (main + ${tag})" >&2
}

main() {
  local phase="${1:-}"
  shift || true
  case "${phase}" in
    test)    phase_test "$@" ;;
    bump)    phase_bump "$@" ;;
    publish) phase_publish "$@" ;;
    push)    phase_push "$@" ;;
    "")      fail "usage: release.sh {test|bump|publish|push}" ;;
    *)       fail "unknown phase: ${phase}" ;;
  esac
}

main "$@"
