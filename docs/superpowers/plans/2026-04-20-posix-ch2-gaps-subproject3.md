# POSIX §2.6.1 Mixed-WordPart Tilde Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `try_parse_assignment` so tildes after an unquoted `:` are recognized across `WordPart` boundaries (e.g. `x=$var:~/bin`), while leaving `~` after a non-Literal part with no intervening `:` as a plain literal per POSIX.

**Architecture:** Single-file change to `src/parser/mod.rs`. Extend `split_tildes_in_literal` with a `start_at_boundary` flag and a returned `ends_with_colon` flag. Walk every `WordPart` of the assignment value, tracking segment-boundary state and calling the extended splitter on each `Literal`. Add 4 E2E tests and flip one existing unit test that currently locks in the buggy behavior.

**Tech Stack:** Rust 2024 edition, yosh parser (`src/parser/mod.rs`), existing `e2e/run_tests.sh` harness for POSIX E2E.

**Spec:** `docs/superpowers/specs/2026-04-20-posix-ch2-gaps-subproject3-design.md`

---

## Prerequisites (before Task 1)

- [ ] **Step 0.1: Build the debug binary**

```bash
cargo build
```
Expected: clean build (7 pre-existing warnings unrelated to this work).

- [ ] **Step 0.2: Record baseline**

```bash
./e2e/run_tests.sh 2>&1 | tail -3
cargo test --lib 2>&1 | tail -5
```
Expected:
- E2E: `Total: 364  Passed: 363  Failed: 0  XFail: 1`
- Lib: `test result: ok. 620 passed`

If counts differ, stop and reconcile before proceeding.

- [ ] **Step 0.3: Verify the buggy behavior is currently locked in**

Confirm the parser ships with a unit test that asserts the bug as current behavior:

```bash
grep -n 'assignment_rhs_parameter_then_tilde_not_expanded' src/parser/mod.rs
```
Expected: matches one test definition (currently at `src/parser/mod.rs:1558`). This test WILL be rewritten in Task 2 Step 2.2 to assert the corrected behavior.

---

## Task 1 (Commit ①): Extend `split_tildes_in_literal` with boundary flag

**Files:**
- Modify: `src/parser/mod.rs` (function `split_tildes_in_literal` at lines ~920–975, its unit tests at ~1401–1497, and its single external call site at ~361)

### Step 1.1: Update the 13 existing `split_tildes_*` unit-test call sites

The tests currently call `split_tildes_in_literal(s)` and compare the result to `Vec<WordPart>`. After the signature change, each call must:
- Pass `true` as the second argument (preserving current "starts at boundary" semantics)
- Unwrap the returned tuple via `.0`

- [ ] Edit `src/parser/mod.rs` to update each of the 13 tests. The test bodies are at approximately:
  - `split_no_tilde_returns_single_literal` (line ~1401)
  - `split_leading_tilde_only` (line ~1406)
  - `split_leading_tilde_slash` (line ~1411)
  - `split_leading_tilde_user` (line ~1419)
  - `split_colon_separated_tildes` (line ~1427)
  - `split_middle_segment_with_tilde` (line ~1440)
  - `split_trailing_colon` (line ~1448)
  - `split_leading_colon` (line ~1456)
  - `split_consecutive_colons` (line ~1464)
  - `split_mid_word_tilde_stays_literal` (line ~1472)
  - `split_double_tilde_invalid_user` (line ~1477)
  - `split_user_name_with_dot_and_dash` (line ~1482)
  - `split_two_tildes_joined_by_colon_no_slash` (line ~1490)

Example transformation — take the first test:
```rust
// BEFORE
assert_eq!(split_tildes_in_literal("foo/bar"), vec![lit("foo/bar")]);

// AFTER
assert_eq!(split_tildes_in_literal("foo/bar", true).0, vec![lit("foo/bar")]);
```

Apply the same `(input, true).0` transformation to each of the 13 test call sites. Do not change the `vec![...]` expected values — those must remain the same (behavior is preserved by `start_at_boundary = true`).

### Step 1.2: Update the single non-test call site in `try_parse_assignment`

- [ ] Edit `src/parser/mod.rs` around line 361. The current code:
```rust
if !after_eq.is_empty() {
    value_parts.extend(split_tildes_in_literal(after_eq));
}
```
Change to (matching the new tuple-returning signature; Task 2 will replace this block entirely, but for Task 1 we only need the call to compile):
```rust
if !after_eq.is_empty() {
    value_parts.extend(split_tildes_in_literal(after_eq, true).0);
}
```

### Step 1.3: Change the function signature and body

