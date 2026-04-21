# Design: `field_split::append_byte` UTF-8 Panic Fix

**Date:** 2026-04-21
**Priority:** P0 (TOP PRIORITY in `TODO.md` / Known Bugs)
**Estimated effort:** ~0.5 day
**Related spec:** `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md`

## 1. Background

`src/expand/field_split.rs::append_byte` panics on multi-byte UTF-8 field content:

```rust
#[inline]
fn append_byte(dest: &mut ExpandedField, source: &ExpandedField, i: usize) {
    let ch = &source.value[i..i + 1];   // <-- panics when i+1 is not a char boundary
    ...
}
```

Reproducer (reaches slow path, panics):

```rust
// Mixed multi-byte content and unquoted ASCII IFS byte.
split(&env_with_ifs(" \t\n"), vec![unquoted("日本 語")]);
// panics: "byte index 1 is not a char boundary"
```

The bare `"日本語"` case is currently masked by the Task 3 fast path (no unquoted IFS byte → input `Vec` returned unchanged). Any multi-byte content mixed with an unquoted IFS byte (space, tab, `:`, etc.) still reaches `split_field`, advances byte-by-byte, and invokes `append_byte` on the first byte of the multi-byte character, which panics.

### 1.1 Root cause

`ExpandedField.value: String` is always valid UTF-8 (Rust guarantee). The byte-level state machine in `split_field` advances `i` by 1 regardless of whether `bytes[i]` is an ASCII byte or the lead byte of a multi-byte UTF-8 character. When control reaches `append_byte` for a multi-byte lead byte, `&value[i..i+1]` slices through the middle of a code point and panics.

### 1.2 Relevant invariants

- `push_quoted` / `push_unquoted` always append a complete `&str`, so every byte of any single UTF-8 character shares the same `quoted_mask` bit. Checking `is_quoted(i)` on the lead byte is sufficient for the entire character.
- `pathname.rs:47`, `field_split.rs:79/179` use `as_bytes()` / `.bytes()` — those return `u8` values, never a `str` sub-slice. They do not panic. The only slice-through-UTF-8 offender in the expand pipeline is `append_byte`.

### 1.3 Reference behavior survey (2026-04-21 bash / dash check)

| Shell / locale | `IFS="日"; s="a日b日c"; set -- $s; echo "$#"` | Semantics |
|---|---|---|
| bash 3.2 / `LC_ALL=en_US.UTF-8` | `3` → `[a] [b] [c]` | character-level IFS (bash extension) |
| bash 3.2 / `LC_ALL=C` | `7` → `[a] [] [] [b] [] [] [c]` | byte-level IFS |
| dash (POSIX-strict) | `7` (same as bash C locale) | byte-level IFS |

yosh's current implementation is byte-level (dash-equivalent) and POSIX XBD 8.3 permits this. Bash-style multi-byte IFS support is **out of scope** for this fix — see §7.3 and the TODO.md entry "Multi-byte IFS support in UTF-8 locale".

## 2. Goal

Fix the panic on multi-byte UTF-8 field content with ASCII IFS. Preserve all existing POSIX XBD 8.3 Field Splitting semantics.

**Non-goals:**
- Bash-style character-level multi-byte IFS matching (separate TODO item).
- Support for IFS containing arbitrary non-ASCII bytes. Those bytes were already mis-handled by the current byte-level loop (split on continuation-byte matches, panics on lead-byte matches); treating them as "ignored" is not a meaningful regression from that broken state.
- Changes to `ExpandedField`'s internal representation. The `Vec<u8>` migration hinted at in the TODO.md entry is not required — `String` + char-aware advancement is sufficient.
- Changes to `pathname.rs`, `param.rs`, or other expand-pipeline modules. They do not exhibit this bug.

## 3. Approach

Change `field_split::split_field` from byte-level to character-aware advancement. Each call to the append helper consumes one complete UTF-8 character; each IFS-byte branch stays at 1-byte advancement (IFS is filtered to ASCII at entry, §4.1).

### 3.1 Invariant established

