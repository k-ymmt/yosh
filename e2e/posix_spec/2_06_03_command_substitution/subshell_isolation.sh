#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution runs in a subshell; assignments do not leak to the parent
# EXPECT_OUTPUT: out
# EXPECT_EXIT: 0
v=out
junk=$(v=in)
echo "$v"
