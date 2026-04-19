# Tilde Expansion on Assignment RHS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make yosh expand `x=~/bin` and `PATH=~/a:~/b` per POSIX §2.6.1 by converting embedded tildes in assignment values into `WordPart::Tilde` parts during parser post-processing, so the existing expander pipeline handles the expansion.

**Architecture:** Single-file change to `src/parser/mod.rs`. A new pure helper `split_tildes_in_literal(&str) -> Vec<WordPart>` scans the `after_eq` string of an assignment, splits on `:` and promotes tildes at segment starts into `Tilde` parts. `try_parse_assignment` calls this helper only when `after_eq` is non-empty, so quoted / escaped / substituted values (which land in `remaining_parts`) are left alone, giving us correct escape semantics for free.

**Tech Stack:** Rust 2024 edition, existing AST (`WordPart::Tilde`) and expander unchanged.

**Spec:** `docs/superpowers/specs/2026-04-19-tilde-assignment-rhs-design.md`

---

## File Structure

**Modify:**

- `src/parser/mod.rs` — add `split_tildes_in_literal` helper and its
  invocation inside `try_parse_assignment` (currently lines 301–347).
  Add a `#[cfg(test)] mod tests { ... }` block at the end of the file
  if one does not exist; otherwise append to the existing block.
- `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh` —
  remove the `# XFAIL:` line so this test runs as PASS.
- `TODO.md` — delete the `§2.6.1 Tilde expansion on assignment RHS`
  entry and add a new one for the deferred mixed-part case.

**Create (new E2E tests, all `644` permissions under
`e2e/posix_spec/2_06_01_tilde_expansion/`):**

- `tilde_rhs_user_form.sh`
- `tilde_rhs_colon_multiple.sh`
- `tilde_rhs_middle_segment.sh`
- `tilde_rhs_quoted_not_expanded.sh`
- `tilde_rhs_double_quoted_not_expanded.sh`
- `tilde_rhs_backslash_not_expanded.sh`
- `tilde_rhs_export.sh`
- `tilde_rhs_readonly.sh`
- `tilde_rhs_command_prefix.sh`
- `tilde_rhs_not_at_start.sh`

---

## Task 0: Verify baseline

- [ ] **Step 1: Confirm tests green**

Run: `cargo test --lib 2>&1 | tail -3`
Expected: `test result: ok.` with 576 tests.

- [ ] **Step 2: Confirm the XFAIL exists**

Run: `grep -n XFAIL e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`
Expected: one line matching `# XFAIL: tilde expansion on assignment RHS not implemented`.

- [ ] **Step 3: Check whether `src/parser/mod.rs` already has a test module**

Run: `grep -n '#\[cfg(test)\]' src/parser/mod.rs | head -5`
Record the result — if a `mod tests` already exists, append to it; otherwise create one at the end of the file.

---

## Task 1: Add `split_tildes_in_literal` helper

**Files:**
- Modify: `src/parser/mod.rs` (append helper after `try_parse_assignment` near line 347, plus `#[cfg(test)] mod tests` at end of file)

- [ ] **Step 1: Write failing unit tests**

