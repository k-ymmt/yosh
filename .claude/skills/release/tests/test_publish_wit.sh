#!/usr/bin/env bash
# Tests for release.sh phase_publish_wit and helpers.
# Run: bash .claude/skills/release/tests/test_publish_wit.sh

# Tests deliberately do NOT enable `set -e` at the top: each test runs
# inside a subshell + `(set -e; ...)` so individual failures do not
# kill the harness. The harness's own logic checks return codes.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
RELEASE_SH="${REPO_ROOT}/.claude/skills/release/scripts/release.sh"

PASS=0
FAIL=0
FAILURES=()

assert_eq() {
  local actual="$1" expected="$2" label="${3:-assertion}"
  if [[ "$actual" == "$expected" ]]; then
    return 0
  fi
  FAILURES+=("${label}: expected '${expected}', got '${actual}'")
  return 1
}

assert_contains() {
  local haystack="$1" needle="$2" label="${3:-assertion}"
  if [[ "$haystack" == *"$needle"* ]]; then
    return 0
  fi
  FAILURES+=("${label}: '${haystack}' did not contain '${needle}'")
  return 1
}

assert_file_exists() {
  local path="$1" label="${2:-assertion}"
  if [[ -f "$path" ]]; then
    return 0
  fi
  FAILURES+=("${label}: file ${path} does not exist")
  return 1
}

run_test() {
  local name="$1" body="$2"
  if eval "( set -e; $body )"; then
    PASS=$((PASS + 1))
    echo "PASS: $name"
  else
    FAIL=$((FAIL + 1))
    echo "FAIL: $name"
  fi
}

# Build a fake repo with crates/yosh-plugin-api/Cargo.toml and wit/yosh-plugin.wit.
# Echo the repo root path on stdout. Caller is responsible for `rm -rf`.
# Sets repo-local git user config so subsequent commits (including the
# amend done by phase_publish_wit) succeed without inheriting global config.
make_fake_repo() {
  local root
  root="$(mktemp -d)"
  mkdir -p "$root/crates/yosh-plugin-api/wit"
  cat > "$root/crates/yosh-plugin-api/Cargo.toml" <<EOF
[package]
name = "yosh-plugin-api"
version = "0.2.6"
edition = "2024"
EOF
  cat > "$root/crates/yosh-plugin-api/wit/yosh-plugin.wit" <<EOF
package yosh:plugin@0.1.0;
interface foo {
  bar: func();
}
EOF
  (
    cd "$root" \
      && git init -q \
      && git config user.email t@t \
      && git config user.name t \
      && git add -A \
      && git commit -q -m "chore: release v0.2.6"
  )
  echo "$root"
}

# Install a stub `wkg` on PATH (for one test). Logs invocations to $WKG_LOG.
install_wkg_stub() {
  local mode="${1:-success}"  # success | fail
  local stubdir
  stubdir="$(mktemp -d)"
  cat > "$stubdir/wkg" <<EOF
#!/usr/bin/env bash
echo "wkg \$*" >> "\$WKG_LOG"
[[ "$mode" == "success" ]] && exit 0 || exit 1
EOF
  chmod +x "$stubdir/wkg"
  echo "$stubdir"
}

# Sanity: harness boots and can source release.sh.
run_test "harness: source release.sh" '
  source "$RELEASE_SH"
  type compute_wit_content_sha >/dev/null 2>&1 || {
    # compute_wit_content_sha not defined yet — that is OK for now; this
    # test only verifies the harness can boot and source.
    true
  }
'

# Spec §8.1: SHA helper unit tests.

run_test "sha: ignores changes to package version line" '
  source "$RELEASE_SH"
  tmp="$(mktemp -d)"
  cat > "$tmp/a.wit" <<WIT
package yosh:plugin@0.1.0;
interface foo {
  bar: func();
}
WIT
  cat > "$tmp/b.wit" <<WIT
