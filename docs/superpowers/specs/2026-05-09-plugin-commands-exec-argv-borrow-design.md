# Plugin perf §4.1 follow-up: Borrow `commands::exec` argv — Design

**Status:** Draft
**Date:** 2026-05-09
**Author:** k-ymmt (with Claude)
**Related:**
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix B follow-up
- `docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md` (`Vec<u8>` rollout, completed `b93fd66`)
- `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md` (§4.1 `String` rollout, completed `2262c7c`)

## 1. Background

§4.1 (`2026-05-08`) borrowed all `String` host-import arguments via
`wasmtime::component::WasmStr`, eliminating one host-side `String` allocation
per crossing per parameter. The `Vec<u8>` follow-up (`2026-05-09`) extended the
same pattern to `list<u8>` parameters via `WasmList<u8>::as_le_slice`. Both
spikes confirmed the per-arg unit savings of **−1,000 blocks per
`--exec-loop 1000`** matches the algorithmic prediction.

The §4.1 report's Appendix B "Follow-up" identifies one remaining codepath
that shares the same allocation problem but uses yet another canonical-ABI
lift: `Vec<String>` (`list<string>`). The only host import using this shape
is `yosh:plugin/commands.exec`, which receives the argv as `Vec<String>` and
incurs `1 outer Vec + N inner String` host-side allocations per crossing.

This spec covers `commands::exec` argv borrow only. Together with the §4.1
`String` rollout and the §4.1 follow-up `Vec<u8>` rollout, this closes the
last remaining canonical-ABI lift allocation in the host import surface.

## 2. Goal & Scope

### Goal

Replace `(program: String, args: Vec<String>)` parameters in
`yosh:plugin/commands.exec` (and its deny pair) with `(WasmStr, WasmList<WasmStr>)`,
read each element via `to_str(&store)` to obtain `Cow<'_, str>` slices, and
collect into a single `Vec<Cow<'_, str>>` of length `N` (one small alloc) so
that the per-element `String` allocations are eliminated.

### Scope (in)

| Component | Change |
|---|---|
| `src/plugin/linker.rs` `commands::exec` granted closure | `(String, Vec<String>)` → `(WasmStr, WasmList<WasmStr>)` |
| `src/plugin/linker.rs` `commands::exec` deny closure | same shape; lazy (no `to_str` calls) |
| `src/plugin/host/commands.rs::host_commands_exec` | `(&mut HostContext, String, Vec<String>)` → `(&HostContext, &str, &[Cow<'_, str>])` |
| `src/plugin/host/commands.rs::deny_commands_exec` | same shape; body unchanged |
| `CommandPattern::matches` | `&[String]` → `&[impl AsRef<str>]` |
| `spawn_with_timeout` | `args: &[String]` → `args: &[Cow<'_, str>]` |
| `src/plugin/host/commands.rs::tests` (9 tests) | signature follow-through; logic unchanged |
| `src/bin/yosh-dhat.rs` | add `noop_commands_exec_borrow` smoke |
| `tests/plugins/perf_plugin/` | add `noop_commands_exec` export if not present |

### Scope (out)

- WIT itself (`exec: func(program: string, args: list<string>) -> result<exec-output, error-code>`) — no change.
- `ExecOutput` return path (`stdout: list<u8>`, `stderr: list<u8>`) — host *produces* the bytes, alloc is unavoidable on the lower side.
- Other TODO.md items (runtime plugin load, linker_cache concurrency story, `yosh-plugin-sdk::exec_to_string` helper, etc.).

## 3. Architecture

The pattern is the §4.1 / §4.1 follow-up borrow pattern generalized to
`T = WasmStr`. Wasmtime 27 facts that make this work:

- `unsafe impl<T: Lift> Lift for WasmList<T>` (typed.rs:1689)
- `unsafe impl Lift for WasmStr` (typed.rs:1381)
- `WasmList<T>::iter(&store) -> impl ExactSizeIterator<Item = Result<T>>`

Therefore `WasmList<WasmStr>` is a valid host parameter type, and each
yielded `WasmStr` can be turned into a `Cow<'_, str>` via `to_str(&store)`
without allocating in the happy path (UTF-8 valid linear memory).

### 3.1 `linker.rs` closures

```rust
// Granted
commands.func_wrap(
    "exec",
    |store, (program, args): (WasmStr, WasmList<WasmStr>)| {
        let program_str = program.to_str(&store)?;
        let args_strs: Vec<Cow<'_, str>> = args
            .iter(&store)
            .map(|res| res.and_then(|w| w.to_str(&store).map_err(Into::into)))
            .collect::<Result<_, _>>()?;
        Ok((host_commands_exec(store.data(), &program_str, &args_strs),))
    },
);

// Deny — never inspects args (lazy)
commands.func_wrap(
    "exec",
    |_store, (_program, _args): (WasmStr, WasmList<WasmStr>)| {
        Ok((deny_commands_exec(),))
    },
);
```

`store.data_mut()` drops to `store.data()` because `host_commands_exec`'s
body only calls `ctx.ensure_bound()` (`&self`) and `ctx.allowed_commands.iter()`
(`&self`); no `ShellEnv` mutation. Same downgrade as §4.1 follow-up's
`host_io_write` change.

