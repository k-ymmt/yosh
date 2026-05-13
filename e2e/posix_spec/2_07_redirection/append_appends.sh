#!/bin/sh
# POSIX_REF: 2.7.3 Appending Redirected Output
# DESCRIPTION: >> appends to existing content
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo a >f
echo b >>f
cat f
