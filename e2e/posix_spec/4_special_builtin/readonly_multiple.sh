#!/bin/sh
# POSIX_REF: 2.15 readonly
# DESCRIPTION: readonly with multiple NAME=value pairs sets all
# EXPECT_OUTPUT: 1-2
# EXPECT_EXIT: 0
readonly a=1 b=2
echo "$a-$b"
