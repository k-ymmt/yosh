# POSIX E2E Edge Case Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 86 edge case E2E tests across all 13 categories to strengthen POSIX compliance coverage

**Architecture:** Each test is a standalone `.sh` file with metadata headers (`POSIX_REF`, `DESCRIPTION`, `EXPECT_OUTPUT`/`EXPECT_EXIT`/`EXPECT_STDERR`, optionally `XFAIL`). Tests are organized into existing category directories under `e2e/`. The test runner `e2e/run_tests.sh` auto-discovers `.sh` files recursively.

**Tech Stack:** POSIX sh test files, existing test runner (`e2e/run_tests.sh`)

---

### Task 1: variable_and_expansion edge cases (12 tests)

**Files:**
- Create: `e2e/variable_and_expansion/unset_vs_empty_default.sh`
- Create: `e2e/variable_and_expansion/unset_vs_empty_assign.sh`
- Create: `e2e/variable_and_expansion/unset_vs_empty_alternate.sh`
- Create: `e2e/variable_and_expansion/error_if_unset_message.sh`
- Create: `e2e/variable_and_expansion/nested_expansion.sh`
- Create: `e2e/variable_and_expansion/strip_pattern_complex.sh`
- Create: `e2e/variable_and_expansion/at_vs_star_quoted.sh`
- Create: `e2e/variable_and_expansion/at_vs_star_unquoted.sh`
- Create: `e2e/variable_and_expansion/special_var_hyphen.sh`
- Create: `e2e/variable_and_expansion/special_var_zero.sh`
- Create: `e2e/variable_and_expansion/multi_digit_positional.sh`
- Create: `e2e/variable_and_expansion/readonly_error.sh`

- [ ] **Step 1: Create unset_vs_empty_default.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var-default} vs ${var:-default} — unset vs empty distinction
# EXPECT_OUTPUT<<END
# default
# 
# default
# default
# END
unset x
echo "${x-default}"
x=
echo "${x-default}"
unset y
echo "${y:-default}"
y=
echo "${y:-default}"
```

- [ ] **Step 2: Create unset_vs_empty_assign.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var=default} vs ${var:=default} — colon presence with assign
# EXPECT_OUTPUT<<END
# default
# 
# default
# default
# END
unset x
echo "${x=default}"
unset y
y=
echo "${y=default}"
unset a
echo "${a:=default}"
b=
echo "${b:=default}"
```

- [ ] **Step 3: Create unset_vs_empty_alternate.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var+alt} vs ${var:+alt} — empty string handling
# EXPECT_OUTPUT<<END
# 
# alt
# 
# 
# alt
# END
unset x
echo "${x+alt}"
x=set
echo "${x+alt}"
y=
echo "${y:+alt}"
unset z
echo "${z:+alt}"
z=set
echo "${z:+alt}"
```

- [ ] **Step 4: Create error_if_unset_message.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var?msg} — custom error message to stderr
# EXPECT_EXIT: 1
# EXPECT_STDERR: my custom error
unset x
: "${x?my custom error}"
```

- [ ] **Step 5: Create nested_expansion.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-$(cmd)} — command substitution in default value
# EXPECT_OUTPUT: fallback
unset x
echo "${x:-$(echo fallback)}"
```

- [ ] **Step 6: Create strip_pattern_complex.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Complex glob pattern in prefix/suffix stripping
# EXPECT_OUTPUT<<END
# /home/user
# document.tar
# END
path="/home/user/documents"
echo "${path%/*}"
file="document.tar.gz"
echo "${file%.*}"
```

- [ ] **Step 7: Create at_vs_star_quoted.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: "$@" vs "$*" — behavior difference in double quotes
# EXPECT_OUTPUT<<END
# 3
# 1
# END
set -- "a b" c d
count=0
for i in "$@"; do count=$((count + 1)); done
echo "$count"
count=0
for i in "$*"; do count=$((count + 1)); done
echo "$count"
```

- [ ] **Step 8: Create at_vs_star_unquoted.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: Unquoted $@ should produce separate fields per positional parameter
# XFAIL: unquoted $@ joins with space instead of producing separate fields
# EXPECT_OUTPUT<<END
# 3
# a b
# c
# d
# END
set -- "a b" c d
count=0
for i in $@; do count=$((count + 1)); done
echo "$count"
for i in "$@"; do echo "$i"; done
```

