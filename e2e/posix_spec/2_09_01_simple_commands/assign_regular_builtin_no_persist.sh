#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Variable assignments preceding a regular built-in do not persist
# EXPECT_OUTPUT: v=[]
# EXPECT_EXIT: 0
v=1 true
echo "v=[$v]"
