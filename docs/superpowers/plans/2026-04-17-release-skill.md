# Release Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a project-scoped Claude Code Skill at `.claude/skills/release/` that automates the yosh release flow: pre-checks, tests, workspace-wide patch bump, ordered `cargo publish`, and push with a version tag.

**Architecture:** SKILL.md (read by Claude) handles interactive pre-checks (git status, branch choice) via `AskUserQuestion`, then calls `scripts/release.sh <phase>` for each deterministic phase (test → bump → publish → push). On any non-zero exit from the script, Claude surfaces stderr and stops; no autonomous recovery.

**Tech Stack:** Bash (portable between macOS BSD and Linux GNU: uses `awk` for conditional rewrites and string-based `sed` for known substitutions), Cargo workspace (4 publishable crates), Claude Code Skill frontmatter (`disable-model-invocation`, `allowed-tools`).

**Spec:** `docs/superpowers/specs/2026-04-17-release-skill-design.md`

**File structure:**
- Create `.claude/skills/release/SKILL.md` — Claude-facing instructions and phase orchestration
- Create `.claude/skills/release/scripts/release.sh` — executable script, dispatches on subcommand (`test|bump|publish|push`)

**Crate publish order (fixed):** `yosh-plugin-api` → `yosh-plugin-sdk` → `yosh-plugin-manager` → `yosh`

**Out of scope:** dry-run mode, automated tests for the skill/script, minor/major version bumps, selective per-crate bumps, autonomous recovery.

---

## Task 1: Create skill directory and release.sh skeleton with phase dispatch

**Files:**
- Create: `.claude/skills/release/scripts/release.sh`

- [ ] **Step 1: Create the directory**

```bash
mkdir -p .claude/skills/release/scripts
```

- [ ] **Step 2: Write `release.sh` skeleton**

Create `.claude/skills/release/scripts/release.sh` with this exact content:

```bash
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
  fail "test phase not implemented yet"
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
```

- [ ] **Step 3: Make it executable**

```bash
chmod +x .claude/skills/release/scripts/release.sh
```

- [ ] **Step 4: Sanity check the dispatcher**

Run: `.claude/skills/release/scripts/release.sh`
Expected: exits 1 with stderr `yosh-release: usage: release.sh {test|bump|publish|push}`

Run: `.claude/skills/release/scripts/release.sh test`
Expected: exits 1 with stderr `yosh-release: test phase not implemented yet`

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "feat(release-skill): add release.sh dispatcher skeleton"
```

---

## Task 2: Implement the `test` phase

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh` (replace `phase_test`)

- [ ] **Step 1: Replace `phase_test` function**

Replace the existing `phase_test()` stub in `.claude/skills/release/scripts/release.sh` with:

```bash
phase_test() {
  echo "yosh-release: running cargo test..." >&2
  cargo test || fail "cargo test failed — fix tests and rerun"
  echo "yosh-release: running e2e tests..." >&2
  ./e2e/run_tests.sh || fail "e2e tests failed — fix tests and rerun"
  echo "yosh-release: all tests passed" >&2
}
```

- [ ] **Step 2: Verify the phase runs end-to-end**

Run: `.claude/skills/release/scripts/release.sh test`
Expected: runs `cargo test`, then `./e2e/run_tests.sh`, prints `all tests passed` on stderr, exits 0.

(If either suite fails for project reasons, fix them separately — they are preconditions for a real release anyway.)

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "feat(release-skill): implement test phase"
```

---

## Task 3: Implement the `bump` phase

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh` (replace `phase_bump`, add helpers)

**Behavior:** reads current `[package].version` from root `Cargo.toml`, verifies all 4 crates share that version (abort on mismatch), increments patch, rewrites `[package].version` in all 4 Cargo.toml files, rewrites the two `yosh-plugin-api = { version = "X.Y.Z", ... }` dependency pins (in root `Cargo.toml` and in `crates/yosh-plugin-sdk/Cargo.toml`), runs `cargo build` to refresh `Cargo.lock`, then creates a release commit.

- [ ] **Step 1: Add version helper functions above `phase_test`**

Insert these helpers in `.claude/skills/release/scripts/release.sh` immediately below the `fail()` function:

```bash
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
```

- [ ] **Step 2: Replace `phase_bump` function**

Replace the `phase_bump()` stub with:

```bash
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

  # Rewrite workspace dep pins (yosh-plugin-api is pinned in root and in sdk).
  rewrite_dep_version "Cargo.toml" "yosh-plugin-api" "$old" "$new" \
    || fail "failed to rewrite yosh-plugin-api pin in Cargo.toml"
  rewrite_dep_version "crates/yosh-plugin-sdk/Cargo.toml" "yosh-plugin-api" "$old" "$new" \
    || fail "failed to rewrite yosh-plugin-api pin in yosh-plugin-sdk/Cargo.toml"

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
  # Expose new version to later phases / to callers via stdout as last line.
  echo "$new"
}
```

