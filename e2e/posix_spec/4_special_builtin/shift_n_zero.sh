#!/bin/sh
# POSIX_REF: 2.15 shift
# DESCRIPTION: shift 0 is a no-op
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
set -- a b c
shift 0
echo "$1 $2 $3"
