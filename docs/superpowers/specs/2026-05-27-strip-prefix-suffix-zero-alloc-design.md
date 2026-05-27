# `strip_prefix` / `strip_suffix` Layer-2 zero-allocation rewrite

**Date:** 2026-05-27
**Status:** Design approved, pending implementation plan
**Workload driver:** W2 (`benches/data/script_heavy.sh`) Section C
**Predecessor:** `2026-05-27-pattern-matches-zero-alloc-design.md` (Layer-1;
landed in commits `d276e7a..4f7627a`). This is the §7 "Layer-2 amplifier"
follow-up recorded there.

## 1. Goal

Eliminate the per-cut-point `String` allocations inside
`src/expand/param.rs::strip_prefix` / `strip_suffix`, which implement
`${VAR#pat}` / `${VAR##pat}` / `${VAR%pat}` / `${VAR%%pat}`. With
`pattern::matches` now `&str`-based (Layer-1), the matcher can be fed
sub-slices of `value` directly, leaving exactly **one** allocation per
operation (the result `String`).

Char-level matching semantics are preserved exactly. No new dependency.
`matches`' public signature and the four call sites in `param.rs` are
unchanged.

## 2. Current implementation (baseline)

```rust
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    let chars: Vec<char> = value.chars().collect();   // one Vec<char>
    let n = chars.len();
    if longest {
        for start in 0..=n {
            let suffix: String = chars[start..].iter().collect();   // O(n) String / cut
            if pattern::matches(pat, &suffix) {
                return chars[..start].iter().collect();             // result String
            }
        }
    } else {
        for start in (0..=n).rev() { /* symmetric */ }
    }
    value.to_string()
}
// strip_prefix is the mirror (builds prefix slices instead of suffix).
```

Cost per operation: **1 `Vec<char>` + O(n) `String` builds (each O(n)) +
1 result `String`** → O(n) allocations, O(n²) bytes copied.

### Why this matters (measurement context)

Per the Layer-1 design §2/§4.4, W2 Section C runs these strip operators
200× and funnels into `matches`. Layer-1 removed `matches`' own per-call
`Vec<char>`; this Layer-2 change removes the upstream `String` builds that
inflate the call count and byte volume. dhat re-measurement (§5) quantifies
the residual reduction.

## 3. Design (Approach B — zero-`Vec` iterator)

### 3.1 Char-boundary helper

```rust
/// All char-boundary byte offsets of `v`, ascending: 0, b1, …, v.len().
/// DoubleEnded so callers pick longest-first (rev) or shortest-first.
fn boundaries(v: &str) -> impl DoubleEndedIterator<Item = usize> + '_ {
    v.char_indices().map(|(i, _)| i).chain(std::iter::once(v.len()))
}
```

`char_indices()`, `Map`, and `Once` are each `DoubleEndedIterator`, so
`Chain<Map<CharIndices>, Once>` is too — `.rev()` is valid without
collecting. For an empty string the iterator yields just `[0]`.

### 3.2 Rewritten functions (signatures unchanged)

```rust
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    // matches() is anchored (full match), so test each candidate prefix slice.
    let cut = |end: usize| pattern::matches(pat, &value[..end])
        .then(|| value[end..].to_string());
    let found = if longest {
        boundaries(value).rev().find_map(cut)   // largest end first
    } else {
        boundaries(value).find_map(cut)          // smallest end first
    };
    found.unwrap_or_else(|| value.to_string())   // no match → unchanged
}

fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    let cut = |start: usize| pattern::matches(pat, &value[start..])
        .then(|| value[..start].to_string());
    let found = if longest {
        boundaries(value).find_map(cut)          // smallest start = longest suffix
    } else {
        boundaries(value).rev().find_map(cut)    // largest start = shortest suffix
    };
    found.unwrap_or_else(|| value.to_string())
}
```

Allocations per operation: **1** (the result `String`, built lazily via
`bool::then` only on a match, or `value.to_string()` on no match).

### 3.3 Operator → (side, longest) mapping (unchanged dispatch)

| Param expr | Call | Order over boundaries |
|------------|------|-----------------------|
| `${v#pat}`  `StripShortPrefix` | `strip_prefix(.., false)` | ascending end (smallest prefix first) |
| `${v##pat}` `StripLongPrefix`  | `strip_prefix(.., true)`  | `.rev()` end (largest prefix first) |
| `${v%pat}`  `StripShortSuffix` | `strip_suffix(.., false)` | `.rev()` start (smallest suffix first) |
| `${v%%pat}` `StripLongSuffix`  | `strip_suffix(.., true)`  | ascending start (largest suffix first) |

## 4. Correctness invariants

- **Char-boundary safety.** Every slice index is a `char_indices` boundary
  or `value.len()`; a multibyte char is never split. No raw byte index is
  used. (This is the one new failure mode the rewrite introduces — guarded
  by the multibyte tests in §5.)
- **Anchored full-match.** `matches(pat, s)` returns true only when `pat`
  matches all of `s` (base case `None => s.is_empty()`). Testing each
  candidate sub-slice for a full match reproduces the current behavior.
