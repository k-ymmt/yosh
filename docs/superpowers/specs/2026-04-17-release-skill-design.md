# Release Skill Design

A project-scoped Claude Code Skill that automates the yosh release flow: version bump across the workspace, tests, `cargo publish` in dependency order, and pushing `main` with a version tag.

## Goals

- Single-command release for yosh. User invokes `/release`; the skill drives everything from pre-checks through publish and push.
- Deterministic and auditable. Mechanical steps (version bump, publish sequence, push) live in a shell script that can be run standalone.
- Fail-closed on errors. Publish is irreversible (crates.io only supports `yank`), so on any failure the skill stops immediately and prints recovery instructions.
- No accidental invocation. The skill is explicitly user-invoked only; Claude must not auto-trigger it from description matching.

## Non-goals

- Supporting minor or major version bumps. Patch-only (`0.0.1` increment).
- Selective per-crate bumps. All four workspace crates are always bumped together.
- Automated rollback. On failure, the user handles recovery with the printed guidance.
- Dry-run mode. Out of scope.
- Automated tests for the skill or its script. Out of scope.

## File Layout

```
.claude/skills/release/
├── SKILL.md          # Instructions Claude follows: interactive checks + phase orchestration
└── scripts/
    └── release.sh    # Deterministic phases: test / bump / publish / push
```

## SKILL.md Frontmatter

```yaml
---
name: release
description: Releases yosh to crates.io by bumping all workspace crate versions, running tests, publishing in dependency order, and pushing main with a version tag. Invoked explicitly by the user only.
disable-model-invocation: true
allowed-tools: Bash, Read, Edit, AskUserQuestion
---
```

- `disable-model-invocation: true` — the skill never auto-fires from description matching. User must invoke explicitly.
- `allowed-tools` restricts runtime permissions to what the skill actually needs. `Write` is intentionally absent (no new files created during a release).

## Responsibility Split

| Concern | Owner | Reason |
|---|---|---|
| git status check, branch check | SKILL.md (Claude) | Requires interactive questions to the user |
| Uncommitted-change flow | SKILL.md (Claude) | Needs user choice (commit vs abort) and diff-based message generation |
| Non-main branch flow | SKILL.md (Claude) | Needs 3-way user choice at runtime |
| `cargo test` + e2e run | `release.sh test` | Mechanical, no user decisions |
| Version bump across 4 Cargo.toml files + Cargo.lock + commit | `release.sh bump` | Mechanical, must be deterministic |
| `cargo publish` for 4 crates in fixed order | `release.sh publish` | Mechanical, order is invariant |
| `git push origin main` + tag + tag push | `release.sh push` | Mechanical |

SKILL.md calls `release.sh <phase>` via Bash and inspects exit codes. It does not re-implement bump/publish logic.

## Workflow

### Phase 1: Pre-checks (SKILL.md)

1. Run `git status`.
   - If the working tree is dirty, use AskUserQuestion:
     - **commit**: Claude reads the diff, generates a commit message, and commits.
     - **abort**: Skill exits 0 with a message.
2. Check current branch.
   - If not `main`, use AskUserQuestion with three options:
     - **merge**: Switch to `main`, `git merge <prev-branch> --no-edit`. On merge conflict, abort without resolving; print guidance to resolve manually and rerun.
     - **switch-only**: `git switch main` without merging; the current branch stays where it is.
     - **abort**: Skill exits 0.

### Phase 2: Tests (`release.sh test`)

1. `cargo test` — full unit + integration suite. Fail → abort.
2. `./e2e/run_tests.sh` — E2E POSIX compliance. Fail → abort.

(No release build step: e2e uses debug build per `CLAUDE.md`.)

### Phase 3: Version bump (`release.sh bump`)

