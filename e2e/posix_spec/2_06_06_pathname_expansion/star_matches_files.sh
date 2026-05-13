#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: * matches any string in filenames (excluding leading .)
# EXPECT_OUTPUT: a.txt b.txt
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a.txt
: > b.txt
echo *.txt
