#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: return sets the function's exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
f() { return 7; }
f
echo $?
