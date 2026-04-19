# POSIX Chapter 2 Conformance Gaps — Sub-project 2: §2.10.1 / §2.10.2 Grammar E2E Coverage

**Date**: 2026-04-19
**Sub-project**: 2 of 5 (POSIX Chapter 2 conformance gap remediation)
**Scope items from TODO.md**:

- §2.10.1 Shell Grammar Lexical Conventions — dedicated tests to be added when lexer spec-compliance is revisited
- §2.10.2 Shell Grammar Rules — dedicated grammar-rule tests to be added

## Context

Sub-project 1 closed the §2.7.5 / §2.7.7 / §2.14.13 gaps. This sub-
project addresses the two remaining "dedicated tests" line items in
TODO.md's "Future: POSIX Conformance Gaps (Chapter 2)" section. Both
are pure test-addition items — the lexer / parser already implement
most rules; what's missing is POSIX-referenced E2E coverage.

Existing coverage context:

- `e2e/posix_spec/2_10_shell_grammar/` has 13 tests covering empty
  compound_list rejection (Rules around Rule 9), case empty body,
  `;` vs newline terminator, and compound_list with newlines. All
  have coarse `POSIX_REF: 2.10 Shell Grammar`.
- `e2e/posix_spec/2_04_reserved_words/` (4 tests) covers parts of
  Rule 10 (keyword recognition in command position) from a Reserved
  Words angle. We will not duplicate those.
- `e2e/posix_spec/2_03_token_recognition/` (3 tests) covers lexer-
  level behavior related to §2.10.1.

Grammar Rules inventory (POSIX XCU §2.10.2):

1. [Command Name] — reserved word vs WORD in command position
2. [Redirection to/from filename] — filename is always WORD
3. [Redirection from here-document] — delimiter is always WORD
4. [Case statement termination] — `esac` handling, `DSEMI`
5. [NAME in for] — must be a valid name, not reserved
6. [Third word of for/case: `in`] — recognized as keyword
7. [Assignment preceding command name] — tokens with leading NAME=
8. [NAME in function] — valid name, not reserved
9. [Body of function / compound_command] — must be a compound_command
10. [Keyword recognition] — reserved words only in command position

Rules 1 and 10 are closely related (both about command-position
keyword recognition); we fold Rule 1 tests into Rule 10 coverage to
avoid duplication.

## Goals

1. Add ~27 new E2E tests covering Rules 2–10 representative
   scenarios and §2.10.1 lexical disambiguation.
2. Upgrade the existing 13 `e2e/posix_spec/2_10_shell_grammar/*.sh`
   `POSIX_REF` headers from coarse `2.10 Shell Grammar` to
   Rule-specific `2.10.2 Rule N - <Rule Name>`.
3. Remove both TODO.md items from "Future: POSIX Conformance Gaps
   (Chapter 2)" on clean pass, or rewrite with concrete XFAIL
   references if edge cases uncover implementation gaps.

## Non-goals

