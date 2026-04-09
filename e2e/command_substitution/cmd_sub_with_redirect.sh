#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Redirection inside command substitution
# EXPECT_OUTPUT: file content
echo "file content" > "$TEST_TMPDIR/input.txt"
x=$(cat < "$TEST_TMPDIR/input.txt")
echo "$x"
