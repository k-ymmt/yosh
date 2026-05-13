#!/bin/sh
# POSIX_REF: 8 Environment Variables - PWD
# DESCRIPTION: cd updates PWD to the new directory
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
cd "$TEST_TMPDIR"
case "$PWD" in "$TEST_TMPDIR") exit 0 ;; *) exit 1 ;; esac
