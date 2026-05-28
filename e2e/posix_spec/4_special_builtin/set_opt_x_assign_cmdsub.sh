#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces assignment with command-sub value after expansion
# EXPECT_STDERR: + x=hi
# EXPECT_EXIT: 0
set -x
x=$(echo hi)
