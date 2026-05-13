#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd -L prints the logical (symlink-preserving) path
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/real"
ln -s real "$TEST_TMPDIR/sym"
cd "$TEST_TMPDIR/sym"
out=$(pwd -L)
case "$out" in
    *sym) exit 0 ;;
    *) exit 1 ;;
esac