After every mutation of `i` inside `split_field`, `i` points to a UTF-8 character boundary of `field.value`.

Proof sketch:
- Initial `i = 0` is a boundary (start of `value`).
- Non-IFS (content) branch: `i += append_char(...)` advances by `char.len_utf8()` → next boundary.
- IFS branch: `i += 1`. IFS bytes are filtered to `< 0x80` (§4.1), so the byte at `i` is a single-byte ASCII character. `i + 1` is therefore also a boundary.

Given this invariant, `&source.value[i..i + ch_len]` inside `append_char` is always a valid `&str` slice.

### 3.2 Why not the TODO.md `Vec<u8>` suggestion

The original TODO entry proposed migrating `ExpandedField.value` to `Vec<u8>` "while preserving UTF-8 at field boundaries". That approach:
- requires touching every caller of `.value` (4 modules, ~10 sites);
- loses UTF-8 safety guarantees at the type level (any `Vec<u8>` can be invalid UTF-8);
- does not solve the underlying problem any better than char-aware advancement.

Char-aware advancement achieves the same correctness with a ~15-line diff confined to one file.

## 4. Change

**File:** `src/expand/field_split.rs` (only).

### 4.1 IFS partition: restrict `ifs_nws` to ASCII

```rust
 let ifs_nws: Vec<u8> = ifs
     .bytes()
     .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
+    .filter(|b| *b < 0x80)
     .collect();
```

`ifs_ws` is unchanged — `b' '`, `b'\t'`, `b'\n'` are all `< 0x80`.

Rationale: non-ASCII IFS bytes could otherwise match UTF-8 lead/continuation bytes inside multi-byte content. Matching a continuation byte breaks the "`i` is a char boundary" invariant on the next iteration. Filtering to ASCII guarantees the invariant at O(|IFS|) cost at `split()` entry (negligible; IFS is short).

### 4.2 Rename `append_byte` → `append_char`, return consumed length

```rust
-/// Append the byte at position `i` in `source` to `dest`, preserving quoting.
-#[inline]
-fn append_byte(dest: &mut ExpandedField, source: &ExpandedField, i: usize) {
-    let ch = &source.value[i..i + 1];
-    if source.is_quoted(i) {
-        dest.push_quoted(ch);
-    } else {
-        dest.push_unquoted(ch);
-    }
-}
+/// Append the UTF-8 character starting at byte position `i` in `source` to
+/// `dest`, preserving quoting. Returns the byte length of the character.
+///
+/// Caller must ensure `i` is on a character boundary. All bytes of a single
+/// UTF-8 character share the same `quoted_mask` bit (push_quoted/push_unquoted
+/// always append a full &str), so testing `is_quoted(i)` on the lead byte
+/// covers the entire character.
+#[inline]
+fn append_char(dest: &mut ExpandedField, source: &ExpandedField, i: usize) -> usize {
+    let ch_len = source.value[i..].chars().next().expect("i on char boundary").len_utf8();
+    let slice = &source.value[i..i + ch_len];
+    if source.is_quoted(i) {
+        dest.push_quoted(slice);
+    } else {
+        dest.push_unquoted(slice);
+    }
+    ch_len
+}
```

### 4.3 `split_field`: advance by character length in content branches

Three content-byte branches currently do `append_byte(...); i += 1;`. Change each to advance by the char's byte length:

```rust
-    append_byte(&mut current, field, i);
-    state = State::InField;
-    i += 1;
+    let ch_len = append_char(&mut current, field, i);
+    state = State::InField;
+    i += ch_len;
```

(Same shape for the `State::InField` self-loop and the `State::AfterWs` → `InField` transition.)

IFS branches keep `i += 1;` — ASCII bytes only, per §4.1.

### 4.4 Module diff summary

- ~3 lines changed in `ifs_nws` partition.
- `append_byte` → `append_char`: ~8 lines, behavioral swap.
- 3 call sites in `split_field`: each `i += 1` → `i += ch_len` with a `let` binding.
- No signature changes to `split` or any public item.

## 5. Semantic Preservation

