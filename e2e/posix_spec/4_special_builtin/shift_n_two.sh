#!/bin/sh
# POSIX_REF: 2.15 shift
# DESCRIPTION: shift 2 shifts two positional parameters
# EXPECT_OUTPUT: c
# EXPECT_EXIT: 0
set -- a b c
shift 2
echo "$1"
