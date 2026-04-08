#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment without command sets variable and returns 0
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
x=hello
echo "$x"
