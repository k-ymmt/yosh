#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd updates $OLDPWD to the previous directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/x"
prev="$PWD"
cd "$TEST_TMPDIR/x"
case "$OLDPWD" in
    "$prev") exit 0 ;;
    *) exit 1 ;;
esac
