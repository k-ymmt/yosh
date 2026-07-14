#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: The exit status of a function definition itself is 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
false
f() { :; }
echo $?
