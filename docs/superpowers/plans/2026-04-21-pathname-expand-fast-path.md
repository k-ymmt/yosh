# `pathname::expand` Non-Glob Fast Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the output-`Vec` allocation in `yosh::expand::pathname::expand` for the common case where no field contains unquoted glob metacharacters, removing the 2.94 MB / 14,020-call hotspot that is currently the #1 dhat site by bytes in W2.

**Architecture:** Single-file change. Add an early-return guard (`if !fields.iter().any(has_unquoted_glob_chars) { return fields; }`) at the top of `expand()` in `src/expand/pathname.rs`. The existing slow path handles the mixed / glob-present case unchanged. Semantics preserved because the guard fires only when no field would have entered the glob branch anyway.

**Tech Stack:** Rust (edition 2024), `cargo test`, `dhat-rs` (feature-gated via `--features dhat-heap`, binary `yosh-dhat`), `scripts/perf/dhat_top_n.py` for extraction.

**Spec:** `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md`

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/expand/pathname.rs` | Modify `expand()` (lines 15-33); add one unit test in the existing `#[cfg(test)] mod tests` block | Pathname expansion fast-path + regression guard test |
| `performance.md` | Update §3.2 dhat Top-10, §4.3 status, §5.1 priority matrix, §5.2 next-project queue, §5.3 items-to-TODO | Record measured outcome |
| `target/perf/dhat-heap-w2.json` | Regenerated (gitignored, not committed) | Post-fix dhat artifact |

No new files.

---

### Task 1: Add regression guard test for multi-field non-glob case

This test locks in the behavior that the fast path must preserve: a mix of non-glob fields and quoted-glob fields must round-trip unchanged. It passes on the current code (behavior is already correct); it serves as a regression guard for the upcoming change.

**Files:**
- Modify: `src/expand/pathname.rs` (append inside `#[cfg(test)] mod tests` at the end of file)

- [ ] **Step 1: Add the test**

Append the following test to the existing `mod tests` block in `src/expand/pathname.rs`, immediately after `test_has_unquoted_glob_chars_false_no_meta`:

```rust
    // ── Fast-path: multi-field non-glob passthrough ──

    #[test]
    fn test_fast_path_preserves_multiple_non_glob_fields() {
        let env = make_env();
        let input = vec![unquoted("hello"), unquoted("world"), quoted_field("*.rs")];
        let result = expand(&env, input);
        // All three fields must survive intact — "hello" and "world" have no
        // glob chars; "*.rs" is fully-quoted so `has_unquoted_glob_chars`
        // returns false. The multi-field non-glob path is the dominant W2
        // case and this regression guard protects it across the fast-path
        // refactor.
        assert_eq!(values(result), vec!["hello", "world", "*.rs"]);
    }
```

- [ ] **Step 2: Run the test (expected: PASS on current code)**

Run: `cargo test --lib expand::pathname::tests::test_fast_path_preserves_multiple_non_glob_fields -- --nocapture`

Expected: `test result: ok. 1 passed`. This confirms the regression guard is consistent with current semantics **before** the fast-path change.

- [ ] **Step 3: Run all pathname tests to ensure no unrelated breakage**

Run: `cargo test --lib expand::pathname`

Expected: all 8 tests pass (7 existing + 1 new).

- [ ] **Step 4: Commit**

```bash
git add src/expand/pathname.rs
git commit -m "test(expand): add multi-field non-glob regression guard for pathname::expand

Regression guard for the upcoming fast-path refactor (P0 §4.3).
Passes on current code; locks in semantics before the change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Implement the fast-path guard

**Files:**
- Modify: `src/expand/pathname.rs:15-33` (the `expand()` function)

- [ ] **Step 1: Apply the fast-path guard**

In `src/expand/pathname.rs`, change the `expand` function from:

```rust
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    let mut result = Vec::new();
    for field in fields {
        if has_unquoted_glob_chars(&field) {
            let matches = glob_match(&field.value);
            if matches.is_empty() {
                // No match — keep original field unchanged.
                result.push(field);
            } else {
                for m in matches {
                    result.push(ExpandedField::all_quoted(m));
                }
            }
        } else {
            result.push(field);
        }
    }
    result
}
```

to:

```rust
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    // Fast path: if no field contains unquoted glob metachars, return input as-is.
    // Avoids the output-Vec allocation that is the #1 dhat site by bytes in W2
    // (~2.94 MB / 14k calls). See docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md.
    if !fields.iter().any(has_unquoted_glob_chars) {
        return fields;
    }
    let mut result = Vec::new();
    for field in fields {
        if has_unquoted_glob_chars(&field) {
            let matches = glob_match(&field.value);
            if matches.is_empty() {
                // No match — keep original field unchanged.
                result.push(field);
            } else {
                for m in matches {
                    result.push(ExpandedField::all_quoted(m));
                }
            }
        } else {
            result.push(field);
        }
    }
    result
}
```

Net change: +4 lines (3 code + 1 blank) at the top of the function body. The slow-path loop is unchanged.

- [ ] **Step 2: Run the pathname test suite (expected: all PASS)**

Run: `cargo test --lib expand::pathname`

Expected: all 8 tests pass. The regression guard from Task 1 exercises the fast path; the slow-path tests (`test_glob_src_files`, `test_no_match_keeps_pattern`) still exercise the loop.

- [ ] **Step 3: Run the full expand module tests**

Run: `cargo test --lib expand::`

Expected: all pass. No module-boundary breakage.

- [ ] **Step 4: Commit**

```bash
git add src/expand/pathname.rs
git commit -m "perf(expand): add non-glob fast path to pathname::expand