package yosh:plugin@9.9.9;
interface foo {
  bar: func();
}
WIT
  sha_a=$(compute_wit_content_sha "$tmp/a.wit")
  sha_b=$(compute_wit_content_sha "$tmp/b.wit")
  assert_eq "$sha_a" "$sha_b" "version-only diff" || exit 1
  rm -rf "$tmp"
'

run_test "sha: detects interface change" '
  source "$RELEASE_SH"
  tmp="$(mktemp -d)"
  cat > "$tmp/a.wit" <<WIT
package yosh:plugin@0.1.0;
interface foo {
  bar: func();
}
WIT
  cat > "$tmp/b.wit" <<WIT
package yosh:plugin@0.1.0;
interface foo {
  bar: func();
  baz: func();
}
WIT
  sha_a=$(compute_wit_content_sha "$tmp/a.wit")
  sha_b=$(compute_wit_content_sha "$tmp/b.wit")
  if [[ "$sha_a" == "$sha_b" ]]; then
    FAILURES+=("interface change should change SHA")
    exit 1
  fi
  rm -rf "$tmp"
'

run_test "sha: detects comment change" '
  source "$RELEASE_SH"
  tmp="$(mktemp -d)"
  cat > "$tmp/a.wit" <<WIT
package yosh:plugin@0.1.0;
interface foo {
  bar: func();
}
WIT
  cat > "$tmp/b.wit" <<WIT
package yosh:plugin@0.1.0;
// extra comment
interface foo {
  bar: func();
}
WIT
  sha_a=$(compute_wit_content_sha "$tmp/a.wit")
  sha_b=$(compute_wit_content_sha "$tmp/b.wit")
  if [[ "$sha_a" == "$sha_b" ]]; then
    FAILURES+=("comment change should change SHA")
    exit 1
  fi
  rm -rf "$tmp"
'

# Spec §8.2 cases 2, 4, 6, 7.

run_test "phase_publish_wit: skip when SHA matches" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  export WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # Pre-populate SHA file with the current content sha so step 7 skips.
  sha=$(compute_wit_content_sha "crates/yosh-plugin-api/wit/yosh-plugin.wit")
  echo "$sha" > "crates/yosh-plugin-api/.last-published-wit.sha256"
  out=$(phase_publish_wit 2>&1)
  rc=$?
  assert_eq "$rc" "0" "exit code on skip" || exit 1
  assert_contains "$out" "WIT unchanged" "skip message" || exit 1
  # wkg must not have been called.
  assert_eq "$(wc -l < "$WKG_LOG" | tr -d " ")" "0" "wkg call count" || exit 1
  rm -rf "$repo" "$stub" "$WKG_LOG"
'

run_test "phase_publish_wit: aborts when wkg missing" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  # Empty PATH so no wkg is reachable. `if`-pattern keeps set -e happy
  # when phase_publish_wit calls fail/exit 1 in the inner subshell.
  if out=$(PATH="/usr/bin:/bin" phase_publish_wit 2>&1); then rc=0; else rc=$?; fi
  if [[ "$rc" -eq 0 ]]; then
    FAILURES+=("expected non-zero exit when wkg missing, got 0")
    exit 1
  fi
  assert_contains "$out" "wkg not found" "missing-wkg message" || exit 1
  rm -rf "$repo"
'

run_test "phase_publish_wit: aborts when WIT package line missing" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  export WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # Strip the package line.
  sed -i.bak "/^package yosh:plugin@/d" \
    "crates/yosh-plugin-api/wit/yosh-plugin.wit"
  rm -f "crates/yosh-plugin-api/wit/yosh-plugin.wit.bak"
  if out=$(phase_publish_wit 2>&1); then rc=0; else rc=$?; fi
  if [[ "$rc" -eq 0 ]]; then
    FAILURES+=("expected non-zero exit when WIT lacks package line, got 0")
    exit 1
  fi
  assert_contains "$out" "package declaration" "missing-package message" || exit 1
  rm -rf "$repo" "$stub" "$WKG_LOG"
'