### 3.2 Host fn signatures

```rust
// src/plugin/host/commands.rs
pub fn host_commands_exec(
    ctx: &HostContext,
    program: &str,
    args: &[Cow<'_, str>],
) -> Result<ExecOutput, ErrorCode>;

pub fn deny_commands_exec() -> Result<ExecOutput, ErrorCode>;
```

`deny_commands_exec` loses its parameters entirely — the deny closure
doesn't `to_str` the inputs, so passing them through would force the lift.

### 3.3 `CommandPattern::matches` generalization

```rust
// Before
pub fn matches(&self, argv: &[String]) -> bool;

// After
pub fn matches<S: AsRef<str>>(&self, argv: &[S]) -> bool;
```

Internal `==` comparisons against `String` change to `s.as_ref() == ...`.
Monomorphization keeps existing `String` callsites zero-cost. The matcher
lives at `src/plugin/pattern.rs:51` (`CommandPattern::matches`); it is
workspace-internal and not exported via the plugin SDK.

### 3.4 `spawn_with_timeout` signature

```rust
fn spawn_with_timeout(
    program: &str,
    args: &[Cow<'_, str>],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode>;
```

`Command::args(args)` works unchanged because `Cow<str>: AsRef<OsStr>`.

### 3.5 Internal argv construction

```rust
// Before (1 + N String clones into a Vec<String>)
let mut argv = Vec::with_capacity(1 + args.len());
argv.push(program.clone());
argv.extend(args.iter().cloned());
if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) { ... }

// After (zero-copy chain into the generalized matcher)
let argv_iter = std::iter::once(program).chain(args.iter().map(|c| c.as_ref()));
let argv_vec: Vec<&str> = argv_iter.collect();  // 1 small Vec<&str>, no String alloc
if !ctx.allowed_commands.iter().any(|p| p.matches(&argv_vec)) { ... }
```

The `Vec<&str>` collection is needed because `matches` takes `&[S]`, not an
iterator. This is one small allocation — `(1 + N) * sizeof(&str)` bytes — and
is dwarfed by what we eliminated. (A future enhancement could change `matches`
to take an iterator, but that is out of scope here.)

## 4. Data flow

```
guest plugin
  └─ commands::exec(program, args)            [WIT: string, list<string>]
       │
       ▼ canonical-ABI lift  (zero host alloc — wasmtime 27)
linker closure
  ├─ program: WasmStr            (ptr+len only)
  ├─ args:    WasmList<WasmStr>  (ptr+len only)
  │
  ├─ program.to_str(&store)  ──→ Cow::Borrowed(&str)  [linear memory ref]
  ├─ args.iter(&store)
  │    └─ for each: WasmStr → to_str(&store) → Cow<'_, str>
  │       collect into Vec<Cow<'_, str>>          [1 small host alloc]
  │
  └─ host_commands_exec(store.data(), &program_str, &args_strs)
        ├─ ctx.ensure_bound()
        ├─ Vec<&str> argv ← chain(once(program), args.iter().map(AsRef::as_ref))
        ├─ CommandPattern::matches(&argv)
        └─ spawn_with_timeout(program, args_strs, 1000ms)
              └─ Command::new(program).args(args_strs).spawn() → ExecOutput
```

Deny path: `deny_commands_exec()` returns `Err(ErrorCode::Denied)` immediately;
no `to_str` / `iter` calls, no inner alloc.

## 5. Error handling

| Failure point | ErrorCode | Origin |
|---|---|---|
| `program.to_str()` UTF-8 decode failure | `InvalidArgument` | wasmtime lift error → mapped via `?` (matches §4.1) |
| `args.iter()` per-element lift failure | `InvalidArgument` | same |
| `program.is_empty()` | `InvalidArgument` | preserved from current code |
| pattern not allowed | `PatternNotAllowed` | preserved |
| `Command::spawn` `ErrorKind::NotFound` | `NotFound` | preserved |
| other IO error | `IoFailed` | preserved |
| timeout (1000ms) → SIGTERM/grace/SIGKILL | `IoFailed` | preserved |

### 5.1 Borrow lifetime soundness

- `program.to_str(&store)` and `args.iter(&store)` both take immutable
  borrows of `store`. Their results (`Cow<'_, str>`, `Result<WasmStr>`)
  carry lifetimes tied to `store`'s linear memory.
- `store.data()` is an additional immutable borrow → no conflict.
- `host_commands_exec` runs entirely within the closure body, so the
  collected `Vec<Cow<'_, str>>` and the linear memory backing it are valid
  for the call's duration.
- Wasmtime guarantees the host call holds the wasm instance's lock; the
  guest cannot mutate linear memory between `to_str` and the host fn
  returning (same guarantee §4.1 / §4.1 follow-up rely on).
- Closure return drops all borrows.

## 6. Verification

### 6.1 dhat acceptance gate

A new smoke `noop_commands_exec_borrow` runs the deny path of
`commands::exec("/bin/echo", &["a", "b"])` × 1000 inside the existing
`yosh-dhat` harness. Deny path is chosen so no subprocess is spawned —
this isolates the canonical-ABI lift allocation from process-creation noise.

