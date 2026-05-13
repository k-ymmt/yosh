#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: [abc] matches any one of the listed characters
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a
: > b
: > c
echo [ab]
