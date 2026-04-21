# field_split UTF-8 Panic Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the panic in `src/expand/field_split.rs::append_byte` on multi-byte UTF-8 content with ASCII IFS, and pin the new behavior with unit tests.

**Architecture:** Switch `split_field` from byte-level to character-aware advancement. The append helper consumes one complete UTF-8 character per call and returns its byte length; the caller advances `i` by that length. IFS bytes are filtered to ASCII at partition time so that IFS-branch `i += 1` still lands on a char boundary. No public-API change; `ExpandedField` internals stay `String`-backed.

**Tech Stack:** Rust 2024, `cargo test` (Criterion advisory).

**Reference spec:** `docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md`

---

## File Structure

**Modified files (single-file implementation change):**
- `src/expand/field_split.rs` — all production and test code.

**Modified docs:**
- `TODO.md` — strike the TOP PRIORITY entry (already lists the follow-up item for multi-byte IFS).
- `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` — update the §9.1 deferral comment for `test_fast_path_utf8_no_false_positive`.

No new files. Tests live in the existing `mod tests` block at the bottom of `src/expand/field_split.rs`.

---

## Task 1: Red — pin the panic with a failing test

**Files:**
- Modify: `src/expand/field_split.rs` (append to `mod tests` at `src/expand/field_split.rs:205`)

- [ ] **Step 1: Append the panic-reproducing unit test**

Append inside `mod tests` just before the closing `}` at the end of `src/expand/field_split.rs`:

```rust
    // ── UTF-8 multi-byte content (spec 2026-04-21-field-split-utf8-panic-fix §9.1) ──

    #[test]
    fn test_utf8_content_ascii_ifs_splits() {
        // Slow path: ASCII IFS byte present, multi-byte content elsewhere.
        // Pre-fix: panics in append_byte on '日' lead byte.
        let env = env_with_ifs(" ");
        let input = vec![unquoted("日本 語")];
        assert_eq!(values(split(&env, input)), vec!["日本", "語"]);
    }
```

- [ ] **Step 2: Confirm the test panics pre-fix**

Run:
```bash
cargo test --lib expand::field_split::tests::test_utf8_content_ascii_ifs_splits 2>&1 | tail -20
```

Expected output contains:
```
thread '...' panicked at 'byte index 1 is not a char boundary ...'
```
and the test is reported as `FAILED`.

If the test passes or panics for any other reason (e.g., assertion mismatch, panic at a different site), stop and re-read the spec — the reproducer assumption is wrong and the plan must be revisited.

- [ ] **Step 3: Commit the red test**

```bash
git add src/expand/field_split.rs
git commit -m "$(cat <<'EOF'
test(expand): pin field_split append_byte UTF-8 panic

Adds the failing regression test for the TOP PRIORITY bug logged in
TODO.md: multi-byte content mixed with unquoted ASCII IFS reaches the
slow path and panics at field_split.rs:187 with
'byte index 1 is not a char boundary'.

Spec: docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md §9.1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Green — restrict `ifs_nws` to ASCII and introduce `append_char`

**Files:**
- Modify: `src/expand/field_split.rs:40-43` (ifs_nws partition)
- Modify: `src/expand/field_split.rs:184-193` (replace `append_byte` with `append_char`)
- Modify: `src/expand/field_split.rs:102-160` (three content-byte branches in `split_field`)

- [ ] **Step 1: Restrict `ifs_nws` to ASCII bytes**

Change `src/expand/field_split.rs:40-43` from:

```rust
    let ifs_nws: Vec<u8> = ifs
        .bytes()
        .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
        .collect();
```

to:

```rust
    // Restrict IFS non-whitespace delimiters to ASCII (< 0x80). Non-ASCII bytes
    // in IFS would otherwise match UTF-8 lead or continuation bytes inside
    // multi-byte content and break split_field's "i is a char boundary"
    // invariant. See spec 2026-04-21-field-split-utf8-panic-fix §4.1 / §5.2.
    let ifs_nws: Vec<u8> = ifs
        .bytes()
        .filter(|b| !matches!(*b, b' ' | b'\t' | b'\n'))
        .filter(|b| *b < 0x80)
        .collect();
