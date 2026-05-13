#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd with no args changes to $HOME
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
HOME="$TEST_TMPDIR"
cd
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
