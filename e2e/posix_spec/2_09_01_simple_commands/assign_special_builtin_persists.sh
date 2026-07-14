#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Variable assignments preceding a special built-in persist in the current environment
# EXPECT_OUTPUT: v=1
# EXPECT_EXIT: 0
v=1 :
echo "v=$v"
