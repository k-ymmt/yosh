# Parser Visibility Tightening Design

**Date**: 2026-05-05
**Status**: Design proposed
**Scope**: Demote 26 `pub fn` methods on `Parser` to `pub(super) fn`, demote one
`pub(crate) fn` to `pub(super) fn`, and remove one stale `#[allow(dead_code)]`.
Mechanical 28-line edit, single commit.

## Overview

Following the parser/mod.rs mechanical split (spec
`2026-05-04-parser-mod-split-design.md`), the `Parser` struct now exposes 36
`pub fn` methods, many of which are recursive-descent helpers (e.g.
`parse_pipeline`, `parse_if_clause`) that were never intended as a public
API. They are `pub` only because the original monolithic file kept everything
public for convenience; nothing in the post-split codebase calls them from
outside `src/parser/`.

A callsite audit across `src/`, `crates/`, `tests/`, and `benches/` confirmed
that:

- 10 methods (plus the `Parser` struct itself) are reached from binaries,
  benches, or other modules within the `yosh` crate. These must remain `pub`.
- 26 methods are reached only from within `src/parser/`. They can drop to
  `pub(super) fn`, which is the minimum visibility that lets multiple
  submodules within `parser/` share `impl Parser` blocks.
- The free function `split_tildes_in_literal`, currently `pub(crate)`, is
  called only from `src/parser/simple.rs::try_parse_assignment` (within
  `parser/`). It can drop to `pub(super)`.
- The `#[allow(dead_code)]` on `current_token` is stale: the method has a
  real external caller (`src/interactive/parse_status.rs:61`).

The total reduction is `pub fn` count 36 → 10 (72% surface reduction). The
change is purely visibility-tightening; no `fn` body, signature, or callsite
is modified.

## Goals

1. Reduce the `pub fn` surface on `Parser` from 36 to 10.
2. Demote one `pub(crate) fn` (`split_tildes_in_literal`) to `pub(super)` to
   match its actual reach.
3. Remove the stale `#[allow(dead_code)]` on `current_token`.
4. Land all changes in a single commit, since the edits are mechanical and
   sit under one architectural intent.

## Non-Goals

- **`pub(crate)` intermediate**: zero `Parser` methods qualify, since every
  external caller is a binary or bench (which see only `pub`). Not in scope.
- **API shape changes**: `try_parse_assignment`'s `pub fn(word: &Word)`
  static-method shape, `current_token`'s `&Token` return type, etc., are
  not revisited.
- **Method body changes**: no logic edits, no parameter additions, no
  return-type changes.
- **New public API**: items that are currently `pub(super)` (e.g.
  `is_valid_name`, the test helpers `parse` and `parse_first_simple`) are
  not promoted.
- **Doc-comment additions**: the 27 demoted items do not get new
  "internal helper" comments.
- **Test changes**: the 99 parser tests stay as-is.
- **`Parser` struct member visibility**: `lexer`, `current`,
  `pre_current_pos` remain private — already minimal.
- **Cross-module edits**: nothing outside `src/parser/` is touched. The
  audit confirmed all external callers point at items that stay `pub`.
- **TODO.md / spec / plan updates**: visibility-only edits do not generate
  new TODO items; no documentation update needed beyond this spec.

## Visibility Matrix

### Stay `pub` (10 methods + `Parser` struct)