- [ ] **Step 3: Dry-check on a throwaway branch (no publish/push)**

Run on a scratch branch to verify the edit and commit work, then revert:

```bash
git switch -c release-skill-bump-smoketest
.claude/skills/release/scripts/release.sh bump
# Inspect the commit:
git log -1 --stat
# Inspect the diff that was committed:
git show HEAD
# Revert and delete the scratch branch:
git switch main
git branch -D release-skill-bump-smoketest
```

Expected: a single commit updating 5 files (4 Cargo.toml + Cargo.lock), diff shows `version = "0.1.1"` → `version = "0.1.2"` in [package] sections, and `yosh-plugin-api = { version = "0.1.1", ... }` → `yosh-plugin-api = { version = "0.1.2", ... }` in root `Cargo.toml` and `crates/yosh-plugin-sdk/Cargo.toml`.

- [ ] **Step 4: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "feat(release-skill): implement bump phase"
```

---

## Task 4: Implement the `publish` phase

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh` (replace `phase_publish`)

**Behavior:** publishes the 4 crates in fixed order. Supports `--from <crate-name>` for idempotent resume after a mid-sequence failure. Abort on first error with a recovery hint that names the `--from` argument to resume.

- [ ] **Step 1: Replace `phase_publish` function**

Replace the `phase_publish()` stub with:

```bash
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
      cmd=(cargo publish)
    else
      cmd=(cargo publish -p "$crate")
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
```

- [ ] **Step 2: Verify argument parsing without actually publishing**

Run: `.claude/skills/release/scripts/release.sh publish --from bogus-crate`
Expected: exits 1 with stderr `yosh-release: --from 'bogus-crate' is not a known crate (expected one of: yosh-plugin-api yosh-plugin-sdk yosh-plugin-manager yosh)`

Run: `.claude/skills/release/scripts/release.sh publish --from`
Expected: exits 1 with stderr mentioning `--from requires a crate name`.

(Do NOT run the command without `--from` / with a valid crate name here — that would actually publish. The real publish is exercised only during an actual release.)

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "feat(release-skill): implement publish phase with --from resume"
```

---

## Task 5: Implement the `push` phase

**Files:**
- Modify: `.claude/skills/release/scripts/release.sh` (replace `phase_push`)

**Behavior:** pushes `main` to origin, then creates and pushes a `v<version>` tag. Reads the current version from root `Cargo.toml` to build the tag name.

- [ ] **Step 1: Replace `phase_push` function**

Replace the `phase_push()` stub with:

```bash
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
```

- [ ] **Step 2: Sanity check that the version is read correctly**

Run: `.claude/skills/release/scripts/release.sh push` with a network-disconnected shell OR just trace the logic by running:

```bash
bash -c '
  source .claude/skills/release/scripts/release.sh 2>/dev/null || true
  # Re-source only the helper after the main guard returns:
  cd .
  awk "/^\[package\]/,/^\[/" Cargo.toml | grep "^version"
'
```

Or simpler — just verify the helper `read_package_version` already works from Task 3 by inspection. The push phase has no standalone test; it is exercised during a real release.

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/release/scripts/release.sh
git commit -m "feat(release-skill): implement push phase"
```

---

## Task 6: Write SKILL.md with pre-check orchestration

**Files:**
- Create: `.claude/skills/release/SKILL.md`

- [ ] **Step 1: Create SKILL.md with frontmatter and instructions**

Create `.claude/skills/release/SKILL.md` with this exact content:

```markdown
---
name: release
description: Releases yosh to crates.io by bumping all workspace crate versions, running tests, publishing in dependency order, and pushing main with a version tag. Invoked explicitly by the user only.
disable-model-invocation: true
allowed-tools: Bash, Read, Edit, AskUserQuestion
---

# Release Skill

Automates the full yosh release flow. The user invoking this skill IS the approval: do not add confirmation gates except where this document explicitly instructs you to ask the user.

`cargo publish` is irreversible (crates.io supports `yank` but not true deletion). If any phase fails, STOP, surface the script's stderr to the user, and wait. Do not autonomously run `git reset`, `git checkout`, or any other recovery action.

## Phases (run in order)

### Phase 1: Pre-checks (you drive this directly)

1. Run: `git status --porcelain`
   If the output is non-empty, the working tree has uncommitted changes. Use **AskUserQuestion** with:
   - Question: "The working tree has uncommitted changes. How should I proceed?"
   - Options:
     - `commit` — Commit the changes, then continue
     - `abort` — Stop the release
   - On `commit`: inspect `git diff` and `git diff --cached`, craft a concise commit message (imperative mood, describing the nature of the changes), run `git add -A`, then `git commit -m "<message>"`. Do NOT use `--no-verify`. If the commit fails, surface the error and stop.
   - On `abort`: print "Release aborted by user." and stop.

2. Run: `git branch --show-current`
   If the current branch is not `main`, use **AskUserQuestion** with:
   - Question: "You are on branch `<current>`. How do you want to reach main?"
   - Options:
     - `merge` — Switch to main and merge `<current>` into it
     - `switch-only` — Switch to main without merging (leave `<current>` as-is)
     - `abort` — Stop the release
   - On `merge`: run `git switch main`, then `git merge <current> --no-edit`. If `git merge` exits non-zero (conflict), print "Merge conflict on main. Run `git merge --abort` or resolve manually, then rerun /release." and stop.
   - On `switch-only`: run `git switch main`.
   - On `abort`: print "Release aborted by user." and stop.

### Phase 2: Tests

Run: `.claude/skills/release/scripts/release.sh test`

If exit code is non-zero, surface stderr verbatim and stop.

### Phase 3: Version bump

Run: `.claude/skills/release/scripts/release.sh bump`

If exit code is non-zero, surface stderr verbatim and stop. On success, the last line of stdout is the new version (e.g. `0.1.2`); remember it for the summary.

### Phase 4: Publish

Run: `.claude/skills/release/scripts/release.sh publish`

If exit code is non-zero, surface stderr verbatim (it already includes the `--from <crate>` resume hint) and stop. Do NOT attempt the push phase.

### Phase 5: Push

Run: `.claude/skills/release/scripts/release.sh push`

If exit code is non-zero, surface stderr verbatim and stop.

## Completion

When all five phases succeed, report a brief summary:

> Released yosh v<new>. Published 4 crates to crates.io, pushed main + tag v<new> to origin.
```

