#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times returns exit status 0 on success
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
times >/dev/null
echo $?
