# field_split::emit Fast Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an early-return fast path to `field_split::split` that skips the state machine and output-Vec allocation when no input field contains an unquoted IFS byte, eliminating the 4.63 MB / 21,051-call `field_split::emit` dhat hotspot in W2 (`benches/data/script_heavy.sh`).

**Architecture:** Two new pieces of code in `src/expand/field_split.rs`: a private `needs_splitting(field, ifs_ws, ifs_nws) -> bool` helper that scans unquoted bytes, and a 3-line guard in `split()` — placed after IFS partitioning and before the per-field loop — that returns the input `Vec` unchanged when `fields.iter().all(|f| !needs_splitting(f, ...))`. Semantically equivalent to the slow path for all inputs where no unquoted IFS byte is present; POSIX XBD 8.3 behavior is preserved.

**Tech Stack:** Rust (edition 2024), criterion benches, dhat-rs heap profiling, existing `yosh-dhat` binary, `./e2e/run_tests.sh` POSIX compliance harness.

**Spec:** [`docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md`](../specs/2026-04-21-field-split-fast-path-design.md)

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/expand/field_split.rs` | Modify | Add `needs_splitting` helper + fast-path guard in `split()`; add 6 regression-guard unit tests |
| `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` | Modify | Append §10 Investigation Log with dhat frames, hypothesis classification, fast-path hit rate |
| `performance.md` | Modify | Update §3.2 dhat Top-10 tables, add new §4.N for the fix outcome, update §5.1 / §5.2 / §5.3 |
| `TODO.md` | Modify | Strike the `field_split::emit` entry added 2026-04-21 |
| `target/perf/dhat-heap-w2-{pre,post}fix.json` | Create (gitignored) | dhat artifacts for before/after measurement |
| `/tmp/w2_prefix.out` | Create (gitignored) | Pre-fix W2 stdout for bit-identical verification |

No new files are created inside the source tree. All artifacts that could be checked in are docs.

---

## Task 1: Investigation phase (§8.1)

**Files:**
- Create: `target/perf/dhat-heap-w2-prefix.json` (artifact, gitignored)
- Create: `/tmp/w2_prefix.out` (artifact, gitignored)
- Modify: `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md:319-321` (replace §10 placeholder with filled log)

**Goal:** Verify hypothesis A (dhat allocation at `field_split.rs:180:9` = `out: Vec<ExpandedField>` growth), confirm single caller, measure fast-path hit rate on W2. No production code changes in this task.

- [ ] **Step 1: Re-confirm caller survey**

Run:
```bash
rg 'field_split::split\b' --type rust
```

Expected: exactly one match at `src/expand/mod.rs:108`. If additional matches appear, note them in the investigation log and re-evaluate the "empty-unquoted drop" question in spec §5.1. If only the expected match appears, proceed with the original plan.

- [ ] **Step 2: Capture pre-fix W2 stdout/stderr baseline**

This is the reference output that post-fix W2 must match bit-for-bit (spec §8.2 "W2 output diff").

Run:
```bash
cargo build --profile profiling --bin yosh
./target/profiling/yosh benches/data/script_heavy.sh > /tmp/w2_prefix.out 2> /tmp/w2_prefix.err
wc -c /tmp/w2_prefix.out /tmp/w2_prefix.err
```

Expected: non-zero byte counts on both files, build completes without errors. Note the byte sizes in the investigation log so the post-fix comparison is reproducible.

- [ ] **Step 3: Capture pre-fix dhat profile**

Run:
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh > /dev/null 2>&1
mkdir -p target/perf
mv dhat-heap.json target/perf/dhat-heap-w2-prefix.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-prefix.json 10
```

Expected output includes a line matching `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` with bytes near 2.94 MB and ~14,020 calls. Record the exact numbers (bytes, calls) for all three `field_split.rs:180:9` entries; they become the pre-fix baseline for DoD-A.

- [ ] **Step 4: Classify hypothesis A vs B via dhat stack inspection**

Launch the dhat HTML viewer so the full call stacks are visible:

```bash
python3 -m http.server 8000 --directory target/perf >/dev/null 2>&1 &
# Open http://localhost:8000/dhat-heap-w2-prefix.json in dh_view.html
# (or use any local dh_view.html that loads the JSON)
kill %1
```