- [ ] Replace the current `split_tildes_in_literal` (at approximately lines 921–975 of `src/parser/mod.rs`) with:

```rust
/// Scan a literal assignment-value segment and promote unquoted
/// tilde-prefixes at segment boundaries into `WordPart::Tilde` nodes.
/// Segments are delimited by `:` so that forms like `PATH=~/a:~/b`
/// expand at both tildes (POSIX §2.6.1).
///
/// If `start_at_boundary` is true, the first segment is eligible for
/// tilde recognition (as when `s` comes directly after `=` or a
/// preceding Literal that ended with `:`). If false, the leading `~`
/// (if any) is treated as a literal character. Internal `:` always
/// starts a new segment at a boundary regardless of `start_at_boundary`.
///
/// Returns the produced AST parts together with a flag indicating
/// whether `s` ended on an unquoted `:` — callers walking a multi-part
/// word use this flag to decide whether the NEXT `WordPart::Literal`
/// begins at a segment boundary.
///
/// Tildes inside quoted, escaped, or substituted parts must never
/// reach this function.
pub(crate) fn split_tildes_in_literal(
    s: &str,
    start_at_boundary: bool,
) -> (Vec<ast::WordPart>, bool) {
    use ast::WordPart;

    fn is_name_safe(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-'
    }

    let mut out: Vec<WordPart> = Vec::new();
    let push_literal = |out: &mut Vec<WordPart>, s: &str| {
        if s.is_empty() {
            return;
        }
        if let Some(WordPart::Literal(last)) = out.last_mut() {
            last.push_str(s);
        } else {
            out.push(WordPart::Literal(s.to_string()));
        }
    };

    for (i, segment) in s.split(':').enumerate() {
        if i > 0 {
            push_literal(&mut out, ":");
        }
        let eligible = if i == 0 { start_at_boundary } else { true };
        if eligible
            && let Some(rest_after_tilde) = segment.strip_prefix('~')
        {
            let (user, tail) = match rest_after_tilde.find('/') {
                Some(p) => (&rest_after_tilde[..p], &rest_after_tilde[p..]),
                None => (rest_after_tilde, ""),
            };
            if user.is_empty() || user.chars().all(is_name_safe) {
                if user.is_empty() {
                    out.push(WordPart::Tilde(None));
                } else {
                    out.push(WordPart::Tilde(Some(user.to_string())));
                }
                if !tail.is_empty() {
                    push_literal(&mut out, tail);
                }
                continue;
            }
            // Fall through: segment stays as a plain literal
        }
        push_literal(&mut out, segment);
    }

    (out, s.ends_with(':'))
}
```

Key differences from the current body:
- Signature adds `start_at_boundary: bool`
- Return type is `(Vec<WordPart>, bool)`
- New `eligible` gate on the `strip_prefix('~')` branch — `false` when we're on the first segment and the caller said the position isn't at a boundary
- Final `return` becomes `(out, s.ends_with(':'))`

### Step 1.4: Add 3 new unit tests

- [ ] Insert the following three tests in `src/parser/mod.rs` **after** the last existing `split_tildes_*` test (`split_two_tildes_joined_by_colon_no_slash`, approximately line 1496) and **before** the `// ── try_parse_assignment integration ────────` separator:

```rust
    #[test]
    fn split_not_at_boundary_skips_leading_tilde() {
        assert_eq!(
            split_tildes_in_literal("~/bin", false),
            (vec![lit("~/bin")], false)
        );
    }

    #[test]
    fn split_not_at_boundary_then_colon_restarts() {
        assert_eq!(
            split_tildes_in_literal(":~/bin", false),
            (vec![lit(":"), WordPart::Tilde(None), lit("/bin")], false)
        );
    }

    #[test]
    fn split_returns_ends_with_colon_flag() {
        assert_eq!(
            split_tildes_in_literal("a:", true),
            (vec![lit("a:")], true)
        );
    }
```

### Step 1.5: Run the parser tests

- [ ] Run:
```bash
cargo test --lib -p yosh -- parser::tests::split 2>&1 | tail -20
```
Expected: 16 tests pass (13 existing + 3 new), 0 failed.

If any of the 13 existing tests fail: the call-site update in Step 1.1 missed a test. Re-inspect and fix.

### Step 1.6: Verify full lib suite and tilde E2E

- [ ] Run:
```bash
cargo test --lib 2>&1 | tail -5
./e2e/run_tests.sh --filter=tilde 2>&1 | tail -3
```
Expected:
- Lib: `test result: ok. 623 passed` (baseline 620 + 3 new tests)
- E2E: `Total: 14  Passed: 14  Failed: 0` (no regression on existing tilde tests — Task 1 preserves behavior because `start_at_boundary=true` is passed at the only call site)

