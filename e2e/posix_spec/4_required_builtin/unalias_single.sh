#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias NAME removes the named alias
# EXPECT_OUTPUT: ran
# EXPECT_EXIT: 0
alias greet='echo aliased'
unalias greet
greet() { echo ran; }
greet
