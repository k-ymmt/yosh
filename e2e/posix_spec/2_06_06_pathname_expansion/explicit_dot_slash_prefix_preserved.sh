#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: An explicit ./ prefix and // runs are kept in glob results (./* never yields a bare -x)
# EXPECT_OUTPUT<<END
# [./-x][./a]
# [./.][./..][./.h]
# [.//-x][.//a]
# [./a]
# [-x][a]
# END
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/dotslash"
cd "$TEST_TMPDIR/dotslash" || exit 1
: > ./-x
: > a
: > .h
printf '[%s]' ./*; echo
printf '[%s]' ./.*; echo
printf '[%s]' .//*; echo
printf '[%s]' ./[a]; echo
printf '[%s]' *; echo
