#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd changes the working directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/subdir"
cd "$TEST_TMPDIR/subdir"
# Verify we can create a file in the new directory
echo ok > testfile
test -f testfile || exit 1
