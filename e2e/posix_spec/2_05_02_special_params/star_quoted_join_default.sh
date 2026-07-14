#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - *
# DESCRIPTION: "$*" joins fields with a space when IFS is unset
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
unset IFS
set -- a b
echo "$*"