### 5.1 POSIX XBD 8.3 invariants

| Invariant | Pre-fix | Post-fix |
|---|---|---|
| IFS unset → default `" \t\n"` | `get_ifs` runs first | same |
| IFS empty → no splitting, drop unquoted empties | early-return branch | same |
| Quoted bytes protected from splitting | `!field.is_quoted(i)` gate | same (checked on char lead byte; all bytes of a char share the bit) |
| Consecutive IFS-whitespace collapses | state machine | same |
| IFS non-whitespace produces empty field at Start/AfterNws | state machine | same |
| Multi-byte UTF-8 content with ASCII IFS | **panics** | splits correctly |

### 5.2 Behavior change: non-ASCII IFS bytes

| Case | Pre-fix | Post-fix |
|---|---|---|
| IFS bytes all `< 0x80` (default, `:`, `,`, etc.) | works | works (identical output) |
| IFS contains `≥ 0x80` byte that matches UTF-8 continuation byte of content | splits mid-character, can produce invalid `String` internally (panics on a later slice) | **byte ignored, no split** |
| IFS contains `≥ 0x80` byte that matches UTF-8 lead byte of content | panics on the next content byte (`append_byte` on continuation byte) | **byte ignored, no split** |
| IFS contains `≥ 0x80` byte with no matching content | no effect | no effect (byte filtered out at partition time) |

All four pre-fix behaviors for non-ASCII IFS bytes were either incorrect (mid-codepoint split) or fatal (panic). Replacing them with "ignore" is a strict improvement and does not affect any well-formed POSIX script.

### 5.3 Empty-unquoted edge case

Unchanged from the fast-path spec (`2026-04-21-field-split-fast-path-design.md` §5.1). The final filter in `expand_word` (`!f.is_empty() || f.was_quoted`) still applies.

## 6. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A hidden caller depends on non-ASCII IFS bytes splitting the current broken way | Very low | Post-fix behavior change | No such caller found (`rg 'IFS.*\\x[89a-f]'` and review of `tests/`, `e2e/`). Document in commit message and release notes. |
| `append_char` overhead regresses `expand_field_split` bench beyond ±5% | Low | W2 perf regression | Run `cargo bench --bench expand_bench -- expand_field_split` pre- and post-fix. If regression > 5%, replace `chars().next().unwrap().len_utf8()` with a 128-byte lookup keyed on `bytes[i]` (1 for `< 0x80`, `2..=4` from UTF-8 lead-byte pattern). |
| A similar panic lurks in `pathname.rs` / `param.rs` | Very low | Separate bug | Pre-fix grep confirmed: only `field_split.rs:187` slices `value[i..j]` across bytes. Other sites use `as_bytes()` / `.bytes()`. |
| `fast_path`'s `needs_splitting` mis-classifies due to non-ASCII IFS filtering | Low | Fast path skipped when it should engage / engaged when it shouldn't | `needs_splitting` uses the same `ifs_ws` / `ifs_nws` slices post-filter, so classification is internally consistent. Added unit test pins this. |

## 7. Scope

### 7.1 In scope
- `src/expand/field_split.rs`: the three changes in §4.
- `src/expand/field_split.rs::tests`: the six new tests in §9, plus re-enabling the deferred `test_fast_path_utf8_no_false_positive`.

### 7.2 Doc updates
- `TODO.md`: strike the TOP PRIORITY entry (done atomically with the implementation commit).
- `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` §9.1: update the deferral comment to note the UTF-8 test is re-enabled.

### 7.3 Out of scope
- Bash-style char-level multi-byte IFS (`IFS="日"`): separate TODO entry (2026-04-21 addition).
- `ExpandedField` internal-representation changes.
- Other expand modules.
- E2E test suite additions: unit tests in-tree fully cover the regression. If future telemetry points to production UTF-8 regressions, an E2E can be added under `e2e/posix_spec/2_06_05_field_splitting/` at that time.

## 8. Verification Plan

### 8.1 Correctness

