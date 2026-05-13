#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -P uses physical handling, resolving symlinks
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
mkdir -p "$TEST_TMPDIR/real"
ln -s "$TEST_TMPDIR/real" "$TEST_TMPDIR/sym"
cd "$TEST_TMPDIR/sym"
cd -P .
# After cd -P . we should be in the resolved (physical) path of real, not sym
case "$PWD" in
    *real) exit 0 ;;
    *sym)  exit 1 ;;
    *)     exit 1 ;;
esac
