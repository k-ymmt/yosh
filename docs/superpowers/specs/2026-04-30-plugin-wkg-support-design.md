# Plugin wkg Support — Design

**Date:** 2026-04-30
**Status:** Approved (brainstorming)
**Scope:** Publish the `yosh:plugin` WIT package to wa.dev so external
plugin authors no longer need a yosh checkout for `cargo component build`.

## 1. Goal

Plugin authors today must reference yosh's source tree to compile
against the `yosh:plugin` WIT interface:

```toml
[package.metadata.component.target.dependencies."yosh:plugin"]
path = "<path-to-yosh-checkout>/crates/yosh-plugin-api/wit"
```

After this work, they can use a registry-resolved version reference:

```toml
[package.metadata.component.target.dependencies."yosh:plugin"]
version = "0.2"
```

This is achieved by publishing the `yosh:plugin` WIT package to
[wa.dev] (the Bytecode Alliance's WebAssembly registry, the default
resolution target for `wkg`/`cargo component`).

[wa.dev]: https://wa.dev/

## 2. Non-Goals

- **Plugin binary distribution via OCI registries.** `yosh plugin
  install` continues to use GitHub Releases for `.wasm` binaries; the
  `PluginSource` enum (`GitHub` / `Local`) is unchanged. This design
  covers only the WIT interface package, not plugin artifacts.
- **`yosh plugin publish` command.** Plugin authors who want to push
  their own components to OCI registries can do so with `wkg publish`
  directly; yosh does not wrap that workflow.
- **In-repo test plugins.** `tests/plugins/test_plugin/` and
  `tests/plugins/trap_plugin/` keep `path = "../../crates/yosh-plugin-api/wit"`.
  In-tree resolution avoids depending on wa.dev for `cargo test` and
  for offline development.
- **CI-driven publish.** Publishing happens from the maintainer's
  local machine via `release.sh`; no GitHub Actions integration in
  this iteration.
- **`yosh-plugin-manager` / `yosh-plugin-sdk` changes.** Neither crate
  is touched.

## 3. Architecture Overview

### 3.1 Files Changed

1. `.claude/skills/release/scripts/release.sh` — new function
   `phase_publish_wit`, wired into the release flow between
   `phase_publish` (crates.io) and `phase_push` (git push).
2. `crates/yosh-plugin-api/wit/yosh-plugin.wit` — the package version
   line `package yosh:plugin@<x.y.z>;` is rewritten by `release.sh`
   when (and only when) the WIT content changes. Other content is
   unchanged by this work.
3. `crates/yosh-plugin-api/.last-published-wit.sha256` — new
   git-tracked file, 64 hex chars + newline. Holds the SHA-256 of the
   last-published WIT content (excluding the `package` version line).
4. `crates/yosh-plugin-api/tests/wit_format.rs` — new Rust test that
   asserts the WIT file starts with `package yosh:plugin@<SemVer>;`.
5. `.claude/skills/release/tests/test_publish_wit.sh` — new bash test
   harness for `phase_publish_wit` (SHA helper + functional cases with
   stubbed `wkg`/`git`/`cargo`).
6. `docs/kish/plugin.md` — Quick Start example and a new
   "Setting up wkg" subsection.

### 3.2 Release Flow

```
phase_bump          (existing, unchanged)
phase_test          (existing, unchanged)
phase_commit_tag    (existing, unchanged)
phase_publish       (existing, unchanged — cargo publish to crates.io)
phase_publish_wit   (NEW — wkg wit publish to wa.dev, conditional)
phase_push          (existing, unchanged)
```

Placement rationale: `phase_publish_wit` runs **after** crates.io
publish so that the only state we may need to reconcile manually is
"crates.io and wa.dev both have version X" (idempotent), never
"wa.dev published but crates.io rejected" (hard to roll back).

### 3.3 Plugin Author Experience

| Case | Action |
|------|--------|
| Existing plugin using `path = "..."` | Continues to work; plugin author may opt in to `version = "..."` at any time. |
| New plugin (yosh 0.2.x onward) | Install `wkg`, set `version = "0.2"`, build. No yosh checkout required. |
| Yosh's own in-repo test plugins | Unchanged (still `path = "..."`). |

## 4. `phase_publish_wit` Detail

### 4.1 Inputs

