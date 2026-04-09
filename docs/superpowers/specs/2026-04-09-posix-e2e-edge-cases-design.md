# POSIX E2E Edge Case Tests Design

**Date:** 2026-04-09
**Scope:** Add 86 edge case tests across all 13 E2E categories to strengthen POSIX compliance coverage
**Approach:** Category-wide — systematic edge case coverage for each existing category
**XFAIL Policy:** Known limitations (documented in TODO.md) are included with XFAIL markers

## Overview

The existing test suite has 142 tests covering core POSIX functionality. This design adds 86 edge case tests (5 with XFAIL) targeting boundary conditions, corner cases, and subtle POSIX semantics that the current suite does not exercise. Total after implementation: 228 tests.

## Test Format

All tests follow the existing convention:
- Metadata header: `POSIX_REF`, `DESCRIPTION`, `EXPECT_OUTPUT`, `EXPECT_EXIT`, `EXPECT_STDERR`, `XFAIL`
- Multi-line output via heredoc syntax: `EXPECT_OUTPUT<<END ... # END`
- Per-test `$TEST_TMPDIR` for file operations

## Tests by Category

### 1. variable_and_expansion (12 tests, 1 XFAIL)

| File | Description | POSIX Ref | XFAIL |
|------|-------------|-----------|-------|
| `unset_vs_empty_default.sh` | `${var-default}` vs `${var:-default}` — unset vs empty distinction | 2.6.2 | — |
| `unset_vs_empty_assign.sh` | `${var=default}` vs `${var:=default}` — colon presence behavior | 2.6.2 | — |
| `unset_vs_empty_alternate.sh` | `${var+alt}` vs `${var:+alt}` — empty string handling | 2.6.2 | — |
| `error_if_unset_message.sh` | `${var?msg}` — custom error message to stderr | 2.6.2 | — |
| `nested_expansion.sh` | `${var:-$(echo fallback)}` — command substitution in default | 2.6.2 | — |
| `strip_pattern_complex.sh` | `${path%%/*/}` — complex glob pattern in suffix stripping | 2.6.2 | — |
| `at_vs_star_quoted.sh` | `"$@"` vs `"$*"` — behavior difference in double quotes | 2.5.2 | — |
| `at_vs_star_unquoted.sh` | Unquoted `$@` vs `$*` — field splitting behavior | 2.5.2 | XFAIL: unquoted $@ field splitting joins with space instead of producing separate fields |
| `special_var_hyphen.sh` | `$-` — current shell option flags | 2.5.2 | — |
| `special_var_zero.sh` | `$0` — shell/script name reference | 2.5.2 | — |
| `multi_digit_positional.sh` | `${10}` `${11}` — multi-digit positional parameters | 2.5.1 | — |
| `readonly_error.sh` | Assignment to readonly variable produces error | 2.9.1 | — |

### 2. quoting (8 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `backslash_line_continuation.sh` | `\` + newline is line continuation | 2.2.1 |
| `backslash_special_in_dquotes.sh` | Inside `"`, only `\$`, `` \` ``, `\"`, `\\` are special | 2.2.3 |
| `adjacent_quoted_strings.sh` | `'a'"b"'c'` concatenates to `abc` | 2.2 |
| `dollar_at_end_of_dquotes.sh` | `"hello$"` — trailing `$` is literal | 2.2.3 |
| `empty_strings_as_args.sh` | `""` and `''` are preserved as arguments | 2.2 |
| `single_quote_in_dquotes.sh` | `"it's"` — single quote inside double quotes | 2.2.3 |
| `backslash_non_special_in_dquotes.sh` | `"\a"` keeps backslash for non-special chars | 2.2.3 |
| `quote_removal_order.sh` | Quote removal correctness after expansion | 2.6.7 |

### 3. field_splitting (8 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `empty_ifs.sh` | `IFS=''` disables field splitting | 2.6.5 |
| `ifs_whitespace_trimming.sh` | IFS whitespace trims leading/trailing | 2.6.5 |
| `ifs_non_whitespace_consecutive.sh` | `IFS=:` with `a::b` produces empty field | 2.6.5 |
| `ifs_mixed_whitespace_non.sh` | `IFS=": "` — mixed whitespace/non-whitespace IFS | 2.6.5 |
| `ifs_unset_default.sh` | `unset IFS` restores default space/tab/newline | 2.6.5 |
| `glob_char_class.sh` | `[a-z]`, `[0-9]` — character class glob | 2.13 |
| `glob_negated_class.sh` | `[!0-9]` — negated character class | 2.13 |
| `glob_dot_files.sh` | `*` does not match dot files | 2.13.3 |

