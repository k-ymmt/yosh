#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd -P prints the physical (resolved) path
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/real"
ln -s real "$TEST_TMPDIR/sym"
cd "$TEST_TMPDIR/sym"
out=$(pwd -P)
case "$out" in
    *real) exit 0 ;;
    *) exit 1 ;;
esac
