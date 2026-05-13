#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: a failed assignment to a read-only does not change the value
# EXPECT_OUTPUT: locked
# EXPECT_EXIT: 0
readonly foo=locked
foo=tried 2>/dev/null
echo "$foo"
