# strip_prefix / strip_suffix Layer-2 zero-allocation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite `strip_prefix` / `strip_suffix` in `src/expand/param.rs` to slice `value` directly via char-boundary byte offsets, eliminating the per-cut-point `String` builds so each `${VAR#}/##/%/%%` operation allocates exactly one `String` (the result).

**Architecture:** A behavior-preserving perf refactor. `pattern::matches` is already `&str`-based (Layer-1, commits `d276e7a..4f7627a`). Add a `boundaries()` `DoubleEndedIterator` helper yielding all char-boundary byte offsets, then drive `find_map` over it in longest-first (`rev()`) or shortest-first order, feeding `matches` sub-slices of `value`. No public signature changes; no new dependency.

**Tech Stack:** Rust (edition 2024), `cargo test`, `./e2e/run_tests.sh`, Criterion (`cargo bench`), dhat (`yosh-dhat` profiling binary).

**Spec:** `docs/superpowers/specs/2026-05-27-strip-prefix-suffix-zero-alloc-design.md`

---

## File Structure

- **Modify:** `src/expand/param.rs`
  - Add private `boundaries(v: &str) -> impl DoubleEndedIterator<Item = usize> + '_` helper.
  - Replace the bodies of `strip_suffix` (currently ~`184-208`) and `strip_prefix` (currently ~`212-236`). Signatures `(&str, &str, bool) -> String` are unchanged.
  - Add multibyte regression tests in the existing `#[cfg(test)] mod tests` block.
- **Modify (final task):** `TODO.md` — delete the completed Layer-2 follow-up item.
- **Modify (final task):** `docs/superpowers/specs/2026-05-27-strip-prefix-suffix-zero-alloc-design.md` — append a `## 10. Results (measured)` section.

No other files change. The four call sites (`param.rs:126-151`) and `pattern::matches` are untouched.

---

## Task 1: Capture pre-change baselines

No code change; captures the "before" artifacts the verification gates compare against. Must run on the **current** (pre-refactor) tree.

**Files:** none (writes artifacts under `target/perf/`).

- [ ] **Step 1: Ensure the perf artifact dir exists**

Run:
```bash
mkdir -p target/perf
```

- [ ] **Step 2: Capture the W2 output baseline with the regular binary (bit-identical gate)**

Run:
```bash
cargo build
./target/debug/yosh benches/data/script_heavy.sh > target/perf/w2-stdout-before.txt 2> target/perf/w2-stderr-before.txt
wc -l target/perf/w2-stdout-before.txt target/perf/w2-stderr-before.txt
```
Expected: build succeeds; both files written (non-error). These are the byte-for-byte reference for Task 4.

- [ ] **Step 3: Capture the dhat allocation baseline**

Run:
```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
./target/profiling/yosh-dhat benches/data/script_heavy.sh > /dev/null 2>&1
mv dhat-heap.json target/perf/dhat-heap-w2-before.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-before.json 15 | tee target/perf/dhat-top-before.txt
```
Expected: the Top-N table prints and is saved. Note the **total allocated bytes / blocks** line and any `src/expand/param.rs` entries (the strip `String` builds) — these are what Task 4 shows shrinking.

- [ ] **Step 4: Capture the Criterion baseline for the param-expansion bench**

Run:
```bash
cargo bench --bench exec_bench -- --save-baseline before exec_param_expansion_200
```
Expected: Criterion runs `exec_param_expansion_200` and saves a baseline named `before` under `target/criterion/`.

---

## Task 2: Add multibyte strip regression tests (refactor safety net)

These are characterization tests for a behavior-preserving refactor. They **pass on the current `Vec<char>` implementation** (confirming the expected values) and must still pass after the rewrite — a raw-byte-index bug in the rewrite would split a multibyte char and **panic**, which these catch.

**Files:**
- Modify/Test: `src/expand/param.rs` (the `#[cfg(test)] mod tests` block, after the existing `test_strip_short_prefix` at ~line 434)

- [ ] **Step 1: Add the multibyte tests**

Insert this block immediately after the existing `test_strip_short_prefix` test (after its closing `}` near line 434, before the `// ── Length` comment):

