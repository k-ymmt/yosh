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