- shall/must-clause granular coverage per Rule (tracked separately
  as "Deepen Chapter 2 POSIX coverage to normative-requirement
  granularity" in TODO.md — out of scope here).
- Migrating legacy `e2e/control_flow/`, `e2e/pipeline_and_list/`,
  `e2e/variable_and_expansion/` tests to `POSIX_REF` headers.
- Implementation changes to `src/lexer/` or `src/parser/`. Failing
  edge cases are XFAIL'd with specific reasons and deferred.
- Rule 1 as a standalone set — merged into Rule 10 tests.

## Architecture

Pure test additions. No `src/` changes. Two directories involved:

```
e2e/posix_spec/
├── 2_10_1_lexical/            (new, 3 files)
│   ├── operator_vs_word.sh
│   ├── operator_extended_max_munch.sh
│   └── comment_terminates_at_newline.sh
└── 2_10_shell_grammar/        (existing, 13 + 24 files)
    ├── <existing 13 files — POSIX_REF headers updated>
    ├── rule07_single_assignment.sh
    ├── rule07_transient_assignment.sh
    ├── rule07_not_at_word_position.sh
    ├── rule10_reserved_after_cmd_is_arg.sh
    ├── rule10_reserved_quoted_not_recognized.sh
    ├── rule10_reserved_after_pipe_in_cmdpos.sh
    ├── rule02_filename_reserved_word.sh
    ├── rule02_filename_leading_dash.sh
    ├── rule03_heredoc_delim_reserved_word.sh
    ├── rule03_heredoc_delim_quoted_reserved_word.sh
    ├── rule04_case_last_item_no_dsemi.sh
    ├── rule04_case_empty_pattern_list_not_allowed.sh
    ├── rule05_for_valid_name.sh
    ├── rule05_for_invalid_name.sh
    ├── rule05_for_reserved_word_rejected.sh
    ├── rule06_for_in_recognized.sh
    ├── rule06_for_without_in_uses_positional.sh
    ├── rule06_case_in_recognized.sh
    ├── rule08_function_valid_name.sh
    ├── rule08_function_reserved_name_rejected.sh
    ├── rule08_function_invalid_name.sh
    ├── rule09_function_body_brace_group.sh
    ├── rule09_function_body_subshell.sh
    └── rule09_function_body_simple_cmd_rejected.sh
```

Rule-numbered prefix gives filter-scoping (`--filter=rule07`) and
cross-Rule sorting by `ls`.

## Test Inventory

### Rule 2 [Redirection filename]

| File | Scenario | Verification |
|---|---|---|
| `rule02_filename_reserved_word.sh` | `echo hi > "$TEST_TMPDIR/if"; cat "$TEST_TMPDIR/if"` — `if` as filename, Rule 2 disables reserved-word recognition | `EXPECT_OUTPUT: hi` |
| `rule02_filename_leading_dash.sh` | `echo hi > "$TEST_TMPDIR/-flag"; cat -- "$TEST_TMPDIR/-flag"` — filename with operator-like lead | `EXPECT_OUTPUT: hi` |

### Rule 3 [Here-document delimiter]

| File | Scenario | Verification |
|---|---|---|
| `rule03_heredoc_delim_reserved_word.sh` | `cat <<if\nhello\nif` — reserved word `if` as unquoted delimiter | `EXPECT_OUTPUT: hello` |
| `rule03_heredoc_delim_quoted_reserved_word.sh` | `cat <<'if'\n$X\nif` — quoted reserved-word delimiter disables body expansion | `EXPECT_OUTPUT: $X` (literal, not expanded) |

### Rule 4 [Case statement termination]

| File | Scenario | Verification |
|---|---|---|
| `rule04_case_last_item_no_dsemi.sh` | `case x in a) echo a esac` without trailing `;;` | `EXPECT_OUTPUT: a` |
| `rule04_case_empty_pattern_list_not_allowed.sh` | `case x in ) echo;; esac` — empty pattern list | `EXPECT_EXIT: 2`, stderr `yosh:` |

### Rule 5 [NAME in for]

| File | Scenario | Verification |
|---|---|---|
| `rule05_for_valid_name.sh` | `for x in a b; do echo $x; done` | `EXPECT_OUTPUT` two lines `a\nb` |
| `rule05_for_invalid_name.sh` | `for 1x in a; do :; done` — name cannot start with digit | `EXPECT_EXIT: 2`, stderr `yosh:` |
| `rule05_for_reserved_word_rejected.sh` | `for if in a; do :; done` — reserved word as NAME | `EXPECT_EXIT: 2`, stderr `yosh:` |

### Rule 6 [Third word `in` recognition]

| File | Scenario | Verification |
|---|---|---|
| `rule06_for_in_recognized.sh` | `for x in 1 2; do echo $x; done` | `EXPECT_OUTPUT` two lines |
| `rule06_for_without_in_uses_positional.sh` | `set -- a b; for x do echo $x; done` — omitted `in` defaults to `"$@"` | `EXPECT_OUTPUT` two lines `a\nb` |
| `rule06_case_in_recognized.sh` | `case x in x) echo y;; esac` | `EXPECT_OUTPUT: y` |

### Rule 7 [Assignment preceding command]

| File | Scenario | Verification |
|---|---|---|
| `rule07_single_assignment.sh` | `FOO=bar; echo $FOO` | `EXPECT_OUTPUT: bar` |
| `rule07_transient_assignment.sh` | `A=1 env \| grep '^A=1' >/dev/null` — transient per POSIX | `EXPECT_EXIT: 0` |
| `rule07_not_at_word_position.sh` | `echo A=1` — after command, not assignment | `EXPECT_OUTPUT: A=1` |

### Rule 8 [NAME in function]

| File | Scenario | Verification |
|---|---|---|
| `rule08_function_valid_name.sh` | `f() { echo ok; }; f` | `EXPECT_OUTPUT: ok` |
| `rule08_function_reserved_name_rejected.sh` | `if() { :; }` — reserved word as function name | `EXPECT_EXIT: 2`, stderr `yosh:` |
| `rule08_function_invalid_name.sh` | `1f() { :; }` — name cannot start with digit | `EXPECT_EXIT: 2`, stderr `yosh:` |

### Rule 9 [Body of function]

| File | Scenario | Verification |
|---|---|---|
| `rule09_function_body_brace_group.sh` | `f() { echo ok; }; f` — brace body | `EXPECT_OUTPUT: ok` |
| `rule09_function_body_subshell.sh` | `f() ( echo ok ); f` — subshell body | `EXPECT_OUTPUT: ok` |
| `rule09_function_body_simple_cmd_rejected.sh` | `f() echo ok` — simple command body | `EXPECT_EXIT: 2`, stderr `yosh:` |

### Rule 10 [Keyword recognition]

| File | Scenario | Verification |
|---|---|---|
| `rule10_reserved_after_cmd_is_arg.sh` | `echo if` — `if` as argument, not keyword | `EXPECT_OUTPUT: if` |
| `rule10_reserved_quoted_not_recognized.sh` | `'if' true` — quoted `if` → external command lookup | exit 127 (command not found); skip if `command -v if` finds it |
| `rule10_reserved_after_pipe_in_cmdpos.sh` | `echo x \| (if true; then cat; fi)` — command position after pipe | `EXPECT_OUTPUT: x` |

### §2.10.1 Lexical Conventions (`2_10_1_lexical/`)

| File | Scenario | Verification |
|---|---|---|
| `operator_vs_word.sh` | `echo a&&echo b` — `&&` recognized without surrounding whitespace | `EXPECT_OUTPUT` two lines |
| `operator_extended_max_munch.sh` | `echo a\|\|echo b` — `\|\|` tokenized as one operator, not two `\|` | `EXPECT_OUTPUT: a` (|| short-circuits; first branch succeeds) |
| `comment_terminates_at_newline.sh` | `echo a # comment\necho b` | `EXPECT_OUTPUT` two lines |

### Existing File POSIX_REF Migration

| Existing file | Old | New |
|---|---|---|
| `case_empty_body_is_ok.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 4 - Case statement termination` |
| `compound_list_newline_between_commands.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list` |
| `empty_brace_group_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_command` |
| `empty_subshell_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_command` |
| `empty_compound_list_in_if_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (if)` |
| `empty_elif_body_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (elif)` |
| `empty_else_body_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (else)` |
| `empty_while_condition_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (while cond)` |
| `empty_while_body_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (while body)` |
| `empty_until_condition_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (until cond)` |
| `empty_for_body_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (for body)` |
| `empty_if_condition_is_error.sh` | `2.10 Shell Grammar` | `2.10.2 Rule 9 - Body of compound_list (if cond)` |
| `terminator_semicolon_equals_newline.sh` | `2.10 Shell Grammar` | `2.10 Shell Grammar - List terminators` (no specific Rule) |

## Verification Strategy

**Deterministic success** → `EXPECT_OUTPUT: <literal>` or `EXPECT_OUTPUT<<DELIM` heredoc.

**Syntax error** → `EXPECT_EXIT: 2` + `EXPECT_STDERR: yosh:` (harness uses substring match per Sub-project 1 findings).

**Runtime failure (command not found)** → `EXPECT_EXIT: 127` + in-script skip if the command happens to exist on PATH. Template:

```sh
command -v if >/dev/null 2>&1 && { echo "skipping: 'if' found on PATH" >&2; exit 0; }
'if' true
```

**Environment-dependent heredoc / expansion tests** → in-script `case` verification mirroring `tilde_rhs_user_form.sh`.

All pattern checks use POSIX `case` glob. No `grep -E`, no bash-isms.

## Workflow

### Step 0 — Harness pre-check

- Confirm latest `cargo build` succeeds.
- Read 1–2 existing `heredoc_*.sh` (e.g., `e2e/redirection/heredoc_basic.sh`) to confirm the harness handles heredocs cleanly when the test file itself contains a `cat <<DELIM` block.
- Record baseline: `./e2e/run_tests.sh 2>&1 | tail -3` (currently 337 pass / 0 fail).

### Step 1 (Commit ①) — POSIX_REF migration of existing 13 files

For each file in the migration table, `Edit` the `# POSIX_REF: ...`
line to the new value. Run `./e2e/run_tests.sh --filter=2_10_shell_grammar`
— must still report 13/13 pass (migration changes metadata only).
Commit.

### Step 2 (Commit ②) — Command-position Rule tests (Rule 7, 10)

Create 6 files (3 × Rule 7, 3 × Rule 10). Run
`./e2e/run_tests.sh --filter=rule07` and `--filter=rule10`. XFAIL any
failure with a specific reason referencing yosh's actual behavior.
Commit.

### Step 3 (Commit ③) — Redirection-context Rule tests (Rule 2, 3)

Create 4 files (2 × Rule 2, 2 × Rule 3). Filter and verify. XFAIL if
needed. Commit.

### Step 4 (Commit ④) — Loop / case / function Rule tests (Rule 4, 5, 6, 8, 9)

Create 14 files. This is the largest commit and the most likely to
uncover implementation gaps — especially:

- `rule09_function_body_simple_cmd_rejected.sh` (yosh may accept
  `f() echo ok`)
- `rule06_for_without_in_uses_positional.sh` (yosh may not implement
  positional-arg fallback)
- `rule08_function_reserved_name_rejected.sh` (yosh may accept
  reserved words as function names)

For each, XFAIL with a concrete reason naming yosh's actual
behavior observed in the FAIL output. Commit.

### Step 5 (Commit ⑤) — §2.10.1 tests + TODO.md cleanup

Create `e2e/posix_spec/2_10_1_lexical/` with 3 files. Run full
`./e2e/run_tests.sh` — total should be 337 + 27 = 364 minus any XFAIL
(which still show `Total: 364  Failed: 0  XFail: N`).

Update TODO.md:
- Remove both §2.10.1 and §2.10.2 lines under "Future: POSIX
  Conformance Gaps (Chapter 2)" on clean pass.
- For any Rule where we XFAIL'd tests, leave a concrete TODO entry
  naming the file(s) and the observed divergence.

Run `cargo test --lib` to confirm no Rust regression.

Commit. Record the sub-project as complete.

## Success Criteria

1. `./e2e/run_tests.sh --filter=2_10_` → `Failed: 0` (pass + XFAIL
   = 40 files: 13 existing + 27 new).
2. `./e2e/run_tests.sh --filter=2_10_1_lexical` → `Failed: 0`.
3. Full `./e2e/run_tests.sh` — no regression in previously passing
   tests; total advances by 27.
4. `cargo test --lib` — 620+ passing, no regressions (same baseline
   observed in Sub-project 1; the `test_classify_incomplete_if/while`
   hang in `tests/interactive.rs` is pre-existing and tracked
   separately).
5. TODO.md "Future: POSIX Conformance Gaps (Chapter 2)":
   - §2.10.1 and §2.10.2 lines removed on clean pass.
   - Any XFAIL'd Rule N is rewritten with a specific technical
     reason (yosh's observed behavior + spec clause violated).
6. All 27 new files: mode `644`, `#!/bin/sh`, `POSIX_REF` + `DESCRIPTION`.
7. All 13 migrated files: new `POSIX_REF` values per migration table.

## File Conventions

Per CLAUDE.md and established E2E patterns:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule N - <Rule Name>
# DESCRIPTION: <one-line description>
# EXPECT_EXIT: <n>
# (optional) # EXPECT_OUTPUT: ...
# (optional) # EXPECT_STDERR: yosh:
# (optional) # XFAIL: <reason>
<test body>
```

`$TEST_TMPDIR` for any file creation. Mode 644. No bash-isms.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| `rule09_function_body_simple_cmd_rejected.sh` passes (i.e., yosh accepts the invalid grammar) | XFAIL + TODO.md "§2.10.2 Rule 9 — function body simple-command rejection not enforced" |
| `rule06_for_without_in_uses_positional.sh` — yosh doesn't implement the no-`in` form | XFAIL + TODO.md "§2.10.2 Rule 6 — `for NAME do ...` positional fallback not implemented" |
| `rule08_function_reserved_name_rejected.sh` — yosh accepts `if() {...}` | XFAIL + TODO.md concrete entry |
| Heredoc inside test fixture confuses the harness | Low risk per existing `heredoc_basic.sh`; if it manifests, switch to indirect expression via `command -v` skip patterns |
| `rule10_reserved_quoted_not_recognized.sh` environment-dependent (`/usr/bin/if`) | In-script skip via `command -v if` |
| `rule02_filename_reserved_word.sh` collides with real `$TEST_TMPDIR/if` state | `$TEST_TMPDIR` is fresh per test; no collision |

## Out of Scope (explicit)

- Every shall/must clause under §2.10.x (a separate "Deepen to
  normative-requirement granularity" TODO covers this).
- Legacy-dir test migration (`e2e/control_flow/`, etc.).
- Any implementation fix uncovered by XFAIL'd tests.
- Sub-projects 3, 4, 5 (§2.6.1 mixed WordPart, §2.6.1 escape, §2.11
  ignored-on-entry).
