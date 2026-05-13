#!/bin/sh
# POSIX_REF: 8 Environment Variables - OLDPWD
# DESCRIPTION: cd - returns to the directory in $OLDPWD
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
mkdir -p "$TEST_TMPDIR/a" "$TEST_TMPDIR/b"
cd "$TEST_TMPDIR/a"
cd "$TEST_TMPDIR/b"
cd - >/dev/null
case "$PWD" in "$TEST_TMPDIR/a") exit 0 ;; *) exit 1 ;; esac
