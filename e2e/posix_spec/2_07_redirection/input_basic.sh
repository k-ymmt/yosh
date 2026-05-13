#!/bin/sh
# POSIX_REF: 2.7.1 Redirecting Input
# DESCRIPTION: < redirects stdin from a file
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
printf 'hi\n' > f
cat <f
