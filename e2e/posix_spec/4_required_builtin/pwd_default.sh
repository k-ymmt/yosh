#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd prints the current working directory
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
cd "$TEST_TMPDIR"
out=$(pwd)
case "$out" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
