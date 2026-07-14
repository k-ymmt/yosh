#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: A negated bracket expression still does not match a leading dot
# EXPECT_OUTPUT: afile
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/d3"
cd "$TEST_TMPDIR/d3" || exit 1
: > .hidden
: > afile
echo [!x]*
