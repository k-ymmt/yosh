#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: $# decreases by n after shift
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
set -- a b c
shift 2
echo $#
