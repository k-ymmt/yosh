#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd updates $PWD on success
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
cd "$TEST_TMPDIR"
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
