#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - changes to $OLDPWD and prints the new directory
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
mkdir -p "$TEST_TMPDIR/a" "$TEST_TMPDIR/b"
cd "$TEST_TMPDIR/a"
cd "$TEST_TMPDIR/b"
out=$(cd -)
case "$out" in
    *"$TEST_TMPDIR/a") exit 0 ;;
    *) exit 1 ;;
esac
