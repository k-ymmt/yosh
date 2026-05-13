#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $? is 1 after a failed command
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
false
echo $?
