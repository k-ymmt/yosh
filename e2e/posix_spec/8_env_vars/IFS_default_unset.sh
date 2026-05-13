#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: when IFS is unset, default IFS (space tab newline) is used for field splitting
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
unset IFS
v='a b c'
set -- $v
echo "$1 $2 $3"
