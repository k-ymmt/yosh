#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x writes a trace of each command to stderr
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: echo 0
set -x
echo 0
