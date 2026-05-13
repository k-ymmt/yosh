#!/bin/sh
# POSIX_REF: 8 Environment Variables - CDPATH
# DESCRIPTION: CDPATH is consulted by cd for relative paths
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/base/sub"
CDPATH="$TEST_TMPDIR/base"
cd sub
case "$PWD" in */base/sub) exit 0 ;; *) exit 1 ;; esac