Alternative without a viewer — extract stacks directly with `jq`:

```bash
jq '.pps[] | select(.fs[0] as $id | .f[$id]?.d // "" | test("field_split.rs:180")) | .fs' \
    target/perf/dhat-heap-w2-prefix.json | head -20
```

Examine the frame list immediately above `field_split::emit`:

- **Hypothesis A confirmed** iff the top non-yosh frame is an `alloc::raw_vec` or `Vec::push`-internal frame (e.g., `alloc::raw_vec::RawVec::grow_amortized`, `core::ptr::write`). Expected for the rank #1/#2 entries.
- **Hypothesis B confirmed** iff the top non-yosh frame is a `String::push_str` or `Vec::<u64>::resize`-internal frame reached through inlined `append_byte` / `push_unquoted` / `push_quoted` / `set_range`.
- **Neither matches** — halt task 1, return to brainstorming (spec §6 risk row 1).

Record the top 3–5 frames verbatim (with Rust demangled names) for each of the three `field_split.rs:180:9` dhat entries.

- [ ] **Step 5: Measure fast-path hit rate on W2 (temporary instrumentation)**

Temporarily add to `src/expand/field_split.rs` inside `split()`, right before the `Vec::new()` line (45):

```rust
    // TEMP instrumentation — do NOT commit
    use std::sync::atomic::{AtomicUsize, Ordering};
    static TOTAL: AtomicUsize = AtomicUsize::new(0);
    static HITS: AtomicUsize = AtomicUsize::new(0);
    let would_fast_path = fields.iter().all(|f| {
        f.value.bytes().enumerate().all(|(i, b)| {
            f.is_quoted(i) || (!ifs_ws.contains(&b) && !ifs_nws.contains(&b))
        })
    });
    let t = TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    let h = if would_fast_path { HITS.fetch_add(1, Ordering::Relaxed) + 1 } else { HITS.load(Ordering::Relaxed) };
    if t % 500 == 0 || t <= 5 {
        eprintln!("[fast-path-probe] total={t} hits={h} ratio={:.3}", h as f64 / t as f64);
    }
```

Rebuild and run W2:

```bash
cargo build --profile profiling --bin yosh
./target/profiling/yosh benches/data/script_heavy.sh > /dev/null 2>/tmp/fp_probe.log
tail -5 /tmp/fp_probe.log
```

Expected: the final line shows `total=N hits=M ratio=X.XXX`. Target thresholds per spec §8.1 step 3:

- `ratio >= 0.9` → proceed as planned (aligns with approach 1 rationale).
- `0.5 <= ratio < 0.9` → proceed with a caveat noted in the investigation log (fast path helps, but <90% means mixed workloads remain a concern).
- `ratio < 0.5` → stop task 1 and escalate: the §5.2 mitigation is to reconsider approach 2 (field-level fast path) from brainstorming Section 1.

**Critical:** after recording the ratio, revert the instrumentation with `git checkout src/expand/field_split.rs` so no temp code is committed.

- [ ] **Step 6: Append investigation log to spec §10**

Edit `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md`. Replace the `§10. Investigation Log` placeholder (current content: `_Placeholder. Filled in after §8.1 completes; ..._`) with:

```markdown
_Populated 2026-04-21 during Task 1 of the implementation plan._

### Caller survey (Step 1)

`rg 'field_split::split\b' --type rust` result: <paste match(es) here>. Decision on empty-unquoted-drop replication: <not needed / needed>.

### Pre-fix baseline (Steps 2–3)

- W2 stdout size: <N> bytes; stderr size: <M> bytes (saved to `/tmp/w2_prefix.out`, `/tmp/w2_prefix.err`).
- `target/perf/dhat-heap-w2-prefix.json`: total allocation <X> MB / <Y> blocks.
- `field_split::emit (src/expand/field_split.rs:180:9)` entries (rank / bytes / calls):
  - Rank #<a>: <bytes> / <calls>
  - Rank #<b>: <bytes> / <calls>
  - Rank #<c>: <bytes> / <calls>
- 3-site sum: <total> MB / <total> calls.

### Hypothesis classification (Step 4)

Top non-yosh frames above `field_split::emit (180:9)` for rank #1 entry:
1. <frame>
2. <frame>
3. <frame>

Classification: **<A / B / other>**. Rationale: <one sentence>.

### Fast-path hit rate (Step 5)

W2: `total=<N> hits=<M> ratio=<X.XXX>`. Decision: **<proceed / proceed-with-caveat / halt>**.
```