```rust
    // ── Multibyte boundary safety (added for the Layer-2 &str rewrite) ──
    #[test]
    fn test_strip_short_suffix_multibyte_ascii_pat() {
        let mut env = make_env();
        env.vars.set("V", "日本語.txt").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix("V".to_string(), Word::literal(".txt")),
        )
        .unwrap();
        assert_eq!(result, "日本語");
    }

    #[test]
    fn test_strip_short_prefix_multibyte_literal() {
        let mut env = make_env();
        env.vars.set("V", "日本語").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortPrefix("V".to_string(), Word::literal("日")),
        )
        .unwrap();
        assert_eq!(result, "本語");
    }

    #[test]
    fn test_strip_short_suffix_multibyte_literal() {
        let mut env = make_env();
        env.vars.set("V", "日本語").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix("V".to_string(), Word::literal("語")),
        )
        .unwrap();
        assert_eq!(result, "日本");
    }

    #[test]
    fn test_strip_long_prefix_multibyte_star() {
        let mut env = make_env();
        env.vars.set("V", "あいうえお").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripLongPrefix("V".to_string(), Word::literal("*う")),
        )
        .unwrap();
        assert_eq!(result, "えお");
    }

    #[test]
    fn test_strip_long_suffix_multibyte_star_all() {
        let mut env = make_env();
        env.vars.set("V", "日本語").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripLongSuffix("V".to_string(), Word::literal("*")),
        )
        .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_short_prefix_multibyte_question() {
        let mut env = make_env();
        env.vars.set("V", "あい").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortPrefix("V".to_string(), Word::literal("?")),
        )
        .unwrap();
        assert_eq!(result, "い");
    }
```

- [ ] **Step 2: Run the new tests against the current implementation**

Run:
```bash
cargo test -p yosh --lib expand::param::tests::test_strip
```
Expected: PASS — all `test_strip_*` tests (existing + 6 new) green. This confirms the expected values are correct and the current `Vec<char>` impl handles multibyte, establishing the baseline behavior the refactor must preserve.

- [ ] **Step 3: Commit**

```bash
git add src/expand/param.rs
git commit -m "test(expand): add multibyte strip_prefix/suffix regression tests

Characterization tests for the upcoming Layer-2 &str rewrite. Cover
multibyte values/patterns across #/##/%/%% so a byte-offset bug that
splits a multibyte char is caught (panic) rather than silently passing.

Task: design+plan the strip_prefix/suffix Layer-2 zero-alloc follow-up.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Rewrite strip_prefix / strip_suffix (Approach B)

**Files:**
- Modify: `src/expand/param.rs` (lines ~182-236 — the two functions and their doc comments)

- [ ] **Step 1: Replace the two functions with the `boundaries` helper + Approach-B bodies**

Find this exact current block (the two functions, ~lines 182-236):

```rust
/// Remove a suffix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();

    if longest {
        // Try from index 0 upward (largest possible suffix = whole string)
        for start in 0..=n {
            let suffix: String = chars[start..].iter().collect();
            if pattern::matches(pat, &suffix) {
                let prefix: String = chars[..start].iter().collect();
                return prefix;
            }
        }
    } else {
        // Try from index n downward (smallest possible suffix)
        for start in (0..=n).rev() {
            let suffix: String = chars[start..].iter().collect();
            if pattern::matches(pat, &suffix) {
                let prefix: String = chars[..start].iter().collect();
                return prefix;
            }
        }
    }
    value.to_string()
}

/// Remove a prefix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();

    if longest {
        // Try from n down to 0 (largest prefix first)
        for end in (0..=n).rev() {
            let prefix: String = chars[..end].iter().collect();
            if pattern::matches(pat, &prefix) {
                let suffix: String = chars[end..].iter().collect();
                return suffix;
            }
        }
    } else {
        // Try from 0 upward (smallest prefix first)
        for end in 0..=n {
            let prefix: String = chars[..end].iter().collect();
            if pattern::matches(pat, &prefix) {
                let suffix: String = chars[end..].iter().collect();
                return suffix;
            }
        }
    }
    value.to_string()
}
```

Replace it entirely with:

```rust
/// All char-boundary byte offsets of `v`, ascending: `0, b1, …, v.len()`.
///
/// `DoubleEndedIterator` so callers iterate longest-first via `rev()` or
/// shortest-first forward. For an empty string this yields just `[0]`.
fn boundaries(v: &str) -> impl DoubleEndedIterator<Item = usize> + '_ {
    v.char_indices().map(|(i, _)| i).chain(std::iter::once(v.len()))
}

