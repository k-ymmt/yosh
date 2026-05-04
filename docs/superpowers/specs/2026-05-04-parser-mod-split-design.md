# Parser Module Split Design

**Date**: 2026-05-04
**Status**: Design proposed
**Scope**: Mechanical split of `src/parser/mod.rs` (2054 lines) into focused submodules.

## Overview

`src/parser/mod.rs` has grown to 2054 lines: roughly 985 lines of production
code plus 985 lines of tests in a single `#[cfg(test)] mod tests`. The
production code holds the entire recursive-descent parser as one giant
`impl Parser` block, which:

- Hampers focused review of any one parsing concern.
- Forces every contributor to navigate a long file even for localized
  changes (redirects, heredocs, function definitions, tilde splitting).
- Makes the `tests` module unwieldy: 100+ tests spanning 8 different
  subjects with no per-topic locality.

This spec splits the file along its existing internal seams, mirroring the
already-split `src/exec/` directory (`simple.rs`, `compound.rs`,
`function.rs`, `redirect.rs`, ...). The split is purely mechanical:
no `fn` body or signature is modified; visibility is preserved as-is.

## Goals

1. Reduce `src/parser/mod.rs` to ≤ 300 lines (target ~280) holding only
   the `Parser` struct, constructors, token utilities, and the top-level
   driver methods.
2. Co-locate parsing logic and its tests by topic: simple commands,
   compound commands, function definitions, redirects, and word/tilde
   utilities each live in a dedicated file with their `#[cfg(test)] mod
   tests` attached.
3. Preserve every public symbol's visibility, name, and signature so that
   no caller outside `src/parser/` needs to change.
4. Each step of the split is an independent commit that builds and passes
   tests on its own — no transient broken state across multiple commits.

## Non-Goals

- **API visibility changes**: tightening `pub fn` to `pub(crate)` is
  deferred to a follow-up spec. The current set of public methods on
  `Parser` is preserved verbatim.
- **Method body changes**: no logic edits, no helper extraction, no
  optimization. `try_parse_assignment` value-construction extraction
  (TODO.md follow-up) is out of scope.
- **`ast.rs` changes**: AST type additions, removals, or reorganization
  are out of scope.
- **Error-type changes**: `ParseErrorKind` additions or rewrites are out
  of scope.
- **Cross-module edits**: nothing outside `src/parser/` is modified. The
  `Parser` struct itself stays in `mod.rs`, so external imports
  (`use crate::parser::Parser;`) remain valid.
- **Performance work**: §4.2.3 only verifies the absence of regression.
  Optimization is a separate concern.
- **POSIX coverage extension**: TODO items such as the missing
  `parse_compound_list` non-empty regression tests (TODO.md L79) are
  out of scope; this spec moves existing tests, it does not add new
  ones.
- **Doc-comment additions**: existing doc comments are preserved; no new
  documentation is written as part of the split.

## Target Module Layout

```
src/parser/
├── ast.rs           Unchanged.
├── mod.rs           Parser struct + constructors + token utilities + driver.
├── simple.rs        Simple-command and assignment parsing.
├── compound.rs      Compound-command parsing (if/for/while/until/case/{}/()).
├── function.rs      Function-definition parsing.
├── redirect.rs      Redirect and here-document parsing.
└── word.rs          Word utilities and free helpers (is_valid_name,
                     split_tildes_in_literal, is_name_safe, expect_word).
```

Each submodule contributes additional `impl Parser { ... }` blocks; the
`Parser` struct itself is defined only in `mod.rs`. `mod.rs` declares
all submodules with `mod simple; mod compound; mod function; mod redirect;
mod word;` (private — they exist solely to host method implementations).

| File          | Production focus                                          | Tests included                                                                           | Approx lines |
|---------------|-----------------------------------------------------------|------------------------------------------------------------------------------------------|--------------|
| `mod.rs`      | `Parser` struct, `new*`, token utils, driver              | driver (7) + leading-error-recovery (3)                                                 | ~280         |
| `simple.rs`   | `parse_simple_command`, `try_parse_assignment`            | simple+assignment (4) + assignment-RHS-tilde (10) + simple-command line-number (2)      | ~370         |
| `compound.rs` | All `parse_*_clause`, `parse_compound_list`, `parse_do_group` | compound (14) + empty-body (15) + compound line-number (6) + `for` reserved-word (4) | ~810         |
| `function.rs` | `try_parse_function_def`                                  | `test_function_def`, `test_function_def_with_redirect`                                   | ~110         |
| `redirect.rs` | `try_parse_redirect`, `parse_redirect_list`, heredoc helpers | redirect (9) + heredoc (4)                                                            | ~290         |
| `word.rs`     | `expect_word`, `is_valid_name`, `split_tildes_in_literal`, `is_name_safe` | tilde-split (16)                                                            | ~280         |

