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