Replace each `<...>` marker with real values. Commit this change alone (docs only):

```bash
git add docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md
git commit -m "$(cat <<'EOF'
docs(perf): fill field_split fast path investigation log

Append §10 Investigation Log with caller survey, pre-fix dhat baseline,
hypothesis classification, and fast-path hit rate from W2.

Prompt: performance.md を参照してで遅い原因を対応してください

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Verify no instrumentation leaked**

Run:
```bash
rg 'fast-path-probe|TEMP instrumentation' src/
git status
```

Expected: zero matches from rg; `git status` shows clean working tree after the docs commit. If either fails, revert the leaked code before proceeding to Task 2.

---

## Task 2: Add regression-guard unit tests

**Files:**
- Modify: `src/expand/field_split.rs:183-308` (append to `mod tests`)

**Goal:** Add five unit tests covering fast-path-relevant input shapes. These tests **must pass without the fast-path code** because the slow path already produces identical output — they exist as regression guards to ensure the fast path remains observationally equivalent when it lands in Task 3.

**Note (2026-04-21):** The originally-planned UTF-8 false-positive test (`test_fast_path_utf8_no_false_positive`) was omitted after discovering a pre-existing `append_byte` UTF-8 slicing bug in the slow path (recorded in `TODO.md` under **Known Bugs**). A UTF-8 regression test for the fast path specifically should be added in a follow-up once the slow-path UTF-8 issue is fixed, or as a Task-3 post-implementation step targeting only the fast-path code path.

- [ ] **Step 1: Open the tests module and add the fast-path section**

Find the end of `mod tests` in `src/expand/field_split.rs` (currently around line 307, the closing `}` of `test_double_colon_empty_field`). Before that closing brace of `mod tests`, append:

```rust

    // ── Fast-path coverage (§9.1 of fast-path spec) ──

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

Each test uses existing helpers defined earlier in `mod tests`: `env_with_ifs`, `unquoted`, `values`, `ExpandedField::new`, and `ExpandedField::push_{quoted,unquoted}`. No new helpers are required.

- [ ] **Step 2: Run the new tests (they must pass against the current slow-path code)**

Run:
```bash
cargo test --lib expand::field_split
```

Expected output (truncated):
```
test expand::field_split::tests::test_fast_path_single_field_no_ifs_chars ... ok
test expand::field_split::tests::test_fast_path_multiple_fields_no_ifs_chars ... ok
test expand::field_split::tests::test_fast_path_mixed_quoted_unquoted_no_ifs ... ok
test expand::field_split::tests::test_slow_path_triggered_by_one_splittable_field ... ok
test expand::field_split::tests::test_fast_path_quoted_ifs_byte_stays_fast ... ok
...
test result: ok. 15 passed; 0 failed; 0 ignored
```

If any test fails at this stage, the slow path has a bug that must be investigated before adding the fast path — STOP and investigate. Expected: all 15 pass (existing 10 + new 5).

- [ ] **Step 3: Run the full crate test suite for no regression**

Run:
```bash
cargo test
```

Expected: all tests pass. Capture the final `test result: ok. N passed; 0 failed` line in the commit body.

- [ ] **Step 4: Commit the regression-guard tests**