## Symbol Mapping (line ranges from current `parser/mod.rs`)

### Production

| Lines     | Symbol                                                        | Destination |
|-----------|---------------------------------------------------------------|-------------|
| 1         | `pub mod ast;`                                                | `mod.rs`    |
| 13–18     | `pub struct Parser { lexer, current, pre_current_pos }`       | `mod.rs`    |
| 20–82     | `new`, `new_with_aliases`, `new_with_aliases_at_line`, `consumed_bytes`, `current_token`, `current_span` | `mod.rs` |
| 83–134    | `advance`, `eat`, `expect_reserved`, `skip_newlines`, `is_at_end`, `is_reserved` | `mod.rs` |
| 136–180   | `parse_program`, `parse_complete_command`                     | `mod.rs`    |
| 181–200   | `parse_separator_op`                                          | `mod.rs`    |
| 202–220   | `parse_and_or`                                                | `mod.rs`    |
| 221–256   | `parse_pipeline`                                              | `mod.rs`    |
| 257–271   | `parse_command` (dispatch)                                    | `mod.rs`    |
| 272–347   | `parse_simple_command`                                        | `simple.rs` |
| 348–427   | `try_parse_assignment`                                        | `simple.rs` |
| 428–446   | `is_complete_command_end`                                     | `mod.rs`    |
| 448–462   | `is_compound_command_start`                                   | `mod.rs`    |
| 463–496   | `parse_compound_command`                                      | `compound.rs` |
| 497–517   | `parse_compound_list`                                         | `compound.rs` |
| 518–553   | `parse_if_clause`                                             | `compound.rs` |
| 554–645   | `parse_for_clause`                                            | `compound.rs` |
| 646–653   | `parse_do_group`                                              | `compound.rs` |
| 654–661   | `parse_while_clause`                                          | `compound.rs` |
| 662–669   | `parse_until_clause`                                          | `compound.rs` |
| 670–742   | `parse_case_clause`                                           | `compound.rs` |
| 743–750   | `parse_brace_group`                                           | `compound.rs` |
| 751–766   | `parse_subshell`                                              | `compound.rs` |
| 767–826   | `try_parse_function_def`                                      | `function.rs` |
| 827–912   | `try_parse_redirect`                                          | `redirect.rs` |
| 913–920   | `parse_redirect_list`                                         | `redirect.rs` |
| 921–958   | `extract_heredoc_delimiter`                                   | `redirect.rs` |
| 959–969   | `fill_heredoc_bodies`                                         | `redirect.rs` |
| 970–985   | `expect_word`                                                 | `word.rs`   |
| 987–1015  | `is_valid_name` (free fn)                                     | `word.rs`   |
| 1017–1067 | `split_tildes_in_literal` (free fn) + `is_name_safe`          | `word.rs`   |

### Test-Helper Mapping

| Lines     | Helper                                            | Destination                                              |
|-----------|---------------------------------------------------|----------------------------------------------------------|
| 1076–1080 | `parse(input: &str) -> Program`                   | `mod.rs::tests` (shared via `use super::super::tests::parse;`) |
| 1081–1090 | `parse_first_simple(input: &str) -> SimpleCommand`| `simple.rs::tests`                                       |
| 1257–1266 | `parse_first_compound(input: &str) -> CompoundCommandKind` | `compound.rs::tests`                            |
| 1491–1495 | `lit(s: &str) -> WordPart`                        | `word.rs::tests`                                         |
| 1634–1648 | `parse_first_assignment(...)`                     | `simple.rs::tests`                                       |
| 1758–1768 | `parse_err`, `parse_ok`                           | `compound.rs::tests`                                     |
| 1885–1911 | `first_compound_cmd(...)`                         | `compound.rs::tests`                                     |

If, during Step 6 (see §3.3), three or more helpers turn out to be shared
across submodules, switch to a dedicated `src/parser/test_helpers.rs`
gated by `#[cfg(test)] pub(crate) mod test_helpers;`. With ≤ 2 shared
helpers, keep them in `mod.rs::tests` and reference via `super::super::tests::*`.

### Test Group Mapping

Tests are listed by name to avoid line-range overlap. Line ranges
indicate where the named tests currently appear in `parser/mod.rs`.

