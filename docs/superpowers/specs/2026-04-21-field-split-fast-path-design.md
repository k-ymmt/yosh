# Design: `field_split::emit` Fast Path (Skip State Machine When No IFS Delimiter)

**Date:** 2026-04-21
**Performance reference:** `performance.md` §5.2 (P0, post-pathname-fastpath)
**Priority:** P0 (new top dhat site at HEAD `610043e`)
**Estimated effort:** ~1 day

## 1. Background

At HEAD (`610343e`, post-`pathname::expand` fast path), `yosh::expand::field_split::emit` is the **#1 dhat site by bytes** in workload W2 (`benches/data/script_heavy.sh`):

| Metric | Value |
|---|---|
| Bytes allocated (site rank #1) | 2.94 MB / 14,020 calls |
| Bytes allocated (site rank #2) | 1.48 MB / 7,013 calls |
| Bytes allocated (site rank #7) | 209.5 KB / 18 calls |
| **3-site aggregate** | **4.63 MB / 21,051 calls** |

All three sites are attributed to `src/expand/field_split.rs:180:9` — the single line `out.push(done);` inside:

```rust
fn emit(current: &mut ExpandedField, out: &mut Vec<ExpandedField>) {
    let done = std::mem::take(current);
    out.push(done);
}
```

`performance.md` §5.2 notes that the 14,020-call shape matches the pre-fastpath `pathname::expand:29:20` entry exactly, and calls out that dhat may be **re-attributing callee allocation through the pipeline** rather than revealing genuine growth inside `emit`. The §5.2 entry therefore starts with an investigation requirement.

### 1.1 Working hypothesis (A)

The allocations are the growth of the `out: Vec<ExpandedField>` backing store. `split()` creates a fresh `result = Vec::new()` per call. Each first `push` allocates `cap=4 × sizeof(ExpandedField) ≈ 224 bytes`. For 14,020 `split()` calls with at least one `emit` per call, that yields ~14,020 × 224 B ≈ 3.14 MB, matching the observed 2.94 MB. This hypothesis is **unconfirmed** at spec-write time and is verified in §8 before the fix is committed.

### 1.2 Alternative hypothesis (B)

The allocations are `String::push_str` / `Vec::<u64>::resize` inside `append_byte` → `push_quoted` / `push_unquoted` / `set_range`, attributed to `emit` by dhat's nearest-frame walk because those helpers are `#[inline]`. In this case, the fast path in §4 still helps — because it skips all of `split_field`, including `append_byte` — but the observed savings would come from a different code path than the hypothesis predicts. Measurement in §8 distinguishes A from B.

### 1.3 Why W2 is a good target

W2 evaluates ~1000 iterations of `for`, 1000 function calls, and many parameter expansions. Typical fields flowing through `split()` are `echo`, `hello`, `world`, `$VAR`, `"$VAR"` — none of which contain unquoted IFS bytes (default IFS is `" \t\n"`). Therefore the fast-path hit rate should be high (target: ≥90% of `split()` invocations in W2).

## 2. Goal

Eliminate the output-`Vec` allocation and associated `emit` calls for invocations of `field_split::split` where **no** input field contains an unquoted IFS byte, preserving all POSIX XBD 8.3 Field Splitting semantics.

**Non-goals:**
- Partial fast path: mixed inputs where only some fields need splitting still go through the full slow path. The effect-per-complexity ratio does not justify the added state (verified in §3).
- Optimizing `split_field` / `append_byte` / `emit` internals (separate work, possibly unnecessary after this lands).
- `Vec::with_capacity` hints on the slow-path `result`. Slow-path totals at HEAD are small (~30 KB across the non-fast-path entries); hinting has low ROI and risks over-allocation.
- `pattern::matches` caching (§4.4, separate P1).
- `exec_function_call` sub-bench / `catch_unwind` work (§4.2, separate P1).

## 3. Approach

Add an early-return fast path to `field_split::split`. After IFS partitioning but **before** the per-field state-machine loop, check whether every input field is splittable-free; if so, return the input `Vec` unchanged.

### 3.1 Why this approach (over alternatives)

Three alternatives were considered (see brainstorming Section 1):

1. **`split()`-level fast path (chosen)** — single full scan over all fields' unquoted bytes. Returns input `Vec` unchanged when no field needs splitting. Zero allocation on fast path.
2. **Field-level fast path inside `split_field`** — per-field decision to bypass state machine and push input `ExpandedField` directly. Rejected as primary: `out.push(done)` still runs once per field, keeping the `out` `Vec` growth allocations that hypothesis A points to. Only helps hypothesis B, and even then less than approach 1.
3. **Combined 1 + 2** — maximum savings for mixed inputs but adds duplicated branching, two code paths to maintain, and measurement-backed benefit over approach 1 alone is not established. Rejected as YAGNI.

The fast-path scan cost on mixed inputs is one extra `iter().all(...)` pass over unquoted bytes before entering the slow path. For the W2 fields that do need splitting, this is <10 bytes/field — negligible compared to the state machine + heap churn that follows.

## 4. Change

**File:** `src/expand/field_split.rs` (only)

### 4.1 New helper

Added after `split_field` (module-private):

```rust
/// Return true iff `field` contains at least one unquoted byte that is
/// an IFS delimiter (whitespace or non-whitespace).
///
/// Used by `split()` as a pre-check: if every input field returns false,
/// the slow-path state machine would emit each input field unchanged,
/// so we can return the input Vec as-is without any allocation.
fn needs_splitting(field: &ExpandedField, ifs_ws: &[u8], ifs_nws: &[u8]) -> bool {
    field.value.bytes().enumerate().any(|(i, b)| {
        !field.is_quoted(i) && (ifs_ws.contains(&b) || ifs_nws.contains(&b))
    })
}
```

### 4.2 `split()` diff

```rust
 pub fn split(env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
     let ifs = get_ifs(env);

     if ifs.is_empty() {
         return fields
             .into_iter()
             .filter(|f| !f.value.is_empty() || f.was_quoted)
             .collect();
     }

     let ifs_ws: Vec<u8> = ifs.bytes()
         .filter(|b| matches!(*b, b' ' | b'\t' | b'\n'))
         .collect();
     let ifs_nws: Vec<u8> = ifs.bytes()
         .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
         .collect();

+    // Fast path: if no field contains an unquoted IFS byte, the state
+    // machine would emit each input field unchanged. Return without
+    // allocating a new Vec or rebuilding ExpandedFields via `emit`.
+    if fields.iter().all(|f| !needs_splitting(f, &ifs_ws, &ifs_nws)) {
+        return fields;
+    }
+
     let mut result = Vec::new();
     for field in fields {
         split_field(&field, &ifs_ws, &ifs_nws, &mut result);
     }
     result
 }
```

Net change: +1 function (~8 lines) + 5 lines in `split()` (guard block). No signature change. No new module dependencies. `is_quoted` is an existing public method on `ExpandedField`.

## 5. Semantic Preservation

The fast path is activated **only** when `fields.iter().all(|f| !needs_splitting(f, ...))` — that is, no unquoted byte in any field matches an IFS delimiter (whitespace or non-whitespace). The slow path, when run on such inputs, would:

1. Enter `split_field` for each input field.
2. `len == 0 && was_quoted` → push a quoted empty `ExpandedField`; return.
3. `len == 0 && !was_quoted` → state stays `Start` through empty iteration; `current.is_empty()` → not flushed; field **dropped**.
4. `len > 0` → state machine runs `Start → InField` on every byte (no whitespace / no non-whitespace delimiter hit), accumulating via `append_byte`; flush at end → one output field whose `value` and `quoted_mask` equal the input's.

The fast path short-circuits all of the above by returning the input `Vec` intact. This differs from the slow path in one edge case:

| Case | Slow path | Fast path |
|---|---|---|
| `[unquoted("")]` (empty, unquoted, no delimiter) | **Drops** field (out is empty) | **Preserves** field (out is input) |
| All other no-delimiter cases | Identical output | Identical output |

### 5.1 Why the empty-unquoted divergence is safe

`expand_word` (`src/expand/mod.rs:114-118`, filter at line 116) applies a final filter `!f.is_empty() || f.was_quoted` before returning to callers. That filter drops the empty unquoted field regardless of whether field splitting dropped it upstream. Every public call site of `field_split::split` that was surveyed (§ 7.2) routes through `expand_word` / `expand_words`, so the external contract is unchanged.

### 5.2 POSIX XBD 8.3 invariants — explicit checks

| Invariant | Fast-path behavior | Slow-path behavior | Equivalent? |
|---|---|---|---|
| IFS unset → default `" \t\n"` | `get_ifs` runs before fast path | same | yes |
| IFS empty → no splitting, drop unquoted empties | `if ifs.is_empty()` branch runs **before** fast path | same | yes |
| Quoted bytes are protected from splitting | `needs_splitting` checks `!is_quoted(i)` | `split_field` checks `!quoted` per byte | yes |
| Consecutive IFS-whitespace collapses | N/A (fast path means no IFS bytes present) | handled by state machine | yes |
| IFS non-whitespace produces empty field | N/A (fast path means no IFS bytes present) | handled by state machine | yes |
| UTF-8 multibyte characters | byte-level scan; UTF-8 continuation bytes (0x80–0xBF) cannot collide with ASCII IFS chars | same byte-level scan | yes |

## 6. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Hypothesis B holds (allocations are inside `append_byte`, not `out` growth) | Medium | Savings still materialize because fast path skips `append_byte` too, but spec narrative becomes misleading | Investigation in §8 confirms hypothesis before writing implementation plan; rewrite §1 / §3 if B holds |
| `needs_splitting` byte-scan adds measurable overhead on slow-path hits | Low | `expand_field_split` Criterion regresses | Criterion comparison in §8; fallback is `[bool; 256]` lookup table keyed by byte (constant-time membership) |
| A hidden caller of `split` relies on empty-unquoted drop | Low | Regression in one specific call path | §7.2 survey + `cargo test` + `./e2e/run_tests.sh` cover the contract |
| `fields.iter().all(...)` full scan on mixed inputs wastes work | Low | Microseconds per call in the worst mixed case | Measured through `expand_field_split` bench in §8; acceptable if within ±5% |
| Hit rate on W2 is low (<50%) | Low | DoD-A / DoD-B miss | Hit-rate probe in §8 step 3; if <50%, revisit approach 2 (field-level fast path) |

## 7. Scope

### 7.1 In scope

- `src/expand/field_split.rs` — add `needs_splitting` helper and fast-path guard in `split()`.
- `src/expand/field_split.rs::tests` — add the 6 unit tests listed in §9.1.

### 7.2 Caller survey (to run during investigation, §8 step 0)

Before writing the implementation plan, grep for call sites of `field_split::split`:

```bash
rg 'field_split::split\b' --type rust
```

Expected sites (from current code reading, confirmed via `rg` pre-spec):
- `src/expand/mod.rs:108` inside `expand_word` — the sole direct caller; routes result through the final filter at line 116. `expand_words` (line 123) reaches `split` only transitively via `expand_word`.

If any additional call site is found that bypasses the final `!f.is_empty() || f.was_quoted` filter, the fast path must replicate the drop behavior there. The simplest mitigation is to add the filter inside the fast path:

```rust
if fields.iter().all(|f| !needs_splitting(f, &ifs_ws, &ifs_nws) && (!f.value.is_empty() || f.was_quoted)) {
    return fields;
}
```

This keeps the fast path zero-allocation while covering the divergent case. Decision is recorded in the implementation plan based on the survey outcome.

### 7.3 Out of scope

- `split_field` / `append_byte` / `emit` internal optimizations
- `pattern::matches` caching (§4.4)
- `exec_function_call` sub-benches (§4.2)
- IFS semantics changes

## 8. Verification Plan

### 8.1 Investigation phase (mandatory, runs first)

Before implementation, produce a short investigation log that will be appended to this spec as §10:

**Step 0 — Caller survey (re-confirm).** §7.2 pre-confirms the single call site at `src/expand/mod.rs:108`. Re-run the grep in case intervening commits added callers; decide whether empty-unquoted-drop replication in the fast path is needed.

**Step 1 — Dhat stack dump.**
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
mv dhat-heap.json target/perf/dhat-heap-w2.json
```
Load in browser: `dh_view.html?file=...`. Drill into the `field_split::emit (180:9)` entry and capture the 3–5 frames immediately above the malloc call.

**Step 2 — Hypothesis classification.**
- Hypothesis A confirmed iff top non-yosh frame is `alloc::raw_vec::finish_grow` (or equivalent) reached via `Vec::<ExpandedField>::push` / `emit`.
- Hypothesis B confirmed iff top non-yosh frame is `String::push_str`-internal reached via `append_byte` / `push_unquoted` / `push_quoted` / `set_range`, with `emit` as the nearest *yosh* frame due to inlining collapse.
- If neither matches, record finding and **halt** (return to brainstorming).

**Step 3 — Fast-path hit-rate probe.** Add a temporary (not committed) `eprintln!` in `split()` logging whether the fast-path branch was taken, run W2, count:
```
fast_path_hits / total_split_calls  ≥ 0.5  (warn threshold)
                                    ≥ 0.9  (target)
```
Below 0.5 → warn and consider approach 2 instead. Between 0.5–0.9 → proceed with caveat. At or above 0.9 → proceed as planned.

### 8.2 Correctness verification (post-implementation)

| Step | Command | Pass criterion |
|---|---|---|
| Unit tests | `cargo test --lib expand::field_split` | 18/18 pass (existing 12 + new 6) |
| Full crate tests | `cargo test` | all pass |
| E2E | `./e2e/run_tests.sh` | all pass (POSIX compliance is primary) |
| W2 output diff | `./target/profiling/yosh benches/data/script_heavy.sh > /tmp/post.out 2>&1; diff /tmp/pre.out /tmp/post.out` | empty diff |

### 8.3 Performance verification (post-implementation)

**DoD-A (site-level):**
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
mv dhat-heap.json target/perf/dhat-heap-w2-postfix.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-postfix.json 10
```
Pass criterion: sum of all `field_split::emit (src/expand/field_split.rs:180:9)` entries in Top-10 **< 2.0 MB** (from pre-fix 4.63 MB).

**DoD-B (total):**
Pass criterion: "total bytes allocated" line in `dhat-heap-w2-postfix.json` **≤ 10.2 MB** (from pre-fix 11.39 MB, −10% minimum).

**Criterion regression check:**
```bash
cargo bench --bench expand_bench -- expand_field_split
```
Pass criterion: median within ±5% of pre-fix (absolute numbers will be recorded; not a blocker unless larger regression observed).

## 9. Tests

### 9.1 Unit tests to add (`src/expand/field_split.rs::tests`)

```rust
// ── Fast-path coverage ──

#[test]
fn test_fast_path_single_field_no_ifs_chars() {
    let env = env_with_ifs(" \t\n");
    let input = vec![unquoted("hello")];
    assert_eq!(values(split(&env, input)), vec!["hello"]);
}

#[test]
fn test_fast_path_multiple_fields_no_ifs_chars() {
    let env = env_with_ifs(" \t\n");
    let input = vec![unquoted("hello"), unquoted("world")];
    assert_eq!(values(split(&env, input)), vec!["hello", "world"]);
}

#[test]
fn test_fast_path_mixed_quoted_unquoted_no_ifs() {
    let env = env_with_ifs(" ");
    let mut f = ExpandedField::new();
    f.push_unquoted("foo");
    f.push_quoted("bar");
    assert_eq!(values(split(&env, vec![f])), vec!["foobar"]);
}

#[test]
fn test_fast_path_utf8_no_false_positive() {
    // UTF-8 continuation bytes (0x80-0xBF) must not be mistaken for IFS.
    let env = env_with_ifs(" \t\n");
    let input = vec![unquoted("日本語")];
    assert_eq!(values(split(&env, input)), vec!["日本語"]);
}

#[test]
fn test_slow_path_triggered_by_one_splittable_field() {
    let env = env_with_ifs(" ");
    let input = vec![unquoted("hello"), unquoted("a b")];
    assert_eq!(values(split(&env, input)), vec!["hello", "a", "b"]);
}

#[test]
fn test_fast_path_quoted_ifs_byte_stays_fast() {
    // IFS byte inside quoted context does not trigger slow path.
    let env = env_with_ifs(" ");
    let mut f = ExpandedField::new();
    f.push_quoted("a b c");
    assert_eq!(values(split(&env, vec![f])), vec!["a b c"]);
}
```

### 9.2 Existing tests (must still pass)

All 12 existing tests in `field_split::tests` remain unchanged. `test_split_quoted_not_split` partially covers the fast path already; the new tests fill in the unquoted-no-delimiter, UTF-8, and mixed-field cases.

## 10. Investigation Log (populated during execution)

_Populated 2026-04-21 during Task 1 of the implementation plan._

### Caller survey (Step 1)

`rg 'field_split::split\b' --type rust` result: exactly one match at `src/expand/mod.rs` (line `let fields = field_split::split(env, fields);` inside `expand_word`). Decision on empty-unquoted-drop replication: not needed — the single call site routes through `expand_word`'s final filter `!f.is_empty() || f.was_quoted` at line 116.

### Pre-fix baseline (Steps 2–3)

- W2 stdout size: 11 bytes; stderr size: 21 bytes (saved to `/tmp/w2_prefix.out`, `/tmp/w2_prefix.err`).
- `target/perf/dhat-heap-w2-prefix.json`: total allocation 11,390,568 bytes (≈ 11.39 MB) / 283,350 blocks.
- `field_split::emit (src/expand/field_split.rs:180:9)` entries (rank / bytes / calls):
  - Rank #1: 2.94 MB / 14,020 calls
  - Rank #2: 1.48 MB / 7,013 calls
  - Rank #7: 209.5 KB / 18 calls
- 3-site sum: ≈ 4.63 MB / 21,051 calls.

### Hypothesis classification (Step 4)

Top non-yosh frames above `field_split::emit (180:9)` for rank #1 entry (frame index 590 in ftbl):
1. `alloc::raw_vec::RawVecInner<A>::finish_grow (???:0:0)`
2. `alloc::raw_vec::RawVecInner<A>::grow_amortized (src/raw_vec/mod.rs:512:33)`
3. `alloc::raw_vec::RawVecInner<A>::grow_one (src/raw_vec/mod.rs:476:41)`
4. `alloc::raw_vec::RawVec<T,A>::grow_one (src/raw_vec/mod.rs:188:29)`
5. `alloc::vec::Vec<T,A>::push_mut (src/vec/mod.rs:1034:22)`

Classification: **Hypothesis A**. Rationale: the top non-yosh frames are `alloc::raw_vec::RawVecInner::grow_amortized` / `grow_one` reached via `Vec::push` inside `emit`, confirming that the allocations are growth of the `out: Vec<ExpandedField>` backing store, not `String` internal growth.

### Fast-path hit rate (Step 5)

W2: `total=11000 hits=10998 ratio=1.000`. Decision: **proceed** — ratio ≥ 0.9 threshold met with ratio = 1.000 (effectively all calls are fast-path eligible on W2).

## 11. Follow-ups

After landing and re-measuring:

- Update `performance.md` §3.2 dhat Top-10 tables (both by bytes and by calls) with new numbers.
- Update `performance.md` §4.N (new section) recording the investigation findings and fix outcome.
- Update `performance.md` §5.1 priority matrix (move §5.2 P0 → done).
- Update `performance.md` §5.2 next-project queue: promote `pattern::matches` (§4.4) to the new P0.
- Update `performance.md` §5.3 TODO delta (strike completed item).
- Update `TODO.md`: strike the `field_split::emit` entry added 2026-04-21; the `exec_function_call` and `pattern::matches` entries remain.
