#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Modulo by zero produces error
# XFAIL: prints error to stderr but returns exit code 0 instead of 1
# EXPECT_EXIT: 1
# EXPECT_STDERR: division by zero
echo $((1 % 0))