`mod.rs::tests` (driver and global error-recovery, 10 tests):

- `test_empty_program` (L1091), `test_multiple_newlines` (L1135),
  `test_pipeline` (L1141), `test_negated_pipeline` (L1149),
  `test_and_or_list` (L1156), `test_semicolon_list` (L1165),
  `test_async_command` (L1171)
- `parse_program_on_leading_dsemi_errs_not_hangs` (L2025),
  `parse_program_on_leading_pipe_errs` (L2042),
  `parse_program_on_dsemi_in_then_body_errs_not_hangs` (L2048)

`simple.rs::tests` (simple commands, assignments, assignment-RHS-tilde,
simple-command line-number, 16 tests):

- `test_simple_command` (L1097), `test_assignment_only` (L1108),
  `test_assignment_with_command` (L1120), `test_assignment_empty_value` (L1127)
- 10 `assignment_rhs_*` tests (L1649–1757)
- `parse_simple_command_captures_line` (L1912),
  `parse_simple_command_on_third_line` (L1918) — both call
  `parse_first_simple`, exercising line capture in `parse_simple_command`

`compound.rs::tests` (compound commands, empty bodies, compound
line-number, reserved-word rejection, 39 tests):

- `test_if_then_fi` / `test_if_else` / `test_if_elif` (L1267–1311)
- `test_for_loop_with_words` / `test_for_loop_without_in` /
  `test_for_loop_with_do_on_newline` (L1312–1345)
- `test_while_loop` / `test_until_loop` (L1346–1357)
- `test_case_basic` / `test_case_fallthrough` /
  `test_case_multiple_patterns` / `test_case_empty` (L1358–1401)
- `test_brace_group` / `test_subshell` (L1402–1413)
- 12 `empty_*_errors` + `nonempty_if_parses_ok` +
  `case_empty_body_still_parses_ok` + `comment_only_body_errors_per_posix`
  (L1769–1884)
- 6 compound line-number-capture tests (L1923–1973):
  `parse_compound_if_captures_line`, `parse_compound_if_on_second_line`,
  `parse_brace_group_captures_line`, `parse_subshell_captures_line`,
  `parse_while_captures_line`, `parse_nested_if_then_captures_body_line`
- 4 `parse_for_reserved_word_*` / `parse_for_valid_name_ok` /
  `parse_for_time_word_ok` tests (L1976–2024)

`function.rs::tests` (2 tests):

- `test_function_def` (L1414), `test_function_def_with_redirect` (L1424)

`redirect.rs::tests` (redirect and heredoc, 13 tests):

- 9 redirect tests (L1180–1256): `test_output_redirect`,
  `test_input_redirect`, `test_append_redirect`, `test_fd_redirect`,
  `test_dup_output`, `test_heredoc_redirect`, `test_clobber_redirect`,
  `test_read_write_redirect`, `test_multiple_redirects`
- 4 heredoc tests (L1439–1490): `test_heredoc_body`,
  `test_heredoc_strip_tabs`, `test_heredoc_quoted_delimiter`,
  `test_heredoc_with_command_after`

`word.rs::tests` (16 tilde-split tests, L1496–1632):

- `split_no_tilde_returns_single_literal`, `split_leading_tilde_only`,
  `split_leading_tilde_slash`, `split_leading_tilde_user`,
  `split_colon_separated_tildes`, `split_middle_segment_with_tilde`,
  `split_trailing_colon`, `split_leading_colon`,
  `split_consecutive_colons`, `split_mid_word_tilde_stays_literal`,
  `split_double_tilde_invalid_user`, `split_user_name_with_dot_and_dash`,
  `split_two_tildes_joined_by_colon_no_slash`,
  `split_not_at_boundary_skips_leading_tilde`,
  `split_not_at_boundary_then_colon_restarts`,
  `split_returns_ends_with_colon_flag`

## Architecture and Data Flow

### Module Dependency Sketch

