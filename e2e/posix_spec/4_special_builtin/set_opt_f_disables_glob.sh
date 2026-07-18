#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -f disables pathname expansion
# EXPECT_OUTPUT: *
# EXPECT_EXIT: 0
set -f
echo *