| Smoke | Crossing measured | Gate (vs HEAD baseline before this rollout) |
|---|---|---|
| `noop_commands_exec_borrow` | `commands::exec("/bin/echo", &["a", "b"])` × 1000, deny path | **≤ −3,000 blocks** |

The −3,000 prediction comes from:
- Per crossing baseline: 1 outer `Vec<String>` + 2 inner `String` = 3 blocks
- Per crossing after: 0 (the `Vec<Cow<'_, str>>` is collected only on the granted path; deny path skips it entirely)
- 1,000 crossings × 3 blocks = 3,000

If the smoke misses by more than 10% (observed savings &lt; 2,700 blocks),
pause and investigate before merging. Likely causes: hidden `String`
construction in lift error path, accidental `to_str` call in deny path, or
linker codegen retaining a `Vec<String>` shape.

### 6.2 Functional regression check

`cargo test --features test-helpers` must pass with no count regression.
Re-record the current count at implementation start (the §4.1 follow-up
recorded 2,177 / 2,177 at `2b02437`; subsequent linker-cache commits may
have changed the count, so don't trust this number until re-measured).

`tests/plugin.rs` has end-to-end coverage for `commands::exec` (granted
and denied paths, pattern matching, timeout, stderr capture). These
exercise the WIT-level surface which is unchanged, so they should pass
without test edits.

### 6.3 Bench noise band

`plugin_exec_*` Criterion benches (3-run median-of-medians) must stay
within ±5% of HEAD baseline. The `burst_var` bench exercises
`variables::get` and is a stability sentinel — it should not move because
this rollout doesn't touch the `variables` interface.

There is no Criterion bench targeting `commands::exec` directly; the dhat
smoke is the per-crossing measurement.

### 6.4 Code-quality gates

- No new `unsafe`.
- No new `to_vec()` / `to_owned()` / `into_owned()` in the converted
  closure or host fn (matching §4.1 follow-up discipline).
- `cargo fmt --all -- --check` clean.
- `cargo clippy --all-targets -- -D warnings` no new warnings (the
  pre-existing `doc_lazy_continuation` warning on `src/plugin/mod.rs:98`
  is a known TODO, not blocking).

## 7. Risks

- **`WasmList<WasmStr>::iter` lift overhead.** Each iteration step
  validates ptr/len for one `WasmStr`. If validation is dominant the
  net savings could be smaller than predicted. Mitigation: dhat smoke
  measures end-to-end blocks, which includes any hidden allocations in
  the validation path.
- **`Cow<str>` allocation on UTF-8 decode failure.** Rare in practice
  (guests sending bad UTF-8 are buggy). Returns `InvalidArgument`,
  matching §4.1 / §4.1 follow-up behavior. Allocation only happens on
  the error path so it doesn't pollute the dhat happy-path measurement.
- **`CommandPattern::matches` generalization touching unrelated callers.**
  If `matches` is called from outside `commands.rs` with a `&[String]`,
  the generic `S: AsRef<str>` infers `S = String` and works zero-cost.
  No callsite changes needed.
- **Test plugin (`tests/plugins/perf_plugin`) regeneration churn.**
  `cargo component build` regenerates `bindings.rs` on every invocation,
  dirtying the git tree. Pre-existing concern logged in TODO.md;
  acknowledged but not addressed by this rollout.

## 8. Out-of-scope follow-ups

- A future `WasmList<T>::iter` enhancement that yields `&T` directly
  (avoiding the per-step validation re-run) would compress the savings
  further. Wasmtime 27 doesn't expose this; revisit on upgrades.
- `CommandPattern::matches` taking an `IntoIterator<Item: AsRef<str>>`
  to skip the `Vec<&str>` collection entirely. Small win, separate
  refactor.
- `host_commands_exec`'s timeout (1000ms hard-coded) → cap-mediated
  configuration. Out of scope; tracked separately if needed.

## 9. Implementation references

- `src/plugin/linker.rs` lines ~263–280 (commands instance, exec closures)
- `src/plugin/host/commands.rs` (`host_commands_exec`, `deny_commands_exec`,
  `spawn_with_timeout`, tests at lines 145–257)
- `src/bin/yosh-dhat.rs` (add `noop_commands_exec_borrow` entry, mirror
  existing `noop_files_write_file_borrow` shape)
- `tests/plugins/perf_plugin/` (add export `noop_commands_exec` if absent;
  mirror `noop_files_write_file` style)
- §4.1 follow-up commits for pattern reference: `9c0e065`, `db2c419`, `b93fd66`

## 10. Success criteria

1. `noop_commands_exec_borrow` meets `≤ −3,000 blocks vs baseline` per `--exec-loop 1000`.
2. `cargo test --features test-helpers` passes with no count regression.
3. `plugin_exec_*` Criterion benches within ±5% of HEAD baseline.
4. No new `unsafe`, no new `to_vec()` / `to_owned()` / `into_owned()` in
   converted code paths.
5. `cargo fmt --all -- --check` clean; no new clippy warnings.
