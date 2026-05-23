# Literal Argv Word-Splitting Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix POSIX XCU §2.6.5 violation where yosh applies field splitting to literal command argv tokens (e.g. `IFS=:; echo a::b` produces 3 fields instead of 1).

**Architecture:** Refactor `ExpandedField` to use two independent per-byte masks — `split_protected_mask` and `glob_protected_mask` — so that the three POSIX byte origins (quoted, literal, expansion) can each be classified correctly. Add `push_literal` that marks bytes as split-protected but glob-subject, and wire it into `Literal` parts so field splitting no longer eats literal IFS bytes while glob expansion of literal `*` is preserved.

**Tech Stack:** Rust (edition 2024), cargo workspace, POSIX shell, dash/bash for cross-reference. Affected modules: `src/expand/mod.rs`, `src/expand/field_split.rs`, `src/expand/pathname.rs`, `src/expand/pipeline.rs`. E2E test format under `e2e/posix_spec/2_06_05_field_splitting/`.

**Spec:** `docs/superpowers/specs/2026-05-23-literal-argv-word-splitting-fix-design.md`

---

## File Map

| File | Responsibility | Action |
|------|----------------|--------|
| `src/expand/mod.rs` | `ExpandedField` data model + push/predicate API | Modify: 2-mask refactor, new `push_literal`, predicate split |
| `src/expand/field_split.rs` | IFS field splitting state machine | Modify: predicate rename + `append_char` 4-way routing |
| `src/expand/pathname.rs` | Glob expansion | Modify: predicate rename |
| `src/expand/pipeline.rs` | `Word` → `ExpandedField` dispatch | Modify: `push_unquoted` → `push_expanded` rename + the 1-line fix in `expand_part_literal` |
| `e2e/posix_spec/2_06_05_field_splitting/literal_argv_not_split.sh` | E2E: bug reproduction | Create |
| `e2e/posix_spec/2_06_05_field_splitting/literal_with_consecutive_nws_ifs.sh` | E2E: adjacent NWS-IFS in literal | Create |
| `e2e/posix_spec/2_06_05_field_splitting/literal_mixed_with_expansion.sh` | E2E: literal + `$var` mix | Create |
| `e2e/posix_spec/2_06_05_field_splitting/literal_glob_metachar_still_globs.sh` | E2E: literal `*` glob still works | Create |

---

## Task 1: ExpandedField data model refactor + in-module API migration (no behavior change)

This is a structural refactor: add the second mask, add new APIs, rename old ones, migrate all call sites within `src/expand/`. The fix itself ships in Task 2. After this task, all existing unit/integration/E2E tests must still pass — the new `push_literal` method exists but has no caller yet.

**Files:**
- Modify: `src/expand/mod.rs:1-478`
- Modify: `src/expand/field_split.rs:1-466`
- Modify: `src/expand/pathname.rs:1-100` (head)
- Modify: `src/expand/pipeline.rs:1-247`

### Steps

- [ ] **Step 1.1: Pin current behavior with a regression-baseline check**

Confirm the SP3 #1 bug currently exists. This is the failing case we will fix in Task 2:

Run: `cargo build 2>&1 | tail -3`
Expected: clean build (or already built).

Run: `printf 'IFS=:; printf "[%%s]\n" a::b\n' | ./target/debug/yosh 2>/dev/null`
Expected output (the bug):
```
[a]
[]
[b]
```

(Plugin-load warnings on stderr are unrelated and may appear.)

- [ ] **Step 1.2: Refactor `ExpandedField` struct + push methods + predicates**

Replace the struct definition, push methods, predicates, `all_quoted`, and `set_range` helper in `src/expand/mod.rs`. The new shape lives between `// ─── ExpandedField ────...` and the start of `impl Default for ExpandedField`.

Replace the section starting at `pub struct ExpandedField {` (current line ~27) through the end of `impl ExpandedField { ... }` (current line ~101) with:

```rust
/// A word that has been through parameter/command/arithmetic expansion.
/// Each byte has two independent attribute bits:
///   - `split_protected_mask`: byte must NOT be split on an IFS character
///     (set for quoted bytes and for `Literal`-origin bytes).
///   - `glob_protected_mask`: byte must NOT be treated as a glob metachar
///     (set for quoted bytes only; `Literal`-origin bytes are glob-subject).
///
/// The two masks are independent. The POSIX byte classification is:
///   - `push_quoted`   → split-protected, glob-protected, was_quoted=true
///   - `push_literal`  → split-protected, glob-subject  (POSIX: literal text)
///   - `push_expanded` → split-subject,   glob-subject  (POSIX: $var, $(...), $((...)))
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// Packed bitset: 1 bit per byte. Bit set = protected from IFS splitting.
    split_protected_mask: Vec<u64>,
    /// Packed bitset: 1 bit per byte. Bit set = protected from glob expansion.
    glob_protected_mask: Vec<u64>,
    /// True if any quoting context applied to this field (even if value is empty).
    /// POSIX requires that quoted empty strings like `''` and `""` produce a
    /// zero-length field rather than being removed.
    pub was_quoted: bool,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            split_protected_mask: Vec::new(),
            glob_protected_mask: Vec::new(),
            was_quoted: false,
        }
    }

    /// True iff byte `i` must not be split on an IFS character.
    pub fn is_split_protected(&self, byte_index: usize) -> bool {
        bit_set(&self.split_protected_mask, byte_index)
    }

    /// True iff byte `i` must not be treated as a glob metacharacter.
    pub fn is_glob_protected(&self, byte_index: usize) -> bool {
        bit_set(&self.glob_protected_mask, byte_index)
    }

    /// Append `s` from a quoted context (single quotes, double quotes,
    /// escaped chars, tilde expansion). Bytes are protected from both
    /// field splitting and glob expansion; `was_quoted` becomes true.
    pub fn push_quoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        set_mask_range(&mut self.split_protected_mask, start, s.len());
        set_mask_range(&mut self.glob_protected_mask, start, s.len());
        self.was_quoted = true;
    }

    /// Append `s` from a literal (un-quoted, non-expansion) text part.
    /// Bytes are protected from field splitting (POSIX XCU §2.6.5 restricts
    /// splitting to expansion results only) but remain glob-subject so that
    /// a literal `*.rs` still triggers pathname expansion.
    pub fn push_literal(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        set_mask_range(&mut self.split_protected_mask, start, s.len());
        // glob_protected_mask intentionally not touched: literal bytes remain
        // glob-subject.
    }

    /// Append `s` from an expansion (parameter, command sub, arithmetic).
    /// Bytes are subject to both field splitting and glob expansion.
    pub fn push_expanded(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        // Neither mask updated: both default 0 (subject) bits are correct.
        // We still need to ensure mask length covers `value.len()` if a later
        // caller queries `is_*_protected` past the previous mask end —
        // the predicates fall back to false (subject) when reading past the
        // mask, so no explicit resize is needed for correctness.
        let _ = start;
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Create a field with all bytes marked protected from both splitting
    /// and glob expansion (used for glob-match results that must not be
    /// re-split or re-globbed).
    pub fn all_quoted(value: String) -> Self {
        let len = value.len();
        let needed_words = len.div_ceil(64);
        let mask = vec![u64::MAX; needed_words];
        Self {
            value,
            split_protected_mask: mask.clone(),
            glob_protected_mask: mask,
            was_quoted: false,
        }
    }
}

#[inline]
fn bit_set(mask: &[u64], byte_index: usize) -> bool {
    let word = byte_index / 64;
    let bit = byte_index % 64;
    mask.get(word).is_some_and(|w| w & (1u64 << bit) != 0)
}

#[inline]
fn set_mask_range(mask: &mut Vec<u64>, start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let end = start + len;
    let needed_words = end.div_ceil(64);
    if mask.len() < needed_words {
        mask.resize(needed_words, 0);
    }
    for i in start..end {
        mask[i / 64] |= 1u64 << (i % 64);
    }
}
```

The old `set_range` private method on `ExpandedField` is removed (its logic is folded into `set_mask_range` and used by both push methods). The old `is_quoted` and `push_unquoted` methods are removed entirely; call-site updates follow in Step 1.4.

- [ ] **Step 1.3: Update `src/expand/mod.rs` tests to use new API names**