| Step | Command | Pass criterion |
|---|---|---|
| Pre-fix panic repro | `cargo test --lib expand::field_split::tests::test_utf8_content_ascii_ifs_splits` (new test, pre-fix) | Panics with "byte index 1 is not a char boundary" |
| Unit tests | `cargo test --lib expand::field_split` | All tests pass (16 existing + 5 new + 1 re-enabled = 22) |
| Full crate tests | `cargo test` | All pass |
| E2E | `./e2e/run_tests.sh` | All pass |

### 8.2 Performance

| Step | Command | Pass criterion |
|---|---|---|
| Criterion regression | `cargo bench --bench expand_bench -- expand_field_split` | Median within ±5% of pre-fix |
| dhat (W2) spot check | `cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- benches/data/script_heavy.sh`; inspect top-10 | `field_split::emit` site rank and bytes unchanged from post-fast-path baseline (~0 after fast-path lands) |

Both are advisory (blockers only if a regression > 5% appears).

## 9. Tests

All additions go into `src/expand/field_split.rs::tests`.

### 9.1 New unit tests

```rust
// ── UTF-8 multi-byte content (§4.3 of this spec) ──

#[test]
fn test_utf8_content_ascii_ifs_splits() {
    // Slow path: ASCII IFS byte present, multi-byte content elsewhere.
    // Pre-fix: panics in append_byte on '日' lead byte.
    let env = env_with_ifs(" ");
    let input = vec![unquoted("日本 語")];
    assert_eq!(values(split(&env, input)), vec!["日本", "語"]);
}

#[test]
fn test_utf8_content_colon_delimiter() {
    // Non-whitespace ASCII IFS surrounding multi-byte content.
    let env = env_with_ifs(":");
    let input = vec![unquoted("a:日:b")];
    assert_eq!(values(split(&env, input)), vec!["a", "日", "b"]);
}

#[test]
fn test_utf8_quoted_not_split() {
    // Quoted multi-byte content including an IFS byte must stay intact.
    let env = env_with_ifs(" ");
    let input = vec![quoted_field("日 本")];
    assert_eq!(values(split(&env, input)), vec!["日 本"]);
}

#[test]
fn test_utf8_leading_trailing_whitespace_around_multibyte() {
    // Trailing/leading IFS-whitespace collapse around multi-byte content.
    let env = env_with_ifs(" \t\n");
    let input = vec![unquoted("  日本語  ")];
    assert_eq!(values(split(&env, input)), vec!["日本語"]);
}

#[test]
fn test_non_ascii_ifs_byte_ignored() {
    // Pin the documented behavior change (§5.2): non-ASCII IFS bytes are
    // filtered out of ifs_nws and have no effect on splitting.
    let env = env_with_ifs("\u{00c0}");  // 0xC3 0x80 — both non-ASCII
    let input = vec![unquoted("À")];
    assert_eq!(values(split(&env, input)), vec!["À"]);
}
```

### 9.2 Re-enable the deferred fast-path UTF-8 test

The fast-path spec deferred this test pending the slow-path UTF-8 fix. Re-add at §9.1 of the fast-path spec test body and inside the same `mod tests`:

```rust
#[test]
fn test_fast_path_utf8_no_false_positive() {
    // UTF-8 continuation bytes (0x80-0xBF) must not be mistaken for IFS bytes
    // by needs_splitting's byte scan; fast path must engage.
    let env = env_with_ifs(" \t\n");
    let input = vec![unquoted("日本語")];
    let out = split(&env, input);
    assert_eq!(values(out), vec!["日本語"]);
}
```

### 9.3 Existing tests

All 16 existing tests remain unchanged and must pass (10 pre-fast-path + 5 fast-path + 1 empty-unquoted guard added 2026-04-21). None of them exercises multi-byte content on the slow path, so they are not affected by the advancement change.

## 10. Rollout

Single commit touching:
1. `src/expand/field_split.rs` (implementation + tests).
2. `TODO.md` (strike the TOP PRIORITY entry).
3. `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` (update §9.1 deferral note).

Commit message should reference both this spec and the originating fast-path spec so future archaeology links the panic discovery, the deferral, and the fix.
