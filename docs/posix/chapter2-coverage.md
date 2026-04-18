# POSIX.1-2017 XCU Chapter 2 Coverage Matrix

Source: https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html

**Legend:**
- `covered` — at least one focused test exists
- `thin` — fewer than 3 tests AND missing major sub-behaviors
- `missing` — no dedicated test file
- `informational` — descriptive/structural section; minimal observation test is enough

## 2.1 Shell Introduction
- Status: covered
- Tests:
  - e2e/command_execution/script_file.sh

## 2.2 Quoting
- Status: covered
- Tests:
  - e2e/quoting/adjacent_quoted_strings.sh
  - e2e/quoting/empty_string.sh
  - e2e/quoting/empty_strings_as_args.sh
  - e2e/quoting/glob_suppressed.sh
  - e2e/quoting/nested_quotes.sh

### 2.2.1 Escape Character (Backslash)
- Status: covered
- Tests:
  - e2e/quoting/backslash_escape.sh
  - e2e/quoting/backslash_line_continuation.sh
  - e2e/quoting/literal_dollar.sh

### 2.2.2 Single-Quotes
- Status: thin
- Tests:
  - e2e/quoting/single_quotes.sh

### 2.2.3 Double-Quotes
- Status: covered
- Tests:
  - e2e/quoting/*.sh (8 files: backslash_in_double_quotes, backslash_non_special_in_dquotes, backslash_special_in_dquotes, dollar_at_end_of_dquotes, double_quotes_expansion, double_quotes, single_quote_in_dquotes, spaces_preserved)

## 2.3 Token Recognition
- Status: covered
- Tests:
  - e2e/posix_spec/2_03_token_recognition/operator_terminates_word.sh
  - e2e/posix_spec/2_03_token_recognition/line_continuation_in_word.sh
  - e2e/posix_spec/2_03_token_recognition/quoted_operator_not_token.sh

### 2.3.1 Alias Substitution
- Status: thin
- Tests:
  - e2e/builtin/alias_basic.sh

## 2.4 Reserved Words
- Status: covered
- Tests:
  - e2e/posix_spec/2_04_reserved_words/if_in_command_position.sh
  - e2e/posix_spec/2_04_reserved_words/if_as_argument.sh
  - e2e/posix_spec/2_04_reserved_words/quoted_if_not_reserved.sh
  - e2e/posix_spec/2_04_reserved_words/brace_group_in_command_position.sh

## 2.5 Parameters and Variables
- Status: thin
- Tests:
  - e2e/variable_and_expansion/simple_assignment.sh

### 2.5.1 Positional Parameters
- Status: covered
- Tests:
  - e2e/variable_and_expansion/braces_required.sh
  - e2e/variable_and_expansion/multi_digit_positional.sh
  - e2e/variable_and_expansion/positional_params.sh

### 2.5.2 Special Parameters
- Status: covered
- Tests:
  - e2e/subshell/dollar_dollar_same.sh
  - e2e/variable_and_expansion/at_vs_star_quoted.sh
  - e2e/variable_and_expansion/at_vs_star_unquoted.sh
  - e2e/variable_and_expansion/special_var_at.sh
  - e2e/variable_and_expansion/special_var_dollar.sh
  - e2e/variable_and_expansion/special_var_hash.sh
  - e2e/variable_and_expansion/special_var_hyphen.sh
  - e2e/variable_and_expansion/special_var_question.sh
  - e2e/variable_and_expansion/special_var_star.sh
  - e2e/variable_and_expansion/special_var_zero.sh

### 2.5.3 Shell Variables
- Status: covered
- Tests:
  - e2e/builtin/source_env_expansion.sh
  - e2e/builtin/source_env.sh
  - e2e/builtin/source_order.sh
  - e2e/builtin/source_yoshrc.sh
  - e2e/posix_spec/2_05_03_shell_variables/home_default_and_override.sh
  - e2e/posix_spec/2_05_03_shell_variables/ifs_custom_splitting.sh
  - e2e/posix_spec/2_05_03_shell_variables/ifs_default_whitespace.sh
  - e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh (XFAIL)
  - e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh (XFAIL)

## 2.6 Word Expansions
- Status: informational
- Tests: (none)

### 2.6.1 Tilde Expansion
- Status: covered
- Tests:
  - e2e/posix_spec/2_06_01_tilde_expansion/tilde_home.sh
  - e2e/posix_spec/2_06_01_tilde_expansion/tilde_slash_path.sh
  - e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh (XFAIL)
  - e2e/posix_spec/2_06_01_tilde_expansion/tilde_quoted_no_expansion.sh

### 2.6.2 Parameter Expansion
- Status: covered
- Tests:
  - e2e/variable_and_expansion/*.sh (18 files: alternate_value, assign_default, default_value_set, default_value, error_if_unset_message, error_if_unset, nested_expansion, string_length, strip_pattern_complex, strip_prefix_long, strip_prefix_short, strip_suffix_long, strip_suffix_short, unset_variable, unset_vs_empty_alternate, unset_vs_empty_assign, unset_vs_empty_default, variable_reference)

### 2.6.3 Command Substitution
- Status: covered
- Tests:
  - e2e/command_substitution/*.sh (19 files)

### 2.6.4 Arithmetic Expansion
- Status: covered
- Tests:
  - e2e/arithmetic/*.sh (19 files)

### 2.6.5 Field Splitting
- Status: covered
- Tests:
  - e2e/field_splitting/*.sh (9 files: custom_ifs, default_ifs, empty_ifs, ifs_colon, ifs_mixed_whitespace_non, ifs_non_whitespace_consecutive, ifs_unset_default, ifs_whitespace_trimming, no_split_in_quotes)

### 2.6.6 Pathname Expansion
- Status: covered
- Tests:
  - e2e/field_splitting/glob_basic.sh
  - e2e/field_splitting/glob_no_match.sh
  - e2e/field_splitting/glob_question_mark.sh
  - e2e/field_splitting/noglob.sh

### 2.6.7 Quote Removal
- Status: thin
- Tests:
  - e2e/quoting/quote_removal_order.sh

## 2.7 Redirection
- Status: covered
- Tests:
  - e2e/redirection/multiple_redirects.sh
  - e2e/redirection/redirect_cmd_sub_filename.sh

### 2.7.1 Redirecting Input
- Status: thin
- Tests:
  - e2e/redirection/input_redirect.sh

### 2.7.2 Redirecting Output
- Status: covered
- Tests:
  - e2e/redirection/*.sh (7 files: dev_null, noclobber_append_bypass, noclobber, output_append, output_redirect, redirect_append_create, stderr_redirect)

### 2.7.3 Appending Redirected Output
- Status: informational
- Tests: (none)
- Note: Append behavior is exercised by output_append.sh / redirect_append_create.sh under §2.7.2.

### 2.7.4 Here-Document
- Status: covered
- Tests:
  - e2e/redirection/heredoc_*.sh (10 files: heredoc_arith_quoted_paren, heredoc_basic, heredoc_cmd_sub_quoted_paren, heredoc_empty, heredoc_expansion, heredoc_multiple, heredoc_pipeline, heredoc_quoted_no_expansion, heredoc_strip_tabs, heredoc_tab_strip_mixed)

### 2.7.5 Duplicating an Input File Descriptor
- Status: missing
- Tests: (none)

### 2.7.6 Duplicating an Output File Descriptor
- Status: thin
- Tests:
  - e2e/redirection/fd_close.sh
  - e2e/redirection/stderr_to_stdout.sh

### 2.7.7 Open File Descriptors for Reading and Writing
- Status: missing
- Tests: (none)

## 2.8 Exit Status and Errors
- Status: informational
- Tests: (none)

### 2.8.1 Consequences of Shell Errors
- Status: covered
- Tests:
  - e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_syntax_error.sh
  - e2e/posix_spec/2_08_01_consequences_of_shell_errors/redirection_error_regular_command.sh
  - e2e/posix_spec/2_08_01_consequences_of_shell_errors/command_not_found_continues.sh

### 2.8.2 Exit Status for Commands
- Status: covered
- Tests:
  - e2e/command_execution/command_not_found.sh
  - e2e/command_execution/exit_code_custom.sh
  - e2e/command_execution/exit_code_success.sh
  - e2e/command_execution/permission_denied.sh

## 2.9 Shell Commands
- Status: informational
- Tests: (none)

### 2.9.1 Simple Commands
- Status: covered
- Tests:
  - e2e/builtin_command/*.sh (8 files: command_not_executable, command_p_when_path_unset, command_skips_function, command_v_alias, command_v_builtin, command_V_external, command_v_finds_external, command_V_not_found)
  - e2e/command_execution/*.sh (8 files: assign_cmd_sub_same_status, assign_preserves_prior_status, assignment_only, echo_simple, empty_command, empty_var_command, path_search, prefix_assignment_external)
  - e2e/function/function_prefix_assignment.sh
  - e2e/function/function_prefix_multi_assign.sh
  - e2e/variable_and_expansion/readonly_error.sh

### 2.9.2 Pipelines
- Status: covered
- Tests:
  - e2e/pipeline_and_list/*.sh (8 files: multi_stage_pipe, negation_with_and_or, negation, pipe_exit_status, pipeline_exit_last, pipeline_subshell, pipeline_with_builtin, simple_pipe)

### 2.9.3 Lists
- Status: covered
- Tests:
  - e2e/command_execution/multiple_commands.sh
  - e2e/pipeline_and_list/*.sh (8 files: and_list_short_circuit, and_list, and_or_combined, background_command, mixed_and_or_list, or_list_short_circuit, or_list, semicolon_list)

### 2.9.4 Compound Commands
- Status: covered
- Tests:
  - 2.9.4.1 if: e2e/control_flow/if_*.sh (5 files: if_elif, if_else, if_false, if_nested, if_true)
  - 2.9.4.2 for: e2e/control_flow/for_*.sh (4 files: for_default_positional, for_empty_list, for_empty, for_list)
  - 2.9.4.3 while: e2e/control_flow/while_*.sh (3 files: while_basic, while_false_body_skipped, while_false_no_exec)
  - 2.9.4.4 until: e2e/control_flow/until_basic.sh
  - 2.9.4.5 case: e2e/control_flow/case_basic.sh, e2e/control_flow/case_empty_pattern.sh, e2e/control_flow/case_glob.sh

### 2.9.5 Function Definition Command
- Status: covered
- Tests:
  - e2e/function/*.sh (11 files: basic_definition, dollar_at_in_function, function_exit_vs_return, function_local_positional, function_nested_definition, function_override_builtin, function_redirect, global_variable, positional_params_restore, recursion, with_arguments)

## 2.10 Shell Grammar
- Status: covered
- Tests:
  - e2e/posix_spec/2_10_shell_grammar/terminator_semicolon_equals_newline.sh
  - e2e/posix_spec/2_10_shell_grammar/compound_list_newline_between_commands.sh
  - e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh (XFAIL)

### 2.10.1 Shell Grammar Lexical Conventions
- Status: missing
- Tests: (none)

### 2.10.2 Shell Grammar Rules
- Status: missing
- Tests: (none)

## 2.11 Signals and Error Handling
- Status: covered
- Tests:
  - e2e/posix_spec/2_11_signals_and_error_handling/trap_exit_runs_on_exit.sh
  - e2e/posix_spec/2_11_signals_and_error_handling/trap_dash_resets_default.sh
  - e2e/posix_spec/2_11_signals_and_error_handling/trap_int_by_name.sh
  - e2e/signal_and_trap/trap_ignore_signal.sh
  - e2e/signal_and_trap/trap_in_subshell_reset.sh
- Note: Additional trap-related tests exist under §2.14.14 trap. Tests labeled "2.11 Job Control" (jobs/bg/fg/set_monitor) are a separate POSIX topic (XCU §2.13 Job Control) and are not counted here; see Open Questions.

## 2.12 Shell Execution Environment
- Status: covered
- Tests:
  - e2e/subshell/*.sh (11 files: alias_isolation, basic_execution, exit_status, function_isolation, nested_subshell, option_isolation, subshell_cd_no_parent, subshell_exit_no_parent, subshell_nested_exit_code, subshell_trap_inherit_ignore, variable_isolation; excludes dollar_dollar_same which is §2.5.2)

## 2.13 Pattern Matching Notation
- Status: covered
- Tests:
  - e2e/field_splitting/glob_char_class.sh
  - e2e/field_splitting/glob_negated_class.sh
  - e2e/posix_spec/2_13_pattern_matching/star_matches_any_string.sh
  - e2e/posix_spec/2_13_pattern_matching/question_matches_single_char.sh
  - e2e/posix_spec/2_13_pattern_matching/bracket_char_class.sh
  - e2e/posix_spec/2_13_pattern_matching/bracket_negated_class.sh
  - e2e/posix_spec/2_13_pattern_matching/quoted_glob_literal.sh

### 2.13.1 Patterns Matching a Single Character
- Status: covered
- Tests:
  - e2e/posix_spec/2_13_pattern_matching/question_matches_single_char.sh
  - e2e/posix_spec/2_13_pattern_matching/bracket_char_class.sh
  - e2e/posix_spec/2_13_pattern_matching/bracket_negated_class.sh

### 2.13.2 Patterns Matching Multiple Characters
- Status: covered
- Tests:
  - e2e/posix_spec/2_13_pattern_matching/star_matches_any_string.sh
  - e2e/posix_spec/2_13_pattern_matching/quoted_glob_literal.sh

### 2.13.3 Patterns Used for Filename Expansion
- Status: thin
- Tests:
  - e2e/field_splitting/glob_dot_files.sh

## 2.14 Special Built-In Utilities
- Status: covered
- Tests:
  - e2e/builtin/*.sh (16 files: colon_noop, echo_no_args, eval_basic, eval_variable, exec_no_args, exec_replace, export_basic, export_format, readonly_basic, set_dash_dash, set_monitor_flag, set_positional, shift_basic, source_file, unset_readonly_error, unset_variable)
  - e2e/control_flow/break_continue.sh, e2e/control_flow/break_nested.sh
  - e2e/function/return_default.sh, e2e/function/return_value.sh
  - e2e/signal_and_trap/trap_*.sh (7 files: trap_display, trap_exit_in_function, trap_exit_on_error, trap_exit, trap_ignore_empty, trap_multiple_commands, trap_reset)

### 2.14.1 break
- Status: covered
- Tests:
  - e2e/control_flow/break_continue.sh
  - e2e/control_flow/break_nested.sh
  - e2e/control_flow/break_with_count.sh

### 2.14.2 colon
- Status: thin
- Tests:
  - e2e/builtin/colon_noop.sh

### 2.14.3 continue
- Status: thin
- Tests:
  - e2e/control_flow/break_continue.sh
  - e2e/control_flow/continue_with_count.sh

### 2.14.4 dot
- Status: thin
- Tests:
  - e2e/builtin/source_file.sh
- Note: `dot` (.) is the POSIX name; tests live under the `source` alias.

### 2.14.5 eval
- Status: thin
- Tests:
  - e2e/builtin/eval_basic.sh
  - e2e/builtin/eval_variable.sh

### 2.14.6 exec
- Status: thin
- Tests:
  - e2e/builtin/exec_no_args.sh
  - e2e/builtin/exec_replace.sh

### 2.14.7 exit
- Status: informational
- Tests: (none)
- Note: Exit status is exercised broadly under §2.8.2 and §2.12.

### 2.14.8 export
- Status: thin
- Tests:
  - e2e/builtin/export_basic.sh
  - e2e/builtin/export_format.sh

### 2.14.9 readonly
- Status: thin
- Tests:
  - e2e/builtin/readonly_basic.sh
  - e2e/variable_and_expansion/readonly_error.sh
  - e2e/builtin/unset_readonly_error.sh

### 2.14.10 return
- Status: thin
- Tests:
  - e2e/function/return_default.sh
  - e2e/function/return_value.sh

### 2.14.11 set
- Status: covered
- Tests:
  - e2e/builtin/set_dash_dash.sh
  - e2e/builtin/set_monitor_flag.sh
  - e2e/builtin/set_positional.sh
- Note: set_monitor_off.sh is tagged `POSIX_REF: 2.11 Job Control` and excluded here per Open Questions #1 policy.

### 2.14.12 shift
- Status: thin
- Tests:
  - e2e/builtin/shift_basic.sh

### 2.14.13 times
- Status: missing
- Tests: (none)

### 2.14.14 trap
- Status: covered
- Tests:
  - e2e/signal_and_trap/trap_display.sh
  - e2e/signal_and_trap/trap_exit_in_function.sh
  - e2e/signal_and_trap/trap_exit_on_error.sh
  - e2e/signal_and_trap/trap_exit.sh
  - e2e/signal_and_trap/trap_ignore_empty.sh
  - e2e/signal_and_trap/trap_ignore_signal.sh
  - e2e/signal_and_trap/trap_in_subshell_reset.sh
  - e2e/signal_and_trap/trap_multiple_commands.sh
  - e2e/signal_and_trap/trap_reset.sh

### 2.14.15 unset
- Status: thin
- Tests:
  - e2e/builtin/unset_variable.sh
  - e2e/builtin/unset_readonly_error.sh

## Summary

| Status | Count |
|---|---|
| covered | 35 |
| thin | 17 |
| missing | 5 |
| informational | 5 |

### Per-section status

| Section | Status |
|---|---|
| 2.1 | covered |
| 2.2 | covered |
| 2.2.1 | covered |
| 2.2.2 | thin |
| 2.2.3 | covered |
| 2.3 | covered |
| 2.3.1 | thin |
| 2.4 | covered |
| 2.5 | thin |
| 2.5.1 | covered |
| 2.5.2 | covered |
| 2.5.3 | covered |
| 2.6 | informational |
| 2.6.1 | covered |
| 2.6.2 | covered |
| 2.6.3 | covered |
| 2.6.4 | covered |
| 2.6.5 | covered |
| 2.6.6 | covered |
| 2.6.7 | thin |
| 2.7 | covered |
| 2.7.1 | thin |
| 2.7.2 | covered |
| 2.7.3 | informational |
| 2.7.4 | covered |
| 2.7.5 | missing |
| 2.7.6 | thin |
| 2.7.7 | missing |
| 2.8 | informational |
| 2.8.1 | covered |
| 2.8.2 | covered |
| 2.9 | informational |
| 2.9.1 | covered |
| 2.9.2 | covered |
| 2.9.3 | covered |
| 2.9.4 | covered |
| 2.9.5 | covered |
| 2.10 | covered |
| 2.10.1 | missing |
| 2.10.2 | missing |
| 2.11 | covered |
| 2.12 | covered |
| 2.13 | covered |
| 2.13.1 | covered |
| 2.13.2 | covered |
| 2.13.3 | thin |
| 2.14 | covered |
| 2.14.1 | covered |
| 2.14.2 | thin |
| 2.14.3 | thin |
| 2.14.4 | thin |
| 2.14.5 | thin |
| 2.14.6 | thin |
| 2.14.7 | informational |
| 2.14.8 | thin |
| 2.14.9 | thin |
| 2.14.10 | thin |
| 2.14.11 | covered |
| 2.14.12 | thin |
| 2.14.13 | missing |
| 2.14.14 | covered |
| 2.14.15 | thin |

## Open Questions

- Tests under `e2e/builtin/` labeled `POSIX_REF: 2.11 Job Control` (bg_no_monitor, fg_no_monitor, jobs_background, jobs_basic, set_monitor_off) and `e2e/builtin/job_spec_*.sh` labeled `POSIX_REF: 3.204 Job Control Job ID` exercise job control, which is POSIX XCU §2.13 Job Control (note: §2.13 in this matrix is Pattern Matching per the chap02 layout — Job Control is a sibling chapter topic). They are not mapped into any §2.* row above. Decide whether to (a) add a §2.x Job Control row covering these, (b) relocate them to an XCU §2.13 Job Control appendix, or (c) leave them as chapter-3 (dictionary) references.
- Tests labeled `POSIX_REF: 4 Utilities - *` (cd, echo, kill, true/false) and `POSIX_REF: 8. Environment Variables (PATH)` are outside XCU Chapter 2 and were intentionally not included here. Confirm whether a cross-chapter coverage index is desired.
- `e2e/README.md` is tagged `POSIX_REF: 2.6.2 Parameter Expansion` but is documentation, not an executable test. It was excluded from the §2.6.2 test list. Confirm the label should be dropped from the README in a later cleanup.
