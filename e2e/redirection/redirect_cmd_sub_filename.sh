#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Command substitution in redirect filename
# EXPECT_EXIT: 0
fname="outfile.txt"
echo hello > "$TEST_TMPDIR/$(echo "$fname")"
result=$(cat "$TEST_TMPDIR/outfile.txt")
test "$result" = "hello"
