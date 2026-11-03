#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: .*/ lists ./ and ../ (the . entry is not lost) and only directories
# EXPECT_OUTPUT<<END
# [../][./][.d/]
# [d/]
# [d/.][d/..][d/.e]
# END
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/dsd"
cd "$TEST_TMPDIR/dsd" || exit 1
mkdir .d d
: > .f
: > f
: > d/.e
printf '[%s]' .*/; echo
printf '[%s]' */; echo
printf '[%s]' d/.*; echo
