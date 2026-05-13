#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias defines a command alias
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
alias greet='echo hello'
greet