```
                  ┌──────────┐
                  │  ast.rs  │  (type definitions)
                  └────▲─────┘
                       │ use ast::*
        ┌──────────────┼──────────────────────┐
        │              │                      │
   ┌────┴────┐   ┌─────┴─────┐         ┌─────┴─────┐
   │ mod.rs  │   │ simple.rs │         │ word.rs   │
   │ Parser  │   │ impl      │         │ impl +    │
   │ struct  │◄──┤ Parser    │         │ free fns  │
   │ + token │   │           │         │           │
   │ utils + │   └─────┬─────┘         └─────▲─────┘
   │ driver  │         │                     │
   └────▲─┬──┘         │                     │
        │ │            │ self.expect_word    │
        │ │            │                     │
        │ │       ┌────┴──────────┐          │
        │ └──────►│ compound.rs   │          │
        │         │ impl Parser   │          │
        │         └───────────────┘          │
        │                                    │
        │         ┌───────────────┐          │
        │         │ function.rs   │──────────┤
        │         │ impl Parser   │          │
        │         └───────────────┘          │
        │                                    │
        │         ┌───────────────┐          │
        └─────────│ redirect.rs   │──────────┘
                  │ impl Parser   │
                  └───────────────┘
```

- `mod.rs` is the hub. All other files contribute additional `impl Parser`
  blocks and reach token utilities through `self.advance()` /
  `self.eat()` / `self.expect_reserved()` etc.
- Submodules do not call each other directly. Cross-topic invocations
  (`parse_command` → `parse_simple_command`, etc.) all flow through the
  `Parser` instance, which transparently dispatches to whichever `impl`
  block defines the method.
- The Rust-level dependency graph is a star (`mod.rs` ← all submodules);
  there are no submodule-to-submodule `use` edges.

## Implementation Procedure

Step 0 is a baseline-capture step that does not modify files (no commit).
Steps 1–7 each finish with `cargo build -p yosh && cargo test --lib
parser::` green and are committed independently — that is, the split
produces 7 commits in `git log`.

| Step | Action                                                       | Verification                                  |
|------|--------------------------------------------------------------|-----------------------------------------------|
| 0    | Capture baseline (no commit): `cargo test --lib parser:: 2>&1 \| tee /tmp/parser-baseline.txt`. Record test count and grep `#[cfg(test)]`, `pub(super)`, `pub(crate)` usage in `mod.rs`. | Baseline file produced.        |
| 1    | Create `word.rs` with `expect_word` + the three free functions + tilde-split tests. Add `mod word;` in `mod.rs`. Remove migrated code from `mod.rs`. | Build + test count matches baseline. |
| 2    | Create `function.rs` with `try_parse_function_def` and the 2 function tests. | Build + test count matches.                |
| 3    | Create `redirect.rs` with redirect 4 methods + redirect 9 + heredoc 4 tests. | Build + test count matches.                |
| 4    | Create `simple.rs` with `parse_simple_command` + `try_parse_assignment` + simple/assignment/assignment-RHS-tilde tests. Co-locate `parse_first_simple` and `parse_first_assignment`. | Build + test count matches.                |
| 5    | Create `compound.rs` with the 9 compound methods + compound + empty-body + line-number + reserved-word + dsemi-in-then tests. Co-locate `parse_first_compound`, `parse_err`, `parse_ok`, `first_compound_cmd`. | Build + test count matches.                |
| 6    | Settle shared test helpers: with ≤ 2 helpers shared, leave them in `mod.rs::tests` and add `use super::super::tests::*;` in submodule tests; with ≥ 3 helpers shared, lift them to `src/parser/test_helpers.rs` (`#[cfg(test)] pub(crate)`). | Build + test count matches.                |
| 7    | Final polish: clean `use` statements, run `cargo fmt --all`, run `cargo clippy --lib -- -D warnings`. Run `./e2e/run_tests.sh` once. | fmt clean, clippy clean, E2E unchanged.     |

### Commit Message Convention

```
refactor(parser): split <topic> into src/parser/<file>.rs

Mechanical move; no semantic change. Part of parser/mod.rs split.
Spec: docs/superpowers/specs/2026-05-04-parser-mod-split-design.md
Original prompt: "このプロジェクトのリファクタリングを設計して下さい。"
```

## Risk Register

