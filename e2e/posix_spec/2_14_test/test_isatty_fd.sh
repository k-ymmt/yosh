#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -t is false for a non-terminal stdin
# EXPECT_EXIT: 1
[ -t 0 ] < /dev/null