### Step 1.7: Commit

- [ ] Run:
```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
refactor(parser): split_tildes_in_literal takes boundary flag

Extends split_tildes_in_literal to accept a start_at_boundary: bool
parameter and return (Vec<WordPart>, bool /* ends_with_colon */).
The flag gates tilde recognition on the first segment so callers
walking a multi-part Word can track whether the next Literal begins
at a colon boundary. Behavior unchanged for the existing call site,
which passes `true`.

No externally visible change yet; wires up the signature for the
subsequent mixed-WordPart fix.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 (Commit ②): Walk all WordParts in `try_parse_assignment`

**Files:**
- Modify: `src/parser/mod.rs`
  - `try_parse_assignment` body (~lines 358–368)
  - existing unit test `assignment_rhs_parameter_then_tilde_not_expanded` (~line 1558) — flipped to assert the corrected behavior and renamed

### Step 2.1: Add new failing unit tests

- [ ] Insert the following tests in `src/parser/mod.rs` after `assignment_rhs_parameter_then_tilde_not_expanded` (around line 1562), before the next `// ── empty compound_list rejection` block:

```rust
    #[test]
    fn assignment_rhs_param_then_colon_tilde_expands() {
        let (name, parts) = parse_first_assignment("x=$var:~/bin\n").unwrap();
        assert_eq!(name, "x");
        use ast::ParamExpr;
        assert_eq!(
            parts,
            vec![
                WordPart::Parameter(ParamExpr::Simple("var".to_string())),
                lit(":"),
                WordPart::Tilde(None),
                lit("/bin"),
            ]
        );
    }

    #[test]
    fn assignment_rhs_param_then_tilde_no_colon_stays_literal() {
        let (name, parts) = parse_first_assignment("x=$var~/bin\n").unwrap();
        assert_eq!(name, "x");
        use ast::ParamExpr;
        assert_eq!(
            parts,
            vec![
                WordPart::Parameter(ParamExpr::Simple("var".to_string())),
                lit("~/bin"),
            ]
        );
    }
```

### Step 2.2: Rewrite the existing bug-locking test

The current `assignment_rhs_parameter_then_tilde_not_expanded` at line 1558 locks in the buggy behavior ("no Tilde when Parameter precedes `:~`"). After our fix, `x=$var:~/bin` WILL produce a `Tilde` — this test must be rewritten.

- [ ] Replace the entire test (from `#[test]` line through the closing `}`) with:

```rust
    #[test]
    fn assignment_rhs_param_then_tilde_expands_after_colon() {
        // POSIX §2.6.1: a tilde-prefix is recognized after `=` and after any
        // unquoted `:` in an assignment value. The colon inside a trailing
        // Literal that follows a Parameter expansion still counts as a
        // segment boundary, so the tilde expands.
        let (_, parts) = parse_first_assignment("x=$var:~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(has_tilde, "parts = {:?}", parts);
    }
```

The new name and comment document the *corrected* POSIX behavior. The new assertion `assert!(has_tilde)` fails today and will pass after Task 2 Step 2.3.

### Step 2.3: Run tests to confirm they fail

- [ ] Run:
```bash
cargo test --lib -- parser::tests::assignment_rhs_param_then 2>&1 | tail -20
```
Expected: all three of
- `assignment_rhs_param_then_colon_tilde_expands`
- `assignment_rhs_param_then_tilde_no_colon_stays_literal`
- `assignment_rhs_param_then_tilde_expands_after_colon`

FAIL with assertion-mismatch output. This confirms the bug exists and the tests detect it.

### Step 2.4: Replace the `try_parse_assignment` value-construction block

- [ ] In `src/parser/mod.rs`, find the block starting at approximately line 358:

```rust
        // Build value word
        let mut value_parts = Vec::new();
        if !after_eq.is_empty() {
            value_parts.extend(split_tildes_in_literal(after_eq, true).0);
        }
        value_parts.extend_from_slice(remaining_parts);
```

Replace it with:

