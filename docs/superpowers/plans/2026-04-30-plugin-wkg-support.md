# Plugin wkg Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish the `yosh:plugin` WIT package to wa.dev (via wkg) so external plugin authors can replace `path = "<yosh-checkout>/..."` with `version = "0.2"` in their `Cargo.toml`.

**Architecture:** Add a conditional `phase_publish_wit` to `release.sh` that runs after `phase_publish` (crates.io) and before `phase_push` (tag creation). The phase uses SHA-256 of WIT content (excluding the `package` version line) to detect interface changes; on change it rewrites the WIT version line to match the crate version, runs `wkg wit publish`, updates the SHA file, and amends the bump commit. Behavior is fully driven from bash; no Rust runtime code changes.

**Tech Stack:** bash 4+ (existing release.sh), `wkg` CLI (Bytecode Alliance), `shasum -a 256`, Rust dev-test in `crates/yosh-plugin-api/tests/`.

**Spec:** `docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md`

---

## File Structure

| Path | Action | Purpose |
|------|--------|---------|
| `.claude/skills/release/scripts/release.sh` | Modify | Add `compute_wit_content_sha`, `phase_publish_wit`, wire into `main()`, add a sourceability guard. |
| `.claude/skills/release/tests/test_publish_wit.sh` | Create | Bash test harness + tests for SHA helper and `phase_publish_wit` cases (stubs `wkg`/`git`/`cargo`). |
| `crates/yosh-plugin-api/.last-published-wit.sha256` | Create (empty initially) | Tracks SHA-256 of last-published WIT content. Created by first release (not by this PR). |
| `crates/yosh-plugin-api/tests/wit_format.rs` | Create | Rust regression test asserting WIT first line matches `package yosh:plugin@<SemVer>;`. |
| `docs/kish/plugin.md` | Modify | Replace `path = "..."` Quick Start example with `version = "0.2"`; add "Setting up wkg" subsection. |

**Note on `.last-published-wit.sha256`:** This file is *not* created by this PR — it is created by the first `release.sh` run after merge. The "first publish" code path handles the absent-file case explicitly. Adding it now would be an empty placeholder with no real meaning.

---

### Task 1: Make `release.sh` sourceable for testing

**Why first:** Tests in later tasks need to `source release.sh` and call functions directly. Currently `release.sh` runs `main "$@"` unconditionally at the bottom, which would execute the dispatch logic on source.

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh:367` (last line)

- [ ] **Step 1: Read current last line**

Run: `tail -1 .claude/skills/release/scripts/release.sh`
Expected output: `main "$@"`

- [ ] **Step 2: Wrap `main` invocation in a "run only when executed, not sourced" guard**

Replace the final `main "$@"` line with:

```bash
# Only dispatch when executed as a script. Sourcing (e.g., from tests) imports
# the helpers without firing main.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
```

- [ ] **Step 3: Verify script still runs in execute mode**

Run: `.claude/skills/release/scripts/release.sh 2>&1 | head -1`
Expected: `yosh-release: usage: release.sh {test|bump|publish|push}` (or whatever `fail` printed before — same behavior as the empty-arg path).

- [ ] **Step 4: Verify script is now sourceable without dispatching**

Run: `bash -c 'source .claude/skills/release/scripts/release.sh && echo SOURCED_OK'`
Expected: `SOURCED_OK` (no usage error, no exit).

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "$(cat <<'EOF'
refactor(release): make release.sh sourceable

Wrap the bottom main invocation in a BASH_SOURCE guard so test
harnesses can `source release.sh` and call individual phase
helpers without firing the dispatch logic. Behavior on direct
invocation is unchanged.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Add bash test harness skeleton

**Files:**
- Create: `.claude/skills/release/tests/test_publish_wit.sh`

- [ ] **Step 1: Create the tests directory and harness file**

```bash
mkdir -p .claude/skills/release/tests
```

Create `.claude/skills/release/tests/test_publish_wit.sh` with:

```bash
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

