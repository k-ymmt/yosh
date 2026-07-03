#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Invoking a directory as a command exits 126 (found but not executable)
# EXPECT_EXIT: 126
# EXPECT_STDERR: permission denied
mkdir -p "$TEST_TMPDIR/somedir"
"$TEST_TMPDIR/somedir"