/// Remove a suffix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    // `matches` is anchored (full match), so test each candidate suffix slice.
    // `start` is a char-boundary byte offset; the suffix is `value[start..]`.
    let cut = |start: usize| pattern::matches(pat, &value[start..]).then(|| value[..start].to_string());
    let found = if longest {
        // smallest start = longest suffix first
        boundaries(value).find_map(cut)
    } else {
        // largest start = shortest suffix first
        boundaries(value).rev().find_map(cut)
    };
    found.unwrap_or_else(|| value.to_string())
}

/// Remove a prefix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    // `matches` is anchored (full match), so test each candidate prefix slice.
    // `end` is a char-boundary byte offset; the prefix is `value[..end]`.
    let cut = |end: usize| pattern::matches(pat, &value[..end]).then(|| value[end..].to_string());
    let found = if longest {
        // largest end = longest prefix first
        boundaries(value).rev().find_map(cut)
    } else {
        // smallest end = shortest prefix first
        boundaries(value).find_map(cut)
    };
    found.unwrap_or_else(|| value.to_string())
}
```

- [ ] **Step 2: Format the file**

Run:
```bash
rustfmt --edition 2024 src/expand/param.rs
```
(Use `rustfmt --edition 2024 <path>` directly — `cargo fmt --check -- <path>` misreads the edition for let-chain/long-line wrapping per TODO.md.)
Expected: no error; the long `cut` closure line may wrap. This is cosmetic.

- [ ] **Step 3: Run the strip tests (existing + multibyte)**

Run:
```bash
cargo test -p yosh --lib expand::param::tests::test_strip
```
Expected: PASS — all `test_strip_*` tests green, including the 6 multibyte tests from Task 2. A panic here means a byte offset split a multibyte char (re-check that every slice index comes from `boundaries`).

- [ ] **Step 4: Run the full library + integration test suite**

Run:
```bash
cargo test
```
Expected: PASS — no regressions. (Per project notes: do NOT use `--workspace`; it host-builds the wasm crates and fails.)

- [ ] **Step 5: Commit**

```bash
git add src/expand/param.rs
git commit -m "perf(expand): rewrite strip_prefix/suffix to slice &str directly

Layer-2 follow-up to the pattern::matches &str rewrite. strip_prefix /
strip_suffix built an O(n) String per cut point (plus a Vec<char>) and
called the matcher O(n) times. Now a boundaries() DoubleEnded iterator
yields char-boundary byte offsets and find_map slices value directly,
leaving exactly one allocation per op (the result String). Behavior is
unchanged; matches() signature and the four call sites are untouched.

Task: implement the strip_prefix/suffix Layer-2 zero-alloc follow-up.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Full verification + record results + close the TODO item

**Files:**
- Modify: `docs/superpowers/specs/2026-05-27-strip-prefix-suffix-zero-alloc-design.md`
- Modify: `TODO.md`

- [ ] **Step 1: e2e parameter-expansion suite (gate 2)**

Run:
```bash
cargo build
./e2e/run_tests.sh --filter=2_06_02_parameter_expansion
```
Expected: all parameter-expansion e2e tests pass.

- [ ] **Step 2: W2 bit-identical diff (gate 3)**

Run:
```bash
./target/debug/yosh benches/data/script_heavy.sh > target/perf/w2-stdout-after.txt 2> target/perf/w2-stderr-after.txt
diff target/perf/w2-stdout-before.txt target/perf/w2-stdout-after.txt && echo "STDOUT IDENTICAL"
diff target/perf/w2-stderr-before.txt target/perf/w2-stderr-after.txt && echo "STDERR IDENTICAL"
```
Expected: both diffs empty; prints `STDOUT IDENTICAL` and `STDERR IDENTICAL`. Any diff means the refactor changed behavior — stop and investigate.

- [ ] **Step 3: dhat re-measure (gate 4)**

Run:
```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
./target/profiling/yosh-dhat benches/data/script_heavy.sh > /dev/null 2>&1
mv dhat-heap.json target/perf/dhat-heap-w2-after.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-after.json 15 | tee target/perf/dhat-top-after.txt
diff target/perf/dhat-top-before.txt target/perf/dhat-top-after.txt || true
```
Expected: the `src/expand/param.rs` strip `String` allocation sites present in `dhat-top-before.txt` are **gone** from `dhat-top-after.txt`; total allocated blocks are lower. Record the before/after **total blocks** numbers for Step 5.

- [ ] **Step 4: Criterion compare (gate 5)**

