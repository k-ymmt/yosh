#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 controls the trace prefix when set -x is in effect
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: TRACE>
# EXPECT_EXIT: 0
PS4='TRACE> '
set -x
echo 0