- [ ] **Step 2: Verify the file exists and is readable**

Run: `ls -la .claude/skills/release/`
Expected: lists `SKILL.md` and `scripts/` directory.

Run: `head -6 .claude/skills/release/SKILL.md`
Expected: shows the YAML frontmatter with `name: release` and `disable-model-invocation: true`.

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/release/SKILL.md
git commit -m "feat(release-skill): add SKILL.md with pre-check orchestration"
```

---

## Task 7: Final integration check (non-destructive)

**Files:** (no changes, verification only)

- [ ] **Step 1: Verify layout**

Run: `find .claude/skills/release -type f`
Expected:
```
.claude/skills/release/SKILL.md
.claude/skills/release/scripts/release.sh
```

- [ ] **Step 2: Verify script permissions**

Run: `ls -l .claude/skills/release/scripts/release.sh`
Expected: the file is executable (mode starts with `-rwx`).

- [ ] **Step 3: Verify dispatcher handles all phase names**

Run each of these; each should either run the phase or exit 1 with a specific in-phase error (not "unknown phase"):

```bash
.claude/skills/release/scripts/release.sh bogus      # expected: "yosh-release: unknown phase: bogus"
.claude/skills/release/scripts/release.sh            # expected: "yosh-release: usage: ..."
```

Do NOT run `test`/`bump`/`publish`/`push` here — those are for a real release.

- [ ] **Step 4: Confirm the skill is discoverable by Claude Code**

Manually: restart the Claude Code session (or reload skills) and confirm the `release` skill appears in the skills list with `disable-model-invocation: true`. This is a human verification step — there is no CLI assertion for it.

- [ ] **Step 5: No commit needed (verification only)**

If everything checks out, the implementation is complete. The real end-to-end verification is the first actual invocation of `/release` to cut v0.1.2.

---

## Self-Review Notes

- Spec coverage:
  - File layout (spec §File Layout) → Tasks 1, 6
  - Frontmatter with `disable-model-invocation`, `allowed-tools` (spec §SKILL.md Frontmatter) → Task 6
  - Responsibility split (spec §Responsibility Split) → Tasks 2–6 implement script phases; Task 6 places interactive checks in SKILL.md
  - Phase 1 pre-checks, uncommitted-changes `commit|abort` choice, branch `merge|switch-only|abort` choice, merge-conflict handling (spec §Workflow Phase 1) → Task 6
  - Phase 2 tests: `cargo test` then `./e2e/run_tests.sh` (spec §Phase 2) → Task 2
  - Phase 3 bump: read version, verify all-equal, patch++, rewrite 4 `[package].version`, rewrite `yosh-plugin-api` pin in root and sdk, `cargo build` for Cargo.lock, commit (spec §Phase 3) → Task 3
  - Phase 4 publish in fixed order, `--from <crate>` resume (spec §Phase 4, §Error Handling) → Task 4
  - Phase 5 push main → tag → push tag (spec §Phase 5) → Task 5
  - Error handling: stop-on-fail, script emits recovery guidance, SKILL.md does not autonomously recover (spec §Error Handling and Recovery) → Tasks 2–6
  - Assumption: `cargo publish` 1.66+ waits for index propagation (spec §Assumptions) → no sleep added in Task 4, matches spec
- Type consistency: phase names (`test|bump|publish|push`), crate list order, and the `CRATES` array are consistent across Tasks 1, 4, and 7.
- Placeholder scan: no TBDs, no "handle appropriately", every code step shows full code.