Run:
```bash
cargo bench --bench exec_bench -- --baseline before exec_param_expansion_200
```
Expected: `exec_param_expansion_200` shows improvement or no regression vs the `before` baseline. (The bench value is only 11 chars, so the wall-clock delta is small/noisy; the dhat block-count drop from Step 3 is the primary signal. Record the median change.)

- [ ] **Step 5: Record measured results in the design spec**

Append this section to the end of `docs/superpowers/specs/2026-05-27-strip-prefix-suffix-zero-alloc-design.md`, filling the bracketed numbers from Steps 3-4:

```markdown

## 10. Results (measured)

**Date:** 2026-05-27 — implemented in commit `[short-sha of Task 3]`.

- **dhat W2 (gate 4):** total blocks `[before]` → `[after]`
  (`[delta]`). The `src/expand/param.rs` strip `String` allocation sites
  (`chars[..].iter().collect()` per cut point) are removed from the Top-15.
- **Criterion `exec_param_expansion_200` (gate 5):** median `[before ns]`
  → `[after ns]` (`[±%]`; within noise / improvement).
- **W2 bit-identical (gate 3):** stdout and stderr byte-for-byte identical
  to the pre-change baseline.
- **Tests:** full `cargo test` green; e2e `2_06_02_parameter_expansion`
  green; 6 multibyte strip tests green.

Allocation per strip operation is now exactly one `String` (the result).
```

- [ ] **Step 6: Delete the completed Layer-2 follow-up from TODO.md**

Remove this exact item from `TODO.md` (under `## Future: Code Quality Improvements`):

```markdown
- [ ] `strip_prefix` / `strip_suffix` Layer-2 allocation amplifier (`src/expand/param.rs:184-236`) — `${VAR#pat}` / `%` / `##` / `%%` build a fresh `String` for every cut point and call `pattern::matches` O(n) times per operation. Now that `match_pat` is `&str`-based (2026-05-27), a follow-up can pass `&str` sub-slices directly and skip the intermediate `String` builds, removing the residual allocation these sites still contribute. Separate perf target; measured in `docs/superpowers/specs/2026-05-27-pattern-matches-zero-alloc-design.md` §7 and `performance.md` §5.3 (P2).
```

(Per CLAUDE.md / TODO.md convention: delete completed items, do not mark `[x]`.)

- [ ] **Step 7: Commit**

```bash
git add docs/superpowers/specs/2026-05-27-strip-prefix-suffix-zero-alloc-design.md TODO.md
git commit -m "docs(perf): record strip_prefix/suffix Layer-2 result; close TODO

Records the measured dhat W2 block-count drop and Criterion delta in the
design spec §10, and removes the completed Layer-2 amplifier follow-up
from TODO.md.

Task: implement the strip_prefix/suffix Layer-2 zero-alloc follow-up.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Spec §3.1 `boundaries` helper → Task 3 Step 1. ✓
- Spec §3.2 rewritten `strip_prefix`/`strip_suffix` → Task 3 Step 1. ✓
- Spec §3.3 operator→order mapping → encoded in the `if longest { rev }` arms (Task 3) + guarded by existing `test_strip_*`. ✓
- Spec §4 invariants (char-boundary, anchored, no-match, empty value/pattern, `let found` precedence) → realized in Task 3 code; multibyte safety verified Task 2/3. ✓
- Spec §5 testing (existing unchanged + multibyte first) → Task 2 (added first, green on old impl), re-run Task 3 Step 3. ✓
- Spec §5 gates 1-5 → Task 3 Steps 3-4 (gate 1) + Task 4 Steps 1-4 (gates 2-5). ✓
- Spec §6 success criteria → verified across Task 3/4. ✓
- Spec §7 out-of-scope (re-parse, anchored matcher, `&str` return) → not touched. ✓

**Placeholder scan:** Bracketed values in Task 4 Step 5 are measurement outputs to fill from the immediately-preceding commands (Steps 3-4), not unspecified work — every other step has concrete code/commands. ✓

**Type consistency:** `boundaries` returns `impl DoubleEndedIterator<Item = usize>`; both `cut` closures take a single `usize` and return `Option<String>` via `bool::then`; `find_map` yields `Option<String>`; `unwrap_or_else` returns `String` matching the `(&str,&str,bool) -> String` signatures. `Word::literal(&str)`, `ParamExpr::StripShort/LongPrefix/Suffix(String, Word)`, `env.vars.set`, `expand(&mut env, &ParamExpr)` all match the existing test idioms in `param.rs`. ✓
