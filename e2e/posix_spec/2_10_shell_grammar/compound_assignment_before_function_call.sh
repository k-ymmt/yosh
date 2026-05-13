#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Assignment Prefix
# DESCRIPTION: Assignment prefix on a function call sets the variable for that call's environment
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
f() { echo "$x"; }
x=1 f
