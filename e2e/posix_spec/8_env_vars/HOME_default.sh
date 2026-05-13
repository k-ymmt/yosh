#!/bin/sh
# POSIX_REF: 8 Environment Variables - HOME
# DESCRIPTION: cd with no args uses $HOME
# EXPECT_EXIT: 0
TEST_TMPDIR=$(cd "$TEST_TMPDIR" && pwd)
HOME="$TEST_TMPDIR"
cd
case "$PWD" in "$TEST_TMPDIR") exit 0 ;; *) exit 1 ;; esac