Eliminates the output-Vec allocation when no field contains unquoted
glob metacharacters — the #1 dhat site by bytes in W2
(2.94 MB / 14,020 calls at HEAD 2261638).

The slow-path semantics are unchanged; the guard fires only when no
field would have entered the glob branch anyway, so POSIX 2.6.6 rules
are preserved.

Spec: docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Full test suite + E2E regression check

**Files:** none modified (verification only).

- [ ] **Step 1: Run the full cargo test suite**

Run: `cargo test` (use a timeout of at least 300000ms — this takes 1-3 minutes on this codebase).

Expected: all tests pass. No regressions introduced by the fast path.

If anything fails, stop and investigate. The fast path should not change any observable behavior; any failure indicates either (a) a test was implicitly depending on the output `Vec` being a distinct allocation from the input, or (b) an unrelated flaky test (see TODO.md `tests/signals.rs parallel-load flakes`). In case (a), the fix is wrong and must be rethought. In case (b), re-run the single failing test file with `cargo test --test <name>` to confirm the flake.

- [ ] **Step 2: Build for E2E**

Run: `cargo build`

Expected: clean build (E2E requires a debug build per CLAUDE.md).

- [ ] **Step 3: Run the POSIX E2E suite**

Run: `./e2e/run_tests.sh`

Expected: the same pass/fail ratio as before the change. Pay particular attention to tests under `e2e/posix_spec/2_06_06_pathname_expansion/` if any exist; more broadly, any E2E test that uses glob patterns exercises `pathname::expand`.

If the POSIX 2.6.6 pass ratio degrades, the fast path has a semantic bug. If totally unrelated tests regress, investigate the flake pattern.

- [ ] **Step 4: Spot-check the E2E output**

Run: `./e2e/run_tests.sh 2>&1 | tail -20`

Expected output format: a summary line like `XXX/XXX passed`. Record the ratio — we will compare against the pre-change ratio (available in prior CI / git history if needed, but the key invariant is "no new failures").

No commit for this task — verification only.

---

### Task 4: Verify allocation reduction via dhat W2

**Files:** none modified (measurement only).

- [ ] **Step 1: Build the dhat-enabled binary**

Run: `cargo build --profile profiling --bin yosh-dhat --features dhat-heap`

Expected: clean build. Uses a 300000ms+ timeout; profiling profile builds are slow.

- [ ] **Step 2: Move any prior dhat artifact aside**

Run: `mv target/perf/dhat-heap-w2.json target/perf/dhat-heap-w2.pre-fastpath.json 2>/dev/null || true`

(Keeps the pre-fix artifact for before/after comparison. The `|| true` tolerates a missing file on fresh machines.)

- [ ] **Step 3: Run the W2 workload under dhat**

Run:
```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
```

Expected: produces `dhat-heap.json` in the working directory. Exit code 0.

- [ ] **Step 4: Move the artifact into `target/perf/`**

Run: `mv dhat-heap.json target/perf/dhat-heap-w2.json`

- [ ] **Step 5: Extract the Top-10 by bytes**

Run:
```bash
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 10
```

Expected: Top-10 table printed to stdout. Save the full output — it will be pasted into `performance.md` §3.2 in Task 5.

- [ ] **Step 6: Verify the success criteria**

Check the Top-10 output against the spec §7 success criteria:

1. Either `src/expand/pathname.rs:29:20` has **disappeared** from the Top-10 by bytes, **or** its byte total has **dropped by ≥90%** (was 2.94 MB; must be ≤0.3 MB).
2. The run's total allocation (printed at the top of the dhat extractor output) has dropped from ~13.78 MB to roughly **~10.8 MB ± 0.3 MB**.

If either criterion fails:
- Double-check that the fast-path change actually landed (`git log -1 --stat` should show the Task 2 commit).
- Re-read the dhat output carefully — the allocation may have shifted to a different line (e.g., `src/expand/pathname.rs:22` if the glob path still allocates). A shift is acceptable as long as the total dropped.
- If the numbers genuinely did not move, the theory in the spec is wrong — stop and investigate rather than papering over the measurement.