- [ ] **Step 9: Create special_var_hyphen.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $- holds current shell option flags
# EXPECT_EXIT: 0
flags="$-"
test -n "$flags"
```

- [ ] **Step 10: Create special_var_zero.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $0 holds shell or script name
# EXPECT_EXIT: 0
test -n "$0"
```

- [ ] **Step 11: Create multi_digit_positional.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: ${10} ${11} — multi-digit positional parameters require braces
# EXPECT_OUTPUT<<END
# ten
# eleven
# END
set -- a b c d e f g h i ten eleven
echo "${10}"
echo "${11}"
```

- [ ] **Step 12: Create readonly_error.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment to readonly variable produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly x=hello
x=world
```

- [ ] **Step 13: Run tests for variable_and_expansion**

Run: `e2e/run_tests.sh --filter=variable_and_expansion`
Expected: All PASS except `at_vs_star_unquoted.sh` which should be XFAIL

- [ ] **Step 14: Commit**

```bash
git add e2e/variable_and_expansion/
git commit -m "test(e2e): add variable_and_expansion edge case tests (12 tests, 1 XFAIL)"
```

---

### Task 2: quoting edge cases (8 tests)

**Files:**
- Create: `e2e/quoting/backslash_line_continuation.sh`
- Create: `e2e/quoting/backslash_special_in_dquotes.sh`
- Create: `e2e/quoting/adjacent_quoted_strings.sh`
- Create: `e2e/quoting/dollar_at_end_of_dquotes.sh`
- Create: `e2e/quoting/empty_strings_as_args.sh`
- Create: `e2e/quoting/single_quote_in_dquotes.sh`
- Create: `e2e/quoting/backslash_non_special_in_dquotes.sh`
- Create: `e2e/quoting/quote_removal_order.sh`

- [ ] **Step 1: Create backslash_line_continuation.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character
# DESCRIPTION: Backslash-newline is line continuation
# EXPECT_OUTPUT: helloworld
echo hello\
world
```

- [ ] **Step 2: Create backslash_special_in_dquotes.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Inside double quotes only \$ \` \" \\ \newline are special
# EXPECT_OUTPUT<<END
# $HOME
# "quoted"
# back\slash
# END
echo "\$HOME"
echo "\"quoted\""
echo "back\\slash"
```

- [ ] **Step 3: Create adjacent_quoted_strings.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Adjacent quoted strings concatenate into one word
# EXPECT_OUTPUT: abc
echo 'a'"b"'c'
```

- [ ] **Step 4: Create dollar_at_end_of_dquotes.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Trailing $ in double quotes is literal
# EXPECT_OUTPUT: hello$
echo "hello$"
```

- [ ] **Step 5: Create empty_strings_as_args.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: "" and '' preserve empty arguments in argument lists
# EXPECT_OUTPUT<<END
# 4
# 3
# END
set -- a "" '' b
echo "$#"
set -- "" x ""
echo "$#"
```

- [ ] **Step 6: Create single_quote_in_dquotes.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Single quote inside double quotes is literal
# EXPECT_OUTPUT: it's fine
echo "it's fine"
```

- [ ] **Step 7: Create backslash_non_special_in_dquotes.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash before non-special char in double quotes is preserved
# EXPECT_OUTPUT<<END
# \a
# \n
# END
echo "\a"
echo "\n"
```

- [ ] **Step 8: Create quote_removal_order.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.7 Quote Removal
# DESCRIPTION: Quotes are removed after all expansions
# EXPECT_OUTPUT<<END
# hello world
# $notavar
# END
x=hello
echo "$x"' world'
echo '$notavar'
```

- [ ] **Step 9: Run tests for quoting**

Run: `e2e/run_tests.sh --filter=quoting`
Expected: All PASS

- [ ] **Step 10: Commit**

```bash
git add e2e/quoting/
git commit -m "test(e2e): add quoting edge case tests (8 tests)"
```

---

### Task 3: field_splitting edge cases (8 tests)

