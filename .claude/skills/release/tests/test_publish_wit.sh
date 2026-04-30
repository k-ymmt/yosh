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
  FAILURES+=("${label}: expected ${expected@Q}, got ${actual@Q}")
  return 1
}

assert_contains() {
  local haystack="$1" needle="$2" label="${3:-assertion}"
  if [[ "$haystack" == *"$needle"* ]]; then
    return 0
  fi
  FAILURES+=("${label}: ${haystack@Q} did not contain ${needle@Q}")
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

echo "----"
echo "Passed: $PASS"
echo "Failed: $FAIL"
for f in "${FAILURES[@]}"; do echo "  - $f"; done
[[ $FAIL -eq 0 ]] || exit 1
