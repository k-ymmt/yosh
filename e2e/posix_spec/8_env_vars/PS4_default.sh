#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: default PS4 is '+ '
# EXPECT_STDERR: + echo 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
unset PS4
set -x
echo 0
