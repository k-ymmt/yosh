# POSIX Chapter 2 Conformance Gaps — Sub-project 3: §2.6.1 Mixed-WordPart Tilde Expansion

**Date**: 2026-04-20
**Sub-project**: 3 of 5 (POSIX Chapter 2 conformance gap remediation)
**Scope item from TODO.md**:

- §2.6.1 Tilde expansion across mixed WordPart boundaries — `x=$var:~/bin` or `x=$var~/bin` does not expand `~` because the colon is in a Literal part that sits after a Parameter part; currently only the first Literal derived from `after_eq` is scanned by `split_tildes_in_literal`

## Context

Sub-projects 1 and 2 closed the test-only Chapter 2 gaps. This
sub-project is the first to include actual parser code change —
fixing a correctness bug in tilde-prefix recognition for assignment
values that mix parameter expansions, command substitutions, and
colon-separated paths.

Investigation:

- `try_parse_assignment` (`src/parser/mod.rs:321-369`) extracts
  `after_eq` from the first `WordPart::Literal` and calls
  `split_tildes_in_literal(after_eq)` on it only.
- `word.parts[1..]` (Parameter, CommandSub, subsequent Literal, etc.)
  is appended via `extend_from_slice` with no tilde processing.
- The existing `split_tildes_in_literal` (`src/parser/mod.rs:930`)
  correctly handles colon-separated tildes within a single string
  (`PATH=~/a:~/b` works today because it is a single Literal).
- For `x=$var:~/bin`, the parser produces parts
  `[Lit("x="), Param(var), Lit(":~/bin")]`. `after_eq` is empty.
  The trailing `Lit(":~/bin")` is copied as-is, so the `~` reaches
  the expander as literal text.
- POSIX §2.6.1: a tilde-prefix is recognized at the start of an
  assignment value and after each unquoted `:`. Tilde characters
  anywhere else (e.g. directly after a parameter expansion with no
  intervening `:`) stay literal.

Related but out of scope:

- `expand_tilde_in_assignment_value` (`src/expand/mod.rs:643`) handles
  the `export NAME=...` / `readonly NAME=...` path at runtime by
  operating on already-expanded strings. It already splits on `:`
  and expands correctly, so `export` with mixed WordParts works
  today via a separate path. The parser-path fix does not touch
  the expand-path code.

## Goals

1. Walk every `WordPart` of the assignment value, tracking whether
   the next character sits at a segment boundary (after `=` or an
   unquoted `:`), and split tildes in all `WordPart::Literal`
   entries that qualify.
2. Preserve POSIX-strict behavior: tilde directly after a non-Literal
   part (Parameter, CommandSub, quoted content) with no intervening
   `:` stays literal.
3. Add 4 E2E tests pinning both the expand-case and the
   no-expand-case for mixed-WordPart inputs.
4. Remove the corresponding line from TODO.md once tests pass.

## Non-goals

- Changes to `expand_tilde_in_assignment_value` (that path already
  handles mixed input correctly via runtime-string splitting).
- The `export NAME=\~/val` escape-preservation fix (sub-project 4).
- AST changes to `WordPart::Tilde` (the existing shape is sufficient).
- Lexer changes.
- Exotic tilde-prefix forms like `~$var/bin` where the user name
  is itself a parameter — POSIX leaves this implementation-defined
  and yosh's current behavior is considered acceptable.

## Architecture

Single-file change: `src/parser/mod.rs`. No lexer, expander, or AST
changes.

```
try_parse_assignment(Word)
  ├─ extract name before '='
  ├─ extract after_eq from the first Literal part
  ├─ at_boundary := true                            ← NEW: tracks segment boundary
  ├─ for after_eq (if non-empty):
  │     (parts, ends_colon) := split_tildes_in_literal(after_eq, at_boundary)
  │     emit parts; at_boundary := ends_colon
  └─ for part in remaining_parts:
        match part:
          Literal(s) → (parts, ends_colon) := split_tildes_in_literal(s, at_boundary)
                       emit parts; at_boundary := ends_colon
          other      → emit as-is; at_boundary := false
```