```

- [ ] **Step 2: Replace `append_byte` with `append_char`**

Replace the block at `src/expand/field_split.rs:184-193`:

```rust
/// Append the byte at position `i` in `source` to `dest`, preserving quoting.
#[inline]
fn append_byte(dest: &mut ExpandedField, source: &ExpandedField, i: usize) {
    let ch = &source.value[i..i + 1];
    if source.is_quoted(i) {
        dest.push_quoted(ch);
    } else {
        dest.push_unquoted(ch);
    }
}
```

with:

```rust
/// Append the UTF-8 character starting at byte position `i` in `source` to
/// `dest`, preserving quoting. Returns the byte length of the character.
///
/// Caller must ensure `i` is on a UTF-8 character boundary. All bytes of a
/// single character share the same `quoted_mask` bit (push_quoted and
/// push_unquoted always append a complete `&str`), so testing `is_quoted(i)`
/// on the lead byte covers the entire character.
#[inline]
fn append_char(dest: &mut ExpandedField, source: &ExpandedField, i: usize) -> usize {
    let ch_len = source.value[i..]
        .chars()
        .next()
        .expect("i on char boundary")
        .len_utf8();
    let slice = &source.value[i..i + ch_len];
    if source.is_quoted(i) {
        dest.push_quoted(slice);
    } else {
        dest.push_unquoted(slice);
    }
    ch_len
}
```

- [ ] **Step 3: Update the three content-byte branches in `split_field`**

At `src/expand/field_split.rs:117-122` (the `State::Start | State::AfterNws` normal-byte branch), change:

```rust
                } else {
                    // Normal byte: start accumulating.
                    append_byte(&mut current, field, i);
                    state = State::InField;
                    i += 1;
                }
```

to:

```rust
                } else {
                    // Normal byte: start accumulating a full UTF-8 character.
                    let ch_len = append_char(&mut current, field, i);
                    state = State::InField;
                    i += ch_len;
                }
```

At `src/expand/field_split.rs:136-139` (the `State::InField` normal-byte branch), change:

```rust
                } else {
                    append_byte(&mut current, field, i);
                    i += 1;
                }
```

to:

```rust
                } else {
                    let ch_len = append_char(&mut current, field, i);
                    i += ch_len;
                }
```

At `src/expand/field_split.rs:152-158` (the `State::AfterWs` normal-byte branch), change:

```rust
                } else {
                    // Normal byte after whitespace: start a new field.
                    append_byte(&mut current, field, i);
                    state = State::InField;
                    i += 1;
                }
```

to:

```rust
                } else {
                    // Normal byte after whitespace: start a new field.
                    let ch_len = append_char(&mut current, field, i);
                    state = State::InField;
                    i += ch_len;
                }
```

- [ ] **Step 4: Verify the red test is now green**

Run:
```bash
cargo test --lib expand::field_split::tests::test_utf8_content_ascii_ifs_splits 2>&1 | tail -10
```

Expected: `test ... ok`, `1 passed`, no panic.

- [ ] **Step 5: Run the full `expand::field_split` test module**

Run:
```bash
cargo test --lib expand::field_split 2>&1 | tail -30
```

Expected: `17 passed` (16 existing + the new UTF-8 test from Task 1). No failures.

If any previously-passing test now fails, stop. The advancement change is almost certainly not the root cause (state machine logic is unchanged); more likely the `ifs_nws` ASCII filter broke a test that used a non-ASCII IFS byte. Grep for `env_with_ifs` calls with non-ASCII string literals and revisit §5.2.

- [ ] **Step 6: Commit the minimal fix**

```bash
git add src/expand/field_split.rs
git commit -m "$(cat <<'EOF'
fix(expand): resolve field_split append_byte UTF-8 panic

Replace append_byte (which sliced source.value[i..i+1] as &str and
panicked on non-ASCII lead/continuation bytes) with append_char that
appends a whole UTF-8 character and returns its byte length. Advance
split_field's content-byte branches by that length so `i` stays on a
char boundary every iteration. Also filter ifs_nws to ASCII bytes so
IFS-byte branches (`i += 1`) cannot push `i` onto a continuation byte.

Closes TOP PRIORITY entry from TODO.md (2026-04-21 known bug).

