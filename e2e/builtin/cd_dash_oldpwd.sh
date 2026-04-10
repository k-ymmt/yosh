#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - changes to OLDPWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/dir1" "$TEST_TMPDIR/dir2"
cd "$TEST_TMPDIR/dir1"
cd "$TEST_TMPDIR/dir2"
cd -
pwd_result=$(pwd)
case "$pwd_result" in
  *dir1) exit 0 ;;
  *) exit 1 ;;
esac
