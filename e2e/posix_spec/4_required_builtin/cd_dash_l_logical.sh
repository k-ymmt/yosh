#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -L uses logical handling of dot-dot (default)
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/a/b"
cd "$TEST_TMPDIR/a/b"
cd -L ..
case "$PWD" in
    */a) exit 0 ;;
    *) exit 1 ;;
esac