`split_tildes_in_literal` signature is extended:

```rust
// Before
fn split_tildes_in_literal(s: &str) -> Vec<WordPart>

// After
fn split_tildes_in_literal(
    s: &str,
    start_at_boundary: bool,
) -> (Vec<WordPart>, bool /* ends_with_colon */)
```

Internal logic: the existing colon-splitting loop is kept. The only
behavior change is that when `i == 0 && !start_at_boundary`, the
leading `~` (if present) is not recognized — the segment stays as a
plain literal. Segments after the first colon are always at a
boundary, regardless of the flag. The return flag is
`s.ends_with(':')`.

### Edge case table

| Input value | Parsed parts | Produced value parts |
|---|---|---|
| `x=$var:~/bin` | `[Lit("x="), Param(var), Lit(":~/bin")]` | `[Param(var), Lit(":"), Tilde(None), Lit("/bin")]` |
| `x=$var~/bin` | `[Lit("x="), Param(var), Lit("~/bin")]` | `[Param(var), Lit("~/bin")]` — no tilde expansion |
| `x=a:$var:~/bin` | `[Lit("x=a:"), Param(var), Lit(":~/bin")]` | `[Lit("a:"), Param(var), Lit(":"), Tilde(None), Lit("/bin")]` |
| `x=$(echo foo):~/bin` | `[Lit("x="), CmdSub, Lit(":~/bin")]` | `[CmdSub, Lit(":"), Tilde(None), Lit("/bin")]` |
| `x=$var` (existing path) | `[Lit("x="), Param(var)]` | `[Param(var)]` — unchanged |
| `PATH=~/a:~/b` (existing path) | `[Lit("x=~/a:~/b")]` | `[Tilde(None), Lit("/a:"), Tilde(None), Lit("/b")]` — unchanged |

## Test Inventory

### Unit tests (`src/parser/mod.rs` tests module)

Existing 11 `test_split_tildes_*` tests: update each call-site to
`split_tildes_in_literal(input, true).0` (tuple unpack, behavior
preserved).

New unit tests (3):

- `test_split_tildes_not_at_boundary_skips_leading_tilde`:
  `split_tildes_in_literal("~/bin", false)` →
  `(vec![Literal("~/bin")], false)`
- `test_split_tildes_not_at_boundary_then_colon`:
  `split_tildes_in_literal(":~/bin", false)` →
  `(vec![Literal(":"), Tilde(None), Literal("/bin")], false)`
- `test_split_tildes_ends_with_colon`:
  `split_tildes_in_literal("a:", true)` →
  `(vec![Literal("a:")], true)` — validates the returned
  `ends_with_colon` flag

New `try_parse_assignment` unit tests (2):

- `test_assignment_param_then_colon_tilde`: parses `x=$var:~/bin`
  and asserts the produced value `Word.parts` matches the expected
  `[Param(var), Lit(":"), Tilde(None), Lit("/bin")]`.
- `test_assignment_param_then_tilde_no_colon`: parses `x=$var~/bin`
  and asserts the produced value `Word.parts` matches
  `[Param(var), Lit("~/bin")]` (no `Tilde` node).

### E2E tests (`e2e/posix_spec/2_06_01_tilde_expansion/`)

Four new files, all mode 644, `#!/bin/sh` + `POSIX_REF` + `DESCRIPTION`
headers per project convention:

**`tilde_mixed_param_before_tilde.sh`**
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

**`tilde_mixed_no_colon_boundary.sh`**
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

**`tilde_mixed_literal_param_literal.sh`**
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

**`tilde_mixed_cmdsub_before_tilde.sh`**
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

## Workflow

### Step 0 — Baseline

- `cargo build` to refresh the debug binary.
- `./e2e/run_tests.sh 2>&1 | tail -3` — record the current pass /
  XFail counts (expected baseline: 364 pass + 1 XFail / 0 Failed).

### Step 1 (Commit ①) — `split_tildes_in_literal` signature extension (TDD)

1. Update the 11 existing `test_split_tildes_*` tests to call
   `split_tildes_in_literal(input, true).0` (tuple unpack).
