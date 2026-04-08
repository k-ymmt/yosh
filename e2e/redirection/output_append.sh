#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: >> appends stdout to a file
# EXPECT_OUTPUT<<END
# first
# second
# END
echo first > "$TEST_TMPDIR/out.txt"
echo second >> "$TEST_TMPDIR/out.txt"
cat "$TEST_TMPDIR/out.txt"
