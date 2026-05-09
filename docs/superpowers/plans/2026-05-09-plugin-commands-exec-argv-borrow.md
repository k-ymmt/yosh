# Plugin perf §4.1 follow-up: Borrow `commands::exec` argv — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `(program: String, args: Vec<String>)` parameters in
`yosh:plugin/commands.exec` with `(WasmStr, WasmList<WasmStr>)`, eliminating
the canonical-ABI lift host-side allocations (`1 String + 1 Vec<String> + N
inner Strings` = `N+2` blocks per crossing) for the last remaining
`Vec<String>` host import in the surface.

**Architecture:** Apply the §4.1 / §4.1 follow-up borrow pattern to
`T = WasmStr`. `WasmList<T>: Lift` for any `T: Lift`, and `WasmStr: Lift`,
so `WasmList<WasmStr>` is a valid host parameter type. The closure iterates
the list, calls `to_str(&store)` per element to produce
`Vec<Cow<'_, str>>`, then hands a `&[Cow<'_, str>]` to a host fn that
builds a single `Vec<&str>` shared between the `CommandPattern` matcher
(generalized to `&[impl AsRef<str>]`) and `spawn_with_timeout`. The
`perf_plugin` test fixture gains a single new no-op command
(`noop_commands_exec`) that goes through the *deny* closure, so the dhat
measurement isolates the canonical-ABI lift cost without subprocess
spawn noise.

**Tech Stack:** Rust 2024, wasmtime 27 component model, `cargo component`
(wasm32-wasip2), dhat-rs heap profiler, Criterion.

**Spec:** `docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md`

---

## File Map

**Created:**
- (none — only edits)