The existing test module at the bottom of `src/expand/mod.rs` (current line ~157 onward) does not call `push_unquoted`, `push_quoted`, or `is_quoted` directly — they exercise `expand_word` / `expand_word_to_string` which go through `pipeline.rs`. So this file's tests need no change.

Verify by running: `cargo test --lib expand::tests 2>&1 | tail -20`
Expected: all tests pass after Step 1.2 (since `pipeline.rs` still uses the old method names; that update happens in Step 1.5). **This step will likely fail compile until Step 1.5 is also done.** Defer running tests until Step 1.5.

- [ ] **Step 1.4: Migrate `src/expand/field_split.rs` callers + update `append_char` routing**

Two changes in this file:

(a) Replace `is_quoted` calls with `is_split_protected` (3 sites in non-test code):

In `needs_splitting` (around line 186), change:
```rust
.any(|(i, b)| !field.is_quoted(i) && (ifs_ws.contains(&b) || ifs_nws.contains(&b)))
```
to:
```rust
.any(|(i, b)| !field.is_split_protected(i) && (ifs_ws.contains(&b) || ifs_nws.contains(&b)))
```

In `split_field` (around line 105), change:
```rust
let quoted = field.is_quoted(i);
```
to:
```rust
let quoted = field.is_split_protected(i);
```

(b) Rewrite `append_char` (around line 201-215) to route the byte into the destination using the correct push method based on the *source's* `(is_split_protected, is_glob_protected)` pair. This preserves literal-ness when a literal byte survives splitting (e.g., split-protected byte copied into a sub-field that pathname expansion will visit later):

Replace the body of `append_char`:
```rust
#[inline]
fn append_char(dest: &mut ExpandedField, source: &ExpandedField, i: usize) -> usize {
    let ch_len = source.value[i..]
        .chars()
        .next()
        .expect("i on char boundary")
        .len_utf8();
    let slice = &source.value[i..i + ch_len];
    let split_p = source.is_split_protected(i);
    let glob_p = source.is_glob_protected(i);
    match (split_p, glob_p) {
        (true, true) => dest.push_quoted(slice),
        (true, false) => dest.push_literal(slice),
        (false, false) => dest.push_expanded(slice),
        // Not produced by the current push API; routed as expanded for
        // forward compatibility. No test required.
        (false, true) => dest.push_expanded(slice),
    }
    ch_len
}
```

Update the doc-comment immediately above (around line 195-200) to remove the "All bytes of a single character share the same `quoted_mask` bit" wording and replace with:
```rust
/// Append the UTF-8 character starting at byte position `i` in `source` to
/// `dest`, preserving the byte's `(split_protected, glob_protected)` attribute
/// pair via the push method matching its origin classification.
///
/// Caller must ensure `i` is on a UTF-8 character boundary.
```

Also update test helpers in `field_split.rs` tests (around line 243-253):

The `unquoted` test helper uses `push_unquoted`. Rename:
```rust
fn unquoted(s: &str) -> ExpandedField {
    let mut f = ExpandedField::new();
    f.push_expanded(s);
    f
}
```

The `quoted_field` test helper stays the same (still calls `push_quoted`).

And update the `test_empty_ifs_drops_empty_fields` test (around line 311-316):
```rust
let mut empty = ExpandedField::new();
empty.push_expanded("");
```

And `test_fast_path_empty_unquoted_field_preserved` (around line 400):
```rust
let mut empty = ExpandedField::new();
empty.push_expanded("");
```

And `test_mixed_quoted_unquoted` (around line 333-337):
```rust
let mut f = ExpandedField::new();
f.push_expanded("foo ");
f.push_quoted("bar baz");
f.push_expanded(" qux");
```

And `test_fast_path_mixed_quoted_unquoted_no_ifs` (around line 367-373):
```rust
let mut f = ExpandedField::new();
f.push_expanded("foo");
f.push_quoted("bar");
```

And `test_slow_path_triggered_by_one_splittable_field` uses the `unquoted` helper — no change beyond the helper rename in this step.

- [ ] **Step 1.5: Migrate `src/expand/pathname.rs` caller**

Change `is_quoted` to `is_glob_protected` in `has_unquoted_glob_chars` (around line 49):
```rust
if !field.is_glob_protected(i) && matches!(b, b'*' | b'?' | b'[') {
    return true;
}
```

