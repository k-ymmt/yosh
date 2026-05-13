#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Unexported variables are not visible in child processes
# EXPECT_OUTPUT: unset
# EXPECT_EXIT: 0
x=value
sh -c 'echo "${x:-unset}"'