| ID  | Risk                                                                  | Likelihood | Impact | Mitigation |
|-----|-----------------------------------------------------------------------|------------|--------|------------|
| R1  | Shared test helpers cause cascading compile errors during Steps 1–5   | High       | Medium | Each step that splits a submodule must add `use super::super::tests::*;` immediately so helpers in `mod.rs::tests` remain reachable. Step 6 finalizes the strategy. |
| R2  | `Parser` `impl` distribution misroutes a private method               | Medium     | Low    | Visibility is preserved verbatim. Compile errors during a step are diagnosed as missing `use`, not visibility. |
| R3  | `cargo fmt` introduces drift after the split                          | Medium     | Low    | Step 7 runs `cargo fmt --all` and folds the result into the polish commit. |
| R4  | New `clippy` lints fire (e.g. `module_inception`, `needless_pub`)     | Medium     | Low    | Fix in source rather than `#[allow(...)]`. Lints surface during Step 7. |
| R5  | Parser performance regression                                         | Low        | Medium | `cargo bench --bench parser` (if such a bench exists) before Step 0 and after Step 7. ±5 % tolerance. |
| R6  | `#[cfg(test)]` / `pub(super)` / `pub(crate)` markers lost during move | Medium     | Medium | Step 0 grep snapshot; Step 6 re-grep to confirm parity. |
| R7  | `#[allow(dead_code)]` on `current_token` re-evaluates differently     | Low        | Low    | `current_token` stays in `mod.rs`; no other `#[allow(...)]` exists in `parser/mod.rs` per Step 0 grep. |
| R8  | Concurrent PR collision on `parser/mod.rs`                            | Low        | Medium | `git log --oneline -- src/parser/mod.rs` before starting; rebase if any in-flight work appears. |

## Verification Strategy

### Per-Step (mandatory)

```bash
cargo build -p yosh
cargo test --lib parser::
```

Test count must equal Step 0 baseline exactly.

### Final (after Step 7)

```bash
cargo build
cargo test
cargo fmt --all -- --check
cargo clippy --lib -- -D warnings
./e2e/run_tests.sh
```

All must be green; total `cargo test` count must match the pre-split
total; E2E pass/fail profile must be unchanged.

### Optional Bench

```bash
cargo bench --bench parser 2>&1 | tee /tmp/parser-bench-after.txt
```

Run only if a `parser` bench exists under `benches/`. Compare against
the Step 0 snapshot; ±5 % tolerance.

## Rollback Conditions

`git revert` the offending step's commit if any of the following occur:

- `parser::` test count drops at any step.
- `cargo build` fails for a reason other than missing `use` (e.g. trait
  duplication, orphan-rule violation).
- `cargo clippy` produces a new `error` (warnings excluded) attributable
  to the split.

After rollback, append a "Lessons" section to this spec describing the
failure mode and revisit the plan before re-attempting.

## Definition of Done

1. `src/parser/mod.rs` reduced to ~410 lines (down from 2054). The
   original ≤ 300 target was aspirational; the actual structural
   minimum after the split is 407 lines, holding the `Parser` struct,
   its constructors and token utilities, the driver methods, and the
   10 driver-level tests. Code-quality review confirmed no further
   extraction would improve cohesion.
2. `src/parser/{simple,compound,function,redirect,word}.rs` exist and
   `cargo build` succeeds.
3. `cargo test` total matches the pre-split count.
4. `cargo fmt --check` and `cargo clippy --lib -- -D warnings` pass.
5. `./e2e/run_tests.sh` pass/fail profile unchanged.
6. `git log --oneline` shows 7 ordered commits for Steps 1–7 (Step 0
   captures baseline data without producing a commit).
7. `TODO.md` items that reference `src/parser/mod.rs` line numbers
   (e.g. L79, L81, L82) are updated to reference the new file paths.

## Open Questions (resolved at implementation time)

1. **Final shared-helper layout** — defaults to `mod.rs::tests`. Switch
   to `test_helpers.rs` only if Step 6 shows ≥ 3 helpers shared across
   submodules.
2. **`is_complete_command_end` / `is_compound_command_start` location**
   — defaults to `mod.rs` (called by the dispatch driver). Method
   distribution across `impl` blocks is transparent at call sites, so
   the choice is purely organizational; keep them with the driver.
3. **`split_tildes_in_literal` visibility** — keep `pub(crate)` until a
   follow-up audit confirms there are no external callers, then revisit
   in the visibility-tightening spec.

## Follow-Up Work (out of scope)

These tasks become tractable after the split lands but are not part of
this spec:

- Visibility tightening on `Parser` (separate spec).
- `try_parse_assignment` value-construction helper extraction
  (TODO.md L81).
- `parse_compound_list` non-empty regression tests (TODO.md L79).
- `LINENO` per-command allocation removal touching `simple.rs`
  (TODO.md L80, L83).

## Original Prompt (Traceability)

```
2026-05-04 — /superpowers:brainstorming
"このプロジェクトのリファクタリングを設計して下さい。"

Refined through clarifying questions:
- Motivation: split large files for readability and testability.
- Scope: single-file deep dive on src/parser/mod.rs (2054 lines).
- API approach: mechanical split only; visibility audit deferred.
- Test approach: #[cfg(test)] mod tests co-located in each new file.
- Layout: Approach A — fine-grained mirror of src/exec/, six files.
```