Also update the inline tests at the bottom of `pathname.rs` (around line 202-208) that use `push_unquoted`:
```rust
f.push_expanded(s);
```
(Apply to both helper calls.)

- [ ] **Step 1.6: Migrate `src/expand/pipeline.rs` callers (renames only — the behavioral fix is Task 2)**

In `pipeline.rs`, replace every `push_unquoted` call with `push_expanded`:

`expand_part_literal` (around line 50-56) — **keep the un-quoted branch on `push_expanded` for now**; Task 2 changes it to `push_literal`:
```rust
fn expand_part_literal(s: &str, fields: &mut [ExpandedField], in_double_quote: bool) {
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(s);
    } else {
        fields.last_mut().unwrap().push_expanded(s);
    }
}
```

`expand_part_command_sub` (around line 98-110):
```rust
fn expand_part_command_sub(
    env: &mut ShellEnv,
    program: &crate::parser::ast::Program,
    fields: &mut [ExpandedField],
    in_double_quote: bool,
) {
    let output = command_sub::execute(env, program);
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(&output);
    } else {
        fields.last_mut().unwrap().push_expanded(&output);
    }
}
```

`expand_part_arith_sub` (around line 112-132):
```rust
fields.last_mut().unwrap().push_expanded(&result);
```
(in the unquoted branch around line 123)

Unquoted `$@` (around line 172-185):
```rust
for (i, p) in params.iter().enumerate() {
    if i == 0 {
        fields.last_mut().unwrap().push_expanded(p);
    } else {
        fields.push(ExpandedField::new());
        fields.last_mut().unwrap().push_expanded(p);
    }
}
```

Default case (around line 188-195):
```rust
_ => {
    let value = param::expand(env, param)?;
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(&value);
    } else {
        fields.last_mut().unwrap().push_expanded(&value);
    }
}
```

Update tests in `pipeline.rs` (around line 214-245) that use `is_quoted`:
```rust
assert!((0..fields[0].value.len()).all(|i| !fields[0].is_split_protected(i)));
assert!((0..fields[1].value.len()).all(|i| !fields[1].is_split_protected(i)));
assert!((0..fields[2].value.len()).all(|i| !fields[2].is_split_protected(i)));
```

- [ ] **Step 1.7: Verify the workspace compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: clean build, no warnings about unused methods (everything is wired up).

If there are compile errors about unresolved `is_quoted` / `push_unquoted` / `quoted_mask`, grep the codebase outside `src/expand/` to confirm no external caller exists:

Run: `grep -rn "is_quoted\|push_unquoted\|quoted_mask" /Users/kazukiyamamoto/Projects/rust/kish/src/ /Users/kazukiyamamoto/Projects/rust/kish/tests/ --include="*.rs" | grep -v "// "`
Expected: no matches outside the lines you've already edited.

