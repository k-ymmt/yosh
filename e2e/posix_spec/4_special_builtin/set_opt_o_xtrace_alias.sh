#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -o xtrace is equivalent to set -x
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: echo 0
set -o xtrace
echo 0
