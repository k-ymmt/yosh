#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: >| overrides noclobber to force overwrite
# EXPECT_OUTPUT: b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo a >f
set -C
echo b >|f
cat f
