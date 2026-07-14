#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set +o output is suitable for re-input to restore option settings
# EXPECT_OUTPUT: f*
# EXPECT_EXIT: 0
set -f
saved=$(set +o)
set +f
eval "$saved"
cd "$TEST_TMPDIR" || exit 1
: > file1.txt
echo f*
