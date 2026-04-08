#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: > redirects stdout to a file
# EXPECT_EXIT: 0
echo hello > "$TEST_TMPDIR/out.txt"
result=$(cat "$TEST_TMPDIR/out.txt")
test "$result" = "hello"