Run:
```bash
git add src/expand/field_split.rs
git commit -m "$(cat <<'EOF'
test(expand): add field_split fast-path regression guards

Add five unit tests covering the input shapes the incoming
field_split::split fast path will handle:
- single/multiple unquoted fields with no IFS bytes
- mixed quoted/unquoted parts with no unquoted IFS
- slow path still triggers when any one field needs splitting
- quoted IFS byte does not trigger slow path

All pass against the current slow-path implementation; they
guard against divergence when the fast path lands.

Prompt: performance.md を参照してで遅い原因を対応してください

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Implement the fast path

**Files:**
- Modify: `src/expand/field_split.rs:24-50` (`split()` function body) and the module tail (add `needs_splitting` helper after `split_field`).

**Goal:** Add `needs_splitting` and the guard block in `split()`. All 15 tests stay green; W2 stdout/stderr remain bit-identical to the Task 1 baseline.

- [ ] **Step 1: Add the `needs_splitting` helper**

Insert after the `split_field` function (currently ending at line 163) and before `append_byte` (currently at line 166). Add:

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

- [ ] **Step 2: Add the fast-path guard in `split()`**

In `src/expand/field_split.rs`, locate `split()` (currently line 24). After the `ifs_nws` `Vec<u8>` binding (currently line 43) and before the existing `let mut result = Vec::new();` (currently line 45), insert:

```rust

    // Fast path: if no field contains an unquoted IFS byte, the state
    // machine would emit each input field unchanged. Return without
    // allocating a new Vec or rebuilding ExpandedFields via `emit`.
    if fields.iter().all(|f| !needs_splitting(f, &ifs_ws, &ifs_nws)) {
        return fields;
    }