| File:Line              | Item                              | External caller(s)                                                                                                |
|------------------------|-----------------------------------|-------------------------------------------------------------------------------------------------------------------|
| `mod.rs:13`            | `pub struct Parser`               | type referenced externally (cannot change)                                                                        |
| `mod.rs:21`            | `Parser::new`                     | `benches/parser_bench.rs:20`, `benches/exec_bench.rs:10`, `bin/yosh-dhat.rs:47`, `expand/mod.rs:223,296`, `expand/arith.rs:94`, `main.rs:169` |
| `mod.rs:35`            | `Parser::new_with_aliases`        | `exec/mod.rs:128,234`, `interactive/parse_status.rs:51,113`, `builtin/special.rs:311`                             |
| `mod.rs:51`            | `Parser::new_with_aliases_at_line`| `main.rs:240`                                                                                                     |
| `mod.rs:70`            | `consumed_bytes`                  | `main.rs:250`                                                                                                     |
| `mod.rs:75`            | `current_token`                   | `interactive/parse_status.rs:61`                                                                                  |
| `mod.rs:83`            | `advance`                         | `interactive/parse_status.rs:63`                                                                                  |
| `mod.rs:126`           | `is_at_end`                       | `main.rs:245`, `interactive/parse_status.rs:55,71,89`                                                             |
| `mod.rs:136`           | `parse_program`                   | `benches/parser_bench.rs:21`, `benches/exec_bench.rs`, `bin/yosh-dhat.rs:47`, `exec/mod.rs:234`, `expand/mod.rs:223,296`, `expand/arith.rs:94`, `builtin/special.rs:311`, `main.rs:169` |
| `mod.rs:147`           | `parse_complete_command`          | `main.rs:248`, `interactive/parse_status.rs:75`                                                                   |
| `simple.rs:84`         | `try_parse_assignment`            | `exec/simple.rs:33`                                                                                               |

`pub(crate)` is not sufficient for any of these because binaries
(`src/main.rs`, `src/bin/yosh-dhat.rs`) and benches (`benches/*.rs`) compile
as separate crates that consume the `yosh` library through its public API.

### Drop `pub fn` → `pub(super) fn` (26 methods)

#### `mod.rs` (11 methods)

| Line | Method                       |
|------|------------------------------|
| 79   | `current_span`               |
| 90   | `eat`                        |
| 100  | `expect_reserved`            |
| 116  | `skip_newlines`              |
| 130  | `is_reserved`                |
| 181  | `parse_separator_op`         |
| 202  | `parse_and_or`               |
| 221  | `parse_pipeline`             |
| 257  | `parse_command`              |
| 273  | `is_complete_command_end`    |
| 293  | `is_compound_command_start`  |

#### `word.rs` (1 method)

| Line | Method        |
|------|---------------|
| 7    | `expect_word` |

#### `redirect.rs` (2 methods)

| Line | Method                |
|------|-----------------------|
| 7    | `try_parse_redirect`  |
| 93   | `parse_redirect_list` |

#### `function.rs` (1 method)

| Line | Method                  |
|------|-------------------------|
| 10   | `try_parse_function_def`|

#### `simple.rs` (1 method)

| Line | Method                 |
|------|------------------------|
| 8    | `parse_simple_command` |

#### `compound.rs` (10 methods)

| Line | Method                    |
|------|---------------------------|
| 8    | `parse_compound_command`  |
| 42   | `parse_compound_list`     |
| 63   | `parse_if_clause`         |
| 99   | `parse_for_clause`        |
| 191  | `parse_do_group`          |
| 199  | `parse_while_clause`      |
| 207  | `parse_until_clause`      |
| 215  | `parse_case_clause`       |
| 288  | `parse_brace_group`       |
| 296  | `parse_subshell`          |

### Drop `pub(crate) fn` → `pub(super) fn` (1 free function)

| File:Line       | Item                       | Internal caller                          |
|-----------------|----------------------------|------------------------------------------|
| `word.rs:54`    | `split_tildes_in_literal`  | `simple.rs::try_parse_assignment` only   |

### Stale `#[allow(dead_code)]` removal

| File:Line   | Item                                               | Reason                                                                 |
|-------------|----------------------------------------------------|------------------------------------------------------------------------|
| `mod.rs:74` | `#[allow(dead_code)]` on `pub fn current_token`    | `current_token` is reachable via `interactive/parse_status.rs:61`; allow is stale |

### Totals

