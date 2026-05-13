#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 controls the trace prefix when set -x is in effect
# XFAIL: not yet implemented (TODO: set -x PS4 prefix not honoured; hardcoded to '+ ')
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: TRACE>
# EXPECT_EXIT: 0
PS4='TRACE> '
set -x
echo 0
