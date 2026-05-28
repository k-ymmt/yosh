#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 undergoes expansion ($LINENO) before trace display
# EXPECT_STDERR: + 9 echo hi
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
PS4='+ $LINENO '
set -x
echo hi
# NOTE: EXPECT_STDERR's "9" is the line of `echo hi`; update it if header lines change.
