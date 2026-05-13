#!/bin/sh
# POSIX_REF: 8 Environment Variables - PWD
# DESCRIPTION: PWD is inherited by subshells
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
cd "$TEST_TMPDIR"
out=$( echo "$PWD" )
case "$out" in "$TEST_TMPDIR") exit 0 ;; *) exit 1 ;; esac
