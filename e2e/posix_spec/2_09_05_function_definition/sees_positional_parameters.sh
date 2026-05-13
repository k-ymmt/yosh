#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function call passes arguments as positional parameters $1..
# EXPECT_OUTPUT: arg
# EXPECT_EXIT: 0
f() { echo "$1"; }
f arg
