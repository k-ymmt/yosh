#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly NAME=value assigns and marks read-only atomically
# EXPECT_OUTPUT: locked
# EXPECT_EXIT: 0
readonly foo=locked
echo "$foo"