**Files:**
- Create: `e2e/field_splitting/empty_ifs.sh`
- Create: `e2e/field_splitting/ifs_whitespace_trimming.sh`
- Create: `e2e/field_splitting/ifs_non_whitespace_consecutive.sh`
- Create: `e2e/field_splitting/ifs_mixed_whitespace_non.sh`
- Create: `e2e/field_splitting/ifs_unset_default.sh`
- Create: `e2e/field_splitting/glob_char_class.sh`
- Create: `e2e/field_splitting/glob_negated_class.sh`
- Create: `e2e/field_splitting/glob_dot_files.sh`

- [ ] **Step 1: Create empty_ifs.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Empty IFS disables field splitting
# EXPECT_OUTPUT: a:b:c
IFS=
x="a:b:c"
for i in $x; do
  echo "$i"
done
```

- [ ] **Step 2: Create ifs_whitespace_trimming.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: IFS whitespace trims leading and trailing
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
x="  a  b  c  "
for i in $x; do
  echo "$i"
done
```

- [ ] **Step 3: Create ifs_non_whitespace_consecutive.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Consecutive non-whitespace IFS delimiters produce empty fields
# EXPECT_OUTPUT<<END
# a
# 
# b
# END
IFS=:
x="a::b"
set -- $x
echo "$1"
echo "$2"
echo "$3"
```

- [ ] **Step 4: Create ifs_mixed_whitespace_non.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Mixed whitespace and non-whitespace IFS characters
# EXPECT_OUTPUT<<END
# one
# two
# three
# END
IFS=": "
x="one: two:three"
for i in $x; do
  echo "$i"
done
```

- [ ] **Step 5: Create ifs_unset_default.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: unset IFS restores default splitting on space/tab/newline
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
IFS=:
unset IFS
x="a b c"
for i in $x; do
  echo "$i"
done
```

- [ ] **Step 6: Create glob_char_class.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Character class glob patterns [a-z] [0-9]
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo x > a1.txt
echo x > b2.txt
echo x > c3.log
count=0
for f in [a-c]*.txt; do
  count=$((count + 1))
done
test "$count" = 2
```

- [ ] **Step 7: Create glob_negated_class.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Negated character class [!0-9] matches non-digits
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo x > abc.txt
echo x > 123.txt
count=0
for f in [!0-9]*.txt; do
  count=$((count + 1))
done
test "$count" = 1
```

- [ ] **Step 8: Create glob_dot_files.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.13.3 Patterns Used for Filename Expansion
# DESCRIPTION: Glob * does not match dot files
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo x > visible.txt
echo x > .hidden.txt
count=0
for f in *; do
  count=$((count + 1))
done
test "$count" = 1
```

- [ ] **Step 9: Run tests for field_splitting**

Run: `e2e/run_tests.sh --filter=field_splitting`
Expected: All PASS

- [ ] **Step 10: Commit**

```bash
git add e2e/field_splitting/
git commit -m "test(e2e): add field_splitting edge case tests (8 tests)"
```

---

### Task 4: redirection edge cases (8 tests)

**Files:**
- Create: `e2e/redirection/heredoc_pipeline.sh`
- Create: `e2e/redirection/heredoc_multiple.sh`
- Create: `e2e/redirection/heredoc_empty.sh`
- Create: `e2e/redirection/fd_close.sh`
- Create: `e2e/redirection/redirect_cmd_sub_filename.sh`
- Create: `e2e/redirection/heredoc_tab_strip_mixed.sh`
- Create: `e2e/redirection/redirect_append_create.sh`
- Create: `e2e/redirection/noclobber_append_bypass.sh`

- [ ] **Step 1: Create heredoc_pipeline.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Heredoc piped to another command
# XFAIL: Phase 4 limitation — heredoc + pipeline produces empty output
# EXPECT_OUTPUT: HELLO
cat <<EOF | tr a-z A-Z
hello
EOF
```

- [ ] **Step 2: Create heredoc_multiple.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Multiple heredocs in sequence
# EXPECT_OUTPUT<<END
# first
# second
# END
cat <<EOF1
first
EOF1
cat <<EOF2
second
EOF2
```

- [ ] **Step 3: Create heredoc_empty.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Empty heredoc produces empty output
# EXPECT_OUTPUT:
cat <<EOF
EOF
```

- [ ] **Step 4: Create fd_close.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: File descriptor close with N>&-
# EXPECT_EXIT: 0
# EXPECT_STDERR: Bad file descriptor
echo "to stderr" >&2 2>&-
```