Find the end of `src/parser/mod.rs`. If there is NO `#[cfg(test)] mod tests { ... }`, append this block. If one already exists, append these tests inside it.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::WordPart;

    // ── split_tildes_in_literal ─────────────────────────────────

    fn lit(s: &str) -> WordPart {
        WordPart::Literal(s.to_string())
    }

    #[test]
    fn split_no_tilde_returns_single_literal() {
        assert_eq!(split_tildes_in_literal("foo/bar"), vec![lit("foo/bar")]);
    }

    #[test]
    fn split_leading_tilde_only() {
        assert_eq!(split_tildes_in_literal("~"), vec![WordPart::Tilde(None)]);
    }

    #[test]
    fn split_leading_tilde_slash() {
        assert_eq!(
            split_tildes_in_literal("~/bin"),
            vec![WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn split_leading_tilde_user() {
        assert_eq!(
            split_tildes_in_literal("~user/bin"),
            vec![WordPart::Tilde(Some("user".to_string())), lit("/bin")]
        );
    }

    #[test]
    fn split_colon_separated_tildes() {
        assert_eq!(
            split_tildes_in_literal("~/a:~/b"),
            vec![
                WordPart::Tilde(None),
                lit("/a:"),
                WordPart::Tilde(None),
                lit("/b"),
            ]
        );
    }

    #[test]
    fn split_middle_segment_with_tilde() {
        assert_eq!(
            split_tildes_in_literal("/usr:~/bin"),
            vec![lit("/usr:"), WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn split_trailing_colon() {
        assert_eq!(
            split_tildes_in_literal("~/a:"),
            vec![WordPart::Tilde(None), lit("/a:")]
        );
    }

    #[test]
    fn split_leading_colon() {
        assert_eq!(
            split_tildes_in_literal(":~/a"),
            vec![lit(":"), WordPart::Tilde(None), lit("/a")]
        );
    }

    #[test]
    fn split_consecutive_colons() {
        assert_eq!(
            split_tildes_in_literal("::~/a"),
            vec![lit("::"), WordPart::Tilde(None), lit("/a")]
        );
    }

    #[test]
    fn split_mid_word_tilde_stays_literal() {
        // "~" not at position 0 of a segment → not a tilde-prefix
        assert_eq!(
            split_tildes_in_literal("foo~/bin"),
            vec![lit("foo~/bin")]
        );
    }

    #[test]
    fn split_double_tilde_invalid_user() {
        // Second ~ is not a name-safe char → whole segment stays literal
        assert_eq!(
            split_tildes_in_literal("~~/bin"),
            vec![lit("~~/bin")]
        );
    }

    #[test]
    fn split_user_name_with_dot_and_dash() {
        assert_eq!(
            split_tildes_in_literal("~a.b-c/bin"),
            vec![WordPart::Tilde(Some("a.b-c".to_string())), lit("/bin")]
        );
    }

    #[test]
    fn split_two_tildes_joined_by_colon_no_slash() {
        assert_eq!(
            split_tildes_in_literal("~:~"),
            vec![
                WordPart::Tilde(None),
                lit(":"),
                WordPart::Tilde(None),
            ]
        );
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test --lib parser::tests::split 2>&1 | tail -20`
Expected: compile error `cannot find function split_tildes_in_literal`.

Paste the tail of the output into your report.

- [ ] **Step 3: Implement `split_tildes_in_literal`**

Append below `try_parse_assignment` (around current line 347) in `src/parser/mod.rs`:

```rust
/// Scan the raw RHS of an assignment (`after_eq`) and promote unquoted
/// tilde-prefixes at segment boundaries into `WordPart::Tilde` nodes.
/// Segments are delimited by `:` so that forms like `PATH=~/a:~/b`
/// expand at both tildes (POSIX §2.6.1).
///
/// The caller must only pass the substring that came directly after
/// the opening `=` of the assignment (and must skip this call entirely
/// when that substring is empty); tildes inside quoted, escaped, or
/// substituted parts must never reach this function.
pub(crate) fn split_tildes_in_literal(s: &str) -> Vec<ast::WordPart> {
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
        if let Some(rest_after_tilde) = segment.strip_prefix('~') {
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

    out
}
```

- [ ] **Step 4: Run tests to confirm they pass**

Run: `cargo test --lib parser::tests::split 2>&1 | tail -5`
Expected: `test result: ok. 13 passed`.

Full suite: `cargo test --lib 2>&1 | tail -5`
Expected: 589 tests pass (576 baseline + 13 new).

- [ ] **Step 5: Commit**

```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
feat(parser): add split_tildes_in_literal for assignment RHS

Pure helper that walks the unquoted RHS of an assignment, splits on
colon boundaries and promotes leading tildes at each segment into
WordPart::Tilde nodes. Prepares the AST for POSIX §2.6.1 tilde
expansion on assignment values.

Task 1/4 of the assignment-RHS tilde rewrite. See
docs/superpowers/specs/2026-04-19-tilde-assignment-rhs-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Wire the helper into `try_parse_assignment`

**Files:**
- Modify: `src/parser/mod.rs` (inside `try_parse_assignment`)

- [ ] **Step 1: Write failing integration tests**

Append inside the `#[cfg(test)] mod tests { ... }` block in `src/parser/mod.rs`, after the unit tests:

```rust
    // ── try_parse_assignment integration ────────────────────────

    use crate::parser::ast::Command;

    // AST shape (verified against src/parser/ast.rs):
    //   Program { commands: Vec<CompleteCommand> }
    //   CompleteCommand { items: Vec<(AndOrList, Option<SeparatorOp>)> }
    //   AndOrList { first: Pipeline, rest: ... }
    //   Pipeline { commands: Vec<Command>, negated: bool }
    //   Command::Simple(SimpleCommand)
    //   SimpleCommand { assignments: Vec<Assignment>, words, redirects }
    fn parse_first_assignment(source: &str) -> Option<(String, Vec<WordPart>)> {
        let mut parser = Parser::new(source);
        let program = parser.parse_program().ok()?;
        let cc = program.commands.into_iter().next()?;
        let (aol, _) = cc.items.into_iter().next()?;
        let cmd = aol.first.commands.into_iter().next()?;
        let Command::Simple(sc) = cmd else {
            return None;
        };
        let a = sc.assignments.into_iter().next()?;
        let parts = a.value.map(|w| w.parts).unwrap_or_default();
        Some((a.name, parts))
    }

    #[test]
    fn assignment_rhs_unquoted_tilde_becomes_tilde_part() {
        let (name, parts) = parse_first_assignment("x=~/bin\n").unwrap();
        assert_eq!(name, "x");
        assert_eq!(
            parts,
            vec![WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn assignment_rhs_multi_colon_tildes() {
        let (name, parts) = parse_first_assignment("PATH=~/a:~/b\n").unwrap();
        assert_eq!(name, "PATH");
        assert_eq!(
            parts,
            vec![
                WordPart::Tilde(None),
                lit("/a:"),
                WordPart::Tilde(None),
                lit("/b"),
            ]
        );
    }

    #[test]
    fn assignment_rhs_backslash_tilde_stays_literal() {
        // `\~` must not be promoted — lexer already splits the literal
        // at the backslash, so the tilde is in a Literal that arrives
        // via remaining_parts, which we leave untouched.
        let (_, parts) = parse_first_assignment("x=\\~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn assignment_rhs_single_quoted_tilde_stays_quoted() {
        let (_, parts) = parse_first_assignment("x='~'/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn assignment_rhs_parameter_then_tilde_not_expanded() {
        // Out-of-scope case: parameter substitution interrupts the literal,
        // so the tilde after the colon must remain literal.
        let (_, parts) = parse_first_assignment("x=$var:~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }
```

Ensure the `use super::*` at the top of the test module brings in
`Parser`, `Command`, and `WordPart` (they come from `crate::parser`
and `crate::parser::ast`). If any type is missing from the test
module's scope, add an explicit `use` at the test-module level.

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test --lib parser::tests::assignment_rhs 2>&1 | tail -20`
Expected: tests panic because `Tilde` parts are not produced yet (or assertions fail).

If the tests don't even compile, fix the `parse_first_assignment` helper to match the real AST shape before proceeding.

- [ ] **Step 3: Invoke `split_tildes_in_literal` from `try_parse_assignment`**

Open `src/parser/mod.rs`. In `try_parse_assignment` (currently lines 301–347), locate this block:

```rust
let mut value_parts = Vec::new();
if !after_eq.is_empty() {
    value_parts.push(WordPart::Literal(after_eq.to_string()));
}
value_parts.extend_from_slice(remaining_parts);
```

Replace it with:

```rust
let mut value_parts = Vec::new();
if !after_eq.is_empty() {
    value_parts.extend(split_tildes_in_literal(after_eq));
}
value_parts.extend_from_slice(remaining_parts);
```

Leave the rest of the function unchanged.

- [ ] **Step 4: Run tests to confirm they pass**

Run: `cargo test --lib parser::tests::assignment_rhs 2>&1 | tail -10`
Expected: all 5 integration tests pass.

Full suite: `cargo test --lib 2>&1 | tail -5`
Expected: 594 tests pass (589 after Task 1 + 5 new).

- [ ] **Step 5: Run clippy to confirm no new warnings**

Run: `cargo clippy --lib 2>&1 | grep -A2 "parser/mod.rs" | head -20`
Expected: no output (or only pre-existing warnings).

- [ ] **Step 6: Commit**

```bash
git add src/parser/mod.rs
git commit -m "$(cat <<'EOF'
feat(parser): expand tildes in assignment RHS per POSIX §2.6.1

try_parse_assignment now routes the after-equals literal through
split_tildes_in_literal, promoting leading and post-colon tildes into
Tilde parts. Quoted, escaped, and substitution-prefixed values are
unaffected because the lexer has already segmented them into
remaining_parts, which we pass through untouched.

Task 2/4 of the assignment-RHS tilde rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Flip XFAIL and add E2E tests

**Files:**
- Modify: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`
- Create: 10 files under `e2e/posix_spec/2_06_01_tilde_expansion/`

- [ ] **Step 1: Remove the XFAIL line**

Edit `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`. Delete the exact line:

```
# XFAIL: tilde expansion on assignment RHS not implemented
```

Leave all other lines (including `HOME=/tmp/hdir`, `x=~/bin`, `echo "$x"`) intact.

- [ ] **Step 2: Build and verify the flip**

```bash
cargo build 2>&1 | tail -3
./e2e/run_tests.sh --filter=tilde_assignment_rhs 2>&1 | tail -5
```

Expected: `[PASS]  posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`. If FAIL, debug before continuing.

- [ ] **Step 3: Create `tilde_rhs_user_form.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_user_form.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde with username resolves via getpwnam when user exists
# EXPECT_EXIT: 0
x=~root/suffix
# On platforms where root exists, value starts with a real directory
# (typically /root or /var/root) and ends with /suffix.
# On platforms where getpwnam fails for 'root', yosh leaves ~root unchanged.
case "$x" in
    /*/suffix) exit 0 ;;
    '~root/suffix') exit 0 ;;
    *) echo "unexpected: $x" >&2; exit 1 ;;
esac
```

- [ ] **Step 4: Create `tilde_rhs_colon_multiple.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_colon_multiple.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Each tilde after an unquoted ':' in an assignment expands
# EXPECT_OUTPUT: /home/x/a:/home/x/b
# EXPECT_EXIT: 0
HOME=/home/x
PATH=~/a:~/b
echo "$PATH"
```

- [ ] **Step 5: Create `tilde_rhs_middle_segment.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_middle_segment.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde after a ':' expands even if the first segment has no tilde
# EXPECT_OUTPUT: /usr:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=/usr:~/bin
echo "$x"
```

- [ ] **Step 6: Create `tilde_rhs_quoted_not_expanded.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_quoted_not_expanded.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Single-quoted tilde is not expanded
# EXPECT_OUTPUT: ~/bin
# EXPECT_EXIT: 0
HOME=/home/x
x='~'/bin
echo "$x"
```

- [ ] **Step 7: Create `tilde_rhs_double_quoted_not_expanded.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_double_quoted_not_expanded.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Double-quoted tilde is not expanded
# EXPECT_OUTPUT: ~/bin
# EXPECT_EXIT: 0
HOME=/home/x
x="~"/bin
echo "$x"
```

- [ ] **Step 8: Create `tilde_rhs_backslash_not_expanded.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_backslash_not_expanded.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Backslash-escaped tilde is not expanded
# EXPECT_OUTPUT: ~/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=\~/bin
echo "$x"
```

- [ ] **Step 9: Create `tilde_rhs_export.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_export.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: export with a tilde RHS expands the tilde
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
export MYVAR=~/bin
echo "$MYVAR"
```

- [ ] **Step 10: Create `tilde_rhs_readonly.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_readonly.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: readonly with a tilde RHS expands the tilde
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
readonly RO=~/bin
echo "$RO"
```

- [ ] **Step 11: Create `tilde_rhs_command_prefix.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_command_prefix.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde in a command-prefix assignment expands before the command runs
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
# Capture the expanded value via a child process that echoes it back.
PREFIXED=~/bin sh -c 'echo "$PREFIXED"'
```

- [ ] **Step 12: Create `tilde_rhs_not_at_start.sh`**

Path: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_not_at_start.sh`

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde that is not at segment start stays literal
# EXPECT_OUTPUT: foo~/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=foo~/bin
echo "$x"
```

- [ ] **Step 13: Set permissions to 644**

```bash
chmod 644 \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_user_form.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_colon_multiple.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_middle_segment.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_quoted_not_expanded.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_double_quoted_not_expanded.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_backslash_not_expanded.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_export.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_readonly.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_command_prefix.sh \
  e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_not_at_start.sh
```

- [ ] **Step 14: Run the filter**

```bash
./e2e/run_tests.sh --filter=tilde 2>&1 | tail -25
```

Expected: every `tilde_*` test shows `[PASS]`. Include the full tail in the report.

- [ ] **Step 15: Commit**

```bash
git add e2e/posix_spec/2_06_01_tilde_expansion/
git commit -m "$(cat <<'EOF'
test(tilde): flip XFAIL and add assignment-RHS tilde E2E coverage

- e2e/posix_spec/.../tilde_assignment_rhs.sh: XFAIL removed; now PASSes.
- 10 new tests under e2e/posix_spec/2_06_01_tilde_expansion/ covering
  ~user, colon-separated multi-tilde, middle-segment tilde, quoted and
  backslash-escaped tildes, export, readonly, command-prefix, and the
  mid-word-tilde-stays-literal regression.

Task 3/4 of the assignment-RHS tilde rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: TODO.md cleanup and final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Edit TODO.md**

Open `TODO.md`. Under the section **"Future: POSIX Conformance Gaps (Chapter 2)"**:

1. **Delete** this exact line:

```
- [ ] §2.6.1 Tilde expansion on assignment RHS — `x=~/bin` does not expand `~` to `$HOME` (see `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh` XFAIL)
```

2. **Insert** the following new line (in the same section, preserving alphabetical-ish ordering of the section — put it where §2.6.1 entries would sit):

```
- [ ] §2.6.1 Tilde expansion across mixed WordPart boundaries — `x=$var:~/bin` or `x=$var~/bin` does not expand `~` because the colon is in a Literal part that sits after a Parameter part; currently only the first Literal derived from `after_eq` is scanned by `split_tildes_in_literal`
```

- [ ] **Step 2: Run the full verification suite**

```bash
cargo test --lib 2>&1 | tail -5
cargo fmt --check 2>&1 | head -20
cargo clippy --lib 2>&1 | grep -E "parser/mod.rs" | head -10
./e2e/run_tests.sh 2>&1 | tail -5
```

Expected:
- `cargo test`: `test result: ok. 594 passed`.
- `cargo fmt --check`: clean (no output).
- `cargo clippy`: no warnings for `src/parser/mod.rs`.
- E2E summary: `Total: 308  Passed: 306  Failed: 0  Timedout: 0  XFail: 2  XPass: 0` (baseline was 298 + 10 new tilde E2E tests; remaining XFails are §2.10 empty compound_list and §2.5.3 LINENO).

If `cargo fmt --check` reports drift inside `src/parser/mod.rs`, run `cargo fmt` on just that file and include in the final commit. Do NOT reformat other files (those will be handled by their own sub-projects / commits).

- [ ] **Step 3: Commit**

```bash
git add TODO.md
# Include src/parser/mod.rs only if fmt touched it.
git commit -m "$(cat <<'EOF'
chore(tilde): remove §2.6.1 assignment-RHS TODO, note mixed-part gap

The PWD-style conformance gap is closed by the parser-level tilde
rewrite (tasks 1-3). Per project convention, completed TODO entries
are deleted rather than marked [x]. A narrower follow-up is recorded
for the deferred mixed-WordPart case (e.g. `x=$var:~/bin`).

Task 4/4 of the assignment-RHS tilde rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Completion Criteria (final check)

1. `cargo test --lib` — 594 passed.
2. `cargo clippy --lib` — no new warnings in `src/parser/mod.rs`.
3. `cargo fmt --check` — clean.
4. `./e2e/run_tests.sh` summary: `XFail: 2, XPass: 0, Failed: 0, Timedout: 0`.
5. Four focused commits (Tasks 1–4), each with its task number in the body.
6. `TODO.md` lists the mixed-part follow-up and no longer lists the closed gap.
