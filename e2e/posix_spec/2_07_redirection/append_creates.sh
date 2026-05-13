#!/bin/sh
# POSIX_REF: 2.7.3 Appending Redirected Output
# DESCRIPTION: >> creates the file if it does not exist
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo hi >>f
cat f
