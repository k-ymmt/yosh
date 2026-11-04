#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: A quoted ! or - in a bracket expression matches literally in pathname expansion
# EXPECT_OUTPUT<<END
# ! b
# !
# - c
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > b
: > '!'
: > -
: > c
echo [\!b]
echo ["!"]
echo [a"-"c]
