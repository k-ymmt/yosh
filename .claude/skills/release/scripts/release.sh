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

phase_test() {
  echo "yosh-release: running cargo test..." >&2
  cargo test || fail "cargo test failed — fix tests and rerun"
  echo "yosh-release: running e2e tests..." >&2
  ./e2e/run_tests.sh || fail "e2e tests failed — fix tests and rerun"
  echo "yosh-release: all tests passed" >&2
}

phase_bump() {
  fail "bump phase not implemented yet"
}

phase_publish() {
  fail "publish phase not implemented yet"
}

phase_push() {
  fail "push phase not implemented yet"
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