- [ ] **Step 5: Create redirect_cmd_sub_filename.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Command substitution in redirect filename
# EXPECT_EXIT: 0
fname="outfile.txt"
echo hello > "$TEST_TMPDIR/$(echo "$fname")"
result=$(cat "$TEST_TMPDIR/outfile.txt")
test "$result" = "hello"
```

- [ ] **Step 6: Create heredoc_tab_strip_mixed.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips only leading tabs, not spaces
# EXPECT_OUTPUT<<END
#   space-indented
# tab-then-content
# END
cat <<-EOF
	  space-indented
	tab-then-content
	EOF
```

- [ ] **Step 7: Create redirect_append_create.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: >> creates the file if it does not exist
# EXPECT_EXIT: 0
echo hello >> "$TEST_TMPDIR/newfile.txt"
result=$(cat "$TEST_TMPDIR/newfile.txt")
test "$result" = "hello"
```

- [ ] **Step 8: Create noclobber_append_bypass.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: set -C does not restrict >> (append)
# EXPECT_OUTPUT<<END
# first
# second
# END
echo first > "$TEST_TMPDIR/file.txt"
set -C
echo second >> "$TEST_TMPDIR/file.txt"
cat "$TEST_TMPDIR/file.txt"
```

- [ ] **Step 9: Run tests for redirection**

Run: `e2e/run_tests.sh --filter=redirection`
Expected: All PASS except `heredoc_pipeline.sh` which should be XFAIL

- [ ] **Step 10: Commit**

```bash
git add e2e/redirection/
git commit -m "test(e2e): add redirection edge case tests (8 tests, 1 XFAIL)"
```

---

### Task 5: arithmetic edge cases (8 tests)

**Files:**
- Create: `e2e/arithmetic/division_by_zero.sh`
- Create: `e2e/arithmetic/modulo_by_zero.sh`
- Create: `e2e/arithmetic/unary_minus.sh`
- Create: `e2e/arithmetic/undefined_var_is_zero.sh`
- Create: `e2e/arithmetic/nested_ternary.sh`
- Create: `e2e/arithmetic/comma_operator.sh`
- Create: `e2e/arithmetic/positional_in_arith.sh`
- Create: `e2e/arithmetic/bitwise_operators.sh`

- [ ] **Step 1: Create division_by_zero.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Division by zero produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: division by zero
echo $((1 / 0))
```

- [ ] **Step 2: Create modulo_by_zero.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Modulo by zero produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: division by zero
echo $((1 % 0))
```

- [ ] **Step 3: Create unary_minus.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Unary minus and plus operators
# EXPECT_OUTPUT<<END
# -5
# 3
# -7
# END
echo $((-5))
echo $((+3))
echo $((-(3 + 4)))
```

- [ ] **Step 4: Create undefined_var_is_zero.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Undefined variable is treated as 0 in arithmetic
# EXPECT_OUTPUT<<END
# 0
# 5
# END
unset x
echo $((x))
echo $((x + 5))
```

- [ ] **Step 5: Create nested_ternary.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Nested ternary conditional operator
# EXPECT_OUTPUT<<END
# 1
# 2
# 3
# END
a=1
b=1
echo $((a ? b ? 1 : 2 : 3))
b=0
echo $((a ? b ? 1 : 2 : 3))
a=0
echo $((a ? b ? 1 : 2 : 3))
```

- [ ] **Step 6: Create comma_operator.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Comma operator evaluates left to right, returns last
# EXPECT_OUTPUT<<END
# 3
# 1
# 2
# END
echo $((a=1, b=2, a+b))
echo "$a"
echo "$b"
```

- [ ] **Step 7: Create positional_in_arith.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Positional parameters in arithmetic expansion
# XFAIL: Phase 5 limitation — $N inside $((...)) not supported
# EXPECT_OUTPUT: 30
set -- 10 20
echo $(($1 + $2))
```

- [ ] **Step 8: Create bitwise_operators.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Bitwise operators &, |, ^, ~, <<, >>
# EXPECT_OUTPUT<<END
# 4
# 7
# 3
# -6
# 20
# 2
# END
echo $((5 & 6))
echo $((5 | 6))
echo $((5 ^ 6))
echo $((~5))
echo $((5 << 2))
echo $((10 >> 2))
```

