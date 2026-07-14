#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: * never matches a slash in pathname expansion
# EXPECT_OUTPUT: abc
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/g/a"
: > "$TEST_TMPDIR/g/a/c"
: > "$TEST_TMPDIR/g/abc"
cd "$TEST_TMPDIR/g" || exit 1
echo a*c
