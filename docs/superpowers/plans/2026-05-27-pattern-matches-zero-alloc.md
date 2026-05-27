# `pattern::matches` Zero-Allocation Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the two per-call `Vec<char>` allocations in `src/expand/pattern.rs::matches` (W2 dhat #1 hotspot, ~1.66 MB / ~66k calls) by rewriting the recursive matcher to operate on `&str` via char-boundary byte offsets, with no new dependency and exact char-level semantics preserved.

**Architecture:** `matches` currently does `pattern.chars().collect()` and `string.chars().collect()` into `Vec<char>` on every call, then runs a recursive `&[char]` matcher. The rewrite makes `match_pat`, `parse_bracket`, and `try_parse_posix_class` operate directly on `&str`, advancing by `char.len_utf8()` byte offsets and slicing at char boundaries. The 50+ existing ASCII unit tests plus new multibyte boundary tests are the regression guard; a bit-identical W2 output diff confirms behavioral equivalence.

**Tech Stack:** Rust (`&str` / `char_indices` / `Chars::as_str`), Criterion + dhat-rs for before/after measurement.

**Spec:** `docs/superpowers/specs/2026-05-27-pattern-matches-zero-alloc-design.md`

---

## File Structure

- **Modify:** `src/expand/pattern.rs` — rewrite `matches` (lines 9-13), `match_pat` (15-55), `parse_bracket` (62-115), `try_parse_posix_class` (183-198); add multibyte tests to the existing `#[cfg(test)] mod tests` block. No other files change — `BracketItem` / `PosixClass` / `POSIX_CLASSES` are untouched, and the public `matches` signature `(&str, &str) -> bool` is preserved so callers in `src/expand/param.rs` and `src/expand/pathname.rs` need no changes.
- **Modify (Task 5):** `performance.md`, `TODO.md` — record measured impact and the Layer-2 follow-up.

---

## Task 1: Capture W2 baseline (verification gates)

No production code. This captures the pre-change W2 output (for the bit-identical gate) and the current dhat numbers (for the allocation-delta gate).

**Files:** none modified.

- [ ] **Step 1: Build the profiling `yosh` binary and the dhat binary**

Run:
```bash
cargo build --profile profiling --bin yosh
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
```
Expected: both finish `Finished` with exit 0. (The dhat binary may already be built from the brainstorming session; this is idempotent.)

- [ ] **Step 2: Capture the W2 stdout+stderr baseline**

Run:
```bash
mkdir -p target/perf
./target/profiling/yosh benches/data/script_heavy.sh > target/perf/w2_baseline.out 2> target/perf/w2_baseline.err
cat target/perf/w2_baseline.out
```
Expected: `w2_baseline.out` contains `sum=500500`. (The exact stdout is small; the file is the diff target for Task 4.)

- [ ] **Step 3: Capture the current dhat W2 numbers**

Run:
```bash
./target/profiling/yosh-dhat benches/data/script_heavy.sh > /dev/null 2>&1
mv dhat-heap.json target/perf/dhat-heap-w2-before.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-before.json 15 | head -25
```
Expected (matches the spec §2 measurement): `Total bytes: ~8.34 MB`, and rank #1 by bytes is `yosh::expand::pattern::matches (src/expand/pattern.rs:11:39)` at ~1.28 MB / ~43,043 calls. Record the "Total bytes" line — Task 5 compares against it.

- [ ] **Step 4: No commit**

Baseline artifacts live under `target/` (gitignored). Nothing to commit.

---

## Task 2: Add multibyte characterization tests

These tests lock in current behavior at multibyte (non-ASCII) boundaries — the exact place a byte-offset rewrite can break. **They PASS on the current `&[char]` implementation** (which is already codepoint-correct), so this is a behavior-preserving refactor guarded by characterization tests, not red→green TDD. Their job is to FAIL (often with a "byte index is not a char boundary" panic) if Task 3 indexes `&str` incorrectly.

**Files:**
- Modify: `src/expand/pattern.rs` (add tests inside the existing `#[cfg(test)] mod tests` block, e.g. after the `missing_colon_close_does_not_panic` test near line 482)

- [ ] **Step 1: Add the multibyte tests**

