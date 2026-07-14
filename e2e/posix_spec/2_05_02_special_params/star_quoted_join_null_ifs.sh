#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - *
# DESCRIPTION: "$*" joins fields without separators when IFS is null
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
# XFAIL: yosh joins "$*" with a space even when IFS is null
IFS=''
set -- a b
echo "$*"
