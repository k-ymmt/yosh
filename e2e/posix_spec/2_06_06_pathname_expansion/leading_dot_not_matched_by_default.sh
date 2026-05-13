#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Files starting with . are not matched by unquoted * by default
# EXPECT_OUTPUT: visible
# EXPECT_EXIT: 0
mkdir "$TEST_TMPDIR/sub"
cd "$TEST_TMPDIR/sub"
: > .hidden
: > visible
echo *