- [ ] **Step 1.8: Run all unit + integration tests, confirm zero regressions**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: all tests pass (numbers should match the count from `git stash; cargo test --lib; git stash pop` baseline — but if you don't have a baseline handy, "no failures, no errors" is the gate).

Run: `cargo test 2>&1 | tail -30`
Expected: all integration tests pass.

Run: `./e2e/run_tests.sh 2>&1 | tail -20`
Expected: same pass/fail/XFAIL counts as before. The bug under SP3 #1 is still present (not fixed yet) — no test currently exercises it as PASS, so the counts are unchanged.

- [ ] **Step 1.9: Add baseline unit tests for the new push API**

These tests document the contract of the new APIs and serve as the unit-level pin for Task 2. They pass immediately under Task 1 because `push_literal` itself works correctly — only its wiring in `pipeline.rs` is missing.

Add to the `tests` module in `src/expand/mod.rs` (after the last existing test, before the closing `}` of `mod tests`):

```rust
    #[test]
    fn push_literal_marks_split_protected_only() {
        let mut f = ExpandedField::new();
        f.push_literal("ab");
        assert!(f.is_split_protected(0));
        assert!(f.is_split_protected(1));
        assert!(!f.is_glob_protected(0));
        assert!(!f.is_glob_protected(1));
        assert!(!f.was_quoted);
    }

    #[test]
    fn push_quoted_marks_both_and_was_quoted() {
        let mut f = ExpandedField::new();
        f.push_quoted("ab");
        assert!(f.is_split_protected(0));
        assert!(f.is_split_protected(1));
        assert!(f.is_glob_protected(0));
        assert!(f.is_glob_protected(1));
        assert!(f.was_quoted);
    }

    #[test]
    fn push_expanded_marks_neither() {
        let mut f = ExpandedField::new();
        f.push_expanded("ab");
        assert!(!f.is_split_protected(0));
        assert!(!f.is_split_protected(1));
        assert!(!f.is_glob_protected(0));
        assert!(!f.is_glob_protected(1));
        assert!(!f.was_quoted);
    }

    #[test]
    fn mixed_push_per_byte_independence() {
        // L|E|Q sequence — verify each byte keeps its own attributes
        let mut f = ExpandedField::new();
        f.push_literal("a");
        f.push_expanded("b");
        f.push_quoted("c");
        assert_eq!(f.value, "abc");
        assert!(f.is_split_protected(0) && !f.is_glob_protected(0)); // a: literal
        assert!(!f.is_split_protected(1) && !f.is_glob_protected(1)); // b: expanded
        assert!(f.is_split_protected(2) && f.is_glob_protected(2)); // c: quoted
        assert!(f.was_quoted);
    }

    #[test]
    fn all_quoted_marks_both() {
        let f = ExpandedField::all_quoted("abc".to_string());
        for i in 0..3 {
            assert!(f.is_split_protected(i), "byte {i} split-protected", i = i);
            assert!(f.is_glob_protected(i), "byte {i} glob-protected", i = i);
        }
        assert!(!f.was_quoted);
    }
```

Run: `cargo test --lib expand::tests 2>&1 | tail -10`
Expected: all 5 new tests pass, plus all existing expand tests.

- [ ] **Step 1.10: Add unit tests pinning the glob-vs-quoted distinction in `pathname.rs`**

These tests document that the `is_glob_protected` predicate (not `is_split_protected`) is what gates glob expansion, and that the two predicates can diverge on literal-origin bytes.

Append to the `tests` module in `src/expand/pathname.rs` (before the closing `}` of `mod tests`):

```rust
    #[test]
    fn literal_asterisk_still_globs() {
        // POSIX: literal `*` triggers pathname expansion. push_literal
        // marks bytes split-protected but glob-subject — glob still fires.
        let mut f = ExpandedField::new();
        f.push_literal("*.rs");
        assert!(
            has_unquoted_glob_chars(&f),
            "literal '*' must be glob-subject"
        );
    }

    #[test]
    fn quoted_asterisk_does_not_glob() {
        // Regression pin: push_quoted marks bytes both split- and
        // glob-protected, so `*` is treated as literal.
        let mut f = ExpandedField::new();
        f.push_quoted("*.rs");
        assert!(
            !has_unquoted_glob_chars(&f),
            "quoted '*' must NOT be glob-subject"
        );
    }

    #[test]
    fn expanded_asterisk_globs() {
        // Sanity: expansion result with `*` is also glob-subject.
        let mut f = ExpandedField::new();
        f.push_expanded("*.rs");
        assert!(
            has_unquoted_glob_chars(&f),
            "expanded '*' must be glob-subject"
        );
    }
```

Run: `cargo test --lib expand::pathname::tests 2>&1 | tail -10`
Expected: all 3 new tests pass, plus existing pathname tests.

- [ ] **Step 1.11: Commit the refactor**

Run:
```bash
git add src/expand/mod.rs src/expand/field_split.rs src/expand/pathname.rs src/expand/pipeline.rs
git status --short
```
Expected: only those 4 files staged (no other modifications).

Commit:
```bash
git commit -m "$(cat <<'EOF'
refactor(expand): split ExpandedField mask into split- vs glob-protected

Introduce push_literal API that marks bytes as split-protected but
glob-subject, alongside renamed push_expanded (was push_unquoted) and
unchanged push_quoted. The two per-byte masks (split_protected_mask,
glob_protected_mask) become independent, enabling the three POSIX byte
origins (quoted, literal, expansion) to be classified correctly.

This is a pure refactor: push_literal has no caller yet, so all
existing behavior is preserved. The wiring change that fixes SP3
follow-up #1 (literal argv word-splitting bug) ships in the next commit.

Spec: docs/superpowers/specs/2026-05-23-literal-argv-word-splitting-fix-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Apply the fix (1-line wiring change) + add failing-then-passing tests

**Files:**
- Modify: `src/expand/pipeline.rs` (1 line)
- Modify: `src/expand/field_split.rs` (add unit tests)
- Create: `e2e/posix_spec/2_06_05_field_splitting/literal_argv_not_split.sh`

### Steps

- [ ] **Step 2.1: Add the E2E test that reproduces SP3 #1**

Create `e2e/posix_spec/2_06_05_field_splitting/literal_argv_not_split.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal argv tokens are not subject to IFS field splitting (XCU §2.6.5 restricts splitting to expansion results)
# EXPECT_OUTPUT: [a::b]
# EXPECT_EXIT: 0
IFS=:
printf "[%s]\n" a::b
```

Set the correct permissions (E2E convention from CLAUDE.md):
```bash
chmod 644 e2e/posix_spec/2_06_05_field_splitting/literal_argv_not_split.sh
```

- [ ] **Step 2.2: Verify the E2E test currently FAILS**

Run: `./e2e/run_tests.sh --filter=literal_argv_not_split 2>&1 | tail -30`
Expected: FAIL — yosh outputs `[a]\n[]\n[b]` instead of `[a::b]`.

This pins the bug at the E2E layer. The next step will make it pass.

- [ ] **Step 2.3: Add a failing unit test in `field_split.rs`**

Append to the `tests` module in `src/expand/field_split.rs` (before the closing `}` of `mod tests`):

```rust
    // ── Literal bytes are not split (POSIX XCU §2.6.5) ──

    #[test]
    fn test_literal_colon_not_split() {
        // SP3 follow-up #1: literal text must not be field-split.
        // Without push_literal wiring in pipeline.rs, the equivalent
        // unit-level check exercises the field_split predicate directly.
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("a::b");
        assert_eq!(values(split(&env, vec![f])), vec!["a::b"]);
    }

    #[test]
    fn test_literal_then_expansion_split_only_in_expansion() {
        // Mixed origin: literal "a::" (protected) + expansion "b:c" (subject).
        // POSIX result: ["a::b", "c"].
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("a::");
        f.push_expanded("b:c");
        assert_eq!(values(split(&env, vec![f])), vec!["a::b", "c"]);
    }

    #[test]
    fn test_literal_then_expansion_then_literal_round_trip() {
        // L "x:" + E "a:b" + L ":y" with IFS=":"
        // Expected fields: ["x:a", "b", ":y"]
        //   - literal "x:" — both bytes split-protected, stay together
        //   - expansion "a:b" — colon splits → "a" then start of next field "b"
        //   - literal ":y" — both bytes split-protected, stay together with prior "b"
        let env = env_with_ifs(":");
        let mut f = ExpandedField::new();
        f.push_literal("x:");
        f.push_expanded("a:b");
        f.push_literal(":y");
        assert_eq!(values(split(&env, vec![f])), vec!["x:a", "b:y"]);
    }
