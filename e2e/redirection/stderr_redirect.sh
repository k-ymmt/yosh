#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: 2> redirects stderr to a file
# EXPECT_EXIT: 0
{ echo error_msg >&2; } 2>"$TEST_TMPDIR/err.txt"
result=$(cat "$TEST_TMPDIR/err.txt")
test "$result" = "error_msg"