echo "----"
echo "Passed: $PASS"
echo "Failed: $FAIL"
for f in "${FAILURES[@]}"; do echo "  - $f"; done
[[ $FAIL -eq 0 ]] || exit 1
```

- [ ] **Step 2: Make it executable and run it**

```bash
chmod +x .claude/skills/release/tests/test_publish_wit.sh
bash .claude/skills/release/tests/test_publish_wit.sh
```

Expected output (last 4 lines):
```
PASS: harness: source release.sh
----
Passed: 1
Failed: 0
```

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/release/tests/test_publish_wit.sh
git commit -m "$(cat <<'EOF'
test(release): add test harness skeleton for phase_publish_wit

Bash assertion helpers (assert_eq, assert_contains, assert_file_exists)
plus a run_test wrapper that runs each case in a `set -e` subshell so
individual failures do not abort the harness. One sanity test verifies
release.sh can be sourced.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §8.1
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Implement `compute_wit_content_sha` (TDD)

**Files:**
- Modify: `.claude/skills/release/tests/test_publish_wit.sh` (add tests)
- Modify: `.claude/skills/release/scripts/release.sh` (add helper)

- [ ] **Step 1: Add failing tests for the SHA helper**

Append to `.claude/skills/release/tests/test_publish_wit.sh` *before* the trailing summary block (the `echo "----"` line). The tests build small WIT fixtures inline.

```bash
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
```

- [ ] **Step 2: Run tests to verify the SHA tests fail**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: tests "sha: ..." print `FAIL` because `compute_wit_content_sha` is undefined; final summary `Failed: 3` (or similar). The script exits with status 1.

- [ ] **Step 3: Implement `compute_wit_content_sha` in `release.sh`**

Add this function in `release.sh` immediately after `rewrite_dep_version` (around line 84, before the `PHASE_TEST_JOBS` block):

```bash
# Compute SHA-256 of a WIT file's content with the `package yosh:plugin@<x>;`
# declaration line stripped. The version line is rewritten by phase_publish_wit
# itself; excluding it makes SHA equality strictly track interface equality.
# Output: 64 hex chars on stdout, no trailing newline beyond what `cut` emits.
compute_wit_content_sha() {
  local wit="$1"
  grep -v '^package yosh:plugin@' "$wit" | shasum -a 256 | cut -d' ' -f1
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: all 4 tests pass (the 1 sanity + 3 sha tests). Final summary `Failed: 0`. Exit status 0.

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh \
        .claude/skills/release/tests/test_publish_wit.sh
git commit -m "$(cat <<'EOF'
feat(release): add compute_wit_content_sha helper

Compute a SHA-256 of WIT file contents with the package version line
stripped, so version-only diffs do not register as content changes.
This is the core primitive that lets phase_publish_wit (next commit)
skip wa.dev re-publish on patch releases that do not change the WIT
interface.

Tests: version-only diff yields equal SHAs; interface and comment
diffs yield different SHAs.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §4.3, §8.1
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Implement `phase_publish_wit` skip-and-guard paths (TDD)

This task implements steps 1–7 of the algorithm in spec §4.2 (everything up to and including the SHA-skip return). The "publish" branch is added in Task 5.

**Files:**
- Modify: `.claude/skills/release/tests/test_publish_wit.sh` (add tests with stubs)
- Modify: `.claude/skills/release/scripts/release.sh` (add `phase_publish_wit` partial)

- [ ] **Step 1: Add stub helpers to the test harness**

Append to `.claude/skills/release/tests/test_publish_wit.sh` (immediately after the `run_test` definition near the top):

```bash
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
```

- [ ] **Step 2: Add failing tests for the skip and guard paths**

Append before the summary block:

```bash
# Spec §8.2 cases 2, 4, 6, 7.

run_test "phase_publish_wit: skip when SHA matches" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  WKG_LOG=$(mktemp)
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
  # Empty PATH so no wkg is reachable.
  out=$(PATH="/usr/bin:/bin" phase_publish_wit 2>&1)
  rc=$?
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
  WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # Strip the package line.
  sed -i.bak "/^package yosh:plugin@/d" \
    "crates/yosh-plugin-api/wit/yosh-plugin.wit"
  rm -f "crates/yosh-plugin-api/wit/yosh-plugin.wit.bak"
  out=$(phase_publish_wit 2>&1)
  rc=$?
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
  WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # Add an extra commit on top so HEAD subject is not "chore: release v0.2.6".
  echo trash > junk.txt
  git add junk.txt
  git -c user.email=t@t -c user.name=t commit -q -m "WIP: random work"
  out=$(phase_publish_wit 2>&1)
  rc=$?
  if [[ "$rc" -eq 0 ]]; then
    FAILURES+=("expected non-zero exit when HEAD is not bump commit, got 0")
    exit 1
  fi
  assert_contains "$out" "chore: release v0.2.6" "head-not-bump message" || exit 1
  rm -rf "$repo" "$stub" "$WKG_LOG"
'
```

- [ ] **Step 3: Run tests and confirm 4 new failures**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: the 4 new `phase_publish_wit: ...` tests fail because `phase_publish_wit` is undefined. Final summary lists `Failed: 4`.

- [ ] **Step 4: Implement the skip and guard paths in `phase_publish_wit`**

Add a new function definition in `release.sh` immediately after `phase_publish` (currently ending around line 327, before `phase_push`):

```bash
phase_publish_wit() {
  local wit="crates/yosh-plugin-api/wit/yosh-plugin.wit"
  local sha_file="crates/yosh-plugin-api/.last-published-wit.sha256"

  # 1. wkg available?
  command -v wkg >/dev/null \
    || fail "wkg not found in PATH. Install via 'cargo install wkg --locked'"

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
    echo "yosh-release: WIT unchanged (sha256=$new_sha), skip wkg wit publish" >&2
    return 0
  fi

  # Steps 8–12 (publish branch) are added in the next task.
  fail "phase_publish_wit: publish branch not yet implemented (NEW_SHA=$new_sha)"
}
```

- [ ] **Step 5: Run tests to verify all pass**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: all 8 tests pass (1 sanity + 3 sha + 4 phase_publish_wit guards). Final `Failed: 0`.

- [ ] **Step 6: Commit**

```bash
git add .claude/skills/release/scripts/release.sh \
        .claude/skills/release/tests/test_publish_wit.sh
git commit -m "$(cat <<'EOF'
feat(release): add phase_publish_wit skip and guard paths

Implements steps 1-7 of the algorithm in §4.2: wkg-on-PATH check,
crate-version read, WIT package-line presence check, HEAD-is-bump
guard, SHA comparison, and the unchanged-skip return. The publish
branch (steps 8-12) is intentionally still a fail() so the
incremental TDD progression is visible in tests.

Tests cover: skip when SHA matches, abort when wkg missing, abort
when WIT lacks package line, abort when HEAD is not the bump commit.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §4.2 (1-7), §8.2 (cases 2, 4, 6, 7)
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Implement `phase_publish_wit` publish path (TDD)

This finishes the algorithm: rewrite the WIT version line, run `wkg wit publish`, write the SHA file, amend the bump commit.

**Files:**
- Modify: `.claude/skills/release/tests/test_publish_wit.sh`
- Modify: `.claude/skills/release/scripts/release.sh`

- [ ] **Step 1: Add failing tests for the publish branch**

Append before the summary:

```bash
# Spec §8.2 cases 1, 3, 5.

run_test "phase_publish_wit: first publish creates SHA file and rewrites WIT version" '
  source "$RELEASE_SH"
  repo=$(make_fake_repo)
  cd "$repo"
  stub=$(install_wkg_stub success)
  WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # No SHA file present (first publish).
  out=$(phase_publish_wit 2>&1)
  rc=$?
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
  WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  # SHA file with a stale value triggers the publish branch.
  echo "0000000000000000000000000000000000000000000000000000000000000000" \
    > "crates/yosh-plugin-api/.last-published-wit.sha256"
  git add crates/yosh-plugin-api/.last-published-wit.sha256
  git commit -q --amend --no-edit
  out=$(phase_publish_wit 2>&1)
  rc=$?
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
  WKG_LOG=$(mktemp)
  PATH="$stub:$PATH"
  out=$(phase_publish_wit 2>&1)
  rc=$?
  if [[ "$rc" -eq 0 ]]; then
    FAILURES+=("expected non-zero exit when wkg fails, got 0")
    exit 1
  fi
  assert_contains "$out" "wkg wit publish failed" "wkg-fail message" || exit 1
  rm -rf "$repo" "$stub" "$WKG_LOG"
'
```

- [ ] **Step 2: Run tests and confirm 3 new failures**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: the 3 new tests fail because the publish branch is still `fail "...not yet implemented"`. Final `Failed: 3`.

- [ ] **Step 3: Replace the placeholder publish branch with the real implementation**

In `release.sh`, replace the line:

```bash
  fail "phase_publish_wit: publish branch not yet implemented (NEW_SHA=$new_sha)"
```

with:

```bash
  # 8. Rewrite WIT package version to match the crate version.
  local sed_in_place
  if [[ "$(uname)" == "Darwin" ]]; then
    sed_in_place=(sed -i '')
  else
    sed_in_place=(sed -i)
  fi
  "${sed_in_place[@]}" \
    "s|^package yosh:plugin@.*|package yosh:plugin@${crate_ver};|" "$wit"

  # 9. Publish to wa.dev.
  echo "yosh-release: publishing yosh:plugin@${crate_ver} to wa.dev..." >&2
  wkg wit publish "$(dirname "$wit")" \
    || fail "wkg wit publish failed (auth / network / dup version) — see stderr above"

  # 10. Persist new SHA.
  echo "$new_sha" > "$sha_file"

  # 11. Amend the bump commit so the WIT/SHA changes are part of the
  # release tag that phase_push will create.
  git add "$wit" "$sha_file" \
    || fail "git add failed for WIT/SHA files"
  git commit --amend --no-edit \
    || fail "git commit --amend failed — recover manually"

  echo "yosh-release: WIT published as yosh:plugin@${crate_ver}, sha256=${new_sha}" >&2
```

- [ ] **Step 4: Run tests to verify all pass**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: all 11 tests pass. Final `Failed: 0`.

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh \
        .claude/skills/release/tests/test_publish_wit.sh
git commit -m "$(cat <<'EOF'
feat(release): implement phase_publish_wit publish branch

Steps 8-12 of §4.2: rewrite the WIT package line to the current
crate version, run `wkg wit publish` against the wit directory,
persist the new content SHA, and amend the bump commit so the WIT
and SHA file land in the tag that phase_push creates.

Tests cover: first publish (no SHA file), changed publish (stale SHA
overwritten), wkg failure abort. All 11 phase_publish_wit / SHA
helper tests pass.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §4.2 (8-12), §8.2 (cases 1, 3, 5)
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Wire `phase_publish_wit` into `main()`

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh:354-365` (`main` and usage string)

- [ ] **Step 1: Update `main()` and the usage error**

Replace the existing `main()` (currently at lines 354–365) with:

```bash
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
```

- [ ] **Step 2: Update the header comment block at the top of `release.sh:4-8`**

Replace the existing comment usage block with:

```bash
# Usage:
#   release.sh test
#   release.sh bump
#   release.sh publish [--from <crate-name>]
#   release.sh publish-wit
#   release.sh push
```

- [ ] **Step 3: Verify the dispatcher recognizes the new phase**

Run: `.claude/skills/release/scripts/release.sh nonexistent 2>&1`
Expected: `yosh-release: unknown phase: nonexistent` (one line, exit 1).

Run: `.claude/skills/release/scripts/release.sh 2>&1`
Expected: `yosh-release: usage: release.sh {test|bump|publish|publish-wit|push}` (exit 1).

- [ ] **Step 4: Add a Phase 5 entry to SKILL.md and renumber Push to Phase 6**

Open `.claude/skills/release/SKILL.md`. The current structure is Phase 2 Tests → Phase 3 Bump → Phase 4 Publish → Phase 5 Push (lines 38–62 in the current file). Insert a new Phase 5 (Publish WIT) between current Phase 4 (Publish) and current Phase 5 (Push), and renumber Push to Phase 6.

Insert the following after the Phase 4 block (immediately before the existing `### Phase 5: Push` heading):

```markdown
### Phase 5: Publish WIT to wa.dev

Precondition: the user must have `wkg` on PATH (`cargo install wkg --locked`) and a wa.dev token configured in `~/.config/wasm-pkg/config.toml` (or `WKG_TOKEN`). If the script fails with "wkg not found" or an auth-related message, surface stderr and stop; the WIT publish is independent of crates.io and can be retried after fixing the local environment.

Run: `.claude/skills/release/scripts/release.sh publish-wit`

This phase is conditional: it only invokes `wkg wit publish` when the WIT content (excluding the `package` version line) has changed since the last successful publish. On a no-op patch release the phase prints "WIT unchanged" and exits 0 without touching wa.dev.

If exit code is non-zero, surface stderr verbatim and stop. crates.io is already up-to-date at this point; the WIT publish can be re-attempted after fixing the cause without unwinding the crates.io publish.

```

Also rename the existing `### Phase 5: Push` heading (currently around line 58) to `### Phase 6: Push`.

Update the Completion summary (currently around lines 64–68) from "all five phases" to "all six phases", and update the wording from "Published 4 crates to crates.io" to "Published 4 crates to crates.io and the yosh:plugin WIT package to wa.dev".

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh .claude/skills/release/SKILL.md
git commit -m "$(cat <<'EOF'
feat(release): wire publish-wit subcommand into main dispatcher

Adds `release.sh publish-wit` to the case statement and updates the
usage error and header comment. The skill doc is updated to mention
the new phase next to publish.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §3.2
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Add Rust WIT format regression test

**Files:**
- Create: `crates/yosh-plugin-api/tests/wit_format.rs`

- [ ] **Step 1: Write the test**

Create `crates/yosh-plugin-api/tests/wit_format.rs`:

```rust
//! Asserts the WIT file's leading `package yosh:plugin@<x.y.z>[-pre];`
//! declaration is well-formed. release.sh's phase_publish_wit relies on
//! this shape (sed rewrite + grep -v selector). A malformed WIT here
//! would silently break the publish pipeline; this test catches it
//! during ordinary `cargo test -p yosh-plugin-api`.

#[test]
fn wit_starts_with_package_declaration() {
    let wit_path = concat!(env!("CARGO_MANIFEST_DIR"), "/wit/yosh-plugin.wit");
    let wit = std::fs::read_to_string(wit_path).expect("read wit");

    let first_line = wit
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("WIT file must have a non-empty line");

    let prefix = "package yosh:plugin@";
    let suffix = ";";
    assert!(
        first_line.starts_with(prefix),
        "first non-blank line missing 'package yosh:plugin@' prefix: {first_line:?}"
    );
    assert!(
        first_line.ends_with(suffix),
        "first non-blank line missing trailing ';': {first_line:?}"
    );

    let ver = &first_line[prefix.len()..first_line.len() - suffix.len()];
    let core: &str = ver.split('-').next().expect("non-empty version");
    let parts: Vec<&str> = core.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "version core must be x.y.z, got {ver:?}"
    );
    for p in &parts {
        assert!(
            !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()),
            "version component is not a non-empty numeric: {p:?}"
        );
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p yosh-plugin-api --test wit_format`
Expected: 1 test, passes (the WIT currently starts with `package yosh:plugin@0.1.0;`, which matches).

- [ ] **Step 3: Verify it fails on a deliberate break (optional sanity check)**

Run a temporary break check (do not commit the break):
```bash
cp crates/yosh-plugin-api/wit/yosh-plugin.wit /tmp/yosh-plugin.wit.bak
sed -i.bak 's/^package yosh:plugin@0.1.0;/package something-else;/' \
  crates/yosh-plugin-api/wit/yosh-plugin.wit
cargo test -p yosh-plugin-api --test wit_format 2>&1 | tail -5
mv /tmp/yosh-plugin.wit.bak crates/yosh-plugin-api/wit/yosh-plugin.wit
rm -f crates/yosh-plugin-api/wit/yosh-plugin.wit.bak
```
Expected: test fails with the "missing 'package yosh:plugin@' prefix" message; restoring the file makes the test pass again. Confirm by running once more:

Run: `cargo test -p yosh-plugin-api --test wit_format`
Expected: 1 passed.

- [ ] **Step 4: Commit**

```bash
git add crates/yosh-plugin-api/tests/wit_format.rs
git commit -m "$(cat <<'EOF'
test(plugin-api): assert WIT first line is package yosh:plugin@<SemVer>

The release.sh phase_publish_wit pipeline (sed rewrite + grep -v
selector) depends on the WIT file's first non-blank line matching
'package yosh:plugin@<x.y.z>[-pre];'. This regression test fails
during ordinary 'cargo test -p yosh-plugin-api' if the WIT file is
malformed, catching the breakage long before release time.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §8.3
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Update plugin author documentation

**Files:**
- Modify: `docs/kish/plugin.md`:
  - Lines 160-161 (the `yosh:plugin` dependency block in Quick Start step 2's `Cargo.toml`)
  - Insert a new "Setting up wkg" step between current step 2's prose (around line 168) and current step 3 (line 173)

- [ ] **Step 1: Replace the `path = "..."` line in the Quick Start `Cargo.toml`**

Currently the file has (around lines 157–162):
```toml
   [package.metadata.component]
   package = "yourname:hello"

   [package.metadata.component.target.dependencies."yosh:plugin"]
   path = "<path-to-yosh-checkout>/crates/yosh-plugin-api/wit"
```

Replace with:
```toml
   [package.metadata.component]
   package = "yourname:hello"

   [package.metadata.component.target.dependencies."yosh:plugin"]
   version = "0.2"
```

- [ ] **Step 2: Insert a new "Setting up wkg" step before current step 3, then renumber later steps**

Current Quick Start headings (these are unique strings in the file):
- `3. Write \`src/lib.rs\`` (line 173)
- `4. Build:` (line 195)
- `5. Install locally:` (line 206)

The `panic = "abort"` paragraph (currently lines 170–171) ends step 2's body. Insert the new step 3 immediately after that paragraph (i.e., between the existing `panic-string formatting from pulling in \`wasi:cli/stderr\` at link time.` line and the existing `3. Write \`src/lib.rs\`:` line). Use this exact block (the outer 4-backtick fence in this plan is just for display — the content goes into plugin.md without it; preserve the leading 3-space indentation that matches the surrounding numbered-list style):

````
3. Set up `wkg` to resolve the `yosh:plugin` WIT package from
   [wa.dev]:

   ```sh
   cargo install wkg --locked
   wkg config --default-registry wa.dev
   ```

   `cargo component build` (step 5) invokes `wkg` automatically to
   fetch `yosh:plugin@<version>` on first build. This replaces the
   `path = "<yosh-checkout>/..."` form used by yosh's in-repo test
   plugins.

   [wa.dev]: https://wa.dev/

````

Then make these three exact edits to renumber the existing headings (each is a unique string, so a literal Edit/replace works):

1. `3. Write \`src/lib.rs\`:` → `4. Write \`src/lib.rs\`:`
2. `4. Build:` → `5. Build:`
3. `5. Install locally:` → `6. Install locally:`

(Order matters: do the renumber edits **after** the insertion, otherwise the insertion-anchor paragraph and the `3. Write ...` line overlap in unexpected ways. If you prefer to do all four edits in one Edit-tool call, perform the renumbers first — bottom-up — then insert the new step 3.)

- [ ] **Step 3: Verify the rendered structure**

Run: `grep -n '^[0-9]\.' docs/kish/plugin.md | sed -n '1,10p'`
Expected: the numbering of the Quick Start steps is now 1–6, with the new "Set up `wkg`" entry between the Cargo.toml step and the `src/lib.rs` step.

Run: `grep -n 'path = "<path-to-yosh-checkout>/' docs/kish/plugin.md`
Expected: no matches (the string was removed from Quick Start).

Run: `grep -n 'wa\.dev\|wkg config' docs/kish/plugin.md`
Expected: at least three matches (the `wa.dev` link reference, the install step, the `wkg config` invocation).

- [ ] **Step 4: Commit**

```bash
git add docs/kish/plugin.md
git commit -m "$(cat <<'EOF'
docs(plugin): switch Quick Start to wkg-based WIT resolution

External plugin authors no longer need a yosh checkout. The Quick
Start now uses `version = "0.2"` for the yosh:plugin WIT dependency
and a new "Set up wkg" step covers the one-time wkg install and
default-registry config. The `path = "..."` form remains supported
for in-repo test plugins but is no longer documented as the
recommended path.

Refs: docs/superpowers/specs/2026-04-30-plugin-wkg-support-design.md §6
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Final verification across the workspace

**Files:** none (verification only).

- [ ] **Step 1: Run the bash test suite**

Run: `bash .claude/skills/release/tests/test_publish_wit.sh`
Expected: 11 tests, 0 failed.

- [ ] **Step 2: Run the yosh-plugin-api test suite**

Run: `cargo test -p yosh-plugin-api`
Expected: existing tests + the new `wit_format::wit_starts_with_package_declaration` all pass.

- [ ] **Step 3: Confirm `release.sh` still parses and dispatches correctly**

Run: `bash -n .claude/skills/release/scripts/release.sh` (syntax check)
Expected: exit 0, no output.

Run: `.claude/skills/release/scripts/release.sh 2>&1`
Expected: `yosh-release: usage: release.sh {test|bump|publish|publish-wit|push}` and exit 1.

- [ ] **Step 4: Confirm the in-repo test plugins still build with the unchanged `path = "..."` form**

Run: `cargo component build -p test_plugin --target wasm32-wasip2 --release`
Expected: builds successfully (no change to those plugins, no change to `crates/yosh-plugin-api/wit/yosh-plugin.wit` apart from what tests/release set; the WIT package line is still `0.1.0` until the first real release).

- [ ] **Step 5: Spot-check the spec for placeholder strings that may have leaked into code**

Run:
```bash
grep -rn '<path-to-yosh-checkout>' docs/kish/ \
  .claude/skills/release/ \
  crates/yosh-plugin-api/
```
Expected: no matches in `docs/kish/`. Test fixtures inside `tests/plugins/*/Cargo.toml` are unrelated (they use real relative paths, not the placeholder).

- [ ] **Step 6: Final summary commit (optional, only if you have uncommitted dust)**

```bash
git status
```

If clean: no commit needed; the plan is complete.
If anything is uncommitted (unlikely — every prior task ended with a commit): inspect, decide intent, commit with a focused message.