```

The full post-edit `split()` body reads:

```rust
pub fn split(env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    let ifs = get_ifs(env);

    // IFS empty: no splitting; drop fully-empty unquoted fields.
    if ifs.is_empty() {
        return fields
            .into_iter()
            .filter(|f| !f.value.is_empty() || f.was_quoted)
            .collect();
    }

    // Partition IFS characters.
    let ifs_ws: Vec<u8> = ifs
        .bytes()
        .filter(|b| matches!(*b, b' ' | b'\t' | b'\n'))
        .collect();
    let ifs_nws: Vec<u8> = ifs
        .bytes()
        .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
        .collect();

    // Fast path: if no field contains an unquoted IFS byte, the state
    // machine would emit each input field unchanged. Return without
    // allocating a new Vec or rebuilding ExpandedFields via `emit`.
    if fields.iter().all(|f| !needs_splitting(f, &ifs_ws, &ifs_nws)) {
        return fields;
    }

    let mut result = Vec::new();
    for field in fields {
        split_field(&field, &ifs_ws, &ifs_nws, &mut result);
    }
    result
}
```

- [ ] **Step 3: Run field_split unit tests**

Run:
```bash
cargo test --lib expand::field_split
```

Expected: 15 passed (existing 10 + new 5), 0 failed. The fast-path tests now exercise the new code path; the slow-path tests still exercise the state machine.

- [ ] **Step 4: Run the full crate test suite**

Run:
```bash
cargo test
```

Expected: all tests pass. In particular, nothing in `src/expand/*` or `src/exec/*` regresses.

- [ ] **Step 5: Run the E2E POSIX compliance suite**

Run:
```bash
cargo build --profile profiling --bin yosh
./e2e/run_tests.sh
```

Expected: the final line reports `Passed: N/N` with no `[FAIL]` or `[TIMEOUT]` lines. If isolated POSIX failures appear, check whether they are pre-existing flakes (see TODO.md "Full E2E suite occasional transient failures") by re-running once; genuine regressions block this task.

- [ ] **Step 6: Verify W2 output is bit-identical with the pre-fix baseline**

Run:
```bash
./target/profiling/yosh benches/data/script_heavy.sh > /tmp/w2_postfix.out 2> /tmp/w2_postfix.err
diff /tmp/w2_prefix.out /tmp/w2_postfix.out
diff /tmp/w2_prefix.err /tmp/w2_postfix.err
```

Expected: both `diff` commands produce no output and exit 0. Any diff is a correctness regression and must be investigated before committing.

- [ ] **Step 7: Commit the implementation**

Run:
```bash
git add src/expand/field_split.rs
git commit -m "$(cat <<'EOF'
perf(expand): add fast path to field_split::split

Add a guard in split() that returns the input Vec unchanged
when no field contains an unquoted IFS byte. Skips the per-field
state machine and the output-Vec allocation for the common case
(verified against W2: see performance.md §4.N and spec §10).

POSIX XBD 8.3 semantics preserved; existing 10 + new 5 unit
tests, full cargo test, and ./e2e/run_tests.sh all pass. W2
stdout/stderr bit-identical with pre-fix baseline.

Prompt: performance.md を参照してで遅い原因を対応してください

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Performance measurement (DoD-A and DoD-B)

**Files:**
- Create: `target/perf/dhat-heap-w2-postfix.json` (artifact, gitignored)

**Goal:** Confirm DoD-A (3-site `field_split::emit` sum < 2.0 MB) and DoD-B (W2 total allocation ≤ 10.2 MB). Record pre/post numbers for the performance.md update in Task 5.

- [ ] **Step 1: Capture post-fix dhat profile**

Run:
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh > /dev/null 2>&1
mv dhat-heap.json target/perf/dhat-heap-w2-postfix.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-postfix.json 10
```

Expected: the Top-10 output shows `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` either absent from the list or with much smaller bytes/calls than pre-fix.

- [ ] **Step 2: Extract and record the W2 totals**

Run:
```bash
jq '.totals // {total_bytes: .total_bytes_alloc, total_blocks: .total_blocks_alloc}' \
    target/perf/dhat-heap-w2-postfix.json 2>/dev/null \
  || python3 -c '
import json; d = json.load(open("target/perf/dhat-heap-w2-postfix.json"))
print("bytes=", d.get("tb", d.get("tbk", "?")))
print("blocks=", d.get("tbk", "?"))
'
```

(`dhat-heap.json` field names vary by dhat-rs version — prefer the summary at the top of the dhat HTML view if the above fails. Record the values verbatim.)

Pre-fix reference (from performance.md §3.2): 11.39 MB allocated / 283,350 blocks.

Record: `post_bytes = X.XX MB`, `post_blocks = Y`.

- [ ] **Step 3: Compute DoD-A**

Sum the bytes of every Top-10 entry whose site is `src/expand/field_split.rs:180:9` (there may be 1, 2, or 3 remaining after the fix).

Pre-fix aggregate: 4.63 MB / 21,051 calls.

Pass criterion: **post-fix aggregate < 2.0 MB**. If the site is entirely absent from Top-10, treat its contribution as 0 and the pass is trivial. Record the aggregate in the investigation log or performance.md §4.N.

- [ ] **Step 4: Check DoD-B**

Pass criterion: **post-fix `total_bytes ≤ 10.2 MB`** (from pre-fix 11.39 MB; −10% minimum).

If either DoD-A or DoD-B fails:
- Do not revert the code. Record the miss.
- Investigate which site is now dominant via the Top-10 output.
- Proceed to Task 5 documentation with the actual numbers; the spec's §8.3 target becomes a recorded miss rather than a blocker.
- File a follow-up in TODO.md for secondary optimization.

- [ ] **Step 5: Run the `expand_field_split` Criterion bench**

Run:
```bash
cargo bench --bench expand_bench -- expand_field_split 2>&1 | tee /tmp/expand_field_split.bench
```

Read the median from the Criterion output (line "time:   [X µs Y µs Z µs]" → middle value is median). Pre-fix median per performance.md §3.2: 2.64 ms.

Pass criterion: median within **±5%** of pre-fix. If it regresses more than 5%, file a follow-up but do not block on it — the DoD for this project is allocation-focused (§5 of spec), not latency-focused.

- [ ] **Step 6: No commit in this task**

Task 4 produces measurements only — they feed Task 5. No code or docs changes yet.

---

## Task 5: Update performance.md and TODO.md

**Files:**
- Modify: `performance.md` (multiple sections: §1, §3.2 dhat tables, add §4.N, §5.1, §5.2, §5.3, §7 Scope amendment)
- Modify: `TODO.md` (remove the `field_split::emit` entry added 2026-04-21)

**Goal:** Record the fix outcome in the project's two tracking documents. This mirrors what `610343e` did for §4.3 (`pathname::expand` fast path) at commits `c014eed` and `9e343d0`.

- [ ] **Step 1: Update performance.md §1 Executive Summary**

Replace the "Remaining hotspots" bullet for §5.2 with the post-fix state:

- `field_split::emit` entry: move to the "Non-findings" / completed list, or strike-through with the new dhat numbers.
- Promote `pattern::matches` (§4.4) to rank 1 remaining P0 in the summary.

Use the same tone as the existing pre-fastpath note in §1 (e.g., "_Fixed 2026-04-21 via …_"). Include the W2 total delta: `11.39 MB → X.XX MB`.

- [ ] **Step 2: Refresh the §3.2 dhat Top-10 tables**

There are two tables in §3.2: "by bytes" and "by call count". Replace them with the post-fix output from `scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-postfix.json 10`.

Update the prose paragraph directly before each table to reflect the new rank-1 site (likely `pattern::matches` or its siblings). Preserve the pre-fix historical table as a footnote if helpful for traceability, following the §3.2 "Pre-fix W2 dhat Top-10 — historical" pattern.

- [ ] **Step 3: Add a new §4.7 for the field_split fast path fix**

Insert after §4.6 (before §5). Use this skeleton, filling in the values from Task 4:

```markdown
### 4.7. `field_split::emit` fast path (fixed 2026-04-21)

**Location:** `src/expand/field_split.rs` — `split()` now guards the per-field loop with `needs_splitting` (added helper).

**Measurement (W2 at pre-fix `<commit>`):** 4.63 MB / 21,051 calls across three dhat sites sharing `src/expand/field_split.rs:180:9` — rank #1 (2.94 MB / 14,020), rank #2 (1.48 MB / 7,013), rank #7 (209.5 KB / 18).

**Root cause (confirmed via hypothesis <A or B>):** <one paragraph summarizing the Task 1 investigation — cite the top frame(s) identified>.

**Fix applied (2026-04-21):**

Added a fast path at the top of `split()`: if `fields.iter().all(|f| !needs_splitting(f, ...))`, return the input `Vec` unchanged. The state machine in `split_field` and the output-`Vec` allocations via `emit` are all skipped. Semantic equivalence is guaranteed by the fact that the slow path would emit each input field unchanged when none contains unquoted IFS bytes. See commit `<sha>` and spec `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md`.

**Measured impact (W2, post-fix vs pre-fix at `<pre-fix-commit>`):**
- `field_split::emit (src/expand/field_split.rs:180:9)` aggregate: 4.63 MB / 21,051 calls → <post-bytes> / <post-calls> (**−<X>%**).
- W2 total allocation: 11.39 MB → <post_bytes> (**−<X>%**).
- `expand_field_split` Criterion median: 2.64 ms → <post-median> (<±X%>, within noise).
- Fast-path hit rate on W2: <ratio from Task 1 step 5>.

**TODO.md cross-check:** completed; entry for `field_split::emit` removed 2026-04-21.
```

- [ ] **Step 4: Update §5.1 priority matrix**

Locate the §4.3 row (already marked `done`) and add a new row for §4.7 between §4.3 and §4.4:

```markdown
| 4.7 — `field_split::emit` fast path | **Medium** (~4.63 MB aggregate pre-fix) | **Low** (+1 helper, 3-line guard) | **done** | Completed 2026-04-21 via `split()` fast path. W2 total <delta>, `field_split.rs:180:9` hotspot <eliminated/demoted> from Top-10. See `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` and `<sha>`. |
```

- [ ] **Step 5: Update §5.2 next-project queue**

Remove the old `P0 — Investigate field_split::emit` item. Re-rank the remaining items:

```markdown
1. **P0 — Fix 4.4 `pattern::matches` recompilation.** Estimated: 2 days. Promoted from P1 after §4.7 landed — now the top remaining allocation and call-count site.
2. **P1 — Add the fine-grained function-call sub-benches from 4.2 candidate #1**, then act on whatever they reveal (likely the `catch_unwind` replacement). Estimated: half a day for sub-benches, 1–3 days for the follow-up fix.
```

Add an "Amendment 2026-04-21 (§4.7)" paragraph mirroring the existing §4.3 amendment style:

```markdown
**Note (2026-04-21):** §4.7 has landed. Top remaining allocation sites are now `pattern::matches` (§4.4) at 50k calls / 1.25 MB and any residuals from `field_split::emit`. The function-call ratio (§4.2) remains ~2.1×.
```

- [ ] **Step 6: Update §5.3 items-to-add-to-TODO.md**

Strike the completed item and add any new follow-ups discovered during measurement:

```markdown
- ~~Investigate `field_split::emit (src/expand/field_split.rs:180:9)` allocation pattern~~ — **Completed 2026-04-21** via `split()` fast path (`<sha>`); dhat aggregate dropped from 4.63 MB to <post>.
- `exec_function_call` residual 2.1× overhead ratio vs arithmetic loop (§4.2) — **P1**, with sub-bench prerequisite.
- `pattern::matches` recompilation (§4.4) — **P0** (promoted from P1 after §4.7 landed).
```

- [ ] **Step 7: Add §7 Scope amendment for §4.7**

Append to §7 at the bottom:

```markdown
**Amendment 2026-04-21 (§4.7):** The `field_split::split` fast path was implemented at commit `<sha>` per `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md`. W2 total allocation fell from 11.39 MB to <post-MB> (−<X>%); the `field_split::emit:180:9` hotspot (pre-fix ranks #1/#2/#7) was <eliminated from Top-10 / demoted to rank #N>. `pattern::matches` (§4.4) is promoted to P0 in the §5.2 queue.
```

- [ ] **Step 8: Remove the `field_split::emit` entry from TODO.md**

Open `TODO.md`. Find the entry starting with:

```markdown
- [ ] `field_split::emit (src/expand/field_split.rs:180:9)` allocation pressure — new #1 dhat site post-fastpath at `610343e` …
```

(currently located under "Future: Code Quality Improvements", near the bottom of that section)

Delete the entire bullet (including any sub-bullets). Per `CLAUDE.md` "TODO.md" guidance: **delete** completed items rather than marking `[x]`.

- [ ] **Step 9: Verify markdown sanity**

Run:
```bash
# Sanity-check for broken references (cheap heuristic)
rg -n '§4\.7' performance.md
rg -n 'field_split::emit' performance.md TODO.md
```

Expected:
- `§4.7` appears in §1, §3.2, §4.7 header, §5.1, §5.2, §5.3, and §7 (at minimum 5 hits).
- `field_split::emit` should only appear in performance.md under the completed `§4.7` and any historical references; no active TODO.md entries.

- [ ] **Step 10: Commit the documentation update**

Run:
```bash
git add performance.md TODO.md
git commit -m "$(cat <<'EOF'
docs(perf): record field_split fast path outcome in performance.md

- Add §4.7 capturing the fix, measured impact, and hypothesis classification
- Update §3.2 dhat Top-10 tables with post-fix numbers
- §5.1 priority matrix: §4.7 marked done
- §5.2 next-project queue: pattern::matches promoted to P0
- §5.3 items-to-add-to-TODO.md: strike completed entry
- §7 Scope statement: add amendment for §4.7
- TODO.md: remove completed field_split::emit entry

Prompt: performance.md を参照してで遅い原因を対応してください

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 11: Final verification**

Run:
```bash
git log --oneline -6
git status
```

Expected: 4 new commits on top of the pre-plan HEAD (spec amendment, tests, implementation, docs); clean working tree. The investigation log commit is optional and can be folded into the spec-amendment commit if you prefer a 3-commit shape — but keeping 4 matches the DoD item 8 shape (spec already committed, so: log + tests + impl + docs = 4 new commits).

---

## Done criteria (mirrors spec §5 / §8)

All must be green before declaring the plan complete:

1. `cargo test --lib expand::field_split` → 15 passed (Task 3 Step 3).
2. `cargo test` → all passed (Task 3 Step 4).
3. `./e2e/run_tests.sh` → all passed (Task 3 Step 5).
4. `diff /tmp/w2_prefix.out /tmp/w2_postfix.out` → empty (Task 3 Step 6).
5. **DoD-A:** `field_split::emit` aggregate < 2.0 MB (Task 4 Step 3).
6. **DoD-B:** W2 total ≤ 10.2 MB (Task 4 Step 4).
7. `expand_field_split` Criterion within ±5% of pre-fix (Task 4 Step 5).
8. performance.md updated with §4.7 and dhat Top-10 refresh (Task 5).
9. TODO.md `field_split::emit` entry deleted (Task 5 Step 8).
10. Four new commits (spec-amendment, tests, impl, docs).