**Modified:**
- `tests/plugins/perf_plugin/src/lib.rs` — add `noop_commands_exec` to `commands()` and `exec()` (no new capability declaration: deny path is intentional).
- `src/plugin/pattern.rs:51` — generalize `CommandPattern::matches` from `&[String]` to `&[impl AsRef<str>]`.
- `src/plugin/host/commands.rs` — update `host_commands_exec`, `deny_commands_exec`, `spawn_with_timeout` signatures; rebuild internal argv as `Vec<&str>`; update 9 unit tests.
- `src/plugin/linker.rs` (lines ~263–280) — change `commands::exec` granted and deny closures to `(WasmStr, WasmList<WasmStr>)`.
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` — append a new Appendix recording the rollout result.
- `TODO.md` — remove the `commands::exec` argv borrow follow-up entry.

**Untouched (verify-only):**
- `crates/yosh-plugin-api/wit/yosh-plugin.wit` — WIT signature unchanged (`exec: func(program: string, args: list<string>) -> result<exec-output, error-code>`).
- `crates/yosh-plugin-sdk/src/lib.rs` — SDK `exec` helper already accepts `&[&str]` and converts guest-side; no change.
- On-disk plugin binaries (`*.cwasm`) — wasm-side ABI unchanged.

---

## Task 1: Add `noop_commands_exec` measurement command to `perf_plugin`

**Files:**
- Modify: `tests/plugins/perf_plugin/src/lib.rs`

**Rationale:** The smoke isolates one `commands::exec` host-import crossing
per loop iteration. `perf_plugin` does NOT declare `Capability::CommandsExec`,
so the linker wires the deny closure regardless of plugins.lock — the deny
path skips `to_str`/`iter` calls and isolates the canonical-ABI *lift* cost
(which is exactly what we're optimizing). No subprocess is spawned.

The 2-element `&["a", "b"]` argv is the smallest non-trivial size; an empty
list might trip a wasmtime fast-path that skips lift entirely.

- [ ] **Step 1: Confirm the SDK `exec` helper signature**

```bash
grep -n "pub fn exec" crates/yosh-plugin-sdk/src/lib.rs
```

Expected:

```
516:pub fn exec(program: &str, args: &[&str]) -> Result<ExecOutput, ErrorCode> {
```

The helper takes `&[&str]` on the guest side and converts to `Vec<String>`
internally for the WIT-bindgen call. Guest-side allocation is irrelevant
to host-side dhat measurement.

- [ ] **Step 2: Read current `perf_plugin` source**

```bash
cat tests/plugins/perf_plugin/src/lib.rs
```

Expected: `PerfPlugin` with 9 commands (post §4.1 follow-up). The `use`
line imports specific SDK helpers; we'll need to add `exec` to it but
qualify the call to avoid colliding with the `Plugin::exec` trait method.

- [ ] **Step 3: Add `noop_commands_exec` command and exec arm**

Modify `tests/plugins/perf_plugin/src/lib.rs`:

Update the `commands()` slice — the new array reads:

```rust
fn commands(&self) -> &[&'static str] {
    &[
        "noop_cmd",
        "noop_var",
        "burst_var",
        "noop_var_set",
        "noop_files_read",
        "noop_files_remove",
        "noop_io_write",
        "noop_files_write_file",
        "noop_files_append_file",
        "noop_commands_exec",
    ]
}
```

Add a matching arm inside `exec()`'s `match command { … }` block, after
the existing `"noop_files_append_file"` arm:

```rust
"noop_commands_exec" => {
    // Deny path measurement: perf_plugin does not declare
    // Capability::CommandsExec, so the linker wires the deny closure
    // for commands::exec. The call still crosses the boundary (lift
    // happens), but the host body short-circuits to Err(Denied)
    // without spawning a subprocess. We discard the result.
    let _ = yosh_plugin_sdk::exec("/bin/echo", &["a", "b"]);
    0
}
```

`yosh_plugin_sdk::exec` is fully qualified rather than imported, because
adding `exec` to the `use` line would collide with the `Plugin::exec`
trait method this `impl` is providing.

**DO NOT add `Capability::CommandsExec` to `required_capabilities()`.**
The deny path is intentional — adding the capability would wire the
granted closure, which spawns `/bin/echo` 1000 times during the dhat
run and pollutes the measurement with process-creation noise.

- [ ] **Step 4: Rebuild the wasm fixture**

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release 2>&1 | tail -5
```

Expected: `Finished release [optimized] target(s) in <N>s` and an
updated `target/wasm32-wasip2/release/perf_plugin.wasm`. If
`cargo-component` complains about `bindings.rs` regeneration (a known
benign nit recorded in TODO.md), continue.

- [ ] **Step 5: Verify the rebuilt wasm**

```bash
ls -la target/wasm32-wasip2/release/perf_plugin.wasm
```

Expected: file exists, mtime within the last minute.

- [ ] **Step 6: Smoke-test that the new command dispatches through the linker**

```bash
cargo test --features test-helpers --test plugin -- t01 2>&1 | tail -10
```

Expected: `test t01_capability_allowlist_applied_to_linker ... ok`.
This confirms the rebuilt wasm still loads. (We're not asserting deny
behavior here — just that the addition didn't break the loader.)

- [ ] **Step 7: Commit**

```bash
git add tests/plugins/perf_plugin/src/lib.rs
git commit -m "$(cat <<'EOF'
test(perf_plugin): add §4.1 follow-up commands::exec measurement command

noop_commands_exec calls yosh_plugin_sdk::exec("/bin/echo", &["a", "b"])
through the deny closure (perf_plugin does not declare CAP_COMMANDS_EXEC),
isolating the canonical-ABI lift cost for the (String, Vec<String>) →
(WasmStr, WasmList<WasmStr>) borrow rollout coming next.

Spec: docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If `tests/plugins/perf_plugin/src/bindings.rs` is dirtied by the rebuild,
do NOT include it in this commit (it auto-regenerates and is the subject
of an existing TODO entry). Verify with:

```bash
git status
```

If `bindings.rs` shows as modified, leave it untracked or revert with
`git checkout -- tests/plugins/perf_plugin/src/bindings.rs`. The committed
wasm consumed by the test loader is the *built* artifact under
`target/`, not the source `bindings.rs`.

---

## Task 2: Capture baseline dhat block count for `noop_commands_exec`

**Files:** none modified (`target/perf/` and `/tmp/yosh-perf-home/` are gitignored / out-of-tree)

**Rationale:** Record the pre-rollout block count for `noop_commands_exec`
so the post-rollout delta is unambiguous. The acceptance gate is
**`B_ce − A_ce ≥ 4000`** (≥ −4,000 blocks per `--exec-loop 1000`),
matching the spec §6.1 prediction (1 program String + 1 args outer Vec
+ 2 args inner Strings = 4 blocks/crossing × 1000 crossings).

**Methodology (mirrors §4.1 follow-up Task 2):**
- Use `HOME=/tmp/yosh-perf-home` to isolate the dhat run from the user's
  real `~/.config/yosh/plugins.lock`. **Never modify the user's actual
  config.**
- Extract the metric from the **stderr `dhat: Total: <N> bytes in <M> blocks`**
  summary line. Do NOT parse `dhat-heap.json`'s `"tb"` field.
- Save artifacts to `target/perf/`; baseline summary to
  `target/perf/argv-rollout-baseline.txt`.

- [ ] **Step 1: Build the dhat-instrumented binary**

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
```

Expected: `Finished profiling profile [optimized + debuginfo]` and
`./target/profiling/yosh-dhat` exists. May take 1–2 minutes — use
`timeout: 300000` (5 minutes) on the bash call.

- [ ] **Step 2: Set up isolated `HOME` with a `plugins.lock` for `perf_plugin`**

```bash
mkdir -p /tmp/yosh-perf-home/.config/yosh
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf"
path = "$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
enabled = true
capabilities = [
    "variables:read",
    "variables:write",
    "files:read",
    "files:write",
    "io",
    "hooks:pre_prompt",
    "hooks:pre_exec",
    "hooks:post_exec",
]
EOF
cat /tmp/yosh-perf-home/.config/yosh/plugins.lock
```

Expected: file written with the same 8 caps as §4.1 follow-up
(no `commands:exec` — deny path is intentional). The `name = "perf"`
matches existing convention; `path` resolves to the wasm rebuilt in
Task 1.

**DO NOT modify `~/.config/yosh/plugins.lock`.** Only the isolated
`HOME=/tmp/yosh-perf-home` is touched.

- [ ] **Step 3: Run baseline for `noop_commands_exec`**

```bash
mkdir -p target/perf
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_commands_exec 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-argv-rollout-noop_commands_exec-baseline.json
```

Expected: the last 5 lines include
`dhat: Total: <bytes> bytes in <blocks> blocks` and
`dhat: The data has been saved to dhat-heap.json`. Record `<blocks>`
as `B_ce`. Plausible range: low to mid thousands (similar §4.1
follow-up smokes were 3,497).

If the dhat run prints a plugin-loading error instead of the dhat
summary, the wasm path or capability list is wrong — investigate
before proceeding. If `noop_commands_exec` is reported as "command
not found", the wasm rebuild from Task 1 didn't propagate; rerun
Task 1 Step 4.

- [ ] **Step 4: Save the baseline summary**

```bash
cat > target/perf/argv-rollout-baseline.txt <<EOF
=== §4.1 follow-up commands::exec argv borrow dhat baseline (commit d85fdf4, before Tasks 3–6) ===
noop_commands_exec  bytes=<bytes_ce>  blocks=<B_ce>

Expected post-rollout delta (per --exec-loop 1000):
  noop_commands_exec: −4,000 blocks (1 program String + 1 args outer Vec + 2 args inner Strings = 4 blocks/crossing × 1000 crossings)

Methodology: HOME=/tmp/yosh-perf-home isolation; metric = "dhat: Total: ... blocks" summary line.
Deny path: perf_plugin does not declare CAP_COMMANDS_EXEC, so the linker wires the deny closure.
EOF
cat target/perf/argv-rollout-baseline.txt
```

Replace `<bytes_ce>` and `<B_ce>` with the recorded integers. (No
commit — `target/perf/` is gitignored scratch.)

---

## Task 3: Generalize `CommandPattern::matches` to `&[impl AsRef<str>]`

**Files:**
- Modify: `src/plugin/pattern.rs` (function signature at line 51, plus optional new test)
- Test: `src/plugin/pattern.rs::tests` (existing tests pass unchanged because `String: AsRef<str>`)

**Rationale:** The `host_commands_exec` rewrite in Task 4 will pass a
`Vec<&str>` (built from `&[Cow<'_, str>]`) to `matches`. The current
signature `matches(&self, &[String])` rejects `&[&str]`. Generalize via
`S: AsRef<str>` so existing `String` callsites stay zero-cost
(monomorphic) and the new `&str` callsite type-checks.

- [ ] **Step 1: Read the current matcher**

```bash
sed -n '50,60p' src/plugin/pattern.rs
```

Expected:

```rust
/// Match this pattern against an argv slice (`[program, arg1, arg2, ...]`).
pub fn matches(&self, argv: &[String]) -> bool {
    if self.has_glob_suffix {
        argv.len() >= self.tokens.len() && self.tokens.iter().zip(argv).all(|(p, a)| p == a)
    } else {
        argv.len() == self.tokens.len() && self.tokens.iter().zip(argv).all(|(p, a)| p == a)
    }
}
```

The comparison `p == a` is between `&String` and `&String` (since `argv:
&[String]` and `self.tokens: Vec<String>`).

- [ ] **Step 2: Write a failing test for the new generic shape**

Add this test to the `mod tests` block at the bottom of
`src/plugin/pattern.rs`:

```rust
#[test]
fn matches_accepts_str_slice_argv() {
    // Locks down the §4.1 follow-up generalization: matcher must
    // accept &[&str] (used by host_commands_exec after the borrow
    // rollout) as well as &[String] (existing callsites).
    let p = CommandPattern::parse("/bin/echo:*").unwrap();
    let argv: &[&str] = &["/bin/echo", "a", "b"];
    assert!(p.matches(argv));
}
```

- [ ] **Step 3: Run it to confirm it fails**

```bash
cargo test --features test-helpers --lib plugin::pattern::tests::matches_accepts_str_slice_argv 2>&1 | tail -10
```

Expected: compile error like
`expected `&[String]`, found `&[&str]`` or
`type mismatch resolving `&str: PartialEq<String>``.

- [ ] **Step 4: Generalize `CommandPattern::matches`**

Replace the `matches` method body in `src/plugin/pattern.rs:51` with:

```rust
/// Match this pattern against an argv slice (`[program, arg1, arg2, ...]`).
///
/// Generic over `S: AsRef<str>` so callers can pass `&[String]`
/// (existing) or `&[&str]` / `&[Cow<'_, str>]` (the canonical-ABI
/// borrow path in `host_commands_exec`).
pub fn matches<S: AsRef<str>>(&self, argv: &[S]) -> bool {
    if self.has_glob_suffix {
        argv.len() >= self.tokens.len()
            && self.tokens.iter().zip(argv).all(|(p, a)| p.as_str() == a.as_ref())
    } else {
        argv.len() == self.tokens.len()
            && self.tokens.iter().zip(argv).all(|(p, a)| p.as_str() == a.as_ref())
    }
}
```

Diff vs. before: signature gained `<S: AsRef<str>>` and `argv` is `&[S]`;
the closure changed `p == a` (`&String == &String`) to
`p.as_str() == a.as_ref()` (`&str == &str`).

- [ ] **Step 5: Run the test to confirm it passes**

```bash
cargo test --features test-helpers --lib plugin::pattern 2>&1 | tail -15
```

Expected: all `plugin::pattern::tests::*` pass, including the new
`matches_accepts_str_slice_argv`. Existing tests still pass because
`String: AsRef<str>` and the call sites typecheck via type inference
(no source change at the existing callers).

- [ ] **Step 6: Run the full lib test to catch any unexpected callers**

```bash
cargo test --features test-helpers --lib 2>&1 | tail -10
```

Expected: clean. If a non-test caller of `matches` exists outside of
`commands.rs` and breaks (unlikely — `matches` is only invoked from
`host_commands_exec`), surface the failure now.

- [ ] **Step 7: Commit**

```bash
git add src/plugin/pattern.rs
git commit -m "$(cat <<'EOF'
refactor(plugin): generalize CommandPattern::matches to AsRef<str>

§4.1 follow-up prep: matcher must accept &[&str] (the borrow-path
argv shape used by host_commands_exec after the canonical-ABI rollout)
in addition to the existing &[String] callsites. Generic over
S: AsRef<str>; existing String callsites stay monomorphic / zero-cost.

Adds one test pinning &[&str] acceptance.

Spec: docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Update `host_commands_exec` and `spawn_with_timeout` signatures

**Files:**
- Modify: `src/plugin/host/commands.rs` (host fn at line 9, deny pair at line 38, spawn helper, and 9 unit tests at lines 142–263)

**Rationale:** Land the host-side signature change before touching the
linker. Tests get updated in lockstep so the build stays green between
this task and Task 5.

- [ ] **Step 1: Read the current state of `commands.rs`**

```bash
sed -n '1,50p' src/plugin/host/commands.rs
sed -n '46,56p' src/plugin/host/commands.rs
```

Expected: signatures match the spec §3 description; `spawn_with_timeout`
takes `args: &[String]` (line 48 area).

- [ ] **Step 2: Update host fn signatures and bodies (lines 9–44)**

Replace lines 9–44 (the two top-level fns up to but not including
`fn spawn_with_timeout`) with:

```rust
pub fn host_commands_exec(
    ctx: &HostContext,
    program: &str,
    args: &[std::borrow::Cow<'_, str>],
) -> Result<ExecOutput, ErrorCode> {
    // The metadata-contract guard runs first. CWD and environment
    // inheritance happen implicitly via std::process::Command::new
    // defaults (spec §5: "CWD is the shell's current directory;
    // environment is the shell's full environment") — `ctx` is read
    // here only for `allowed_commands`, not for ShellEnv state.
    ctx.ensure_bound()?;
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    // argv = [program, args...]; pattern matcher consumes &str slices
    // (no PATH resolution, no basename normalization — see spec §5).
    // One Vec<&str> allocation, reused for both the matcher and spawn.
    let argv: Vec<&str> = std::iter::once(program)
        .chain(args.iter().map(|c| c.as_ref()))
        .collect();

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    spawn_with_timeout(program, &argv[1..], std::time::Duration::from_millis(1000))
}

pub fn deny_commands_exec() -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Diff vs before:
- `&mut HostContext` → `&HostContext` on `host_commands_exec`.
- `program: String` → `program: &str`.
- `args: Vec<String>` → `args: &[Cow<'_, str>]`.
- Internal `argv: Vec<String>` (via `.clone()` + `.cloned()`) → `argv: Vec<&str>` (single small alloc).
- `spawn_with_timeout(&program, &args, …)` → `spawn_with_timeout(program, &argv[1..], …)`.
- `deny_commands_exec` parameters dropped entirely (deny closure ignores them — see Task 5).

- [ ] **Step 3: Update `spawn_with_timeout` signature**

In the same file, locate `fn spawn_with_timeout` (around line 46). Change
the signature line:

```rust
// Before
fn spawn_with_timeout(
    program: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {

// After
fn spawn_with_timeout(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
```

The body is unchanged: `Command::new(program).args(args).…` works because
`&str: AsRef<OsStr>`. The `.spawn()` and pipe-handling code below line 67
operates on the spawned `Child`, not on `args`, so no further edits.

- [ ] **Step 4: Update the 9 unit tests at lines 142–263**

The existing tests pass `&mut ctx, "X".into(), vec!["Y".into()]`. Update
each call site to `&ctx, "X", &[Cow::Borrowed("Y")]` (or
`&[Cow::from("Y")]` — equivalent for `&str`). The deny variant tests
that previously called `deny_commands_exec(&mut ctx, …)` collapse to
`deny_commands_exec()`.

Add the import at the top of the `mod tests` block (after `use super::*;`):

```rust
use std::borrow::Cow;
```

Test-by-test edits (line numbers as of HEAD `d85fdf4`):

**Line 145** (`commands_exec_denied_when_env_null`):

```rust
// Before
let mut ctx = null_env_ctx();
let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hi".into()]);

// After
let ctx = null_env_ctx();
let result = host_commands_exec(&ctx, "/bin/echo", &[Cow::Borrowed("hi")]);
```

**Line 153** (`host_commands_exec_invalid_argument_on_empty_program`):

```rust
// Before
let mut ctx = bound_env_ctx(&mut env);
let result = host_commands_exec(&mut ctx, String::new(), vec![]);

// After
let ctx = bound_env_ctx(&mut env);
let result = host_commands_exec(&ctx, "", &[]);
```

**Line 161** (`host_commands_exec_pattern_not_allowed_when_no_match`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["ls:*"]);
let result = host_commands_exec(&mut ctx, "echo".into(), vec!["hi".into()]);

// After
let ctx = ctx_with_allowed(&mut env, &["ls:*"]);
let result = host_commands_exec(&ctx, "echo", &[Cow::Borrowed("hi")]);
```

**Line 169** (`host_commands_exec_runs_when_pattern_matches`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["/bin/echo:*"]);
let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hello".into()])
    .expect("echo should succeed");

// After
let ctx = ctx_with_allowed(&mut env, &["/bin/echo:*"]);
let result = host_commands_exec(&ctx, "/bin/echo", &[Cow::Borrowed("hello")])
    .expect("echo should succeed");
```

**Line 180** (`host_commands_exec_captures_stderr_separately`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
let result = host_commands_exec(
    &mut ctx,
    "/bin/sh".into(),
    vec!["-c".into(), "echo out; echo err 1>&2".into()],
)
.expect("sh should succeed");

// After
let ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
let result = host_commands_exec(
    &ctx,
    "/bin/sh",
    &[Cow::Borrowed("-c"), Cow::Borrowed("echo out; echo err 1>&2")],
)
.expect("sh should succeed");
```

**Line 208** (`host_commands_exec_propagates_nonzero_exit`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
let result = host_commands_exec(
    &mut ctx,
    "/bin/sh".into(),
    vec!["-c".into(), "exit 42".into()],
)
.expect("sh should run to exit");

// After
let ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
let result = host_commands_exec(
    &ctx,
    "/bin/sh",
    &[Cow::Borrowed("-c"), Cow::Borrowed("exit 42")],
)
.expect("sh should run to exit");
```

**Line 221** (`host_commands_exec_returns_not_found_for_missing_binary`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["/no/such/binary-xyz:*"]);
let result = host_commands_exec(&mut ctx, "/no/such/binary-xyz".into(), vec![]);

// After
let ctx = ctx_with_allowed(&mut env, &["/no/such/binary-xyz:*"]);
let result = host_commands_exec(&ctx, "/no/such/binary-xyz", &[]);
```

**Line 230** (`host_commands_exec_timeout_after_1000ms`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
let start = std::time::Instant::now();
let result = host_commands_exec(&mut ctx, "/bin/sleep".into(), vec!["5".into()]);

// After
let ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
let start = std::time::Instant::now();
let result = host_commands_exec(&ctx, "/bin/sleep", &[Cow::Borrowed("5")]);
```

**Line 257** (`host_commands_exec_kills_child_on_timeout`):

```rust
// Before
let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
let start = std::time::Instant::now();
let result = host_commands_exec(&mut ctx, "/bin/sleep".into(), vec!["5".into()]);

// After
let ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
let start = std::time::Instant::now();
let result = host_commands_exec(&ctx, "/bin/sleep", &[Cow::Borrowed("5")]);
```

Note: `bound_env_ctx`/`ctx_with_allowed` already return `HostContext`
by value (they take `&mut ShellEnv`). The local binding changes from
`let mut ctx = …` to `let ctx = …` because `host_commands_exec` no longer
needs `&mut HostContext`.

- [ ] **Step 5: Build (linker.rs is still on the old shape; expect a localized failure)**

```bash
cargo build --features test-helpers 2>&1 | tail -20
```

Expected: build FAILS in `src/plugin/linker.rs` (the closures still pass
`(String, Vec<String>)` to the now-changed `host_commands_exec`). This
is a deliberate intermediate state — Task 5 is the matching half. The
failure should be exactly two errors (one for granted, one for deny),
both inside the `commands::exec` block (lines ~263–280). If errors
appear elsewhere, the host fn or test edits are wrong; pause and inspect
before continuing.

If the failure is in `src/plugin/host/commands.rs` (e.g., the body itself
doesn't compile), fix it before moving on. The body should be fully
clean by the end of this step; only the `linker.rs` callers should be
red.

- [ ] **Step 6: DO NOT commit yet**

The intermediate state has a broken build. Holding off on the commit
keeps `git bisect` clean. Task 5 will commit a single
linker+integration commit that resolves the failure.

---

## Task 5: Update `linker.rs` closures to `(WasmStr, WasmList<WasmStr>)`

**Files:**
- Modify: `src/plugin/linker.rs` lines 263–280

**Rationale:** Wires the new host fn signature to the canonical-ABI
borrow types. Closure body lifts `program` to `Cow<'_, str>` and `args`
to `Vec<Cow<'_, str>>`, then calls the new host fn.

- [ ] **Step 1: Read the current closures**

```bash
sed -n '262,282p' src/plugin/linker.rs
```

Expected: granted closure passes `(String, Vec<String>)` directly;
deny closure does the same. Both call `store.data_mut()`.

- [ ] **Step 2: Replace the entire `commands::exec` block (lines ~263–280)**

```rust
    // ── yosh:plugin/commands ───────────────────────────────────────────
    let mut commands = linker.instance("yosh:plugin/commands@0.2.1")?;
    if has(allowed, CAP_COMMANDS_EXEC) {
        commands.func_wrap(
            "exec",
            |store,
             (program, args): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<wasmtime::component::WasmStr>,
            )| {
                let program_str = program.to_str(&store)?;
                let args_strs: Vec<std::borrow::Cow<'_, str>> = args
                    .iter(&store)
                    .map(|res| res.and_then(|w| w.to_str(&store).map_err(Into::into)))
                    .collect::<wasmtime::Result<_>>()?;
                Ok((host_commands_exec(store.data(), &program_str, &args_strs),))
            },
        )?;
    } else {
        commands.func_wrap(
            "exec",
            |_store,
             (_program, _args): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<wasmtime::component::WasmStr>,
            )| { Ok((deny_commands_exec(),)) },
        )?;
    }
```

Notes:
- The granted closure: `mut store` → `store` (immutable; `store.data()`
  is the new accessor). `program.to_str(&store)?` returns
  `Cow<'_, str>` borrowing from linear memory. The `args.iter(&store)`
  yields `Result<WasmStr>`; we lift each to a `Cow<'_, str>` via
  `to_str(&store)` and collect into `Vec<Cow<'_, str>>`.
- The deny closure: `_store, (_program, _args)` are all underscored —
  the lift happens (canonical-ABI ptr/len validation only, no
  per-element work) but neither field is dereferenced. This is the
  *zero-allocation* deny path that the dhat smoke measures.
- `Into::into` on the `to_str` `Result` works because `wasmtime::Result`
  is `Result<_, anyhow::Error>` and the per-element error type from
  `to_str` (or from `iter`'s `Result<WasmStr>`) coerces into
  `anyhow::Error`. If the compiler complains about the `?` at
  `collect::<wasmtime::Result<_>>()`, fall back to:

```rust
let args_strs: Vec<std::borrow::Cow<'_, str>> = args
    .iter(&store)
    .map(|res| {
        let w = res?;
        w.to_str(&store).map_err(anyhow::Error::from)
    })
    .collect::<wasmtime::Result<_>>()?;
```

If `wasmtime::Result` is not in scope, add
`use wasmtime::Result as WasmtimeResult;` at the top of the file (or
inline it as `Result<_, anyhow::Error>`).

- [ ] **Step 3: Build the workspace**

```bash
cargo build --features test-helpers 2>&1 | tail -10
```

Expected: clean build. If errors remain in `linker.rs`, the most
common causes are:
- Missing `wasmtime::component::WasmList` / `WasmStr` imports — they're
  fully qualified above so this should work, but if the file already
  imports `WasmStr`, the existing import shadows the qualifier (benign).
- The `args.iter(&store)` closure capturing `&store` while a later
  `store.data()` call wants `&store` again — both are immutable, so the
  borrow checker allows it. If there's a conflict, restructure the
  collect to bind a local `let store_ref = &store;` first.

- [ ] **Step 4: Run the host commands tests**

```bash
cargo test --features test-helpers --lib plugin::host::commands 2>&1 | tail -20
```

Expected: all 9 tests pass (the same 9 from before, with updated call
shapes). If `host_commands_exec_runs_when_pattern_matches` or the sh
tests fail with `Err(PatternNotAllowed)`, the matcher generalization
(Task 3) regressed; rerun
`cargo test --features test-helpers --lib plugin::pattern` to confirm
that side is still green.

- [ ] **Step 5: Run the full plugin integration test**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -20
```

Expected: pass (~30 tests in this binary). The integration tests
exercise `commands::exec` end-to-end via the wasm component layer —
this is the strongest signal that the WIT-bindings `exec` import
still wires correctly.

- [ ] **Step 6: Re-measure dhat for `noop_commands_exec`**

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_commands_exec 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-argv-rollout-noop_commands_exec-after.json
```

Read the `dhat: Total: <bytes> bytes in <blocks> blocks` line; the
`<blocks>` integer is `A_ce`. **Acceptance gate: `B_ce − A_ce ≥ 4000`**
(i.e., at least 4,000 fewer blocks than the baseline recorded in
`target/perf/argv-rollout-baseline.txt`).

If the delta is less than 3,600 (more than 10% miss), pause and
investigate. Common causes:
- One of the closures still passes `(String, Vec<String>)` (e.g., the
  deny branch wasn't updated) — re-read both closures and verify the
  parameter types are `(WasmStr, WasmList<WasmStr>)`.
- The collect path on the granted closure runs even on the deny path
  because the `if has(allowed, CAP_COMMANDS_EXEC)` branch picks the
  wrong arm — verify `perf_plugin` does NOT declare `CommandsExec`
  capability and the plugins.lock does NOT grant `commands:exec`.
- The wasm fixture wasn't rebuilt — rerun Task 1 Step 4.

If the delta is much larger than 4,000 (e.g., 100,000+), it likely
indicates a secondary alloc that was riding on the canonical-ABI
lift (similar to the `noop_io_write` anomaly in the §4.1 follow-up,
where `Vec<u8>` lift triggered ~127 secondary allocations per crossing).
Record it as a benign over-shoot, no investigation needed.

- [ ] **Step 7: Commit (Task 4 + Task 5 together)**

The host fn change (Task 4) and the closure change (Task 5) are paired
— neither makes sense on its own and the build is broken between
them. Commit them together:

```bash
git add src/plugin/host/commands.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
perf(plugin): borrow argv in commands::exec via WasmList<WasmStr>

§4.1 follow-up final piece: replace (String, Vec<String>) with
(WasmStr, WasmList<WasmStr>) in commands::exec. host_commands_exec
drops to &HostContext, &str, &[Cow<'_, str>]; spawn_with_timeout
takes &[&str]. CommandPattern::matches was generalized to
S: AsRef<str> in the previous commit.

Closes the §4.1 follow-up surface — io.write, files.write-file,
files.append-file (Vec<u8>) plus commands::exec (Vec<String>) are
now all canonical-ABI borrows.

dhat --exec-loop 1000 noop_commands_exec: <B_ce> -> <A_ce>
blocks (Δ=<B_ce − A_ce>; target ≥ 4,000)

Spec: docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Substitute the actual numbers for `<B_ce>`, `<A_ce>`, and `<B_ce − A_ce>`
from the recorded values.

---

## Task 6: Final regression sweep, perf-report Appendix, TODO.md cleanup

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (append a new Appendix at end)
- Modify: `TODO.md` (delete the `commands::exec` argv borrow follow-up bullet)

- [ ] **Step 1: Run the full test suite**

```bash
cargo test --features test-helpers 2>&1 | tail -20
```

Expected: count matches the §4.1 follow-up baseline of **2,177 / 2,177 pass**
(or higher if anything was added since), PLUS the one new test added in
Task 3 (`matches_accepts_str_slice_argv`), so the new total should be
**2,178** at minimum. If lower, identify the regression before
declaring success.

Use `timeout: 300000` (5 minutes) — full suite typically runs ~30–90s
on this machine.

- [ ] **Step 2: Run the Criterion noise sentinel**

```bash
cargo bench --bench plugin_exec_bench -- burst_var 2>&1 | tail -20
```

Expected: median within ±5% of the §4.1 baseline of ~1,205 ns. The
`burst_var` bench exercises `variables::get`, which this rollout does
not touch; any movement is codegen noise. If the bench harness is
named differently (`grep -l 'burst_var\|plugin_exec' benches/`),
substitute the actual name. If the bench is missing, skip and note
in the perf-report append.

- [ ] **Step 3: Append the rollout result to the perf report**

Append at the very end of
`docs/superpowers/specs/2026-05-08-plugin-perf-report.md`:

```markdown
## Appendix E: §4.1 Follow-up commands::exec Argv Borrow Rollout — Result

**Date:** 2026-05-09
**Spec:** `docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md`
**Plan:** `docs/superpowers/plans/2026-05-09-plugin-commands-exec-argv-borrow.md`
**Commits:** see `git log` between this entry and Appendix C's HEAD

### Coverage

One host import converted from `(String, Vec<String>)` to
`(wasmtime::component::WasmStr, wasmtime::component::WasmList<WasmStr>)`:

- `commands::exec` (closure + `host_commands_exec` signature dropped to `&HostContext, &str, &[Cow<'_, str>]`)

The deny counterpart was simplified to take no parameters, since the
deny closure does not dereference the lifted args. `CommandPattern::matches`
was generalized to `S: AsRef<str>` to accept both `&[String]` (existing
callsites) and `&[&str]` (the new borrow path); `spawn_with_timeout` was
narrowed to `args: &[&str]`.

Closes the §4.1 follow-up surface: `io.write`, `files.write-file`,
`files.append-file` (`Vec<u8>` → `WasmList<u8>::as_le_slice`) plus
`commands::exec` (`Vec<String>` → `WasmList<WasmStr>` + per-element
`to_str`). All canonical-ABI lift-side allocations in the host import
surface are now zero-copy.

### Decisive cross-check (dhat `--exec-loop 1000`)

| Smoke | Baseline blocks | After blocks | Δ | Target | Verdict |
|---|---|---|---|---|---|
| `noop_commands_exec` | <B_ce> | <A_ce> | **<B_ce − A_ce>** | −4,000 | <verdict> |

`<verdict>` is ✅ exact (Δ ≥ −4,000), ✅ over (Δ much larger), or ❌ if
the gate was missed. Substitute the actual numbers from Tasks 2–5.

The −4,000 prediction comes from per-crossing baseline allocs of:
1 `String` (program) + 1 outer `Vec<String>` (args) + 2 inner `String`
(args elements) = 4 blocks/crossing × 1000 crossings.

### Regression check

- `cargo test --features test-helpers`: **<X> / <Y> pass** (no count change vs HEAD before this rollout, plus +1 new pattern test).
- `plugin_exec_burst_var` Criterion median: <N> ns (baseline ~1,205 ns from §4.1; within ±5%). [Or "skipped — bench harness not present at this commit" if applicable.]

### §4.1 follow-up surface — closed

With this rollout, all three canonical-ABI lift codepaths in the host
import surface are converted:

| Codepath | Borrow type | Pattern |
|---|---|---|
| `string` | `WasmStr` | `to_str(&store)` |
| `list<u8>` | `WasmList<u8>` | `as_le_slice(&store)` |
| `list<string>` | `WasmList<WasmStr>` | `iter(&store).map(\|w\| w.to_str(&store))` |

Future canonical-ABI parameter types not yet exercised (`record`s with
string fields, nested `list`s) will need their own spike when added.
```

Substitute the placeholders (`<B_ce>`, `<A_ce>`, `<verdict>`, `<X>`, `<Y>`,
`<N>`) with the actual values you recorded.

If Appendix C has been renamed to D since this plan was written (the
file currently has two "Appendix C" blocks — check `grep '^## Appendix' docs/superpowers/specs/2026-05-08-plugin-perf-report.md`),
choose the next-available letter (E, F, …) and update the heading
accordingly. The body content does not depend on the letter.

- [ ] **Step 4: Remove the `commands::exec` argv entry from TODO.md**

In `TODO.md`, locate the bullet (currently the second one under
"Future: Plugin System Enhancements" mentioning Plugin perf):

```
- [ ] Plugin perf: borrow `commands::exec` argv (`Vec<String>` → `list<string>`). Separate `list<string>` lift codepath; needs its own spike. Each argv element is a `String` allocation per crossing, so savings could be substantial for command-heavy workloads. See report Appendix B follow-up.
```

Delete the entire bullet (it is one wrapped paragraph). Per `CLAUDE.md`,
completed items are deleted, not crossed off.

The `linker_cache concurrency story` and `Appendix D delta note` items
stay — they remain out-of-scope.

- [ ] **Step 5: Verify the docs and TODO edits look right**

```bash
git diff docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
```

Expected: appendix addition + one bullet deletion in TODO.md.

- [ ] **Step 6: Final commit**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.1 follow-up commands::exec argv result

commands::exec converted to (WasmStr, WasmList<WasmStr>) borrow.
dhat verdict:
- noop_commands_exec: Δ=<B_ce − A_ce> blocks (target −4,000)

Test suite: <X> / <Y> pass (no regression vs HEAD before rollout,
plus +1 new pattern test).

Closes the §4.1 follow-up surface — string, list<u8>, list<string>
are all canonical-ABI borrows now. Removes the corresponding TODO.md
follow-up entry.

Spec: docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md
Plan: docs/superpowers/plans/2026-05-09-plugin-commands-exec-argv-borrow.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Verify clean state**

```bash
git status
git log --oneline -8
```

Expected: working tree clean; the last 5–6 commits are the spec, the
perf_plugin command, the matcher generalization, the host+linker
rollout commit, and the docs/TODO commit, in order.

---

## Done

Success criteria from the spec:

1. ✅ `noop_commands_exec` meets `≤ −4,000 blocks vs baseline` per `--exec-loop 1000`.
2. ✅ `cargo test --features test-helpers` passes with no count regression (plus +1 new pattern test).
3. ✅ `plugin_exec_*` Criterion benches within ±5% of HEAD baseline.
4. ✅ No new `unsafe`; no `to_vec()` / `to_owned()` / `into_owned()` introduced in converted code paths.
5. ✅ `cargo fmt --all -- --check` clean; no new clippy warnings.

If any criterion failed, `git revert` the implementation commits and
investigate before declaring done.
