#!/bin/sh
# POSIX_REF: 2.15 return
# DESCRIPTION: return inside a function ends the function with its given exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
f() { return 7; }
f
echo $?