- `crates/yosh-plugin-api/wit/yosh-plugin.wit`
- `crates/yosh-plugin-api/.last-published-wit.sha256` (may be absent on first run)
- `crates/yosh-plugin-api/Cargo.toml` `version` field (post-`phase_bump`)
- `wkg` binary on `PATH`
- Existing wkg/wa.dev auth in `~/.config/wasm-pkg/config.toml` (or
  `WKG_TOKEN` env var) — yosh delegates auth handling to wkg.

### 4.2 Algorithm

```
phase_publish_wit:

  1. command -v wkg >/dev/null \
       || die "wkg not found in PATH. Install via 'cargo install wkg --locked'"

  2. CRATE_VER = (cargo metadata: yosh-plugin-api.version)

  3. Verify the WIT file has exactly one matching `package` line:
       grep -c '^package yosh:plugin@' "$WIT" == 1
     else die "WIT package declaration missing or duplicated"

  4. Verify HEAD matches the tag created by phase_commit_tag:
       git rev-parse HEAD == git rev-list -n 1 "v$CRATE_VER"
     else die "HEAD diverged from tag v$CRATE_VER (manual commit?)"

  5. Compute content SHA, excluding the version line:
       NEW_SHA = sha256(grep -v '^package yosh:plugin@' "$WIT")

  6. OLD_SHA = (cat .last-published-wit.sha256 if present, else "")

  7. if NEW_SHA == OLD_SHA:
       echo "WIT unchanged (sha256=$NEW_SHA), skip wkg wit publish"
       return 0

  8. Rewrite package line in place:
       sed -i.bak "s|^package yosh:plugin@.*|package yosh:plugin@${CRATE_VER};|" "$WIT"
       rm -f "${WIT}.bak"

  9. wkg wit publish "$(dirname $WIT)" \
       || die "wkg wit publish failed"

  10. echo "$NEW_SHA" > .last-published-wit.sha256

  11. git add yosh-plugin.wit .last-published-wit.sha256
      git commit --amend --no-edit
      git tag -f "v$CRATE_VER" HEAD

  12. echo "WIT published as yosh:plugin@${CRATE_VER}, sha256=$NEW_SHA"
```

### 4.3 Why SHA Excludes the Version Line

The version line itself is rewritten by step 8 every time the
content changes, so including it in the SHA would make the SHA flip
twice on each release (once for the real change, once for the
version bump). Excluding it makes SHA equality strictly track
**interface** equality, which is what we want for "skip publish when
unchanged".

### 4.4 Why Rewrite the Version Line Only at Publish Time

`phase_bump` (which bumps Cargo.toml versions) deliberately does not
touch the WIT file. The WIT version is only synchronized with the
crate version at the moment we publish. Consequence: the WIT file
in the repo can lag behind the crate version when no interface
change has occurred — this is fine and matches what wa.dev shows.

### 4.5 `commit --amend` and `tag -f` Safety

These operations rewrite history, but they target a commit and tag
that `release.sh` itself just created in `phase_commit_tag` and that
have not yet been pushed to the remote (push happens in `phase_push`,
later). Step 4 (HEAD vs. tag check) guards against a maintainer
accidentally committing between phases. The CLAUDE.md prohibition on
force-pushing applies to remote refs only, so local amend is
acceptable.

## 5. Versioning Policy

- WIT package version is rewritten to match
  `yosh-plugin-api`'s crate version **only when WIT content changes**
  (SHA-detected).
- For releases that do not touch the interface (typical patch
  releases), the WIT file's `package yosh:plugin@<x>;` line stays at
  the previous version and `wkg wit publish` is skipped.
- This effectively means: WIT version == "the crate version at the
  most recent interface change". This matches the user's stated
  intent ("sync with crate") while avoiding empty wa.dev publishes.

## 6. Documentation Changes (`docs/kish/plugin.md`)

### 6.1 Quick Start `Cargo.toml` Example

In the Quick Start `Cargo.toml` block, replace the
`[package.metadata.component.target.dependencies."yosh:plugin"]`
entry that points at a `path = "<path-to-yosh-checkout>/..."` with:

```toml
[package.metadata.component.target.dependencies."yosh:plugin"]
version = "0.2"
```

### 6.2 New Subsection: "Setting up wkg" (insert before Quick Start step 4)