Spec: docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Expand test coverage for the surrounding behaviors

**Files:**
- Modify: `src/expand/field_split.rs` (append further tests inside `mod tests`)

- [ ] **Step 1: Append the four remaining UTF-8 unit tests**

Append inside `mod tests`, directly after `test_utf8_content_ascii_ifs_splits`:

```rust
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
        // Pin the documented behavior change (spec §5.2): non-ASCII IFS bytes
        // are filtered out of ifs_nws and have no effect on splitting.
        // 'À' is 0xC3 0x80; both bytes are ≥ 0x80 and therefore ignored.
        let env = env_with_ifs("\u{00c0}");
        let input = vec![unquoted("À")];
        assert_eq!(values(split(&env, input)), vec!["À"]);
    }
```

- [ ] **Step 2: Run the four new tests**

Run:
```bash
cargo test --lib expand::field_split::tests::test_utf8_content_colon_delimiter \
                 expand::field_split::tests::test_utf8_quoted_not_split \
                 expand::field_split::tests::test_utf8_leading_trailing_whitespace_around_multibyte \
                 expand::field_split::tests::test_non_ascii_ifs_byte_ignored 2>&1 | tail -10
```

Expected: `4 passed`.

- [ ] **Step 3: Commit**

```bash
git add src/expand/field_split.rs
git commit -m "$(cat <<'EOF'
test(expand): cover field_split UTF-8 content cases

Pin four surrounding behaviors for the slow-path UTF-8 fix:
- colon-delimited multi-byte content
- quoted multi-byte content with an IFS byte (no split)
- trailing/leading whitespace collapse around multi-byte content
- non-ASCII IFS byte ignored (documented behavior change, spec §5.2)

Spec: docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md §9.1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Re-enable the deferred fast-path UTF-8 test

**Files:**
- Modify: `src/expand/field_split.rs` (append inside `mod tests`)
- Modify: `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` §9.1 (remove the DEFERRED comment)

- [ ] **Step 1: Append the re-enabled fast-path test**

Append inside `mod tests`, directly after `test_fast_path_empty_unquoted_field_preserved`:

```rust
    #[test]
    fn test_fast_path_utf8_no_false_positive() {
        // Deferred from spec 2026-04-21-field-split-fast-path-design §9.1 until
        // the slow-path UTF-8 panic (2026-04-21-field-split-utf8-panic-fix) was
        // resolved. UTF-8 continuation bytes (0x80-0xBF) cannot collide with
        // ASCII IFS bytes, so the fast path must engage for multi-byte-only
        // input with no unquoted IFS byte.
        let env = env_with_ifs(" \t\n");
        let input = vec![unquoted("日本語")];
        assert_eq!(values(split(&env, input)), vec!["日本語"]);
    }
```

- [ ] **Step 2: Run the re-enabled test**

Run:
```bash
cargo test --lib expand::field_split::tests::test_fast_path_utf8_no_false_positive 2>&1 | tail -10
```

Expected: `1 passed`.

- [ ] **Step 3: Update the fast-path spec deferral comment**

The fast-path spec at `docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md` §9.1 currently contains this 9-line Rust-style comment block inside the rust code fence (exact contents — replace every line of it):

```rust
// DEFERRED during Task 2 (see plan "Note (2026-04-21)"): the originally-planned
// `test_fast_path_utf8_no_false_positive` uncovered a pre-existing slow-path
// UTF-8 panic in `append_byte` (TOP PRIORITY entry in TODO.md `## Known Bugs`).
// The fast-path's UTF-8 safety guarantee from §5.2 still holds — continuation
// bytes 0x80-0xBF cannot collide with ASCII IFS bytes — and will be re-tested
// once the slow-path UTF-8 fix lands. For now, the empty-unquoted guard in
// `test_fast_path_empty_unquoted_field_preserved` (added 2026-04-21) plus the
// remaining five tests below cover the fast-path invariants we can exercise
// without tripping the slow-path bug.
```

Replace those 9 lines with this 4-line Rust-style comment (still inside the same rust code fence):

```rust
// Re-enabled 2026-04-21 after the slow-path UTF-8 panic fix
// (docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md).
// UTF-8 continuation bytes cannot collide with ASCII IFS bytes, so the fast
// path must engage for multi-byte-only input without any unquoted IFS byte.
```

Leave the surrounding `#[test]` blocks (`test_fast_path_single_field_no_ifs_chars`, etc.) and all other text in §9.1 unchanged.

