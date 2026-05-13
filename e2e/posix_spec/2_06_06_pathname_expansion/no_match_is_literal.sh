#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Pattern with no match expands to the literal pattern
# EXPECT_OUTPUT: *.nomatch
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo *.nomatch
