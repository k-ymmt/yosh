#!/bin/sh
# POSIX_REF: 8 Environment Variables - CDPATH
# DESCRIPTION: an empty CDPATH entry means the current directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/local"
cd "$TEST_TMPDIR"
CDPATH=":/no/such/dir"
cd local
case "$PWD" in */local) exit 0 ;; *) exit 1 ;; esac
