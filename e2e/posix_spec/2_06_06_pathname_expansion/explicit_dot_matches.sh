#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: An explicit leading . in the pattern matches dot-files
# EXPECT_OUTPUT: .foo
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/d2"
cd "$TEST_TMPDIR/d2" || exit 1
: > .foo
echo .f*
