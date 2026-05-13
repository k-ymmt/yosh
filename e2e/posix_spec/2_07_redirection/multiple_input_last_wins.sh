#!/bin/sh
# POSIX_REF: 2.7.1 Redirecting Input
# DESCRIPTION: Multiple input redirections — the last one is effective
# EXPECT_OUTPUT: B
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
printf 'A\n' > a
printf 'B\n' > b
cat <a <b
