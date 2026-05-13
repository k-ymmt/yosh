#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd returns 0 on success
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
pwd >/dev/null
echo $?
