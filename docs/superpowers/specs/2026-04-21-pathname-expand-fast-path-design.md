# Design: `pathname::expand` Non-Glob Fast Path

**Date:** 2026-04-21
**Performance reference:** `performance.md` §4.3
**Priority:** P0 (from `performance.md` §5.2)
**Estimated effort:** 1–2 hours

## 1. Background

At HEAD (`2261638`), `yosh::expand::pathname::expand` is the **#1 dhat site by bytes** in workload W2 (`benches/data/script_heavy.sh`):

| Metric | Value |
|---|---|
| Bytes allocated | 2.94 MB |
| Calls | 14,020 |
| Dhat rank | #1 (bytes), #4 (calls) |

The allocation is attributed to line `src/expand/pathname.rs:29:20` — `result.push(field)` inside the per-field loop. The cost is not clones of the fields (which are moved, not cloned); it is the **growth of the output `Vec`** (`Vec::new()` → 4 → 8 → … reallocations).

W2 is representative: almost every field flowing through `pathname::expand` has no unquoted glob metacharacter (`*`, `?`, `[`), so the output Vec is a pure copy of the input Vec. Eliminating that copy for the non-glob case captures nearly all 2.94 MB.

## 2. Goal

Eliminate the output-Vec allocation for invocations of `pathname::expand` where **no** field contains unquoted glob metacharacters, preserving all POSIX 2.6.6 semantics.

**Non-goals:**
- Reducing allocation for mixed glob/non-glob inputs (deferred; not observed in W2).
- Caching compiled patterns (§4.4, separate P2 project).
- Modifying `glob_match` / `glob_path` / `expand_components` / `glob_in_dir` internals.

## 3. Approach

Add an early-return fast path to `pathname::expand`. If `fields.iter().any(has_unquoted_glob_chars)` returns `false`, return the input `Vec` unchanged.

### Why this approach (over alternatives)

Two alternatives from `performance.md` §4.3 were considered:

1. **Fast-path pass-through (chosen)** — single full scan, return input unchanged when no glob present.
2. **Per-field `mem::take` + `Vec::with_capacity`** — pre-size output and avoid clones in mixed case. **Rejected**: the dhat allocation is the output `Vec` itself, not field clones (which do not exist today). `with_capacity` reduces reallocation growth but still allocates the `Vec`; it does not capture the 2.94 MB.
3. **Combined A + B** — adds per-field complexity without measurement-backed benefit. Deferred as YAGNI.

The fast path cost in the mixed case is one extra scan of field bytes — negligible compared to the filesystem-touching glob work that follows.

## 4. Change

**File:** `src/expand/pathname.rs`
**Function:** `expand()` (currently lines 15–33)

```rust
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    if !fields.iter().any(has_unquoted_glob_chars) {
        return fields;
    }
    let mut result = Vec::new();
    for field in fields {
        if has_unquoted_glob_chars(&field) {
            let matches = glob_match(&field.value);
            if matches.is_empty() {
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

Net change: +3 lines (guard + brace). No signature change. No dependency on new symbols — `has_unquoted_glob_chars` is already a private function in the same module.

## 5. Semantic Preservation

The fast path is activated **only** when no field contains an unquoted glob metacharacter. POSIX 2.6.6 specifies that pathname expansion transforms fields that contain unquoted `*`, `?`, or `[`; fields without these metacharacters are passed through unchanged. Therefore:

| Invariant | How preserved |
|---|---|
| Field order | Fast path returns input `Vec` as-is; slow path unchanged |
| Field contents | Same — ownership is moved in both paths, no clones |
| Quoted-metacharacter handling | `has_unquoted_glob_chars` already consults `field.is_quoted(i)`, so `"*.rs"` triggers the fast path |
| Empty input | `any()` on empty iterator returns `false` → fast path returns empty `Vec` (equivalent to slow path's empty result) |
| Dotfile / `/` rules (POSIX 2.6.6 rule 4/5) | Unchanged — implemented in slow path only, which is unaffected |

## 6. Testing

### 6.1 Existing tests (must continue to pass)

All 7 tests currently in `src/expand/pathname.rs` mod tests:
- `test_no_glob_passthrough` — exercises fast path (single non-glob field).
- `test_quoted_glob_not_expanded` — exercises fast path (quoted metacharacter, `has_unquoted_glob_chars` returns false).
- `test_glob_src_files` — exercises slow path (unquoted `*.rs`).
- `test_no_match_keeps_pattern` — exercises slow path (glob present but no match).
- `test_star_does_not_match_dotfiles` — calls `glob_in_dir` directly; not affected.
- `test_has_unquoted_glob_chars_true` / `_false_quoted` / `_false_no_meta` — test `has_unquoted_glob_chars` directly; not affected.

### 6.2 New unit test

```rust
#[test]
fn test_fast_path_preserves_multiple_non_glob_fields() {
    let env = make_env();
    let input = vec![unquoted("hello"), unquoted("world"), quoted_field("*.rs")];
    let result = expand(&env, input);
    assert_eq!(values(result), vec!["hello", "world", "*.rs"]);
}
```

Rationale: covers the multi-field fast-path case (the dominant W2 case) which no existing test exercises directly.

### 6.3 Integration / E2E

- `cargo test` full suite — no regressions.
- `./e2e/run_tests.sh` — no regressions, with particular attention to POSIX 2.6.6 tests.

## 7. Verification (B scope — from brainstorming)

Measurement to confirm the 2.94 MB reduction:

```bash
cargo build --profile profiling --bin yosh-dhat --features dhat-heap
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
mv dhat-heap.json target/perf/dhat-heap-w2.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 10
```

**Success criteria:**
- `src/expand/pathname.rs:29:20` either disappears from the W2 dhat Top-10 by bytes, or its byte total drops by ≥90%.
- W2 total allocation drops from ~13.78 MB to roughly ~10.8 MB (−2.94 MB, ±0.3 MB tolerance for run-to-run variance).

Criterion benchmarks are **not** re-run as part of this project — the §3.2 Criterion table is noise-dominated at the 2× level, and the fast path's CPU impact is below that noise floor.

## 8. Documentation updates

- **`performance.md`:**
  - §3.2 dhat Top-10 table — replace with post-fix snapshot.
  - §4.3 — add "Fixed 2026-04-21" header, record measured improvement.
  - §5.1 priority matrix — mark §4.3 row as "done".
  - §5.2 next-project queue — promote §4.4 to P0 (current P0 is §4.3).
  - §5.3 items-to-add-to-TODO — strike §4.3 as completed.
- **`TODO.md`:** no new entry (tracked in `performance.md` §5).

## 9. Out of scope

- §4.2 function-call residual 2.1× overhead (P1, separate project, needs sub-benches first).
- §4.4 `pattern::matches` recompilation (P2, separate project).
- Mixed glob/non-glob optimization (deferred, YAGNI).