```

Run: `cargo test --lib expand::field_split::tests::test_literal_colon_not_split 2>&1 | tail -10`
Expected: PASS. The unit-level fix already works because `push_literal` itself is correct (the bug is only in `pipeline.rs`'s call site choice). The E2E from Step 2.1 still FAILS until Step 2.4.

Run: `cargo test --lib expand::field_split::tests::test_literal_then_expansion 2>&1 | tail -10`
Expected: both new tests PASS.

(Note: these unit tests pass under Task 1 already because `push_literal` is correctly implemented. They serve as the contract pin for the API. The E2E test is what fails until Step 2.4.)

- [ ] **Step 2.4: Apply the 1-line fix in `pipeline.rs`**

In `src/expand/pipeline.rs`, locate `expand_part_literal` (around line 50-56) and change the un-quoted branch from `push_expanded` to `push_literal`:

```rust
fn expand_part_literal(s: &str, fields: &mut [ExpandedField], in_double_quote: bool) {
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(s);
    } else {
        fields.last_mut().unwrap().push_literal(s);
    }
}
```

This is the entire bug fix.

- [ ] **Step 2.5: Verify the E2E test now PASSES**

Build and run:
```bash
cargo build 2>&1 | tail -3 && ./e2e/run_tests.sh --filter=literal_argv_not_split 2>&1 | tail -10
```
Expected: PASS.

Manual smoke also:
```bash
printf 'IFS=:; printf "[%%s]\n" a::b\n' | ./target/debug/yosh 2>/dev/null
```
Expected: `[a::b]`.

- [ ] **Step 2.6: Run the full unit + integration suite — confirm no regressions**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: all tests pass.

Run: `cargo test 2>&1 | tail -30`
Expected: all integration tests pass. The `read` round-trip path (`tests/...`) must still work since `push_expanded` semantics on `$var` and `$(...)` are unchanged.

Sanity check the `read` path manually:
```bash
printf 'printf "a::b\n" | { IFS=:; read x y z; printf "[%%s][%%s][%%s]\n" "$x" "$y" "$z"; }\n' | ./target/debug/yosh 2>/dev/null
```
Expected: `[a][][b]` (read does its own IFS splitting; unchanged behavior).

- [ ] **Step 2.7: Run the full E2E suite — confirm pass/fail/XFAIL counts**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: pass count increases by exactly 1 (the new `literal_argv_not_split.sh`); fail and XFAIL counts unchanged.

If any unrelated test goes from PASS to FAIL, investigate before continuing — likely a hidden dependency on the broken behavior.

- [ ] **Step 2.8: Commit the fix**

```bash
git add src/expand/pipeline.rs src/expand/field_split.rs e2e/posix_spec/2_06_05_field_splitting/literal_argv_not_split.sh
git commit -m "$(cat <<'EOF'
fix(expand): protect literal argv tokens from field splitting

