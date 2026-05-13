#!/bin/sh
# POSIX_REF: 2.14.15 times
# DESCRIPTION: times returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
times >/dev/null
echo $?
