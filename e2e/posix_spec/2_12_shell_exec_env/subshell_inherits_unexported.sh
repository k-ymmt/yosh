#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: A subshell (parenthesized list) inherits the parent's variables
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
(echo "$x")
