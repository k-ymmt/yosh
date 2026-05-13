#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Files starting with . are not matched by unquoted * by default
# EXPECT_OUTPUT: visible
# EXPECT_EXIT: 0
# XFAIL: yosh does not yet exclude leading-dot files from * glob expansion
cd "$TEST_TMPDIR"
: > .hidden
: > visible
echo *