- [ ] **Step 4: Run the full expand::field_split suite**

Run:
```bash
cargo test --lib expand::field_split 2>&1 | tail -10
```

Expected: `22 passed` (16 existing + 5 from Tasks 1 and 3 + 1 re-enabled).

- [ ] **Step 5: Commit**

```bash
git add src/expand/field_split.rs docs/superpowers/specs/2026-04-21-field-split-fast-path-design.md
git commit -m "$(cat <<'EOF'
test(expand): re-enable deferred fast-path UTF-8 test

The fast-path spec's test_fast_path_utf8_no_false_positive was
deferred pending the slow-path UTF-8 panic fix. With that fix now
landed, restore the test and update the spec §9.1 deferral comment
to point at the follow-up spec.

Spec: docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md §9.2

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Remove the TOP PRIORITY entry from TODO.md

**Files:**
- Modify: `TODO.md` (remove the TOP PRIORITY bullet, keep the "Multi-byte IFS support" follow-up)

- [ ] **Step 1: Delete the TOP PRIORITY line**

Open `TODO.md` and delete the entire line that begins with `- [ ] **[TOP PRIORITY]** \`src/expand/field_split.rs::append_byte\` panics on multi-byte UTF-8 input` (currently at `TODO.md:5`). Keep the surrounding `## Known Bugs` header and any other entries in that section. Leave the "Multi-byte IFS support in UTF-8 locale" entry in `## Future: Code Quality Improvements` untouched.

- [ ] **Step 2: Verify the section still has a header but the bullet is gone**

Run:
```bash
grep -n "TOP PRIORITY" TODO.md; echo "exit=$?"
```

Expected:
```
exit=1
```
(i.e. grep finds nothing, which exits non-zero).

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): strike field_split UTF-8 panic from Known Bugs

The TOP PRIORITY entry is now fixed (see
docs/superpowers/specs/2026-04-21-field-split-utf8-panic-fix-design.md).
The separate 'Multi-byte IFS support in UTF-8 locale' follow-up under
Future: Code Quality Improvements remains.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Final verification

**Files:** none modified.

- [ ] **Step 1: Run the full crate test suite**

Run (in the background if preferred — the suite takes 1–3 minutes per project conventions):
```bash
cargo test 2>&1 | tail -30
```

Expected: all tests pass. No panic messages. No new failures.

If any non-`expand::field_split` test fails, stop and inspect. The fix is confined to `field_split.rs`, so collateral failures would indicate either (a) a hidden call site was depending on `append_byte`'s single-byte behavior (unlikely — it was module-private) or (b) a test in another module was implicitly relying on non-ASCII IFS handling.

- [ ] **Step 2: Run the E2E POSIX compliance suite**

Run:
```bash
cargo build && ./e2e/run_tests.sh 2>&1 | tail -20
```

Expected: all tests pass (or the same pass/fail ratio as pre-fix; e2e flakes noted in TODO.md may surface but should not be new).

If the summary shows a higher failure count than the pre-fix baseline, capture the log (`./e2e/run_tests.sh 2>&1 | tee /tmp/e2e.log`), grep `\[FAIL\]`, and inspect the failed tests. Any failures involving IFS or field splitting should be re-triaged against §5.2 of the spec.

- [ ] **Step 3: Criterion spot check (advisory)**

Run:
```bash
cargo bench --bench expand_bench -- expand_field_split 2>&1 | tail -20
```

Expected: criterion prints median times. Compare against the pre-fix numbers recorded in `performance.md` / the fast-path spec's §8.3. Target: within ±5%. Not a blocker; informational only.

- [ ] **Step 4: Final commit check**

Run:
```bash
git log --oneline -10
git status
```

Expected: clean working tree; the last 5 commits are (in order) Task 1 red test, Task 2 fix, Task 3 coverage, Task 4 re-enabled test, Task 5 TODO strike.