- [ ] **Step 9: Run tests for arithmetic**

Run: `e2e/run_tests.sh --filter=arithmetic`
Expected: All PASS except `positional_in_arith.sh` which should be XFAIL

- [ ] **Step 10: Commit**

```bash
git add e2e/arithmetic/
git commit -m "test(e2e): add arithmetic edge case tests (8 tests, 1 XFAIL)"
```

---

### Task 6: command_substitution edge cases (6 tests)

**Files:**
- Create: `e2e/command_substitution/trailing_newlines_multiple.sh`
- Create: `e2e/command_substitution/empty_command_sub.sh`
- Create: `e2e/command_substitution/backtick_syntax.sh`
- Create: `e2e/command_substitution/nested_with_quotes.sh`
- Create: `e2e/command_substitution/cmd_sub_with_redirect.sh`
- Create: `e2e/command_substitution/cmd_sub_preserves_spaces.sh`

- [ ] **Step 1: Create trailing_newlines_multiple.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution strips all trailing newlines
# EXPECT_OUTPUT: a-end
x=$(printf 'a\n\n\n')
echo "${x}-end"
```

- [ ] **Step 2: Create empty_command_sub.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Empty command substitution produces empty string
# EXPECT_OUTPUT: -end
x=$()
echo "${x}-end"
```

- [ ] **Step 3: Create backtick_syntax.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Backtick syntax for command substitution
# EXPECT_OUTPUT: hello
echo `echo hello`
```

- [ ] **Step 4: Create nested_with_quotes.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution with quotes
# EXPECT_OUTPUT: inner value
echo "$(echo "$(echo 'inner value')")"
```

- [ ] **Step 5: Create cmd_sub_with_redirect.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Redirection inside command substitution
# EXPECT_OUTPUT: file content
echo "file content" > "$TEST_TMPDIR/input.txt"
x=$(cat < "$TEST_TMPDIR/input.txt")
echo "$x"
```

- [ ] **Step 6: Create cmd_sub_preserves_spaces.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution in double quotes preserves spaces
# EXPECT_OUTPUT: a  b  c
echo "$(echo 'a  b  c')"
```

- [ ] **Step 7: Run tests for command_substitution**

Run: `e2e/run_tests.sh --filter=command_substitution`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add e2e/command_substitution/
git commit -m "test(e2e): add command_substitution edge case tests (6 tests)"
```

---

### Task 7: control_flow edge cases (6 tests)

**Files:**
- Create: `e2e/control_flow/for_no_in_clause.sh`
- Create: `e2e/control_flow/for_empty_list.sh`
- Create: `e2e/control_flow/break_with_count.sh`
- Create: `e2e/control_flow/continue_with_count.sh`
- Create: `e2e/control_flow/case_empty_pattern.sh`
- Create: `e2e/control_flow/while_false_body_skipped.sh`

- [ ] **Step 1: Create for_no_in_clause.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for loop without in clause iterates over "$@"
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
set -- a b c
for i; do
  echo "$i"
done
```

- [ ] **Step 2: Create for_empty_list.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for loop with empty list does not execute body
# EXPECT_OUTPUT: done
for i in; do
  echo "should not print"
done
echo done
```

- [ ] **Step 3: Create break_with_count.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14.4 break
# DESCRIPTION: break N exits N enclosing loops
# EXPECT_OUTPUT<<END
# 1-a
# END
for i in 1 2 3; do
  for j in a b c; do
    echo "${i}-${j}"
    break 2
  done
done
```

- [ ] **Step 4: Create continue_with_count.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue N skips to Nth enclosing loop
# EXPECT_OUTPUT<<END
# 1-a
# 2-a
# 3-a
# END
for i in 1 2 3; do
  for j in a b c; do
    echo "${i}-${j}"
    continue 2
  done
done
```

- [ ] **Step 5: Create case_empty_pattern.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: Case matches empty string pattern
# EXPECT_OUTPUT<<END
# empty
# not-empty
# END
x=
case "$x" in
  '') echo empty ;;
  *) echo fail ;;
esac
x=hello
case "$x" in
  '') echo fail ;;
  *) echo not-empty ;;
esac
```