### 4. redirection (8 tests, 1 XFAIL)

| File | Description | POSIX Ref | XFAIL |
|------|-------------|-----------|-------|
| `heredoc_pipeline.sh` | `cat <<EOF \| tr a-z A-Z` — heredoc + pipeline | 2.7.4 | XFAIL: Phase 4 limitation — heredoc + pipeline produces empty output |
| `heredoc_multiple.sh` | Multiple heredocs on one command | 2.7.4 | — |
| `heredoc_empty.sh` | Empty heredoc (delimiter only) | 2.7.4 | — |
| `fd_close.sh` | `N>&-` — explicit file descriptor close | 2.7.6 | — |
| `redirect_cmd_sub_filename.sh` | `echo x > $(echo file)` — command sub in filename | 2.7 | — |
| `heredoc_tab_strip_mixed.sh` | `<<-` strips only tabs, not spaces | 2.7.4 | — |
| `redirect_append_create.sh` | `>>` creates file if nonexistent | 2.7.2 | — |
| `noclobber_append_bypass.sh` | `set -C` does not restrict `>>` | 2.7.2 | — |

### 5. arithmetic (8 tests, 1 XFAIL)

| File | Description | POSIX Ref | XFAIL |
|------|-------------|-----------|-------|
| `division_by_zero.sh` | `$((1/0))` — division by zero error | 2.6.4 | — |
| `modulo_by_zero.sh` | `$((1%0))` — modulo by zero error | 2.6.4 | — |
| `unary_minus.sh` | `$((-5))`, `$((- 3))` — unary minus | 2.6.4 | — |
| `undefined_var_is_zero.sh` | Undefined variable treated as 0 in arithmetic | 2.6.4 | — |
| `nested_ternary.sh` | `$((a ? b ? 1 : 2 : 3))` — nested ternary | 2.6.4 | — |
| `comma_operator.sh` | `$((a=1, b=2, a+b))` — comma operator | 2.6.4 | — |
| `positional_in_arith.sh` | `$(($1 + $2))` — positional params in arithmetic | 2.6.4 | XFAIL: Phase 5 limitation — $N inside $((...)) not supported |
| `bitwise_operators.sh` | `&`, `\|`, `^`, `~`, `<<`, `>>` — bitwise ops | 2.6.4 | — |

### 6. command_substitution (6 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `trailing_newlines_multiple.sh` | `$(printf 'a\n\n\n')` strips all trailing newlines | 2.6.3 |
| `empty_command_sub.sh` | `$()` produces empty string | 2.6.3 |
| `backtick_syntax.sh` | `` `echo hello` `` backtick syntax | 2.6.3 |
| `nested_with_quotes.sh` | `$(echo "$(echo 'inner')")` — nested with quotes | 2.6.3 |
| `cmd_sub_with_redirect.sh` | `$(cat < file)` — redirect inside command sub | 2.6.3 |
| `cmd_sub_preserves_spaces.sh` | `"$(echo 'a  b')"` — spaces preserved in quotes | 2.6.3 |

### 7. control_flow (6 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `for_no_in_clause.sh` | `for x; do ... done` uses `"$@"` | 2.9.4.2 |
| `for_empty_list.sh` | `for x in; do ... done` — empty list, no execution | 2.9.4.2 |
| `break_with_count.sh` | `break 2` exits multiple loops | 2.14.4 |
| `continue_with_count.sh` | `continue 2` skips to outer loop | 2.14.5 |
| `case_empty_pattern.sh` | `case "$x" in '') ...` — empty string pattern | 2.9.4.5 |
| `while_false_body_skipped.sh` | `while false; do ... done` — body not executed | 2.9.4.3 |

### 8. function (6 tests, 1 XFAIL)