POSIX XCU §2.6.5 restricts field splitting to results of parameter,
command, and arithmetic expansion — never literal text. yosh previously
routed Literal WordParts through push_unquoted (now push_expanded),
making them eligible for IFS splitting; this caused literal argv tokens
like a::b with IFS=":" to expand to three fields.

Fix: in expand_part_literal's un-quoted branch, call push_literal
(introduced in the previous refactor) instead of push_expanded. Literal
bytes become split-protected while remaining glob-subject (so a literal
*.rs continues to glob-expand correctly).

Closes SP3 follow-up #1 in TODO.md.

Spec: docs/superpowers/specs/2026-05-23-literal-argv-word-splitting-fix-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add E2E behavioral coverage from spec §6

Expand the E2E suite to lock down the full behavior matrix from the spec, preventing future regressions in any of the four orthogonal cases.

**Files:**
- Create: `e2e/posix_spec/2_06_05_field_splitting/literal_with_consecutive_nws_ifs.sh`
- Create: `e2e/posix_spec/2_06_05_field_splitting/literal_mixed_with_expansion.sh`
- Create: `e2e/posix_spec/2_06_05_field_splitting/literal_glob_metachar_still_globs.sh`

### Steps

- [ ] **Step 3.1: Add adjacent-NWS-IFS literal test**

Create `e2e/posix_spec/2_06_05_field_splitting/literal_with_consecutive_nws_ifs.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Multiple adjacent non-whitespace IFS chars in a literal stay intact (no empty fields)
# EXPECT_OUTPUT: [a:b:c]
# EXPECT_EXIT: 0
IFS=:
printf "[%s]\n" a:b:c
```

```bash
chmod 644 e2e/posix_spec/2_06_05_field_splitting/literal_with_consecutive_nws_ifs.sh
```

Run: `./e2e/run_tests.sh --filter=literal_with_consecutive_nws_ifs 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 3.2: Add mixed literal + expansion test**

Create `e2e/posix_spec/2_06_05_field_splitting/literal_mixed_with_expansion.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal text stays intact; only $var expansion is split (one printf line per argv field)
# EXPECT_OUTPUT: [a::b]
# [x]
# [y]
# [c::d]
# EXPECT_EXIT: 0
IFS=:
v=x:y
printf "[%s]\n" a::b $v c::d
```

```bash
chmod 644 e2e/posix_spec/2_06_05_field_splitting/literal_mixed_with_expansion.sh
```

Run: `./e2e/run_tests.sh --filter=literal_mixed_with_expansion 2>&1 | tail -5`
Expected: PASS.

If the test runner does not support multi-line `EXPECT_OUTPUT`, replace with a counting variant:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal text stays intact; only $var expansion is split (4 argv fields total)
# EXPECT_OUTPUT: 4
# EXPECT_EXIT: 0
IFS=:
v=x:y
set -- a::b $v c::d
echo $#
```
(Inspect `e2e/run_tests.sh` `EXPECT_OUTPUT` parsing if uncertain; the multi-line form is preferred for clarity.)