- [ ] **Step 6: Create while_false_body_skipped.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: while false never executes body
# EXPECT_OUTPUT: done
while false; do
  echo "should not print"
done
echo done
```

- [ ] **Step 7: Run tests for control_flow**

Run: `e2e/run_tests.sh --filter=control_flow`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add e2e/control_flow/
git commit -m "test(e2e): add control_flow edge case tests (6 tests)"
```

---

### Task 8: function edge cases (6 tests)

**Files:**
- Create: `e2e/function/function_override_builtin.sh`
- Create: `e2e/function/function_nested_definition.sh`
- Create: `e2e/function/function_redirect.sh`
- Create: `e2e/function/function_exit_vs_return.sh`
- Create: `e2e/function/function_local_positional.sh`
- Create: `e2e/function/function_prefix_assignment.sh`

- [ ] **Step 1: Create function_override_builtin.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function overrides a regular builtin
# EXPECT_OUTPUT: custom echo
echo() { printf 'custom echo\n'; }
echo anything
unset -f echo
```

- [ ] **Step 2: Create function_nested_definition.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function defined inside another function
# EXPECT_OUTPUT<<END
# outer
# inner
# END
outer() {
  echo outer
  inner() { echo inner; }
  inner
}
outer
```

- [ ] **Step 3: Create function_redirect.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Redirect applied to function definition
# EXPECT_EXIT: 0
myfunc() {
  echo "to file"
}
myfunc > "$TEST_TMPDIR/output.txt"
result=$(cat "$TEST_TMPDIR/output.txt")
test "$result" = "to file"
```

- [ ] **Step 4: Create function_exit_vs_return.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: exit inside function terminates the entire shell
# EXPECT_EXIT: 42
myfunc() {
  exit 42
}
myfunc
echo "should not reach here"
```

- [ ] **Step 5: Create function_local_positional.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Caller positional parameters restored after function call
# EXPECT_OUTPUT<<END
# inner-a inner-b
# x y z
# END
myfunc() {
  echo "$1 $2"
}
set -- x y z
myfunc inner-a inner-b
echo "$1 $2 $3"
```

- [ ] **Step 6: Create function_prefix_assignment.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: VAR=val func — prefix assignment scoped to function
# XFAIL: Phase 5 limitation — function-scoped prefix assignments not implemented
# EXPECT_OUTPUT<<END
# in-func
# original
# END
MY_VAR=original
show_var() { echo "$MY_VAR"; }
MY_VAR=in-func show_var
echo "$MY_VAR"
```

- [ ] **Step 7: Run tests for function**

Run: `e2e/run_tests.sh --filter=function`
Expected: All PASS except `function_prefix_assignment.sh` which should be XFAIL

- [ ] **Step 8: Commit**

```bash
git add e2e/function/
git commit -m "test(e2e): add function edge case tests (6 tests, 1 XFAIL)"
```

---

### Task 9: pipeline_and_list edge cases (6 tests)

**Files:**
- Create: `e2e/pipeline_and_list/negation_with_and_or.sh`
- Create: `e2e/pipeline_and_list/pipeline_subshell.sh`
- Create: `e2e/pipeline_and_list/or_list_short_circuit.sh`
- Create: `e2e/pipeline_and_list/and_list_short_circuit.sh`
- Create: `e2e/pipeline_and_list/mixed_and_or_list.sh`
- Create: `e2e/pipeline_and_list/pipeline_exit_last.sh`

- [ ] **Step 1: Create negation_with_and_or.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! negation with && and || — precedence
# EXPECT_OUTPUT<<END
# yes
# yes
# END
! false && echo yes
! true || echo yes
```

- [ ] **Step 2: Create pipeline_subshell.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline commands run in subshells — variable changes do not propagate
# EXPECT_OUTPUT: before
x=before
echo test | x=after
echo "$x"
```

- [ ] **Step 3: Create or_list_short_circuit.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: OR list short-circuits on success
# EXPECT_OUTPUT: first
true || echo "should not print"
echo first
```

- [ ] **Step 4: Create and_list_short_circuit.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: AND list short-circuits on failure
# EXPECT_OUTPUT: done
false && echo "should not print"
echo done
```

