#!/bin/sh
# POSIX_REF: 2.15 return
# DESCRIPTION: return with no operand returns the status of the last command run inside the function
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
f() { false; return; }
f
echo $?