2. Change the function signature to
   `(s: &str, start_at_boundary: bool) -> (Vec<WordPart>, bool)`.
3. Update the body: in the segment loop, when `i == 0 && !start_at_boundary`,
   skip the tilde-strip attempt and fall through to the plain literal
   push. Compute `ends_with_colon = s.ends_with(':')` and return it.
4. Update the doc comment to describe the new contract.
5. Add the three new unit tests above.
6. `cargo test --lib split_tildes` — expect 14/14 pass.
7. Commit.

### Step 2 (Commit ②) — `try_parse_assignment` multi-part traversal (TDD)

1. Add the two new `test_assignment_*` unit tests. They should fail
   against the current single-Literal-only implementation.
2. Replace the current `value_parts` construction block with the
   boundary-tracking loop from §Architecture above.
3. `cargo test --lib try_parse_assignment` — expect pass on the new
   tests plus the existing assignment tests.
4. `cargo test --lib` — no regressions in the broader parser suite.
5. `./e2e/run_tests.sh --filter=tilde` — all existing 14 tilde tests
   still pass (regression check).
6. Commit.

### Step 3 (Commit ③) — E2E tests + TODO.md cleanup

1. Create the four files above under
   `e2e/posix_spec/2_06_01_tilde_expansion/`, mode 644.
2. `./e2e/run_tests.sh --filter=tilde_mixed` — expect 4/4 pass.
3. `./e2e/run_tests.sh` — full suite; new total 368 pass + 1 XFail,
   0 Failed.
4. Remove the §2.6.1 mixed-WordPart line from TODO.md's
   "Future: POSIX Conformance Gaps (Chapter 2)" section.
5. Commit.

## Success Criteria

1. All four new E2E tests pass.
2. All 14 existing `e2e/posix_spec/2_06_01_tilde_expansion/*.sh` tests
   still pass (no regression).
3. Full `./e2e/run_tests.sh` — total advances from 364 to 368 with
   `Failed: 0` (XFail count from sub-project 2 may persist unchanged).
4. `cargo test --lib` — 620+ passing, no unit-test regressions.
5. `cargo build` — clean, no new compiler warnings in touched code.
6. TODO.md: the §2.6.1 mixed-WordPart line is removed.
7. Three commits — one per Workflow step.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Existing tilde tests regress because boundary tracking changes observable behavior | Step 2 runs `./e2e/run_tests.sh --filter=tilde` before committing. The 14 existing tests only exercise tildes within single Literals; boundary tracking does not affect that path. |
| `split_tildes_in_literal` has callers outside `try_parse_assignment` | Confirmed single call site via `grep split_tildes_in_literal src/`. The function is `pub(crate)` and the signature change ripples only to one call site plus the 11 unit tests. |
| Mixed quoting edge case `x="a:"$var:~/bin` not covered | `DoubleQuoted` is a separate `WordPart` variant — not `Literal`. The boundary tracker hits the `other` arm, resets `at_boundary = false`, and the following `Lit(":~/bin")` is handled via the boundary restart at its internal `:`. Correct by construction. Not explicitly tested; considered out of scope for the 4-test set. |
| `ParamExpr` with internal tilde-prefix (`${foo:-~/bin}`) | Separate code path in `src/parser/mod.rs`'s ParamExpr parsing; not touched by this change. Existing behavior preserved. |
| YAGNI on `start_at_boundary`: is the new flag actually needed? | Yes — without it, the second and later Literals would be treated as if they always start at a boundary, so `x=$var~/bin` would incorrectly promote `~` to `Tilde(None)`. The flag is the defining correctness control. |

## Out of Scope (explicit)

- `expand_tilde_in_assignment_value` refactor (sub-project 4 handles
  the escape-preservation issue in the same region).
- Legacy `e2e/` directory migration to `POSIX_REF` metadata.
- Normative-granularity §2.6.1 coverage (tracked separately as the
  "Deepen Chapter 2 POSIX coverage" TODO item).
- Sub-projects 4 and 5 (escape preservation, §2.11 ignored-on-entry).