1. Read the `[package].version` of the root `Cargo.toml`.
2. Verify all four workspace crates share the same version. Mismatch → abort.
3. Compute the new version by incrementing the patch component (e.g., `0.1.1` → `0.1.2`).
4. Rewrite four `Cargo.toml` files:
   - `Cargo.toml` (root, `yosh`)
   - `crates/yosh-plugin-api/Cargo.toml`
   - `crates/yosh-plugin-sdk/Cargo.toml`
   - `crates/yosh-plugin-manager/Cargo.toml`
5. Rewrite dependency version pins that reference workspace crates. Current known sites:
   - root `Cargo.toml`: `yosh-plugin-api = { version = "X.Y.Z", path = "..." }`
   - `crates/yosh-plugin-sdk/Cargo.toml`: same
6. Run `cargo build` to regenerate `Cargo.lock`.
7. `git add` the four `Cargo.toml` files and `Cargo.lock`.
8. `git commit -m "chore: release v<new>\n\n- yosh, yosh-plugin-api, yosh-plugin-sdk, yosh-plugin-manager: <old> -> <new>"`.

The script uses plain `sed`/`grep`. It does not depend on `cargo-edit` or other external tools.

### Phase 4: Publish (`release.sh publish`)

Publish in fixed dependency order. The first failure aborts the whole release.

1. `cargo publish -p yosh-plugin-api`
2. `cargo publish -p yosh-plugin-sdk`
3. `cargo publish -p yosh-plugin-manager`
4. `cargo publish` (root `yosh`)

`cargo publish` (1.66+) waits for the index to propagate before returning, so no explicit sleep is needed between crates.

### Phase 5: Push (`release.sh push`)

1. `git push origin main`
2. `git tag v<new>`
3. `git push origin v<new>`

Order is: commit (phase 3) → publish (phase 4) → push + tag (phase 5). If publish fails, nothing has been pushed to origin; the bump commit remains local only, which is recoverable.

## Error Handling and Recovery

The script uses `set -euo pipefail`. On any failure it prints to stderr: which phase failed, what command failed, and the specific next step to recover, then exits non-zero.

| Phase | Failure | Remaining state | Recovery guidance printed |
|---|---|---|---|
| Pre-check | commit fails | Changes unstaged or partially staged | `git status`, resolve manually, rerun `/release` |
| Test | `cargo test` or e2e fails | No changes | Fix tests, rerun `/release` |
| Bump | sed / Cargo.lock update fails | Cargo.toml may be partially rewritten | `git checkout Cargo.toml crates/*/Cargo.toml Cargo.lock`, rerun |
| Bump | `git commit` fails | Bumped, uncommitted | Commit manually or `git checkout` to revert, rerun |
| Publish | `yosh-plugin-api` fails | Bump committed locally, nothing published | Fix the cause, run `scripts/release.sh publish` |
| Publish | subsequent crate fails | Earlier crates are public; later crates are not | Use `scripts/release.sh publish --from <crate>` to resume |
| Push | `git push origin main` fails | Publish complete, nothing pushed | Resolve remote divergence, run `scripts/release.sh push` |
| Push | tag push fails | Main pushed, tag not pushed | Run `git push origin v<new>` manually |

The `publish` phase supports `--from <crate-name>` for idempotent resume. Claude does not take recovery actions autonomously; it surfaces the script's stderr to the user and waits.

## Invocation

The user invokes the skill explicitly. With `disable-model-invocation: true`, the skill does not auto-fire. Invocation happens via the `Skill` tool by name (`release`) when the user types `/release` or otherwise explicitly requests it.

Interactive decisions inside phase 1 use the `AskUserQuestion` tool so choices are recorded cleanly in the transcript.

## Assumptions and Constraints

- The user has valid `~/.cargo/credentials.toml` for crates.io. Auth failure surfaces as a phase 4 failure with recovery guidance.
- All four workspace crates are currently on the same version. This is a precondition enforced by `release.sh bump`.
- `origin` points to `https://github.com/k-ymmt/yosh`. The script does not validate the remote URL.
- The user understands that `cargo publish` is irreversible except via `yank`. The lack of confirmation gates is an explicit trade: the `/release` invocation itself is the approval.
