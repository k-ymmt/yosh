#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: A wildcard in a non-final path component expands per component
# EXPECT_OUTPUT: sub/data.txt
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/d4/sub"
: > "$TEST_TMPDIR/d4/sub/data.txt"
cd "$TEST_TMPDIR/d4" || exit 1
echo */data.txt
