#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Quoted glob metacharacters are not expanded
# EXPECT_OUTPUT: *.txt
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a.txt
echo "*.txt"