- [ ] **Step 5: Create mixed_and_or_list.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Mixed && and || evaluate left to right
# EXPECT_OUTPUT<<END
# recovered
# final
# END
false && echo no || echo recovered
true && echo final || echo no
```

- [ ] **Step 6: Create pipeline_exit_last.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline exit status is from the last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
false | true
echo "$?"
true | false
echo "$?"
```

- [ ] **Step 7: Run tests for pipeline_and_list**

Run: `e2e/run_tests.sh --filter=pipeline_and_list`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add e2e/pipeline_and_list/
git commit -m "test(e2e): add pipeline_and_list edge case tests (6 tests)"
```

---

### Task 10: builtin edge cases (6 tests)

**Files:**
- Create: `e2e/builtin/cd_dash_oldpwd.sh`
- Create: `e2e/builtin/echo_no_args.sh`
- Create: `e2e/builtin/export_format.sh`
- Create: `e2e/builtin/set_dash_dash.sh`
- Create: `e2e/builtin/unset_readonly_error.sh`
- Create: `e2e/builtin/colon_always_success.sh`

- [ ] **Step 1: Create cd_dash_oldpwd.sh**

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - changes to OLDPWD
# XFAIL: Phase 2 limitation — cd - not implemented
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/dir1" "$TEST_TMPDIR/dir2"
cd "$TEST_TMPDIR/dir1"
cd "$TEST_TMPDIR/dir2"
cd -
pwd_result=$(pwd)
case "$pwd_result" in
  *dir1) exit 0 ;;
  *) exit 1 ;;
esac
```

- [ ] **Step 2: Create echo_no_args.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: echo with no arguments outputs only a newline
# EXPECT_OUTPUT:
echo
```

- [ ] **Step 3: Create export_format.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: export -p output is suitable for re-input
# EXPECT_EXIT: 0
export MY_TEST_EXPORT_VAR=hello
output=$(export -p)
case "$output" in
  *"export MY_TEST_EXPORT_VAR"*) exit 0 ;;
  *) exit 1 ;;
esac
```

- [ ] **Step 4: Create set_dash_dash.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: set -- replaces positional parameters
# EXPECT_OUTPUT<<END
# 3
# a
# b
# c
# END
set -- a b c
echo "$#"
echo "$1"
echo "$2"
echo "$3"
```

- [ ] **Step 5: Create unset_readonly_error.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: Unsetting a readonly variable produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly MY_RO_VAR=test
unset MY_RO_VAR
```

- [ ] **Step 6: Create colon_always_success.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: : (colon) always returns exit code 0
# EXPECT_OUTPUT: 0
:
echo "$?"
```

- [ ] **Step 7: Run tests for builtin**

Run: `e2e/run_tests.sh --filter=builtin`
Expected: All PASS except `cd_dash_oldpwd.sh` which should be XFAIL

- [ ] **Step 8: Commit**

```bash
git add e2e/builtin/
git commit -m "test(e2e): add builtin edge case tests (6 tests, 1 XFAIL)"
```

---

### Task 11: signal_and_trap edge cases (4 tests)

**Files:**
- Create: `e2e/signal_and_trap/trap_reset_dash.sh`
- Create: `e2e/signal_and_trap/trap_ignore_empty.sh`
- Create: `e2e/signal_and_trap/trap_exit_in_function.sh`
- Create: `e2e/signal_and_trap/trap_subshell_reset.sh`

- [ ] **Step 1: Create trap_reset_dash.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap - SIGNAL resets handler to default
# EXPECT_OUTPUT: hello
trap 'echo trapped' EXIT
trap - EXIT
echo hello
```

- [ ] **Step 2: Create trap_ignore_empty.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap '' SIGNAL ignores the signal
# EXPECT_EXIT: 0
trap '' USR1
kill -USR1 $$
echo "survived"
```

- [ ] **Step 3: Create trap_exit_in_function.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: EXIT trap set in function fires at shell exit
# EXPECT_OUTPUT<<END
# hello
# goodbye
# END
setup() {
  trap 'echo goodbye' EXIT
}
setup
echo hello
```

