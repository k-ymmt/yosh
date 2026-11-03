#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Temporary assignment to a readonly variable is a variable assignment error: diagnosed, command not run, non-interactive shell exits (dash agrees)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly r=1
r=2 true
echo continued