Insert this block just before the closing `}` of `mod tests` in `src/expand/pattern.rs`:

```rust
    // ── Multibyte (UTF-8) boundary tests ──
    // These pass on the &[char] implementation and guard the &str rewrite
    // against splitting a multibyte char at a non-char-boundary byte offset.
    #[test]
    fn multibyte_literal_and_star() {
        assert!(matches("日*", "日本語"));
        assert!(matches("*語", "日本語"));
        assert!(matches("日本語", "日本語"));
        assert!(!matches("日*", "本日"));
    }

    #[test]
    fn multibyte_question() {
        assert!(matches("?", "あ"));
        assert!(!matches("?", "あい"));
        assert!(matches("a?c", "aあc"));
    }

    #[test]
    fn multibyte_bracket_range() {
        // あ=U+3042, か=U+304B, ん=U+3093, ン=U+30F3 (katakana, out of range)
        assert!(matches("[あ-ん]", "か"));
        assert!(!matches("[あ-ん]", "ン"));
        assert!(matches("[0-9]語", "5語"));
    }

    #[test]
    fn multibyte_backslash_trailing() {
        assert!(matches("あ\\", "あ\\"));
        assert!(matches("\\あ", "あ"));
    }
```

- [ ] **Step 2: Run the new tests — verify they PASS on current code**

Run:
```bash
cargo test --lib expand::pattern::tests::multibyte -- --nocapture
```
Expected: PASS (4 tests). If any FAIL, the test expectation is wrong — fix the test, not the implementation (current `&[char]` code is the reference). Do not proceed until all 4 pass.

- [ ] **Step 3: Run the full pattern test module — confirm no regressions**

Run:
```bash
cargo test --lib expand::pattern
```
Expected: all tests pass (existing ~50 + 4 new).

- [ ] **Step 4: Commit**

```bash
git add src/expand/pattern.rs
git commit -m "test(expand): add multibyte boundary tests for pattern::matches

Characterization tests guarding the upcoming &str rewrite against
char-boundary byte-offset bugs. Pass on the current &[char] impl.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Rewrite the matcher to `&str` (atomic)

`matches`, `match_pat`, `parse_bracket`, and `try_parse_posix_class` are mutually dependent and must change together to compile. Replace all four at once with the `&str` versions below.

**Files:**
- Modify: `src/expand/pattern.rs:9-13` (`matches`), `:15-55` (`match_pat`), `:62-115` (`parse_bracket`), `:183-198` (`try_parse_posix_class`)

- [ ] **Step 1: Replace `matches` and `match_pat`**

Replace the current `matches` (lines 9-13) and `match_pat` (lines 15-55) with:

```rust
pub fn matches(pattern: &str, string: &str) -> bool {
    match_pat(pattern, string)
}

