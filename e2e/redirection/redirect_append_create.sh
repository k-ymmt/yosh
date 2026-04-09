#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: >> creates the file if it does not exist
# EXPECT_EXIT: 0
echo hello >> "$TEST_TMPDIR/newfile.txt"
result=$(cat "$TEST_TMPDIR/newfile.txt")
test "$result" = "hello"