The new subsection has the title `### Setting up wkg` and reads:

> The `yosh:plugin` WIT package is published to [wa.dev], the
> Bytecode Alliance's WebAssembly registry. Plugin authors need `wkg`
> installed and configured to resolve the dependency.
>
> 1. Install `wkg`:
>
>    ```sh
>    cargo install wkg --locked
>    # or: brew install bytecodealliance/tap/wkg
>    ```
>
> 2. Configure wa.dev as the default registry (one-time, persists in
>    `~/.config/wasm-pkg/config.toml`):
>
>    ```sh
>    wkg config --default-registry wa.dev
>    ```
>
> 3. `cargo component build` invokes `wkg` automatically to fetch
>    `yosh:plugin@<version>` on first build.
>
> [wa.dev]: https://wa.dev/

### 6.3 No Migration Note

The `path = "..."` form remains a private detail used only by yosh's
own in-repo test plugins. External docs use `version = "0.2"`
exclusively. (User-confirmed during brainstorming.)

### 6.4 No Changes To

- `CLAUDE.md` (in-repo test plugins keep `path = "..."`)
- "The Plugin Trait", "export! Macro", "Distributing via GitHub
  Releases" sections of `plugin.md` (unrelated to WIT resolution)

## 7. Error Handling

| # | Condition | Detection | Behavior |
|---|-----------|-----------|----------|
| 1 | `wkg` missing from PATH | `command -v wkg` | Abort at top of phase. `cargo publish` already done; rerun release.sh after installing wkg (idempotent — SHA-skip protects re-publish). |
| 2 | wa.dev auth failure / network error | `wkg wit publish` non-zero exit | Abort, propagate stderr. Maintainer fixes auth/network and reruns. |
| 3 | Same version already on wa.dev | wkg stderr | Should not occur (SHA-skip catches it earlier). If it does, the local `.last-published-wit.sha256` is out of sync with wa.dev — maintainer runs `wkg wit get yosh:plugin@<ver>`, compares, and either updates the SHA file (true match) or fixes a missed crate-version bump (real divergence). |
| 4 | Missing/duplicated `package` line in WIT | step 3 grep count check | Abort. |
| 5 | `cargo metadata` fails / version unreadable | non-zero exit | Abort. |
| 6 | HEAD diverged from tag | step 4 rev-parse comparison | Abort with "HEAD diverged from tag" message. |
| 7 | Partial success (publish OK, but step 10 SHA-write or step 11 amend/tag failed) | `git status` and `cat .last-published-wit.sha256` reveal which steps ran | Manual recovery, `wkg` is **not** retried (wa.dev rejects duplicate versions). If SHA file is up-to-date but the amend never ran, working tree shows uncommitted WIT + SHA changes — run `git add yosh-plugin.wit .last-published-wit.sha256 && git commit --amend --no-edit && git tag -f v<ver> HEAD` manually. If the SHA file was not updated, `wkg wit get yosh:plugin@<ver>` confirms wa.dev state and the maintainer either rolls the SHA file forward (publish succeeded) or re-runs after diagnosing why publish appeared to succeed. |

### 7.1 Plugin-Author-Side Failures (Out of Scope, but Noted)

| # | Condition | Yosh's responsibility |
|---|-----------|------------------------|
| α | Plugin author specifies `version = "0.1"` and 0.1.x is on wa.dev | None — wkg resolves normally. |
| β | `wkg` not installed | None — cargo-component surfaces the error; docs (§6.2) cover setup. |
| γ | wa.dev temporarily down | None — author may temporarily fall back to `path = "..."` against a yosh checkout if they have one. Docs do not mention this fallback to avoid encouraging the legacy form. |

### 7.2 Deliberately Not Implemented

- **Strict `wkg` version pinning.** wkg is on a 0.x release line; version checks would rot. Documentation states "tested with wkg 0.x" without a hard floor.
- **`wkg wit publish --dry-run` pre-check.** SHA comparison already serves as the pre-check.
- **WIT lint before publish.** `cargo component build -p test_plugin` in `phase_test` already exercises the WIT file end-to-end.

## 8. Testing Strategy

### 8.1 SHA Helper Unit Tests (Automated)