fn match_pat(pat: &str, s: &str) -> bool {
    let mut pat_chars = pat.chars();
    match pat_chars.next() {
        None => s.is_empty(),

        Some('*') => {
            // Try matching the rest of the pattern against every suffix of s,
            // including the empty suffix (the loop tries the empty string just
            // before `chars().next()` returns None and we give up).
            let rest = pat_chars.as_str();
            let mut rem = s;
            loop {
                if match_pat(rest, rem) {
                    return true;
                }
                match rem.chars().next() {
                    Some(c) => rem = &rem[c.len_utf8()..],
                    None => return false,
                }
            }
        }

        Some('?') => match s.chars().next() {
            Some(c) => match_pat(pat_chars.as_str(), &s[c.len_utf8()..]),
            None => false,
        },

        Some('[') => {
            let after_bracket = pat_chars.as_str();
            let s_first = s.chars().next();
            if let Some((consumed, matched_char)) = parse_bracket(after_bracket, s_first) {
                // Bracket expressions always match exactly one character.
                match s_first {
                    Some(c) if matched_char => {
                        match_pat(&after_bracket[consumed..], &s[c.len_utf8()..])
                    }
                    _ => false,
                }
            } else {
                // Malformed bracket — treat '[' as a literal.
                match s.chars().next() {
                    Some('[') => match_pat(after_bracket, &s['['.len_utf8()..]),
                    _ => false,
                }
            }
        }

        Some('\\') => {
            let after_bs = pat_chars.as_str();
            match after_bs.chars().next() {
                // '\x' matches literal 'x'.
                Some(pc) => match s.chars().next() {
                    Some(sc) if sc == pc => {
                        match_pat(&after_bs[pc.len_utf8()..], &s[sc.len_utf8()..])
                    }
                    _ => false,
                },
                // Trailing backslash — match a literal backslash.
                None => match s.chars().next() {
                    Some('\\') => match_pat(after_bs, &s['\\'.len_utf8()..]),
                    _ => false,
                },
            }
        }

        Some(c) => match s.chars().next() {
            Some(sc) if sc == c => match_pat(pat_chars.as_str(), &s[sc.len_utf8()..]),
            _ => false,
        },
    }
}
```

- [ ] **Step 2: Replace `parse_bracket`**

Replace the current `parse_bracket` (lines 62-115) with this `&str` version. `consumed` is now a **byte length** into `pat` (the slice after the opening `[`), computed as `pat.len() - rest.len()`:

```rust
/// Parse a bracket expression starting *after* the opening `[`.
/// Returns `Some((bytes_consumed_including_closing_bracket, did_match))` on
/// success, or `None` if the bracket is malformed (no closing `]`).
///
/// `bytes_consumed` is a byte length into `pat`, so the caller advances with
/// `&pat[bytes_consumed..]`. `ch` is the character from the string being
/// matched (if any).
fn parse_bracket(pat: &str, ch: Option<char>) -> Option<(usize, bool)> {
    if pat.is_empty() {
        return None;
    }

    let mut rest = pat;
    let negate = rest.starts_with('!');
    if negate {
        rest = &rest['!'.len_utf8()..];
    }

    let mut members: Vec<BracketItem> = Vec::new();
    let mut found_close = false;

    while let Some(c0) = rest.chars().next() {
        // Closing ']' (but not when it would make an empty class — a leading
        // ']' is treated as a literal member).
        if c0 == ']' && !members.is_empty() {
            rest = &rest[c0.len_utf8()..];
            found_close = true;
            break;
        }

        // POSIX character class [:class:]
        if c0 == '[' && rest[c0.len_utf8()..].starts_with(':') {
            let after_open = &rest[c0.len_utf8() + ':'.len_utf8()..];
            if let Some((consumed, class)) = try_parse_posix_class(after_open) {
                members.push(BracketItem::Class(class));
                rest = &after_open[consumed..];
                continue;
            }
            // Fall through to literal handling on a malformed class.
        }

        // Range: x-y  (only if '-' is followed by another non-']' char).
        let after_c0 = &rest[c0.len_utf8()..];
        if let Some('-') = after_c0.chars().next() {
            let after_dash = &after_c0['-'.len_utf8()..];
            if let Some(hi) = after_dash.chars().next() {
                if hi != ']' {
                    members.push(BracketItem::Range(c0, hi));
                    rest = &after_dash[hi.len_utf8()..];
                    continue;
                }
            }
        }

        members.push(BracketItem::Char(c0));
        rest = &rest[c0.len_utf8()..];
    }

    if !found_close {
        return None;
    }

    let inner_match = ch
        .map(|c| members.iter().any(|m| m.matches(c)))
        .unwrap_or(false);
    let result = if negate { !inner_match } else { inner_match };

    let consumed = pat.len() - rest.len();
    Some((consumed, result))
}
```

- [ ] **Step 3: Replace `try_parse_posix_class`**

Replace the current `try_parse_posix_class` (lines 183-198) with this `&str` version. `pat` is the slice starting after `[:`; `consumed` is a byte length covering the class name plus the trailing `:]`:

```rust
/// Try to parse a `[:class:]` POSIX character-class form.
///
/// `pat` is the slice starting AFTER the opening `[:` (i.e., the first
/// character of the class name). Returns `Some((bytes_consumed, class))`
/// where `bytes_consumed` covers the class name and the trailing `:]` (so the
/// caller advances by that many bytes from the first name character).
fn try_parse_posix_class(pat: &str) -> Option<(usize, PosixClass)> {
    let pos = pat.find(":]")?;
    let name = &pat[..pos];
    for (n, c) in POSIX_CLASSES {
        if name == *n {
            return Some((pos + ":]".len(), *c));
        }
    }
    None
}
```

- [ ] **Step 4: Verify it compiles**

Run:
```bash
cargo build --lib
```
Expected: `Finished` with exit 0, no errors. (If `parse_bracket` / `try_parse_posix_class` warn as unused that means a call site was missed — re-check `match_pat`'s `[` arm.)

- [ ] **Step 5: Run the full pattern test module**

Run:
```bash
cargo test --lib expand::pattern
```
Expected: ALL tests pass — the ~50 existing ASCII tests AND the 4 multibyte tests from Task 2. A panic mentioning "byte index … is not a char boundary" means a slice landed mid-char; re-check the `len_utf8()` advances. A logic failure in a bracket/range test means a `parse_bracket` off-by-one in the byte-length `consumed`.

- [ ] **Step 6: Commit**

```bash
git add src/expand/pattern.rs
git commit -m "perf(expand): rewrite pattern::matches on &str to drop per-call Vec<char>

match_pat/parse_bracket/try_parse_posix_class now operate directly on
&str via char-boundary byte offsets, eliminating the two per-call
Vec<char> allocations (W2 dhat #1 site, ~1.66 MB / ~66k calls). Public
matches(&str,&str)->bool signature unchanged; char-level semantics
preserved. Guarded by existing ASCII tests + new multibyte tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Full regression + bit-identical W2 verification

Confirm the rewrite changed nothing observable: full test suite, e2e, and a byte-for-byte W2 output diff against the Task 1 baseline.

**Files:** none modified (unless a test needs adjusting, which is not expected).

- [ ] **Step 1: Full unit + integration test suite**

Run (background per CLAUDE.md guidance — the suite can take minutes):
```bash
cargo test 2>&1 | tail -30
```
Expected: `test result: ok` for every binary; 0 failed.

- [ ] **Step 2: E2E POSIX compliance suite (glob / pattern / param-expansion)**

Run:
```bash
cargo build --bin yosh
./e2e/run_tests.sh --filter=param
./e2e/run_tests.sh --filter=glob
```
Expected: all selected tests PASS, 0 FAIL, 0 XFAIL regressions. (If a filter matches nothing, run `./e2e/run_tests.sh` in full.)

- [ ] **Step 3: Bit-identical W2 output diff**

Run:
```bash
cargo build --profile profiling --bin yosh
./target/profiling/yosh benches/data/script_heavy.sh > target/perf/w2_after.out 2> target/perf/w2_after.err
diff target/perf/w2_baseline.out target/perf/w2_after.out && echo "STDOUT IDENTICAL"
diff target/perf/w2_baseline.err target/perf/w2_after.err && echo "STDERR IDENTICAL"
```
Expected: both `diff`s produce no output and print `STDOUT IDENTICAL` / `STDERR IDENTICAL`. Any difference is a behavioral regression — stop and investigate before continuing.

- [ ] **Step 4: No commit**

Verification only; no source changes. If Step 1-3 surfaced a needed fix, make it, re-run the affected gate, and commit with a descriptive message before proceeding.

---

## Task 5: Re-measure and document impact

Capture the after numbers, confirm the success criteria, and record the result in `performance.md` and the Layer-2 follow-up in `TODO.md`.

**Files:**
- Modify: `performance.md` (§4.4 + §5.2), `TODO.md`

- [ ] **Step 1: Re-measure dhat W2 (after)**

Run:
```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
./target/profiling/yosh-dhat benches/data/script_heavy.sh > /dev/null 2>&1
mv dhat-heap.json target/perf/dhat-heap-w2-after.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-after.json 15 | head -25
```
Expected: `yosh::expand::pattern::matches` no longer appears in the Top-10 by bytes; `Total bytes` drops from ~8.34 MB to ~6.7 MB (~20% reduction, ~1.66 MB saved). Record the new `Total bytes` and confirm the delta. If `pattern::matches` is still present, the rewrite did not remove the allocation — investigate before documenting.

- [ ] **Step 2: Re-measure Criterion (exec + expand)**

Run (background; benches take a few minutes):
```bash
cargo bench --bench exec_bench --bench expand_bench 2>&1 | grep -E "exec_param_expansion_200|expand_param_default|expand_field_split|time:"
```
Expected: `exec_param_expansion_200` and the `expand_*` medians are no worse than the `performance.md` §3.2 figures (and likely improved, since the char-conversion CPU work is removed). Record the medians. Treat any >10% regression on an unrelated bench as a blocker.

- [ ] **Step 3: Update `performance.md`**

Add an amendment dated 2026-05-27 to `performance.md`: in §4.4 note that the HEAD re-measurement found `pattern::matches` at ~1.66 MB / ~66k calls (#1 by bytes and calls), that the dominant site is the subject-string `Vec<char>` (line 11) — correcting the §4.4 LRU-cache proposal — and that the fix was the `&str` zero-alloc rewrite. Record the measured impact from Steps 1-2 (W2 total before/after, the Criterion medians). In §5.2, mark the §4.4 P0 item **done** and promote the §4.2 function-call item to the head of the queue. Match the existing amendment style (see the §4.3 / §4.7 "Fix applied" blocks).

- [ ] **Step 4: Add the Layer-2 follow-up to `TODO.md`**

Append to the appropriate follow-ups section of `TODO.md`:

```markdown
- [ ] `strip_prefix` / `strip_suffix` Layer-2 allocation amplifier
      (`src/expand/param.rs:184-236`) — `${VAR#pat}` / `%` / `##` / `%%`
      build a fresh `String` for every cut point and call `pattern::matches`
      O(n) times per operation. Now that `match_pat` is `&str`-based
      (2026-05-27), a follow-up can pass sub-slices directly and skip the
      intermediate `String` builds. Separate perf target; measured in
      `docs/superpowers/specs/2026-05-27-pattern-matches-zero-alloc-design.md` §7.
```

- [ ] **Step 5: Commit**

```bash
git add performance.md TODO.md
git commit -m "docs(perf): record pattern::matches &str rewrite impact + Layer-2 follow-up

W2 total allocation <BEFORE> MB -> <AFTER> MB (~20% / ~1.66 MB saved);
pattern::matches dropped out of the dhat Top-10. Corrects performance.md
§4.4 (LRU-cache proposal would not have addressed the #1 subject-string
site). Records the strip_prefix/strip_suffix Layer-2 follow-up in TODO.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```
Replace `<BEFORE>` / `<AFTER>` with the measured `Total bytes` values from Step 1.

---

## Self-Review

**Spec coverage:**
- Spec §1 goal (eliminate per-call `Vec<char>`, `&str`, no new dep) → Task 3.
- Spec §4.1 `matches` signature unchanged → Task 3 Step 1.
- Spec §4.2 `match_pat` → Task 3 Step 1; §4.3 `parse_bracket` byte-length `consumed` → Step 2; §4.4 `try_parse_posix_class` → Step 3.
- Spec §5 testing: existing tests unchanged + multibyte tests → Task 2; verification gates (cargo test, e2e, bit-identical, dhat, Criterion) → Tasks 4 & 5.
- Spec §6 success criteria → Task 5 Steps 1-2.
- Spec §7 Layer-2 follow-up → Task 5 Step 4.
- No gaps.

**Placeholder scan:** The only intentional fill-ins are `<BEFORE>`/`<AFTER>` in the Task 5 commit message (measured values, can't be known until Step 1 runs) — explicitly flagged. No TBD/TODO/"handle edge cases" placeholders; all code blocks are complete.

**Type consistency:** `parse_bracket(pat: &str, ch: Option<char>) -> Option<(usize, bool)>` and `try_parse_posix_class(pat: &str) -> Option<(usize, PosixClass)>` signatures match between their definitions (Task 3 Steps 2-3) and their call sites in `match_pat` (Task 3 Step 1: `parse_bracket(after_bracket, s_first)`, `&after_bracket[consumed..]`; and inside `parse_bracket`: `try_parse_posix_class(after_open)`, `&after_open[consumed..]`). `consumed` is consistently a byte length throughout. `matches(&str, &str) -> bool` is unchanged, so `param.rs` / `pathname.rs` callers stay valid.
