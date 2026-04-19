# POSIX Chapter 2 Conformance Gaps — Sub-project 4: §2.6.1 Escape Metadata for Tilde Expansion

**Date**: 2026-04-20
**Sub-project**: 4 of 5 (POSIX Chapter 2 conformance gap remediation)
**Scope items from TODO.md**:

- §2.6.1 Tilde escape info lost at export/readonly — `export NAME=\~/val` wrongly expands because word expansion drops the backslash before `expand_tilde_in_assignment_value` sees the argument
- §2.6.1 Line-continuation tilde after unquoted `:` — `x=foo:\<newline>~/bin` does not expand the tilde because the `\<newline>` line-continuation causes the lexer to split into adjacent `WordPart::Literal` entries, which the parser's `prev_was_literal` heuristic then suppresses
- Sub-project 4 must REMOVE `prev_was_literal` — when escape metadata lands, the heuristic should be deleted in the same commit, replaced by a precise escape check

## Context

Sub-project 3 added the mixed-WordPart boundary walker in
`try_parse_assignment` and, as a temporary measure, a `prev_was_literal`
heuristic that blocks tilde recognition at any adjacent-Literal
boundary. The heuristic relied on the assumption that adjacent
`WordPart::Literal` entries arise ONLY from backslash-metachar escapes
like `\~` — but `\<newline>` line-continuation also produces adjacent
Literals (the lexer flushes the accumulated buffer, then `read_backslash`
returns an empty `Literal` for `\<newline>`, which is filtered out
post-factum leaving neighbouring Literals adjacent). Hence the
`prev_was_literal` heuristic over-suppresses tilde recognition after
line-continuation.

Similarly, `expand_tilde_in_assignment_value` in
`src/expand/mod.rs:643` receives a post-expansion `String` in which
the original backslash is already gone, so `export NAME=\~/val` and
`export NAME=~/val` look identical to the builtin — and both get
tilde-expanded.