- 26 methods: `pub fn` → `pub(super) fn`
- 1 free fn: `pub(crate) fn` → `pub(super) fn`
- 1 attribute line: removed
- **28 line edits across 6 files**

## Implementation Procedure

Single commit. Apply edits per file with intermediate `cargo build -p yosh`
checks so a missed external caller is localized to the file in which it
appears.

| Step | Action                                                                 | Verification             |
|------|------------------------------------------------------------------------|--------------------------|
| 0    | Capture baseline: `cargo test --lib parser:: 2>&1 \| grep "test result"`. Confirm `99 passed`. Record `wc -l src/parser/*.rs`. | Baseline data printed.   |
| 1    | Edit `mod.rs`: 11 demotions + remove `#[allow(dead_code)]` on `current_token`. | `cargo build -p yosh`    |
| 2    | Edit `word.rs`: demote `expect_word` and `split_tildes_in_literal`.    | `cargo build -p yosh`    |
| 3    | Edit `function.rs`: demote `try_parse_function_def`.                   | `cargo build -p yosh`    |
| 4    | Edit `redirect.rs`: demote `try_parse_redirect` and `parse_redirect_list`. | `cargo build -p yosh` |
| 5    | Edit `simple.rs`: demote `parse_simple_command`.                       | `cargo build -p yosh`    |
| 6    | Edit `compound.rs`: 10 demotions.                                      | `cargo build -p yosh`    |
| 7    | Run final verification: `cargo build && cargo test --lib && cargo fmt --check src/parser/ && cargo clippy --lib -- -D warnings`. Optional: `./e2e/run_tests.sh`. | All green; `cargo test --lib` matches baseline. |
| 8    | Commit (single).                                                       | `git log -1` shows new commit. |

### Commit Message

```
refactor(parser): tighten visibility of internal-only methods

Drops 26 `pub fn` to `pub(super) fn` and the free fn
`split_tildes_in_literal` from `pub(crate)` to `pub(super)`,
based on a callsite audit showing no external callers (binary,
bench, or other-crate). Also removes a stale `#[allow(dead_code)]`
on `current_token`, which is reachable via parse_status.rs.

API surface reduction: 36 → 10 `pub fn` on Parser (72%).

Spec: docs/superpowers/specs/2026-05-05-parser-visibility-tightening-design.md
Original prompt: parser/mod.rs split の follow-up — Parser API の可視性整理。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

## Risk Register

| ID | Risk                                                                       | Likelihood | Impact | Mitigation                                                                                              |
|----|----------------------------------------------------------------------------|------------|--------|---------------------------------------------------------------------------------------------------------|
| R1 | grep audit missed an external caller; compilation fails after demotion     | Medium     | Low    | Per-file `cargo build` localizes the failure. Compiler points at the offending callsite. Restore `pub` on the affected method and re-build. |
| R2 | proc-macro / `#[derive]` reflectively reaches a demoted method             | Very low   | Low    | yosh's parser has no proc-macro derives requiring `pub` access; verified in `Cargo.toml`.                |
| R3 | doc-tests reference demoted items                                          | Low        | Low    | yosh's parser has no doc-tests; verified by `grep -rn '/// \?\(pub fn\|use parser\|Parser::\)' src/parser/`. |
| R4 | rustdoc output shrinks                                                     | Certain    | None   | This is the goal: internal API disappears from generated docs.                                          |
| R5 | new clippy lint fires (e.g. `unreachable_pub`)                             | Low        | Low    | `pub(super)` resolves the existing `unreachable_pub` direction. New unrelated lints can be addressed inline. |
| R6 | rustfmt re-flows around edited lines                                       | Medium     | None   | `pub fn` → `pub(super) fn` is in-line. Step 7 runs `cargo fmt --check src/parser/` to confirm.          |

## Verification Strategy

### Per-Step

```bash
cargo build -p yosh
```

After each file edit. Compiler is the source of truth for whether a
demotion broke an external caller.

