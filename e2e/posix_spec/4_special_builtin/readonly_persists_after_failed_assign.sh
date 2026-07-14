#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: a failed assignment to a read-only does not change the value
# EXPECT_OUTPUT: locked
# EXPECT_EXIT: 0
# The failed assignment runs in a subshell: per POSIX 2.8.1 a variable
# assignment error makes a non-interactive shell exit, so the parent shell
# performs the assignment attempt in a child and then checks the value.
readonly foo=locked
(foo=tried) 2>/dev/null
echo "$foo"
