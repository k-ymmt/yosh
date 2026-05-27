# `pattern::matches` zero-allocation rewrite (`&str`-based matcher)

**Date:** 2026-05-27
**Status:** Design approved, pending implementation plan
**Workload driver:** W2 (`benches/data/script_heavy.sh`)
**Measurement basis:** dhat heap profile re-captured at HEAD on 2026-05-27 (see §2)

## 1. Goal

Eliminate the two per-call `Vec<char>` allocations inside
`src/expand/pattern.rs::matches` by rewriting the recursive matcher to
operate directly on `&str` via char-boundary byte offsets. Char-level
matching semantics are preserved exactly. No new dependency is added.

This is a single-target, measurement-driven optimization selected by
re-measuring the W2 allocation profile at HEAD.

## 2. Measurement (HEAD, 2026-05-27)

Re-measured because `performance.md` was authored 2026-04-21 and ~5 weeks
of intervening commits (ulimit, readonly, export, etc.) could have shifted
the hotspot ranking. Reproduce with:

```sh
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
./target/profiling/yosh-dhat benches/data/script_heavy.sh > /dev/null 2>&1
mv dhat-heap.json target/perf/dhat-heap-w2.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 15
```

| Metric | Value |
|--------|-------|
| W2 total allocated | 8.34 MB / 264,143 blocks |
| `pattern::matches` aggregate | **~1.66 MB / ~66,000 calls** (~20% of bytes, ~25% of calls) |
| Rank #1 by bytes **and** calls | `pattern.rs:11` — `string.chars().collect()` (`s`): 1.28 MB / 43,043 calls |
| Other sites | `pattern.rs:10` — `pattern.chars().collect()` (`pat`): 312.8 KB / 20,020 + 65.6 KB / 2,800 calls |

The measurement **corrects** `performance.md` §4.4, which proposed an LRU
cache of compiled patterns. The dominant site is the *subject string* `s`
(line 11, 1.28 MB), which is the data being matched and varies on every
call — a pattern cache would only address the smaller `pat` site (line 10).
The correct fix targets the per-call `Vec<char>` materialization itself.

### Call-explosion context (why ~66k calls)

Two call paths feed the hotspot:

1. **Prefix/suffix removal** — `strip_prefix` / `strip_suffix`
   (`src/expand/param.rs:184-236`) implement `${VAR#pat}`, `${VAR%pat}`,
   `##`, `%%`. Each builds a fresh `String` for every cut point and calls
   `matches` O(n) times per operation. W2 Section C runs these 200×.
2. **Pathname globbing** — `glob_in_dir` (`src/expand/pathname.rs:181`)
   calls `matches` once per directory entry (25,025 calls in W2).

Both paths funnel into the same `matches` per-call allocation, so fixing
`matches` itself improves both uniformly. The Layer-2 amplification in
`strip_prefix` / `strip_suffix` (O(n) `String` builds) is **out of scope**
for this target — recorded as a follow-up in §7.

## 3. Current implementation (baseline)

```rust
pub fn matches(pattern: &str, string: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();   // line 10 — allocates
    let s: Vec<char> = string.chars().collect();      // line 11 — allocates (#1)
    match_pat(&pat, &s)
}

fn match_pat(pat: &[char], s: &[char]) -> bool { /* recursive, slices &[char] */ }
fn parse_bracket(pat: &[char], ch: Option<char>) -> Option<(usize, bool)> { /* index arithmetic */ }
fn try_parse_posix_class(pat: &[char]) -> Option<(usize, PosixClass)> { /* ... */ }
```

`match_pat` relies on cheap O(1) slicing (`&pat[1..]`, `&s[i..]`) and
random indexing (`s[0]`, `pat[i+2]`), which the `Vec<char>` provides at the
cost of the upfront allocation.

## 4. Design

### 4.1 `matches` (public API — signature unchanged)

```rust
pub fn matches(pattern: &str, string: &str) -> bool {
    match_pat(pattern, string)
}
```

Public signature `(&str, &str) -> bool` is preserved; only the body and the
private helpers change. No caller in `param.rs` / `pathname.rs` changes.

### 4.2 `match_pat(pat: &str, s: &str) -> bool`

Rewrite each arm to operate on `&str`:

- **First pattern char:** `pat.chars().next()`.
- **Advance pattern by one char:** `&pat[c.len_utf8()..]`.
- **Advance string by one char:** `&s[s_first.len_utf8()..]` where
  `s_first = s.chars().next()`.
- **`*` (star) — every suffix of `s`:** iterate `s.char_indices()` to get
  the byte offset of each suffix start, plus the trailing empty suffix
  (`&s[s.len()..]`). For each, try `match_pat(rest, suffix)`. Equivalent to
  the current `for i in 0..=s.len()` over char positions.