- **No-match fallback.** `find_map` yields `None` → `value.to_string()`,
  matching the current `value.to_string()` tail.
- **Empty value.** `boundaries("")` = `[0]`; the single offset 0 tests the
  empty prefix/suffix, identical to the current `n = 0` loop.
- **Empty pattern** (`${v#}`): `matches("", "")` is true, so the empty
  prefix/suffix is removed (no change to value) at the appropriate order
  endpoint — same as today.
- **Implementation note.** Bind the `if/else` to `let found = …` *before*
  calling `.unwrap_or_else`, so the fallback applies to both branches (not
  just `else`). `bool::then(|| …)` defers the result-`String` build to the
  matching iteration.

## 5. Testing (TDD)

The existing `test_strip_*` unit tests in `param.rs` and the e2e
parameter-expansion suite are the behavioral spec and must pass
**unchanged** — the primary regression guard.

The byte-offset slicing introduces a multibyte-split failure mode, and the
current strip tests are ASCII-only. Add multibyte tests **first** (they
must pass on the rewrite; a raw-byte-index bug would panic):

- `${V%.txt}` on `"日本語.txt"` → `"日本語"` (ASCII pat × multibyte value)
- `${V#日}` on `"日本語"` → `"本語"` (`StripShortPrefix`)
- `${V%語}` on `"日本語"` → `"日本"` (`StripShortSuffix`)
- `${V##*う}` on `"あいうえお"` → `"えお"` (longest prefix, `*` + multibyte)
- `${V%%*}` on a multibyte value → `""` (longest suffix consumes all)
- `${V#?}` on `"あい"` → `"い"` (single multibyte char for `?`)

### Verification gates (full)

1. `cargo test` (unit incl. new multibyte strip tests + integration) green.
2. `./e2e/run_tests.sh --filter=2_06_02` (parameter-expansion suite) green.
3. **W2 bit-identical.** Capture baseline stdout/stderr of
   `target/profiling/yosh-dhat benches/data/script_heavy.sh` *before* the
   change; `diff` against the post-change output is empty.
4. **dhat re-measure** (Layer-1 §2 procedure on W2): the
   `param.rs` per-cut `String` allocation sites disappear; total blocks
   drop. Record before/after top-N.
5. **Criterion** `exec_param_expansion_200` before/after (improvement or
   no regression). Note: the existing bench uses an 11-char value, so the
   wall-clock delta is modest; the clearer quantitative signal is the dhat
   block-count drop in gate 4. A longer-string strip micro-bench is
   *optional*; the existing bench baseline is kept for comparison.

## 6. Success criteria

- `strip_prefix` / `strip_suffix` allocate exactly one `String` per call;
  no `Vec` and no per-cut `String` builds remain.
- All unit, integration, and e2e tests pass; W2 stdout/stderr bit-identical
  to the pre-change baseline.
- dhat W2 shows the `param.rs` strip allocation sites removed and a lower
  block count; `exec_param_expansion_200` shows no regression.
- No new dependency; `matches` signature and the four `param.rs` call sites
  unchanged.

## 7. Scope boundaries

**In scope:** rewrite `strip_prefix` / `strip_suffix` to Approach B; add the
`boundaries` helper; add multibyte strip tests.

**Out of scope (follow-ups / rejected):**

- **Pattern re-parse per call.** `matches` re-walks `pat` on every candidate
  (O(n) parses per strip op, including a fresh `Vec<BracketItem>` per
  bracket). A compiled-pattern AST matched many times is a larger,
  separate redesign; not addressed here.
- **Anchored single-pass matcher.** The brute-force cut-point scan is
  inherently O(n²) character comparisons. Replacing it with a left/right
  anchored matcher is a separate algorithmic change.
- **Returning `&str` to defer the result allocation.** The expansion result
  is owned up the call chain, so one `String` is unavoidable; changing the
  signature only relocates it. Rejected.

## 8. Risk & mitigation

| Risk | Mitigation |
|------|-----------|
| Raw byte index splits a multibyte char → panic | All slice offsets from `char_indices` / `len()`; multibyte tests added first |
| `.unwrap_or_else` binds to `else` branch only | Bind `if/else` to `let found` before the fallback (§4 note) |
| Wrong longest/shortest ordering per operator | §3.3 mapping table + existing `test_strip_*` regression tests |
| Behavioral drift vs Layer-1 baseline | W2 bit-identical diff gate (gate 3) |

## 9. References

- Layer-1 design: `docs/superpowers/specs/2026-05-27-pattern-matches-zero-alloc-design.md` (§7 records this follow-up)
- `src/expand/param.rs` (target: `strip_prefix`, `strip_suffix`, lines ~182-236)
- `src/expand/pattern.rs::matches` (`&str`-based consumer, unchanged)
- `benches/exec_bench.rs::exec_param_expansion_200`, `benches/data/script_heavy.sh` Section C
- dhat profile: `target/perf/dhat-heap-w2.json` (regenerate per Layer-1 §2)
