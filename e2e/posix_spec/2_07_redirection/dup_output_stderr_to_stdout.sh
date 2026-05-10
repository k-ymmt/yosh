#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: 2>&1 redirects stderr to stdout (canonical 2>&1 form)
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
out="$TEST_TMPDIR/combined.txt"
{ echo from_stderr >&2; } > "$out" 2>&1
result=$(cat "$out")
test "$result" = "from_stderr"