### Final

```bash
cargo build                              # all targets (lib, bins, benches)
cargo test --lib parser::                # 99 passed (baseline match)
cargo test --lib                         # 903 passed (full lib baseline)
cargo fmt --check src/parser/*.rs        # parser/ fmt-clean
cargo clippy --lib -- -D warnings 2>&1 | grep -A2 'parser/'   # parser/* clean
./e2e/run_tests.sh                       # 393/393 (optional but recommended)
```

`cargo build` (plain, all targets) is critical: it builds binaries and
benches as separate compilation units, which is what verifies the
crate-external visibility surface. `cargo build -p yosh` (lib only) does
not exercise that surface.

## Rollback

If any of the following occur, `git revert` or `git reset --soft HEAD`:

- `cargo build` fails outside `src/parser/` after a demotion (audit miss).
- `cargo test --lib` count drops below the 903 baseline.
- `cargo clippy --lib -- -D warnings` produces a new error attributable
  to `src/parser/*`.
- E2E tests show a new failure not present pre-commit.

Partial rollback option: if exactly one demoted method has an external
caller, restore that single `pub` and amend the commit before pushing.

## Definition of Done

1. `pub fn` count on `Parser` reduced from 36 to 10.
2. `split_tildes_in_literal` is `pub(super) fn`.
3. `#[allow(dead_code)]` on `current_token` removed.
4. `cargo build` (all targets) succeeds.
5. `cargo test --lib` total count matches the pre-change baseline (903).
6. `cargo fmt --check src/parser/` is clean.
7. `cargo clippy --lib -- -D warnings` produces no new lint inside
   `src/parser/*` (pre-existing `src/plugin/mod.rs` lints are unrelated).
8. E2E suite pass/fail profile unchanged.
9. A single commit appears in `git log` with the spec's required message
   format.

## Open Questions (resolved at implementation time)

1. **Should `current_span` move with `current_token` to a private fn?**
   No — keeping it `pub(super)` is the minimum that lets cross-submodule
   parser code call it. Verified `current_span` has no external callers.
2. **Should `Parser::new_with_aliases_at_line` be considered for `pub(crate)`?**
   No — it is called from `src/main.rs:240`, and `main.rs` is a separate
   compilation crate that requires `pub`.

## Follow-Up Work (out of scope)

These items become tractable after this tightening but are not part of
this spec:

1. **`current_token` API shape**: callers compare against
   `&Token::Newline` boilerplate; a `is_token(&self, t: &Token) -> bool`
   predicate would be ergonomic.
2. **`try_parse_assignment` static-fn vs free-fn**: it is `Parser::try_parse_assignment(word: &Word)` taking no `self`; could become `pub fn try_parse_assignment(word: &Word)` at the module level.
3. **Bench API surface**: `Parser::new` and `parse_program` are the only
   two items benches require. A bench-only helper module that wraps them
   could let both drop to `pub(crate)`. Requires bench refactor.

## Original Prompt (Traceability)

```
2026-05-05 — /superpowers:brainstorming
"Parser API の可視性整理(parser/mod.rs split の follow-up)。
 `pub fn` を実呼び出しに基づき `pub(crate)` または `pub(super)` に
 降格する。`split_tildes_in_literal` 含む。"

Refined through clarifying questions:
- Audit: grep all callsites in src/, crates/, tests/, benches/ to determine
  external (binary/bench) vs parser-internal usage.
- Result: zero items qualify for `pub(crate)` because every external caller
  reaches the symbol via a binary or bench (separate compilation units that
  see only `pub`).
- Scope: 26 methods `pub` → `pub(super)`, 1 free fn `pub(crate)` →
  `pub(super)`, 1 stale `#[allow(dead_code)]` removed.
- API surface: 36 → 10 `pub fn` (72% reduction).
- Phasing: single commit (28 mechanical line edits) with per-file
  intermediate `cargo build` checks.
```