- [ ] **Step 7: Record the numbers**

In a scratch note (terminal output is fine), record:
- Pre-fix total bytes / blocks (from §3.2 of `performance.md`: **13.78 MB / 293,382 blocks**).
- Post-fix total bytes / blocks (from Step 5 output).
- Pre-fix `pathname::expand` rank 1 entry: **2.94 MB / 14,020 calls** (from §3.2).
- Post-fix `pathname::expand` entries (may be absent, may be lower-ranked).
- The new Top-10 table verbatim — needed in Task 5.

No commit for this task — measurements only. The updated `dhat-heap-w2.json` is gitignored.

---

### Task 5: Update performance.md with post-fix numbers

**Files:**
- Modify: `performance.md` §1 (exec summary line), §3.2 (dhat Top-10 by bytes + by calls + intro paragraph), §4.3 (add Fixed header + measured improvement), §5.1 (priority matrix row), §5.2 (promote §4.4 to P0), §5.3 (mark §4.3 completed)

This task is a series of surgical `Edit` tool operations. Each bullet below is one `Edit`.

- [ ] **Step 1: Update §1 Executive Summary — remaining hotspots list**

In `performance.md`, find the line:

```
2. **`expand::pathname::expand` always allocates a new Vec** — 14k calls / 2.94 MB in W2 even though most fields contain no glob metachars. Now the #1 dhat site by bytes (§4.3).
```

Replace it with (using the measured post-fix numbers from Task 4, Step 7 — substitute `<X>` and `<Y>` with actual values; if `pathname::expand` is no longer in the Top-10, phrase as "removed from Top-10"):

```
2. **`expand::pathname::expand` always allocates a new Vec** — ~~14k calls / 2.94 MB in W2~~. **Fixed 2026-04-21** via non-glob fast-path (§4.3); dropped to <X> MB / <Y> calls (or removed from Top-10).
```

- [ ] **Step 2: Update §3.2 dhat intro paragraph — run totals**

Find the line starting `Run totals: **13.78 MB allocated across 293,382 blocks**` and update it to reflect the post-fix numbers. Preserve the format; replace the numbers with the Task 4, Step 7 measurements. Example (use actual numbers):

```
Run totals: **<post-fix total> MB allocated across <post-fix blocks> blocks** (vs pre-fastpath 13.78 MB / 293,382 blocks at `2c36c5e` → −<delta>% bytes, −<delta>% blocks; vs pre-bracket-fix 68.1 MB / 808,896 blocks at `1e1b738` → −<cumulative>% bytes).
```

- [ ] **Step 3: Update §3.2 dhat Top-10 by bytes table**

Replace the Top-10 by bytes table (the one starting `| Rank | Site | Bytes | Calls |` with rank 1 being `pathname::expand 2.94 MB`) with the post-fix table extracted in Task 4, Step 5. Keep the existing table header and formatting.

- [ ] **Step 4: Update §3.2 dhat Top-10 by calls table**

Replace the Top-10 by call count table (the one where rank 1 is `pattern::matches 34,034 calls`) with the post-fix table from Task 4.

- [ ] **Step 5: Update §3.2 the paragraph after the by-bytes table**

Find:

```
`expand::pathname::expand` at 14k calls and `expand::pattern::matches` at ~50k calls surface the primary remaining allocation pressure: pathname expansion runs its globbing machinery on words that have no glob metacharacters. See §4.3.
```

Replace with:

```
After the §4.3 fast-path fix landed 2026-04-21, `expand::pattern::matches` (~50k calls / ~1.25 MB) becomes the top remaining expansion-pipeline allocation source. See §4.4.
```

Also update the **TODO.md cross-check** block that follows to reflect the new top-site:

```
**TODO.md cross-check:** the existing entry "`LINENO` update allocates a `String` per command" is **not** in the W2 Top-10. At HEAD post-fastpath, `pattern::matches` / `field_split::emit` are the largest allocation sites and should be prioritized ahead of `LINENO`.
```

- [ ] **Step 6: Mark §4.3 as fixed**

At the top of §4.3 (`### 4.3. pathname::expand allocates a new Vec per invocation, even with no glob chars`), change the heading to:

```
### 4.3. `pathname::expand` allocates a new Vec per invocation, even with no glob chars (fixed 2026-04-21)
```

Then, immediately below the `**TODO.md cross-check:** not present. **P0** at HEAD — cheapest remaining win after §4.1 landed.` line, append a new **Fix applied** block:

