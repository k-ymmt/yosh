#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: shift with no operand shifts by 1
# EXPECT_OUTPUT: b c
# EXPECT_EXIT: 0
set -- a b c
shift
echo "$1 $2"