Root cause: escape information is discarded at the lexer / expansion
boundary before any downstream code can distinguish "this `~` came from
`\~` (suppress)" vs "this `~` came from plain `~` (expand)" vs "this
segment was joined by `\<newline>` (transparent, don't suppress)".

## Goals

1. Fix `export NAME=\~/val` to preserve `~/val` as a literal.
2. Fix `x=foo:\<newline>~/bin` to expand the tilde (POSIX §2.2.1:
   `\<newline>` is removed before tokenization).
3. Remove the `prev_was_literal` heuristic entirely — replace the
   proxy with precise escape metadata in the AST.
4. Retire `expand_tilde_in_assignment_value` — the export/readonly
   path uses AST-aware handling instead of string-level post-hoc
   parsing.

## Non-goals

- Changes to `alias`, `unalias`, `command`, `local`, or other
  non-assignment-recognizing builtins.
- Generalization of "assignment-recognizing utilities" beyond
  `export` and `readonly`. POSIX §2.9.1 applies these semantics to
  that pair; other extensions are scope-4 deferred.
- Lexer refactor beyond what's necessary for escape metadata.
- Normative-granularity §2.6.1 coverage (tracked separately).

## Architecture

Four-layer change:

1. **AST** (`src/parser/ast.rs`): add `WordPart::EscapedLiteral(String)`
   variant.
2. **Lexer** (`src/lexer/word.rs`): make `\<newline>` transparent (no
   split, no new part); emit `\<char>` as `EscapedLiteral(char)`.
3. **Parser** (`src/parser/mod.rs`): handle `EscapedLiteral` in
   `try_parse_assignment` walker (treat as non-Literal, reset
   boundary); delete `prev_was_literal` flag and all its references.
4. **Executor + Builtin** (`src/exec/simple.rs`, `src/builtin/special.rs`,
   `src/expand/mod.rs`): route `export`/`readonly` args through
   `try_parse_assignment` pre-expansion so the value Word carries
   proper `Tilde`/`EscapedLiteral` nodes; delete
   `expand_tilde_in_assignment_value`.

### New AST variant

```rust
pub enum WordPart {
    Literal(String),
    EscapedLiteral(String),   // NEW
    SingleQuoted(String),
    DoubleQuoted(Vec<WordPart>),
    DollarSingleQuoted(String),
    Parameter(ParamExpr),
    CommandSub(Program),
    ArithSub(String),
    Tilde(Option<String>),
}
```

Semantics:
- Content is the escaped character(s) without the backslash. `\~` →
  `EscapedLiteral("~")`.
- For tilde recognition, `EscapedLiteral` is treated as a non-Literal
  (same arm as `Parameter` / `CommandSub` in the walker): its content
  is emitted verbatim, and `at_boundary` is reset to `false`.
- For expansion output, `EscapedLiteral` is identical to `Literal` —
  just emit the text. The escape has already served its purpose by
  suppressing tilde recognition at parse time.

### Lexer behaviour change

Unquoted context (`read_backslash` + caller at `src/lexer/word.rs:91-96`):

```rust
// Before
b'\\' => {
    if !literal.is_empty() {
        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
    }
    parts.push(self.read_backslash()?);  // Literal("") for \<newline>, Literal(ch) for \<char>
}

// After
b'\\' => {
    if self.peek_next_byte() == Some(b'\n') {
        // Line-continuation: consume \<newline>, continue accumulating (no split)
        self.advance(); // consume '\'
        self.advance(); // consume '\n'
    } else {
        // Escape: flush current literal, emit EscapedLiteral
        if !literal.is_empty() {
            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
        }
        parts.push(self.read_escape_unquoted()?);
    }
}

fn read_escape_unquoted(&mut self) -> error::Result<WordPart> {
    self.advance(); // consume '\'
    if self.at_end() {
        return Ok(WordPart::Literal("\\".to_string()));
    }
    let ch = self.current_byte();
    self.advance();
    Ok(WordPart::EscapedLiteral((ch as char).to_string()))
}
```

Double-quoted context (`read_backslash_in_double_quote`):
- `\$`, `\\`, `\"`, `` \` `` → `EscapedLiteral(ch)` (was `Literal(ch)`)
- `\<newline>` → transparent (no split, but the existing code returns
  empty `Literal` which the caller's loop pushes; simplest: change to
  skip without pushing)
- other `\<ch>` → `Literal(format!("\\{}", ch))` (preserved as-is per
  POSIX; this path is unchanged)

The post-loop filter `filter(|p| !matches!(p, WordPart::Literal(s) if s.is_empty()))`
at `src/lexer/word.rs:128-132` becomes dead code once line-continuation
is transparent. Retain as a defensive no-op.

### Parser walker change (remove `prev_was_literal`)

Current (`src/parser/mod.rs:385-419` after sub-project 3):
```rust
let mut at_boundary = true;
let mut prev_was_literal = true;
// ...
for part in remaining_parts {
    match part {
        WordPart::Literal(s) => {
            let effective_boundary = if prev_was_literal { false } else { at_boundary };
            let (parts, ends_colon) = split_tildes_in_literal(s, effective_boundary);
            value_parts.extend(parts);
            at_boundary = ends_colon;
            prev_was_literal = true;
        }
        other => {
            value_parts.push(other.clone());
            at_boundary = false;
            prev_was_literal = false;
        }
    }
}
```

Target:
```rust
let mut at_boundary = true;
// ...
for part in remaining_parts {
    match part {
        WordPart::Literal(s) => {
            let (parts, ends_colon) = split_tildes_in_literal(s, at_boundary);
            value_parts.extend(parts);
            at_boundary = ends_colon;
        }
        WordPart::EscapedLiteral(_) => {
            // Escape bypasses tilde recognition and breaks the segment
            // for tilde-prefix purposes — same as other non-Literal parts.
            value_parts.push(part.clone());
            at_boundary = false;
        }
        other => {
            value_parts.push(other.clone());
            at_boundary = false;
        }
    }
}
```

The 21-line doc comment above the walker is revised to describe the
simplified invariant (no `prev_was_literal` reference).

### Executor routing for export/readonly

When `args[0]` resolves to `export` or `readonly`, bypass the normal
`expand_words → Vec<String>` path and instead run each subsequent
Word through `try_parse_assignment`:

```rust
// src/exec/simple.rs — new helper
fn exec_assignment_builtin(
    name: &str,
    words: &[Word],
    env: &mut ShellEnv,
) -> Result<i32, ShellError> {
    let mut entries = Vec::new();
    for word in words {
        match Parser::try_parse_assignment(word) {
            Some(Assignment { name: n, value: Some(value_word) }) => {
                let value = expand_assignment_value(&value_word, env)?;
                entries.push((n, Some(value)));
            }
            Some(Assignment { name: n, value: None }) => {
                entries.push((n, None));
            }
            None => {
                // Plain name without '=' (e.g. `export PATH`)
                let expanded = expand_word_joined(word, env)?;
                entries.push((expanded, None));
            }
        }
    }
    match name {
        "export" => builtin_export_parsed(&entries, env),
        "readonly" => builtin_readonly_parsed(&entries, env),
        _ => unreachable!(),
    }
}
```

`expand_assignment_value` is the existing `expand_word` specialized
with field-splitting disabled (per POSIX §2.9.1, assignment words skip
field splitting). If the existing `expand_word` signature lets the
caller suppress splitting, use it directly.

`builtin_export_parsed` / `builtin_readonly_parsed` replace the current
`builtin_export(args: &[String], ...)` / `builtin_readonly(args: &[String], ...)`
at `src/builtin/special.rs:104, 153`. Signature:

```rust
fn builtin_export_parsed(
    entries: &[(String, Option<String>)],
    env: &mut ShellEnv,
) -> Result<i32, ShellError>
```

The `entries` slice carries `(name, Some(value))` for assignment form
and `(name, None)` for bare-name form. Internally the builtin sets
or exports each binding; no string re-parsing needed.

### Removed symbols

After sub-project 4:
- `prev_was_literal` (variable, its references, the comment block
  explaining it)
- `expand_tilde_in_assignment_value` (public helper + its unit tests,
  if any)

Confirm removal via `grep`:
```bash
grep -rn 'prev_was_literal\|expand_tilde_in_assignment_value' src/
```
Expected: no matches.

## Test Inventory

### Unit tests

**Lexer** (`src/lexer/mod.rs` or equivalent):
- `lexer_backslash_escape_emits_escaped_literal` — `x=\~/bin` →
  `[Literal("x="), EscapedLiteral("~"), Literal("/bin")]`
- `lexer_line_continuation_merges_literals` — `x=foo\<newline>bar` →
  `[Literal("x=foobar")]`
- Update existing `test_line_continuation` (line ~258 of
  `src/lexer/mod.rs`) — adjacent-Literal expectation → single-Literal
  expectation.

**Parser** (`src/parser/mod.rs` tests module):
- Flip `assignment_rhs_line_continuation_tilde_known_regression`
  (added in sub-project 3 fixup `7d74bab`) to
  `assignment_rhs_line_continuation_tilde_expands`:
  ```rust
  let (_, parts) = parse_first_assignment("x=foo:\\\n~/bin\n").unwrap();
  let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
  assert!(has_tilde, "parts = {:?}", parts);
  ```
- Keep `assignment_rhs_backslash_tilde_after_colon_stays_literal`
  (sub-project 3) — still passes via `EscapedLiteral` path.
- Keep `assignment_rhs_backslash_tilde_stays_literal` — same.
- Add `assignment_rhs_param_then_escaped_tilde_stays_literal`:
  `x=$var:\~/bin` — `~` is escaped, must NOT expand.
- Delete `prev_was_literal` TODO commentary.

**Builtin / executor**:
- Unit test for `exec_assignment_builtin` happy path if the executor
  has existing unit tests (otherwise E2E is sufficient).

### E2E tests (all under `e2e/posix_spec/2_06_01_tilde_expansion/`, mode 644)

| File | Scenario | EXPECT_OUTPUT |
|---|---|---|
| `tilde_mixed_line_continuation_expands.sh` | `x=foo:\<newline>~/bin; echo "$x"` | `foo:/home/x/bin` |
| `tilde_export_escape_preserved.sh` | `export NAME=\~/val; echo "$NAME"` | `~/val` |
| `tilde_readonly_escape_preserved.sh` | `readonly NAME=\~/val; echo "$NAME"` | `~/val` |

Each sets `HOME=/home/x` before the assignment.

### Regression surface

- All 14 + 4 = 18 existing `2_06_01_tilde_expansion/*.sh` E2E tests
  continue to pass.
- All `tilde_rhs_export.sh` / `tilde_rhs_readonly.sh` pass after the
  executor routing change (these test the happy path, not escape).
- `cargo test --lib` ≥ 625 (sub-project 3's 627 minus the 2 removed
  tests for `prev_was_literal` is not needed — instead those tests
  keep their POSIX-accurate assertions; full count depends on
  flipped/added tests).

## Workflow

### Step 0 — Baseline

- `cargo build` (clean).
- `./e2e/run_tests.sh 2>&1 | tail -3` — expect 368 / 367 pass + 1
  XFail / 0 Failed.
- `cargo test --lib 2>&1 | tail -3` — expect 627 passed.

### Step 1 (Commit ①) — AST variant + match-site network

1. Add `EscapedLiteral(String)` to `WordPart` in `src/parser/ast.rs`.
2. Add handler arms in every match statement that examines
   `WordPart::Literal`:
   - `src/expand/mod.rs` — `EscapedLiteral` arm treats as Literal
     (output text unchanged).
   - `src/parser/mod.rs` — `try_parse_assignment` walker gets the
     `EscapedLiteral` arm (treat as non-Literal — push as-is, reset
     boundary). Note: `prev_was_literal` is still present at this
     stage; the walker still uses it for `Literal` parts. The
     new `EscapedLiteral` arm does NOT set `prev_was_literal = true`
     (that's the whole point of distinguishing it).
   - `src/exec/simple.rs` — command-sub detection adds
     `EscapedLiteral => false` (not a command sub).
3. Verify: `cargo test --lib` — all green. `./e2e/run_tests.sh` — no
   regression. (Lexer hasn't started emitting `EscapedLiteral` yet.)
4. Commit.

### Step 2 (Commit ②) — Lexer changes

1. `src/lexer/word.rs`:
   - Detect `\<newline>` in the outer byte-dispatch; skip both
     characters without flushing `literal`.
   - Rename `read_backslash` → `read_escape_unquoted`; emit
     `EscapedLiteral` for `\<char>`.
   - Same for double-quoted: `\$\\<\`<"` → `EscapedLiteral`.
2. Update `test_line_continuation` in `src/lexer/mod.rs` — expected
   output becomes a single merged Literal.
3. Add lexer unit tests: `backslash_escape_emits_escaped_literal`,
   `line_continuation_merges_literals`.
4. Verify: `cargo test --lib` — all green.
   `./e2e/run_tests.sh --filter=tilde` — 18 tests still pass.
   `./e2e/run_tests.sh` — no regression.
   - At this point, `assignment_rhs_line_continuation_tilde_known_regression`
     is still asserting `!has_tilde`. With the lexer now merging the
     line-continuation case into a single Literal, `split_tildes_in_literal`
     will find the `~` at a colon boundary and produce a `Tilde` node.
     The test will FAIL.
   - Fix it at this step: edit the test body to match the new (correct)
     behavior, i.e. `assert!(has_tilde, ...)`.
5. Commit.

### Step 3 (Commit ③) — Remove `prev_was_literal`, rename flipped test

1. `src/parser/mod.rs`:
   - Delete the `let mut prev_was_literal = true;` declaration.
   - Delete the 5-line comment block explaining `prev_was_literal`
     (introduced in sub-project 3 + refined in sub-project 3's
     fixup).
   - Delete `prev_was_literal = true;` and `prev_was_literal = false;`
     assignments in the walker.
   - Delete the `effective_boundary = if prev_was_literal { false } else { at_boundary };`
     line; the walker now calls `split_tildes_in_literal(s, at_boundary)`
     directly.
2. Revise the 21-line doc comment above the walker: remove the
   `prev_was_literal` rationale; add a one-line note that
   `EscapedLiteral` is treated as a non-Literal part (same as
   Parameter/CommandSub) so the previous adjacent-Literal heuristic
   is no longer needed.
3. Rename `assignment_rhs_line_continuation_tilde_known_regression`
   → `assignment_rhs_line_continuation_tilde_expands`. Body already
   matches the new behavior from Step 2; just update the test name
   and drop the "known_regression" comment.
4. Add `assignment_rhs_param_then_escaped_tilde_stays_literal`:
   ```rust
   let (_, parts) = parse_first_assignment("x=$var:\\~/bin\n").unwrap();
   let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
   assert!(!has_tilde, "parts = {:?}", parts);
   ```
5. Verify: `grep prev_was_literal src/` → no matches.
   `cargo test --lib` — all green. `./e2e/run_tests.sh --filter=tilde`
   — 18 pass.
6. Commit.

### Step 4 (Commit ④) — Executor/builtin routing + E2E + TODO cleanup

1. `src/exec/simple.rs`: add `exec_assignment_builtin` helper and
   dispatch path for `export`/`readonly`.
2. `src/builtin/special.rs`: replace `builtin_export` /
   `builtin_readonly` string-based implementations with
   `builtin_export_parsed` / `builtin_readonly_parsed` taking
   `&[(String, Option<String>)]`.
3. `src/expand/mod.rs`: delete `expand_tilde_in_assignment_value`
   and its tests.
4. Add the 3 E2E files listed above. Chmod 644.
5. Verify:
   ```bash
   ./e2e/run_tests.sh --filter=tilde_mixed_line_continuation 2>&1 | tail -2
   ./e2e/run_tests.sh --filter=tilde_export_escape 2>&1 | tail -2
   ./e2e/run_tests.sh --filter=tilde_readonly_escape 2>&1 | tail -2
   ./e2e/run_tests.sh 2>&1 | tail -5
   cargo test --lib 2>&1 | tail -5
   ```
   Expected: each filter 1/1 pass; full E2E `Total: 371 Passed: 370
   Failed: 0 XFail: 1`; lib ≥ 627.
6. Update `TODO.md` — under "Future: POSIX Conformance Gaps
   (Chapter 2)", delete:
   - `§2.6.1 Tilde escape info lost at export/readonly — ...`
   - `§2.6.1 Line-continuation tilde after unquoted `:` — ...`
   - `Sub-project 4 must REMOVE prev_was_literal — ...`
7. `grep expand_tilde_in_assignment_value src/` → no matches.
8. Commit.

## Success Criteria

1. `export NAME=\~/val; echo "$NAME"` outputs `~/val` literally.
2. `x=foo:\<newline>~/bin; echo "$x"` outputs `foo:/home/x/bin`.
3. `prev_was_literal` symbol does not appear anywhere under `src/`.
4. `expand_tilde_in_assignment_value` symbol does not appear
   anywhere under `src/`.
5. All 18 existing `e2e/posix_spec/2_06_01_tilde_expansion/*.sh`
   tests still pass.
6. 3 new E2E tests pass.
7. Full `./e2e/run_tests.sh`: total = 371, `Failed: 0`, XFail = 1.
8. `cargo test --lib`: no failures; count ≥ 627.
9. TODO.md: the three listed gap items are removed; the §2.11 and
   §2.10.2 Rule 5 items remain.
10. 4 commits, each independently building and passing all tests at
    its own tip.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Adding `EscapedLiteral` misses a pattern-match site and causes non-exhaustive-match compile error | Fix compile errors as they arise in Step 1; `grep 'WordPart::Literal' src/` enumerates candidates |
| Lexer change to `\<newline>` breaks existing heredoc or other escape-using tests | Run `./e2e/run_tests.sh` after Step 2 in full; inspect `src/lexer/heredoc.rs` for assumptions |
| Executor routing for `export`/`readonly` changes observable behavior for the `export -p` (list) form or `export NAME` (no value) form | The routing uses `try_parse_assignment` which returns `None` for non-assignment words; fall back to plain-name path preserves those forms |
| `expand_tilde_in_assignment_value` had unit tests | Delete them alongside the function; there are no public API consumers outside `src/` |
| IFS splitting difference between `expand_word` and the retired `expand_tilde_in_assignment_value` path | Use field-splitting-disabled expansion for assignment values (POSIX §2.9.1) — if the existing `expand_word` only supports one mode, thread a new `no_split` parameter |
| Sub-project 3's `prev_was_literal` pinning test flips unexpectedly in Step 2 (before Step 3 removes the flag) | Expected — the test's `#known_regression` marker is explicitly designed to flip when the lexer behavior changes. Step 2 updates the body; Step 3 updates the test name. Sequence documented in Workflow |

## Out of Scope (explicit)

- `local`, `command`, `alias` assignment-like handling (if any
  applies).
- `typeset`, `declare`, etc. — not POSIX, non-issue.
- Normative-clause granularity for §2.6.1.
- Sub-project 5 (§2.11 ignored-on-entry).
