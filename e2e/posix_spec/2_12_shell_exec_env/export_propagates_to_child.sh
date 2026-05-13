#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Exported variables are visible in child processes
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
export x=value
sh -c 'echo "$x"'