- [ ] **Step 4: Create trap_subshell_reset.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Non-ignored traps are reset in subshells
# EXPECT_EXIT: 0
trap 'echo main_trap' USR1
(
  output=$(trap)
  case "$output" in
    *USR1*) exit 1 ;;
    *) exit 0 ;;
  esac
)
```

- [ ] **Step 5: Run tests for signal_and_trap**

Run: `e2e/run_tests.sh --filter=signal_and_trap`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add e2e/signal_and_trap/
git commit -m "test(e2e): add signal_and_trap edge case tests (4 tests)"
```

---

### Task 12: subshell edge cases (4 tests)

**Files:**
- Create: `e2e/subshell/subshell_exit_no_parent.sh`
- Create: `e2e/subshell/subshell_cd_no_parent.sh`
- Create: `e2e/subshell/subshell_trap_inherit_ignore.sh`
- Create: `e2e/subshell/subshell_nested_exit_code.sh`

- [ ] **Step 1: Create subshell_exit_no_parent.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Exit in subshell does not terminate parent
# EXPECT_OUTPUT<<END
# sub-exiting
# parent-alive
# END
(echo sub-exiting; exit 1)
echo parent-alive
```

- [ ] **Step 2: Create subshell_cd_no_parent.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: cd in subshell does not affect parent cwd
# EXPECT_EXIT: 0
original=$(pwd)
(cd /tmp)
current=$(pwd)
test "$original" = "$current"
```

- [ ] **Step 3: Create subshell_trap_inherit_ignore.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Ignored signals are inherited by subshells
# EXPECT_OUTPUT: survived
trap '' USR1
(
  kill -USR1 $$
  echo survived
)
```

- [ ] **Step 4: Create subshell_nested_exit_code.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Subshell exit code is reflected in $?
# EXPECT_OUTPUT<<END
# 0
# 42
# END
(exit 0)
echo "$?"
(exit 42)
echo "$?"
```

- [ ] **Step 5: Run tests for subshell**

Run: `e2e/run_tests.sh --filter=subshell`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add e2e/subshell/
git commit -m "test(e2e): add subshell edge case tests (4 tests)"
```

---

### Task 13: command_execution edge cases (4 tests)

**Files:**
- Create: `e2e/command_execution/command_not_found.sh`
- Create: `e2e/command_execution/permission_denied.sh`
- Create: `e2e/command_execution/empty_var_command.sh`
- Create: `e2e/command_execution/prefix_assignment_scope.sh`

- [ ] **Step 1: Create command_not_found.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Command not found exits with 127
# EXPECT_EXIT: 127
nonexistent_command_xyz_12345
```

- [ ] **Step 2: Create permission_denied.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Non-executable file exits with 126
# EXPECT_EXIT: 126
echo "not executable" > "$TEST_TMPDIR/noperm.sh"
chmod -x "$TEST_TMPDIR/noperm.sh"
"$TEST_TMPDIR/noperm.sh"
```

- [ ] **Step 3: Create empty_var_command.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Empty variable as command — only assignments and redirects execute
# EXPECT_EXIT: 0
empty=
MY_EDGE_VAR=set $empty
test "$MY_EDGE_VAR" = "set"
```

- [ ] **Step 4: Create prefix_assignment_scope.sh**

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Prefix assignment is scoped to the command environment only
# EXPECT_OUTPUT:
MY_SCOPED_VAR=hello /usr/bin/true
echo "$MY_SCOPED_VAR"
```

- [ ] **Step 5: Run tests for command_execution**

Run: `e2e/run_tests.sh --filter=command_execution`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add e2e/command_execution/
git commit -m "test(e2e): add command_execution edge case tests (4 tests)"
```

---

### Task 14: Full test suite verification

- [ ] **Step 1: Run entire test suite**

Run: `e2e/run_tests.sh`
Expected: 228 total tests. 223 PASS, 5 XFAIL, 0 FAIL, 0 TIMEOUT.

- [ ] **Step 2: Verify test count**

Confirm the output summary shows exactly 228 tests total.

- [ ] **Step 3: Verify XFAIL tests**

Confirm exactly 5 XFAIL results:
- `variable_and_expansion/at_vs_star_unquoted.sh`
- `redirection/heredoc_pipeline.sh`
- `arithmetic/positional_in_arith.sh`
- `function/function_prefix_assignment.sh`
- `builtin/cd_dash_oldpwd.sh`
