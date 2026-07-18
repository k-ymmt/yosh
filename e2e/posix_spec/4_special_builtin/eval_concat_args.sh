#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval concatenates its operands with spaces and re-parses
# EXPECT_OUTPUT: hello world
# EXPECT_EXIT: 0
eval echo "hello" "world"
