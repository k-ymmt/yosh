#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly -- treats following operands as names
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
readonly -- foo=ok
echo "$foo"
