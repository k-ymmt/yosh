#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: ? matches exactly one character in filenames
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a
: > b
: > ab
echo ?