```rust
        // Build value word with boundary-aware tilde splitting across all parts.
        // The segment boundary starts true (we just consumed '='), is tracked
        // through subsequent Literal parts via the returned ends_with_colon flag,
        // and is reset to false whenever a non-Literal part (Parameter, CommandSub,
        // quoted content) appears — because such parts cannot contain an unquoted
        // `:` in the AST (quoted `:` is inside SingleQuoted/DoubleQuoted variants).
        let mut value_parts = Vec::new();
        let mut at_boundary = true;
        if !after_eq.is_empty() {
            let (parts, ends_colon) = split_tildes_in_literal(after_eq, at_boundary);
            value_parts.extend(parts);
            at_boundary = ends_colon;
        }
        for part in remaining_parts {
            match part {
                WordPart::Literal(s) => {
                    let (parts, ends_colon) = split_tildes_in_literal(s, at_boundary);
                    value_parts.extend(parts);
                    at_boundary = ends_colon;
                }
                other => {
                    value_parts.push(other.clone());
                    at_boundary = false;
                }
            }
        }
```

### Step 2.5: Run the three failing tests

- [ ] Run:
```bash
cargo test --lib -- parser::tests::assignment_rhs_param_then 2>&1 | tail -20
```
Expected: all three PASS.

### Step 2.6: Full parser regression check

- [ ] Run:
```bash
cargo test --lib 2>&1 | tail -5
./e2e/run_tests.sh --filter=tilde 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected:
- Lib: `test result: ok. 625 passed` (620 baseline + 3 Task 1 + 2 Task 2)
- Tilde E2E: `Total: 14  Passed: 14  Failed: 0`
- Full E2E: `Total: 364  Passed: 363  Failed: 0  XFail: 1` (baseline; Task 2 adds no E2E files yet)

If any existing tilde test fails: the multi-part traversal altered behavior for a single-Literal word. Investigate `push_literal` merging — the new code path for a `Literal`-only word must collapse to the same output as the old single-call path.

### Step 2.7: Commit

- [ ] Run:
```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
feat(parser): expand tildes across mixed WordPart boundaries

try_parse_assignment now walks every WordPart of the assignment value
and tracks segment-boundary state (after '=' or an unquoted ':').
Each Literal part is re-scanned with split_tildes_in_literal using the
incoming boundary flag, so forms like x=$var:~/bin correctly produce a
Tilde node for the ':~' segment. Non-Literal parts (Parameter,
CommandSub, quoted) reset the boundary to false so x=$var~/bin (no
colon before the tilde) stays literal per POSIX §2.6.1.

Rewrites the existing assignment_rhs_parameter_then_tilde_not_expanded
unit test — which was pinning the buggy behavior — into
assignment_rhs_param_then_tilde_expands_after_colon asserting the
corrected POSIX behavior. Adds two more focused tests for the
colon-then-tilde expand case and the no-colon no-expand case.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 (Commit ③): E2E coverage + TODO.md cleanup

**Files:**
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_param_before_tilde.sh`
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_no_colon_boundary.sh`
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_literal_param_literal.sh`
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_cmdsub_before_tilde.sh`
- Modify: `TODO.md` — remove the §2.6.1 mixed-WordPart line

### Step 3.1: Write `tilde_mixed_param_before_tilde.sh`

- [ ] Create with exact content:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands when the preceding segment is a parameter expansion
# EXPECT_OUTPUT: /base:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
base=/base
x=$base:~/bin
echo "$x"
```

Chmod 644:
```bash
chmod 644 e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_param_before_tilde.sh
```

### Step 3.2: Write `tilde_mixed_no_colon_boundary.sh`

- [ ] Create with exact content:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde directly after a parameter expansion (no colon) stays literal per POSIX
# EXPECT_OUTPUT: /base~/bin
# EXPECT_EXIT: 0
HOME=/home/x
base=/base
x=$base~/bin
echo "$x"
```

Chmod 644.

### Step 3.3: Write `tilde_mixed_literal_param_literal.sh`

- [ ] Create with exact content:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands even when surrounded by literal and parameter parts
# EXPECT_OUTPUT: /a:/base:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
base=/base
x=/a:$base:~/bin
echo "$x"
```

Chmod 644.

### Step 3.4: Write `tilde_mixed_cmdsub_before_tilde.sh`

- [ ] Create with exact content:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands when the preceding segment is a command substitution
# EXPECT_OUTPUT: foo:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=$(echo foo):~/bin
echo "$x"
```

Chmod 644.

