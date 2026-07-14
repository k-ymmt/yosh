#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Expansion results are sorted in collation order
# EXPECT_OUTPUT: a.txt b.txt c.txt
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/d"
cd "$TEST_TMPDIR/d" || exit 1
: > b.txt
: > a.txt
: > c.txt
echo *.txt
