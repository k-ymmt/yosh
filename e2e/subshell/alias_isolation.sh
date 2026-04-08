#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Aliases defined in subshell do not exist in parent
# EXPECT_OUTPUT: original
alias greet='echo original'
(alias greet='echo modified')
greet