- [ ] **Step 3.3: Add literal-glob-metachar test**

Create `e2e/posix_spec/2_06_05_field_splitting/literal_glob_metachar_still_globs.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal * still triggers pathname expansion (field splitting protection does not imply glob protection)
# EXPECT_OUTPUT: Cargo.toml
# EXPECT_EXIT: 0
# This test assumes the runner CWD contains Cargo.toml (true for yosh repo root).
IFS=:
cd "$(dirname "$0")/../../.."
echo *.toml
```

```bash
chmod 644 e2e/posix_spec/2_06_05_field_splitting/literal_glob_metachar_still_globs.sh
```

Run: `./e2e/run_tests.sh --filter=literal_glob_metachar 2>&1 | tail -10`
Expected: PASS.

If the runner does not preserve CWD or the relative path is unreliable, simplify by creating a temp dir:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal * still triggers pathname expansion under IFS=:
# EXPECT_OUTPUT: a.tmpext
# EXPECT_EXIT: 0
IFS=:
d=$(mktemp -d)
trap 'rm -rf "$d"' EXIT
touch "$d/a.tmpext"
cd "$d"
echo *.tmpext
```

- [ ] **Step 3.4: Run full E2E and confirm clean state**

Run: `./e2e/run_tests.sh 2>&1 | tail -5`
Expected: pass count up by 3 (or 4 cumulative with Task 2's), no new failures, XFAIL count unchanged.

- [ ] **Step 3.5: Remove the closed TODO entry**

Per CLAUDE.md (`TODO.md` format), delete completed items rather than marking `[x]`. Remove the SP3 #1 entry from `TODO.md`:

Open `TODO.md`, locate the SP3 section, and delete the bullet that starts:
```
- [ ] Word-splitting applied to literal command argv tokens — uncovered
      during SP3 Task 5 manual smoke. ...
```
(7 indented lines through `Verified read itself is unaffected.`)

Verify: `grep -n "literal command argv" TODO.md`
Expected: no output.

- [ ] **Step 3.6: Final commit**

```bash
git add e2e/posix_spec/2_06_05_field_splitting/literal_with_consecutive_nws_ifs.sh \
        e2e/posix_spec/2_06_05_field_splitting/literal_mixed_with_expansion.sh \
        e2e/posix_spec/2_06_05_field_splitting/literal_glob_metachar_still_globs.sh \
        TODO.md
git commit -m "$(cat <<'EOF'
test(e2e): add behavioral coverage for literal-argv field-splitting fix

Lock down the four orthogonal cases from the design spec §6 behavior
matrix:
- adjacent non-whitespace IFS chars in literal stay intact
- mixed literal + $var expansion splits only the expansion region
- literal * still triggers pathname expansion (split-protection !=
  glob-protection)

Also remove the now-closed SP3 follow-up #1 entry from TODO.md per
the project's "delete completed items" convention.

Spec: docs/superpowers/specs/2026-05-23-literal-argv-word-splitting-fix-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Verification Checklist

After all three tasks complete:

- [ ] `cargo build` — clean
- [ ] `cargo test --lib` — all pass
- [ ] `cargo test` — all integration pass
- [ ] `./e2e/run_tests.sh` — pass count up by 4 vs HEAD (1 from Task 2 + 3 from Task 3); fail and XFAIL counts unchanged
- [ ] `grep -rn "is_quoted\|push_unquoted\|quoted_mask" src/ tests/ --include="*.rs"` — no matches (full migration)
- [ ] `grep "literal command argv" TODO.md` — no matches (closed)
- [ ] `git log --oneline -3` — three commits: refactor, fix, e2e coverage