| File | Description | POSIX Ref | XFAIL |
|------|-------------|-----------|-------|
| `function_override_builtin.sh` | Function with same name as builtin | 2.9.5 | — |
| `function_nested_definition.sh` | Function defined inside another function | 2.9.5 | — |
| `function_redirect.sh` | `func() { ...; } > file` — redirect on function | 2.9.5 | — |
| `function_exit_vs_return.sh` | `exit` inside function terminates entire shell | 2.9.5 | — |
| `function_local_positional.sh` | Caller's `$@` restored after function call | 2.9.5 | — |
| `function_prefix_assignment.sh` | `VAR=val func` — prefix assignment scoped to function | 2.9.1 | XFAIL: Phase 5 limitation — function-scoped prefix assignments not implemented |

### 9. pipeline_and_list (6 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `negation_with_and_or.sh` | `! cmd && echo yes` — `!` precedence with `&&`/`\|\|` | 2.9.2 |
| `pipeline_subshell.sh` | Each pipeline command runs in subshell | 2.9.2 |
| `or_list_short_circuit.sh` | `true \|\| echo no` — right side not evaluated | 2.9.3 |
| `and_list_short_circuit.sh` | `false && echo no` — right side not evaluated | 2.9.3 |
| `mixed_and_or_list.sh` | `a && b \|\| c && d` — left-to-right evaluation | 2.9.3 |
| `pipeline_exit_last.sh` | Pipeline exit code is from last command | 2.9.2 |

### 10. builtin (6 tests, 1 XFAIL)

| File | Description | POSIX Ref | XFAIL |
|------|-------------|-----------|-------|
| `cd_dash_oldpwd.sh` | `cd -` returns to OLDPWD | 2.14.3 | XFAIL: Phase 2 limitation — cd - not implemented |
| `echo_no_args.sh` | `echo` with no args outputs newline only | 2.14.8 | — |
| `export_format.sh` | `export -p` output is re-inputtable | 2.14.22 | — |
| `set_dash_dash.sh` | `set -- a b c` sets positional parameters | 2.14.33 | — |
| `unset_readonly_error.sh` | Unsetting readonly variable is an error | 2.14.40 | — |
| `colon_always_success.sh` | `:` always exits 0 | 2.14.1 | — |

### 11. signal_and_trap (4 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `trap_reset_dash.sh` | `trap - SIGNAL` resets to default | 2.14.38 |
| `trap_ignore_empty.sh` | `trap '' SIGNAL` ignores signal | 2.14.38 |
| `trap_exit_in_function.sh` | EXIT trap in function fires at shell exit | 2.14.38 |
| `trap_subshell_reset.sh` | Non-ignored traps reset in subshell | 2.14.38 |

### 12. subshell (4 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `subshell_exit_no_parent.sh` | Exit in subshell does not affect parent | 2.12 |
| `subshell_cd_no_parent.sh` | cd in subshell does not affect parent cwd | 2.12 |
| `subshell_trap_inherit_ignore.sh` | Ignored signals inherited by subshell | 2.12 |
| `subshell_nested_exit_code.sh` | `(exit 42)` — subshell exit code in `$?` | 2.12 |

### 13. command_execution (4 tests, 0 XFAIL)

| File | Description | POSIX Ref |
|------|-------------|-----------|
| `command_not_found.sh` | Nonexistent command exits 127 | 2.8.2 |
| `permission_denied.sh` | Non-executable file exits 126 | 2.8.2 |
| `empty_var_command.sh` | Empty var as command — only redirects/assignments run | 2.9.1 |
| `prefix_assignment_scope.sh` | `VAR=val cmd` — prefix assignment scoped to command | 2.9.1 |

## Summary

- **Total new tests:** 86
- **XFAIL tests:** 5 (at_vs_star_unquoted, heredoc_pipeline, positional_in_arith, function_prefix_assignment, cd_dash_oldpwd)
- **Categories covered:** All 13
- **Expected total after implementation:** 228

## XFAIL Reference

| Test | Known Limitation | TODO Phase |
|------|------------------|------------|
| `at_vs_star_unquoted.sh` | Unquoted `$@` joins with space instead of separate fields | Phase 3 |
| `heredoc_pipeline.sh` | Heredoc + pipeline produces empty output | Phase 4 |
| `positional_in_arith.sh` | `$N` inside `$((...))` not supported | Phase 5 |
| `function_prefix_assignment.sh` | Function-scoped prefix assignments not implemented | Phase 5 |
| `cd_dash_oldpwd.sh` | `cd -` not implemented | Phase 2 |
