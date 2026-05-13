#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: 2> form of output redirection sends stderr to a file
# EXPECT_OUTPUT: err
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
sh -c 'echo err >&2' 2>e
cat e