run_test "phase_publish_wit: aborts when HEAD is not the bump commit" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  export WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # Add an extra commit on top so HEAD subject is not "chore: release v0.2.6".
  echo trash > junk.txt
  git add junk.txt
  git -c user.email=t@t -c user.name=t commit -q -m "WIP: random work"
  if out=$(phase_publish_wit 2>&1); then rc=0; else rc=$?; fi
  if [[ "$rc" -eq 0 ]]; then
    FAILURES+=("expected non-zero exit when HEAD is not bump commit, got 0")
    exit 1
  fi
  assert_contains "$out" "chore: release v0.2.6" "head-not-bump message" || exit 1
  rm -rf "$repo" "$stub" "$WKG_LOG"
'

# Spec §8.2 cases 1, 3, 5.

run_test "phase_publish_wit: first publish creates SHA file and rewrites WIT version" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  export WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # No SHA file present (first publish).
  if out=$(phase_publish_wit 2>&1); then rc=0; else rc=$?; fi
  assert_eq "$rc" "0" "exit code on first publish" || exit 1
  # wkg should have been invoked exactly once with the wit dir.
  assert_eq "$(wc -l < "$WKG_LOG" | tr -d " ")" "1" "wkg call count" || exit 1
  assert_contains "$(cat "$WKG_LOG")" "wit publish" "wkg subcommand" || exit 1
  # SHA file exists.
  assert_file_exists "crates/yosh-plugin-api/.last-published-wit.sha256" \
    "sha file" || exit 1
  # WIT version was rewritten to match the crate version 0.2.6.
  pkg_line=$(grep "^package yosh:plugin@" \
    crates/yosh-plugin-api/wit/yosh-plugin.wit)
  assert_eq "$pkg_line" "package yosh:plugin@0.2.6;" "wit version line" || exit 1
  # HEAD subject is still the bump commit (amend kept the message).
  subj=$(git log -1 --format=%s)
  assert_eq "$subj" "chore: release v0.2.6" "head subject after amend" || exit 1
  # WIT and SHA file are part of HEAD (committed via amend).
  git diff --quiet HEAD -- crates/yosh-plugin-api/wit/yosh-plugin.wit \
    || { FAILURES+=("WIT not committed by amend"); exit 1; }
  git diff --quiet HEAD -- crates/yosh-plugin-api/.last-published-wit.sha256 \
    || { FAILURES+=("SHA file not committed by amend"); exit 1; }
  rm -rf "$repo" "$stub" "$WKG_LOG"
'

run_test "phase_publish_wit: changed publish overwrites stale SHA" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  export WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # SHA file with a stale value triggers the publish branch.
  echo "0000000000000000000000000000000000000000000000000000000000000000" \
    > "crates/yosh-plugin-api/.last-published-wit.sha256"
  git add crates/yosh-plugin-api/.last-published-wit.sha256
  git commit -q --amend --no-edit
  if out=$(phase_publish_wit 2>&1); then rc=0; else rc=$?; fi
  assert_eq "$rc" "0" "exit code on changed publish" || exit 1
  new_sha=$(cat "crates/yosh-plugin-api/.last-published-wit.sha256")
  if [[ "$new_sha" == "0000000000000000000000000000000000000000000000000000000000000000" ]]; then
    FAILURES+=("SHA file was not updated")
    exit 1
  fi
  rm -rf "$repo" "$stub" "$WKG_LOG"
'

run_test "phase_publish_wit: aborts when wkg fails" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub fail)
  export WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  if out=$(phase_publish_wit 2>&1); then rc=0; else rc=$?; fi
  if [[ "$rc" -eq 0 ]]; then
    FAILURES+=("expected non-zero exit when wkg fails, got 0")
    exit 1
  fi
  assert_contains "$out" "wkg wit publish failed" "wkg-fail message" || exit 1
  rm -rf "$repo" "$stub" "$WKG_LOG"
'

echo "----"
echo "Passed: $PASS"
echo "Failed: $FAIL"
for f in "${FAILURES[@]}"; do echo "  - $f"; done
[[ $FAIL -eq 0 ]] || exit 1