### Step 3.5: Run the filtered E2E

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=tilde_mixed 2>&1 | tail -2
```
Expected: `Total: 4  Passed: 4  Failed: 0`.

Failure handling: if any test fails, observe the actual output vs. expected and reconcile. Common causes:
- `HOME` leaking from the host env: ensure each test sets `HOME=/home/x` before the assignment, as specified.
- The `no_colon_boundary` test printing `/base/home/x/bin` instead of `/base~/bin`: Task 2 logic incorrectly promoted the trailing `~` to `Tilde` — check the `at_boundary = false` reset on the non-Literal branch.
- The `literal_param_literal` test printing `/a/home/x/b:/base:/home/x/bin` or similar shuffled output: the split's `ends_with_colon` flag was dropped between iterations — re-inspect the loop.

### Step 3.6: Full E2E regression

- [ ] Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected: `Total: 368  Passed: 367  Failed: 0  XFail: 1`. (Baseline 364 + 4 new.)

### Step 3.7: Remove the addressed TODO.md line

- [ ] Open `TODO.md`. Under `## Future: POSIX Conformance Gaps (Chapter 2)`, delete exactly this line (currently present):
```
- [ ] §2.6.1 Tilde expansion across mixed WordPart boundaries — `x=$var:~/bin` or `x=$var~/bin` does not expand `~` because the colon is in a Literal part that sits after a Parameter part; currently only the first Literal derived from `after_eq` is scanned by `split_tildes_in_literal`
```

No other line in TODO.md should change. Preserve the three remaining Chapter-2 lines (§2.6.1 escape, §2.11 ignored-on-entry, §2.10.2 Rule 5 XFAIL) exactly.

### Step 3.8: Commit

- [ ] Run:
```bash
git add e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_*.sh TODO.md
git commit -m "$(cat <<'EOF'
test(tilde): E2E coverage for §2.6.1 mixed-WordPart tilde + close TODO

Four new E2E tests covering $var:~ (expand), $var~ without colon
(no expand per POSIX), $(cmdsub):~ (expand), and the
literal/param/literal interleaving. Removes the matching line from
TODO.md's "Future: POSIX Conformance Gaps (Chapter 2)".

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

### Step 3.9: Final verification

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=2_06_01_tilde_expansion 2>&1 | tail -2
cargo test --lib 2>&1 | tail -5
git status
git log --oneline a2f637a..HEAD
```
Expected:
- Tilde E2E: `Total: 18  Passed: 18  Failed: 0` (14 existing + 4 new).
- Lib: `test result: ok. 625 passed`.
- `git status`: `nothing to commit, working tree clean`.
- `git log`: three commits (one per Task) on top of the plan/spec commits.

---

## Success Criteria (restated from spec)

- All 4 new E2E tests pass (`tilde_mixed_*.sh`).
- All 14 existing `e2e/posix_spec/2_06_01_tilde_expansion/*.sh` tests still pass.
- Full `./e2e/run_tests.sh`: total = 368 (baseline 364 + 4); `Failed: 0`; XFail count unchanged.
- `cargo test --lib`: ≥ 625 passing; 0 failures.
- No new compiler warnings in changed code.
- TODO.md: the §2.6.1 mixed-WordPart line is removed; the three remaining Chapter-2 lines are intact.
- Three commits on top of the spec commit `a2f637a`.

## Notes for the executor

- **TDD discipline matters here.** Task 2 Step 2.3 deliberately runs the new tests *before* Step 2.4's implementation. They must fail to confirm the bug is real and caught by the tests. Don't skip that step.
- **Do NOT modify `src/expand/mod.rs`.** The `expand_tilde_in_assignment_value` path handles `export`/`readonly` already via runtime-string splitting; this sub-project is parser-only.
- **Do NOT touch the lexer or AST.** The fix is purely in the parser's assignment-to-value construction.
- **`EXPECT_OUTPUT` is exact match.** The 4 E2E tests assert precise strings. Do not add trailing whitespace or differ in case.
- **`$TEST_TMPDIR` not needed** for these 4 tests — they don't create files. Sub-project 2 notes about `$TEST_TMPDIR` do not apply here.
- **Rewriting the existing `assignment_rhs_parameter_then_tilde_not_expanded` test** (Task 2 Step 2.2) is structurally important: the old test name embeds the buggy behavior as an invariant, so leaving it would either (a) block the fix from being mergeable or (b) silently ship as inverted documentation. Rename + invert is the correct approach, not `#[ignore]`.
- **`use ast::ParamExpr;`** inside the new unit tests (Step 2.1) is required because the tests live in `src/parser/mod.rs`'s `#[cfg(test)] mod tests` block where `ast::` is already imported at module-top; the explicit `use` narrows the `ParamExpr` import to the test function to avoid polluting sibling tests.
- **Commit boundaries**: Task 1's commit compiles and passes all existing tests on its own (signature refactor + new unit tests); Task 2's commit fixes the behavior (flips the pinned test); Task 3's commit adds E2E coverage and closes the TODO. Each commit is independently bisectable.
