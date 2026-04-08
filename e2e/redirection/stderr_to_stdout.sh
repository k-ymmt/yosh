#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: 2>&1 redirects stderr to stdout
# EXPECT_EXIT: 0
echo error_msg > "$TEST_TMPDIR/combined.txt" 2>&1
result=$(cat "$TEST_TMPDIR/combined.txt")
test "$result" = "error_msg"
