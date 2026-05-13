#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Variable changes in a subshell do not affect the parent
# EXPECT_OUTPUT: original
# EXPECT_EXIT: 0
x=original
(x=changed)
echo "$x"
