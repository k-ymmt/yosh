#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: > truncates the target file before writing
# EXPECT_OUTPUT: b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo a >f
echo b >f
cat f