Extract the SHA computation into a function `compute_wit_content_sha`
within `release.sh`. Test in `.claude/skills/release/tests/test_publish_wit.sh`:

1. Standard WIT fixture → matches a known fixed SHA.
2. Same content with only the `package yosh:plugin@<x>;` line changed
   → SHA matches case 1 (proves version-line exclusion).
3. Interface change (added field/function) → SHA differs.
4. Comment-only diff → SHA differs (we hash everything except the
   version line; comments count as content).

### 8.2 `phase_publish_wit` Functional Tests (Automated)

Same test file (`test_publish_wit.sh`). Stub `wkg`, `git`, `cargo` as
bash functions that record their invocations into temp logs.

Cases:

1. **First publish** — no `.last-published-wit.sha256` present. Assert:
   `wkg` was called, SHA file created, WIT version line rewritten to
   the crate version.
2. **Unchanged skip** — pre-populate SHA file matching current
   content. Assert: `wkg` was not called, exit code 0, stdout
   contains "WIT unchanged".
3. **Changed publish** — pre-populate SHA file with a different
   value. Assert: `wkg` called, SHA file updated, version rewritten,
   `git commit --amend` and `git tag -f` invoked.
4. **wkg missing** — temp PATH without wkg. Assert: abort at top,
   error mentions "wkg not found".
5. **wkg fails** — stub returns 1. Assert: abort, stderr propagated.
6. **HEAD divergence** — stub `git rev-parse HEAD` to return a SHA
   different from `git rev-list -n 1 v<ver>`. Assert: abort with
   "HEAD diverged" message.
7. **Missing package line** — fixture WIT without
   `^package yosh:plugin@` line. Assert: abort.

### 8.3 WIT File Format Regression Test (Automated, Rust)

`crates/yosh-plugin-api/tests/wit_format.rs`:

- Read `wit/yosh-plugin.wit`.
- Assert first non-blank line matches the regex
  `^package yosh:plugin@(\d+)\.(\d+)\.(\d+)(-[A-Za-z0-9.-]+)?;$`.
- Runs as part of `cargo test -p yosh-plugin-api`. Catches accidental
  malformation that would break `phase_publish_wit`'s sed step.

### 8.4 Manual cargo-component Smoke (Pre-First-Release)

Once before the first release that includes this work, the
maintainer runs:

1. `cargo component init --lib hello-test` in a fresh tempdir
2. Add `version = "0.2"` form to `Cargo.toml`
3. `cargo component build --target wasm32-wasip2 --release`
4. Confirm success and that the produced `.wasm` is well-formed.

Not in CI (depends on wa.dev availability and cargo-component's
moving registry-resolution code).

### 8.5 First Release Observation (Manual)

On the first `release.sh` run after this work merges:

- Watch `phase_publish_wit` output: should publish (initial state),
  create `.last-published-wit.sha256`, and rewrite the WIT version line.
- Verify `yosh:plugin@<ver>` is visible on wa.dev.
- Optionally run a follow-up patch release: `phase_publish_wit`
  should print "WIT unchanged, skip wkg wit publish".

### 8.6 Coverage Summary

- §8.1 + §8.2 cover the script's main paths and major error branches.
- §8.3 catches WIT format regressions during ordinary `cargo test`.
- §8.4 + §8.5 are one-time validations for the first release; later
  releases rely on the automated layers.

## 9. Rollout

- All changes land in a single PR (small surface, tightly coupled).
- The first release after merge is the smoke test (§8.5).
- Existing plugin authors are not forced to migrate; their
  `path = "..."` references continue working indefinitely.

## 10. Open Questions / Future Work

- **CI-driven publish.** If yosh later adds a release CI on tag
  push, `phase_publish_wit` can be lifted out of `release.sh` into a
  GitHub Actions step with minimal change (just stage the WIT and
  SHA file in the workflow's working tree). Out of scope here.
- **wkg version policy.** When wkg reaches 1.0 and stabilizes its
  CLI, consider adding a soft version floor in
  `docs/kish/plugin.md` for plugin authors.
- **`yosh-plugin-api` crate version vs. published WIT version drift.**
  The two can diverge when the WIT is unchanged across crate patch
  releases. This is intentional (§5). If a future workflow requires
  strict equality, revisit by adding a small tool that re-publishes
  the WIT with a fresh version even on no-content-change releases.