```
**Fix applied (2026-04-21):**

Implemented fix candidate #1 (fast-path pass-through). Added a guard at the top of `pathname::expand`: if no field contains unquoted glob metachars, return the input `Vec` unchanged. See commit `<SHA from Task 2>` and spec `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md`.

**Measured impact (W2, post-fix vs pre-fastpath at `2c36c5e`):**
- `pathname::expand` (rank #1 at `2c36c5e`): 2.94 MB / 14,020 calls → <post-fix values; "removed from Top-10" if gone>.
- W2 total allocation: 13.78 MB → <post-fix MB> (−<delta>%).
- W2 total blocks: 293,382 → <post-fix blocks> (−<delta>%).
```

Substitute `<SHA from Task 2>` with the actual commit hash (run `git log --oneline -1` after Task 2 committed — it's probably easier to capture this hash in Task 4's scratch note).

- [ ] **Step 7: Update §5.1 priority matrix row for §4.3**

Find the row:

```
| 4.3 — `pathname::expand` non-glob alloc | **Medium** (~2.94 MB, now #1 dhat site) | **Low** (5-line fast path) | **P0** | Promoted to P0 now that §4.1 is done — cheapest remaining win. |
```

Replace the priority column and notes:

```
| 4.3 — `pathname::expand` non-glob alloc | **Medium** (~2.94 MB at rank #1 pre-fastpath) | **Low** (5-line fast path) | **done** | Completed 2026-04-21 via non-glob fast-path. See `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md`. |
```

- [ ] **Step 8: Update §5.2 next-project queue**

Replace the entire §5.2 content (everything under the `### 5.2 Next-project queue` header) with:

```
In order (revised after §4.3 fast-path landed 2026-04-21):

**Note (2026-04-21):** §4.3 has landed. Current top allocation sites at HEAD are `pattern::matches` (§4.4) and `field_split::emit`. The function-call ratio (§4.2) remains at ~2.1× — measurable but below the new P0 bar.

1. **P0 — Fix 4.4 `pattern::matches` recompilation.** Estimated: 2 days. Now the top remaining expansion-pipeline allocation source (~1.25 MB / 50k calls).
2. **P1 — Add the fine-grained function-call sub-benches from 4.2 candidate #1**, then act on whatever they reveal (likely the `catch_unwind` replacement). Estimated: half a day for sub-benches, 1–3 days for the follow-up fix.
3. **P2 — Investigate `field_split::emit`** (~2.37 MB across two call sites). Deferred pending a dedicated pass.
```

- [ ] **Step 9: Update §5.3 items-to-add-to-TODO**

Find the line:

```
- `pathname::expand` Vec allocation with no glob chars (§4.3) — **P0** (promoted after HEAD re-measurement; now the #1 dhat site).
```

Replace with:

```
- ~~`pathname::expand` Vec allocation with no glob chars (§4.3)~~ — **Completed 2026-04-21** via non-glob fast-path; dropped from #1 dhat site to <post-fix status>.
```

- [ ] **Step 10: Run a visual sanity pass**

Read `performance.md` top-to-bottom and verify:
- No dangling references to "2.94 MB" or "14,020 calls" that weren't updated.
- No "P0" label still attached to §4.3.
- The §5.2 queue now lists §4.4 as the new P0.

- [ ] **Step 11: Commit**

```bash
git add performance.md
git commit -m "docs(perf): record pathname::expand fast-path outcome in performance.md

Updates §1 exec summary, §3.2 dhat tables and intro, §4.3 fix marker
+ measured improvement, §5.1 priority matrix (§4.3 → done), §5.2
next-project queue (§4.4 → P0), §5.3 TODO items (§4.3 struck).

Spec: docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:**
  - §2 goal → Task 2 (fast-path guard).
  - §3 approach → Task 2 step 1 (exact code).
  - §4 change → Task 2 step 1 (before/after diff).
  - §5 semantic preservation → Task 3 (full suite + E2E regression check).
  - §6.1 existing tests → Task 2 step 2 and step 3 run them.
  - §6.2 new unit test → Task 1.
  - §6.3 integration/E2E → Task 3.
  - §7 verification → Task 4.
  - §8 documentation updates → Task 5.
  - §9 out-of-scope → explicitly not planned for; no tasks added.
- **Placeholder scan:** `<X>`, `<Y>`, `<delta>`, `<SHA from Task 2>`, `<post-fix ...>` appear in Task 5 as substitution markers. These are **not plan placeholders** — they are explicit "paste the value you measured in the previous task here" slots. Each is called out in context with the exact source of the value. Acceptable because the values cannot be known until Task 4 runs.
- **Type consistency:** `expand`, `has_unquoted_glob_chars`, `ExpandedField`, `ShellEnv`, `glob_match` all match the names in `src/expand/pathname.rs`. Test helpers `make_env`, `unquoted`, `quoted_field`, `values` match existing tests in the same file.

---

Plan complete and saved to `docs/superpowers/plans/2026-04-21-pathname-expand-fast-path.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
