#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd .. moves up one directory
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
mkdir -p "$TEST_TMPDIR/sub"
cd "$TEST_TMPDIR/sub"
cd ..
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
