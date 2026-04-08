#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Multiple redirections on one command
# EXPECT_EXIT: 0
echo stdout_msg > "$TEST_TMPDIR/out.txt" 2> "$TEST_TMPDIR/err.txt"
result=$(cat "$TEST_TMPDIR/out.txt")
test "$result" = "stdout_msg"