- **`?`:** non-empty `s`, recurse on `(&pat[1..], s_advanced_one_char)`.
- **`\\`:** escaped literal — compare the escaped pattern char to the first
  string char; trailing backslash matches a literal backslash (unchanged
  semantics).
- **`[`:** delegate to `parse_bracket`.
- **literal char:** compare first chars of `pat` and `s`.

All indices land on char boundaries via `len_utf8()` / `char_indices`.
**Never** index `&str` with a raw byte offset that could split a char.

### 4.3 `parse_bracket(pat: &str, ch: Option<char>) -> Option<(usize, bool)>`

(Highest-risk component.) Walk `pat` with a char cursor instead of
`&[char]` index arithmetic. The returned `consumed` becomes a **byte
length** (so the caller advances `&pat[1 + consumed..]`). Preserve every
existing rule:

- leading `!` negation;
- `]` as a literal when it is the first member;
- POSIX `[:class:]` via `try_parse_posix_class`;
- ranges `x-y` (only when `-` is followed by a non-`]` char);
- malformed bracket (no closing `]`) → `None`, caller treats `[` as literal.

`BracketItem` and `PosixClass` are unchanged — they already match on
`char` and remain so (preserving codepoint-range and LC_CTYPE=C semantics).

### 4.4 `try_parse_posix_class(pat: &str) -> Option<(usize, PosixClass)>`

Scan for `:]` with a char cursor; return `consumed` as a byte length
covering the class name plus the trailing `:]`.

## 5. Testing (TDD)

The existing 50+ unit tests in `pattern.rs` are the behavioral spec and
must all pass **unchanged** — they are the primary regression guard.

The current tests are **ASCII-only**. The byte-offset rewrite introduces a
new failure mode (splitting a multibyte char), so add multibyte tests
**first** (they must pass on the rewrite, and would catch a raw-byte-index
bug with a panic):

- `matches("日*", "日本語")` → true
- `matches("*語", "日本語")` → true
- `matches("?", "あ")` → true; `matches("?", "あい")` → false
- `matches("[あ-ん]", "か")` → true; `matches("[あ-ん]", "ン")` → false
- `matches("a?c", "aあc")` → true (single multibyte char for `?`)
- multibyte char following a bracket expression, e.g. `matches("[0-9]語", "5語")`
- trailing backslash before a multibyte string char

### Verification gates

1. `cargo test` (unit + integration) — all green.
2. `./e2e/run_tests.sh` — glob, pathname, and parameter-expansion suites green.
3. **W2 output bit-identical:** `diff <(yosh script_heavy.sh) <(baseline)` empty.
4. **Re-measure dhat W2:** `pattern::matches` drops out of the Top-10;
   W2 total bytes fall ~1.5–1.7 MB (~20%).
5. **Re-measure Criterion:** `exec_param_expansion_200` and the `expand_*`
   benches before/after (expect improvement, no regression elsewhere).

## 6. Success criteria

- `pattern::matches` allocation sites removed from the W2 dhat Top-10;
  W2 total allocated bytes reduced by ~1.5–1.7 MB (~20%).
- All unit, integration, and e2e tests pass; W2 stdout/stderr bit-identical
  to the pre-change baseline.
- No new dependency added; `matches` public signature unchanged.

## 7. Scope boundaries

**In scope:** rewrite `match_pat`, `parse_bracket`, `try_parse_posix_class`
to `&str`; add multibyte boundary tests.

**Out of scope (follow-ups):**

- **Layer-2 amplifier** — `strip_prefix` / `strip_suffix`
  (`src/expand/param.rs`) build an O(n) `String` per cut point and call
  `matches` O(n) times. With `match_pat` now `&str`-based, a follow-up can
  pass sub-slices directly and avoid the intermediate `String` builds.
  Separate target; record in TODO.md after this lands.
- **LRU pattern cache** (`performance.md` §4.4) — measurement shows it would
  not address the #1 site; deprioritized.
- **Byte-level matching** — would change multibyte semantics; rejected.

## 8. Risk & mitigation

| Risk | Mitigation |
|------|-----------|
| Raw byte index splits a multibyte char → panic | All advances via `len_utf8()` / `char_indices`; multibyte tests added first |
| Off-by-one in `parse_bracket` byte-length `consumed` | 20+ existing bracket/class tests + new multibyte bracket tests |
| Behavioral drift in `*` suffix iteration | W2 bit-identical diff gate + existing star tests |

## 9. References

- `performance.md` §4.4 (original P0 prediction; corrected here in §2)
- `src/expand/pattern.rs` (target), `src/expand/param.rs`,
  `src/expand/pathname.rs` (callers)
- dhat profile: `target/perf/dhat-heap-w2.json` (regenerate per §2)
